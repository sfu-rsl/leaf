use crate::backends::basic::{
    concrete::Concretizer, config::SymbolicPlaceStrategy, state::SymPlaceHandler,
};

use super::{ConcreteValueRef, SymValueRef, ValueRef};
use common::{log_debug, log_info};

pub(crate) fn make_sym_place_handler(
    config: SymbolicPlaceStrategy,
    concretizer_factory: impl FnOnce() -> Box<dyn Concretizer>,
) -> Box<
    dyn SymPlaceHandler<SymEntity = SymValueRef, ConcEntity = ConcreteValueRef, Entity = ValueRef>,
> {
    log_debug!(
        "Creating a symbolic place handler for strategy {:?}",
        config
    );
    use SymbolicPlaceStrategy::*;
    match config {
        Panic => Box::new(PanicSymPlaceHandler),
        ProjExpression => Box::new(ProjExprSymPlaceHandler),
        Concretization => Box::new(ConcretizerSymPlaceHandler),
        Stamping => Box::new(StamperSymPlaceHandler {
            concretizer: concretizer_factory(),
        }),
    }
}

struct PanicSymPlaceHandler;
impl SymPlaceHandler for PanicSymPlaceHandler {
    type Entity = ValueRef;

    fn handle<'a>(
        &mut self,
        sym_value: Self::SymEntity,
        _get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        panic!("Faced symbolic place: {:?}", sym_value)
    }
}

struct ProjExprSymPlaceHandler;
impl SymPlaceHandler for ProjExprSymPlaceHandler {
    type Entity = ValueRef;

    fn handle<'a>(
        &mut self,
        sym_value: Self::SymEntity,
        _get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        sym_value.into()
    }
}

struct ConcretizerSymPlaceHandler;
impl SymPlaceHandler for ConcretizerSymPlaceHandler {
    type Entity = ValueRef;

    fn handle<'a>(
        &mut self,
        sym_value: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        log_info!("Concretizing symbolic value: {}", sym_value);
        get_conc(&sym_value).into()
    }
}

struct StamperSymPlaceHandler {
    concretizer: Box<dyn Concretizer>,
}
impl SymPlaceHandler for StamperSymPlaceHandler {
    type Entity = ValueRef;

    fn handle<'a>(
        &mut self,
        sym_value: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        let conc_value = get_conc(&sym_value);

        log_info!("Stamping {} == {}", sym_value, conc_value,);

        self.concretizer
            .stamp(sym_value.clone(), conc_value.clone());

        ConcretizerSymPlaceHandler.handle(sym_value, Box::new(|_| conc_value))
    }
}
