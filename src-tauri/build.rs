const COMMANDS: &[&str] = &[
    "session_open_ssh",
    "session_write",
    "session_resize",
    "session_ack",
    "session_close",
    "session_answer_host_key",
    "session_open_profile",
    "session_open_telnet",
    "session_open_serial",
    "serial_ports",
    "library_list",
    "profile_create",
    "profile_update",
    "profile_delete",
    "profile_save_secret",
    "folder_create",
    "folder_rename",
    "folder_delete",
    "profile_move",
    "folder_move",
    "known_hosts_replace",
    "session_send_sequence",
    "buttons_list",
    "button_create",
    "button_update",
    "button_delete",
    "settings_get",
    "settings_update",
    "sftp_list",
    "sftp_mkdir",
    "sftp_download",
    "sftp_upload",
    "transfer_cancel",
];

fn main() {
    // Declaring the app commands makes Tauri generate one permission per command
    // (`allow-session-write`...). They must then be granted explicitly in
    // `capabilities/`, instead of every command being callable by default.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    // Build script: panicking is the standard way to fail the build (never runs
    // in the shipped application).
    .expect("failed to run tauri-build");
}
