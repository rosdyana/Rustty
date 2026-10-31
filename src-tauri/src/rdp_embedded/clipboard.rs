//! Portapapeles compartido de la sesión integrada (canal `CLIPRDR`): texto y
//! ficheros en los dos sentidos.
//!
//! IronRDP lleva el protocolo; aquí va lo que toca al sistema:
//!
//! - [`Proxy`] es el backend que IronRDP llama **dentro** de `ActiveStage::process`.
//!   No bloquea: convierte cada aviso en un [`ClipIn`] y lo manda al hilo del
//!   portapapeles.
//! - El hilo ([`Worker`]) es quien toca el portapapeles del sistema (`arboard`)
//!   y el disco. Responde con [`ClipOut`], que el bucle de la sesión aplica sobre
//!   el `CliprdrClient` y envía al servidor.
//!
//! **Local → remoto.** Al enfocar el escritorio (y antes de cada Ctrl+V) el
//! frontend pide sincronizar: si el portapapeles local cambió, se anuncia al
//! servidor (texto, o la lista de ficheros con sus carpetas). Los datos solo
//! viajan cuando el remoto pega: se leen del disco a trozos, a demanda.
//!
//! **Remoto → local.** Cuando el remoto copia, el texto se escribe directamente
//! en el portapapeles local. Los ficheros se descargan a una carpeta temporal de
//! la sesión y, al terminar, se ponen en el portapapeles local como ficheros
//! copiados: se pegan en el explorador como cualquier otro.
//!
//! Los nombres que llegan del servidor no son de fiar: IronRDP ya los sanea y
//! aquí, además, cada componente se valida antes de crear nada en disco, y todo
//! queda dentro de la carpeta temporal de esa descarga.

use std::collections::HashMap;
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, ClipboardFormat, ClipboardFormatId, ClipboardFormatName,
    ClipboardGeneralCapabilityFlags, FileContentsFlags, FileContentsRequest, FileContentsResponse,
    FileDescriptor, FormatDataRequest, FormatDataResponse, LockDataId, OwnedFormatDataResponse,
};
use ironrdp_cliprdr::{CliprdrClient, MAX_LOCKED_FILE_LISTS};
use serde::Serialize;
use tokio::sync::mpsc;

/// Trozo de cada petición de descarga. Windows sirve sin problema 1 MiB.
const DOWNLOAD_CHUNK: u32 = 1024 * 1024;
/// Máximo que se sirve en una respuesta de subida, pida lo que pida el servidor.
const UPLOAD_CHUNK_MAX: u32 = 4 * 1024 * 1024;
/// Tope de una descarga (suma de los ficheros copiados en el remoto).
const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Tope de entradas al anunciar carpetas locales (ficheros + subcarpetas).
const MAX_LOCAL_ENTRIES: usize = 20_000;
/// Profundidad máxima al recorrer carpetas locales.
const MAX_LOCAL_DEPTH: usize = 32;
/// Longitud máxima del nombre en el cable (`relative_path\name`), en caracteres.
const MAX_WIRE_NAME: usize = 259;
/// Cada cuánto, como mucho, se avisa del progreso de una descarga.
const PROGRESS_EVERY: Duration = Duration::from_millis(400);
/// Segundos entre 1601-01-01 (FILETIME) y 1970-01-01.
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

/// Qué se comparte por el portapapeles.
#[derive(Debug, Clone, Copy)]
pub struct ClipboardOptions {
    /// También ficheros (si no, solo texto).
    pub files: bool,
}

/// Avisos del hilo al hilo del bucle de la sesión.
#[derive(Debug)]
pub enum ClipOut {
    InitiateCopy(Vec<ClipboardFormat>),
    InitiateFileCopy(Vec<FileDescriptor>),
    InitiatePaste(ClipboardFormatId),
    FormatData(OwnedFormatDataResponse),
    FileContents(FileContentsResponse<'static>),
    RequestFileContents(FileContentsRequest),
}

/// Entradas del hilo del portapapeles.
#[derive(Debug)]
pub enum ClipIn {
    /// El canal está listo, o el servidor pide nuestra lista de formatos.
    Announce,
    /// El frontend pide comparar el portapapeles local con lo último anunciado.
    Sync,
    Capabilities(ClipboardGeneralCapabilityFlags),
    RemoteCopy(Vec<ClipboardFormat>),
    FormatDataRequest(ClipboardFormatId),
    FormatDataResponse {
        ok: bool,
        data: Vec<u8>,
    },
    RemoteFileList(Vec<FileDescriptor>, Option<u32>),
    FileContentsRequest(FileContentsRequest),
    FileContentsResponse {
        stream_id: u32,
        ok: bool,
        data: Vec<u8>,
    },
    Lock(u32),
    Unlock(u32),
    Shutdown,
}

/// Aviso al frontend (`rdp-embed-clip-{id}`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipNotice {
    /// `download-start` | `download-progress` | `download-done` |
    /// `download-error` | `upload-start` | `unavailable`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ClipNotice {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            files: None,
            bytes: None,
            total: None,
            reason: None,
        }
    }
}

