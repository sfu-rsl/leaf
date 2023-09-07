use std::{
    cell::RefCell,
    collections::{hash_map::Entry, HashMap},
    rc::Rc,
};

use delegate::delegate;

use crate::{
    abs::RawPointer,
    backends::basic::{place::LocalWithAddress, VariablesState},
    utils::SelfHierarchical,
};

use super::{
    super::{
        alias::SymValueRefProjector as SymbolicProjector, expr::prelude::*,
        place::PlaceWithAddress, ValueRef,
    },
    proj::{apply_projs_sym, LocalMap, ProjectionResolutionExt},
};

type Local = LocalWithAddress;
type Place = PlaceWithAddress;
type Projection = crate::abs::Projection<Local>;

type RRef<T> = Rc<RefCell<T>>;

pub(in super::super) struct RawPointerVariableState<
    VS: VariablesState<Place>,
    SP: SymbolicProjector,
> {
    memory: HashMap<RawPointer, SymValueRef>,
    fallback: VS,
    sym_projector: RRef<SP>,
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> RawPointerVariableState<VS, SP> {
    pub fn new(fallback: VS, sym_projector: RRef<SP>) -> Self {
        Self {
            memory: HashMap::new(),
            fallback,
            sym_projector,
        }
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> SelfHierarchical
    for RawPointerVariableState<VS, SP>
where
    VS: SelfHierarchical,
{
    fn add_layer(self) -> Self {
        Self {
            fallback: self.fallback.add_layer(),
            ..self
        }
    }

    fn drop_layer(self) -> Option<Self> {
        self.fallback.drop_layer().map(|f| Self {
            fallback: f,
            ..self
        })
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> LocalMap<Local>
    for RawPointerVariableState<VS, SP>
where
    VS: LocalMap<Local>,
{
    fn get(&self, local: &Local) -> Option<ValueRef> {
        let Some(address) = local.address() else {
            return self.fallback.get(local);
        };

        Some(if let Some(sym_val) = self.get(&address) {
            sym_val.clone_to()
        } else {
            UnevalValue::Lazy(address).to_value_ref()
        })
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> VariablesState<Place>
    for RawPointerVariableState<VS, SP>
where
    Self: LocalMap<Local>,
{
    delegate! {
        to self.fallback {
            fn id(&self) -> usize;
        }
    }

    fn copy_place(&self, place: &Place) -> ValueRef {
        let Some(address) = place.address() else {
            return self.fallback.copy_place(place);
        };

        if let Some((sym_val, sym_projs)) = self.first_symbolic_value(place) {
            apply_projs_sym(
                self.sym_projector.clone(),
                sym_val,
                sym_projs.iter().map(|p| p.resolved_index(self)),
            )
            .into()
        } else {
            UnevalValue::Lazy(address).to_value_ref()
        }
    }

    fn try_take_place(&mut self, place: &Place) -> Option<ValueRef> {
        let Some(address) = place.address() else {
            return self.fallback.try_take_place(place);
        };

        let result = if let Some((sym_val, sym_projs)) = self.first_symbolic_value(place) {
            if sym_projs.is_empty() {
                let value = sym_val.clone_to();
                self.memory.remove(&address);
                value
            } else {
                apply_projs_sym(
                    self.sym_projector.clone(),
                    sym_val,
                    sym_projs.iter().map(|p| p.resolved_index(self)),
                )
                .into()
            }
        } else {
            UnevalValue::Lazy(address).to_value_ref()
        };
        Some(result)
    }

    fn set_place(&mut self, place: &Place, value: ValueRef) {
        let Some(address) = place.address() else {
            return self.fallback.set_place(place, value);
        };

        if let Some((_sym_val, sym_projs)) = self.first_symbolic_value(place) {
            if !sym_projs.is_empty() {
                todo!("#238");
            }
        }

        let entry = self.memory.entry(address);
        if !value.is_symbolic() {
            if let Entry::Occupied(entry) = entry {
                entry.remove();
            }

            return;
        }

        entry.insert_entry(SymValueRef::new(value));
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> RawPointerVariableState<VS, SP> {
    /// Finds the first symbolic value in the chain of projections leading to the place.
    /// # Returns
    /// The first symbolic value and the remaining projections to be applied on it.
    fn first_symbolic_value<'a, 'b>(
        &'a self,
        place: &'b Place,
    ) -> Option<(&'a SymValueRef, &'b [Projection])> {
        let projs = place.projections();
        if let Some(sym_val) = self.get(&place.local().address()?) {
            Some((sym_val, projs))
        } else {
            // Checking for the value after each projection.
            place
                .proj_addresses()
                .enumerate()
                .find_map(|(i, addr)| {
                    addr.and_then(|addr| self.get(&addr))
                        .map(|sym_val| (i, sym_val))
                })
                .map(|(i, sym_val)| (sym_val, &projs[i..projs.len()]))
        }
    }

    #[inline]
    fn get<'a, 'b>(&'a self, address: &'b RawPointer) -> Option<&'a SymValueRef> {
        self.memory.get(address)
    }
}
