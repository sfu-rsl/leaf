mod alias;
mod annotation;
mod assignment;
mod call;
mod concrete;
mod config;
mod constraint;
mod expr;
mod implication;
mod instance;
mod memory;
mod operand;
mod outgen;
mod place;
mod state;
mod sym_vars;
mod trace;
mod type_info;

use std::{cell::RefCell, rc::Rc};

use common::{
    log_info,
    pri::{AssignmentId, BasicBlockIndex, FieldIndex},
    types::InstanceKindId,
};

use crate::{
    abs::{
        PlaceUsage, PointerOffset, SymVariable, Tag, TypeId, TypeSize, VariantIndex, backend::*,
    },
    backends::basic::place::DiscriminantPossiblePlace,
    pri::fluent::backend::*,
    utils::{HasIndex, RefView, alias::RRef},
};

use self::{
    alias::{TraceManager, TypeDatabase, VariablesState},
    expr::{SymVarId, prelude::*},
    implication::{Antecedents, Implied, Precondition},
    state::make_sym_place_handler,
    trace::default_trace_querier,
};

pub(crate) use self::{config::SymExBackendConfig, instance::SymExInstanceManager};

mod associated_types {
    use super::*;

    pub(super) type SymExPlaceInfo = place::PlaceWithMetadata;
    pub(super) type SymExPlaceValue = PlaceValueRef;
    pub(super) type SymExValue = Implied<ValueRef>;

    pub(super) type SymExPlaceBuilder = place::SymExPlaceBuilder;
    pub(super) type SymExPlaceHandler<'a> = place::SymExPlaceHandler<'a>;
    pub(super) type SymExOperandHandler<'a> = operand::SymExOperandHandler<'a>;

    pub(super) type SymExVariablesState = alias::SymExVariablesState;
    pub(super) type SymExSymPlaceHandler = alias::SymExSymPlaceHandler;

    pub(super) type SymExExprBuilder = alias::DefaultExprBuilder;

    pub(super) type SymExAssignmentHandler<'a> =
        assignment::SymExAssignmentHandler<'a, 'a, SymExExprBuilder>;
    #[cfg(feature = "implicit_flow")]
    pub(super) type SymExImplicationInvestigator = dyn ImplicationInvestigator;
    pub(super) type SymExMemoryHandler<'a> = state::SymExMemoryHandler<'a>;
    pub(super) type SymExRawMemoryHandler<'a> = memory::SymExRawMemoryHandler<'a, SymExExprBuilder>;

    pub(super) type SymExSymVariablesManager = sym_vars::DefaultSymVariablesManager;
    pub(super) type SymExConcretizer<EB> = concrete::DefaultConcretizer<EB>;

    pub(super) type SymExConstraint = constraint::Constraint;
    pub(super) type SymExConstraintDecisionCase = constraint::DecisionCase;

    pub(super) type SymExConstraintHandler<'a> =
        constraint::SymExConstraintHandler<'a, SymExExprBuilder>;

    pub(super) type SymExCallFlowManager = call::SymExCallFlowManager;
    pub(super) type SymExCallHandler<'a> = call::SymExCallHandler<'a>;

    pub(super) type SymExAnnotationHandler<'a> = annotation::SymExAnnotationHandler<'a>;

    pub(super) type SymExTraceManager = dyn TraceManager;
    pub(super) type SymExExeTraceRecorder = trace::SymExExeTraceRecorder;
    pub(super) type SymExTypeManager = dyn TypeDatabase;
}
use associated_types::*;

pub struct SymExBackend {
    vars_state: SymExVariablesState,
    call_flow_manager: SymExCallFlowManager,
    vars_state_factory: Box<dyn Fn() -> SymExVariablesState>,
    trace_manager: RRef<SymExTraceManager>,
    trace_recorder: RRef<SymExExeTraceRecorder>,
    expr_builder: RRef<SymExExprBuilder>,
    sym_values: RRef<SymExSymVariablesManager>,
    type_manager: Rc<SymExTypeManager>,
    sym_place_handler: RRef<SymExSymPlaceHandler>,
    #[cfg(feature = "implicit_flow")]
    implication_investigator: Rc<SymExImplicationInvestigator>,
    tags: RRef<Vec<Tag>>,
}

