use core::fmt::Debug;

use derive_more as dm;

use common::{log_debug, log_trace, log_warn};

use crate::abs::{CalleeDef, FuncDef};

// FIXME: Completely separate data and control flow for data agnostic backends.

/* Here is what happens during function calls in the instrumentation:
 * +-----------------+---------------------------+
 * | Caller          | Callee                    |
 * +-----------------+---------------------------+
 * | before_call_*() |                           |
 * | take_data()?    |                           |
 * |                 | enter()                   |
 * |                 | emplace_arguments()?      |
 * |                 | override_return_value()?  |
 * |                 | return()                  |
 * | after_call()    |                           |
 * +-----------------+---------------------------+
 * Or equivalently (names are changed to better reflect the current position of program):
 * +--------------------+---------------------------+--------------------------------+
 * | Caller             | Callee                    | Log Span                       |
 * +--------------------+---------------------------+--------------------------------+
 * | prepare_for_calling|                           | depth = n, trans: dir = "call" |
 * | prepare_for_call_with_values()?                |                                |
 * |                    | enter()                   |                                |
 * |                    | emplace_args()            | depth = n + 1                  |
 * |                    | [override_return_value()] |                                |
 * |                    | start_return()            | depth = n, trans: dir = "ret"  |
 * |                    | grab_return_value()?      |                                |
 * | finalize_call()    |                           | depth = n                      |
 * | give_return_value()|                           | depth = n                      |
 * +--------------------+---------------------------+--------------------------------+
 *
 * Log span timeline:
 * +---------------------------------+--------------------------------+
 * | Function                        | Log Span                       |
 * +---------------------------------+--------------------------------+
 * |                                 | depth = n                      |
 * | prepare_for_calling()           | depth = n, trans: dir = "call" |
 * | prepare_for_call_with_values()? |                                |
 * | enter()                         |                                |
 * | emplace_args()                  | depth = n + 1                  |
 * | [override_return_value()]       |                                |
 * | start_return()                  | depth = n, trans: dir = "ret"  |
 * | grab_return_value()?            |                                |
 * | finalize_call()                 | depth = n                      |
 * | give_return_value()?            |                                |
 * +---------------------------------+--------------------------------+
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
 * Detection is mainly done by setting a function id on the caller side and comparing it with
 * the current function id on the callee side.
 * However, following some scenarios below, you can have cases that the id is not checked at all that
 * can also be used to deduce the presence of external functions.
 * Specification:
 * - Using `from_caller.expected_func` to store the information.
 * - `latest_call_sanity` is stack-based data meant for the caller. It is set when
 *    an internal function is called, otherwise remains unset or is set to false.
 * Scenarios:
 * - internal -> external:
 *   When returned to the caller, `latest_call_sanity` is unset, `from_caller` is unconsumed.
 * - external -> internal:
 *   When entered the callee, `from_caller` is not available.
 * - internal -> external -> internal
 *   When entered the internal callee, `from_caller.expected_func` does not match.
 *   `latest_call_sanity` is set.
 * - internal -> external -> internal+
 *   When entered the first internal callee, same as above.
 *   When entered the second internal callee, `from_caller` is not available, but `from_callee` is available.
 *   When returned to the initial caller, `from_callee` from the latest internal callee is not consumed.
 * - internal -> drop -> internal
 *   (Temporarily unsupported by instrumentation)
 *   When entered the internal callee, `from_caller` is not available.
 *   When returned to the caller, `from_callee` is not consumed, but no finalization is performed either.
 */

/// The places corresponding to the function base locals, i.e., arguments and return value.
#[derive(Debug)]
pub(crate) struct SignaturePlaces<P> {
    pub args: Vec<P>,
    pub return_val: P,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallFlowSanity<E = (), B = (), U = ()> {
    Expected(E),
    /// The stack is broken (e.g., external function in between)
    Broken(B),
    /// Expected callee was not provided.
    Unknown(U),
}

impl<E, B, U> CallFlowSanity<E, B, U> {
    fn no_info(&self) -> CallFlowSanity<(), (), ()> {
        match self {
            CallFlowSanity::Expected(_) => CallFlowSanity::Expected(()),
            CallFlowSanity::Broken(_) => CallFlowSanity::Broken(()),
            CallFlowSanity::Unknown(_) => CallFlowSanity::Unknown(()),
        }
    }

