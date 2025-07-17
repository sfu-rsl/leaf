use crate::backends::basic::{
    concrete::{ConcolicValueObtainer, Concretizer},
    config::SymbolicPlaceStrategy,
    state::{SymPlaceHandler, SymPlaceSymEntity},
};

use super::{ConcreteValueRef, ValueRef};
use common::{log_debug, log_info};

pub(crate) fn make_sym_place_handler(
    config: SymbolicPlaceStrategy,
    concretizer_factory: impl FnOnce() -> Box<dyn Concretizer>,
) -> Box<
    dyn SymPlaceHandler<
            SymEntity = SymPlaceSymEntity,
            ConcEntity = ConcreteValueRef,
            Entity = ValueRef,
        >,
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
        _get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
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
        _get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
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
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity {
        log_info!("Concretizing symbolic value: {}", sym_value.value);
        get_conc().into()
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
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity {
        let conc_value = self.concretizer.stamp(sym_value.clone(), get_conc);
        ConcretizerSymPlaceHandler.handle(sym_value, Box::new(|| conc_value))
    }
}

pub(in super::super) struct IndexOnlySymPlaceHandler<H>(pub H);
impl<H> SymPlaceHandler for IndexOnlySymPlaceHandler<H>
where
    H: SymPlaceHandler<SymEntity = SymPlaceSymEntity>,
{
    type SymEntity = SymPlaceSymEntity;
    type ConcEntity = H::ConcEntity;
    type Entity = H::Entity;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity {
        if !sym_entity.is_index {
            sym_entity.into()
        } else {
            self.0.handle(sym_entity, get_conc)
        }
    }
}

impl From<SymPlaceSymEntity> for ValueRef {
    #[inline]
    fn from(value: SymPlaceSymEntity) -> Self {
        value.value.into()
    }
}