impl SymExBackend {
    pub fn new(
        config: SymExBackendConfig,
        types_db: impl crate::type_info::TypeDatabase<'static> + 'static,
    ) -> Self {
        let type_manager_ref = Rc::new(type_info::default_type_manager(types_db));
        let expr_builder_ref = Rc::new(RefCell::new(expr::builders::new_expr_builder(
            type_manager_ref.clone(),
        )));
        let expr_builder = expr_builder_ref.clone();
        let sym_var_manager = Rc::new(RefCell::new(SymExSymVariablesManager::default()));

        let tags_ref = Rc::new(RefCell::new(Vec::new()));

        let type_manager = type_manager_ref.clone();

        let trace_recorder_ref = Rc::new(RefCell::new(trace::create_trace_recorder(
            config.exe_trace.control_flow_dump.as_ref(),
        )));

        let trace_manager = trace::create_trace_manager(
            trace_recorder_ref.clone(),
            tags_ref.clone(),
            sym_var_manager.clone(),
            &config.exe_trace,
            &config.outputs,
            &config.solver,
        );
        #[cfg(feature = "implicit_flow")]
        let implication_investigator = {
            let constraint_steps = TraceViewProvider::view(&trace_manager);
            let constraints = TraceViewProvider::<SymExConstraint>::view(&trace_manager);
            let sym_dependent_step_indices =
                TraceIndicesProvider::<trace::SymDependentMarker>::indices(&trace_manager);
            let trace_querier = Rc::new(default_trace_querier(
                trace_recorder_ref.borrow().records(),
                constraint_steps,
                constraints,
                sym_dependent_step_indices,
            ));
            Rc::new(implication::default_implication_investigator(trace_querier))
        };
        let trace_manager_ref = Rc::new(RefCell::new(trace_manager));

        let sym_place_handler_factory = |s| {
            Rc::new(RefCell::from(make_sym_place_handler(s, || {
                Box::new(SymExConcretizer::new(
                    expr_builder_ref.clone(),
                    trace_manager_ref.clone(),
                ))
            })))
        };
        let sym_read_handler_ref = sym_place_handler_factory(config.sym_place.read);
        let sym_write_handler_ref = sym_place_handler_factory(config.sym_place.write);
        // Writes are more difficult, and the handler is usually more restrictive, so we use the write handler as the general one.
        let sym_place_handler = sym_write_handler_ref.clone();

        let variables_state_factory = Box::new(move || {
            SymExVariablesState::new(
                type_manager_ref.clone(),
                sym_read_handler_ref.clone(),
                sym_write_handler_ref.clone(),
                Rc::new(RefCell::new(expr::builders::to_sym_expr_builder(
                    expr_builder_ref.clone(),
                ))),
            )
        });

        Self {
            call_flow_manager: call::default_flow_manager(config.call),
            vars_state: variables_state_factory(),
            vars_state_factory: variables_state_factory,
            trace_manager: trace_manager_ref.clone(),
            trace_recorder: trace_recorder_ref.clone(),
            expr_builder: Rc::new(RefCell::new(expr::builders::to_implied_expr_builder(
                expr_builder,
            ))),
            sym_values: sym_var_manager.clone(),
            type_manager,
            sym_place_handler,
            #[cfg(feature = "implicit_flow")]
            implication_investigator,
            tags: tags_ref.clone(),
        }
    }
}

impl RuntimeBackend for SymExBackend {
    type PlaceHandler<'a>
        = SymExPlaceHandler<'a>
    where
        Self: 'a;

    type OperandHandler<'a>
        = SymExOperandHandler<'a>
    where
        Self: 'a;

    type AssignmentHandler<'a>
        = SymExAssignmentHandler<'a>
    where
        Self: 'a;

    type MemoryHandler<'a>
        = SymExMemoryHandler<'a>
    where
        Self: 'a;

    type RawMemoryHandler<'a>
        = SymExRawMemoryHandler<'a>
    where
        Self: 'a;

    type ConstraintHandler<'a>
        = SymExConstraintHandler<'a>
    where
        Self: 'a;

    type CallHandler<'a>
        = SymExCallHandler<'a>
    where
        Self: 'a;

    type DropHandler<'a>
        = SymExCallHandler<'a>
    where
        Self: 'a;

    type AnnotationHandler<'a>
        = SymExAnnotationHandler<'a>
    where
        Self: 'a;

    type PlaceInfo = SymExPlaceInfo;
    type Place = PlaceValueRef;
    type DiscriminablePlace = DiscriminantPossiblePlace;
    type Operand = SymExValue;

