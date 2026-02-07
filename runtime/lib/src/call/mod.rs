use core::fmt::Debug;

use derive_more as dm;

use common::{log_debug, log_trace};

use crate::abs::{CalleeDef, FuncDef};

// FIXME: Completely separate data and control flow for data agnostic backends.

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
 * |                    | start_enter()             |                                |
 * |                    | finalize_enter()          | __                             |
 * |                    | [override_return_value()] | depth = n + 1                  |
 * |                    | finalize_return()         | depth = n, trans: dir = "ret"  |
 * | finalize_call()    |                           | depth = n                      |
 * +--------------------+---------------------------+--------------------------------+
 * Log spans:
 * +---------------------------+--------------------------------+
 * | Function                  | Log Span                       |
 * +---------------------------+--------------------------------+
 * |                           | __ depth = n                   |
 * | prepare_for_call()        | depth = n, trans: dir = "call" |
 * | set_places()              |                                |
 * | start_enter()             |                                |
 * | finalize_enter()          | __                             |
 * |                           | depth = n + 1                  |
 * | [override_return_value()] | __                             |
 * | finalize_return()         | depth = n, trans: dir = "ret"  |
 * | finalize_call()           | depth = n                      |
 * +---------------------------+--------------------------------+
 *
 */

/* Functionality Specification:
 * - `from_caller` holds the information from the caller sent to the callee.
 *   It is set in `prepare_for_call` and consumed in `start_enter()` or cleared in `finalize_call`.
 * - `from_callee` holds the return information, not necessarily the expected function's (look at external functions).
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
 * - Using `from_caller.expected_func` to store the information.
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
 * Any existing return value is from an internal function called by the external function.
 */

/// The places corresponding to the function base locals, i.e., arguments and return value.
#[derive(Debug)]
pub(crate) struct SignaturePlaces<P> {
    pub arg_places: Vec<P>,
    pub return_val_place: P,
}

pub(crate) enum CallFlowSanity {
    Expected,
    /// The stack is broken (e.g., external function in between)
    Broken,
}

/// Manages the call flow.
/// Particularly, it takes care of the data transfer during call and return,
/// and detecting breakage in the call flow (e.g., external function in between).
pub(crate) trait CallFlowManager {
    type Place;
    type Value;
    type EntranceToken;

    fn current_func(&self) -> FuncDef;

    /* NOTE: Why `are_args_tupled` are passed?
     * First, arguments are tupled at the call site, where this function is called.
     * Second, when untupling, we should make sure that the arguments were tupled.
     * If closures are converted to a function pointer, then the arguments are not tupled.
     */
    fn prepare_for_call(
        &mut self,
        def: CalleeDef,
        func: Self::Value,
        args: Vec<Self::Value>,
        are_args_tupled: bool,
    );

    fn start_enter(
        &mut self,
        entered_func: FuncDef,
        places: SignaturePlaces<Self::Place>,
    ) -> Self::EntranceToken;

    fn finalize_enter<'a, 'h>(
        &'a mut self,
        token: Self::EntranceToken,
        tupling: impl tupling::ArgsTuplingInfoProvider<'h, 'a, Self::Place, Self::Value>,
        memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
    ) -> CallFlowSanity;

    fn override_return_value(&mut self, value: Self::Value);

    fn start_return(
        &mut self,
        memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
    );

    fn finalize_call(&mut self) -> (Self::Value, CallFlowSanity);
}

/// Provides memory functionalities expected to handle data transfer during function calls and returns.
/// Particularly, to set arguments passed from in the caller in the callee's memory,
/// and to take the return value from the callee's memory.
pub(crate) trait CallShadowMemory<P> {
    type Value;

    fn take_place(&mut self, place: &P) -> Self::Value;

    fn set_place(&mut self, place: &P, value: Self::Value);

    fn set_args(&mut self, places: &[P], values: Vec<Self::Value>) {
        assert_eq!(
            places.len(),
            values.len(),
            "Inconsistent number of argument places and values."
        );
        for (place, value) in places.into_iter().zip(values) {
            self.set_place(&place, value);
        }
    }
}

