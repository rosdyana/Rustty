//! Escritorio remoto RDP **dentro de la pestaña** (cliente integrado IronRDP).
//!
//! Es el modo por defecto; el lanzador de cliente externo (`rdp_manager`)
//! queda para quien lo elija en Preferencias (`prefs.rdpMode`). Además de la
//! imagen y la entrada, la sesión puede llevar portapapeles compartido
//! ([`clipboard`], texto y ficheros) y una carpeta local como unidad
//! ([`drive`]). Aquí viven el gestor de sesiones y los comandos Tauri, todos
//! acotados a una sesión que el gestor conoce:
//!
//! | Comando                 | Para qué                                                   |
//! |-------------------------|------------------------------------------------------------|
//! | `rdp_embed_connect`     | Abre la sesión de un perfil (el secreto se resuelve aquí)  |
//! | `rdp_embed_input`       | Lote binario de ratón y teclado (cuerpo crudo)             |
//! | `rdp_embed_resize`      | Pide un tamaño nuevo del escritorio                        |
//! | `rdp_embed_ack`         | Confirma el último fotograma pintado (contrapresión)       |
//! | `rdp_embed_disconnect`  | Cierra la sesión                                           |
//! | `rdp_embed_clipboard_sync` | Anuncia al remoto el portapapeles local si cambió       |
//! | `rdp_shared_folder_*`   | Carpeta local compartida como unidad (la elige el backend) |
//! | `rdp_cert_response`     | Respuesta al diálogo de certificado (TOFU)                 |
//! | `rdp_known_certs_list`  | Gestor: huellas guardadas                                  |
//! | `rdp_known_cert_remove` | Gestor: olvidar una huella                                 |
//!
//! Ninguno añade permisos de plugin: son comandos de la app, sin acceso a
//! ficheros ni a la red más allá de la conexión RDP que abre el propio backend.

pub mod certs;
pub mod clipboard;
pub mod drive;
pub mod frames;
pub mod input;
mod perf;
mod session;
mod tls;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::{Channel, InvokeBody, Request, Response};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;

use crate::commands::{
    apply_connect_overrides, apply_credential, external_forward, resolve_connection_secret,
    ConnectOverrides,
};
use crate::credentials::{self, CredentialStore};
use crate::ipc::{event_name, EventKind};
use crate::jump_forward::ForwardRegistry;
use crate::jump_pool::JumpPool;
use crate::jump_route::JumpRequest;
use crate::locks::MutexExt;
use crate::profiles::ProfileManager;
use crate::DataDir;

use session::{ConnectParams, Ended, SessionCmd, Shared};

/// Órdenes en cola por sesión. La entrada que no quepa se descarta antes que
/// acumular memoria (el estado de teclas se resincroniza al soltar el foco).
const CMD_QUEUE: usize = 256;

struct Handle {
    generation: u64,
    cmds: mpsc::Sender<SessionCmd>,
    shared: Arc<Shared>,
}

/// Sesiones integradas vivas, por `session_id`.
#[derive(Default)]
pub struct EmbeddedRdpManager {
    sessions: Mutex<HashMap<String, Handle>>,
    generation: AtomicU64,
}

impl EmbeddedRdpManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, session_id: &str) -> Option<(mpsc::Sender<SessionCmd>, Arc<Shared>)> {
        self.sessions
            .lock_recover()
            .get(session_id)
            .map(|h| (h.cmds.clone(), h.shared.clone()))
    }

    /// Cierra todas las sesiones (cierre de la ventana).
    pub fn disconnect_all(&self) {
        for (_, h) in self.sessions.lock_recover().drain() {
            let _ = h.cmds.try_send(SessionCmd::Close);
        }
    }
}

/// Estado de la sesión que ve la pestaña (`rdp-embed-status-{id}`).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent {
    /// `connecting` | `connected` | `closed` | `error`.
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u16>,
}

