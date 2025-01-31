const LOG_ENV: &str = "LEAFO_LOG";

pub fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
    let env_filter = EnvFilter::builder()
        .with_env_var(LOG_ENV.to_string())
        .from_env_lossy();

    let fmt_layer = fmt::layer().with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
