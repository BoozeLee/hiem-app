mod agent;
mod commands;
mod credentials;
mod jwt_auth;
mod installation;

/// Initialise tracing & LangSmith streaming.
///
/// Loads the `.env` file from the project root (lazily, no error if absent)
/// so `LANGSMITH_API_KEY`, `LANGSMITH_PROJECT`, etc. are available before
/// `langsmith-rust::init()` is called.
///
/// `langsmith-rust` reads those env vars automatically and begins streaming
/// `tracing` spans to LangSmith. A `FmtSubscriber` fallback is registered so
/// spans are visible in local log output too.
fn init_tracing() {
    let _ = dotenvy::dotenv();
    let _ = langsmith_rust::init();

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "hiem=info,warn".to_string()),
        )
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    eprintln!("[hiem] keyring ready: {}", crate::installation::has_keyring_credentials());
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ── OAuth / API ───────────────────────────────────────────────────
            commands::get_device_code,
            commands::poll_token,
            commands::get_repos,
            commands::get_prs,
            commands::get_issues,
            commands::get_branches,
            commands::whoami,
            commands::get_gh_token,
            // ── GitHub App credential store ───────────────────────────────────
            commands::store_github_app_key,
            commands::list_keyring_entries,
            commands::whoami_with_installation_token,
            // ── chat ──────────────────────────────────────────────────────────
            commands::chat,
            // ── browser / clipboard ────────────────────────────────────────────
            commands::open_url,
            commands::copy_to_clipboard
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
