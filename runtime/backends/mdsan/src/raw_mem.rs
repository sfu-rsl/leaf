use common::log_warn;

use leaf_runtime::{
    abs::{AssignmentId, PlaceUsage, RawAddress, TypeId, TypeSize},
    pri::fluent::backend::{AssignmentHandler, RawMemoryHandler, RuntimeBackend},
};

use super::alias::backend;
use backend::{
    MdMemoryState, MdSanBackend, MdSanPlaceInspector, MdSanPlaceValue, MdSanValue, PlaceInspector,
    assignment::{self, AssignmentServices},
    state::MemoryRegion,
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
        _ptr: Self::Operand,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::Place {
        self.services
            .vars_state
            .ref_place_by_ptr(conc_ptr, ptr_type_id, usage)
    }

    fn copy(
        self,
        _assignment_id: AssignmentId,
        _src_ptr: Self::Operand,
        conc_src_ptr: RawAddress,
        _dst_ptr: Self::Operand,
        conc_dst_ptr: RawAddress,
        _count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    ) {
        self.services.vars_state.copy_raw_memory(
            conc_src_ptr,
            conc_dst_ptr,
            ptr_type_id,
            conc_count,
        );
    }

    fn set(
        self,
        _assignment_id: AssignmentId,
        _ptr: Self::Operand,
        conc_ptr: RawAddress,
        _value: Self::Operand,
        _count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    ) {
        let pointee_ty = self
            .services
            .type_manager
            .get_pointee_ty(&ptr_type_id)
            .unwrap();
        let size = self.services.type_manager.get_size(&pointee_ty).unwrap();

        let erased_any = self.services.vars_state.erase_place(&MemoryRegion {
            addr: conc_ptr,
            size: size * conc_count as TypeSize,
        });

        if erased_any {
            log_warn!(
                "Low-level setting bytes at {:p} caused erasure of labels",
                conc_ptr
            );
        }
    }

    fn swap(
        mut self,
        _assignment_id: AssignmentId,
        _first_ptr: Self::Operand,
        conc_first_ptr: RawAddress,
        _second_ptr: Self::Operand,
        conc_second_ptr: RawAddress,
        ptr_type_id: TypeId,
    ) {
        macro_rules! place_from_first {
            ($usage:expr) => {
                self.place_from_ptr_inner(conc_first_ptr, ptr_type_id, $usage)
            };
        }
        macro_rules! place_from_second {
            ($usage:expr) => {
                self.place_from_ptr_inner(conc_second_ptr, ptr_type_id, $usage)
            };
        }

        let first_value = self
            .services
            .vars_state
            .take_place(&place_from_first!(PlaceUsage::Move));

        let second_value = self
            .services
            .vars_state
            .take_place(&place_from_second!(PlaceUsage::Move));

        macro_rules! assign {
            ($place:expr, $value:expr) => {
                AssignmentHandlerImpl::with_services($place, (&mut self.services).into())
                    .use_of($value);
            };
        }

        assign!(place_from_first!(PlaceUsage::Write), second_value);
        assign!(place_from_second!(PlaceUsage::Write), first_value);
    }

    fn raw_eq(
        self,
        _first_ref: Self::Operand,
        _conc_first_ptr: RawAddress,
        _second_ref: Self::Operand,
        _conc_second_ptr: RawAddress,
        _ptr_type_id: TypeId,
    ) -> Self::Operand {
        // No inspection required as we the references are already inspected when they are created.
        MdSanValue::non_rel()
    }

    fn compare_bytes(
        self,
        _first_ptr: Self::Operand,
        conc_first_ptr: RawAddress,
        _second_ptr: Self::Operand,
        conc_second_ptr: RawAddress,
        _count: Self::Operand,
        _conc_count: usize,
        ptr_type_id: TypeId,
    ) -> Self::Operand {
        let inspector = MdSanPlaceInspector::new(self.services.vars_state);
        for i in 0.._conc_count {
            for ptr in [conc_first_ptr, conc_second_ptr] {
                inspector.inspect_place_for_access(&self.place_from_ptr_inner(
                    ptr.wrapping_byte_add(i),
                    ptr_type_id,
                    PlaceUsage::Copy,
                ));
            }
        }

        MdSanValue::non_rel()
    }
}

impl<'a> MdSanRawMemoryHandler<'a> {
    fn place_from_ptr_inner(
        &self,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> MdSanPlaceValue {
        self.services
            .vars_state
            .ref_place_by_ptr(conc_ptr, ptr_type_id, usage)
    }
}
