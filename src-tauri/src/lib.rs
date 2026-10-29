mod commands;
mod errors;
mod sessions;
mod transport;

use sessions::SessionManager;
use tracing_subscriber::EnvFilter;

fn init_logging() {
    // Default: our own info logs + warnings from dependencies. Override with
    // RUST_LOG (e.g. `RUST_LOG=netforge_lib=debug`). Never log terminal content or
    // secrets: dependencies at `trace` level could, so keep them at `warn` here.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,netforge_lib=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let result = tauri::Builder::default()
        .manage(SessionManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::session_open_ssh,
            commands::session_write,
            commands::session_resize,
            commands::session_ack,
            commands::session_close,
            commands::session_answer_host_key,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("application error: {e}");
        std::process::exit(1);
    }
}