    pub(crate) fn is_broken(&self) -> Option<bool> {
        match self {
            CallFlowSanity::Expected(_) => Some(false),
            CallFlowSanity::Broken(_) => Some(true),
            CallFlowSanity::Unknown(_) => None,
        }
    }
}

/// Manages the call flow.
/// Four phases exist in a function call: before call (in the caller),
/// entrance (in the callee), return (in the callee), and after call (in the caller).
pub(crate) trait CallFlowManager {
    type Value;
    type ReturnToken;
    type FinalizationToken;

    fn current_func(&self) -> FuncDef;

    fn prepare_for_call(&mut self);

    fn enter(&mut self, entered_func: FuncDef) -> CallFlowSanity;

    fn start_return(&mut self) -> Self::ReturnToken;

    fn finalize_call(&mut self) -> Self::FinalizationToken;
}

/// An extension of `CallFlowManager` with additional data for flow breakage handling.
pub(crate) trait CallControlFlowManager: CallFlowManager {
    fn prepare_for_calling(&mut self, def: CalleeDef);
}

/// An extension of `CallFlowManager` with functionalities for data transfer
/// during function calls and returns.
pub(crate) trait CallDataFlowManager: CallFlowManager {
    type Place;

    /* NOTE: Why `are_args_tupled` are passed?
     * First, arguments are tupled at the call site, where this function is called.
     * Second, when untupling, we should make sure that the arguments were tupled.
     * If closures are converted to a function pointer, then the arguments are not tupled.
     */
    fn prepare_for_call_with_values(
        &mut self,
        func: Self::Value,
        args: Vec<Self::Value>,
        are_args_tupled: bool,
    );

    fn emplace_args<'a, 'h>(
        &'a mut self,
        places: SignaturePlaces<Self::Place>,
        tupling: impl tupling::ArgsTuplingInfoProvider<'h, 'a, Self::Place, Self::Value>,
        memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
    );

    fn override_return_value(&mut self, value: Self::Value);

    fn grab_return_value(
        &mut self,
        token: Self::ReturnToken,
        memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
    );

    fn give_return_value(&mut self, token: Self::FinalizationToken) -> Self::Value;
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
        callee: Option<CalleeDef>,
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

    /// Handles a breakage detected when entering an internal function with a previously returned value.
    /// # Arguments
    /// * `callee` - The previous callee function whose returned value is not consumed.
    /// * `current` - The current function being entered.
    /// * `unconsumed_returned_value` - The returned value from `callee`.
    /// # Remarks
    /// The only case currently know to be possible for this is
    /// `i -> e -> i+`, where `i` is internal and `e` is external.
    /// You can expect [at_enter_with_no_caller](CallFlowBreakageCallback::at_enter_with_no_caller) to be called after this.
    fn at_enter_with_return_val(
        &mut self,
        callee: FuncDef,
        current: FuncDef,
        unconsumed_return_value: V,
    );

    /// Handles a breakage detected when entering an internal function with no caller information.
    /// # Arguments
    /// * `current` - The current function being entered.
    /// * `current_arg_places` - The argument places of the current function.
    /// # Returns
    /// Argument values to be used in the entered function. It must be of the same length as `current_arg_places`.
    /// # Remarks
    /// The only case currently known to be possible is the entry point of the program.
    fn at_enter_with_no_caller(&mut self, current: FuncDef, current_arg_places: &[P]) -> Vec<V>;

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
    /// You can expect [at_enter](CallFlowBreakageCallback::at_enter) to be called before this.
    fn after_return_with_return_val(
        &mut self,
        callee: FuncDef,
        current: FuncDef,
        unconsumed_return_value: V,
    ) -> V;