impl StatusEvent {
    fn bare(state: &'static str) -> Self {
        Self {
            state,
            code: None,
            message: None,
            width: None,
            height: None,
        }
    }
}

struct EventReporter {
    app: AppHandle,
    event: String,
}

impl session::Reporter for EventReporter {
    fn connected(&self, width: u16, height: u16) {
        let _ = self.app.emit(
            &self.event,
            StatusEvent {
                width: Some(width),
                height: Some(height),
                ..StatusEvent::bare("connected")
            },
        );
    }
}

/// Base de las descargas del portapapeles: la caché de la app (por usuario).
/// En la build portable, junto a sus datos, para no dejar rastro fuera.
#[must_use]
pub fn clipboard_downloads_base(app: &AppHandle, data_dir: &std::path::Path) -> std::path::PathBuf {
    use tauri::Manager as _;
    let cache = if crate::is_portable() {
        None
    } else {
        app.path().app_cache_dir().ok()
    };
    cache
        .unwrap_or_else(|| data_dir.join("cache"))
        .join("rdp-clipboard")
}

struct ClipEventNotifier {
    app: AppHandle,
    event: String,
}

impl clipboard::ClipNotifier for ClipEventNotifier {
    fn notice(&self, notice: clipboard::ClipNotice) {
        let _ = self.app.emit(&self.event, notice);
    }
}

/// Canales extra de la sesión, según los ajustes del frontend.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedOptions {
    /// Portapapeles compartido (texto).
    #[serde(default)]
    pub clipboard: bool,
    /// Además, ficheros por el portapapeles.
    #[serde(default)]
    pub clipboard_files: bool,
    /// Compartir la carpeta elegida en Ajustes (la ruta la guarda el backend).
    #[serde(default)]
    pub shared_folder: bool,
    /// Perfil de rendimiento (`speed`, `balanced`, `quality`).
    #[serde(default)]
    pub perf_profile: perf::PerfProfile,
}

/// Respuesta de `rdp_embed_connect`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectReply {
    /// No hay contraseña guardada: el frontend la pide y vuelve a llamar con
    /// `password`. No se ha abierto ninguna conexión.
    pub needs_password: bool,
}