/// Provides callbacks to handle breakages detected in the call flow.
pub(crate) trait CallFlowBreakageCallback<P, V> {
    /// Handles a breakage detected when finalizing an external call,
    /// i.e., when returned back to the internal caller.
    /// # Arguments
    /// * `callee` - The callee that has not acknowledged the call (is external).
    /// * `current` - The current function returned to.
    /// * `unconsumed_args` - The arguments passed by the latest internal caller but not consumed yet.
    /// # Returns
    /// Return value to be used for the external call.
    /// # Remarks
    /// The only case currently possible for this is
    /// `i -> e`, where `i` is internal and `e` is external.
    /// This should be the common case where we have code that is not instrumented.
    fn after_return_with_args(
        &mut self,
        callee: CalleeDef,
        current: FuncDef,
        unconsumed_args: Vec<V>,
    ) -> V;

    /// Handles a breakage detected when entering an internal function (from an external call).
    /// # Arguments
    /// * `caller` - The latest caller function (that has sent the arguments).
    /// * `expected_callee` - The expected callee function.
    /// * `current` - The current function being entered.
    /// * `unconsumed_args` - The arguments passed by the latest internal caller but not consumed yet.
    /// * `current_arg_places` - The argument places of the current function.
    /// # Returns
    /// Argument values to be used in the entered function. It must be of the same length as `current_arg_places`.
    /// # Remarks
    /// The only case currently known to be possible for this is the call path of the following call chain:
    /// `i -> e -> i`, where `i` is internal and `e` is external.
    /// An example is a wrapper that is not instrumented around an instrumented function.
    fn at_enter(
        &mut self,
        caller: FuncDef,
        expected_callee: CalleeDef,
        current: FuncDef,
        unconsumed_args: Vec<V>,
        current_arg_places: &[P],
    ) -> Vec<V>;

    /// Handles a breakage detected when entering an internal function with no caller information.
    /// # Arguments
    /// * `current` - The current function being entered.
    /// * `current_arg_places` - The argument places of the current function.
    /// # Returns
    /// Argument values to be used in the entered function. It must be of the same length as `current_arg_places`.
    /// # Remarks
    /// The only case currently known to be possible is the entry point of the program.
    fn at_enter_with_no_caller(&mut self, current: FuncDef, current_arg_places: &[P]) -> Vec<V>;

    /// Handles a breakage detected when returning from an internal function.
    /// # Arguments
    /// * `callee` - The previous callee function whose returned value is not consumed.
    /// * `current` - The current function being returned from.
    /// * `unconsumed_returned_value` - The returned value from `callee`.
    /// # Remarks
    /// The only case currently know to be possible for this is
    /// `i -> e -> i+`, where `i` is internal and `e` is external.
    /// You can expect [handle_with_args_at_enter](CallFlowBreakageValueHandler::handle_with_args_at_enter) to be called before this.
    fn before_return_with_return_val(
        &mut self,
        callee: FuncDef,
        current: FuncDef,
        unconsumed_return_value: V,
    );

    /// Handles a breakage detected when returned to an internal caller.
    /// # Arguments
    /// * `callee` - The callee function whose returned value is not consumed.
    /// * `current` - The current function being returned to.
    /// * `unconsumed_returned_value` - The returned value from `callee`.
    /// # Returns
    /// Return value to be used for the external call.
    /// # Remarks
    /// The only case currently know to be possible for this is the return path of the following call chain:
    /// `i -> e -> i`, where `i` is internal and `e` is external.
    /// You can expect [handle_with_args_at_enter](CallFlowBreakageValueHandler::handle_with_args_at_enter) to be called before this.
    fn after_return_with_return_val(
        &mut self,
        callee: FuncDef,
        current: FuncDef,
        unconsumed_return_value: V,
    ) -> V;
}

pub(crate) mod tupling {
    use crate::abs::{FieldIndex, LocalIndex};

    use super::*;
    type LazyTuplingHelper<'h, 'f, P, V> =
        Box<dyn FnOnce() -> Box<dyn TuplingHelper<P, V> + 'h> + 'f>;

    #[derive(dm::Debug)]
    pub(crate) enum ArgsTuplingInfo<'h, 'f, P, V> {
        Normal,
        /// This function expects untupled arguments, but they are passed as a tupled argument.
        /// Expected to happen in closures.
        Untupled {
            #[debug(ignore)]
            tupled_arg_index: LocalIndex,
            #[debug(ignore)]
            tupling_helper: LazyTuplingHelper<'h, 'f, P, V>,
        },
        /// This function expects tupled arguments, but they are passed as separate arguments.
        /// Expected to happen only in `FnOnce` implementation of a non-capturing closure.
        Tupled {
            #[debug(ignore)]
            head_args: Box<dyn FnOnce() -> Vec<V> + 'f>,
            #[debug(ignore)]
            tupling_helper: LazyTuplingHelper<'h, 'f, P, V>,
        },
    }