    /// Handles a breakage detected when returning from an unexpected call.
    /// # Arguments
    /// * `current` - The current function returning from.
    /// * `returned_value` - The returned value.
    /// # Remarks
    /// The only case currently know to be possible for this is the return path of the following call chain:
    /// `i ? i`, where `i` is internal.
    fn at_return_with_return_val(&mut self, current: FuncDef, unconsumed_return_value: V);
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
    use derive_more as dm;
    use itertools::Either;

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
        // (Control Domain)
        /// The current function being executed in this frame.
        def: FuncDef,
        // (Control Domain)
        /// Determines whether the callee is external or not.
        /// Associated with the latest call performed or being performed.
        latest_call_sanity: Option<CallFlowSanity>,
        // (Data Domain)
        /// The place where the return value is stored.
        /// Used when returning from the function to retrieve value to be sent back to the caller.
        return_val_place: Option<P>,
        /// The return value forced by a call to `override_return_value`.
        /// If it is set in an internal call, it will be consumed as the returned
        /// value of the current function when popping the frame.
        /// If it is set in an external call, it will be consumed as the returned
        /// value from the external call when storing the returned value in the
        /// destination variable.
        overridden_return_val: Option<V>,
        /// This call was unexpected, i.e., the caller did not ask for preparation for it. (due to uninstrumented call)
        is_unexpected: bool,
    }

    // FIXME: The fields here are probably exclusive. May be replaced with an enum.
    struct EphemeralInfo<V> {
        /// Exists starting the call point (in the caller) until the entrance point (in the callee).
        from_caller: Option<CallerParcel<V>>,
        /// ## Lifetime
        /// Filled by the definite entrance call, and used by the further entrance calls.
        /// Cleaned by prepare or return.
        entrance: Option<EntranceInfo<V>>,
        /// Exists starting the exit point (in the callee) until the return point (in the caller).
        from_callee: Option<CalleeParcel<V>>,
    }

    impl<V> Default for EphemeralInfo<V> {
        fn default() -> Self {
            Self {
                from_caller: None,
                entrance: None,
                from_callee: None,
            }
        }
    }

    #[derive(Debug)]
    struct EntranceInfo<V>(
        CallFlowSanity<
            CallerParcel<V>,
            Either<CallerParcel<V>, CalleeParcel<V>>,
            Option<CallerParcel<V>>,
        >,
    );

    /// The data passed between from the caller to the callee.
    #[derive(Debug)]
    struct CallerParcel<V> {
        // (Control Domain)
        /// Availability: Control flow methods
        expected_func: Option<CalleeDef>,
        // (Data Domain)
        /// Availability: Data flow methods
        args: Option<PassedArgs<V>>,
    }

    impl<V> Default for CallerParcel<V> {
        fn default() -> Self {
            Self {
                expected_func: None,
                args: None,
            }
        }
    }

    /// The data passed between from the callee to the caller.
    #[derive(Debug)]
    struct CalleeParcel<V> {
        func: FuncDef,
        /// Availability: Data flow methods
        return_val: Option<V>,
    }

    #[derive(Debug)]
    struct PassedArgs<V> {
        values: Vec<V>,
        are_tupled: bool,
    }

    impl<P, V, BC> DefaultCallFlowManager<P, V, BC> {
        pub(crate) fn new(breakage_callback: BC) -> Self {
            Self {
                stack: vec![],
                ephemeral: EphemeralInfo::default(),
                log_span: tracing::Span::none().entered(),
                breakage_callback,
            }
        }
    }

    impl<P, V, BC: Default> Default for DefaultCallFlowManager<P, V, BC> {
        fn default() -> Self {
            Self::new(BC::default())
        }
    }

    impl<P, V, BC> DefaultCallFlowManager<P, V, BC> {
        fn prepare_for_call_partial(
            &mut self,
            expected_func: Option<CalleeDef>,
            args: Option<PassedArgs<V>>,
        ) where
            V: Debug,
        {
            self.clear_entrance();

            if self.ephemeral.from_caller.is_none() {
                self.log_span_start_trans(logging::TransitionDirection::Call);
            }

            macro_rules! cleanup_msg {
                () => {
                    concat!(
                        "The caller parcel is not consumed or cleaned correctly. ",
                        "This is due to a problem in external function handling or instrumentation. ",
                        "{:?}",
                    )
                };
            }

            let parcel = self.ephemeral.from_caller.get_or_insert_default();
            if let Some(expected_func) = expected_func {
                debug_assert!(
                    parcel.expected_func.is_none(),
                    cleanup_msg!(),
                    self.ephemeral.from_caller,
                );
                parcel.expected_func = Some(expected_func);
            }
            if let Some(args) = args {
                debug_assert!(
                    parcel.args.is_none(),
                    cleanup_msg!(),
                    self.ephemeral.from_caller,
                );
                parcel.args = Some(args);
            }
        }

