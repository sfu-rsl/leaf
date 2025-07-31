mod pointer_based;
mod utils;

use core::num::NonZero;

use std::borrow::Cow;

use common::{log_warn, pri::TypeSize};
use derive_more as dm;

use crate::{
    abs::{
        PlaceUsage,
        backend::{AssignmentHandler, MemoryHandler, PlaceHandler, RuntimeBackend},
        expr::BinaryExprBuilder,
    },
    backends::basic::{
        BasicPlaceValue, BasicValue, GenericVariablesState, Implied, Precondition,
        ValueRefExprBuilderWrapper,
        expr::{ConcreteValue, ConstValue, UnevalValue, Value, ValueRef},
        implication::PreconditionConstruct,
    },
};

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use pointer_based::sym_place::strategies::make_sym_place_handler;

use crate::backends::basic as backend;
use backend::{
    AssignmentId, BasicBackend, CallStackInfo, ConcreteValueRef, SymValueRef, TypeId,
    VariablesState, concrete::ConcolicValueObtainer,
};

#[derive(Debug, dm::Deref)]
pub(super) struct SymPlaceSymEntity {
    #[deref]
    value: SymValueRef,
    kind: ValueUsageInPlace,
}

#[derive(Debug)]
pub(super) enum ValueUsageInPlace {
    Deref,
    Index,
    Size,
}

pub(super) trait SymPlaceHandler {
    type SymEntity = SymPlaceSymEntity;
    type ConcEntity = ConcreteValueRef;
    type Entity: From<Self::SymEntity> + From<Self::ConcEntity>;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity;
}

pub(crate) struct BasicMemoryHandler<'a> {
    // FIXME: Keeping a reference to the backend is a result of low cohesion.
    // By separating the operations this can be avoided.
    backend: &'a mut BasicBackend,
}

impl<'a> BasicMemoryHandler<'a> {
    pub(super) fn new(backend: &'a mut BasicBackend) -> BasicMemoryHandler<'a> {
        BasicMemoryHandler { backend }
    }

    fn vars_state(&mut self) -> &mut impl VariablesState {
        self.backend.call_stack_manager.top()
    }
}

impl<'a> MemoryHandler for BasicMemoryHandler<'a> {
    type Place = BasicPlaceValue;
    type Operand = BasicValue;

    fn mark_dead(mut self, place: Self::Place) {
        self.vars_state().drop_place(&place);
    }

    fn copy(
        mut self,
        assignment_id: AssignmentId,
        src_ptr: Self::Operand,
        dst_ptr: Self::Operand,
        count: Self::Operand,
        ptr_type_id: TypeId,
        conc_count: usize,
    ) {
        self.check_count(&count, conc_count);
        let count = count.map_value(|_| conc_count);

        /* NOTE: This is a naive implementation.
         * Currently, we prefer code reuse and simplicity over performance.
         * This operation can be optimized in various phases, including but not
         * limited to the reads, writes, and the antecedents.
         * Until a considerable performance issue is observed, we will keep it simple.
         */

        // Read everything first, so that overlapping writes do not cause issues.

        let size = NonZero::new(self.backend.type_manager.get_size(&ptr_type_id).unwrap()).unwrap();

        let values = self
            .ptr_at_offsets(&src_ptr, count.clone(), size)
            .map(|src_at_i| {
                let src_place_at_i = self
                    .backend
                    .place(PlaceUsage::Read)
                    .from_ptr(src_at_i, ptr_type_id);
                self.vars_state().copy_place(&src_place_at_i)
            })
            .collect::<Vec<_>>();

        // Write
        for (dst_at_i, value) in self
            .ptr_at_offsets(&dst_ptr, count, size)
            .zip(values.into_iter())
        {
            let dst_place_at_i = self
                .backend
                .place(PlaceUsage::Write)
                .from_ptr(dst_at_i, ptr_type_id);
            self.backend
                .assign_to(assignment_id, dst_place_at_i)
                .use_of(value);
        }
    }
}

impl<'a> BasicMemoryHandler<'a> {
    fn check_count(&mut self, count: &BasicValue, conc_count: usize) {
        if count.is_symbolic() {
            let count = self.backend.sym_place_handler.handle(
                SymPlaceSymEntity {
                    value: SymValueRef::new(count.value.clone()),
                    kind: ValueUsageInPlace::Size,
                },
                Box::new(|| ConcreteValueRef::new(ConstValue::from(conc_count).to_value_ref())),
            );
            if count.is_symbolic() {
                log_warn!(
                    "Symbolic count {} is not supported and will be ignored",
                    count
                );
            }
        }
    }

    fn ptr_at_offsets(
        &mut self,
        ptr: &BasicValue,
        count: Implied<usize>,
        size: NonZero<TypeSize>,
    ) -> impl Iterator<Item = BasicValue> {
        let precondition = Precondition::merge([ptr.by.clone(), count.by.clone()]);

        let values: Box<dyn Iterator<Item = ValueRef>> = match ptr.as_ref() {
            Value::Concrete(conc_value) => {
                let retrieved = match conc_value {
                    ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
                        let retrieved = unsafe {
                            raw.try_retrieve_as_scalar(self.backend.type_manager.as_ref())
                        }
                        .expect("Expected a raw pointer of a sized type");
                        Cow::Owned(retrieved.into())
                    }
                    _ => Cow::Borrowed(conc_value),
                };

                let ptr = match retrieved.as_ref() {
                    ConcreteValue::Const(ConstValue::Addr(addr)) => *addr,
                    _ => panic!("Expected a concrete pointer, got: {}", retrieved),
                };

                let size = size.get() as usize;
                Box::new((0..count.value).map(move |i| {
                    ConstValue::Addr(ptr.wrapping_byte_add(i as usize * size)).to_value_ref()
                }))
            }
            Value::Symbolic(..) => {
                let expr_builder = self.backend.expr_builder.clone();
                let ptr = ptr.value.clone();
                Box::new((0..count.value).map(move |i| {
                    expr_builder
                        .borrow_mut()
                        .inner()
                        .offset((ptr.clone(), ConstValue::from(i).to_value_ref()))
                }))
            }
        };

        values.map(move |v| Implied {
            by: precondition.clone(),
            value: v,
        })
    }
}
