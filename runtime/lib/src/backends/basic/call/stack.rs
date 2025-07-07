use core::iter;

use crate::{
    abs::{CalleeDef, Constant, FieldIndex, FuncDef},
    backends::basic::CallStackInfo,
    utils::InPlaceSelfHierarchical,
};

use crate::backends::basic as backend;
use backend::{
    BasicValue, ConcreteValue, Implied, VariablesState,
    config::{CallConfig, ExternalCallStrategy},
    expr::prelude::DeterPlaceValueRef,
};

use super::{ArgsTuplingInfo, BasicUntupleHelper, CallFlowSanity, GenericCallStackManager};

use common::{log_debug, log_trace, log_warn, types::RawAddress};

type VariablesStateFactory<VS> = Box<dyn Fn(usize) -> VS>;

use self::logging::TAG;

/* Here is what happens during function calls:
 * +---------------+---------------------------+
 * | Caller        | Callee                    |
 * +---------------+---------------------------+
 * | before_call() |                           |
 * |               | enter()                   |
 * |               | [override_return_value()] |
 * |               | return()                  |
 * | after_call()  |                           |
 * +---------------+---------------------------+
 * Or equivalently (names are changed to better reflect the current position of program):
 * +--------------------+---------------------------+--------------------------------+
 * | Caller             | Callee                    | Log Span                       |
 * +--------------------+---------------------------+--------------------------------+
 * | prepare_for_call() |                           | depth = n, trans: dir = "call" |
 * |                    | set_places()              |                                |
 * |                    | [try_untuple_argument()]  |                                |
 * |                    | notify_enter()            | __                             |
 * |                    | [override_return_value()] | depth = n + 1                  |
 * |                    | pop_stack_frame()         | depth = n, trans: dir = "ret"  |
 * | finalize_call()    |                           | depth = n                      |
 * +--------------------+---------------------------+--------------------------------+
 * Log spans:
 * +---------------------------+--------------------------------+
 * | Function                  | Log Span                       |
 * +---------------------------+--------------------------------+
 * |                           | __ depth = n                   |
 * | prepare_for_call()        | depth = n, trans: dir = "call" |
 * | set_places()              |                                |
 * | [try_untuple_argument()]  |                                |
 * | notify_enter()            | __                             |
 * |                           | depth = n + 1                  |
 * | [override_return_value()] | __                             |
 * | pop_stack_frame()         | depth = n, trans: dir = "ret"  |
 * | finalize_call()           | depth = n                      |
 * +---------------------------+--------------------------------+
 */

/* Functionality Specification:
 * - `latest_call` holds the information from the caller sent to the callee.
 *   It is set in `prepare_for_call` and consumed in `notify_enter()` or cleared in `finalize_external_call`.
 * - `latest_returned_val` holds the latest returned value, not necessarily the called function's
 *   return value (look at external functions).
 * - `args_metadata` and `return_val_metadata` are expected to be set before `notify_enter`
 *   and get consumed by it.
 */
/* How are external function calls detected/handled?
 * (internal = instrumented, external = not instrumented)
 * It is important to consider both scenarios where an external function is in the programs call stack.
 * 1. When the external function is called from an internal one.
 * 2. When the external function calls an internal one.
 *
 * Detection is handled by setting a function id on the caller side and comparing it with the current
 * function id on the callee side.
 * Specification:
 * - Using `latest_call.expected_func` to store the information.
 * - `is_callee_external` is stack-based and works in pessimistic mode, i.e., it is set to true if
 *   the function id is the same as the expected function id, otherwise remains unset or is set to false.
 * Scenarios:
 * - internal -> external:
 *   When returned to the caller, `is_callee_external` is unset.
 * - external -> internal:
 *   When entered the callee, the expected function id does not match and
 *   `is_callee_external` is set to false.
 *   - internal -> external -> internal
 *   When returned to the first caller, `is_callee_external` is false.
 *
 * Return Value
 * The return value is set based on the external call strategy.
 * Any existing return value is from an internal function called by the external function
 * and thus should be discarded.
 */

pub(crate) struct BasicCallStackManager<VS: VariablesState> {
    /// The call stack. Each frame consists of the data that is held for the
    /// current function call and is preserved through calls and returns.
    stack: Vec<CallStackFrame<VS::Value>>,
    vars_state_factory: VariablesStateFactory<VS>,
    /// The data passed between from the call point (in the caller)
    /// to the entrance point (in the callee).
    latest_call: Option<CallInfo<VS::Value>>,
    arg_places: Vec<DeterPlaceValueRef>,
    return_val_place: Option<DeterPlaceValueRef>,
    /// The data (return value) passed between the exit point (in the callee)
    /// to the return point (in the caller).
    latest_returned_val: Option<VS::Value>,
    vars_state: VS,
    config: CallConfig,
    log_span: tracing::span::EnteredSpan,
}

