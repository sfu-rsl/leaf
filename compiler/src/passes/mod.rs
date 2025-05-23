mod codegen;
mod program_dep;
mod instr;
pub(crate) mod logger;
mod noop;
mod p_map_exp;
mod runtime_adder;
pub(crate) mod tyexp;

use common::log_debug;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rustc_ast as ast;
use rustc_driver::{self as driver, Compilation};
use rustc_interface::interface;
use rustc_middle::{mir, ty as mir_ty};
use rustc_session::Session;

use paste::paste;

use self::implementation::CompilationPassAdapter;
use crate::config::LeafCompilerConfig;
use crate::utils::Chain;

pub(crate) use codegen::MonoItemInternalizer;
pub(crate) use program_dep::ProgramDependenceMapExporter;
pub(crate) use instr::Instrumentor;
pub(crate) use logger::CompilationPassLogExt;
#[allow(unused)]
pub(crate) use noop::NoOpPass;
pub(crate) use noop::OverrideFlagsForcePass;
pub(crate) use p_map_exp::ProgramMapExporter;
pub(crate) use runtime_adder::RuntimeExternCrateAdder;
pub(crate) use tyexp::TypeExporter;

pub(super) type Callbacks = dyn CallbacksExt + Send;

pub(crate) trait HasResult<R> {
    fn into_result(self) -> R;
}

macro_rules! visit_before_after {
    (fn $name:ident $($sig:tt)*) => {
        paste!{
            fn [<visit_ $name _before>] $($sig)*

            fn [<visit_ $name _after>] $($sig)*
        }
    };
}

#[allow(unused)]
pub(crate) trait CompilationPass {
    fn override_flags() -> OverrideFlags {
        OverrideFlags::empty()
    }

    visit_before_after! {
        fn ast(&mut self, krate: &ast::Crate, storage: &mut dyn Storage) -> Compilation {
            Compilation::Continue
        }
    }

    fn transform_ast(
        &mut self,
        session: &Session,
        krate: &mut ast::Crate,
        storage: &mut dyn Storage,
    ) {
        Default::default()
    }

    fn visit_tcx_after_analysis(
        &mut self,
        tcx: mir_ty::TyCtxt,
        storage: &mut dyn Storage,
    ) -> Compilation {
        Compilation::Continue
    }

    visit_before_after! {
        fn tcx_at_codegen(
            &mut self,
            tcx: mir_ty::TyCtxt,
            storage: &mut dyn Storage,
        ) {
            Default::default()
        }
    }

    visit_before_after! {
        fn mir_body<'tcx>(
            tcx: mir_ty::TyCtxt<'tcx>,
            body: &mir::Body<'tcx>,
            storage: &mut dyn Storage,
        ) {
            Default::default()
        }
    }

    /* As we need function pointers in the query structures, the following functions
     * cannot take self. Users may want to work with storage to work in a stateful
     * fashion.
     */
    fn transform_mir_body<'tcx>(
        tcx: mir_ty::TyCtxt<'tcx>,
        body: &mut mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        Default::default()
    }

    fn visit_codegen_units<'tcx>(
        tcx: mir_ty::TyCtxt<'tcx>,
        units: &mut [mir::mono::CodegenUnit<'tcx>],
        storage: &mut dyn Storage,
    ) {
        Default::default()
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy)]
    pub(crate) struct OverrideFlags: u8 {
        const OPTIMIZED_MIR = 1;
        const EXTERN_OPTIMIZED_MIR = 1 << 1;
        const MIR_SHIMS = 1 << 5;

        const SHOULD_CODEGEN = 1 << 2;

        const COLLECT_PARTITION = 1 << 3;

        const MAKE_CODEGEN_BACKEND = 1 << 4;
    }
}

pub(crate) trait CompilationPassExt: CompilationPass {
    fn to_callbacks(self) -> CompilationPassAdapter<Self>
    where
        Self: Send;
}
impl<T: CompilationPass + Send + Sync + 'static> CompilationPassExt for T {
    fn to_callbacks(self) -> CompilationPassAdapter<Self> {
        CompilationPassAdapter::new(self, Self::override_flags())
    }
}

pub(crate) trait CallbacksExt: driver::Callbacks {
    fn set_leaf_config(&mut self, config: LeafCompilerConfig);

    fn add_config_callback(
        &mut self,
        callback: Box<dyn FnOnce(&mut interface::Config, &mut LeafCompilerConfig) + Send + 'static>,
    );
}