    fn place(&mut self, usage: PlaceUsage) -> Self::PlaceHandler<'_> {
        SymExPlaceHandler::new(usage, self)
    }

    fn operand(&mut self) -> Self::OperandHandler<'_> {
        SymExOperandHandler::new(self)
    }

    fn assign_to<'a>(
        &'a mut self,
        id: AssignmentId,
        dest: <Self::AssignmentHandler<'a> as AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a> {
        SymExAssignmentHandler::new(id, dest, self)
    }

    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a> {
        SymExMemoryHandler::new(self)
    }

    fn raw_memory<'a>(&'a mut self) -> Self::RawMemoryHandler<'a> {
        SymExRawMemoryHandler::new(self)
    }

    fn constraint_at(&mut self, location: BasicBlockIndex) -> Self::ConstraintHandler<'_> {
        SymExConstraintHandler::new(self, location)
    }

    fn call_control(&mut self) -> Self::CallHandler<'_> {
        SymExCallHandler::new(self)
    }

    fn dropping(&mut self) -> Self::DropHandler<'_> {
        SymExCallHandler::new(self)
    }

    fn annotate(&mut self) -> Self::AnnotationHandler<'_> {
        SymExAnnotationHandler::new(self)
    }
}

impl Shutdown for SymExBackend {
    fn shutdown(&mut self) {
        log_info!("Shutting down the backend");
        self.trace_manager.borrow_mut().shutdown();
    }
}

trait SymVariablesManager {
    fn add_variable(&mut self, var: SymVariable<SymExValue>) -> SymValueRef;

    fn iter_variables(
        &self,
    ) -> impl ExactSizeIterator<Item = (&SymVarId, &SymValueRef, &ConcreteValueRef)>;

    fn iter_concretization_constraints(
        &self,
    ) -> impl ExactSizeIterator<Item = (&SymVarId, &crate::abs::Constraint<SymValueRef, ConstValue>)>;
}

trait GenericVariablesState {
    type PlaceInfo;
    type PlaceValue;
    type Value;

    fn id(&self) -> usize;

    /// Returns a value that corresponds to the place itself.
    /// The returned value does not necessarily access the actual value but
    /// should be dereferenceable to get the actual value.
    fn ref_place(&self, place: &Self::PlaceInfo, usage: PlaceUsage) -> Self::PlaceValue;

    /// Returns a value that corresponds to the place pointer by the pointer.
    /// Effectively, this is equivalent to the place that would be represented by `*ptr`.
    fn ref_place_by_ptr(
        &self,
        ptr: Self::Value,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::PlaceValue;

    /// Returns a copy of the value stored at the given place. May not physically copy the value
    /// but the returned value should be independently usable from the original value.
    fn copy_place(&self, place: &Self::PlaceValue) -> Self::Value;

    /// Returns the value stored at the given place.
    /// Conceptually, it is required that the place will not contain the value right after this operation.
    fn take_place(&mut self, place: &Self::PlaceValue) -> Self::Value;

    /// Sets the value of a place. Overwrites the previous value if any, also defines a new local
    /// variable if it does not exist.
    fn set_place(&mut self, place: &Self::PlaceValue, value: Self::Value);

    fn drop_place(&mut self, place: &Self::PlaceValue);
}

trait ExeTraceStorage {
    type Record;

    fn records(&self) -> RefView<Vec<Self::Record>>;
}

trait TraceViewProvider<T> {
    fn view(&self) -> RefView<Vec<T>>;
}

trait TraceIndicesProvider<T> {
    fn indices(&self) -> RefView<Vec<usize>>;
}

trait GenericTraceQuerier {
    type Record;
    type Constraint;

    fn any_sym_dependent_in_current_call(&self, body_id: InstanceKindId) -> bool;

    fn find_map_in_current_func<'a, T>(
        &'a self,
        body_id: InstanceKindId,
        f: impl FnMut(BasicBlockIndex, &Self::Constraint) -> Option<T>,
    ) -> Option<(
        impl AsRef<BasicBlockIndex> + HasIndex + AsRef<Self::Constraint>,
        T,
    )>;
}

#[derive(Debug)]
struct EnumAntecedentsResult {
    tag: Antecedents,
    fields: Option<Antecedents>,
}

#[cfg(feature = "implicit_flow")]
trait ImplicationInvestigator {
    fn antecedent_of_latest_assignment(
        &self,
        assignment_id: (InstanceKindId, AssignmentId),
    ) -> Option<Antecedents>;

    fn antecedent_of_latest_enum_assignment(
        &self,
        assignment_id: (InstanceKindId, AssignmentId),
    ) -> Option<EnumAntecedentsResult>;
}

trait TypeLayoutResolver<'t> {
    fn resolve_array_elements(
        &self,
        type_id: TypeId,
    ) -> (TypeId, impl Iterator<Item = (PointerOffset, TypeSize)> + 't);

    /// # Remarks
    /// The items will be emitted in the order of the field offsets.
    fn resolve_adt_fields(
        &self,
        type_id: TypeId,
        variant: Option<VariantIndex>,
    ) -> impl Iterator<Item = (FieldIndex, TypeId, PointerOffset, TypeSize)> + 't;
}
