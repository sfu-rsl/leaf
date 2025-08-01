use derive_more as dm;

use crate::backends::basic as backend;
use backend::{
    concrete::ConcolicValueObtainer,
    expr::prelude::{ConcreteValueRef, SymValueRef, ValueRef},
};

#[derive(Debug, dm::Deref)]
pub(crate) struct SymPlaceSymEntity {
    #[deref]
    value: SymValueRef,
    kind: ValueUsageInPlace,
}

#[derive(Debug)]
enum ValueUsageInPlace {
    Deref,
    Index,
    Size,
}

pub(crate) trait SymPlaceHandler {
    type SymEntity = SymPlaceSymEntity;
    type ConcEntity = ConcreteValueRef;
    type Entity: From<Self::SymEntity> + From<Self::ConcEntity>;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity;
}

impl SymPlaceSymEntity {
    pub(crate) fn of_index(value: SymValueRef) -> Self {
        Self {
            value,
            kind: ValueUsageInPlace::Index,
        }
    }

    pub(crate) fn of_deref(value: SymValueRef) -> Self {
        Self {
            value,
            kind: ValueUsageInPlace::Deref,
        }
    }

    pub(crate) fn of_size(value: SymValueRef) -> Self {
        Self {
            value,
            kind: ValueUsageInPlace::Size,
        }
    }
}

pub(super) mod strategies {
    use common::{log_debug, log_info};

    use crate::backends::basic::{concrete::Concretizer, config::SymbolicPlaceStrategy};

    use super::*;

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
}

mod utils {
    use crate::utils::alias::RRef;

    use super::*;

    impl<SE, CE, E: From<SE> + From<CE>> SymPlaceHandler
        for Box<dyn SymPlaceHandler<SymEntity = SE, ConcEntity = CE, Entity = E>>
    {
        type SymEntity = SE;
        type ConcEntity = CE;
        type Entity = E;

        fn handle<'a>(
            &mut self,
            sym_entity: Self::SymEntity,
            get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
        ) -> Self::Entity {
            self.as_mut().handle(sym_entity, get_conc)
        }
    }

    impl<SE, CE, E: From<SE> + From<CE>> SymPlaceHandler
        for RRef<dyn SymPlaceHandler<SymEntity = SE, ConcEntity = CE, Entity = E>>
    {
        type SymEntity = SE;
        type ConcEntity = CE;
        type Entity = E;

        #[inline]
        fn handle<'a>(
            &mut self,
            sym_entity: Self::SymEntity,
            get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
        ) -> Self::Entity {
            let mut this = self as &RRef<_>;
            this.handle(sym_entity, get_conc)
        }
    }

    impl<SE, CE, E: From<SE> + From<CE>> SymPlaceHandler
        for &RRef<dyn SymPlaceHandler<SymEntity = SE, ConcEntity = CE, Entity = E>>
    {
        type SymEntity = SE;
        type ConcEntity = CE;
        type Entity = E;

        #[inline]
        fn handle<'a>(
            &mut self,
            sym_entity: Self::SymEntity,
            get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
        ) -> Self::Entity {
            self.borrow_mut().handle(sym_entity, get_conc)
        }
    }
}
