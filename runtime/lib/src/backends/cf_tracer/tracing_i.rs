use std::sync::OnceLock;

use tracing::Level;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{
    Layer,
    filter::{FilterExt, Targets},
    layer::Filter,
};

use crate::utils::logging::LeafTracingSubLayerFactory;

pub(super) const TARGET: &str = "__program";
pub(super) const LEVEL: Level = Level::INFO;

static WRITER: OnceLock<(NonBlocking, WorkerGuard)> = OnceLock::new();

pub(super) struct LayerFactory;

impl LeafTracingSubLayerFactory for LayerFactory {
    fn layer() -> impl Layer<tracing_subscriber::Registry> + Sized + Send + Sync + 'static {
        let writer = WRITER
            .get_or_init(|| tracing_appender::non_blocking(std::io::stdout()))
            .0
            .clone();

        let mut layer = json_subscriber::fmt::layer();
        layer
            .inner_layer_mut()
            .add_dynamic_field("name", |e, _s| Some(e.metadata().name()));
        layer
            .with_span_list(false)
            .with_current_span(false)
            .with_target(false)
            .with_thread_ids(true)
            .with_writer(writer)
            .with_filter(Targets::new().with_target(TARGET, LEVEL))
    }

    fn filter_rest<S: 'static>() -> impl Filter<S> + Sized + Send + Sync + 'static {
        Targets::new().with_target(TARGET, LEVEL).not()
    }
}
