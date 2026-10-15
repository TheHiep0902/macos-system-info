use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Runtime, State};

use crate::model::{Snapshot, StaticInfo};
use crate::popover;
use crate::sampler::{SharedState, Sink};
use crate::settings::{self, SettingsPayload, SettingsStore};
use crate::tray::TRAY_ID;

pub const POPOVER_LABEL: &str = "popover";
pub const EVENT_SNAPSHOT: &str = "metrics://snapshot";
pub const EVENT_DISKS_CHANGED: &str = "disks://changed";
pub const EVENT_POPOVER_VISIBILITY: &str = "popover://visibility";

const SETTINGS_SAVE_FAILED: &str = "settings could not be saved";

pub struct AppState(pub Arc<SharedState>);

#[tauri::command]
pub fn get_static_info(state: State<'_, AppState>) -> Option<StaticInfo> {
    state.0.static_info()
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Option<Snapshot> {
    state.0.snapshot()
}

#[tauri::command]
pub fn set_detail_enabled(state: State<'_, AppState>, enabled: bool) {
    state.0.detail_enabled.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
pub fn get_settings<R: Runtime>(
    app: AppHandle<R>,
    store: State<'_, SettingsStore>,
) -> SettingsPayload {
    SettingsPayload {
        settings: store.get(),
        launch_at_login: settings::launch_at_login(&app),
    }
}

#[tauri::command]
pub fn set_settings<R: Runtime>(
    app: AppHandle<R>,
    store: State<'_, SettingsStore>,
    settings: SettingsPayload,
) -> Result<SettingsPayload, String> {
    let previous = store.get();
    let next = settings.settings.normalized();
    store.save(next.clone()).map_err(|err| {
        log::warn!("could not save settings: {err}");
        SETTINGS_SAVE_FAILED.to_owned()
    })?;
    settings::apply(&app, &previous, &next);
    let launch_at_login = settings::set_launch_at_login(&app, settings.launch_at_login);
    Ok(SettingsPayload {
        settings: store.get(),
        launch_at_login,
    })
}

#[tauri::command]
pub fn hide_popover<R: Runtime>(app: AppHandle<R>) {
    popover::hide(&app, true);
}

#[tauri::command]
pub fn resize_popover<R: Runtime>(app: AppHandle<R>, height: u32) {
    popover::resize(&app, height);
}

#[tauri::command]
pub fn quit_app<R: Runtime>(app: AppHandle<R>) {
    app.exit(0);
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