    pub(crate) trait TuplingHelper<P, V>: CallShadowMemory<P, Value = V> {
        fn make_tupled_arg_pseudo_place(&mut self) -> P;

        /// Returns the number of fields in the tuple.
        fn num_fields(&mut self) -> FieldIndex;

        /// Takes a place and returns a place with projection to the field.
        /// It should make a valid place to be used with the memory.
        fn field_place(&mut self, base: &P, field: FieldIndex) -> P;
    }

    /// # Remarks
    /// This is lazy loaded as we might have call flow breakage that does not need tupling resolution.
    pub(crate) trait ArgsTuplingInfoProvider<'h, 'f, P, V> {
        fn get(self) -> ArgsTuplingInfo<'h, 'f, P, V>;
    }

    impl<'h, 'f, P, V, F> ArgsTuplingInfoProvider<'h, 'f, P, V> for F
    where
        F: FnOnce() -> ArgsTuplingInfo<'h, 'f, P, V> + 'f,
    {
        fn get(self) -> ArgsTuplingInfo<'h, 'f, P, V> {
            self()
        }
    }
}

mod implementation {
    use crate::call::tupling::ArgsTuplingInfoProvider;

    use super::*;

    pub(crate) struct DefaultCallFlowManager<P, V, BC> {
        /// Stacked storage that holds data living during the function execution.
        stack: Vec<StackInfo<P, V>>,

        /// Non-stacked storage that holds data living during call transfers or
        /// between consecutive operations in the function.
        ephemeral: EphemeralInfo<V>,

        /// The callback to be called when breakage in the call flow is detected.
        breakage_callback: BC,

        log_span: tracing::span::EnteredSpan,
    }

    struct StackInfo<P, V> {
        /// The current function being executed in this frame.
        def: FuncDef,
        /// The place where the return value is stored.
        /// Used when returning from the function to retrieve value to be sent back to the caller.
        return_val_place: P,
        /// Determines whether the callee is external or not.
        /// Associated with the latest call performed or being performed.
        is_callee_external: Option<bool>,
        /// The return value forced by a call to `override_return_value`.
        /// If it is set in an internal call, it will be consumed as the returned
        /// value of the current function when popping the frame.
        /// If it is set in an external call, it will be consumed as the returned
        /// value from the external call when storing the returned value in the
        /// destination variable.
        overridden_return_val: Option<V>,
    }

    struct EphemeralInfo<V> {
        /// Exists starting the call point (in the caller) until the entrance point (in the callee).
        from_caller: Option<CallerParcel<V>>,
        /// Exists starting the exit point (in the callee) until the return point (in the caller).
        from_callee: Option<CalleeParcel<V>>,
    }

    impl<V> Default for EphemeralInfo<V> {
        fn default() -> Self {
            Self {
                from_caller: None,
                from_callee: None,
            }
        }
    }

    /// The data passed between from the caller to the callee.
    #[derive(Debug)]
    struct CallerParcel<V> {
        expected_func: CalleeDef,
        args: PassedArgs<V>,
    }

    /// The data passed between from the callee to the caller.
    struct CalleeParcel<V> {
        func: FuncDef,
        return_val: V,
    }

    /// The information passed from the phases of entrance.
    pub(crate) struct EntranceToken<P, V> {
        entered_func: FuncDef,
        arg_places: Vec<P>,
        return_val_place: P,
        from_caller: Result<CallerParcel<V>, Option<CallerParcel<V>>>,
    }

    #[derive(Debug)]
    struct PassedArgs<V> {
        values: Vec<V>,
        are_tupled: bool,
    }

    impl<P, V, BC> DefaultCallFlowManager<P, V, BC> {
        pub(crate) fn new(breakage_callback: BC) -> Self
        where
            BC: CallFlowBreakageCallback<P, V>,
        {
            Self {
                stack: vec![],
                ephemeral: EphemeralInfo::default(),
                log_span: tracing::Span::none().entered(),
                breakage_callback,
            }
        }
    }