pub(super) struct CallStackFrame<V> {
    /* this doesn't refer to the current stack frame,
     * but the function that is about to be / was just called by this function.
     */
    is_callee_external: Option<bool>,
    /// The return value forced by a call to `override_return_value`.
    /// If it is set in an internal call, it will be consumed as the returned
    /// value of the current function when popping the frame.
    /// If it is set in an external call, it will be consumed as the returned
    /// value from the external call when storing the returned value in the
    /// destination variable.
    overridden_return_val: Option<V>,
    return_val_place: Option<DeterPlaceValueRef>,
    def: Option<FuncDef>,
}

impl<V> Default for CallStackFrame<V> {
    fn default() -> Self {
        Self {
            is_callee_external: Default::default(),
            overridden_return_val: Default::default(),
            return_val_place: Default::default(),
            def: Default::default(),
        }
    }
}

#[derive(Debug)]
pub(super) struct CallInfo<V> {
    expected_func: CalleeDef,
    args: Vec<V>,
    are_args_tupled: bool,
}

impl<VS: VariablesState> BasicCallStackManager<VS> {
    pub(crate) fn new(vars_state_factory: VariablesStateFactory<VS>, config: &CallConfig) -> Self {
        let log_span = tracing::Span::none().entered();
        Self {
            stack: vec![],
            vars_state: vars_state_factory(0),
            vars_state_factory,
            latest_call: None,
            arg_places: vec![],
            return_val_place: None,
            latest_returned_val: None,
            config: config.clone(),
            log_span,
        }
    }
}

impl<VS: VariablesState + InPlaceSelfHierarchical> BasicCallStackManager<VS> {
    fn push_new_stack_frame(
        &mut self,
        args: impl Iterator<Item = (DeterPlaceValueRef, VS::Value)>,
        frame: CallStackFrame<VS::Value>,
    ) {
        self.vars_state.add_layer();
        for (local, value) in args {
            self.vars_state.set_place(local.as_ref(), value);
        }

        self.stack.push(frame);
        self.log_span_reset();
    }

    fn top_frame(&mut self) -> &mut CallStackFrame<VS::Value> {
        self.stack
            .last_mut()
            .expect("Call stack should not be empty")
    }

    fn finalize_internal_call(&mut self) -> VS::Value {
        if let Some(returned_val) = self.latest_returned_val.take() {
            returned_val
        } else {
            // The unit return type
            Implied::always(ConcreteValue::from(Constant::Zst).to_value_ref())
        }
    }

    fn finalize_external_call(&mut self) -> VS::Value {
        // Clearing the data that is not cleared by the external function.
        let latest_call = self.latest_call.take();
        let symbolic_args = latest_call
            .as_ref()
            .map(|info| self.inspect_external_call_info(&info.args));
        self.latest_returned_val
            .take()
            .inspect(|val| self.inspect_returned_value(val));

        if let Some(overridden) = self.top_frame().overridden_return_val.take() {
            log_debug!(
                target: TAG,
                concat!(
                "Consuming the overridden return value as the returned value ",
                "from the external function."
                ),
            );
            return overridden;
        }

        enum Action {
            Concretize,
            OverApproximate,
        }
        use Action::*;

        let action = match self.config.external_call {
            ExternalCallStrategy::Panic => panic!("External function call detected."),
            ExternalCallStrategy::Concretization => Concretize,
            ExternalCallStrategy::OverApproximation => OverApproximate,
            ExternalCallStrategy::OptimisticConcretization => {
                /* NOTE: What is optimistic here?
                 * It correspond to the optimistic assumption that the callee has been a
                 * pure function and no symbolic input results in no symbolic output. */
                /* FIXME: With the current implementation, references to symbolic values
                 * skip this check. */
                if !symbolic_args.map(|args| !args.is_empty()).unwrap_or(false) {
                    Concretize
                } else {
                    OverApproximate
                }
            }
        };
        match action {
            Concretize => unknown_value(),
            OverApproximate => {
                todo!("#306: Over-approximated symbolic values are not supported.")
            }
        }
    }