pub trait ClipNotifier: Send + Sync {
    fn notice(&self, notice: ClipNotice);
}

// ─── Backend de IronRDP ──────────────────────────────────────────────────────

/// Backend `CLIPRDR`: solo reenvía al hilo del portapapeles.
#[derive(Debug)]
struct Proxy {
    tx: std_mpsc::Sender<ClipIn>,
    files: bool,
    temp_dir: String,
}

ironrdp_core::impl_as_any!(Proxy);

impl Proxy {
    fn send(&self, msg: ClipIn) {
        let _ = self.tx.send(msg);
    }
}

impl CliprdrBackend for Proxy {
    fn temporary_directory(&self) -> &str {
        &self.temp_dir
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        if self.files {
            ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
                | ClipboardGeneralCapabilityFlags::FILECLIP_NO_FILE_PATHS
                | ClipboardGeneralCapabilityFlags::CAN_LOCK_CLIPDATA
                | ClipboardGeneralCapabilityFlags::HUGE_FILE_SUPPORT_ENABLED
        } else {
            ClipboardGeneralCapabilityFlags::empty()
        }
    }

    fn on_ready(&mut self) {
        self.send(ClipIn::Announce);
    }

    fn on_request_format_list(&mut self) {
        self.send(ClipIn::Announce);
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        capabilities: ClipboardGeneralCapabilityFlags,
    ) {
        self.send(ClipIn::Capabilities(capabilities));
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        self.send(ClipIn::RemoteCopy(available_formats.to_vec()));
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.send(ClipIn::FormatDataRequest(request.format));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        self.send(ClipIn::FormatDataResponse {
            ok: !response.is_error(),
            data: response.data().to_vec(),
        });
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        self.send(ClipIn::FileContentsRequest(request));
    }

    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        self.send(ClipIn::FileContentsResponse {
            stream_id: response.stream_id(),
            ok: !response.is_error(),
            data: response.data().to_vec(),
        });
    }

    fn on_lock(&mut self, data_id: LockDataId) {
        self.send(ClipIn::Lock(data_id.0));
    }

    fn on_unlock(&mut self, data_id: LockDataId) {
        self.send(ClipIn::Unlock(data_id.0));
    }

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], clip_data_id: Option<u32>) {
        self.send(ClipIn::RemoteFileList(files.to_vec(), clip_data_id));
    }
}

/// Extremo de la sesión: el canal para IronRDP y las colas del hilo.
pub struct ClipboardChannel {
    /// Para `Sync` (y `Shutdown` al soltarlo).
    pub input: std_mpsc::Sender<ClipIn>,
    /// Lo que el bucle de la sesión aplica sobre el `CliprdrClient`.
    pub output: mpsc::UnboundedReceiver<ClipOut>,
}

impl Drop for ClipboardChannel {
    fn drop(&mut self) {
        let _ = self.input.send(ClipIn::Shutdown);
    }
}

/// Crea el canal `CLIPRDR` y arranca su hilo.
///
/// `temp_root` es la carpeta de las descargas de **esta** sesión.
pub fn start(
    options: ClipboardOptions,
    temp_root: PathBuf,
    notifier: Arc<dyn ClipNotifier>,
) -> (CliprdrClient, ClipboardChannel) {
    let (in_tx, in_rx) = std_mpsc::channel();
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let proxy = Proxy {
        tx: in_tx.clone(),
        files: options.files,
        temp_dir: std::env::temp_dir().to_string_lossy().into_owned(),
    };
    let worker = Worker::new(options, temp_root, out_tx, notifier);
    let spawned = std::thread::Builder::new()
        .name("rdp-clipboard".into())
        .spawn(move || worker.run(&in_rx));
    if let Err(e) = spawned {
        log::warn!("RDP integrado: no se pudo arrancar el hilo del portapapeles: {e}");
    }
    (
        CliprdrClient::new(Box::new(proxy)),
        ClipboardChannel {
            input: in_tx,
            output: out_rx,
        },
    )
}

