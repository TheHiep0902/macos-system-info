#[cfg(target_arch = "x86_64")]
pub mod ioreport;
pub mod ipc;
pub mod model;
pub mod platform;
pub mod sampler;
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
use crate::sampler::clock::SystemClock;
use crate::sampler::{HostBackendFactory, SharedState};

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
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState(Arc::clone(&shared)))
        .invoke_handler(tauri::generate_handler![
            ipc::get_static_info,
            ipc::get_snapshot
        ])
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            let tray = tray::create(app.handle())?;
            app.manage(TrayHandle { _tray: tray });
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
        RunEvent::Exit => {
            if let Some(running) = sampler.take() {
                running.stop(&shared);
            }
        }
        _ => {}
    });
}