    fn resolve_tupling(&mut self, args: &mut PassedArgs<VS::Value>, tupling: ArgsTuplingInfo) {
        match tupling {
            ArgsTuplingInfo::Untupled {
                tupled_arg_index: arg_index,
                tupling_helper,
            } if args.are_tupled => {
                core::hint::cold_path();
                let arg_index = arg_index as usize - 1;
                log_debug!(target: TAG, "Untupling argument at index {}", arg_index);
                let tupled_value = args.values.remove(arg_index);
                let untupled_args = Self::untuple(
                    tupled_value,
                    self.arg_places.get(arg_index).map(|m| m.address()),
                    tupling_helper().as_mut(),
                    (self.vars_state_factory)(usize::MAX),
                );
                // Replace the tupled argument with separate ones.
                args.values.splice(arg_index..arg_index, untupled_args);
            }
            ArgsTuplingInfo::Tupled {
                head_args,
                tupling_helper,
            } if !args.are_tupled => {
                core::hint::cold_path();
                log_debug!(target: TAG, "Tupling arguments");
                // Replace the separate arguments with a tupled one.
                let separated_args = core::mem::replace(&mut args.values, head_args());
                args.values.push(Self::tuple(
                    separated_args,
                    self.arg_places.get(args.values.len()).map(|m| m.address()),
                    tupling_helper().as_mut(),
                    (self.vars_state_factory)(usize::MAX),
                ));
            }
            _ => {
                log_trace!(
                    target: TAG,
                    "No tupling/untupling needed {:?}, {}",
                    tupling,
                    args.are_tupled,
                );
            }
        }
    }

    fn untuple(
        tupled_value: VS::Value,
        tupled_arg_addr: Option<RawAddress>,
        untuple_helper: &mut (impl BasicUntupleHelper + ?Sized),
        isolated_vars_state: VS,
    ) -> Vec<VS::Value> {
        let tupled_pseudo_place = untuple_helper.make_tupled_arg_pseudo_place(
            /* NOTE: The address should not really matter, but let's keep it realistic. */
            tupled_arg_addr.unwrap_or(core::ptr::null() as RawAddress),
        );

        // Write the value to the pseudo place in an isolated state, then read the fields
        let mut vars_state = isolated_vars_state;
        let num_fields = untuple_helper.num_fields();
        vars_state.set_place(tupled_pseudo_place.as_ref(), tupled_value);
        // Read the fields (values inside the tuple) one by one.
        (0..num_fields)
            .into_iter()
            .map(|i| untuple_helper.field_place(tupled_pseudo_place.clone(), i))
            .map(|arg_place| vars_state.take_place(arg_place.as_ref()))
            .collect()
    }

    fn tuple(
        separate_values: Vec<VS::Value>,
        tupled_arg_addr: Option<RawAddress>,
        untuple_helper: &mut (impl BasicUntupleHelper + ?Sized),
        isolated_vars_state: VS,
    ) -> VS::Value {
        let tupled_pseudo_place = untuple_helper.make_tupled_arg_pseudo_place(
            /* NOTE: The address should not really matter, but let's keep it realistic. */
            tupled_arg_addr.unwrap_or(core::ptr::null() as RawAddress),
        );

        // Write the values to the field pseudo places in an isolated state, then read the tuple
        let mut vars_state = isolated_vars_state;
        debug_assert_eq!(separate_values.len(), untuple_helper.num_fields() as usize,);
        separate_values
            .into_iter()
            .enumerate()
            .for_each(|(i, value)| {
                vars_state.set_place(
                    untuple_helper
                        .field_place(tupled_pseudo_place.clone(), i as FieldIndex)
                        .as_ref(),
                    value,
                )
            });
        // Read the whole tuple.
        vars_state.take_place(tupled_pseudo_place.as_ref())
    }

    fn inspect_external_call_info<'a>(
        &self,
        arg_values: &'a [VS::Value],
    ) -> Vec<(usize, &'a VS::Value)> {
        let symbolic_args: Vec<_> = arg_values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_symbolic())
            .collect();
        if !symbolic_args.is_empty() {
            log_warn!(
                target: TAG,
                "Possible loss of symbolic arguments in external function call",
            );
            log_debug!(
                target: TAG,
                "Symbolic arguments passed to the function: {:?}",
                symbolic_args,
            );
        }
        symbolic_args
    }

    fn inspect_returned_value<'a>(&self, returned_value: &VS::Value) {
        if returned_value.is_symbolic() {
            log_warn!(
                target: TAG,
                "Possible loss of symbolic return value in external function call",
            );
            log_debug!(
                target: TAG,
                "Symbolic returned value from a function: {:?}",
                returned_value,
            );
        }
    }
}