/// Aplica una orden del hilo sobre el canal y devuelve los mensajes a enviar.
pub fn apply(
    cliprdr: &mut CliprdrClient,
    out: ClipOut,
) -> ironrdp_pdu::PduResult<ironrdp_cliprdr::CliprdrSvcMessages<ironrdp_cliprdr::Client>> {
    match out {
        ClipOut::InitiateCopy(formats) => cliprdr.initiate_copy(&formats),
        ClipOut::InitiateFileCopy(files) => cliprdr.initiate_file_copy(files),
        ClipOut::InitiatePaste(format) => cliprdr.initiate_paste(format),
        ClipOut::FormatData(response) => cliprdr.submit_format_data(response),
        ClipOut::FileContents(response) => cliprdr.submit_file_contents(response),
        ClipOut::RequestFileContents(request) => cliprdr.request_file_contents(request),
    }
}

// ─── Hilo del portapapeles ───────────────────────────────────────────────────

/// Huella de lo último anunciado o escrito, para no anunciar dos veces lo mismo
/// (y no devolver al remoto lo que acaba de copiar él).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fingerprint {
    Text(u64),
    Files(Vec<PathBuf>),
}

fn text_fingerprint(text: &str) -> Fingerprint {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    Fingerprint::Text(h.finish())
}

/// Una entrada de la lista local anunciada, alineada por índice con la lista
/// de `FileDescriptor` enviada.
#[derive(Debug)]
struct LocalEntry {
    path: PathBuf,
    dir: bool,
}

/// Lo que el remoto espera del texto pedido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingPaste {
    None,
    Text,
}

struct DlItem {
    index: i32,
    path: PathBuf,
    size: Option<u64>,
    dir: bool,
}

struct Download {
    root: PathBuf,
    items: Vec<DlItem>,
    cur: usize,
    file: Option<File>,
    pos: u64,
    /// `stream_id` de la petición en vuelo.
    waiting: Option<u32>,
    lock: Option<u32>,
    total: u64,
    done: u64,
    top_level: Vec<PathBuf>,
    last_notice: Instant,
}

struct Worker {
    options: ClipboardOptions,
    temp_root: PathBuf,
    out: mpsc::UnboundedSender<ClipOut>,
    notifier: Arc<dyn ClipNotifier>,
    clipboard: Option<arboard::Clipboard>,
    files_negotiated: bool,
    last: Option<Fingerprint>,
    local_text: Option<String>,
    local_files: Arc<Vec<LocalEntry>>,
    locked: HashMap<u32, Arc<Vec<LocalEntry>>>,
    /// Fichero abierto para servir la subida en curso (índice, lista, fichero).
    reader: Option<(usize, usize, File)>,
    pending: PendingPaste,
    download: Option<Download>,
    batch: u32,
    next_stream: u32,
}

impl Worker {
    fn new(
        options: ClipboardOptions,
        temp_root: PathBuf,
        out: mpsc::UnboundedSender<ClipOut>,
        notifier: Arc<dyn ClipNotifier>,
    ) -> Self {
        Self {
            options,
            temp_root,
            out,
            notifier,
            clipboard: None,
            files_negotiated: false,
            last: None,
            local_text: None,
            local_files: Arc::new(Vec::new()),
            locked: HashMap::new(),
            reader: None,
            pending: PendingPaste::None,
            download: None,
            batch: 0,
            next_stream: 1,
        }
    }

    fn run(mut self, rx: &std_mpsc::Receiver<ClipIn>) {
        match arboard::Clipboard::new() {
            Ok(c) => self.clipboard = Some(c),
            Err(e) => {
                log::warn!("RDP integrado: portapapeles del sistema no disponible: {e}");
                let mut n = ClipNotice::new("unavailable");
                n.reason = Some(e.to_string());
                self.notifier.notice(n);
            }
        }
        while let Ok(msg) = rx.recv() {
            match msg {
                ClipIn::Shutdown => break,
                ClipIn::Announce => self.sync(true),
                ClipIn::Sync => self.sync(false),
                ClipIn::Capabilities(flags) => {
                    self.files_negotiated = self.options.files
                        && flags.contains(ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED);
                }
                ClipIn::RemoteCopy(formats) => self.on_remote_copy(&formats),
                ClipIn::FormatDataRequest(format) => self.on_format_data_request(format),
                ClipIn::FormatDataResponse { ok, data } => self.on_format_data_response(ok, &data),
                ClipIn::RemoteFileList(files, lock) => self.on_remote_file_list(&files, lock),
                ClipIn::FileContentsRequest(req) => self.on_file_contents_request(&req),
                ClipIn::FileContentsResponse {
                    stream_id,
                    ok,
                    data,
                } => self.on_file_contents_response(stream_id, ok, &data),
                ClipIn::Lock(id) => {
                    if self.locked.len() < MAX_LOCKED_FILE_LISTS {
                        self.locked.insert(id, Arc::clone(&self.local_files));
                    }
                }
                ClipIn::Unlock(id) => {
                    self.locked.remove(&id);
                }
            }
        }
        // Las descargas a medias no sirven para nada: fuera. La última completa
        // se queda, que puede estar en el portapapeles todavía.
        if let Some(dl) = self.download.take() {
            let _ = std::fs::remove_dir_all(&dl.root);
        }
    }

