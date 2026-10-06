fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "runtime_info",
                "get_settings",
                "set_settings",
                "ua_options",
                "open_add_dialog",
                "close_add_dialog",
                "add_account",
                "remove_account",
                "rename_account",
                "switch_account",
                "show_grid",
                "open_chat",
                "reload_session",
                "clear_session_data",
                "test_notification",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