impl<VS: VariablesState + InPlaceSelfHierarchical> CallStackInfo for BasicCallStackManager<VS> {
    type VariablesState = VS;

    fn top(&mut self) -> &mut VS {
        &mut self.vars_state
    }

    fn current_func(&self) -> FuncDef {
        self.stack
            .last()
            .and_then(|f| f.def.clone())
            .expect("Current function is not set")
    }
}

impl<VS: VariablesState + InPlaceSelfHierarchical> GenericCallStackManager
    for BasicCallStackManager<VS>
{
    type Place = DeterPlaceValueRef;

    fn prepare_for_call(
        &mut self,
        def: CalleeDef,
        func: Self::Value,
        args: Vec<Self::Value>,
        are_args_tupled: bool,
    ) {
        self.log_span_start_trans(logging::TransitionDirection::Call);

        // Some sanity checks
        let is_currently_clean = self.latest_call.is_none()
            && self.arg_places.is_empty()
            && self.return_val_place.is_none();
        debug_assert!(
            is_currently_clean,
            concat!(
                "The call information is not consumed or cleaned correctly. ",
                "This is due to a problem in external function handling or instrumentation. ",
                "`latest_call`: {:?}, ",
                "`args_metadata`: {:?}, ",
                "`return_val_place`: {:?}",
            ),
            self.latest_call, self.arg_places, self.return_val_place,
        );

        if func.is_symbolic() {
            log_warn!("Calling a symbolic function {:?}", func);
        }

        self.latest_call = Some(CallInfo {
            expected_func: def,
            args,
            are_args_tupled,
        });
    }

    fn set_places(&mut self, arg_places: Vec<Self::Place>, ret_val_place: Self::Place) {
        self.arg_places = arg_places;
        self.return_val_place = Some(ret_val_place);
    }

    fn start_enter(&mut self, current_func: FuncDef) -> EntranceToken<Self::Value> {
        let arg_places = core::mem::replace(&mut self.arg_places, Default::default());

        let arg_values_if_not_broken = if let Some(call_info) = self.latest_call.take() {
            if current_func == call_info.expected_func {
                log_trace!(
                    target: TAG,
                    "Entering the function: {} with expected function: {}",
                    current_func, call_info.expected_func,
                );
                Ok(PassedArgs {
                    values: call_info.args,
                    are_tupled: call_info.are_args_tupled,
                })
            } else {
                log_debug!(
                    target: TAG,
                    "External function call detected. Expected: {} but got: {}",
                    call_info.expected_func,
                    current_func,
                );
                Err(Some(PassedArgs {
                    values: call_info.args,
                    are_tupled: call_info.are_args_tupled,
                }))
            }
        } else {
            // NOTE: An example of this case is when an external function calls multiple internal ones.
            log_debug!(target: TAG, "External function call detected with no call information available");
            Err(None)
        };

        EntranceToken {
            entered_func_def: current_func,
            arg_places,
            return_val_place: self
                .return_val_place
                .take()
                .expect("Return value place must be set for instrumented functions."),
            passed_args: arg_values_if_not_broken,
        }
    }

    fn finalize_enter<'a, 'h>(
        &'a mut self,
        EntranceToken {
            entered_func_def,
            arg_places,
            return_val_place,
            passed_args,
        }: EntranceToken<Self::Value>,
        tupling: Box<dyn FnOnce() -> ArgsTuplingInfo<'h, 'a> + 'a>,
    ) -> CallFlowSanity {
        let is_broken = passed_args.is_err();
        if let Some(parent_frame) = self.stack.last_mut() {
            parent_frame.is_callee_external = Some(is_broken);
        }

        let arg_values = match passed_args {
            Ok(mut args) => {
                self.resolve_tupling(&mut args, tupling());
                assert_eq!(
                    args.values.len(),
                    arg_places.len(),
                    "Inconsistent number of passed arguments."
                );
                args.values
            }
            Err(call_info) => {
                call_info.inspect(|info| {
                    self.inspect_external_call_info(&info.values);
                });
                iter::repeat_n(unknown_value(), arg_places.len()).collect()
            }
        };

        let call_stack_frame = CallStackFrame {
            def: Some(entered_func_def),
            return_val_place: Some(return_val_place),
            ..Default::default()
        };
        self.push_new_stack_frame(arg_places.into_iter().zip(arg_values), call_stack_frame);

        log_debug!(target: TAG, "Entered the function");

        if is_broken {
            CallFlowSanity::Broken
        } else {
            CallFlowSanity::Expected
        }
    }

    fn pop_stack_frame(&mut self) {
        self.vars_state.drop_layer().or_else(|| {
            if self.stack.len() == 0 {
                log_warn!(target: TAG, "The initial call stack was getting popped.");
            }
            None
        });

        let popped_frame = self.stack.pop().unwrap();

        self.log_span_reset();
        if self
            .stack
            .last()
            .is_some_and(|f| !f.is_callee_external.unwrap())
        {
            self.log_span_start_trans(logging::TransitionDirection::Return);
        }

        let return_val = self
            .top()
            .take_place(&popped_frame.return_val_place.unwrap().into());
        self.latest_returned_val
            .replace(return_val)
            .inspect(|old_unconsumed_value| self.inspect_returned_value(old_unconsumed_value));
        if let Some(overridden) = popped_frame.overridden_return_val {
            if self.latest_returned_val.is_some() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "The return value is overridden while an actual value was available. ",
                        "This may not be intended."
                    )
                );
            }
            self.latest_returned_val = Some(overridden);
        }
    }

    fn finalize_call(&mut self) -> (Self::Value, CallFlowSanity) {
        self.log_span_reset();

        let is_external = self
            .top_frame()
            .is_callee_external
            .take()
            .unwrap_or_else(|| {
                log_debug!(target: TAG, "External function call detected with no acknowledged entrance.");
                true
            });

        let return_val = if !is_external {
            log_debug!(target: TAG, "Finalizing an internal call.");
            self.finalize_internal_call()
        } else {
            log_debug!(target: TAG, "Finalizing an external call.");
            self.finalize_external_call()
        };

        let sanity = if is_external {
            CallFlowSanity::Broken
        } else {
            CallFlowSanity::Expected
        };

        (return_val, sanity)
    }

    fn override_return_value(&mut self, value: Self::Value) {
        log_debug!(target: TAG, "Overriding the return value with {:?}", value);
        self.top_frame().overridden_return_val = Some(value);
    }
}