    fn emit(&self, out: ClipOut) {
        let _ = self.out.send(out);
    }

    // ── Local → remoto ──

    /// Compara el portapapeles local con lo último anunciado y, si cambió, lo
    /// anuncia. `force`: el servidor espera una lista sí o sí (aunque sea vacía).
    fn sync(&mut self, force: bool) {
        let Some(clipboard) = self.clipboard.as_mut() else {
            if force {
                self.emit(ClipOut::InitiateCopy(Vec::new()));
            }
            return;
        };

        if self.files_negotiated {
            if let Ok(paths) = clipboard.get().file_list() {
                if !paths.is_empty() {
                    let fp = Fingerprint::Files(paths.clone());
                    if !force && self.last.as_ref() == Some(&fp) {
                        return;
                    }
                    let (descriptors, entries) = collect_local_files(&paths);
                    if !descriptors.is_empty() {
                        self.last = Some(fp);
                        self.local_text = None;
                        self.local_files = Arc::new(entries);
                        self.reader = None;
                        self.emit(ClipOut::InitiateFileCopy(descriptors));
                        return;
                    }
                }
            }
        }

        match clipboard.get().text() {
            Ok(text) if !text.is_empty() => {
                let fp = text_fingerprint(&text);
                if !force && self.last.as_ref() == Some(&fp) {
                    return;
                }
                self.last = Some(fp);
                self.local_text = Some(text);
                self.emit(ClipOut::InitiateCopy(vec![ClipboardFormat::new(
                    ClipboardFormatId::CF_UNICODETEXT,
                )]));
            }
            _ => {
                if force {
                    self.emit(ClipOut::InitiateCopy(Vec::new()));
                }
            }
        }
    }

    fn on_format_data_request(&mut self, format: ClipboardFormatId) {
        let response = match (&self.local_text, format) {
            (Some(text), ClipboardFormatId::CF_UNICODETEXT) => {
                OwnedFormatDataResponse::new_unicode_string(text)
            }
            _ => OwnedFormatDataResponse::new_error(),
        };
        self.emit(ClipOut::FormatData(response));
    }

    fn on_file_contents_request(&mut self, req: &FileContentsRequest) {
        let entries = match req.data_id.and_then(|id| self.locked.get(&id)) {
            Some(list) => Arc::clone(list),
            None => Arc::clone(&self.local_files),
        };
        let response = serve_file_contents(&entries, req, &mut self.reader)
            .unwrap_or_else(|| FileContentsResponse::new_error(req.stream_id));
        self.emit(ClipOut::FileContents(response));
    }

