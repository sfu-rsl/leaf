use rustc_abi::{FieldsShape, LayoutData, Scalar, TagEncoding, Variants};
use rustc_middle::mir::{self, visit::Visitor};
use rustc_middle::ty::{
    EarlyBinder, GenericArgsRef, Ty, TyCtxt, TyKind, TypeSuperVisitable, TypeVisitable,
    TypeVisitableExt, TypeVisitor, TypingEnv,
    layout::{HasTyCtxt, HasTypingEnv, LayoutCx, TyAndLayout},
};
use rustc_target::abi::{FieldIdx, Layout, VariantIdx};
use rustc_type_ir::inherent::AdtDef;

use std::collections::HashMap;
use std::env::{self};

use common::{
    log_debug, log_info, log_warn,
    type_info::{self, *},
};

use crate::utils::file::TyCtxtFileExt;

use super::{CompilationPass, Storage};

const TAG_TYPE_EXPORT: &str = "type_export";

#[derive(Default)]
pub(crate) struct TypeInfoExporter;

impl CompilationPass for TypeInfoExporter {
    fn override_flags() -> super::OverrideFlags {
        super::OverrideFlags::MAKE_CODEGEN_BACKEND
    }

    fn visit_tcx_at_codegen_after(
        &mut self,
        tcx: rustc_middle::ty::TyCtxt,
        _storage: &mut dyn Storage,
    ) {
        log_info!("Exporting type info");

        let type_map = capture_all_types(tcx);

        let out_dir = tcx.output_dir();
        let is_single_file_program =
            out_dir.as_os_str().is_empty() || !rustc_session::utils::was_invoked_from_cargo();
        let out_dirs = if is_single_file_program {
            vec![out_dir.as_path()]
        } else if env::var("CARGO_PRIMARY_PACKAGE").is_ok() {
            /* For compiling a single file program, the final type export file is placed in the same directory as the program file.
             * For compiling a project, the final type export file is also placed in the output directory (i.e. "./target/debug/") */
            vec![out_dir.as_path(), out_dir.parent().unwrap()]
        } else {
            // Types for dependencies should not be required.
            vec![]
        };

        let write = move || -> Result<(), Box<dyn core::error::Error>> {
            let mut out_dirs = out_dirs.into_iter();
            if let Some(out_dir) = out_dirs.next() {
                let path = type_info::rw::write_types_db_in(
                    type_map.values(),
                    get_core_types(tcx).map(|t| type_id(tcx, t)),
                    &out_dir,
                )?;
                for out_dir in out_dirs {
                    std::fs::copy(&path, out_dir.join(path.file_name().unwrap()))
                        .map_err(|e| Box::new(e))?;
                }
            } else {
                log_debug!("Type info export is skipped")
            }
            Ok(())
        };
        write().expect("Failed to write type info");
    }
}

fn capture_all_types<'s>(tcx: TyCtxt) -> HashMap<TypeId, TypeInfo> {
    let mut type_map = Default::default();

    tcx.collect_and_partition_mono_items(())
        .codegen_units
        .iter()
        .for_each(|unit| {
            unit.items().iter().for_each(|(item, _)| match item {
                mir::mono::MonoItem::Fn(instance) => {
                    let body = tcx.instance_mir(instance.def);
                    log_debug!(target: TAG_TYPE_EXPORT, "Exporting types in {:?}", instance);
                    let mut place_visitor = TyVisitor {
                        tcx,
                        type_map: &mut type_map,
                        args: instance.args,
                        typing_env: TypingEnv::post_analysis(tcx, body.source.def_id()),
                        local_decls: &body.local_decls,
                    };
                    place_visitor.visit_body(body);
                }
                _ => {}
            })
        });

    for ty in CoreTypes::from(get_core_types(tcx)).as_ref() {
        add_type_information_to_map(&mut type_map, tcx, *ty, TypingEnv::fully_monomorphized());
    }

    type_map
}

fn get_core_types<'tcx>(tcx: TyCtxt<'tcx>) -> CoreTypes<Ty<'tcx>> {
    NamedCoreTypes {
        bool: tcx.types.bool,
        char: tcx.types.char,
        i8: tcx.types.i8,
        i16: tcx.types.i16,
        i32: tcx.types.i32,
        i64: tcx.types.i64,
        i128: tcx.types.i128,
        isize: tcx.types.isize,
        u8: tcx.types.u8,
        u16: tcx.types.u16,
        u32: tcx.types.u32,
        u64: tcx.types.u64,
        u128: tcx.types.u128,
        usize: tcx.types.usize,
        f16: tcx.types.f16,
        f32: tcx.types.f32,
        f64: tcx.types.f64,
        f128: tcx.types.f128,
        raw_addr: Ty::new_imm_ptr(tcx, tcx.types.unit),
        raw_mut_addr: Ty::new_mut_ptr(tcx, tcx.types.unit),
    }
    .into()
}

