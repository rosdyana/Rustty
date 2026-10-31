//! Una sesión RDP integrada: conexión, autenticación y bucle activo.
//!
//! Corre en una tarea tokio propia. Recibe órdenes (entrada, resize, cierre)
//! por un `mpsc` acotado y confirmaciones de pintado por un contador atómico,
//! y manda al canvas los fotogramas por el `Channel` de la sesión.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ironrdp_connector::{
    self as connector, ClientConnector, ConnectionResult, Credentials, DesktopSize,
};
use ironrdp_displaycontrol::client::DisplayControlClient;
use ironrdp_displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp_dvc::DrdynvcClient;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_input::Database;
use ironrdp_pdu::gcc::{ConnectionType, KeyboardType};
use ironrdp_pdu::rdp::capability_sets::{client_codecs_capabilities, MajorPlatformType};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use ironrdp_tokio::{FramedWrite as _, TokioFramed};
use tauri::ipc::{Channel, Response};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Notify};
use zeroize::Zeroizing;

use super::certs;
use super::frames::{self, Damage, Flow, Rect};
use super::input::InputEvent;
use super::tls;

/// Plazo para abrir el TCP y para cada etapa de la negociación.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Plazo de la secuencia de reactivación tras un `DeactivateAll`.
const REACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Órdenes del frontend hacia la sesión.
#[derive(Debug)]
pub enum SessionCmd {
    Input(Vec<InputEvent>),
    Resize {
        width: u32,
        height: u32,
        scale: Option<u32>,
    },
    Close,
}

/// Estado compartido entre los comandos y la tarea de la sesión.
#[derive(Default)]
pub struct Shared {
    /// `seq` más alto pintado por el frontend.
    pub acked: AtomicU32,
    pub ack_notify: Notify,
}

/// Datos para conectar. La contraseña va en `Zeroizing`: se borra de memoria
/// al soltarla, justo después de la autenticación.
pub struct ConnectParams {
    /// Destino real (para el TOFU, el SNI y CredSSP).
    pub host: String,
    pub port: u16,
    /// Dónde abrir el TCP: el destino o el reenvío local del salto.
    pub dial_host: String,
    pub dial_port: u16,
    pub via_jump: bool,
    pub username: String,
    pub domain: Option<String>,
    pub password: Zeroizing<String>,
    pub width: u32,
    pub height: u32,
    /// Escala del escritorio en % (100..=500), del `devicePixelRatio`.
    pub scale: Option<u32>,
    pub cert_store: PathBuf,
}

/// Fallo de la sesión con un código estable para el frontend.
#[derive(Debug, Clone)]
pub struct SessionError {
    pub code: &'static str,
    pub message: String,
}

impl SessionError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Cómo terminó una sesión que llegó a estar activa.
#[derive(Debug, Clone)]
pub enum Ended {
    /// La cerró el usuario.
    Local,
    /// La cerró el servidor (cierre de sesión, desconexión administrativa…).
    Server(String),
}

/// Avisos de la sesión hacia el gestor (que los convierte en eventos).
pub trait Reporter: Send + Sync {
    fn connected(&self, width: u16, height: u16);
}

type Framed = TokioFramed<tokio_rustls::client::TlsStream<TcpStream>>;

fn os_platform() -> MajorPlatformType {
    if cfg!(windows) {
        MajorPlatformType::WINDOWS
    } else if cfg!(target_os = "macos") {
        MajorPlatformType::MACINTOSH
    } else {
        MajorPlatformType::UNIX
    }
}

/// Ajusta un tamaño a lo que admite el protocolo (200..=8192, ancho par).
#[must_use]
pub fn adjust_size(width: u32, height: u32) -> (u32, u32) {
    MonitorLayoutEntry::adjust_display_size(width, height)
}

/// `deviceScaleFactor` solo admite 100, 140 o 180.
fn device_scale(scale: u32) -> u32 {
    if scale >= 160 {
        180
    } else if scale >= 120 {
        140
    } else {
        100
    }
}

/// Separa `DOMINIO\usuario` cuando el perfil no trae dominio aparte.
fn split_domain(username: &str, domain: Option<&str>) -> (String, Option<String>) {
    let domain = domain.map(str::trim).filter(|d| !d.is_empty());
    if domain.is_none() {
        if let Some((d, u)) = username.split_once('\\') {
            if !d.is_empty() && !u.is_empty() {
                return (u.to_string(), Some(d.to_string()));
            }
        }
    }
    (username.to_string(), domain.map(str::to_string))
}

