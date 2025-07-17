use crate::utils::alias::RRef;

use super::{ConcolicValueObtainer, SymPlaceHandler};

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