fn valid_session_id(id: &str) -> bool {
    id.starts_with("rdp-")
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Abre una sesión RDP integrada para un perfil guardado.
///
/// El frontend pasa **solo** el id del perfil (y la identidad elegida): la
/// contraseña se resuelve aquí (keyring, KeePass, credencial maestra,
/// `${…}`). `password` solo llega cuando no había ninguna guardada y el
/// usuario la acaba de teclear en el diálogo.
///
/// Vuelve en cuanto la sesión está lanzada; el progreso (diálogo del
/// certificado, autenticación, cierre) llega por `rdp-embed-status-{id}` y los
/// fotogramas por `on_frame`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn rdp_embed_connect(
    app: AppHandle,
    manager: State<'_, EmbeddedRdpManager>,
    profile_state: State<'_, ProfileManager>,
    cred_state: State<'_, CredentialStore>,
    jump_pool: State<'_, JumpPool>,
    forwards: State<'_, ForwardRegistry>,
    data_dir: State<'_, DataDir>,
    session_id: String,
    profile_id: String,
    credential_id: Option<String>,
    password: Option<String>,
    jumps: Option<Vec<JumpRequest>>,
    overrides: Option<ConnectOverrides>,
    width: u32,
    height: u32,
    scale: Option<u32>,
    options: Option<EmbedOptions>,
    on_frame: Channel<Response>,
) -> Result<ConnectReply, String> {
    if !valid_session_id(&session_id) {
        return Err("Identificador de sesión no válido".into());
    }
    let profiles = profile_state.load_all().map_err(|e| e.to_string())?;
    let mut profile = profiles
        .iter()
        .find(|p| p.id == profile_id && p.connection_type == "rdp")
        .cloned()
        .ok_or_else(|| format!("Perfil RDP {profile_id} no encontrado"))?;
    if let Some(cid) = credential_id.as_deref() {
        apply_credential(&mut profile, cid)?;
    }
    if let Some(ov) = overrides.as_ref() {
        apply_connect_overrides(&mut profile, ov);
    }
    credentials::substitute_connection_fields(&mut profile, &cred_state);

    let Some(secret) = resolve_connection_secret(
        &profile,
        &profile_id,
        credential_id.as_deref(),
        &cred_state,
        password,
    )?
    else {
        return Ok(ConnectReply {
            needs_password: true,
        });
    };

    let event = event_name(EventKind::RdpEmbedStatus, &session_id);
    let _ = app.emit(&event, StatusEvent::bare("connecting"));

    let forward = external_forward(
        "RDP",
        &profiles,
        &profile,
        Some(secret.as_str()),
        jumps.as_deref(),
        &cred_state,
        &jump_pool,
        &forwards,
    )
    .await?;
    let (dial_host, dial_port) = match forward.as_ref() {
        Some(f) => (f.host().to_string(), f.port()),
        None => (profile.host.clone(), profile.port),
    };

    let options = options.unwrap_or_default();
    let params = ConnectParams {
        host: profile.host.clone(),
        port: profile.port,
        dial_host,
        dial_port,
        via_jump: forward.is_some(),
        username: profile.username.clone(),
        domain: profile.domain.clone(),
        password: secret,
        width,
        height,
        scale,
        cert_store: certs::store_path(&data_dir.0),
        clipboard: options.clipboard.then_some(clipboard::ClipboardOptions {
            files: options.clipboard_files,
        }),
        clipboard_temp: clipboard::session_temp_root(
            &clipboard_downloads_base(&app, &data_dir.0),
            &session_id,
        ),
        clipboard_notifier: Arc::new(ClipEventNotifier {
            app: app.clone(),
            event: event_name(EventKind::RdpEmbedClip, &session_id),
        }),
        shared_folder: if options.shared_folder {
            drive::configured(&data_dir.0)
        } else {
            None
        },
        perf: options.perf_profile,
    };

    let (tx, rx) = mpsc::channel(CMD_QUEUE);
    let shared = Arc::new(Shared::default());
    let generation = manager.generation.fetch_add(1, Ordering::Relaxed);
    // Reconectar reutiliza el id: la sesión anterior, si sigue viva, se cierra.
    if let Some(old) = manager.sessions.lock_recover().insert(
        session_id.clone(),
        Handle {
            generation,
            cmds: tx,
            shared: shared.clone(),
        },
    ) {
        let _ = old.cmds.try_send(SessionCmd::Close);
    }

    let reporter = Arc::new(EventReporter {
        app: app.clone(),
        event: event.clone(),
    });
    let app_task = app.clone();
    tauri::async_runtime::spawn(async move {
        // El reenvío del salto vive lo que la sesión.
        let _forward = forward;
        let outcome = session::run(params, on_frame, rx, shared, reporter).await;
        let status = match outcome {
            Ok(Ended::Local) => StatusEvent::bare("closed"),
            Ok(Ended::Server(reason)) => StatusEvent {
                code: Some("server-closed"),
                message: (!reason.is_empty()).then_some(reason),
                ..StatusEvent::bare("closed")
            },
            Err(err) => {
                log::warn!("RDP integrado {session_id}: {} ({})", err.code, err.message);
                StatusEvent {
                    code: Some(err.code),
                    message: Some(err.message),
                    ..StatusEvent::bare("error")
                }
            }
        };
        let manager = tauri::Manager::state::<EmbeddedRdpManager>(&app_task);
        let mut sessions = manager.sessions.lock_recover();
        if sessions
            .get(&session_id)
            .is_some_and(|h| h.generation == generation)
        {
            sessions.remove(&session_id);
        }
        drop(sessions);
        let _ = app_task.emit(&event, status);
    });

    Ok(ConnectReply {
        needs_password: false,
    })
}