    impl<P, V, BC: Default> Default for DefaultCallFlowManager<P, V, BC>
    where
        BC: CallFlowBreakageCallback<P, V>,
    {
        fn default() -> Self {
            Self::new(BC::default())
        }
    }

    impl<P, V, BC> DefaultCallFlowManager<P, V, BC>
    where
        BC: CallFlowBreakageCallback<P, V>,
    {
        fn push_new_stack_frame(
            &mut self,
            entered_func: FuncDef,
            return_val_place: P,
            args: (Vec<P>, Vec<V>),
            memory: &mut impl CallShadowMemory<P, Value = V>,
        ) {
            memory.set_args(args.0.as_slice(), args.1);

            self.stack.push(StackInfo {
                def: entered_func,
                return_val_place,
                is_callee_external: None,
                overridden_return_val: None,
            });
            self.log_span_reset();
        }

        fn top_frame(&mut self) -> &mut StackInfo<P, V> {
            self.stack
                .last_mut()
                .expect("Call stack should not be empty")
        }

        fn finalize_internal_call(&mut self) -> V {
            self.ephemeral
                .from_callee
                .take()
                .expect("Internal callee is expected to pass the return value")
                .return_val
        }

        fn finalize_external_call(&mut self) -> V {
            // Clearing the data that is may not be cleared by the external function.
            let mut return_val = if let Some(from_caller) = self.ephemeral.from_caller.take() {
                self.breakage_callback.after_return_with_args(
                    from_caller.expected_func,
                    self.current_func(),
                    from_caller.args.values,
                )
            } else if let Some(from_callee) = self.ephemeral.from_callee.take() {
                self.breakage_callback.after_return_with_return_val(
                    from_callee.func,
                    self.current_func(),
                    from_callee.return_val,
                )
            } else {
                panic!(
                    "Caller information is consumed but no return value is available from the callee."
                );
            };

            if let Some(overridden) = self.top_frame().overridden_return_val.take() {
                log_debug!(
                    target: TAG,
                    concat!(
                    "Consuming the overridden return value as the return value ",
                    "for the function call."
                    ),
                );
                return_val = overridden;
            }

            return_val
        }

        fn current_func(&self) -> FuncDef {
            self.stack
                .last()
                .expect("Stack should not be empty")
                .def
                .clone()
        }
    }

    pub(super) mod tupling {
        use crate::abs::FieldIndex;

        use super::super::tupling::*;
        use super::*;

        impl<P, V, BC> DefaultCallFlowManager<P, V, BC> {
            pub(super) fn resolve_tupling(
                &mut self,
                args: &mut PassedArgs<V>,
                tupling: ArgsTuplingInfo<P, V>,
            ) {
                match tupling {
                    ArgsTuplingInfo::Untupled {
                        tupled_arg_index: arg_index,
                        tupling_helper,
                    } if args.are_tupled => {
                        core::hint::cold_path();
                        let arg_index = arg_index as usize - 1; // It is the local index, so starts from 1.
                        log_debug!(target: TAG, "Untupling argument at index {}", arg_index);
                        let tupled_value = args.values.remove(arg_index);
                        let untupled_args = Self::untuple(tupled_value, tupling_helper().as_mut());
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
                        args.values
                            .push(Self::tuple(separated_args, tupling_helper().as_mut()));
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
                tupled_value: V,
                helper: &mut (impl TuplingHelper<P, V> + ?Sized),
            ) -> Vec<V> {
                let tupled_pseudo_place = helper.make_tupled_arg_pseudo_place();

                // Write the value to the pseudo place in memory, then read the fields
                let num_fields = helper.num_fields();
                helper.set_place(&tupled_pseudo_place, tupled_value);
                // Read the fields (values inside the tuple) one by one.
                (0..num_fields)
                    .into_iter()
                    .map(|i| {
                        let field_place = helper.field_place(&tupled_pseudo_place, i);
                        helper.take_place(&field_place)
                    })
                    .collect()
            }

            fn tuple(
                separate_values: Vec<V>,
                helper: &mut (impl TuplingHelper<P, V> + ?Sized),
            ) -> V {
                let tupled_pseudo_place = helper.make_tupled_arg_pseudo_place();

                // Write the values to the field pseudo places in an isolated state, then read the tuple
                debug_assert_eq!(separate_values.len(), helper.num_fields() as usize);
                separate_values
                    .into_iter()
                    .enumerate()
                    .for_each(|(i, value)| {
                        let field_place = helper.field_place(&tupled_pseudo_place, i as FieldIndex);
                        helper.set_place(&field_place, value)
                    });
                // Read the whole tuple.
                helper.take_place(&tupled_pseudo_place)
            }
        }

        pub(crate) struct NoOpArgsTuplingInfoProvider;

        // NOTE: Update this based on the internal logics of tupling resolution.
        impl<'h, 'f, P, V> ArgsTuplingInfoProvider<'h, 'f, P, V> for NoOpArgsTuplingInfoProvider {
            fn get(self) -> ArgsTuplingInfo<'h, 'f, P, V> {
                ArgsTuplingInfo::Normal
            }
        }
    }

