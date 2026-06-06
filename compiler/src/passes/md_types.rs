use itertools::Itertools;
use rustc_middle::mir::{self, visit::Visitor};
use rustc_middle::ty::{
    EarlyBinder, GenericArgsRef, Instance, Ty, TyCtxt, TyKind, TypeSuperVisitable, TypeVisitable,
    TypeVisitableExt, TypeVisitor, TypingEnv,
};
use rustc_type_ir::inherent::AdtDef;

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use common::{
    log_debug, log_info,
    type_info::{MetadataValue, TypeId},
};

use super::{CompilationPass, Storage};

const TAG: &str = "md_types";

#[derive(Default)]
pub(crate) struct MdInfoExporter;

impl CompilationPass for MdInfoExporter {
    fn override_flags() -> super::OverrideFlags {
        super::OverrideFlags::MAKE_CODEGEN_BACKEND
    }

    fn visit_tcx_at_codegen_after(
        &mut self,
        tcx: rustc_middle::ty::TyCtxt,
        storage: &mut dyn Storage,
    ) {
        log_info!("Exporting ManuallyDrop info");

        let (md_touching_instances, md_containers, found_mds) = scan_all_bodies(tcx);

        log_info!(
            "MIR Bodies That Touch ManuallyDrop: {}",
            serde_json::to_string(&list_bodies_to_include(tcx, &md_touching_instances)).unwrap()
        );

        let set_to_array = |s: &HashSet<TypeId>| {
            MetadataValue::Array(
                s.iter()
                    .cloned()
                    .map(|id| MetadataValue::Number(id.get()))
                    .collect(),
            )
        };
        const KEY_MD_TYPES: &str = "md_types";
        const KEY_MD_CONTAINER_TYPES: &str = "md_container_types";
        super::type_info::add_metadata_to_types_db(
            storage,
            KEY_MD_TYPES.to_owned(),
            set_to_array(&found_mds),
        );
        super::type_info::add_metadata_to_types_db(
            storage,
            KEY_MD_CONTAINER_TYPES.into(),
            set_to_array(&md_containers),
        );
    }
}

fn scan_all_bodies<'s>(tcx: TyCtxt) -> (HashSet<Instance>, HashSet<TypeId>, HashSet<TypeId>) {
    let mut instances = HashMap::new();
    let mut evaluated_types = Default::default();
    let mut fully_visited_types = Default::default();
    let mut found_mds = Default::default();

    tcx.collect_and_partition_mono_items(())
        .codegen_units
        .iter()
        .for_each(|unit| {
            unit.items().iter().for_each(|(item, _)| match item {
                mir::mono::MonoItem::Fn(instance) => {
                    let body = tcx.instance_mir(instance.def);
                    log_debug!(target: TAG, "Scanning types in for ManuallyDrop {:?}", instance);

                    let has_any = MdCollectorVisitor {
                        tcx,
                        args: instance.args,
                        typing_env: TypingEnv::post_analysis(tcx, body.source.def_id()),
                        local_decls: &body.local_decls,
                        evaluated_types: &mut evaluated_types,
                        fully_visited_types: &mut fully_visited_types,
                        mds: &mut found_mds,
                        visited_any: false,
                    }
                    .collect_and_check_if_any(body);
                    instances.insert(instance.clone(), has_any);
                    if instance.def_id().is_local() {
                        log_info!(target: TAG, "Instance {:?} {}", instance, has_any);
                    }
                }
                _ => {}
            })
        });
    let md_touching_instances = instances
        .iter()
        .filter_map(|(instance, touches_md)| {
            if *touches_md {
                Some(instance.clone())
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
    let md_containers = evaluated_types
        .iter()
        .filter_map(|(id, is_md)| if *is_md { Some(*id) } else { None })
        .collect::<HashSet<_>>();

    fn to_type_ids<'tcx>(
        tcx: TyCtxt<'tcx>,
        types: impl IntoIterator<Item = Ty<'tcx>>,
    ) -> HashSet<TypeId> {
        types.into_iter().map(|t| type_id(tcx, t)).collect()
    }
    (
        md_touching_instances,
        to_type_ids(tcx, md_containers),
        to_type_ids(tcx, found_mds),
    )
}

fn type_id<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> TypeId {
    TypeId::new(tcx.type_id_hash(ty).as_u128()).unwrap()
}

fn list_bodies_to_include<'tcx>(
    tcx: TyCtxt<'tcx>,
    md_touching_instances: &HashSet<Instance>,
) -> Vec<common::types::DefId> {
    md_touching_instances
        .iter()
        .map(|i| i.def_id())
        .filter(|def_id| {
            tcx.crate_name(def_id.krate).as_str()
                != *super::instr::pri_utils::sym::RUNTIME_LIB_CRATE
        })
        .map(|def_id| common::types::DefId(def_id.krate.as_u32(), def_id.index.as_u32()))
        .sorted()
        .collect()
}

struct MdCollectorVisitor<'tcx, 's, 'b> {
    tcx: TyCtxt<'tcx>,
    args: GenericArgsRef<'tcx>,
    typing_env: TypingEnv<'tcx>,
    local_decls: &'b mir::LocalDecls<'tcx>,
    evaluated_types: &'s mut HashMap<Ty<'tcx>, bool>,
    fully_visited_types: &'s mut HashSet<Ty<'tcx>>,
    mds: &'s mut HashSet<Ty<'tcx>>,
    visited_any: bool,
}

impl<'tcx, 's, 'b> MdCollectorVisitor<'tcx, 's, 'b> {
    fn collect_and_check_if_any(mut self, body: &mir::Body<'tcx>) -> bool {
        self.visited_any = false;
        self.visit_body(body);
        self.visited_any
    }
}