use implementation::ValueBorrow;
pub(crate) trait Storage: std::fmt::Debug {
    fn get_raw_or_insert_with<'a>(
        &'a mut self,
        key: String,
        default: Box<dyn FnOnce(&mut dyn Storage) -> Box<dyn Any> + '_>,
    ) -> ValueBorrow<'a>;

    fn get_raw_mut<'a>(&'a mut self, key: &String) -> Option<ValueBorrow<'a>>;
}

pub(crate) trait StorageExt {
    type MutAccessor<'a, T>
    where
        Self: 'a,
        T: 'a;
    type ManualBorrow<T>;

    fn get_or_insert_with<'a, V: Any>(
        &'a mut self,
        key: String,
        default: impl FnOnce() -> V + 'a,
    ) -> Self::MutAccessor<'a, V> {
        self.get_or_insert_with_acc(key, |_| default())
    }

    fn get_or_insert_with_acc<'a, V: Any>(
        &'a mut self,
        key: String,
        default: impl FnOnce(&mut dyn Storage) -> V + 'a,
    ) -> Self::MutAccessor<'a, V>;

    fn get_or_default<'a, V: Default + Any>(&'a mut self, key: String) -> Self::MutAccessor<'a, V> {
        self.get_or_insert_with(key, V::default)
    }

    fn get_mut<'a, V: Any>(&'a mut self, key: &String) -> Option<Self::MutAccessor<'a, V>>;
}

mod implementation {
    use super::*;

    use mir::mono::{CodegenUnit, MonoItemPartitions};
    use mir_ty::TyCtxt;
    use rustc_driver::Compilation;
    use rustc_hir::def_id::DefId;
    use rustc_middle::query::TyCtxtAt;
    use rustc_span::{Symbol, def_id::LocalDefId};

    use std::cell::Cell;
    type RRef<T> = std::rc::Rc<std::cell::RefCell<T>>;

    pub(crate) use storage::ValueBorrow;

    /* NOTE: How does `queries` work?
     * Here, the queries object is a structure that performs compiler-related
     * operations in a lazy fashion (on the first call they will be performed and
     * the result is reused afterwards).
     * The result is stored in the shape of Steel object. A `Steel` permits
     * immutable and mutable borrows to the data it holds. Also, provides a `steel`
     * mechanism to take the ownership of the data. Once it is done, the data cannot
     * be accessed anymore. The important point is that the driver itself steels the
     * data at particular point. For example, the result of `parse` query is consumed
     * (stolen) by the driver. Therefore, it is not possible to access it in the
     * callbacks following `after_parsing`.
     */

    /* NOTE: What happen inside the driver?
     * In short, it is wrapper around `rustc_interface`. It creates the appropriate
     * configuration for it and then calls compilation functions — mostly on the
     * `queries` object — and provides feedback (using callbacks) at some phases.
     *
     * At the first step it gives us the configuration it has created
     * (based on args) for the interface (`config` callback).
     *
     * Next, parsing happens and `after_parsing` gets called. We can access the AST
     * using the `parse` query.
     * Next, it expands macros and `after_expansion` gets called.
     * Next, it performs some analysis (not sure what at the moment) and
     * `after_analysis` gets called.
     *
     * After this point, the driver starts to perform the actual compilation.
     * It is done through `ongoing_codegen` and `linker` in the final steps.
     *
     * It looks like `override_queries` is only called in `create_global_ctxt` inside
     * `rustc_interface`, which is called for creating `global_ctxt` that is done
     * after the parsing and before the expansion, or as the documentation says
     * "just after we have populated the list of queries".
     */

    /* NOTE: What is the relation between provider functions?
     * It is not tracked completely at the moment. However, the following call trace
     * is identified in the last investigation:
     * optimized_mir
     * -> mir_drops_elaborated_and_const_checked
     * -> mir_promoted
     * -> mir_const (not just for constants)
     * -> mir_built (the actual translation to MIR)
     *
     * Note that most of these functions are query thus memoized, which means that their result
     * will be stored and later calls will give the same instance. Also, it means
     * you have only one chance to perform any manipulation.
     */

