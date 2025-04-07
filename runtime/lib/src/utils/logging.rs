use std::fmt::Display;
pub(crate) use common::utils::comma_separated;

pub(crate) fn init_logging() {
    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
    use tracing_tree::{HierarchicalLayer, time::FormatTime};

    const ENV_LOG: &str = "LEAF_LOG";
    const ENV_WRITE_STYLE: &str = "LEAF_LOG_STYLE";
    const ENV_FLAME_GRAPH: &str = "FLAME_OUTPUT";

    let env = std::env::var(ENV_LOG).unwrap_or_default();
    let flame_graph_output =
        std::env::var(ENV_FLAME_GRAPH).unwrap_or_else(|_| "tracing.folded".into());

    #[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
    struct ChronoLocalDateTime;

    impl FormatTime for ChronoLocalDateTime {
        fn format_time(&self, w: &mut impl std::fmt::Write) -> std::fmt::Result {
            let time = chrono::Local::now();
            write!(w, "{}", time.format("%Y-%m-%d %H:%M:%S"))
        }

        fn style_timestamp(
            &self,
            _ansi: bool,
            _elapsed: std::time::Duration,
            _w: &mut impl std::fmt::Write,
        ) -> std::fmt::Result {
            Ok(())
        }
    }

    let fmt_layer = HierarchicalLayer::default()
        .with_ansi(std::env::var(ENV_WRITE_STYLE).map_or(true, |val| val != "false"))
        .with_writer(std::io::stderr)
        .with_thread_names(false)
        .with_bracketed_fields(false)
        .with_verbose_exit(false)
        .with_verbose_entry(false)
        .with_timer(ChronoLocalDateTime)
        .with_deferred_spans(false)
        .with_targets(true);

    let default_directive = format!("pri=off,z3=off,{}", env);
    let env_filter = EnvFilter::builder().parse_lossy(default_directive);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    #[cfg(feature = "profile_flame")]
    let subscriber = {
        let (flame_layer, _guard) =
            tracing_flame::FlameLayer::with_file(flame_graph_output).unwrap();
        subscriber.with(flame_layer)
    };

    #[cfg(feature = "profile_tracy")]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());

    subscriber.try_init().unwrap();
}
