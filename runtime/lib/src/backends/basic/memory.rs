use std::borrow::Cow;

use common::log_warn;

use crate::abs::{
    AssignmentId, PlaceUsage, RawAddress, TypeId, TypeSize,
    backend::{AssignmentHandler, RawMemoryHandler, RuntimeBackend},
    expr::BinaryExprBuilder,
};

use crate::backends::basic::{self as backend};
use backend::{
    BasicBackend, BasicExprBuilder, BasicPlaceValue, BasicSymPlaceHandler, BasicValue,
    BasicValueExprBuilder, TypeDatabase,
    assignment::{self, AssignmentServices},
    expr::prelude::{
        ConcatExpr, ConcreteValue, ConcreteValueRef, ConstValue, SymValueRef, UnevalValue, Value,
        ValueRef,
    },
    implication::{Implied, Precondition, PreconditionConstruct},
    state::SymPlaceSymEntity,
};

type AssignmentHandlerImpl<'a> = <BasicBackend as RuntimeBackend>::AssignmentHandler<'a>;

pub(crate) struct BasicRawMemoryHandler<'a, EB> {
    services: AssignmentServices<'a, EB>,
    sym_size_handler: &'a mut BasicSymPlaceHandler,
}

impl BasicRawMemoryHandler<'_, BasicExprBuilder> {
    pub(super) fn new<'a>(
        backend: &'a mut BasicBackend,
    ) -> BasicRawMemoryHandler<'a, BasicExprBuilder> {
        let sym_size_handler = &mut backend.sym_place_handler;
        let services = assignment::services_from_backend!(backend);

        BasicRawMemoryHandler {
            services,
            sym_size_handler,
        }
    }
}

impl<'a, EB: BasicValueExprBuilder + 'static> RawMemoryHandler for BasicRawMemoryHandler<'a, EB> {
    type Place = BasicPlaceValue;
    type Operand = BasicValue;

    fn place_from_ptr(
        self,
        ptr: Self::Operand,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::Place {
        // FIXME: Add support for implicit flow
        self.place_from_ptr_inner(ptr, conc_ptr, ptr_type_id, usage)
    }

    /* NOTE: These are naive implementations.
     * Currently, we prefer code reuse and simplicity over performance.
     * This operation can be optimized in various phases, including but not
     * limited to the reads, writes, symbolic place handling, etc.
     * Until a considerable performance issue is observed, we will keep it simple.
     */

    fn copy(
        mut self,
        assignment_id: AssignmentId,
        src_ptr: Self::Operand,
        conc_src_ptr: RawAddress,
        dst_ptr: Self::Operand,
        conc_dst_ptr: RawAddress,
        count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    ) {
        self.check_count(&count, conc_count);
        let count = count.map_value(|_| conc_count);

        let size = self.pointee_size(ptr_type_id);

        // Read everything first, so that overlapping writes do not cause issues.
        let values = self
            .ptr_at_offsets(&src_ptr, conc_src_ptr, count.clone(), size)
            .map(|(src_at_i, conc_src_at_i)| {
                let src_place_at_i = self.place_from_ptr_inner(
                    src_at_i,
                    conc_src_at_i,
                    ptr_type_id,
                    PlaceUsage::Read,
                );
                self.services.vars_state.copy_place(&src_place_at_i)
            })
            .collect::<Vec<_>>();

        // Write
        for ((dst_at_i, conc_dst_at_i), value) in self
            .ptr_at_offsets(&dst_ptr, conc_dst_ptr, count, size)
            .zip(values.into_iter())
        {
            let dst_place_at_i =
                self.place_from_ptr_inner(dst_at_i, conc_dst_at_i, ptr_type_id, PlaceUsage::Write);
            {
                AssignmentHandlerImpl::with_services(
                    assignment_id,
                    dst_place_at_i,
                    (&mut self.services).into(),
                )
                .use_of(value)
            };
        }
    }

    fn set(
        mut self,
        assignment_id: AssignmentId,
        ptr: Self::Operand,
        conc_ptr: RawAddress,
        value: Self::Operand,
        count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    ) {
        self.check_count(&count, conc_count);
        let count = count.map_value(|_| conc_count);

        /* Note: This one of two possible ways to handle the `set` operation.
         * 1. Cast the pointer to *mut u8 and write the value.
         * 2. Make an array-like value with `value` repeated as size of the pointee.
         * We are choosing the second as currently, we have all pieces available for the implementation.
         */

        let pointee_ty = self
            .type_manager()
            .get_type(&ptr_type_id)
            .pointee_ty
            .unwrap();

        let size = self.type_manager().get_size(&pointee_ty).unwrap();

        let value = if value.is_symbolic() {
            value.map_value(|v| match size {
                0 => UnevalValue::Some.to_value_ref(),
                1 => v,
                1.. => ConcatExpr::new(
                    core::iter::repeat_n(v, size as usize).collect(),
                    pointee_ty.into(),
                )
                .to_value_ref()
                .into(),
            })
        } else {
            value.map_value(|_| UnevalValue::Some.to_value_ref())
        };

        // Write
        for (dst_at_i, conc_dst_at_i) in self.ptr_at_offsets(&ptr, conc_ptr, count, size) {
            let place_at_i =
                self.place_from_ptr_inner(dst_at_i, conc_dst_at_i, ptr_type_id, PlaceUsage::Write);
            {
                AssignmentHandlerImpl::with_services(
                    assignment_id,
                    place_at_i,
                    (&mut self.services).into(),
                )
                .use_of(value.clone())
            };
        }
    }

    fn swap(
        mut self,
        assignment_id: AssignmentId,
        first_ptr: Self::Operand,
        conc_first_ptr: RawAddress,
        second_ptr: Self::Operand,
        conc_second_ptr: RawAddress,
        ptr_type_id: TypeId,
    ) {
        macro_rules! place_from_first {
            ($usage:expr) => {
                self.place_from_ptr_inner(first_ptr.clone(), conc_first_ptr, ptr_type_id, $usage)
            };
        }
        macro_rules! place_from_second {
            ($usage:expr) => {
                self.place_from_ptr_inner(second_ptr.clone(), conc_second_ptr, ptr_type_id, $usage)
            };
        }

        let first_value = self
            .services
            .vars_state
            .copy_place(&place_from_first!(PlaceUsage::Read));

        let second_value = self
            .services
            .vars_state
            .copy_place(&place_from_second!(PlaceUsage::Read));

        macro_rules! assign {
            ($place:expr, $value:expr) => {
                AssignmentHandlerImpl::with_services(
                    assignment_id,
                    $place,
                    (&mut self.services).into(),
                )
                .use_of($value);
            };
        }

        assign!(place_from_first!(PlaceUsage::Write), second_value);
        assign!(place_from_second!(PlaceUsage::Write), first_value);
    }
}