/// Lote de entrada del canvas. Cuerpo crudo (ver `input`), con el id de la
/// sesión en la cabecera `x-session`.
#[tauri::command]
pub fn rdp_embed_input(
    manager: State<'_, EmbeddedRdpManager>,
    request: Request<'_>,
) -> Result<(), String> {
    let session_id = request
        .headers()
        .get("x-session")
        .and_then(|v| v.to_str().ok())
        .ok_or("Falta la sesión")?;
    let InvokeBody::Raw(body) = request.body() else {
        return Err("Se esperaba un cuerpo binario".into());
    };
    let events = input::decode_batch(body).map_err(str::to_string)?;
    if events.is_empty() {
        return Ok(());
    }
    let Some((cmds, _)) = manager.get(session_id) else {
        return Ok(());
    };
    // Cola llena = el servidor no da abasto: se descarta este lote.
    let _ = cmds.try_send(SessionCmd::Input(events));
    Ok(())
}

/// Pide un tamaño nuevo del escritorio (píxeles físicos y escala en %).
#[tauri::command]
pub fn rdp_embed_resize(
    manager: State<'_, EmbeddedRdpManager>,
    session_id: String,
    width: u32,
    height: u32,
    scale: Option<u32>,
) {
    if let Some((cmds, _)) = manager.get(&session_id) {
        let _ = cmds.try_send(SessionCmd::Resize {
            width,
            height,
            scale,
        });
    }
}

/// Confirma que el frontend ya pintó el fotograma `seq`.
#[tauri::command]
pub fn rdp_embed_ack(manager: State<'_, EmbeddedRdpManager>, session_id: String, seq: u32) {
    if let Some((_, shared)) = manager.get(&session_id) {
        shared.acked.store(seq, Ordering::Release);
        shared.ack_notify.notify_one();
    }
}

#[tauri::command]
pub fn rdp_embed_disconnect(manager: State<'_, EmbeddedRdpManager>, session_id: String) {
    if let Some((cmds, _)) = manager.get(&session_id) {
        let _ = cmds.try_send(SessionCmd::Close);
    }
}

/// El escritorio recibió el foco (o va a pegar): si el portapapeles local
/// cambió desde lo último anunciado, se anuncia al remoto.
#[tauri::command]
pub fn rdp_embed_clipboard_sync(manager: State<'_, EmbeddedRdpManager>, session_id: String) {
    if let Some((cmds, _)) = manager.get(&session_id) {
        let _ = cmds.try_send(SessionCmd::ClipboardSync);
    }
}

/// Respuesta al diálogo del certificado (`rdp-cert-prompt`).
#[tauri::command]
pub fn rdp_cert_response(prompt_id: String, accept: bool) -> bool {
    certs::resolve_prompt(&prompt_id, accept)
}

#[tauri::command]
pub fn rdp_known_certs_list(data_dir: State<'_, DataDir>) -> Vec<certs::KnownCert> {
    certs::list(&certs::store_path(&data_dir.0))
}

#[tauri::command]
pub fn rdp_known_cert_remove(
    data_dir: State<'_, DataDir>,
    host_port: String,
) -> Result<bool, String> {
    certs::forget(&certs::store_path(&data_dir.0), &host_port).map_err(|e| e.to_string())
}

/// `true`: esta build trae el cliente integrado.
#[tauri::command]
pub fn rdp_embed_available() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_de_sesion_validos() {
        assert!(valid_session_id("rdp-0f8e2c1a-1b2c-4d5e-8f90-123456789abc"));
        assert!(!valid_session_id("ssh-1"));
        assert!(!valid_session_id("rdp-../x"));
        assert!(!valid_session_id(&format!("rdp-{}", "a".repeat(80))));
    }
}
