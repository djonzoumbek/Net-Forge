mod commands;
mod errors;
mod forwarding;
mod persistence;
mod platform;
mod security;
mod sessions;
mod transfers;
mod transport;

use std::path::PathBuf;
use std::sync::Arc;

use persistence::Database;
use security::known_hosts::KnownHosts;
use security::secrets::Secrets;
use sessions::SessionManager;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

fn init_logging() {
    // Default: our own info logs + warnings from dependencies. Override with
    // RUST_LOG (e.g. `RUST_LOG=netforge_lib=debug`). Never log terminal content or
    // secrets: dependencies at `trace` level could, so keep them at `warn` here.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,netforge_lib=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// Opens the database and known_hosts in the app data directory
/// (Linux: `~/.local/share/dev.kamis.netforge/`, mode 0700; Windows:
/// `%APPDATA%\dev.kamis.netforge\`).
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    // Also when the directory already exists (WebKit may have created it first).
    platform::restrict(&data_dir, true)?;
    tracing::info!(dir = %data_dir.display(), "data directory");

    let database = Database::open(&data_dir.join("netforge.db"))?;
    // `~/.ssh/known_hosts` on Linux, `%USERPROFILE%\.ssh\known_hosts` on Windows.
    let user_known_hosts: Option<PathBuf> = app
        .path()
        .home_dir()
        .ok()
        .map(|home| home.join(".ssh").join("known_hosts"));
    let known_hosts = KnownHosts::new(data_dir.join("known_hosts"), user_known_hosts);

    app.manage(database);
    app.manage(Arc::new(known_hosts));
    app.manage(Arc::new(Secrets::system()));
    app.manage(SessionManager::default());
    app.manage(transfers::TransferManager::default());
    create_main_window(app)?;
    Ok(())
}

/// The window is described in tauri.conf.json (`"create": false`) but built here:
/// clipboard access can only be enabled from Rust. Without it, WebKitGTK refuses
/// `navigator.clipboard.readText()`, so Ctrl+Shift+V and right-click paste fail.
/// Acceptable here: the page only runs our own code (strict CSP, no remote content).
fn create_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or("fenêtre \"main\" absente de tauri.conf.json")?
        .clone();
    tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?
        .enable_clipboard_access()
        .build()?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let result = tauri::Builder::default()
        // Native open/save dialogs (SFTP: file to upload, where to save a download).
        .plugin(tauri_plugin_dialog::init())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::sessions::session_open_ssh,
            commands::sessions::session_open_profile,
            commands::sessions::session_open_telnet,
            commands::sessions::session_open_serial,
            commands::sessions::serial_ports,
            commands::sessions::session_write,
            commands::sessions::session_send_sequence,
            commands::sessions::session_resize,
            commands::sessions::session_ack,
            commands::sessions::session_close,
            commands::sessions::session_answer_host_key,
            commands::forwards::forward_start,
            commands::forwards::forward_stop,
            commands::forwards::forward_list,
            commands::profiles::library_list,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_delete,
            commands::profiles::profile_save_secret,
            commands::profiles::folder_create,
            commands::profiles::folder_rename,
            commands::profiles::folder_delete,
            commands::profiles::profile_move,
            commands::profiles::folder_move,
            commands::known_hosts::known_hosts_replace,
            commands::buttons::buttons_list,
            commands::buttons::button_create,
            commands::buttons::button_update,
            commands::buttons::button_delete,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::transfers::sftp_list,
            commands::transfers::sftp_mkdir,
            commands::transfers::sftp_download,
            commands::transfers::sftp_upload,
            commands::transfers::transfer_cancel,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("application error: {e}");
        std::process::exit(1);
    }
}