    // ── Remoto → local ──

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        let file_list = formats.iter().find(|f| {
            f.name()
                .is_some_and(|n| n.value() == ClipboardFormatName::FILE_LIST.value())
        });
        if let (true, Some(fmt)) = (self.files_negotiated, file_list) {
            self.pending = PendingPaste::None;
            self.emit(ClipOut::InitiatePaste(fmt.id()));
            return;
        }
        if formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT)
        {
            self.pending = PendingPaste::Text;
            self.emit(ClipOut::InitiatePaste(ClipboardFormatId::CF_UNICODETEXT));
        }
    }

    fn on_format_data_response(&mut self, ok: bool, data: &[u8]) {
        if self.pending != PendingPaste::Text {
            return;
        }
        self.pending = PendingPaste::None;
        if !ok {
            return;
        }
        let text = decode_utf16z(data);
        let Some(clipboard) = self.clipboard.as_mut() else {
            return;
        };
        if clipboard.set().text(text.clone()).is_ok() {
            self.last = Some(text_fingerprint(&text));
            // Lo que hay ahora en el portapapeles local es del remoto: no se
            // vuelve a servir como si fuera nuestro.
            self.local_text = None;
        }
    }

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], lock: Option<u32>) {
        // Una copia nueva sustituye a la descarga en curso y a las anteriores.
        if let Some(old) = self.download.take() {
            let _ = std::fs::remove_dir_all(&old.root);
        }
        if self.batch > 0 {
            let _ = std::fs::remove_dir_all(self.temp_root.join(self.batch.to_string()));
        }
        self.batch = self.batch.wrapping_add(1);
        let root = self.temp_root.join(self.batch.to_string());

        let plan = match plan_download(files, &root) {
            Ok(plan) => plan,
            Err(reason) => {
                self.fail_download(reason);
                return;
            }
        };
        if plan.total > MAX_DOWNLOAD_BYTES {
            self.fail_download("too-large".into());
            return;
        }
        if let Err(e) = create_private_dir(&self.temp_root).and_then(|()| create_private_dir(&root))
        {
            self.fail_download(e.to_string());
            return;
        }
        let mut n = ClipNotice::new("download-start");
        n.files = Some(plan.items.iter().filter(|i| !i.dir).count());
        n.total = Some(plan.total);
        self.notifier.notice(n);
        self.download = Some(Download {
            root,
            items: plan.items,
            cur: 0,
            file: None,
            pos: 0,
            waiting: None,
            lock,
            total: plan.total,
            done: 0,
            top_level: plan.top_level,
            last_notice: Instant::now(),
        });
        self.advance_download();
    }

    fn fail_download(&mut self, reason: String) {
        if let Some(dl) = self.download.take() {
            let _ = std::fs::remove_dir_all(&dl.root);
        }
        let mut n = ClipNotice::new("download-error");
        n.reason = Some(reason);
        self.notifier.notice(n);
    }

    fn stream_id(&mut self) -> u32 {
        let id = self.next_stream;
        self.next_stream = self.next_stream.wrapping_add(1).max(1);
        id
    }

    /// Pide el siguiente trozo, o pasa al siguiente fichero, o termina.
    fn advance_download(&mut self) {
        loop {
            let stream_id = self.stream_id();
            let Some(dl) = self.download.as_mut() else {
                return;
            };
            let Some(item) = dl.items.get(dl.cur) else {
                self.finish_download();
                return;
            };
            if item.dir {
                if let Err(e) = create_private_dir(&item.path) {
                    let reason = e.to_string();
                    self.fail_download(reason);
                    return;
                }
                dl.cur += 1;
                continue;
            }
            if dl.file.is_none() {
                if let Some(parent) = item.path.parent() {
                    let _ = create_private_dir(parent);
                }
                match create_new_private_file(&item.path) {
                    Ok(f) => dl.file = Some(f),
                    Err(e) => {
                        let reason = e.to_string();
                        self.fail_download(reason);
                        return;
                    }
                }
                dl.pos = 0;
            }
            let request = match item.size {
                None => FileContentsRequest {
                    stream_id,
                    index: item.index,
                    flags: FileContentsFlags::SIZE,
                    position: 0,
                    requested_size: 8,
                    data_id: dl.lock,
                },
                Some(size) if dl.pos >= size => {
                    dl.file = None;
                    dl.cur += 1;
                    continue;
                }
                Some(size) => FileContentsRequest {
                    stream_id,
                    index: item.index,
                    flags: FileContentsFlags::RANGE,
                    position: dl.pos,
                    requested_size: u32::try_from((size - dl.pos).min(u64::from(DOWNLOAD_CHUNK)))
                        .unwrap_or(DOWNLOAD_CHUNK),
                    data_id: dl.lock,
                },
            };
            dl.waiting = Some(stream_id);
            self.emit(ClipOut::RequestFileContents(request));
            return;
        }
    }

    fn on_file_contents_response(&mut self, stream_id: u32, ok: bool, data: &[u8]) {
        let Some(dl) = self.download.as_mut() else {
            return;
        };
        if dl.waiting != Some(stream_id) {
            return;
        }
        dl.waiting = None;
        if !ok {
            self.fail_download("remote".into());
            return;
        }
        let Some(item) = dl.items.get_mut(dl.cur) else {
            return;
        };
        match item.size {
            None => {
                let size = data
                    .get(..8)
                    .and_then(|b| <[u8; 8]>::try_from(b).ok())
                    .map_or(0, u64::from_le_bytes);
                item.size = Some(size);
                dl.total = dl.total.saturating_add(size);
                if dl.total > MAX_DOWNLOAD_BYTES {
                    self.fail_download("too-large".into());
                    return;
                }
            }
            Some(size) => {
                if data.is_empty() {
                    // Sin avance no hay final: el fichero cambió en el remoto.
                    self.fail_download("short-read".into());
                    return;
                }
                let take = usize::try_from((size - dl.pos).min(data.len() as u64)).unwrap_or(0);
                let write = dl
                    .file
                    .as_mut()
                    .map_or(Ok(()), |f| f.write_all(&data[..take]));
                if let Err(e) = write {
                    let reason = e.to_string();
                    self.fail_download(reason);
                    return;
                }
                dl.pos += take as u64;
                dl.done += take as u64;
                if dl.last_notice.elapsed() >= PROGRESS_EVERY {
                    dl.last_notice = Instant::now();
                    let mut n = ClipNotice::new("download-progress");
                    n.bytes = Some(dl.done);
                    n.total = Some(dl.total);
                    self.notifier.notice(n);
                }
            }
        }
        self.advance_download();
    }

    fn finish_download(&mut self) {
        let Some(dl) = self.download.take() else {
            return;
        };
        let count = dl.items.iter().filter(|i| !i.dir).count();
        let result = match self.clipboard.as_mut() {
            Some(c) => c.set().file_list(&dl.top_level).map_err(|e| e.to_string()),
            None => Err("clipboard".into()),
        };
        match result {
            Ok(()) => {
                self.last = Some(Fingerprint::Files(dl.top_level.clone()));
                self.local_text = None;
                let mut n = ClipNotice::new("download-done");
                n.files = Some(count);
                n.bytes = Some(dl.done);
                self.notifier.notice(n);
            }
            Err(reason) => {
                let _ = std::fs::remove_dir_all(&dl.root);
                let mut n = ClipNotice::new("download-error");
                n.reason = Some(reason);
                self.notifier.notice(n);
            }
        }
    }
}

