mod commands;
mod errors;
mod persistence;
mod security;
mod sessions;
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
/// (`~/.local/share/dev.kamis.netforge/`), restricted to mode 0700.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    // Also when the directory already exists (WebKit may have created it first).
    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
    tracing::info!(dir = %data_dir.display(), "data directory");

    let database = Database::open(&data_dir.join("netforge.db"))?;
    let user_known_hosts =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ssh/known_hosts"));
    let known_hosts = KnownHosts::new(data_dir.join("known_hosts"), user_known_hosts);

    app.manage(database);
    app.manage(Arc::new(known_hosts));
    app.manage(Arc::new(Secrets::system()));
    app.manage(SessionManager::default());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let result = tauri::Builder::default()
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::sessions::session_open_ssh,
            commands::sessions::session_open_profile,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_ack,
            commands::sessions::session_close,
            commands::sessions::session_answer_host_key,
            commands::profiles::library_list,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_delete,
            commands::profiles::profile_save_secret,
            commands::profiles::folder_create,
            commands::profiles::folder_rename,
            commands::profiles::folder_delete,
            commands::known_hosts::known_hosts_replace,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("application error: {e}");
        std::process::exit(1);
    }
}