fn add_type_information_to_map<'tcx>(
    type_map: &mut HashMap<TypeId, TypeInfo>,
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    typing_env: TypingEnv<'tcx>,
) {
    // The type passed here is instantiated, so this check should be the same as inside the layout_of function.
    match ty.kind() {
        // Although should not really appear at this stage, we have faced errors before.
        TyKind::Bound(..) | TyKind::CoroutineWitness(..) | TyKind::Infer(_) | TyKind::Error(_) => {
            return;
        }
        _ => {}
    }

    let layout = match tcx.layout_of(typing_env.as_query_input(ty)) {
        Ok(TyAndLayout { layout, .. }) => layout,
        Err(err) => {
            log_warn!("Failed to get layout of type {:?}: {:?}", ty, err);
            return;
        }
    };
    log_debug!(target: TAG_TYPE_EXPORT, "Generating type information for {:?}", ty);
    let cx = LayoutCx::new(tcx, typing_env);
    let type_info: TypeInfo = layout.to_runtime(&cx, ty);
    type_map.insert(type_info.id, type_info);
}

fn type_id<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> TypeId {
    TypeId::new(tcx.type_id_hash(ty).as_u128()).unwrap()
}

struct TyVisitor<'tcx, 's, 'b> {
    tcx: TyCtxt<'tcx>,
    type_map: &'s mut HashMap<TypeId, TypeInfo>,
    args: GenericArgsRef<'tcx>,
    typing_env: TypingEnv<'tcx>,
    local_decls: &'b mir::LocalDecls<'tcx>,
}

impl<'tcx, 's, 'b> Visitor<'tcx> for TyVisitor<'tcx, 's, 'b> {
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
            mir::tcx::PlaceTy::from_ty(self.local_decls[place.local].ty),
            |p_ty, x| {
                let p_ty = p_ty.projection_ty(self.tcx, x.1);
                p_ty.visit_with(self);
                p_ty
            },
        );
    }
}

impl<'tcx, 's, 'b> TypeVisitor<TyCtxt<'tcx>> for TyVisitor<'tcx, 's, 'b> {
    fn visit_ty(&mut self, ty: Ty<'tcx>) -> Self::Result {
        if ty.has_escaping_bound_vars() {
            return;
        }

        let normalized_ty = self.tcx.instantiate_and_normalize_erasing_regions(
            self.args,
            self.typing_env,
            EarlyBinder::bind(ty),
        );
        if normalized_ty != ty {
            log_debug!(target: TAG_TYPE_EXPORT, "Normalized ty with param: {} -> {}", ty, normalized_ty);
        }
        let ty = normalized_ty;

        if self
            .type_map
            .contains_key(&type_id(self.tcx, normalized_ty))
        {
            return;
        }

        add_type_information_to_map(self.type_map, self.tcx, normalized_ty, self.typing_env);

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

trait ToRuntimeInfo<'tcx, Cx, T> {
    type Def;

    fn to_runtime(self, cx: &Cx, ty: Self::Def) -> T
    where
        Cx: 'tcx;
}

impl<'tcx, Cx> ToRuntimeInfo<'tcx, Cx, TypeInfo> for Layout<'tcx>
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    type Def = Ty<'tcx>;

    fn to_runtime(self, cx: &Cx, ty: Self::Def) -> TypeInfo
    where
        Cx: 'tcx,
    {
        let tcx = cx.tcx();
        // FIXME: Reconstruction of deconstructed layout.
        /* FIXME: As all conversions now get TyAndLayout as definition,
         * we can move it inside `cx`. */
        let ty_layout = TyAndLayout { ty, layout: self };
        let size = if self.is_sized() {
            self.size().bytes()
        } else {
            TypeInfo::SIZE_UNSIZED
        };

        let (variants, tag) = match &self.variants {
            Variants::Empty => (vec![], None),
            Variants::Single { index, .. } => (
                vec![self.0.to_runtime(cx, ty_layout)],
                if ty.is_enum() {
                    Some(TagInfo::Constant {
                        discr_bit_rep: ty.discriminant_for_variant(tcx, *index).unwrap().val,
                    })
                } else {
                    None
                },
            ),
            Variants::Multiple {
                variants,
                tag,
                tag_encoding,
                tag_field,
            } => (
                variants
                    .iter_enumerated()
                    .map(|(i, v)| v.to_runtime(cx, ty_layout.for_variant(cx, i)))
                    .collect(),
                Some((tag, tag_encoding, tag_field).to_runtime(cx, ty_layout)),
            ),
        };

        TypeInfo {
            id: type_id(tcx, ty),
            name: ty.to_string(),
            size,
            align: self.align().abi.bytes(),
            variants,
            tag,
            // NOTE: This also includes `Box` which may not be desired.
            pointee_ty: ty.builtin_deref(true).map(|t| type_id(tcx, t)),
        }
    }
}

impl<'tcx, Cx> ToRuntimeInfo<'tcx, Cx, VariantInfo> for &LayoutData<FieldIdx, VariantIdx>
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    type Def = TyAndLayout<'tcx>;

    fn to_runtime(self, cx: &Cx, ty_layout: Self::Def) -> VariantInfo
    where
        Cx: 'tcx,
    {
        let index = match self.variants {
            Variants::Single { index } => index,
            Variants::Empty | Variants::Multiple { .. } => {
                unreachable!("Empty and recursive variants are not expected")
            }
        };

        VariantInfo {
            index: index.as_u32(),
            fields: self.fields.to_runtime(cx, ty_layout),
        }
    }
}

