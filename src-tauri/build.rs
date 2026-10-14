const APP_COMMANDS: &[&str] = &["get_static_info", "get_snapshot"];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("tauri-build failed");
}
