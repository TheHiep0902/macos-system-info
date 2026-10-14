#[cfg(target_arch = "x86_64")]
pub mod ioreport;
pub mod ipc;
pub mod model;
pub mod platform;
pub mod popover;
pub mod sampler;
pub mod settings;
#[cfg(target_arch = "x86_64")]
pub mod smc;
pub mod sys;
pub mod topology;
pub mod tray;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, Thread};
use std::time::Duration;

use tauri::tray::TrayIcon;
use tauri::{ActivationPolicy, AppHandle, Manager, RunEvent, Runtime};
use tauri_plugin_autostart::MacosLauncher;

use crate::ipc::{AppState, TauriSink};
use crate::popover::{Anchor, PopoverState};
use crate::sampler::clock::SystemClock;
use crate::sampler::{HostBackendFactory, SharedState};
use crate::settings::SettingsStore;

const SAMPLER_STOP_TIMEOUT: Duration = Duration::from_millis(500);

struct TrayHandle<R: Runtime> {
    _tray: TrayIcon<R>,
}

struct SamplerHandle {
    thread: Thread,
    finished: Receiver<()>,
}

impl SamplerHandle {
    fn stop(&self, shared: &SharedState) {
        shared.request_shutdown();
        self.thread.unpark();
        if self.finished.recv_timeout(SAMPLER_STOP_TIMEOUT).is_err() {
            log::debug!(
                "sampler still busy after {SAMPLER_STOP_TIMEOUT:?}; exiting without waiting"
            );
        }
    }
}

fn spawn_sampler<R: Runtime>(
    app: &AppHandle<R>,
    shared: Arc<SharedState>,
) -> Option<SamplerHandle> {
    let sink = TauriSink::new(app.clone());
    let (done, finished) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            sampler::supervise(&shared, &sink, &SystemClock, &HostBackendFactory);
            done.send(()).ok();
        });
    match spawned {
        Ok(handle) => Some(SamplerHandle {
            thread: handle.thread().clone(),
            finished,
        }),
        Err(err) => {
            log::error!("could not start sampler thread: {err}");
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let shared = Arc::new(SharedState::default());
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let handle = app.clone();
            let scheduled =
                app.run_on_main_thread(move || popover::show(&handle, Anchor::Tray, None));
            if let Err(err) = scheduled {
                log::warn!("could not open the popover for a second launch: {err}");
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState(Arc::clone(&shared)))
        .manage(PopoverState::default())
        .invoke_handler(tauri::generate_handler![
            ipc::get_static_info,
            ipc::get_snapshot,
            ipc::set_detail_enabled,
            ipc::get_settings,
            ipc::set_settings,
            ipc::hide_popover,
            ipc::resize_popover,
            ipc::quit_app
        ])
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            let store = SettingsStore::load(app.path().app_config_dir().ok());
            let current = store.get();
            settings::apply_shared(&app.state::<AppState>().0, &current);
            app.manage(store);
            let handle = app.handle();
            let (tray, menu) = tray::create(
                handle,
                current.show_tray_icon,
                settings::launch_at_login(handle),
            )?;
            app.manage(TrayHandle { _tray: tray });
            app.manage(menu);
            if !current.memory_saver
                && let Err(err) = popover::create(handle, &current)
            {
                log::warn!("could not pre-create the popover window: {err}");
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Pulsebar application");

    let mut sampler: Option<SamplerHandle> = None;
    app.run(move |handle, event| match event {
        RunEvent::Ready => {
            if sampler.is_none() {
                sampler = spawn_sampler(handle, Arc::clone(&shared));
            }
        }
        RunEvent::Reopen { .. } => popover::show(handle, Anchor::Tray, None),
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        RunEvent::Exit => {
            handle.state::<PopoverState>().shutdown();
            if let Some(running) = sampler.take() {
                running.stop(&shared);
            }
        }
        _ => {}
    });
}