impl<'tcx, Cx> ToRuntimeInfo<'tcx, Cx, TagInfo> for (&Scalar, &TagEncoding<VariantIdx>, &usize)
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    type Def = TyAndLayout<'tcx>;

    fn to_runtime(self, cx: &Cx, ty_layout: TyAndLayout<'tcx>) -> TagInfo
    where
        Cx: 'tcx,
    {
        let (tag, encoding, field) = self;
        log_debug!(target: TAG_TYPE_EXPORT, "Tag info: {:?}, {:?}, {:?} ", tag, encoding, field);
        TagInfo::Regular {
            as_field: to_field_info(ty_layout, cx, FieldIdx::from_usize(*field)),
            encoding: encoding.to_runtime(cx, ()),
        }
    }
}

impl<'tcx, Cx> ToRuntimeInfo<'tcx, Cx, TagEncodingInfo> for &TagEncoding<VariantIdx> {
    type Def = ();

    fn to_runtime(self, _cx: &Cx, _: ()) -> TagEncodingInfo
    where
        Cx: 'tcx,
    {
        match self {
            TagEncoding::Direct => TagEncodingInfo::Direct,
            TagEncoding::Niche {
                untagged_variant,
                niche_variants,
                niche_start,
            } => TagEncodingInfo::Niche {
                // The variant index is implicitly used as the value for the discriminant.
                non_niche_value: untagged_variant.as_u32() as u128,
                niche_value_range: (niche_variants.start().as_u32() as u128)
                    ..=(niche_variants.end().as_u32() as u128),
                tag_value_start: *niche_start,
            },
        }
    }
}

impl<'tcx, Cx> ToRuntimeInfo<'tcx, Cx, FieldsShapeInfo> for &FieldsShape<FieldIdx>
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    type Def = TyAndLayout<'tcx>;

    fn to_runtime(self, cx: &Cx, ty_layout: Self::Def) -> FieldsShapeInfo
    where
        Cx: 'tcx,
    {
        let tcx = cx.tcx();
        match self {
            FieldsShape::Primitive => FieldsShapeInfo::NoFields,
            FieldsShape::Union(count) => FieldsShapeInfo::Union(StructShape::new(
                (0..(*count).into())
                    .into_iter()
                    .map(|idx| to_field_info(ty_layout, cx, FieldIdx::from_usize(idx)))
                    .collect(),
            )),
            FieldsShape::Array { count, .. } => FieldsShapeInfo::Array(ArrayShape {
                len: *count,
                item_ty: type_id(tcx, field_ty(ty_layout, cx, FieldIdx::from_usize(0))),
            }),
            FieldsShape::Arbitrary { offsets, .. } => FieldsShapeInfo::Struct(StructShape::new(
                offsets
                    .clone()
                    .into_iter_enumerated()
                    .map(|(idx, _)| to_field_info(ty_layout, cx, idx))
                    .collect(),
            )),
        }
    }
}

fn to_field_info<'tcx, Cx>(ty_layout: TyAndLayout<'tcx>, cx: &Cx, index: FieldIdx) -> FieldInfo
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    let ty = field_ty(ty_layout, cx, index);
    FieldInfo {
        ty: type_id(cx.tcx(), ty),
        offset: ty_layout.fields.offset(index.as_usize()).bytes(),
    }
}

fn field_ty<'tcx, Cx>(ty_layout: TyAndLayout<'tcx>, cx: &Cx, index: FieldIdx) -> Ty<'tcx>
where
    Cx: HasTyCtxt<'tcx> + HasTypingEnv<'tcx>,
{
    /* NOTE: Guarantee on functionality correctness.
     * This method is obtained by checking the compiler's source code.
     * There are two places found so far that we can rely on to map the index
     * available in the layout to the type.
     * 1. `layout_of_uncached` in `rustc_ty_utils::layout`: The main function
     * that computes the layout of a type. It is called by `layout_of` in `TyCtxt`.
     * 2. `ty_and_layout_field` provided by `TyAbiInterface` trait.
     * The latter returns what we want directly and it takes shorter paths in
     * some cases. The former provides no direct method, and we probably can only
     * use the source to implement it.
     * The sources are checked manually and match for what we want.
     */
    use rustc_target::abi::TyAbiInterface;
    let result = Ty::ty_and_layout_field(ty_layout, cx, index.as_usize()).ty;

    if matches!(ty_layout.ty.kind(), TyKind::Ref(..) | TyKind::RawPtr(..)) && index.as_usize() == 0
    {
        // NOTE: The function returns the fat pointer type itself for the first
        // field if this is a fat pointer. We change it to the type of a pointer to unit.
        debug_assert_eq!(result, ty_layout.ty, "The assumption is not true.");
        Ty::new_mut_ptr(cx.tcx(), cx.tcx().types.unit)
    } else {
        result
    }
}