fn build_config(p: &ConnectParams) -> Result<connector::Config, SessionError> {
    let (width, height) = adjust_size(p.width, p.height);
    let (username, domain) = split_domain(&p.username, p.domain.as_deref());
    let codecs = client_codecs_capabilities(&[])
        .map_err(|e| SessionError::new("internal", format!("códecs: {e}")))?;
    let scale = p.scale.filter(|s| (100..=500).contains(s));
    Ok(connector::Config {
        credentials: Credentials::UsernamePassword {
            username,
            // Copia para IronRDP: vive dentro del conector, que se consume en la
            // autenticación. Nuestra copia sí se borra (`Zeroizing`).
            password: p.password.as_str().to_owned(),
        },
        domain,
        enable_tls: true,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        // 0 = la distribución del servidor: los scancodes son posicionales.
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        connection_type: ConnectionType::Lan,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: DesktopSize {
            width: u16::try_from(width).unwrap_or(1280),
            height: u16::try_from(height).unwrap_or(800),
        },
        monitor_layout: None,
        bitmap: Some(connector::BitmapConfig {
            lossy_compression: true,
            color_depth: 32,
            codecs,
        }),
        client_build: 0,
        client_name: "rustty".to_owned(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
        platform: os_platform(),
        enable_server_pointer: true,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        enable_audio_capture: false,
        compression_type: None,
        pointer_software_rendering: true,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
        performance_flags: PerformanceFlags::default(),
        desktop_scale_factor: scale.unwrap_or(0),
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: scale.map(device_scale),
        monitor_extended_layout: None,
        relative_mouse_input: false,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
    })
}

/// Cliente de red de CredSSP. Solo lo usa Kerberos para hablar con un KDC,
/// que esta versión no soporta: con NTLM nunca se llama.
struct NoKdc;

impl ironrdp_tokio::NetworkClient for NoKdc {
    async fn send(
        &mut self,
        _request: &ironrdp_connector::sspi::generator::NetworkRequest,
    ) -> connector::ConnectorResult<Vec<u8>> {
        Err(connector::general_err!(
            "Kerberos con KDC no soportado en el cliente integrado"
        ))
    }
}

fn classify_connect_error(err: &connector::ConnectorError) -> SessionError {
    // El texto de IronRDP no es estable; se busca lo justo para distinguir lo
    // que el usuario puede arreglar (credenciales) de lo que no.
    let text = format!("{err}");
    let lower = text.to_ascii_lowercase();
    let code = if lower.contains("logon")
        || lower.contains("credential")
        || lower.contains("access denied")
        || lower.contains("authentication")
    {
        "auth-failed"
    } else if lower.contains("negotiation") || lower.contains("security protocol") {
        "unsupported"
    } else {
        "protocol"
    };
    SessionError::new(code, text)
}

async fn with_timeout<T, E>(
    what: &str,
    fut: impl std::future::Future<Output = Result<T, E>>,
    map: impl FnOnce(E) -> SessionError,
) -> Result<T, SessionError> {
    match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
        Ok(r) => r.map_err(map),
        Err(_) => Err(SessionError::new(
            "timeout",
            format!("{what}: tiempo agotado"),
        )),
    }
}

