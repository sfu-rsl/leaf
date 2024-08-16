use crate::{
    abs::ValueType,
    backends::basic::{
        concrete::Concretizer, config::SymbolicPlaceStrategy, place::PlaceMetadata,
        state::SymPlaceHandler, ValueRef,
    },
};

use super::{create_lazy, ConcreteValueRef, SymValueRef};
use common::log_debug;

pub(crate) fn make_sym_place_handler(
    config: SymbolicPlaceStrategy,
    concretizer_factory: impl FnOnce() -> Box<dyn Concretizer>,
) -> Box<dyn SymPlaceHandler<PlaceMetadata, SymPlaceValue = SymValueRef, PlaceValue = ValueRef>> {
    log_debug!(
        "Creating a symbolic place handler for strategy {:?}",
        config
    );
    use SymbolicPlaceStrategy::*;
    match config {
        ProjExpression => Box::new(ProjExprSymPlaceHandler),
        Concretization => Box::new(ConcretizerSymPlaceHandler),
        Stamping => Box::new(StamperSymPlaceHandler {
            concretizer: concretizer_factory(),
        }),
    }
}

struct ProjExprSymPlaceHandler;
impl<M> SymPlaceHandler<M> for ProjExprSymPlaceHandler {
    type SymPlaceValue = SymValueRef;
    type PlaceValue = ValueRef;

    fn handle(&mut self, place_value: SymValueRef, _metadata: &M) -> ValueRef {
        // NOTE: Later we might want to handle more parts here.
        place_value.into()
    }
}

struct ConcretizerSymPlaceHandler;
impl SymPlaceHandler<PlaceMetadata> for ConcretizerSymPlaceHandler {
    type SymPlaceValue = SymValueRef;
    type PlaceValue = ValueRef;

    fn handle(&mut self, place_value: SymValueRef, place_meta: &PlaceMetadata) -> ValueRef {
        let conc_val = get_concrete_value(&place_meta, place_value.as_ref().try_into().ok());
        log_debug!(
            "Concretizing symbolic value {} with value {}",
            place_value,
            conc_val,
        );
        conc_val
    }
}

struct StamperSymPlaceHandler {
    concretizer: Box<dyn Concretizer>,
}
impl SymPlaceHandler<PlaceMetadata> for StamperSymPlaceHandler {
    type SymPlaceValue = SymValueRef;
    type PlaceValue = ValueRef;

    fn handle(&mut self, place_value: SymValueRef, place_meta: &PlaceMetadata) -> ValueRef {
        let conc_val = get_concrete_value(&place_meta, place_value.as_ref().try_into().ok());
        log_debug!(
            "Stamping symbolic value {} with concrete value {}",
            place_value,
            conc_val
        );
        self.concretizer
            .stamp(place_value, ConcreteValueRef::new(conc_val.clone()));
        conc_val
    }
}

fn get_concrete_value(place_meta: &PlaceMetadata, ty: Option<ValueType>) -> ValueRef {
    let addr = place_meta.address();
    let ty = ty.or_else(|| place_meta.ty().cloned());
    create_lazy(addr, ty)
}