impl<'tcx, 's, 'b> Visitor<'tcx> for MdCollectorVisitor<'tcx, 's, 'b> {
    fn visit_ty(&mut self, ty: Ty<'tcx>, _context: mir::visit::TyContext) {
        ty.visit_with(self);
    }

    fn visit_place(
        &mut self,
        place: &mir::Place<'tcx>,
        context: mir::visit::PlaceContext,
        location: mir::Location,
    ) {
        self.super_place(place, context, location);
        // Process intermediate types as well
        place.iter_projections().fold(
            mir::PlaceTy::from_ty(self.local_decls[place.local].ty),
            |p_ty, x| {
                let p_ty = p_ty.projection_ty(self.tcx, x.1);
                p_ty.visit_with(self);
                p_ty
            },
        );
    }
}

impl<'tcx, 's, 'b> TypeVisitor<TyCtxt<'tcx>> for MdCollectorVisitor<'tcx, 's, 'b> {
    fn visit_ty(&mut self, ty: Ty<'tcx>) -> Self::Result {
        if ty.has_escaping_bound_vars() {
            return;
        }

        let ty = self.tcx.instantiate_and_normalize_erasing_regions(
            self.args,
            self.typing_env,
            EarlyBinder::bind(ty),
        );

        if !self.fully_visited_types.insert(ty) {
            self.visited_any |= self.evaluated_types[&ty];
            return;
        }

        if (IsMdContainerVisitor {
            tcx: self.tcx,
            args: self.args,
            typing_env: self.typing_env,
            evaluated_types: self.evaluated_types,
            mds: self.mds,
        })
        .is_md_container(ty)
        {
            self.visited_any = true;
        }

        // Go through everything
        ty.super_visit_with(self);
        // Additional recursions
        match ty.kind() {
            TyKind::Adt(adt, args) => adt
                .all_field_tys(self.tcx)
                .iter_instantiated(self.tcx, args)
                .for_each(|t| t.visit_with(self)),
            _ => {}
        }
    }
}

struct IsMdContainerVisitor<'tcx, 's> {
    tcx: TyCtxt<'tcx>,
    args: GenericArgsRef<'tcx>,
    typing_env: TypingEnv<'tcx>,
    evaluated_types: &'s mut HashMap<Ty<'tcx>, bool>,
    mds: &'s mut HashSet<Ty<'tcx>>,
}

impl<'tcx, 's> TypeVisitor<TyCtxt<'tcx>> for IsMdContainerVisitor<'tcx, 's> {
    type Result = ControlFlow<()>;

    fn visit_ty(&mut self, ty: Ty<'tcx>) -> Self::Result {
        if ty.has_escaping_bound_vars() {
            return ControlFlow::Continue(());
        }

        let ty = self.tcx.instantiate_and_normalize_erasing_regions(
            self.args,
            self.typing_env,
            EarlyBinder::bind(ty),
        );

        if let Some(&is_container) = self.evaluated_types.get(&ty) {
            return if is_container {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            };
        }

        if ty.ty_adt_def().is_some_and(|adt| adt.is_manually_drop()) {
            self.mds.insert(ty);
            self.evaluated_types.insert(ty, true);
            return ControlFlow::Break(());
        }

        match self.ty_super_visit(ty) {
            ControlFlow::Break(()) => {
                self.evaluated_types.insert(ty, true);
                ControlFlow::Break(())
            }
            ControlFlow::Continue(()) => {
                self.evaluated_types.insert(ty, false);
                ControlFlow::Continue(())
            }
        }
    }
}

impl<'tcx, 's> IsMdContainerVisitor<'tcx, 's> {
    // A customization of the original `Ty::super_visit_with` for recursing over contained types.
    fn ty_super_visit(&mut self, ty: Ty<'tcx>) -> <Self as TypeVisitor<TyCtxt<'tcx>>>::Result {
        use rustc_type_ir::TyKind::*;
        match ty.kind() {
            RawPtr(_ty, _mutbl) => (), // Not contained
            Array(typ, _sz) => {
                typ.visit_with(self)?;
                () // The size is not a contained type
            }
            Slice(typ) => typ.visit_with(self)?,
            Adt(adt, args) => adt
                .all_field_tys(self.tcx)
                .iter_instantiated(self.tcx, args)
                .try_for_each(|t| t.visit_with(self))?, // Recurse into fields of ADTs
            Dynamic(_trait_ty, _reg) => (), // Not contained
            Tuple(ts) => ts.visit_with(self)?,
            FnDef(..) => (), // Not contained
            FnPtr(..) => (), // Not contained
            UnsafeBinder(f) => f.visit_with(self)?,
            Ref(..) => (), // Not contained
            Coroutine(_did, args) => args.visit_with(self)?,
            CoroutineWitness(_did, args) => args.visit_with(self)?,
            Closure(_did, args) => args.visit_with(self)?,
            CoroutineClosure(_did, args) => args.visit_with(self)?,
            Alias(_, data) => data.visit_with(self)?,

            Pat(ty, pat) => {
                // Unlikely to need to, but not sure if we should skip visiting this
                ty.visit_with(self)?;
                pat.visit_with(self)?;
            }
            Error(..) => (),
            Bool | Char | Str | Int(_) | Uint(_) | Float(_) | Infer(_) | Bound(..)
            | Placeholder(..) | Param(..) | Never | Foreign(..) => (),
        }

        ControlFlow::Continue(())
    }

    fn is_md_container(&mut self, ty: Ty<'tcx>) -> bool {
        matches!(ty.visit_with(self), ControlFlow::Break(()))
    }
}