// ─── Utilidades ──────────────────────────────────────────────────────────────

/// Texto UTF-16LE terminado (o no) en NUL.
fn decode_utf16z(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn filetime(meta: &std::fs::Metadata) -> Option<u64> {
    let secs = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .unwrap_or_default();
    let ft_secs = secs.as_secs().checked_add(FILETIME_UNIX_OFFSET)?;
    ft_secs
        .checked_mul(10_000_000)?
        .checked_add(u64::from(secs.subsec_nanos() / 100))
}

/// Recorre lo copiado en local (ficheros y carpetas, en preorden) y devuelve la
/// lista para el servidor y, alineadas por índice, las rutas locales.
///
/// Lo que el protocolo no puede representar (nombres demasiado largos) o no
/// conviene seguir (enlaces simbólicos) se salta: así IronRDP no descarta nada
/// por su cuenta y los índices siguen alineados.
fn collect_local_files(paths: &[PathBuf]) -> (Vec<FileDescriptor>, Vec<LocalEntry>) {
    let mut descriptors = Vec::new();
    let mut entries = Vec::new();
    for path in paths {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        walk(path, &name, None, 0, &mut descriptors, &mut entries);
    }
    (descriptors, entries)
}

fn walk(
    path: &Path,
    name: &str,
    relative: Option<&str>,
    depth: usize,
    descriptors: &mut Vec<FileDescriptor>,
    entries: &mut Vec<LocalEntry>,
) {
    if entries.len() >= MAX_LOCAL_ENTRIES || depth > MAX_LOCAL_DEPTH {
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() || name.is_empty() {
        return;
    }
    let wire = match relative {
        Some(r) => format!("{r}\\{name}"),
        None => name.to_string(),
    };
    if wire.chars().count() > MAX_WIRE_NAME {
        return;
    }
    let mut descriptor = FileDescriptor::new(name);
    if let Some(r) = relative {
        descriptor = descriptor.with_relative_path(r);
    }
    if let Some(t) = filetime(&meta) {
        descriptor = descriptor.with_last_write_time(t);
    }
    if meta.is_dir() {
        descriptors.push(descriptor.with_attributes(ClipboardFileAttributes::DIRECTORY));
        entries.push(LocalEntry {
            path: path.to_path_buf(),
            dir: true,
        });
        let Ok(read) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<_> = read.filter_map(Result::ok).collect();
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let child_name = child.file_name().to_string_lossy().into_owned();
            walk(
                &child.path(),
                &child_name,
                Some(&wire),
                depth + 1,
                descriptors,
                entries,
            );
        }
    } else if meta.is_file() {
        descriptors.push(
            descriptor
                .with_attributes(ClipboardFileAttributes::ARCHIVE)
                .with_file_size(meta.len()),
        );
        entries.push(LocalEntry {
            path: path.to_path_buf(),
            dir: false,
        });
    }
}

/// Responde a una petición del servidor sobre un fichero anunciado.
fn serve_file_contents(
    entries: &Arc<Vec<LocalEntry>>,
    req: &FileContentsRequest,
    reader: &mut Option<(usize, usize, File)>,
) -> Option<FileContentsResponse<'static>> {
    let index = usize::try_from(req.index).ok()?;
    let entry = entries.get(index)?;
    if entry.dir {
        return None;
    }
    if req.flags.contains(FileContentsFlags::SIZE) {
        let len = std::fs::metadata(&entry.path).ok()?.len();
        return Some(FileContentsResponse::new_size_response(req.stream_id, len));
    }
    // El fichero se mantiene abierto entre trozos consecutivos del mismo.
    let list_id = Arc::as_ptr(entries) as usize;
    let reuse = matches!(reader, Some((i, l, _)) if *i == index && *l == list_id);
    if !reuse {
        *reader = Some((index, list_id, File::open(&entry.path).ok()?));
    }
    let (_, _, file) = reader.as_mut()?;
    file.seek(SeekFrom::Start(req.position)).ok()?;
    let want = req.requested_size.min(UPLOAD_CHUNK_MAX) as usize;
    let mut buf = Vec::with_capacity(want);
    file.take(want as u64).read_to_end(&mut buf).ok()?;
    Some(FileContentsResponse::new_data_response(req.stream_id, buf))
}