    thread_local! {
         /*
          * NOTE: Both `override_queries` and `optimized_mir` are function pointers, which means
          * that they cannot be closures with captured variables. Therefore, we use these statically
          * allocated cells to store the original functions.
          * As there will be a single transformation pass in the project and the original pointer
          * is the same anyway, this should not cause any problems.
          */
         #[allow(clippy::type_complexity)]
         static ORIGINAL_OVERRIDE: Cell<
             Option<fn(&Session, &mut rustc_middle::util::Providers)>,
         > = Cell::new(None);
         static ORIGINAL_OPTIMIZED_MIR: Cell<
             for<'tcx> fn(TyCtxt<'tcx>, LocalDefId) -> &'tcx mir::Body<'tcx>
         > = Cell::new(|_, _| unreachable!());
         static ORIGINAL_EXTERN_OPTIMIZED_MIR: Cell<
             for<'tcx> fn(TyCtxt<'tcx>, DefId) -> &'tcx mir::Body<'tcx>
         > = Cell::new(|_, _| unreachable!());
         static ORIGINAL_MIR_SHIMS: Cell<
             for<'tcx> fn(TyCtxt<'tcx>, mir_ty::InstanceKind<'tcx>) -> mir::Body<'tcx>
         > = Cell::new(|_, _| unreachable!());
         static ORIGINAL_SHOULD_CODEGEN: Cell<
             for<'tcx> fn(TyCtxtAt<'tcx>, mir_ty::Instance<'tcx>) -> bool
         > = Cell::new(|_, _| unreachable!());
         static ORIGINAL_COLLECT_PARTITION: Cell<
             for<'tcx> fn(TyCtxt<'tcx>, ()) -> MonoItemPartitions<'tcx>
         > = Cell::new(|_, _| unreachable!());
         static ORIGINAL_REGISTERED_TOOLS: Cell<
            for <'tcx> fn(TyCtxt<'tcx>, ()) -> mir_ty::RegisteredTools
         > = Cell::new(|_,_| unreachable!());
    }

    struct PassHolder<T: CompilationPass + ?Sized>(Arc<Mutex<T>>);

    impl<T: CompilationPass> PassHolder<T> {
        fn new(pass: T) -> Self {
            Self(Arc::new(Mutex::new(pass)))
        }

        fn into_pass(self) -> T {
            Arc::into_inner(self.0).unwrap().into_inner().unwrap()
        }
    }

    impl<T: CompilationPass + ?Sized> PassHolder<T> {
        pub fn acquire(&self) -> std::sync::MutexGuard<'_, T> {
            self.0
                .lock()
                .expect("Reentrance and concurrency are not expected in this design.")
        }
    }

    impl<T: CompilationPass + ?Sized> Clone for PassHolder<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    pub(crate) struct CompilationPassAdapter<T: CompilationPass + Send + ?Sized> {
        pass: PassHolder<T>,
        overrides: OverrideFlags,
        leaf_config: Option<LeafCompilerConfig>,
        config_callbacks:
            Vec<Box<dyn FnOnce(&mut interface::Config, &mut LeafCompilerConfig) + Send>>,
    }

    macro_rules! stop_if_stop {
        ($result:expr) => {
            if $result == Compilation::Stop {
                return Compilation::Stop;
            }
        };
    }

    impl<T: CompilationPass + Send + Sync + ?Sized + 'static> driver::Callbacks
        for CompilationPassAdapter<T>
    {
        fn config(&mut self, config: &mut interface::Config) {
            if let Some(leaf_config) = self.leaf_config.as_ref() {
                if leaf_config.codegen_all_mir {
                    assert!(self.overrides.contains(OverrideFlags::SHOULD_CODEGEN));
                    global::FORCE_CODEGEN_FOR_ALL.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }

            *global::OVERRIDE_FLAGS.lock().unwrap() = self.overrides;
            log_debug!("Overriding queries with flags: {:b}", self.overrides);

            ORIGINAL_OVERRIDE.set(config.override_queries.take());
            config.override_queries = Some(
                move |session, providers: &mut rustc_middle::util::Providers| {
                    if let Some(existing_override) = ORIGINAL_OVERRIDE.get() {
                        existing_override(session, providers);
                    }

                    let overrides = global::OVERRIDE_FLAGS.lock().unwrap();

                    if overrides.contains(OverrideFlags::OPTIMIZED_MIR) {
                        ORIGINAL_OPTIMIZED_MIR.set(providers.optimized_mir);
                        providers.optimized_mir = Self::optimized_mir;
                    }
                    if overrides.contains(OverrideFlags::EXTERN_OPTIMIZED_MIR) {
                        ORIGINAL_EXTERN_OPTIMIZED_MIR.set(providers.extern_queries.optimized_mir);
                        providers.extern_queries.optimized_mir = Self::extern_optimized_mir;
                    }
                    if overrides.contains(OverrideFlags::MIR_SHIMS) {
                        ORIGINAL_MIR_SHIMS.set(providers.mir_shims);
                        providers.mir_shims = Self::mir_shims;
                    }
                    if overrides.contains(OverrideFlags::SHOULD_CODEGEN) {
                        /* Currently we only do forcing code generation in this override,
                         * Thus, overriding it is equivalent to forcing it.
                         * If it was overridden for other purposes, we should check inside the
                         * override. */
                        if global::FORCE_CODEGEN_FOR_ALL.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            ORIGINAL_SHOULD_CODEGEN.set(providers.hooks.should_codegen_locally);
                            providers.hooks.should_codegen_locally = |tcx, instance| {
                                let def_id = instance.def.def_id();

                                if tcx.is_foreign_item(def_id) {
                                    // Foreign items are always linked against, there's no way of instantiating them.
                                    return false;
                                }

                                // Force everything to be codegened.
                                true
                            };
                        }
                    }

                    if overrides.contains(OverrideFlags::COLLECT_PARTITION) {
                        ORIGINAL_COLLECT_PARTITION.set(providers.collect_and_partition_mono_items);
                        providers.collect_and_partition_mono_items =
                            Self::collect_and_partition_mono_items;
                    }

                    {
                        ORIGINAL_REGISTERED_TOOLS.set(providers.registered_tools);
                        providers.registered_tools = Self::registered_tools;
                    }
                },
            );

            if self.overrides.contains(OverrideFlags::MAKE_CODEGEN_BACKEND) {
                config.make_codegen_backend = Some(codegen::get_backend_maker(self.pass.clone()));
            }

            global::clear_ctxt_id_and_storage();

            for callback in self.config_callbacks.drain(..) {
                callback(
                    config,
                    &mut self.leaf_config.as_mut().expect("Leaf config is not set"),
                );
            }
        }

        fn after_crate_root_parsing<'tcx>(
            &mut self,
            compiler: &interface::Compiler,
            krate: &mut ast::Crate,
        ) -> Compilation {
            let mut pass = self.pass.acquire();
            stop_if_stop!(pass.visit_ast_before(krate, &mut global::get_storage()));
            pass.transform_ast(&compiler.sess, krate, &mut global::get_storage());
            stop_if_stop!(pass.visit_ast_after(krate, &mut global::get_storage()));

            Compilation::Continue
        }

        fn after_expansion<'tcx>(
            &mut self,
            _compiler: &interface::Compiler,
            _tcx: TyCtxt<'tcx>,
        ) -> Compilation {
            Compilation::Continue
        }

        fn after_analysis<'tcx>(
            &mut self,
            _compiler: &interface::Compiler,
            tcx: TyCtxt<'tcx>,
        ) -> Compilation {
            global::set_ctxt_id(tcx);
            let mut pass = self.pass.acquire();
            pass.visit_tcx_after_analysis(tcx, &mut global::get_storage())
        }
    }

    impl<T: CompilationPass + Send + ?Sized> CompilationPassAdapter<T> {
        fn optimized_mir(tcx: TyCtxt, id: LocalDefId) -> &mir::Body {
            // NOTE: It is possible that this function is called before the callbacks.
            global::set_ctxt_id(tcx);

            /* NOTE: Currently, it seems that there is no way to deallocate
             * something from arena. So, we have to clone the body. */
            let mut body = ORIGINAL_OPTIMIZED_MIR.get()(tcx, id).clone();
            let mut storage = global::get_storage();
            /* NOTE: Ideally this should not happen.
             * However, it is observed that in some cases (presumably because of inlining),
             * there are existing calls to our instrumentation functions, and they are
             * treated as regular blocks.
             * Clearing existing instrumentation (and redoing them) is not supposed to change
             * the behavior, at least as long as the control flow is changed by the instrumentation. */
            instr::clear_existing_instrumentation(tcx, &mut body, &mut storage);
            T::visit_mir_body_before(tcx, &body, &mut storage);
            T::transform_mir_body(tcx, &mut body, &mut storage);
            T::visit_mir_body_after(tcx, &body, &mut storage);
            tcx.arena.alloc(body)
        }

        fn extern_optimized_mir(tcx: TyCtxt, id: DefId) -> &mir::Body {
            // NOTE: It is possible that this function is called before the callbacks.
            global::set_ctxt_id(tcx);

            /* NOTE: Currently, it seems that there is no way to deallocate
             * something from arena. So, we have to clone the body. */
            let mut body = ORIGINAL_EXTERN_OPTIMIZED_MIR.get()(tcx, id).clone();
            let mut storage = global::get_storage();
            instr::clear_existing_instrumentation(tcx, &mut body, &mut storage);
            T::visit_mir_body_before(tcx, &body, &mut storage);
            T::transform_mir_body(tcx, &mut body, &mut storage);
            T::visit_mir_body_after(tcx, &body, &mut storage);
            tcx.arena.alloc(body)
        }

        fn mir_shims<'tcx>(
            tcx: TyCtxt<'tcx>,
            instance: mir_ty::InstanceKind<'tcx>,
        ) -> mir::Body<'tcx> {
            // NOTE: It is possible that this function is called before the callbacks.
            global::set_ctxt_id(tcx);

            /* NOTE: Currently, it seems that there is no way to deallocate
             * something from arena. So, we have to clone the body. */
            let mut body = ORIGINAL_MIR_SHIMS.get()(tcx, instance);
            let mut storage = global::get_storage();
            T::visit_mir_body_before(tcx, &body, &mut storage);
            T::transform_mir_body(tcx, &mut body, &mut storage);
            T::visit_mir_body_after(tcx, &body, &mut storage);
            body
        }

        fn collect_and_partition_mono_items(tcx: TyCtxt, _: ()) -> MonoItemPartitions {
            fn clone_cgu<'tcx>(cgu: &CodegenUnit<'tcx>) -> CodegenUnit<'tcx> {
                let mut new_cgu = CodegenUnit::new(cgu.name().clone());
                new_cgu.items_mut().extend(cgu.items().clone());
                new_cgu.compute_size_estimate();
                if cgu.is_primary() {
                    new_cgu.make_primary();
                }
                if cgu.is_code_coverage_dead_code_cgu() {
                    new_cgu.make_code_coverage_dead_code_cgu();
                }
                new_cgu
            }

            global::set_ctxt_id(tcx);
            let MonoItemPartitions {
                codegen_units,
                all_mono_items,
            } = ORIGINAL_COLLECT_PARTITION.get()(tcx, ());
            let mut units = codegen_units.iter().map(clone_cgu).collect::<Vec<_>>();
            T::visit_codegen_units(tcx, &mut units, &mut global::get_storage());
            MonoItemPartitions {
                codegen_units: tcx.arena.alloc_from_iter(units),
                all_mono_items,
            }
        }

        fn registered_tools(tcx: TyCtxt, _: ()) -> mir_ty::RegisteredTools {
            // FIXME: Make it configurable
            const TOOL_NAME: &str = crate::constants::TOOL_LEAF;

            let mut tools = ORIGINAL_REGISTERED_TOOLS.get()(tcx, ()).clone();

            // #![register_tool(leaf_attr)]
            tools.insert(rustc_span::Ident {
                name: Symbol::intern(TOOL_NAME),
                span: rustc_span::DUMMY_SP,
            });

            tools
        }
    }

    impl<T: CompilationPass + Send> CompilationPassAdapter<T> {
        pub(crate) fn new(pass: T, overrides: OverrideFlags) -> Self {
            Self {
                pass: PassHolder::new(pass),
                overrides,
                leaf_config: None,
                config_callbacks: Vec::new(),
            }
        }

        pub(crate) fn into_pass(self) -> T {
            self.pass.into_pass()
        }
    }

    impl<T: CompilationPass + Send + ?Sized> CallbacksExt for CompilationPassAdapter<T>
    where
        Self: driver::Callbacks,
    {
        fn set_leaf_config(&mut self, config: LeafCompilerConfig) {
            self.leaf_config = Some(config);
        }

        fn add_config_callback(
            &mut self,
            callback: Box<
                dyn FnOnce(&mut interface::Config, &mut LeafCompilerConfig) + Send + 'static,
            >,
        ) {
            self.config_callbacks.push(callback);
        }
    }

    impl<A, B> CompilationPass for Chain<A, B>
    where
        A: CompilationPass,
        B: CompilationPass,
    {
        fn override_flags() -> OverrideFlags {
            A::override_flags() | B::override_flags()
        }

        fn visit_ast_before(
            &mut self,
            krate: &ast::Crate,
            storage: &mut dyn Storage,
        ) -> Compilation {
            stop_if_stop!(self.first.visit_ast_before(krate, storage));
            stop_if_stop!(self.second.visit_ast_before(krate, storage));

            Compilation::Continue
        }

        fn visit_ast_after(
            &mut self,
            krate: &ast::Crate,
            storage: &mut dyn Storage,
        ) -> Compilation {
            // NOTE: We perform in the telescope order.
            stop_if_stop!(self.second.visit_ast_after(krate, storage));
            stop_if_stop!(self.first.visit_ast_after(krate, storage));

            Compilation::Continue
        }

        fn visit_tcx_after_analysis(
            &mut self,
            tcx: mir_ty::TyCtxt,
            storage: &mut dyn Storage,
        ) -> Compilation {
            stop_if_stop!(self.first.visit_tcx_after_analysis(tcx, storage));
            stop_if_stop!(self.second.visit_tcx_after_analysis(tcx, storage));

            Compilation::Continue
        }

        fn visit_tcx_at_codegen_before(&mut self, tcx: mir_ty::TyCtxt, storage: &mut dyn Storage) {
            self.first.visit_tcx_at_codegen_before(tcx, storage);
            self.second.visit_tcx_at_codegen_before(tcx, storage);
        }

        fn visit_tcx_at_codegen_after(&mut self, tcx: mir_ty::TyCtxt, storage: &mut dyn Storage) {
            self.first.visit_tcx_at_codegen_after(tcx, storage);
            self.second.visit_tcx_at_codegen_after(tcx, storage);
        }

        fn visit_mir_body_before<'tcx>(
            tcx: mir_ty::TyCtxt<'tcx>,
            body: &mir::Body<'tcx>,
            storage: &mut dyn Storage,
        ) {
            A::visit_mir_body_before(tcx, body, storage);
            B::visit_mir_body_before(tcx, body, storage);
        }

        fn visit_mir_body_after<'tcx>(
            tcx: mir_ty::TyCtxt<'tcx>,
            body: &mir::Body<'tcx>,
            storage: &mut dyn Storage,
        ) {
            A::visit_mir_body_after(tcx, body, storage);
            B::visit_mir_body_after(tcx, body, storage);
        }

        fn transform_ast(
            &mut self,
            session: &Session,
            krate: &mut ast::Crate,
            storage: &mut dyn Storage,
        ) {
            self.first.transform_ast(session, krate, storage);
            self.second.transform_ast(session, krate, storage);
        }

        fn transform_mir_body<'tcx>(
            tcx: mir_ty::TyCtxt<'tcx>,
            body: &mut mir::Body<'tcx>,
            storage: &mut dyn Storage,
        ) {
            A::transform_mir_body(tcx, body, storage);
            B::transform_mir_body(tcx, body, storage);
        }

        fn visit_codegen_units<'tcx>(
            tcx: mir_ty::TyCtxt<'tcx>,
            units: &mut [CodegenUnit<'tcx>],
            storage: &mut dyn Storage,
        ) {
            A::visit_codegen_units(tcx, units, storage);
            B::visit_codegen_units(tcx, units, storage);
        }
    }

    pub mod codegen {
        /* NOTE: Currently we do not do any modification during code generation.
         * This wrapper is only supposed to provide callbacks
         */
        use super::*;

        use rustc_codegen_ssa::{CodegenResults, traits::CodegenBackend};
        use rustc_data_structures::fx::FxIndexMap;
        use rustc_metadata::{EncodedMetadata, creader::MetadataLoaderDyn};
        use rustc_middle::util::Providers;
        use rustc_query_system::dep_graph::{WorkProduct, WorkProductId};
        use rustc_session::{
            Session,
            config::{self, OutputFilenames, PrintRequest},
        };
        use rustc_span::Symbol;

        use delegate::delegate;

        pub(super) struct CodegenBackendWrapper<T: CompilationPass + ?Sized> {
            backend: Box<dyn CodegenBackend>,
            pass: PassHolder<T>,
        }

        impl<T: CompilationPass + ?Sized> CodegenBackend for CodegenBackendWrapper<T> {
            delegate! {
                to self.backend {
                    fn locale_resource(&self) -> &'static str;

                    fn init(&self, sess: &Session);
                    fn print(&self, req: &PrintRequest, out: &mut String, sess: &Session);
                    fn target_features_cfg(&self, sess: &Session, allow_unstable: bool) -> Vec<Symbol>;
                    fn print_passes(&self);
                    fn print_version(&self);

                    fn metadata_loader(&self) -> Box<MetadataLoaderDyn>;

                    fn provide(&self, providers: &mut Providers);

                    fn join_codegen(
                        &self,
                        ongoing_codegen: Box<dyn Any>,
                        sess: &Session,
                        outputs: &OutputFilenames,
                    ) -> (CodegenResults, FxIndexMap<WorkProductId, WorkProduct>);

                    fn link(
                        &self,
                        sess: &Session,
                        codegen_results: CodegenResults,
                        outputs: &OutputFilenames,
                    );

                    fn supports_parallel(&self) -> bool;
                }
            }

            fn codegen_crate<'tcx>(
                &self,
                tcx: TyCtxt<'tcx>,
                metadata: EncodedMetadata,
                need_metadata_module: bool,
            ) -> Box<dyn Any> {
                let result = {
                    let mut pass = self.pass.acquire();
                    pass.visit_tcx_at_codegen_before(tcx, &mut global::get_storage());
                    self.backend
                        .codegen_crate(tcx, metadata, need_metadata_module)
                };
                {
                    let mut pass = self.pass.acquire();
                    pass.visit_tcx_at_codegen_after(tcx, &mut global::get_storage());
                }
                result
            }
        }

        pub(super) fn get_backend_maker<T: CompilationPass + Send + Sync + ?Sized + 'static>(
            pass: PassHolder<T>,
        ) -> Box<dyn FnOnce(&config::Options) -> Box<dyn CodegenBackend> + Send> {
            Box::new(|opts| {
                // This is the default implementation taken from `interface::run_compiler`.
                let early_dcx = rustc_session::EarlyDiagCtxt::new(opts.error_format);
                let sysroot =
                    rustc_session::filesearch::materialize_sysroot(opts.maybe_sysroot.clone());
                let target = config::build_target_config(&early_dcx, &opts.target_triple, &sysroot);
                let backend = rustc_interface::util::get_codegen_backend(
                    &early_dcx,
                    &sysroot,
                    opts.unstable_opts.codegen_backend.as_deref(),
                    &target,
                );
                Box::new(CodegenBackendWrapper { backend, pass })
            })
        }
    }

    mod storage {
        use std::ops::{Deref, DerefMut};

        use super::*;

        /* NOTE: How is the global storage implemented?
         * The global Storage has the following properties:
         * - It is a thread-local storage.
         * - May be referenced mutably multiple times in a single trace.
         *   e.g. codegen -> optimized_mir
         *   Therefore, we use interior mutability and the storage is borrowed for short time slots.
         * - A key is not expected to be accessed multiple times in a single trace.
         *   Also, in the current code, each key is local to a single module.
         *   Therefore, on each access it temporarily removes the value from the storage.
         *   An alternative approach would be using `Rc` and `RefCell` to access
         *   items without removing them from the storage. However, unnecessary
         *   complexity is avoided for now. Particularly, the mapping to the
         *   downcast type is not easy to implement with `RefCell`.
         */

        pub(super) type StorageValueImpl = Box<dyn Any>;
        pub(super) type StorageImpl = HashMap<String, StorageValueImpl>;

        pub(super) struct GlobalStorage(pub RRef<StorageImpl>);

        impl Storage for GlobalStorage {
            fn get_raw_or_insert_with<'a>(
                &'a mut self,
                key: String,
                default: Box<dyn FnOnce(&mut dyn Storage) -> Box<dyn Any> + '_>,
            ) -> ValueBorrow<'a> {
                /* Although the signature requires mutably borrowing the storage,
                 * for the global storage, we use interior mutability. */
                let value = self.0.borrow_mut().remove(&key);
                let value = value.unwrap_or_else(|| default(self));

                ValueBorrow::new(self, key, value)
            }

            fn get_raw_mut<'a>(&'a mut self, key: &String) -> Option<ValueBorrow<'a>> {
                log_debug!("Getting mutable reference with key: {}", key);
                let value = self.0.borrow_mut().remove(key)?;
                Some(ValueBorrow::new(self, key.clone(), value))
            }
        }

        impl core::fmt::Debug for GlobalStorage {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.borrow().fmt(f)
            }
        }

        pub(crate) struct ValueBorrow<'a> {
            storage: &'a GlobalStorage,
            key: Option<String>,
            value: Option<StorageValueImpl>,
            leaked: bool,
        }

        impl<'a> ValueBorrow<'a> {
            fn new(storage: &'a GlobalStorage, key: String, value: StorageValueImpl) -> Self {
                ValueBorrow {
                    storage,
                    key: Some(key),
                    value: Some(value),
                    leaked: false,
                }
            }

            pub(crate) fn leak(mut self) -> StorageValueImpl {
                self.leaked = true;
                self.value.take().unwrap()
            }
        }

        impl Drop for ValueBorrow<'_> {
            fn drop(&mut self) {
                if self.leaked {
                    return;
                }
                self.storage
                    .0
                    .borrow_mut()
                    .insert(self.key.take().unwrap(), self.value.take().unwrap());
            }
        }

        impl Deref for ValueBorrow<'_> {
            type Target = StorageValueImpl;

            fn deref(&self) -> &Self::Target {
                self.value.as_ref().unwrap()
            }
        }

        impl DerefMut for ValueBorrow<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.value.as_mut().unwrap()
            }
        }

        impl<S: Storage + ?Sized> StorageExt for S {
            type MutAccessor<'a, T>
                = DowncastValueBorrow<'a, T>
            where
                Self: 'a,
                T: 'a;
            type ManualBorrow<T> = ManualBorrow<T>;

            fn get_or_insert_with_acc<'a, V: Any>(
                &'a mut self,
                key: String,
                default: impl FnOnce(&mut dyn Storage) -> V + 'a,
            ) -> Self::MutAccessor<'a, V> {
                self.get_raw_or_insert_with(key.clone(), Box::new(|s| Box::new(default(s))));
                self.get_mut(&key).unwrap()
            }

            fn get_mut<'a, V: Any>(&'a mut self, key: &String) -> Option<Self::MutAccessor<'a, V>> {
                self.get_raw_mut(key)
                    .map(|v| DowncastValueBorrow::<'a, V>(v, Default::default()))
            }
        }

        pub(crate) struct DowncastValueBorrow<'a, V: 'a>(
            ValueBorrow<'a>,
            std::marker::PhantomData<&'a mut V>,
        );

        impl<V: 'static> DowncastValueBorrow<'_, V> {
            pub(crate) fn leak(mut self) -> ManualBorrow<V> {
                ManualBorrow {
                    key: self.0.key.take().unwrap(),
                    value: self.0.leak().downcast::<V>().unwrap(),
                }
            }
        }

        impl<V: 'static> Deref for DowncastValueBorrow<'_, V> {
            type Target = V;

            fn deref(&self) -> &Self::Target {
                self.0.downcast_ref::<V>().unwrap()
            }
        }

        impl<V: 'static> DerefMut for DowncastValueBorrow<'_, V> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.0.downcast_mut::<V>().unwrap()
            }
        }

        impl<V: 'static> AsMut<V> for DowncastValueBorrow<'_, V> {
            fn as_mut(&mut self) -> &mut V {
                self.0.downcast_mut::<V>().unwrap()
            }
        }

        pub(crate) struct ManualBorrow<V> {
            key: String,
            value: Box<V>,
        }

        impl<V> Deref for ManualBorrow<V> {
            type Target = Box<V>;

            fn deref(&self) -> &Self::Target {
                &self.value
            }
        }

        impl<V> AsRef<V> for ManualBorrow<V> {
            fn as_ref(&self) -> &V {
                &self.value
            }
        }

        impl<V: 'static> ManualBorrow<V> {
            pub(crate) fn return_to(self, storage: &mut dyn Storage) {
                storage.get_raw_or_insert_with(self.key, Box::new(|_| self.value));
            }
        }
    }

    mod global {
        use super::{
            storage::{GlobalStorage, StorageImpl},
            *,
        };

        use std::{cell::Cell, sync::atomic::AtomicBool};

        use rustc_middle::ty::TyCtxt;

        pub(super) static OVERRIDE_FLAGS: Mutex<OverrideFlags> = Mutex::new(OverrideFlags::empty());
        pub(super) static FORCE_CODEGEN_FOR_ALL: AtomicBool = AtomicBool::new(false);

        thread_local! {
            /*
             * NOTE: We need to provide a global storage for our transformation passes.
             * In the compiler itself, all the shared information is stored in the context.
             * Unless we extend the compiler codes, we cannot put our data there.
             * Thus, we rely on global memory, which is fine for the current state and
             * how the compiler is used. We also have some guards to prevent invalid
             * states that may happen by parallel compilations.
             */
            static CTXT_ID : Cell<Option<usize>> = Cell::new(None);
            static STORAGE: RRef<StorageImpl> = Default::default();
        }

        pub(crate) fn clear_ctxt_id_and_storage() {
            CTXT_ID.replace(None);
            STORAGE.with(|s| s.borrow_mut().clear());
        }

        pub(crate) fn set_ctxt_id(tcx: TyCtxt) {
            let id = get_ctxt_id(tcx);
            let old_id = CTXT_ID.replace(Some(id));
            assert_eq!(
                old_id.unwrap_or(id),
                id,
                "The id has been set before and not cleared. Possibly the thread is being used by other passes."
            );
        }

        #[inline]
        pub(crate) fn get_storage() -> GlobalStorage {
            GlobalStorage(STORAGE.with(RRef::clone))
        }

        #[inline]
        fn get_ctxt_id(tcx: TyCtxt) -> usize {
            (*std::ops::Deref::deref(&tcx)) as *const _ as usize
        }
    }
}
