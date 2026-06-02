use std::borrow::Cow;

use common::log_warn;

use crate::{
    abs::{AssignmentId, PlaceUsage, RawAddress, TypeId, TypeSize, expr::BinaryExprBuilder},
    backends::mdsan::MdMemoryState,
    pri::fluent::backend::{AssignmentHandler, RawMemoryHandler, RuntimeBackend},
};

use super::alias::backend;
use backend::{
    MdSanBackend, MdSanPlaceValue, MdSanValue, TypeDatabase,
    assignment::{self, AssignmentServices},
};

type AssignmentHandlerImpl<'a> = <MdSanBackend as RuntimeBackend>::AssignmentHandler<'a>;

pub(crate) struct MdSanRawMemoryHandler<'a> {
    services: AssignmentServices<'a>,
}

impl MdSanRawMemoryHandler<'_> {
    pub(super) fn new<'a>(backend: &'a mut MdSanBackend) -> MdSanRawMemoryHandler<'a> {
        let services = assignment::services_from_backend!(backend);

        MdSanRawMemoryHandler { services }
    }
}

impl<'a> RawMemoryHandler for MdSanRawMemoryHandler<'a> {
    type Place = MdSanPlaceValue;
    type Operand = MdSanValue;

    fn place_from_ptr(
        self,
        ptr: Self::Operand,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::Place {
        self.services
            .vars_state
            .ref_place_by_ptr(conc_ptr, ptr_type_id, usage)
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
        // TODO
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
        // TODO
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
        // TODO
    }
}

impl<'a> MdSanRawMemoryHandler<'a> {
    fn place_from_ptr_inner(
        &self,
        ptr: MdSanValue,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> MdSanPlaceValue {
        todo!()
    }

    fn type_manager(&self) -> &'a dyn TypeDatabase {
        todo!()
    }

    fn pointee_size(&self, ptr_type_id: TypeId) -> TypeSize {
        self.type_manager()
            .get_pointee_size(&ptr_type_id)
            .unwrap_or_else(|| panic!("Pointer to unsized type is not expected: {}", ptr_type_id))
    }

    fn check_count(&mut self, count: &MdSanValue, conc_count: usize) {}
}

impl<'a> MdSanRawMemoryHandler<'a> {
    // fn ptr_at_offsets(
    //     &self,
    //     ptr: &MdSanValue,
    //     conc_ptr: RawAddress,
    //     count: Implied<usize>,
    //     size: TypeSize,
    // ) -> impl Iterator<Item = (MdSanValue, RawAddress)> {
    //     let precondition = Precondition::merge([ptr.by.clone(), count.by.clone()]);

    //     let values: Box<dyn Iterator<Item = ValueRef>> = match ptr.as_ref() {
    //         Value::Concrete(conc_value) => {
    //             let ptr = {
    //                 if cfg!(debug_assertions) {
    //                     let retrieved = match conc_value {
    //                         ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
    //                             let retrieved =
    //                                 unsafe { raw.try_retrieve_as_scalar(self.type_manager()) }
    //                                     .expect("Expected a raw pointer of a sized type");
    //                             Cow::Owned(retrieved.into())
    //                         }
    //                         _ => Cow::Borrowed(conc_value),
    //                     };

    //                     let ptr = match retrieved.as_ref() {
    //                         ConcreteValue::Const(ConstValue::Addr(addr)) => *addr,
    //                         _ => panic!("Expected a concrete pointer, got: {}", retrieved),
    //                     };

    //                     assert_eq!(ptr, conc_ptr, "Concrete value does not match");
    //                 }
    //                 conc_ptr
    //             };

    //             let size = size as usize;
    //             Box::new((0..count.value).map(move |i| {
    //                 ConstValue::Addr(ptr.wrapping_byte_add(i as usize * size)).to_value_ref()
    //             }))
    //         }
    //         Value::Symbolic(..) => {
    //             // FIXME: Concretize (if place handler does) once and reuse
    //             let expr_builder = self.services.expr_builder.clone();
    //             let ptr = ptr.value.clone();
    //             Box::new((0..count.value).map(move |i| {
    //                 expr_builder
    //                     .borrow_mut()
    //                     .inner()
    //                     .offset((ptr.clone(), ConstValue::from(i).to_value_ref()), size)
    //             }))
    //         }
    //     };

    //     values
    //         .map(move |v| Implied {
    //             by: precondition.clone(),
    //             value: v,
    //         })
    //         .zip((0..count.value).map(move |i| conc_ptr.wrapping_byte_add(i * size as usize)))
    // }
}