/// Conecta, comprueba el certificado (TOFU) y autentica.
async fn connect(params: ConnectParams) -> Result<(ConnectionResult, Framed), SessionError> {
    let tcp = with_timeout(
        "Conexión TCP",
        TcpStream::connect((params.dial_host.as_str(), params.dial_port)),
        |e| SessionError::new("unreachable", e.to_string()),
    )
    .await?;
    let _ = tcp.set_nodelay(true);
    let client_addr = tcp
        .local_addr()
        .map_err(|e| SessionError::new("unreachable", e.to_string()))?;

    let config = build_config(&params)?;
    let drdynvc =
        DrdynvcClient::new().with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())));
    let mut connector = ClientConnector::new(config, client_addr).with_static_channel(drdynvc);

    let mut framed = TokioFramed::new(tcp);
    let should_upgrade = with_timeout(
        "Negociación",
        ironrdp_tokio::connect_begin(&mut framed, &mut connector),
        |e| classify_connect_error(&e),
    )
    .await?;

    let tcp = framed.into_inner_no_leftover();
    let upgraded = with_timeout("TLS", tls::upgrade(tcp, &params.host), |e| {
        SessionError::new("tls", e.to_string())
    })
    .await?;

    // TOFU **antes** de CredSSP: sin certificado confirmado no sale ninguna
    // credencial. Sin plazo propio: el diálogo tiene el suyo.
    certs::verify(
        &params.cert_store,
        &params.host,
        params.port,
        &upgraded.cert_der,
        params.via_jump,
        certs::Policy::current(),
    )
    .await
    .map_err(|r| SessionError::new(r.code, r.message))?;

    let marker = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = TokioFramed::new(upgraded.stream);
    let server_name = params.host.clone();
    let outcome = with_timeout(
        "Autenticación",
        ironrdp_tokio::connect_finalize(
            marker,
            connector,
            &mut framed,
            &mut NoKdc,
            server_name.into(),
            upgraded.server_public_key,
            None,
        ),
        |e| classify_connect_error(&e),
    )
    .await?;
    // La contraseña ya no hace falta: se borra aquí (no al final de la sesión).
    drop(params.password);
    let result = outcome
        .into_connected()
        .map_err(|e| SessionError::new("redirect", e.to_string()))?;
    Ok((result, framed))
}

/// Emisor de fotogramas con contrapresión.
struct Sink {
    channel: Channel<Response>,
    flow: Flow,
    damage: Damage,
}

impl Sink {
    fn send(&mut self, bytes: Vec<u8>) -> bool {
        self.channel.send(Response::new(bytes)).is_ok()
    }

    fn control(&mut self, kind: u8) -> bool {
        let seq = self.flow.next_seq();
        self.send(frames::encode_simple(kind, seq))
    }

    /// Manda lo pendiente si el frontend va al día. `false` = canal cerrado.
    fn flush(&mut self, image: &DecodedImage) -> bool {
        if self.damage.is_empty() || !self.flow.can_send() {
            return true;
        }
        let rects = self.damage.take(image.width(), image.height());
        if rects.is_empty() {
            return true;
        }
        let seq = self.flow.next_seq();
        let msg = frames::encode_rects(seq, &rects, image.data(), image.stride());
        self.send(msg)
    }
}

/// Ejecuta la sesión completa. Vuelve cuando termina.
pub async fn run(
    params: ConnectParams,
    channel: Channel<Response>,
    mut cmds: mpsc::Receiver<SessionCmd>,
    shared: Arc<Shared>,
    reporter: Arc<dyn Reporter>,
) -> Result<Ended, SessionError> {
    // Una orden de cierre durante la conexión (incluido el diálogo del
    // certificado) la corta en seco.
    let (result, mut framed) = tokio::select! {
        r = connect(params) => r?,
        cmd = wait_close(&mut cmds) => {
            let () = cmd;
            return Ok(Ended::Local);
        }
    };

    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let activation_factory = result.activation_factory.clone();
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    let mut sink = Sink {
        channel,
        flow: Flow::default(),
        damage: Damage::default(),
    };
    let mut input_db = Database::new();

    let seq = sink.flow.next_seq();
    sink.send(frames::encode_desktop_size(
        seq,
        image.width(),
        image.height(),
    ));
    reporter.connected(image.width(), image.height());

    loop {
        if !sink.flush(&image) {
            // El frontend cerró el canal (pestaña cerrada): cierre ordenado.
            shutdown(&mut stage, &mut framed).await;
            return Ok(Ended::Local);
        }

        tokio::select! {
            cmd = cmds.recv() => match cmd {
                None | Some(SessionCmd::Close) => {
                    shutdown(&mut stage, &mut framed).await;
                    return Ok(Ended::Local);
                }
                Some(SessionCmd::Input(events)) => {
                    let fastpath = to_fastpath(&mut input_db, events);
                    if !fastpath.is_empty() {
                        let outputs = stage
                            .process_fastpath_input(&mut image, &fastpath)
                            .map_err(|e| SessionError::new("protocol", e.to_string()))?;
                        if let Some(end) = handle_outputs(outputs, &mut framed, &mut sink, &mut image, &mut stage, &activation_factory).await? {
                            return Ok(end);
                        }
                    }
                }
                Some(SessionCmd::Resize { width, height, scale }) => {
                    // Fase 2: Display Control. Hasta entonces el canvas escala.
                    let _ = (width, height, scale);
                }
            },
            () = shared.ack_notify.notified() => {
                sink.flow.ack(shared.acked.load(Ordering::Acquire));
            }
            pdu = framed.read_pdu() => {
                let (action, payload) = match pdu {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        return Ok(Ended::Server(String::new()));
                    }
                    Err(e) => return Err(SessionError::new("network", e.to_string())),
                };
                let received_at = framed.last_read_at();
                let outputs = stage
                    .process(&mut image, action, &payload, received_at)
                    .map_err(|e| SessionError::new("protocol", e.to_string()))?;
                if let Some(end) = handle_outputs(outputs, &mut framed, &mut sink, &mut image, &mut stage, &activation_factory).await? {
                    return Ok(end);
                }
            }
        }
    }
}