struct DownloadPlan {
    items: Vec<DlItem>,
    total: u64,
    top_level: Vec<PathBuf>,
}

/// Un componente de ruta que llega del servidor y se puede crear sin salir de
/// la carpeta de la descarga.
fn safe_component(c: &str) -> bool {
    !c.is_empty()
        && c != "."
        && c != ".."
        && !c.contains(['/', '\\', ':', '\0'])
        && !c.ends_with(['.', ' '])
        && !ironrdp_cliprdr::is_windows_device_name(c)
}

/// Traduce la lista remota a rutas bajo `root`. Falla entera si un nombre no es
/// seguro: mejor no copiar nada que copiar a medias en un sitio raro.
fn plan_download(files: &[FileDescriptor], root: &Path) -> Result<DownloadPlan, String> {
    let mut items = Vec::with_capacity(files.len());
    let mut total = 0u64;
    let mut top_level = Vec::new();
    for (index, fd) in files.iter().enumerate() {
        let mut path = root.to_path_buf();
        let mut components = 0usize;
        if let Some(rel) = fd.relative_path.as_deref().filter(|r| !r.is_empty()) {
            for c in rel.split('\\') {
                if !safe_component(c) {
                    return Err("unsafe-name".into());
                }
                path.push(c);
                components += 1;
            }
        }
        if !safe_component(&fd.name) {
            return Err("unsafe-name".into());
        }
        path.push(&fd.name);
        let dir = fd
            .attributes
            .is_some_and(|a| a.contains(ClipboardFileAttributes::DIRECTORY));
        if components == 0 && !top_level.contains(&path) {
            top_level.push(path.clone());
        }
        let size = if dir { Some(0) } else { fd.file_size };
        total = total.saturating_add(size.unwrap_or(0));
        items.push(DlItem {
            index: i32::try_from(index).map_err(|_| "too-many".to_string())?,
            path,
            size,
            dir,
        });
    }
    if top_level.is_empty() {
        return Err("empty".into());
    }
    Ok(DownloadPlan {
        items,
        total,
        top_level,
    })
}

/// Carpeta de las descargas del portapapeles de una sesión, bajo `base` (la
/// caché **del usuario**, nunca el `/tmp` compartido: ahí otro usuario podría
/// crear antes la carpeta o dejar enlaces simbólicos donde vamos a escribir).
#[must_use]
pub fn session_temp_root(base: &Path, session_id: &str) -> PathBuf {
    base.join(session_id)
}

/// Crea una carpeta (y las que falten) que solo ve el usuario, y comprueba que
/// lo que hay en esa ruta es una carpeta de verdad y no un enlace.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(std::io::Error::other(
            "la carpeta de descargas no es una carpeta",
        ));
    }
    Ok(())
}

