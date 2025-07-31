use crate::backends::basic::{
    concrete::{ConcolicValueObtainer, Concretizer},
    config::SymbolicPlaceStrategy,
    state::{SymPlaceHandler, SymPlaceSymEntity, ValueUsageInPlace},
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
        ProjExpression => Box::new(ProjExprSymPlaceHandler {
            size_handler: StamperSymPlaceHandler {
                concretizer: concretizer_factory(),
            },
        }),
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

struct ProjExprSymPlaceHandler<H> {
    size_handler: H,
}
impl<H> SymPlaceHandler for ProjExprSymPlaceHandler<H>
where
    H: SymPlaceHandler<SymEntity = SymPlaceSymEntity, Entity = ValueRef>,
    ValueRef: From<H::ConcEntity>,
{
    type ConcEntity = H::ConcEntity;
    type Entity = ValueRef;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity {
        match sym_entity.kind {
            ValueUsageInPlace::Size => {
                log_info!(
                    "Symbolic size observed: {}, which is out of the scope of support for this backend",
                    sym_entity.value
                );
                self.size_handler.handle(sym_entity, get_conc)
            }
            ValueUsageInPlace::Deref | ValueUsageInPlace::Index => sym_entity.into(),
        }
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
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity {
        let conc_value = self.concretizer.stamp(sym_entity.clone(), get_conc);
        ConcretizerSymPlaceHandler.handle(sym_entity, Box::new(|| conc_value))
    }
}

pub(in super::super) struct DerefBypassSymPlaceHandler<H>(pub H);
impl<H> SymPlaceHandler for DerefBypassSymPlaceHandler<H>
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
        if let ValueUsageInPlace::Deref = sym_entity.kind {
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
