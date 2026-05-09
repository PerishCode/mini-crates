use tracing_subscriber::{fmt, EnvFilter};

pub fn init_tracing(service_name: &'static str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("mini_crates_api=info,tower_http=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_writer(std::io::stderr)
        .init();
    tracing::info!(service = service_name, "tracing initialized");
}
