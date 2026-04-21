use rustc_middle::mir::{BasicBlockData, Operand};
use rustc_span::source_map::Spanned;
use rustc_type_ir::ClosureArgs;

use core::{assert_matches::debug_assert_matches, iter};

use crate::{
    passes::instr::{
        call::{PlaceReferencer, context::ConfigProvider},
        ctxtreqs::ForPlaceRef,
    },
    utils::mir::BodyExt,
};

use super::{
    DropHandler, FunctionHandler, InsertionLocation, OperandReferencer,
    context::{AssignmentInfoProvider, BodyProvider, SourceInfoProvider},
    ctxt_reqs::{Basic, ForDropping, ForFunctionCalling},
    prelude::{mir::*, *},
};

impl<'tcx, C> FunctionHandler<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + DebugInfoHandler,
    C: ForFunctionCalling<'tcx>,
{
    fn before_call_func(
        &mut self,
        func: &Operand<'tcx>,
        args: &[Spanned<Operand<'tcx>>],
        no_def: bool,
    ) {
        debug_assert_matches!(
            self.context.insertion_loc(),
            InsertionLocation::Before(..),
            "Inserting before_call after a block is not expected."
        );

        self.debug_info(&format!("{}", func.ty(self, self.tcx())));

        let mut added = false;
        if self.config().call_flow_filter.call_control {
            self.before_call_control(no_def, func, args.first().map(|a| &a.node));
            added = true;
        }

        if self.config().call_flow_filter.call_input {
            self.before_call_data(func, args);
            added = true;
        }

        if !added {
            self.before_call_some();
        }
    }

    fn enter_func(&mut self) {
        self.debug_info(&format!("{}", utils::body_func_ty(self.tcx(), self.body())));

        self.enter_func();

        if self.config().call_flow_filter.call_input {
            self.enter_func_data();
        }
    }

    fn return_from_func(&mut self) {
        let block =
            self.make_bb_for_call(sym::return_from_func, vec![self.original_bb_index_as_arg()]);
        self.insert_blocks([block]);
    }

    fn after_call_func(&mut self)
    where
        Self: AssignmentInfoProvider<'tcx>,
    {
        let block = self.make_bb_for_call(
            sym::after_call_func,
            vec![
                operand::const_from_uint(self.tcx(), self.assignment_id()),
                operand::copy_for_local(self.dest_ref().into()),
            ],
        );
        debug_assert_matches!(
            self.context.insertion_loc(),
            InsertionLocation::After(..),
            "Inserting after_call before a block is not expected."
        );
        self.insert_blocks([block]);
    }
}

impl<'tcx, C> DropHandler<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + DebugInfoHandler,
    C: ForDropping<'tcx>,
{
    fn before_call_drop(&mut self, place: &Place<'tcx>) {
        debug_assert_matches!(
            self.context.insertion_loc(),
            InsertionLocation::Before(..),
            "Inserting before_drop after a block is not expected."
        );

        let tcx = self.tcx();
        let func = operand::func(
            tcx,
            tcx.lang_items()
                .drop_in_place_fn()
                .expect("Lang item required for instrumenting drops"),
            [place.ty(self, tcx).ty.into()],
        );

        self.debug_info(&format!("{}", func.ty(self, self.tcx())));

        let mut added = false;
        if self.config().drop_filter.control {
            self.before_drop_control(func.clone());
            added = true;
        }

        if self.config().drop_filter.input {
            self.before_drop_data(&func, place.clone());
            added = true;
        }

        if !added {
            self.before_drop_some();
        }
    }

    fn after_call_drop(&mut self) {
        let block = self.make_bb_for_call(sym::after_drop, vec![]);
        debug_assert_matches!(
            self.context.insertion_loc(),
            InsertionLocation::After(..),
            "Inserting after_call before a block is not expected."
        );
        self.insert_blocks([block]);
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: Basic<'tcx> + SourceInfoProvider,
{
    fn before_call_control(
        &mut self,
        no_def: bool,
        func: &Operand<'tcx>,
        first_arg: Option<&Operand<'tcx>>,
    ) where
        C: ForFunctionCalling<'tcx>,
    {
        let mut blocks = vec![];

        let def_local = {
            let func = if !no_def {
                func.clone()
            } else {
                operand::func(
                    self.tcx(),
                    *self.pri_helper_funcs().special_func_placeholder,
                    iter::empty(),
                )
            };
            let BlocksAndResult(def_blocks, def_local) =
                definition_of_callee(self.tcx(), self, func, first_arg);
            blocks.extend(def_blocks);
            def_local
        };

        blocks.push(self.make_bb_for_call(
            sym::before_call_control,
            vec![
                operand::move_for_local(def_local),
                self.original_bb_index_as_arg(),
            ],
        ));

        self.insert_blocks(blocks);
    }

    fn before_call_data(&mut self, func: &Operand<'tcx>, args: &[Spanned<Operand<'tcx>>])
    where
        C: ForFunctionCalling<'tcx>,
    {
        let tcx = self.tcx();
        let mut blocks = vec![];

        let func_ref = self.reference_operand(func);

        let arg_refs = args
            .iter()
            .map(|a| self.reference_operand_spanned(a))
            .map(|a| operand::move_for_local(a.into()))
            .collect();
        let operand_ref_ty = self.context.pri_types().operand_ref(tcx);
        let (arguments_local, additional_stmts) =
            utils::prepare_operand_for_slice(tcx, &mut self.context, operand_ref_ty, arg_refs);

        let are_args_tupled = are_args_tupled(
            tcx,
            self,
            func,
            args.iter().map(|a| &a.node),
            self.current_typing_env(),
        );

        let mut block = self.make_bb_for_call(
            sym::before_call_data,
            vec![
                operand::move_for_local(func_ref.into()),
                operand::move_for_local(arguments_local),
                operand::const_from_bool(tcx, are_args_tupled),
            ],
        );
        block.statements.extend(additional_stmts);
        blocks.push(block);

        self.insert_blocks(blocks);
    }

    fn before_call_some(&mut self) {
        let block = self.make_bb_for_call(sym::before_call_some, vec![]);
        self.insert_blocks([block]);
    }

    fn enter_func(&mut self) {
        let mut blocks = vec![];

        let def_local = {
            let BlocksAndResult(def_blocks, def_local) =
                utils::definition_of_func(self.tcx(), self, self.current_typing_env());
            blocks.extend(def_blocks);
            def_local
        };

        blocks
            .push(self.make_bb_for_call(sym::enter_func, vec![operand::move_for_local(def_local)]));

        self.insert_blocks(blocks);
    }

    fn enter_func_data(&mut self)
    where
        C: ForPlaceRef<'tcx>,
    {
        let tcx = self.tcx();
        let mut blocks = vec![];

        let (argument_places_local, additional_stmts) = {
            let arg_places_refs = self
                .body()
                .args_iter_x()
                .map(|a| self.reference_place_local(a))
                .map(|BlocksAndResult(ref_blocks, place_ref)| {
                    blocks.extend(ref_blocks);
                    place_ref
                })
                .map(|place_ref| operand::move_for_local(place_ref))
                .collect();
            let place_ref_ty = self.context.pri_types().place_ref(tcx);
            utils::prepare_operand_for_slice(tcx, &mut self.context, place_ref_ty, arg_places_refs)
        };

        let ret_val_place_local = {
            let BlocksAndResult(ref_blocks, place_ref) =
                self.reference_place_local(Place::return_place().as_local().unwrap());
            blocks.extend(ref_blocks);
            place_ref
        };

        let base_args = vec![
            operand::move_for_local(argument_places_local),
            operand::move_for_local(ret_val_place_local),
        ];

        let mut block = if let TyKind::Closure(_, args) = tcx
            .type_of(self.current_func_id())
            .instantiate_identity()
            .kind()
        {
            let (arg_blocks, tupled_args) = self.make_enter_func_tupled_args(args.as_closure());
            blocks.extend(arg_blocks);
            self.make_bb_for_call(
                sym::enter_func_data_untupled_args,
                [base_args, tupled_args.to_vec()].concat(),
            )
        } else if utils::is_fn_trait_call_func(tcx, self.current_func_id()) {
            self.make_bb_for_call(sym::enter_func_data_tupled_args, base_args)
        } else {
            self.make_bb_for_call(sym::enter_func_data, base_args)
        };

        block.statements.extend(additional_stmts);
        blocks.push(block);

        self.insert_blocks(blocks);
    }

    fn make_enter_func_tupled_args(
        &mut self,
        args: ClosureArgs<TyCtxt<'tcx>>,
    ) -> (Vec<BasicBlockData<'tcx>>, [Operand<'tcx>; 2]) {
        let mut blocks = vec![];

        let tuple_id_local = {
            let (block, id_local) =
                self.make_type_id_of_bb(ty::erased_tupled_closure_inputs(self.tcx(), args));
            blocks.push(block);
            id_local
        };

        (
            blocks,
            [
                operand::const_from_uint(self.tcx(), 2 as common::types::LocalIndex),
                operand::move_for_local(tuple_id_local),
            ],
        )
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: Basic<'tcx> + SourceInfoProvider,
{
    fn before_drop_control(&mut self, drop_in_place_fn: Operand<'tcx>)
    where
        C: ForDropping<'tcx>,
    {
        let mut blocks = vec![];

        let def_local = {
            let BlocksAndResult(def_blocks, def_local) =
                definition_of_callee(self.tcx(), self, drop_in_place_fn, None);
            blocks.extend(def_blocks);
            def_local
        };

        blocks.push(self.make_bb_for_call(
            sym::before_drop_control,
            vec![
                operand::move_for_local(def_local),
                self.original_bb_index_as_arg(),
            ],
        ));

        self.insert_blocks(blocks);
    }

    fn before_drop_data(&mut self, drop_in_place_fn: &Operand<'tcx>, place: Place<'tcx>)
    where
        C: ForDropping<'tcx>,
    {
        let tcx = self.tcx();
        let mut blocks = vec![];

        let func_ref = self.reference_operand(drop_in_place_fn);

        let place_ref = self.reference_place(&place);
        let arg_ref = {
            let (additional_stmt, ptr_local) =
                utils::ptr_to_place(tcx, self, place, place.ty(self, tcx).ty);
            let BlocksAndResult(mut additional_blocks, local) =
                self.internal_reference_operand(&operand::move_for_local(ptr_local));
            additional_blocks
                .first_mut()
                .unwrap()
                .statements
                .insert(0, additional_stmt);
            blocks.extend(additional_blocks);
            local
        };

        let block = self.make_bb_for_call(
            sym::before_drop_data,
            vec![
                operand::move_for_local(func_ref.into()),
                operand::move_for_local(arg_ref),
                operand::move_for_local(place_ref.into()),
            ],
        );
        blocks.push(block);

        self.insert_blocks(blocks);
    }

    fn before_drop_some(&mut self) {
        let block = self.make_bb_for_call(sym::before_drop_some, vec![]);
        self.insert_blocks([block]);
    }
}

mod utils {
    use rustc_middle::{
        mir::{Body, Local, Operand, Place, Rvalue, Statement},
        ty::{
            AssocItem, ClosureArgs, ExistentialPredicateStableCmpExt, InstanceKind, PolyFnSig,
            TraitRef, Ty, TyCtxt, TyKind, TypingEnv,
        },
    };
    use rustc_span::def_id::DefId;
    use rustc_trait_selection::traits::is_vtable_safe_method;

    use common::{log_debug, log_info};
    use itertools::Itertools;

    use core::{assert_matches::debug_assert_matches, iter};

    use crate::{
        passes::instr::{MirSourceExt, decision::rules::accept_dyn_def_filter_rules},
        utils::mir::{BodyExt, InstanceKindExt},
    };

    pub(super) use super::super::utils::{
        assignment, operand, prepare_operand_for_slice, ptr_to_place, terminator, ty::TyExt,
    };
    use super::super::{
        BlocksAndResult, BodyLocalManager, BodyProvider, HasLocalDecls, MirCallAdder,
        PriItemsProvider, StorageProvider,
    };

    pub(super) mod rvalue {
        pub use super::super::super::utils::rvalue::*;

        use super::*;

        pub fn cast_to_fn_ptr<'tcx>(
            tcx: TyCtxt<'tcx>,
            operand: Operand<'tcx>,
            operand_ty: Ty<'tcx>,
        ) -> Rvalue<'tcx> {
            cast_to_coerced_as(
                rustc_middle::ty::adjustment::PointerCoercion::ReifyFnPointer(
                    rustc_hir::Safety::Safe,
                ),
                operand,
                Ty::new_fn_ptr(tcx, ty::fn_ptr_sig(tcx, operand_ty)),
            )
        }
    }

    pub(super) mod ty {

        use super::*;

        /// Returns the corresponding FnDef type of a closure when called,
        /// i.e. `<closure as Fn*<I>>::call*()`
        ///
        /// [`ty`]: closure type
        pub fn fn_def_of_closure_call<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
            let TyKind::Closure(_, args) = ty.kind() else {
                panic!("Expected closure type but received: {}", ty)
            };

            log_debug!("Getting FnDef type of closure: {:?}", ty);
            let args = args.as_closure();

            // Finding the call* method in the Fn* trait.
            let fn_trait_fn_id = def_id_of_single_func_of_trait(
                tcx,
                tcx.fn_trait_kind_to_def_id(args.kind()).unwrap(),
            );

            let inputs = erased_tupled_closure_inputs(tcx, args);
            Ty::new_fn_def(tcx, fn_trait_fn_id, [ty, inputs])
        }

        pub fn erased_tupled_closure_inputs<'tcx>(
            tcx: TyCtxt<'tcx>,
            args: ClosureArgs<TyCtxt<'tcx>>,
        ) -> Ty<'tcx> {
            // Inputs types are collated into a tuple and are the only generic argument of the Fn trait.
            let inputs = args.sig().inputs().map_bound(|inputs| inputs[0]);
            tcx.instantiate_bound_regions_with_erased(inputs)
        }

        /// Returns the corresponding FnDef type of a coroutine when resumed,
        /// i.e. `<coroutine as Coroutine<R>>::resume()`
        ///
        /// [`ty`]: coroutine type
        pub fn fn_def_of_coroutine_resume<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
            let TyKind::Coroutine(_, args) = ty.kind() else {
                panic!("Expected coroutine type but received: {}", ty)
            };
            log_debug!("Getting FnDef type of coroutine: {:?}", ty);
            let args = args.as_coroutine();

            // Finding `resume` method in `Coroutine` trait.
            let coroutine_trait_fn_id =
                def_id_of_single_func_of_trait(tcx, tcx.lang_items().coroutine_trait().unwrap());

            // NOTE: Currently, either zero or one parameters are supported for coroutines.
            let inputs = args.sig().resume_ty;
            Ty::new_fn_def(tcx, coroutine_trait_fn_id, [ty, inputs])
        }

        /// Returns the corresponding FnDef type of an async coroutine when awaited,
        /// i.e. `<coroutine as Future>::poll()`
        ///
        /// [`ty`]: coroutine type
        pub fn fn_def_of_coroutine_await<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
            let TyKind::Coroutine(def_id, args) = ty.kind() else {
                panic!("Expected coroutine type but received: {}", ty)
            };
            assert!(
                tcx.coroutine_is_async(*def_id),
                "Expected async coroutine kind but received: {:?}",
                tcx.coroutine_kind(*def_id),
            );

            log_debug!("Getting FnDef type of async coroutine: {:?}", ty);
            let args = args.as_coroutine();

            // Finding `resume` method in `Coroutine` trait.
            let future_trait_fn_id = tcx.lang_items().future_poll_fn().unwrap();

            Ty::new_fn_def(tcx, future_trait_fn_id, [ty])
        }

        /// Returns the corresponding FnDef type of calling a function through Fn* traits.
        /// i.e. `<fn_ty as Fn*<I>>::call*()`
        pub fn fn_def_of_fn_ptr_shim<'tcx>(
            tcx: TyCtxt<'tcx>,
            fn_trait_fn_id: DefId,
            fn_ty: Ty<'tcx>,
        ) -> Ty<'tcx> {
            debug_assert_matches!(fn_ty.kind(), TyKind::FnPtr(..) | TyKind::FnDef(..));
            let inputs = fn_ty
                .fn_sig(tcx)
                .inputs()
                .map_bound(|ts| Ty::new_tup(tcx, ts));
            let inputs = tcx.instantiate_bound_regions_with_erased(inputs);
            Ty::new_fn_def(tcx, fn_trait_fn_id, [fn_ty, inputs])
        }

        pub fn fn_def_of_closure_once_shim<'tcx>(
            tcx: TyCtxt<'tcx>,
            fn_trait_fn_id: DefId,
            input_arg_tys: &[Ty<'tcx>],
        ) -> Ty<'tcx> {
            let [closure_ty, args_ty] = input_arg_tys else {
                panic!(
                    "Expected two arguments for (Self, (Args)) but received: {:?}",
                    input_arg_tys
                )
            };

            Ty::new_fn_def(tcx, fn_trait_fn_id, [*closure_ty, *args_ty])
        }

        pub fn fn_def_of_clone_shim<'tcx>(
            tcx: TyCtxt<'tcx>,
            clone_fn_id: DefId,
            ty: Ty<'tcx>,
        ) -> Ty<'tcx> {
            let TyKind::Ref(_, ty, _) = ty.kind() else {
                /* NOTE: It is not really obvious where or why,
                 * but apparently a reference to the type is passed for the shim. */
                panic!("Expected reference type but received: {:?}", ty)
            };
            Ty::new_fn_def(tcx, clone_fn_id, [*ty])
        }

        fn def_id_of_single_func_of_trait(tcx: TyCtxt, trait_id: DefId) -> DefId {
            let mut funcs = tcx
                .associated_items(trait_id)
                .in_definition_order()
                .filter(|x| matches!(x.kind, rustc_middle::ty::AssocKind::Fn { .. }));
            let func = funcs.next().expect("No function found in the trait.");
            assert!(
                funcs.next().is_none(),
                "More than one function found in the trait."
            );
            func.def_id
        }

        pub fn fn_ptr_sig<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> PolyFnSig<'tcx> {
            match ty.kind() {
                TyKind::FnDef(..) => ty.fn_sig(tcx),
                TyKind::FnPtr(tys, header) => tys.with(*header),
                _ => unreachable!(
                    "Unexpected type to get function pointer directly from: {}",
                    ty
                ),
            }
        }
    }

    pub fn body_func_ty<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Ty<'tcx> {
        let source = body.source;
        log_debug!("Creating type of current function: {}", source.to_log_str());

        use InstanceKind::*;
        let fn_def_ty = match source.instance {
            Item(def_id) => {
                let ty = tcx.type_of(def_id).instantiate_identity();
                match ty.kind() {
                    TyKind::FnDef(..) => ty,
                    TyKind::Closure(..) => ty::fn_def_of_closure_call(tcx, ty),
                    TyKind::Coroutine(def_id, ..) => {
                        use rustc_hir::{CoroutineDesugaring::*, CoroutineKind::*};
                        match tcx.coroutine_kind(*def_id).unwrap() {
                            Coroutine(..) => ty::fn_def_of_coroutine_resume(tcx, ty),
                            Desugared(Async, ..) => ty::fn_def_of_coroutine_await(tcx, ty),
                            Desugared(des, ..) => unimplemented!(
                                "This type of coroutine is unstable and currently out of scope: {:?}, source: {:?}",
                                des,
                                ty,
                            ),
                        }
                    }
                    _ => unreachable!("Unexpected type for body instance: {:?}", ty),
                }
            }
            ReifyShim(def_id, _) => tcx.type_of(def_id).instantiate_identity(),
            FnPtrShim(fn_trait_fn_id, fn_ptr_ty) => {
                ty::fn_def_of_fn_ptr_shim(tcx, fn_trait_fn_id, fn_ptr_ty)
            }
            ClosureOnceShim { call_once, .. } => {
                let arg_tys = body
                    .args_iter()
                    .map(|local| body.local_decls()[local].ty)
                    .collect::<Vec<_>>();
                ty::fn_def_of_closure_once_shim(tcx, call_once, &arg_tys)
            }
            CloneShim(clone_fn_id, self_ty) => ty::fn_def_of_clone_shim(tcx, clone_fn_id, self_ty),
            DropGlue(def_id, Some(ty)) => Ty::new_fn_def(tcx, def_id, [ty]),
            instance @ _ => unreachable!("Unsupported instance: {:?}", instance),
        };

        debug_assert_matches!(fn_def_ty.kind(), TyKind::FnDef(..));
        fn_def_ty
    }

    pub(super) fn is_fn_trait_method_call(tcx: TyCtxt, func_ty: Ty) -> bool {
        let TyKind::FnDef(def_id, ..) = func_ty.kind() else {
            return false;
        };
        tcx.trait_of_assoc(*def_id)
            .is_some_and(|id| tcx.is_fn_trait(id))
    }

    pub fn definition_of_func<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (
                 impl BodyLocalManager<'tcx>
                 + BodyProvider<'tcx>
                 + MirCallAdder<'tcx>
                 + PriItemsProvider<'tcx>
                 + StorageProvider
             ),
        typing_env: TypingEnv<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let instance_kind = call_adder.body().source.instance;
        let fn_def_ty = body_func_ty(tcx, call_adder.body());
        let TyKind::FnDef(def_id, generic_args) = *fn_def_ty.kind() else {
            unreachable!(
                "Expected function definition type but received: {}",
                fn_def_ty
            )
        };

        let fn_value = operand::func(tcx, def_id, generic_args);

        if let Some((trait_ref, trait_item)) = as_dyn_compatible_method(tcx, def_id)
            && trait_ref.self_ty().is_sized(tcx, typing_env)
        {
            let ruled_out = accept_dyn_def_filter_rules(call_adder.storage(), &(tcx, def_id))
                .map_or(false, |include| !include);
            if ruled_out {
                log_info!(
                    "Dyn-compatible method will be defined as static: {:?}",
                    def_id
                );
                def_of_static_func(tcx, call_adder, instance_kind, fn_value)
            } else {
                def_of_dyn_compatible_method(
                    tcx,
                    call_adder,
                    instance_kind,
                    fn_value,
                    trait_ref,
                    trait_item.def_id,
                )
            }
        } else {
            def_of_static_func(tcx, call_adder, instance_kind, fn_value)
        }
    }

    fn def_of_static_func<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>),
        instance_kind: InstanceKind<'tcx>,
        fn_value: Operand<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
            to_fn_ptr(tcx, call_adder, fn_value.clone());

        let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
            call_adder.pri_helper_funcs().func_def_static,
            vec![fn_ptr_ty.into()],
            [
                vec![operand::move_for_local(fn_ptr_local)],
                instance_kind_id_operand_triple(tcx, instance_kind).into(),
            ]
            .concat(),
            None,
        );
        block.statements.push(ptr_assignment);
        BlocksAndResult::from((block, id_local))
    }

    fn as_dyn_compatible_method<'tcx>(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
    ) -> Option<(TraitRef<'tcx>, AssocItem)> {
        let trait_ref = tcx
            .impl_of_assoc(def_id)
            .and_then(|impl_id| tcx.impl_opt_trait_ref(impl_id))
            .map(|trait_ref| trait_ref.instantiate_identity())
            .filter(|trait_ref| tcx.is_dyn_compatible(trait_ref.def_id))
            .filter(|trait_ref| trait_ref.def_id != tcx.lang_items().deref_trait().unwrap())?;

        let item = to_trait_associated_item(tcx, def_id);
        is_vtable_safe_method(tcx, trait_ref.def_id, item).then_some((trait_ref, item))
    }

    fn def_of_dyn_compatible_method<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (
                 impl BodyLocalManager<'tcx>
                 + BodyProvider<'tcx>
                 + MirCallAdder<'tcx>
                 + PriItemsProvider<'tcx>
             ),
        instance_kind: InstanceKind<'tcx>,
        fn_value: Operand<'tcx>,
        trait_ref: TraitRef<'tcx>,
        method_id: DefId,
    ) -> BlocksAndResult<'tcx> {
        let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
            to_fn_ptr(tcx, call_adder, fn_value.clone());
        let self_ty = trait_ref.self_ty();
        let dyn_ty = dyn_ty_from_impl(tcx, trait_ref);

        let identifier = identifier_of_method(tcx, method_id);

        let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
            call_adder.pri_helper_funcs().func_def_dyn_method,
            vec![fn_ptr_ty.into(), self_ty.into(), dyn_ty.into()],
            [
                vec![
                    operand::move_for_local(fn_ptr_local),
                    operand::const_from_uint(tcx, identifier),
                ],
                instance_kind_id_operand_triple(tcx, instance_kind).into(),
            ]
            .concat(),
            None,
        );
        block.statements.push(ptr_assignment);

        (block, id_local).into()
    }

    /* dyn Tr<U, V, A1 = X, A2 = Y, ...>
     * where we have:
     * impl Tr<U, V> for T {
     *   type A1 = X;
     *   type A2 = Y;
     *   ...
     * }
     *
     * Another representation is:
     * dyn Tr<U, V, A1 = <T as Tr<U, V>>::A1, A2 = <T as Tr<U, V>>::A2, ...>
     */
    fn dyn_ty_from_impl<'tcx>(tcx: TyCtxt<'tcx>, trait_ref: TraitRef<'tcx>) -> Ty<'tcx> {
        Ty::new_dynamic(
            tcx,
            {
                use rustc_type_ir::{
                    Binder, ExistentialPredicate, ExistentialProjection, ExistentialTraitRef,
                    elaborate,
                };
                // Source: the assertion in `new_dynamic`.
                let associated_types = elaborate::supertraits(tcx, Binder::dummy(trait_ref))
                    .flat_map(|principal| {
                        tcx.associated_items(principal.def_id())
                            .in_definition_order()
                            .filter(|item| item.is_type())
                            .filter(|item| !item.is_impl_trait_in_trait())
                            .filter(|item| !tcx.generics_require_sized_self(item.def_id))
                            .map(move |item| {
                                (tcx.instantiate_bound_regions_with_erased(principal), item)
                            })
                    })
                    .collect_vec();
                tcx.mk_poly_existential_predicates_from_iter(
                    // impl Tr<U, V>
                    iter::once(ExistentialPredicate::Trait(
                        ExistentialTraitRef::erase_self_ty(tcx, trait_ref),
                    ))
                    // A1 = X, A2 = Y, ...
                    .chain(associated_types.into_iter().map(|(principal, item)| {
                        // <T as Tr<U, V>>::A1
                        let proj_term =
                            Ty::new_projection_from_args(tcx, item.def_id, principal.args).into();
                        ExistentialPredicate::Projection(ExistentialProjection::new(
                            tcx,
                            item.def_id,
                            ExistentialTraitRef::erase_self_ty(tcx, principal).args,
                            proj_term,
                        ))
                    }))
                    // Required by: `mk_poly_existential_predicates`
                    .sorted_by(|a, b| a.stable_cmp(tcx, b))
                    .map(|p| Binder::dummy(p)),
                )
            },
            tcx.lifetimes.re_erased,
        )
    }

    pub fn definition_of_callee<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>),
        fn_value: Operand<'tcx>,
        first_arg: Option<&Operand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        let fn_ty = fn_value.ty(call_adder, tcx);

        match fn_ty.kind() {
            TyKind::FnDef(def_id, generic_args) => {
                let def_id = *def_id;
                /* NOTE: As the bodies are still polymorphic, it is still possible that the
                 * instance is not resolvable. Thus we cannot use `expect_resolve` to decide
                 * at this point. */
                // let is_dynamic = UNKNOWN;

                if let Some(item) = as_possibly_dynamic_method_call(tcx, def_id) {
                    let receiver = first_arg.unwrap_or_else(|| {
                        panic!("Expected receiver for a dyn-compatible method: {:?}", fn_ty)
                    });
                    let self_ty = generic_args.type_at(0);
                    def_of_possibly_virtual_callee(
                        tcx,
                        call_adder,
                        fn_value,
                        item.def_id,
                        self_ty,
                        receiver,
                    )
                } else {
                    def_of_static_callee(tcx, call_adder, fn_value)
                }
            }
            TyKind::FnPtr(..) => def_of_static_callee(tcx, call_adder, fn_value),
            _ => {
                unreachable!("Unexpected type of callee: {:?}: {:?}", fn_value, fn_ty);
            }
        }
    }

    fn def_of_static_callee<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>),
        fn_value: Operand<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let (fn_ptr_ty, fn_ptr_local, ptr_assignment) = to_fn_ptr(tcx, call_adder, fn_value);

        let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
            call_adder.pri_helper_funcs().callee_def_static,
            vec![fn_ptr_ty.into()],
            vec![operand::move_for_local(fn_ptr_local)],
            None,
        );
        block.statements.push(ptr_assignment);
        BlocksAndResult::from((block, id_local))
    }

    fn def_of_possibly_virtual_callee<'tcx>(
        tcx: TyCtxt<'tcx>,
        call_adder: &mut (impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>),
        fn_value: Operand<'tcx>,
        method_id: DefId,
        self_ty: Ty<'tcx>,
        receiver: &Operand<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let (fn_ptr_ty, fn_ptr_local, ptr_assignment) = to_fn_ptr(tcx, call_adder, fn_value);

        let receiver_ty = receiver.ty(call_adder, tcx);
        // receiver -> &receiver
        let (receiver_ref_local, receiver_ref_stmts) = {
            let mut statements = Vec::new();
            let receiver_place = match receiver {
                Operand::Copy(place) | Operand::Move(place) => *place,
                Operand::Constant(..) => {
                    let local = call_adder.add_local(receiver_ty);
                    let assignment =
                        assignment::create(Place::from(local), Rvalue::Use(receiver.clone()));
                    statements.push(assignment);
                    local.into()
                }
                Operand::RuntimeChecks(..) => {
                    unreachable!("Unexpected runtime checks operand as receiver.")
                }
            };
            let receiver_ref_local =
                call_adder.add_local(Ty::new_imm_ref(tcx, tcx.lifetimes.re_erased, receiver_ty));
            let receiver_ref_assignment = super::assignment::create(
                receiver_ref_local.into(),
                rvalue::ref_of(receiver_place, tcx),
            );
            statements.push(receiver_ref_assignment);
            (receiver_ref_local, statements)
        };
        let identifier = identifier_of_method(tcx, method_id);

        let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
            call_adder.pri_helper_funcs().callee_def_maybe_virtual,
            vec![fn_ptr_ty.into(), self_ty.into(), receiver_ty.into()],
            vec![
                operand::move_for_local(fn_ptr_local),
                operand::move_for_local(receiver_ref_local),
                operand::const_from_uint(tcx, identifier),
            ],
            Default::default(),
        );
        block.statements.push(ptr_assignment);
        block.statements.extend(receiver_ref_stmts);
        (block, id_local).into()
    }

    fn as_possibly_dynamic_method_call<'tcx>(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
    ) -> Option<AssocItem> {
        let trait_id = tcx.trait_of_assoc(def_id)?;
        let item = to_trait_associated_item(tcx, def_id);
        is_vtable_safe_method(tcx, trait_id, item).then_some(item)
    }

    fn to_fn_ptr<'tcx>(
        tcx: TyCtxt<'tcx>,
        local_manager: &mut impl BodyLocalManager<'tcx>,
        fn_value: Operand<'tcx>,
    ) -> (Ty<'tcx>, Local, Statement<'tcx>) {
        let value_ty = fn_value.ty(local_manager, tcx);
        let fn_ptr_ty = Ty::new_fn_ptr(tcx, ty::fn_ptr_sig(tcx, value_ty));
        let fn_rvalue = if fn_ptr_ty != value_ty {
            rvalue::cast_to_fn_ptr(tcx, fn_value, value_ty)
        } else {
            Rvalue::Use(fn_value)
        };
        let fn_ptr_local = local_manager.add_local(fn_ptr_ty);
        let fn_ptr_assign = assignment::create(Place::from(fn_ptr_local), fn_rvalue);
        (fn_ptr_ty, fn_ptr_local, fn_ptr_assign)
    }

    /// Returns the corresponding trait item of a trait method or an implementation of it.
    fn to_trait_associated_item<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> AssocItem {
        let item = tcx.associated_item(def_id);
        if let Some(id) = item.trait_item_def_id()
            && id != def_id
        {
            to_trait_associated_item(tcx, id)
        } else {
            item
        }
    }

    fn identifier_of_method<'tcx>(tcx: TyCtxt<'tcx>, method: DefId) -> u64 {
        let method_item = to_trait_associated_item(tcx, method);
        let trait_id = method_item
            .trait_container(tcx)
            .expect("Expected a trait method");
        tcx.associated_items(trait_id)
            .in_definition_order()
            .position(|item| item == &method_item)
            .unwrap() as u64
    }

    pub fn are_args_tupled<'tcx: 'a, 'a>(
        tcx: TyCtxt<'tcx>,
        local_manager: &impl HasLocalDecls<'tcx>,
        callee: &Operand<'tcx>,
        args: impl Iterator<Item = &'a Operand<'tcx>>,
        typing_env: TypingEnv<'tcx>,
    ) -> bool {
        // Tupling is only observed in fn trait (closure) calls.
        if !is_fn_trait_method_call(tcx, callee.ty(local_manager, tcx)) {
            return false;
        }

        // Ensure assumptions that runtime may rely upon.
        let args = args.collect::<Vec<_>>();
        assert_eq!(args.len(), 2);
        assert!(
            args.last()
                .unwrap()
                .ty(local_manager, tcx)
                .is_tuple(tcx, typing_env),
            "Fn trait method call without tupled arguments observed. {:?}, {:?}",
            callee,
            args.iter()
                .map(|a| a.ty(local_manager, tcx))
                .collect::<Vec<_>>()
        );
        true
    }

    pub fn instance_kind_id_operand_triple<'tcx>(
        tcx: TyCtxt<'tcx>,
        instance_kind: InstanceKind<'tcx>,
    ) -> [Operand<'tcx>; 3] {
        let def_id = def_id_operand_pair(tcx, instance_kind.def_id());
        [
            operand::const_from_uint(tcx, instance_kind.discriminant()),
            def_id.0,
            def_id.1,
        ]
    }

    fn def_id_operand_pair<'tcx>(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
    ) -> (Operand<'tcx>, Operand<'tcx>) {
        (
            operand::const_from_uint(tcx, def_id.krate.as_u32()),
            operand::const_from_uint(tcx, def_id.index.as_u32()),
        )
    }

    pub fn is_fn_trait_call_func<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> bool {
        matches!(tcx.def_kind(def_id), rustc_hir::def::DefKind::AssocFn)
            && to_trait_associated_item(tcx, def_id)
                .trait_container(tcx)
                .is_some_and(|trait_id| tcx.is_fn_trait(trait_id))
    }
}
use utils::*;