impl<'a, EB> BasicRawMemoryHandler<'a, EB> {
    fn place_from_ptr_inner(
        &self,
        ptr: BasicValue,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> BasicPlaceValue {
        self.services
            .vars_state
            .ref_place_by_ptr(ptr, conc_ptr, ptr_type_id, usage)
    }

    fn type_manager(&self) -> &'a dyn TypeDatabase {
        self.services.type_manager
    }

    fn pointee_size(&self, ptr_type_id: TypeId) -> TypeSize {
        self.type_manager()
            .get_pointee_size(&ptr_type_id)
            .unwrap_or_else(|| panic!("Pointer to unsized type is not expected: {}", ptr_type_id))
    }

    fn check_count(&mut self, count: &BasicValue, conc_count: usize) {
        if count.is_symbolic() {
            let count = self.sym_size_handler.handle(
                SymPlaceSymEntity::of_size(SymValueRef::new(count.value.clone())),
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
}

impl<'a, EB: BasicValueExprBuilder + 'static> BasicRawMemoryHandler<'a, EB> {
    fn ptr_at_offsets(
        &self,
        ptr: &BasicValue,
        conc_ptr: RawAddress,
        count: Implied<usize>,
        size: TypeSize,
    ) -> impl Iterator<Item = (BasicValue, RawAddress)> {
        let precondition = Precondition::merge([ptr.by.clone(), count.by.clone()]);

        let values: Box<dyn Iterator<Item = ValueRef>> = match ptr.as_ref() {
            Value::Concrete(conc_value) => {
                let ptr = {
                    if cfg!(debug_assertions) {
                        let retrieved = match conc_value {
                            ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
                                let retrieved =
                                    unsafe { raw.try_retrieve_as_scalar(self.type_manager()) }
                                        .expect("Expected a raw pointer of a sized type");
                                Cow::Owned(retrieved.into())
                            }
                            _ => Cow::Borrowed(conc_value),
                        };

                        let ptr = match retrieved.as_ref() {
                            ConcreteValue::Const(ConstValue::Addr(addr)) => *addr,
                            _ => panic!("Expected a concrete pointer, got: {}", retrieved),
                        };

                        assert_eq!(ptr, conc_ptr, "Concrete value does not match");
                    }
                    conc_ptr
                };

                let size = size as usize;
                Box::new((0..count.value).map(move |i| {
                    ConstValue::Addr(ptr.wrapping_byte_add(i as usize * size)).to_value_ref()
                }))
            }
            Value::Symbolic(..) => {
                // FIXME: Concretize (if place handler does) once and reuse
                let expr_builder = self.services.expr_builder.clone();
                let ptr = ptr.value.clone();
                Box::new((0..count.value).map(move |i| {
                    expr_builder
                        .borrow_mut()
                        .inner()
                        .offset((ptr.clone(), ConstValue::from(i).to_value_ref()), size)
                }))
            }
        };

        values
            .map(move |v| Implied {
                by: precondition.clone(),
                value: v,
            })
            .zip((0..count.value).map(move |i| conc_ptr.wrapping_byte_add(i * size as usize)))
    }
}