fn unknown_value() -> BasicValue {
    Implied::by_unknown(ConcreteValue::from(Constant::Some).to_value_ref())
}

impl PartialEq<CalleeDef> for FuncDef {
    fn eq(&self, other: &CalleeDef) -> bool {
        if self.static_addr == other.static_addr {
            return true;
        }

        if self
            .as_dyn_method
            .zip(other.as_virtual)
            .is_some_and(|(s, o)| s == o)
        {
            return true;
        }

        false
    }
}

mod logging {
    use super::*;

    use const_format::concatcp;
    use tracing::{Span, debug_span};

    pub(super) const TAG: &str = "call_manager";
    const TAG_STACK: &str = concatcp!(TAG, "::stack");
    const SPAN_CALL: &str = "call";
    const SPAN_TRANSITION: &str = "call_trans";
    const FIELD_DEPTH: &str = "depth";
    const FIELD_TRANS_DIR: &str = "dir";
    #[derive(derive_more::Display)]
    pub(super) enum TransitionDirection {
        #[display("call")]
        Call,
        #[display("ret")]
        Return,
    }

    impl<VS: VariablesState> BasicCallStackManager<VS> {
        #[inline]
        pub(super) fn log_span_reset(&mut self) {
            // We avoid the hierarchical structure of log spans as they don't suit our purpose.
            self.log_span = debug_span!(
                target: TAG_STACK,
                parent: Span::none(),
                SPAN_CALL,
                { FIELD_DEPTH } = self.stack.len(),
            )
            .entered();
        }

        #[inline]
        pub(super) fn log_span_start_trans(&mut self, direction: TransitionDirection) {
            self.log_span = debug_span!(
                target: TAG_STACK,
                parent: self.log_span.id(),
                SPAN_TRANSITION,
                { FIELD_TRANS_DIR } = direction.to_string(),
            )
            .entered();
        }
    }
}

struct PassedArgs<V> {
    values: Vec<V>,
    are_tupled: bool,
}

pub(super) struct EntranceToken<V> {
    entered_func_def: FuncDef,
    arg_places: Vec<DeterPlaceValueRef>,
    return_val_place: DeterPlaceValueRef,
    passed_args: Result<PassedArgs<V>, Option<PassedArgs<V>>>,
}
