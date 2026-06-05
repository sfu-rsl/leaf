use core::iter;

use crate::{
    abs::{AssignmentId, FieldIndex, TypeId},
    pri::fluent::backend::AssignmentHandler,
    type_info::{TypeLayoutResolver, TypeLayoutResolverExt},
    utils::MutAccess,
};

use super::alias::backend;
use backend::{
    MdMemoryState, MdSanBackend, MdSanPlaceInspector, MdSanPlaceValue, MdSanTypeManager,
    MdSanValue, MdSanVariablesState, PlaceInspector,
};

pub(super) struct AssignmentServices<'a> {
    pub(super) vars_state: &'a mut MdSanVariablesState,
    pub(super) type_manager: &'a MdSanTypeManager,
}

// Meant for leveraging field-level borrowing to avoid borrowing issues.
macro_rules! services_from_backend {
    ($backend:expr) => {{
        AssignmentServices {
            vars_state: &mut $backend.vars_state,
            type_manager: $backend.type_manager.as_ref(),
        }
    }};
}
use common::types::PointerOffset;
pub(super) use services_from_backend;

pub(crate) struct MdSanAssignmentHandler<'s, 'a: 's> {
    id: AssignmentId,
    dest: MdSanPlaceValue,
    services: MutAccess<'s, AssignmentServices<'a>>,
}

impl MdSanAssignmentHandler<'_, '_> {
    pub(super) fn new<'a>(
        id: AssignmentId,
        dest: MdSanPlaceValue,
        backend: &'a mut MdSanBackend,
    ) -> MdSanAssignmentHandler<'a, 'a> {
        MdSanAssignmentHandler::with_services(id, dest, services_from_backend!(backend).into())
    }

    pub(super) fn with_services<'s, 'a>(
        id: AssignmentId,
        dest: MdSanPlaceValue,
        services: MutAccess<'s, AssignmentServices<'a>>,
    ) -> MdSanAssignmentHandler<'s, 'a> {
        MdSanAssignmentHandler { id, dest, services }
    }
}

impl<'s, 'a: 's> AssignmentHandler for MdSanAssignmentHandler<'s, 'a> {
    type Place = MdSanPlaceValue;
    type Operand = MdSanValue;

    fn use_of(mut self, operand: Self::Operand) {
        self.set(operand);
    }

    fn repeat_of(self, operand: Self::Operand, count: usize) {
        if !operand.is_rel() {
            return self.some();
        }

        self.array_from(iter::repeat_n(operand, count))
    }

    fn array_from(mut self, items: impl Iterator<Item = Self::Operand>) {
        self.set_array_value(items.map(|i| i.is_rel().then_some(i)));
    }

    fn adt_from(
        mut self,
        fields: impl Iterator<Item = Self::Operand>,
        variant: Option<common::pri::VariantIndex>,
    ) {
        if let Some(true) = self.dest.is_md(self.services.type_manager) {
            // FIXME: What if we have MD<MD<T>>?
            return self.services.vars_state.set_place_alive(&self.dest);
        }

        self.set_adt_value(fields.map(|f| f.is_rel().then_some(f)), variant);
    }

    fn union_from(mut self, active_field: common::pri::FieldIndex, value: Self::Operand) {
        let fields = (0..active_field)
            .map(|_| None)
            .chain(iter::once(Some(value)));
        self.set_adt_value(fields, None)
    }

    fn discriminant_from(self, place: Self::DiscriminablePlace) {
        self.inspect_place_for_access(&place);
        self.some()
    }

    fn ref_to(self, place: Self::Place, _is_mutable: bool) {
        self.inspect_place_for_access(&place);
        self.some();
    }

    fn address_of(self, place: Self::Place, is_mutable: bool) {
        self.ref_to(place, is_mutable)
    }

    fn some(self) {
        // Nothing to do.
    }

    fn use_if_eq(self, _current: Self::Operand, _expected: Self::Operand, _then: Self::Operand) {
        // Not possible to be an MD.
        self.some()
    }

    fn use_and_check_eq(self, _val: Self::Operand, _expected: Self::Operand) {
        // Not possible to be an MD.
        self.some()
    }

    fn thread_local_ref_to(self) {
        self.some()
    }

    fn cast_of(self, _operand: Self::Operand, _target: crate::abs::CastKind) {
        self.some()
    }
    fn binary_op_between(
        self,
        _operator: crate::abs::BinaryOp,
        _first: Self::Operand,
        _second: Self::Operand,
    ) {
        self.some()
    }
    fn unary_op_on(self, _operator: crate::abs::UnaryOp, _operand: Self::Operand) {
        self.some()
    }
    fn raw_ptr_from(self, _data_ptr: Self::Operand, _metadata: Self::Operand, _is_mutable: bool) {
        self.some()
    }
    fn variant_index(self, _variant_index: common::pri::VariantIndex) {
        self.some()
    }
    fn shallow_init_box_from(self, _value: Self::Operand) {
        self.some()
    }
    fn wrap_in_unsafe_binder(self, _value: Self::Operand) {
        self.some()
    }
}

impl<'s, 'a: 's> MdSanAssignmentHandler<'s, 'a> {
    fn set(&mut self, value: MdSanValue) {
        self.services.vars_state.set_place(&self.dest, value);
    }

    fn set_adt_value(
        &mut self,
        fields: impl Iterator<Item = Option<MdSanValue>>,
        variant_index: Option<common::pri::VariantIndex>,
    ) {
        let mut has_any = false;
        let fields = fields
            .inspect(|f| has_any |= f.is_some())
            .collect::<Vec<_>>();

        let value = if !has_any {
            MdSanValue::non_rel()
        } else {
            let field_offsets = self
                .services
                .type_manager
                .layouts()
                .resolve_adt_fields(self.dest_type_id(), variant_index)
                .map(|(index, _type_id, offset, _size)| (index, offset));
            lay_out(fields, field_offsets)
        };

        self.set(value);
    }

    fn set_array_value(&mut self, items: impl Iterator<Item = Option<MdSanValue>>) {
        let mut has_any = false;
        let items = items
            .inspect(|f| has_any |= f.is_some())
            .collect::<Vec<_>>();

        let value = if !has_any {
            MdSanValue::non_rel()
        } else {
            let field_offsets = self
                .services
                .type_manager
                .layouts()
                .resolve_array_elements(self.dest_type_id())
                .1
                .enumerate()
                .map(|(index, (offset, _size))| (index as _, offset));
            lay_out(items, field_offsets)
        };

        self.set(value);
    }

    fn inspect_place_for_access(&self, place: &MdSanPlaceValue) {
        MdSanPlaceInspector::new(self.services.vars_state).inspect_place_for_access(place);
    }

    fn dest_type_id(&self) -> TypeId {
        self.dest
            .type_id(self.services.type_manager)
            .unwrap_or_else(|| panic!("Unknown type for assignment destination: {:?}", self.dest))
    }
}

fn lay_out(
    mut fields: Vec<Option<MdSanValue>>,
    field_offsets: impl Iterator<Item = (FieldIndex, PointerOffset)>,
) -> MdSanValue {
    let mut labels = Vec::with_capacity(fields.len());

    for (field_index, field_offset) in field_offsets {
        if let Some(value) = fields
            // For unions, the list of fields may be incomplete.
            .get_mut(field_index as usize)
            .and_then(|f| f.take())
        {
            labels.extend(value.labels_with_base(field_offset));
        }
    }

    MdSanValue::new(labels)
}