        fn top_frame(&mut self) -> &mut StackInfo<P, V> {
            self.stack
                .last_mut()
                .expect("Call stack should not be empty")
        }

        fn current_func(&self) -> FuncDef {
            self.stack
                .last()
                .expect("Call stack should not be empty")
                .def
                .clone()
        }

        fn latest_caller_func(&self) -> FuncDef {
            self.stack
                .get(self.stack.len().saturating_sub(2))
                .expect("A caller was expected.")
                .def
                .clone()
        }

        fn clear_entrance(&mut self) {
            self.ephemeral.entrance = None;
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

    pub(crate) struct ReturnToken<P, V> {
        popped_frame: StackInfo<P, V>,
    }

    #[derive(dm::From)]
    pub(crate) struct FinalizationToken<V>(
        CallFlowSanity<CalleeParcel<V>, Either<CallerParcel<V>, CalleeParcel<V>>, !>,
    );

    impl<V> FinalizationToken<V> {
        pub(crate) fn sanity(&self) -> CallFlowSanity<(), (), !> {
            match &self.0 {
                CallFlowSanity::Expected(..) => CallFlowSanity::Expected(()),
                CallFlowSanity::Broken(..) => CallFlowSanity::Broken(()),
                CallFlowSanity::Unknown(..) => unreachable!(),
            }
        }
    }

    impl<P, V: Debug, BC> CallFlowManager for DefaultCallFlowManager<P, V, BC> {
        type Value = V;
        type ReturnToken = ReturnToken<P, V>;
        type FinalizationToken = FinalizationToken<V>;

        fn current_func(&self) -> FuncDef {
            self.current_func()
        }

        fn prepare_for_call(&mut self) {
            self.prepare_for_call_partial(None, None);
        }

        fn enter(&mut self, entered_func: FuncDef) -> CallFlowSanity {
            debug_assert!(
                self.ephemeral.entrance.is_none(),
                concat!(
                    "The entrance parcel is not consumed or cleaned correctly. ",
                    "This is due to a problem in external function handling or instrumentation. ",
                    "{:?}",
                ),
                self.ephemeral.entrance,
            );

            let entrance = if let Some(call_info) = self.ephemeral.from_caller.take() {
                if let Some(expected_func) = call_info.expected_func {
                    if entered_func == expected_func {
                        log_trace!(
                            target: TAG,
                            "Entering the function: {} with expected function: {}",
                            entered_func, expected_func,
                        );
                        EntranceInfo(CallFlowSanity::Expected(call_info))
                    } else {
                        log_debug!(
                            target: TAG,
                            "External 3: Expected: {} got: {}",
                            expected_func,
                            entered_func,
                        );
                        EntranceInfo(CallFlowSanity::Broken(Either::Left(call_info)))
                    }
                } else {
                    EntranceInfo(CallFlowSanity::Unknown(Some(call_info)))
                }
            } else {
                if let Some(from_callee) = self.ephemeral.from_callee.take() {
                    log_debug!(target: TAG, "External 4: No call information available");
                    EntranceInfo(CallFlowSanity::Broken(Either::Right(from_callee)))
                } else {
                    log_debug!(target: TAG, "External 2: No call information available");
                    if !self.stack.is_empty() {
                        log_warn!(
                            target: TAG,
                            "Observing unexpected call. Probable caller: {}, entered: {}.",
                            self.current_func(),
                            entered_func,
                        );
                    }
                    EntranceInfo(CallFlowSanity::Unknown(None))
                }
            };
            let sanity = entrance.0.no_info();

            if let Some(parent_frame) = self.stack.last_mut() {
                parent_frame.latest_call_sanity = Some(sanity);
            }

            self.stack.push(StackInfo {
                def: entered_func,
                latest_call_sanity: None,
                return_val_place: None,
                overridden_return_val: None,
                is_unexpected: matches!(entrance.0, CallFlowSanity::Unknown(None))
                    && !self.stack.is_empty(),
            });

            self.ephemeral.entrance = Some(entrance);

            self.log_span_reset();
            log_debug!(target: TAG, "Entered the function");

            sanity
        }

        fn start_return(&mut self) -> Self::ReturnToken {
            let current_func = self.current_func();

            self.clear_entrance();
            if let Some(from_caller) = self.ephemeral.from_callee.take() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Unconsumed callee parcel found at return of {}. {:?}. ",
                        "Unless this is happening in an exceptional path, it should be a problem in instrumentation.",
                    ),
                    current_func,
                    from_caller,
                );
            }

