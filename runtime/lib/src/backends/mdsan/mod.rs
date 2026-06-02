use std::rc::Rc;

use crate::abs::backend::Shutdown;
use crate::abs::{PlaceUsage, RawAddress, TypeId};
use crate::pri::fluent::backend::{
    RuntimeBackend,
    shared::noop::{NoOpAnnotationHandler, NoOpConstraintHandler},
};

mod alias;
mod assignment;
mod call;
mod instance;
mod memory;
mod operand;
mod place;
mod state;
mod type_info;

use alias::*;

pub(crate) use self::instance::MdSanInstanceManager;

mod associated_types {
    use super::*;

    pub(super) type MdSanPlaceInfo = place::PlaceWithMetadata;
    pub(super) type MdSanPlaceValue = state::PlaceValue2;
    pub(super) type MdSanValue = state::Value;

    pub(super) type MdSanPlaceBuilder = place::MdSanPlaceBuilder;
    pub(super) type MdSanPlaceHandler<'a> = place::MdSanPlaceHandler<'a>;
    pub(super) type MdSanOperandHandler<'a> = operand::MdSanOperandHandler<'a>;

    pub(super) type MdSanAssignmentHandler<'a> = assignment::MdSanAssignmentHandler<'a, 'a>;

    pub(super) type MdSanMemoryHandler<'a> = state::MdSanMemoryHandler<'a>;
    pub(super) type MdSanRawMemoryHandler<'a> = memory::MdSanRawMemoryHandler<'a>;

    pub(super) type MdSanCallFlowManager = call::MdSanCallFlowManager;
    pub(super) type MdSanCallHandler<'a> = call::MdSanCallHandler<'a>;

    pub(super) type MdSanVariablesState = state::RawPointerVariableState;
    pub(super) type MdSanPlaceInspector<'a> = state::DefaultPlaceInspector<'a>;

    pub(super) type MdSanTypeManager = dyn TypeDatabase;
}
use associated_types::*;

pub(crate) struct MdSanBackend {
    vars_state: MdSanVariablesState,
    vars_state_factory: Box<dyn Fn() -> MdSanVariablesState>,
    call_flow_manager: MdSanCallFlowManager,
    type_manager: Rc<MdSanTypeManager>,
}

impl MdSanBackend {
    pub(crate) fn new(types_db: impl crate::type_info::TypeDatabase<'static> + 'static) -> Self {
        let type_manager_ref = Rc::new(types_db);
        let type_manager = type_manager_ref.clone();

        let vars_state_factory =
            Box::new(move || MdSanVariablesState::new(type_manager_ref.clone()));

        Self {
            vars_state: vars_state_factory(),
            vars_state_factory,
            call_flow_manager: call::default_flow_manager(),
            type_manager,
        }
    }
}

impl RuntimeBackend for MdSanBackend {
    type PlaceHandler<'a>
        = MdSanPlaceHandler<'a>
    where
        Self: 'a;

    type OperandHandler<'a>
        = MdSanOperandHandler<'a>
    where
        Self: 'a;

    type AssignmentHandler<'a>
        = MdSanAssignmentHandler<'a>
    where
        Self: 'a;

    type MemoryHandler<'a>
        = MdSanMemoryHandler<'a>
    where
        Self: 'a;

    type RawMemoryHandler<'a>
        = MdSanRawMemoryHandler<'a>
    where
        Self: 'a;

    type ConstraintHandler<'a>
        = NoOpConstraintHandler<Self::Operand>
    where
        Self: 'a;

    type CallHandler<'a>
        = MdSanCallHandler<'a>
    where
        Self: 'a;

    type DropHandler<'a>
        = MdSanCallHandler<'a>
    where
        Self: 'a;

    type AnnotationHandler<'a>
        = NoOpAnnotationHandler
    where
        Self: 'a;

    type PlaceInfo = MdSanPlaceInfo;

    type Place = MdSanPlaceValue;

    type DiscriminablePlace = Self::Place;

    type Operand = MdSanValue;

    fn place(&mut self, usage: crate::abs::PlaceUsage) -> Self::PlaceHandler<'_> {
        MdSanPlaceHandler::new(usage, self)
    }

    fn operand(&mut self) -> Self::OperandHandler<'_> {
        MdSanOperandHandler::new(self)
    }

    fn assign_to<'a>(
        &'a mut self,
        id: common::pri::AssignmentId,
        dest: <Self::AssignmentHandler<'a> as crate::pri::fluent::backend::AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a> {
        MdSanAssignmentHandler::new(id, dest, self)
    }

    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a> {
        MdSanMemoryHandler::new(self)
    }

    fn raw_memory<'a>(&'a mut self) -> Self::RawMemoryHandler<'a> {
        MdSanRawMemoryHandler::new(self)
    }

    fn constraint_at<'a>(
        &'a mut self,
        _location: common::pri::BasicBlockIndex,
    ) -> Self::ConstraintHandler<'a> {
        Default::default()
    }

    fn call_control(&mut self) -> Self::CallHandler<'_> {
        MdSanCallHandler::new(self)
    }

    fn dropping(&mut self) -> Self::DropHandler<'_> {
        MdSanCallHandler::new(self)
    }

    fn annotate(&mut self) -> Self::AnnotationHandler<'_> {
        Default::default()
    }
}

impl Shutdown for MdSanBackend {
    fn shutdown(&mut self) {
        // Nothing to do for now.
    }
}

trait MdMemoryState {
    type PlaceInfo;
    type PlaceValue;
    type ToInspectPlaceValue;
    type ToTakePlaceValue;
    type ToSetPlaceValue;
    type ToUpdatePlaceValue;
    type ToErasePlaceValue;
    type ValueForAddress;
    type Value;

    fn ref_place(&self, place: Self::PlaceInfo, usage: PlaceUsage) -> Self::PlaceValue;

    fn ref_place_by_ptr(
        &self,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::PlaceValue;

    fn peek_place(&self, place: &Self::ToInspectPlaceValue) -> Option<&Self::ValueForAddress>;

    fn take_place(&mut self, place: &Self::ToTakePlaceValue) -> Self::Value;

    fn set_place(&mut self, place: &Self::ToSetPlaceValue, value: Self::Value);

    fn update_place(
        &mut self,
        place: &Self::ToUpdatePlaceValue,
        value: Self::ValueForAddress,
    ) -> bool;

    fn erase_place(&mut self, place: &Self::ToErasePlaceValue);
}

trait MdTypeProvider {
    fn is_md_type(&self, ty: &common::type_info::TypeInfo) -> bool;

    fn is_md_container_type2(&self, type_id: TypeId) -> bool;

    fn is_md_type2(&self, type_id: TypeId) -> bool;

    fn is_md_wrapped_type(&self, type_id: TypeId) -> bool;
}

trait PlaceInspector {
    fn inspect_place_for_access(&self, place: &MdSanPlaceValue);
}