async fn wait_close(cmds: &mut mpsc::Receiver<SessionCmd>) {
    loop {
        match cmds.recv().await {
            None | Some(SessionCmd::Close) => return,
            // Entrada o resize antes de estar conectados: se descartan.
            Some(_) => {}
        }
    }
}

fn to_fastpath(
    db: &mut Database,
    events: Vec<InputEvent>,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    let mut out = Vec::new();
    let mut ops = Vec::new();
    for ev in events {
        match ev {
            InputEvent::Op(op) => {
                if let Some(op) = op.to_operation() {
                    ops.push(op);
                }
            }
            InputEvent::ReleaseAll => {
                out.extend(db.apply(ops.drain(..)));
                out.extend(db.release_all());
            }
            InputEvent::Sync {
                scroll,
                num,
                caps,
                kana,
            } => {
                out.extend(db.apply(ops.drain(..)));
                out.push(ironrdp_input::synchronize_event(scroll, num, caps, kana));
            }
        }
    }
    out.extend(db.apply(ops));
    out
}

async fn shutdown(stage: &mut ActiveStage, framed: &mut Framed) {
    if let Ok(outputs) = stage.graceful_shutdown() {
        for out in outputs {
            if let ActiveStageOutput::ResponseFrame(frame) = out {
                let _ =
                    tokio::time::timeout(Duration::from_secs(2), framed.write_all(&frame)).await;
            }
        }
    }
}