            let popped_frame = self.stack.pop().unwrap();

            self.log_span_reset();

            let caller_frame = self.stack.last();
            if caller_frame
                .is_some_and(|f| !f.latest_call_sanity.unwrap().is_broken().unwrap_or(false))
                && !popped_frame.is_unexpected
            {
                self.log_span_start_trans(logging::TransitionDirection::Return);
            }

            if !popped_frame.is_unexpected {
                self.ephemeral.from_callee = Some(CalleeParcel {
                    func: current_func,
                    return_val: None,
                });
            } else {
                log_debug!(target:TAG, "Returning from an unexpected call {}.", current_func);
            }

            ReturnToken { popped_frame }
        }

        fn finalize_call(&mut self) -> Self::FinalizationToken {
            self.log_span_reset();
            log_debug!(target: TAG, "Finalizing call");

            let sanity = self
                .top_frame()
                .latest_call_sanity
                .take()
                .unwrap_or_else(|| {
                    log_debug!(target: TAG, "External 1: No entrance was acknowledged.");
                    CallFlowSanity::Broken(())
                });

            match sanity {
                CallFlowSanity::Expected(..) | CallFlowSanity::Unknown(..) => {
                    CallFlowSanity::Expected(self.ephemeral.from_callee.take().expect(
                        "Callee parcel is expected to be set when the sanity is not broken.",
                    ))
                }
                CallFlowSanity::Broken(..) => CallFlowSanity::Broken(
                    if let Some(from_caller) = self.ephemeral.from_caller.take() {
                        debug_assert!(
                            self.ephemeral.from_callee.is_none(),
                            concat!(
                                "Both caller and callee parcels are available. ",
                                "This is due to a problem in external function handling or instrumentation. ",
                                "{:?}, {:?}",
                            ),
                            from_caller,
                            self.ephemeral.from_callee,
                        );
                        Either::Left(from_caller)
                    } else if let Some(from_callee) = self.ephemeral.from_callee.take() {
                        Either::Right(from_callee)
                    } else {
                        panic!("Caller parcel is consumed but no callee parcel is available.")
                    },
                ),
            }.into()
        }
    }

    impl<P, V: Debug, BC> CallControlFlowManager for DefaultCallFlowManager<P, V, BC> {
        fn prepare_for_calling(&mut self, def: CalleeDef) {
            self.prepare_for_call_partial(Some(def), None);
        }
    }

    const MSG_DATA_UNAVAILABLE: &str = concat!(
        "Arguments from the caller or the return value from the callee are not transmitted. ",
        "Is the instrumentation incomplete or incorrect?",
    );

    impl<P, V: Debug, BC> CallDataFlowManager for DefaultCallFlowManager<P, V, BC>
    where
        BC: CallFlowBreakageCallback<P, V>,
    {
        type Place = P;

        fn prepare_for_call_with_values(
            &mut self,
            _func: Self::Value,
            args: Vec<Self::Value>,
            are_args_tupled: bool,
        ) {
            self.prepare_for_call_partial(
                None,
                Some(PassedArgs {
                    values: args,
                    are_tupled: are_args_tupled,
                }),
            );
        }

        fn emplace_args<'a, 'h>(
            &'a mut self,
            places: SignaturePlaces<Self::Place>,
            tupling: impl super::tupling::ArgsTuplingInfoProvider<'h, 'a, Self::Place, Self::Value>,
            memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
        ) {
            let entrance = self
                .ephemeral
                .entrance
                .take()
                .expect("Entrance information is expected to be set in the start_enter phase.");

            let entered_func = self.current_func();

            let arg_values = match entrance.0 {
                CallFlowSanity::Expected(from_caller)
                | CallFlowSanity::Unknown(Some(from_caller)) => {
                    let mut args = from_caller.args.expect(MSG_DATA_UNAVAILABLE);
                    self.resolve_tupling(&mut args, tupling.get());
                    args.values
                }
                CallFlowSanity::Broken(unconsumed_parcel) => match unconsumed_parcel {
                    Either::Left(from_caller) => self.breakage_callback.at_enter(
                        self.latest_caller_func(),
                        from_caller
                            .expected_func
                            .expect("Sanity cannot be evaluated without expected function."),
                        entered_func,
                        from_caller.args.expect(MSG_DATA_UNAVAILABLE).values,
                        &places.args,
                    ),
                    Either::Right(unconsumed_callee) => {
                        self.breakage_callback.at_enter_with_return_val(
                            unconsumed_callee.func,
                            entered_func,
                            unconsumed_callee.return_val.expect(MSG_DATA_UNAVAILABLE),
                        );
                        self.breakage_callback
                            .at_enter_with_no_caller(entered_func, &places.args)
                    }
                },
                CallFlowSanity::Unknown(None) => self
                    .breakage_callback
                    .at_enter_with_no_caller(entered_func, &places.args),
            };

            self.top_frame().return_val_place = Some(places.return_val);
            memory.set_args(&places.args, arg_values);
        }

        fn override_return_value(&mut self, value: Self::Value) {
            log_debug!(target: TAG, "Overriding the return value with {:?}", value);
            self.top_frame().overridden_return_val = Some(value);
        }

        fn grab_return_value(
            &mut self,
            mut token: Self::ReturnToken,
            memory: &mut impl CallShadowMemory<Self::Place, Value = Self::Value>,
        ) {
            let mut return_val = memory.take_place(
                &token
                    .popped_frame
                    .return_val_place
                    .as_ref()
                    .expect(MSG_DATA_UNAVAILABLE),
            );
            if let Some(overridden) = token.popped_frame.overridden_return_val.take() {
                return_val = overridden;
            }

            if !token.popped_frame.is_unexpected {
                self.ephemeral.from_callee.as_mut().unwrap().return_val = Some(return_val);
            } else {
                self.breakage_callback
                    .at_return_with_return_val(token.popped_frame.def, return_val);
            }
        }

        fn give_return_value(&mut self, token: Self::FinalizationToken) -> Self::Value {
            if let Some(overridden) = self.top_frame().overridden_return_val.take() {
                log_debug!(
                    target: TAG,
                    "Consuming the overridden return value as the return value for the function call.",
                );
                return overridden;
            }

            match token.0 {
                CallFlowSanity::Expected(from_callee) => {
                    from_callee.return_val.expect(MSG_DATA_UNAVAILABLE)
                }
                CallFlowSanity::Broken(unconsumed) => match unconsumed {
                    Either::Left(from_caller) => self.breakage_callback.after_return_with_args(
                        from_caller.expected_func,
                        self.current_func(),
                        from_caller.args.expect(MSG_DATA_UNAVAILABLE).values,
                    ),
                    Either::Right(from_callee) => {
                        self.breakage_callback.after_return_with_return_val(
                            from_callee.func,
                            self.current_func(),
                            from_callee.return_val.expect(MSG_DATA_UNAVAILABLE),
                        )
                    }
                },
                CallFlowSanity::Unknown(_) => unreachable!(),
            }
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
            _callee: Option<CalleeDef>,
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

        fn at_enter_with_return_val(
            &mut self,
            _callee: FuncDef,
            _current: FuncDef,
            _unconsumed_return_value: V,
        ) {
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

        fn after_return_with_return_val(
            &mut self,
            _callee: FuncDef,
            _current: FuncDef,
            _unconsumed_return_value: V,
        ) -> V {
            (self.unknown_value_factory)()
        }

        fn at_return_with_return_val(&mut self, _current: FuncDef, _unconsumed_return_value: V) {}
    }

    impl PartialEq<CalleeDef> for FuncDef {
        fn eq(&self, other: &CalleeDef) -> bool {
            if let Some((this, other)) = self.raw.as_ref().zip(other.raw.as_ref()) {
                if this.static_addr == other.static_addr {
                    true
                } else if this
                    .as_dyn_method
                    .zip(other.as_dyn_method)
                    .is_some_and(|(s, o)| s == o)
                {
                    true
                } else {
                    false
                }
            } else {
                self.body_id == other.callee_id
            }
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
                if !tracing::enabled!(target: TAG_STACK, tracing::Level::DEBUG) {
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
                if !tracing::enabled!(target: TAG_STACK, tracing::Level::DEBUG) {
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