    impl<P: Debug, V: Debug, BC> CallFlowManager for DefaultCallFlowManager<P, V, BC>
    where
        BC: CallFlowBreakageCallback<P, V>,
    {
        type Place = P;
        type Value = V;
        type EntranceToken = EntranceToken<P, V>;

        fn current_func(&self) -> FuncDef {
            self.current_func()
        }

        fn prepare_for_call(
            &mut self,
            def: CalleeDef,
            _func: Self::Value,
            args: Vec<Self::Value>,
            are_args_tupled: bool,
        ) {
            self.log_span_start_trans(logging::TransitionDirection::Call);

            debug_assert!(
                self.ephemeral.from_caller.is_none(),
                concat!(
                    "The caller parcel is not consumed or cleaned correctly. ",
                    "This is due to a problem in external function handling or instrumentation. ",
                    "{:?}",
                ),
                self.ephemeral.from_caller,
            );

            self.ephemeral.from_caller = Some(CallerParcel {
                expected_func: def,
                args: PassedArgs {
                    values: args,
                    are_tupled: are_args_tupled,
                },
            });
        }

        fn start_enter(
            &mut self,
            entered_func: FuncDef,
            sig_places: SignaturePlaces<Self::Place>,
        ) -> Self::EntranceToken {
            let arg_values_if_not_broken = if let Some(call_info) =
                self.ephemeral.from_caller.take()
            {
                if entered_func == call_info.expected_func {
                    log_trace!(
                        target: TAG,
                        "Entering the function: {} with expected function: {}",
                        entered_func, call_info.expected_func,
                    );
                    Ok(call_info)
                } else {
                    log_debug!(
                        target: TAG,
                        "External function call detected. Expected: {} but got: {}",
                        call_info.expected_func,
                        entered_func,
                    );
                    Err(Some(call_info))
                }
            } else {
                // NOTE: An example of this case is when an external function calls multiple internal ones.
                log_debug!(target: TAG, "External function call detected with no call information available");
                Err(None)
            };

            EntranceToken {
                entered_func,
                arg_places: sig_places.arg_places,
                return_val_place: sig_places.return_val_place,
                from_caller: arg_values_if_not_broken,
            }
        }

        fn finalize_enter<'a, 'h>(
            &'a mut self,
            EntranceToken {
                entered_func,
                arg_places,
                return_val_place,
                from_caller,
            }: Self::EntranceToken,
            tupling: impl ArgsTuplingInfoProvider<'h, 'a, Self::Place, Self::Value>,
            memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
        ) -> CallFlowSanity {
            let is_broken = from_caller.is_err();
            if let Some(parent_frame) = self.stack.last_mut() {
                parent_frame.is_callee_external = Some(is_broken);
            }

            let arg_values = match from_caller {
                Ok(CallerParcel { mut args, .. }) => {
                    self.resolve_tupling(&mut args, tupling.get());
                    args.values
                }
                Err(Some(from_caller)) => {
                    self.breakage_callback.at_enter(
                        self.current_func(), // Still we haven't pushed the new frame, so it's correct.
                        from_caller.expected_func,
                        entered_func,
                        from_caller.args.values,
                        &arg_places,
                    )
                }
                Err(None) => self
                    .breakage_callback
                    .at_enter_with_no_caller(entered_func, &arg_places),
            };

            self.push_new_stack_frame(
                entered_func,
                return_val_place,
                (arg_places, arg_values),
                memory,
            );

            log_debug!(target: TAG, "Entered the function");

            if is_broken {
                CallFlowSanity::Broken
            } else {
                CallFlowSanity::Expected
            }
        }

        fn override_return_value(&mut self, value: Self::Value) {
            log_debug!(target: TAG, "Overriding the return value with {:?}", value);
            self.top_frame().overridden_return_val = Some(value);
        }

        fn start_return(
            &mut self,
            memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
        ) {
            let current_func = self.current_func();
            let popped_frame = self.stack.pop().unwrap();

            self.log_span_reset();
            if self
                .stack
                .last()
                .is_some_and(|f| !f.is_callee_external.unwrap())
            {
                self.log_span_start_trans(logging::TransitionDirection::Return);
            }

            let mut return_val = memory.take_place(&popped_frame.return_val_place.into());
            if let Some(overridden) = popped_frame.overridden_return_val {
                return_val = overridden;
            }

            let from_callee = CalleeParcel {
                func: current_func,
                return_val,
            };
            let unconsumed = self.ephemeral.from_callee.replace(from_callee);

            if let Some(unconsumed) = unconsumed {
                self.breakage_callback.before_return_with_return_val(
                    unconsumed.func,
                    current_func,
                    unconsumed.return_val,
                );
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
    }
    pub(crate) struct NoOpCallFlowBreakageCallback<F> {
        unknown_value_factory: F,
    }

    impl<V> Default for NoOpCallFlowBreakageCallback<fn() -> V>
    where
        V: Default,
    {
        fn default() -> Self {
            Self {
                unknown_value_factory: Default::default,
            }
        }
    }

    impl<F> NoOpCallFlowBreakageCallback<F> {
        pub(crate) fn new<V>(unknown_value_factory: F) -> Self
        where
            F: Fn() -> V,
        {
            Self {
                unknown_value_factory,
            }
        }
    }

    impl<P, V, F> CallFlowBreakageCallback<P, V> for NoOpCallFlowBreakageCallback<F>
    where
        F: Fn() -> V,
    {
        fn after_return_with_args(
            &mut self,
            _callee: CalleeDef,
            _current: FuncDef,
            _unconsumed_args: Vec<V>,
        ) -> V {
            (self.unknown_value_factory)()
        }

        fn at_enter(
            &mut self,
            _caller: FuncDef,
            _expected_callee: CalleeDef,
            current: FuncDef,
            _unconsumed_args: Vec<V>,
            current_arg_places: &[P],
        ) -> Vec<V> {
            self.at_enter_with_no_caller(current, current_arg_places)
        }

        fn at_enter_with_no_caller(
            &mut self,
            _current: FuncDef,
            current_arg_places: &[P],
        ) -> Vec<V> {
            core::iter::repeat_with(&self.unknown_value_factory)
                .take(current_arg_places.len())
                .collect()
        }

        fn before_return_with_return_val(
            &mut self,
            _callee: FuncDef,
            _current: FuncDef,
            _unconsumed_return_value: V,
        ) {
        }

        fn after_return_with_return_val(
            &mut self,
            _callee: FuncDef,
            _current: FuncDef,
            _unconsumed_return_value: V,
        ) -> V {
            (self.unknown_value_factory)()
        }
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

        pub(crate) const TAG: &str = "call_manager";
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

        impl<V, P, BC> DefaultCallFlowManager<V, P, BC> {
            #[inline(always)]
            pub(super) fn log_span_reset(&mut self) {
                if !tracing::enabled!(tracing::Level::DEBUG) {
                    return;
                }

                if self.stack.len() == 0 {
                    self.log_span = Span::none().entered();
                    return;
                }

                // We avoid the hierarchical structure of log spans as they don't suit our purpose.
                self.log_span = debug_span!(
                    target: TAG_STACK,
                    parent: Span::none(),
                    SPAN_CALL,
                    { FIELD_DEPTH } = self.stack.len(),
                )
                .entered();
            }

            #[inline(always)]
            pub(super) fn log_span_start_trans(&mut self, direction: TransitionDirection) {
                if !tracing::enabled!(tracing::Level::DEBUG) {
                    return;
                }

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
    pub(crate) use logging::TAG;
}
pub(crate) use implementation::{
    DefaultCallFlowManager, NoOpCallFlowBreakageCallback, TAG, tupling::NoOpArgsTuplingInfoProvider,
};