/// Crea un fichero **nuevo** (falla si ya existe algo en esa ruta, también un
/// enlace simbólico: nunca se sigue ni se sobrescribe nada que no sea nuestro).
fn create_new_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// Borra las descargas de sesiones de hace más de un día (al arrancar la app).
pub fn cleanup_stale_downloads(root: &Path) {
    let Ok(read) = std::fs::read_dir(root) else {
        return;
    };
    let day = Duration::from_secs(24 * 3600);
    for entry in read.filter_map(Result::ok) {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichero_nuevo_no_pisa_lo_que_ya_existe() {
        let dir = std::env::temp_dir().join(format!("rustty-clip-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_private_dir(&dir.join("a")).unwrap();
        let path = dir.join("a").join("f.txt");
        create_new_private_file(&path).unwrap();
        assert!(create_new_private_file(&path).is_err());
        assert!(
            create_private_dir(&path).is_err(),
            "un fichero no vale como carpeta"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texto_utf16_hasta_el_nul() {
        let mut data: Vec<u8> = "hola ñ".encode_utf16().flat_map(u16::to_le_bytes).collect();
        data.extend([0, 0, b'x', 0]);
        assert_eq!(decode_utf16z(&data), "hola ñ");
    }

    #[test]
    fn componentes_inseguros_rechazados() {
        for bad in [
            "", ".", "..", "a/b", "a\\b", "C:", "con", "NUL.txt", "x.", "x ",
        ] {
            assert!(!safe_component(bad), "{bad:?}");
        }
        for ok in ["informe.pdf", "carpeta", ".bashrc", "a b"] {
            assert!(safe_component(ok), "{ok:?}");
        }
    }

    #[test]
    fn plan_de_descarga_queda_dentro_de_la_raiz() {
        let root = PathBuf::from("raiz");
        let files = vec![
            FileDescriptor::new("docs").with_attributes(ClipboardFileAttributes::DIRECTORY),
            FileDescriptor::new("a.txt")
                .with_relative_path("docs")
                .with_file_size(5),
            FileDescriptor::new("b.bin").with_file_size(7),
        ];
        let plan = plan_download(&files, &root).unwrap();
        assert_eq!(plan.total, 12);
        assert_eq!(plan.top_level, vec![root.join("docs"), root.join("b.bin")]);
        assert_eq!(plan.items[1].path, root.join("docs").join("a.txt"));
        assert!(plan.items.iter().all(|i| i.path.starts_with(&root)));

        let traversal = vec![FileDescriptor::new("x").with_relative_path("..\\..")];
        assert!(plan_download(&traversal, &root).is_err());
    }

    #[test]
    fn lista_local_en_preorden_con_rutas_relativas() {
        let dir = std::env::temp_dir().join(format!("rustty-clip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("f.txt"), b"abc").unwrap();
        std::fs::write(dir.join("g.txt"), b"hello").unwrap();

        let (descs, entries) = collect_local_files(std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        let top = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(descs.len(), entries.len());
        assert_eq!(descs[0].name, top);
        assert!(entries[0].dir);
        let g = descs.iter().position(|d| d.name == "g.txt").unwrap();
        assert_eq!(descs[g].relative_path.as_deref(), Some(top.as_str()));
        assert_eq!(descs[g].file_size, Some(5));
        let f = descs.iter().position(|d| d.name == "f.txt").unwrap();
        let sub = descs.iter().position(|d| d.name == "sub").unwrap();
        assert!(sub < f, "la carpeta va antes que su contenido");
        assert_eq!(
            descs[f].relative_path.as_deref(),
            Some(format!("{top}\\sub").as_str())
        );
    }

    #[test]
    fn sirve_tamano_y_trozos_de_un_fichero() {
        let path = std::env::temp_dir().join(format!("rustty-clip-serve-{}", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();
        let entries = Arc::new(vec![LocalEntry {
            path: path.clone(),
            dir: false,
        }]);
        let mut reader = None;
        let size = serve_file_contents(
            &entries,
            &FileContentsRequest {
                stream_id: 1,
                index: 0,
                flags: FileContentsFlags::SIZE,
                position: 0,
                requested_size: 8,
                data_id: None,
            },
            &mut reader,
        )
        .unwrap();
        assert_eq!(size.data_as_size().unwrap(), 10);
        let range = serve_file_contents(
            &entries,
            &FileContentsRequest {
                stream_id: 2,
                index: 0,
                flags: FileContentsFlags::RANGE,
                position: 3,
                requested_size: 4,
                data_id: None,
            },
            &mut reader,
        )
        .unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(range.data(), b"3456");
        assert!(serve_file_contents(
            &entries,
            &FileContentsRequest {
                stream_id: 3,
                index: 5,
                flags: FileContentsFlags::SIZE,
                position: 0,
                requested_size: 8,
                data_id: None,
            },
            &mut reader,
        )
        .is_none());
    }
}
