mod pointer_based;

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use pointer_based::sym_place::strategies::make_sym_place_handler;

use super::{ConcreteValueRef, SymValueRef};

pub(crate) trait SymPlaceHandler {
    type SymEntity = SymValueRef;
    type ConcEntity = ConcreteValueRef;
    type Entity: From<Self::SymEntity> + From<Self::ConcEntity>;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity;
}

impl<SE, CE, E: From<SE> + From<CE>> SymPlaceHandler
    for Box<dyn SymPlaceHandler<SymEntity = SE, ConcEntity = CE, Entity = E>>
{
    type SymEntity = SE;
    type ConcEntity = CE;
    type Entity = E;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        self.as_mut().handle(sym_entity, get_conc)
    }
}
