fn main() {
    // The app's own commands, so capabilities can grant them to pages
    // (capabilities/default.json): the app's own pages call them.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
        "daemon_status",
        "retry",
        "compat",
        "compat_fix",
        "compat_skip",
        "cloud_status",
        "cloud_signin",
        "cloud_local",
    ])))
    .expect("tauri-build");
}
