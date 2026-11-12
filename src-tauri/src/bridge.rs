//! Tauri side of the browser hand-off: the confirmation dialog window and the pipe request handler.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use openidm_core::manager::Manager;
use openidm_core::protocol::{request_id, Request, Response};
use openidm_core::winutil::pipe_name;
use tauri::{AppHandle, Emitter, Manager as _, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::capture::{self, Choice, Prompter, Suggested, UiHooks};
use crate::{pipe, tray};

type Answer = (Choice, Option<String>);

struct PendingConfirm {
    suggested: Suggested,
    answer: oneshot::Sender<Answer>,
}

/// Confirmation dialogs waiting for the user, keyed by dialog key.
#[derive(Default)]
pub struct Confirms(Mutex<HashMap<String, PendingConfirm>>);

impl Confirms {
    fn take(&self, key: &str) -> Option<PendingConfirm> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(key)
    }
}

fn label(key: &str) -> String {
    format!("confirm-{key}")
}

/// Dropping a dialog window without choosing counts as "Batal".
pub fn on_confirm_window_gone(app: &AppHandle, window_label: &str) {
    if let Some(key) = window_label.strip_prefix("confirm-") {
        app.state::<Confirms>().take(key);
    }
}

struct DialogPrompter {
    app: AppHandle,
    /// Cancelled when the browser side of this request goes away.
    closed: CancellationToken,
}

impl Prompter for DialogPrompter {
    async fn ask(&self, key: &str, suggested: &Suggested) -> Answer {
        let (tx, rx) = oneshot::channel();
        let confirms = self.app.state::<Confirms>();
        confirms
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), PendingConfirm { suggested: suggested.clone(), answer: tx });

        let window = WebviewWindowBuilder::new(&self.app, label(key), WebviewUrl::App(format!("confirm.html?key={key}").into()))
            .title("OpenIDM - Unduhan baru")
            .inner_size(520.0, 372.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .always_on_top(true)
            .center()
            .focused(true)
            .build();
        let Ok(window) = window else {
            confirms.take(key);
            return (Choice::Cancel, None);
        };
        let _ = window.set_focus();

        let answer = tokio::select! {
            answer = rx => answer.ok(),
            _ = self.closed.cancelled() => None,
        };
        confirms.take(key);
        let _ = window.close();
        answer.unwrap_or((Choice::Cancel, None))
    }
}

struct WindowUi(AppHandle);

impl UiHooks for WindowUi {
    fn show_main(&self, add_url: Option<String>) {
        tray::show_main(&self.0);
        if let Some(url) = add_url {
            let _ = self.0.emit_to("main", "openidm://add-url", url);
        }
    }
}

async fn handle_frame(app: AppHandle, m: Arc<Manager>, frame: Vec<u8>, closed: CancellationToken) -> Option<Vec<u8>> {
    let response = match serde_json::from_slice::<Request>(&frame) {
        Ok(request) => {
            let prompter = DialogPrompter { app: app.clone(), closed };
            capture::handle(request, &m, &prompter, &WindowUi(app)).await
        }
        // Unknown or malformed: answer when the sender can be told, otherwise ignore.
        Err(_) => Response::error(request_id(&frame)?, "Permintaan tidak dikenal."),
    };
    serde_json::to_vec(&response).ok()
}

/// Listens for the Native Messaging host for as long as the app runs.
pub async fn serve(app: AppHandle, m: Arc<Manager>) {
    let Ok(name) = pipe_name() else { return };
    let _ = pipe::serve_on(name, move |frame, closed| handle_frame(app.clone(), m.clone(), frame, closed)).await;
}

#[tauri::command]
pub fn confirm_get(confirms: State<Confirms>, key: String) -> Result<Suggested, String> {
    confirms
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&key)
        .map(|pending| pending.suggested.clone())
        .ok_or_else(|| "Permintaan unduhan ini sudah tidak berlaku.".to_string())
}

#[tauri::command]
pub fn confirm_decide(
    window: WebviewWindow,
    confirms: State<Confirms>,
    key: String,
    choice: String,
    filename: Option<String>,
) -> Result<(), String> {
    // A dialog may only answer its own request.
    if window.label() != label(&key) {
        return Err("Permintaan tidak dikenal.".into());
    }
    let choice = match choice.as_str() {
        "start" => Choice::Start,
        "later" => Choice::Later,
        "cancel" => Choice::Cancel,
        _ => return Err("Perintah tidak dikenal.".into()),
    };
    let filename = filename.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
    if let Some(pending) = confirms.take(&key) {
        let _ = pending.answer.send((choice, filename));
    }
    Ok(())
}
