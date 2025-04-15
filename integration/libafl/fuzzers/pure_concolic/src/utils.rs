use tracing_log::LogTracer;
use tracing_subscriber::fmt::format::FmtSpan;

const LOG_ENV: &str = "LEAFF_LOG";

pub fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
    let env_filter = EnvFilter::builder()
        .with_env_var(LOG_ENV.to_string())
        .from_env_lossy();

    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::ENTER);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    let _ = LogTracer::init();
}
