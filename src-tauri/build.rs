const APP_COMMANDS: &[&str] = &[
    "get_static_info",
    "get_snapshot",
    "set_detail_enabled",
    "get_settings",
    "set_settings",
    "hide_popover",
    "resize_popover",
    "quit_app",
    "open_login_items_settings",
    "get_update_state",
    "check_for_updates",
    "install_update",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("tauri-build failed");
}
