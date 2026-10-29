const COMMANDS: &[&str] = &[
    "session_open_ssh",
    "session_write",
    "session_resize",
    "session_ack",
    "session_close",
    "session_answer_host_key",
    "session_open_profile",
    "library_list",
    "profile_create",
    "profile_update",
    "profile_delete",
    "profile_save_secret",
    "folder_create",
    "folder_rename",
    "folder_delete",
    "known_hosts_replace",
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