async fn handle_outputs(
    outputs: Vec<ActiveStageOutput>,
    framed: &mut Framed,
    sink: &mut Sink,
    image: &mut DecodedImage,
    stage: &mut ActiveStage,
    activation_factory: &connector::connection_activation::ConnectionActivationFactory,
) -> Result<Option<Ended>, SessionError> {
    for out in outputs {
        match out {
            ActiveStageOutput::ResponseFrame(frame) => {
                framed
                    .write_all(&frame)
                    .await
                    .map_err(|e| SessionError::new("network", e.to_string()))?;
            }
            ActiveStageOutput::GraphicsUpdate(r) => {
                if let Some(rect) = Rect::from_inclusive(r.left, r.top, r.right, r.bottom) {
                    sink.damage.add(rect);
                }
            }
            ActiveStageOutput::PointerDefault => {
                sink.control(frames::MSG_POINTER_DEFAULT);
            }
            ActiveStageOutput::PointerHidden => {
                sink.control(frames::MSG_POINTER_HIDDEN);
            }
            ActiveStageOutput::PointerBitmap(p) => {
                let seq = sink.flow.next_seq();
                let msg = frames::encode_pointer_bitmap(
                    seq,
                    p.width,
                    p.height,
                    p.hotspot_x,
                    p.hotspot_y,
                    &p.bitmap_data,
                );
                sink.send(msg);
            }
            ActiveStageOutput::DeactivateAll => {
                reactivate(framed, sink, image, stage, activation_factory).await?;
            }
            ActiveStageOutput::Terminate(reason) => {
                return Ok(Some(Ended::Server(reason.to_string())));
            }
            ActiveStageOutput::ServerRedirection(_) => {
                return Err(SessionError::new(
                    "redirect",
                    "El servidor pide redirigir la sesión, y el cliente integrado no lo soporta",
                ));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Secuencia de desactivación-reactivación (cambio de resolución, reconexión
/// del escritorio). El canvas cambia de tamaño **aquí** y no al pedir el
/// resize: hasta este punto aún pueden llegar fotogramas del tamaño anterior.
async fn reactivate(
    framed: &mut Framed,
    sink: &mut Sink,
    image: &mut DecodedImage,
    stage: &mut ActiveStage,
    factory: &connector::connection_activation::ConnectionActivationFactory,
) -> Result<(), SessionError> {
    use connector::connection_activation::ConnectionActivationState;
    use ironrdp_core::WriteBuf;

    let mut seq = factory.create();
    let mut buf = WriteBuf::new();
    let fut = async {
        loop {
            let written = ironrdp_tokio::single_sequence_step_read(framed, &mut seq, &mut buf)
                .await
                .map_err(|e| SessionError::new("protocol", e.to_string()))?;
            if written.size().is_some() {
                framed
                    .write_all(buf.filled())
                    .await
                    .map_err(|e| SessionError::new("network", e.to_string()))?;
            }
            if let ConnectionActivationState::Finalized {
                desktop_size,
                share_id,
                enable_server_pointer,
                pointer_software_rendering,
                static_channel_chunk_size,
                window_support_level,
                ..
            } = seq.connection_activation_state()
            {
                return Ok::<_, SessionError>((
                    desktop_size,
                    share_id,
                    enable_server_pointer,
                    pointer_software_rendering,
                    static_channel_chunk_size,
                    window_support_level,
                ));
            }
        }
    };
    let (size, share_id, server_pointer, sw_pointer, chunk, window_level) =
        match tokio::time::timeout(REACTIVATION_TIMEOUT, fut).await {
            Ok(r) => r?,
            Err(_) => {
                return Err(SessionError::new(
                    "reactivation-timeout",
                    "La reactivación no terminó a tiempo",
                ))
            }
        };

    // Suelta el framebuffer viejo antes de reservar el nuevo.
    *image = DecodedImage::new(PixelFormat::RgbA32, 1, 1);
    *image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
    if !stage.reactivate(
        seq.io_channel_id(),
        seq.user_channel_id(),
        share_id,
        server_pointer,
        sw_pointer,
        chunk,
    ) {
        return Err(SessionError::new(
            "protocol",
            "Tamaño de bloque de canal inválido",
        ));
    }
    stage.set_window_support_level(window_level);
    sink.damage.clear();
    let seq_no = sink.flow.next_seq();
    sink.send(frames::encode_desktop_size(seq_no, size.width, size.height));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_dominio_del_usuario() {
        assert_eq!(
            split_domain("CORP\\ana", None),
            ("ana".to_string(), Some("CORP".to_string()))
        );
        assert_eq!(
            split_domain("CORP\\ana", Some("OTRO")),
            ("CORP\\ana".to_string(), Some("OTRO".to_string()))
        );
        assert_eq!(split_domain("ana", Some("  ")), ("ana".to_string(), None));
        assert_eq!(
            split_domain("ana@corp.local", None),
            ("ana@corp.local".to_string(), None)
        );
    }

    #[test]
    fn escala_de_dispositivo_por_tramos() {
        assert_eq!(device_scale(100), 100);
        assert_eq!(device_scale(125), 140);
        assert_eq!(device_scale(150), 140);
        assert_eq!(device_scale(200), 180);
    }

    #[test]
    fn tamano_ajustado_al_protocolo() {
        assert_eq!(adjust_size(1921, 100), (1920, 200));
        assert_eq!(adjust_size(9000, 9000), (8192, 8192));
    }

    #[test]
    fn configuracion_lleva_credenciales_y_tamano() {
        let p = ConnectParams {
            host: "srv".into(),
            port: 3389,
            dial_host: "srv".into(),
            dial_port: 3389,
            via_jump: false,
            username: "DOM\\u".into(),
            domain: None,
            password: Zeroizing::new("x".into()),
            width: 1001,
            height: 700,
            scale: Some(150),
            cert_store: PathBuf::new(),
        };
        let c = build_config(&p).unwrap();
        assert_eq!(c.desktop_size.width, 1000);
        assert_eq!(c.domain.as_deref(), Some("DOM"));
        assert_eq!(c.desktop_scale_factor, 150);
        assert_eq!(c.device_scale_factor, Some(140));
        assert!(c.enable_credssp);
        assert!(!c.enable_standard_rdp_security);
    }
}
