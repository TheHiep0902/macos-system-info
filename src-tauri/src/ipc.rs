use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Runtime, State};

use crate::model::{Snapshot, StaticInfo};
use crate::sampler::{SharedState, Sink};
use crate::tray::TRAY_ID;

pub const POPOVER_LABEL: &str = "popover";
pub const EVENT_SNAPSHOT: &str = "metrics://snapshot";
pub const EVENT_DISKS_CHANGED: &str = "disks://changed";

pub struct AppState(pub Arc<SharedState>);

#[tauri::command]
pub fn get_static_info(state: State<'_, AppState>) -> Option<StaticInfo> {
    state.0.static_info()
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Option<Snapshot> {
    state.0.snapshot()
}

pub struct TauriSink<R: Runtime> {
    app: AppHandle<R>,
    title_failed: AtomicBool,
    emit_failed: AtomicBool,
}

impl<R: Runtime> TauriSink<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        TauriSink {
            app,
            title_failed: AtomicBool::new(false),
            emit_failed: AtomicBool::new(false),
        }
    }

    fn report(&self, flag: &AtomicBool, result: Result<(), String>, what: &str) {
        match result {
            Ok(()) => {
                if flag.swap(false, Ordering::Relaxed) {
                    log::info!("{what} recovered");
                }
            }
            Err(err) => {
                if !flag.swap(true, Ordering::Relaxed) {
                    log::warn!("{what} failed: {err}");
                }
            }
        }
    }

    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        let result = self
            .app
            .emit_to(POPOVER_LABEL, event, payload)
            .map_err(|err| err.to_string());
        self.report(&self.emit_failed, result, "event emit");
    }
}

impl<R: Runtime> Sink for TauriSink<R> {
    fn set_title(&self, title: &str) {
        let result = match self.app.tray_by_id(TRAY_ID) {
            Some(tray) => tray.set_title(Some(title)).map_err(|err| err.to_string()),
            None => Err(format!("tray {TRAY_ID} not found")),
        };
        self.report(&self.title_failed, result, "tray title update");
    }

    fn emit_snapshot(&self, snapshot: &Snapshot) {
        self.emit(EVENT_SNAPSHOT, snapshot);
    }

    fn emit_disks_changed(&self) {
        self.emit(EVENT_DISKS_CHANGED, ());
    }
}
