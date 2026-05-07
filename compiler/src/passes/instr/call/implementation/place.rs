use rustc_middle::mir::{PlaceRef as MirPlaceRef, ProjectionElem};

use common::log_warn;

use super::{
    BodyProvider, PlaceReferencer, PlaceStructureRules,
    ctxt_reqs::ForPlaceRef,
    prelude::{mir::*, *},
};

impl<'tcx, C> PlaceReferencer<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForPlaceRef<'tcx>,
{
    fn reference_place(&mut self, place: &Place<'tcx>) -> PlaceRef {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_place(place);
        self.insert_blocks(new_blocks);
        reference.into()
    }
}
impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForPlaceRef<'tcx>,
{
    pub(super) fn internal_reference_place(
        &mut self,
        place: &Place<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let mut blocks = Vec::new();

        let tcx = self.tcx();

        let referrals =
            filter_and_fold_place(&self.context.config().place_info_filter.structure, place);

        let set_type_addr = |this: &mut Self,
                             blocks: &mut Vec<BasicBlockData<'tcx>>,
                             mut place_ref: Local,
                             rel_place: MirPlaceRef<'tcx>| {
            let ty = rel_place.ty(this.local_decls(), tcx).ty;
            if let Some(BlocksAndResult(new_blocks, new_ref)) = this.add_place_type(place_ref, ty) {
                blocks.extend(new_blocks);
                place_ref = new_ref;
            }
            if let Some(BlocksAndResult(new_blocks, new_ref)) =
                this.add_place_addr(place_ref, rel_place.to_place(tcx), ty)
            {
                blocks.extend(new_blocks);
                place_ref = new_ref;
            }
            place_ref
        };

        // For setting addresses we have to remake a cumulative place up to each projection.
        let mut place_ref = {
            let (block, mut place_ref, rel_place) = match referrals.base {
                PlaceReferralBase::Local(local) => {
                    let (block, place_ref) = self.internal_reference_place_local(local);
                    (block, place_ref, local.into())
                }
                PlaceReferralBase::SomePlace(rel_place) => {
                    let (block, place_ref) = self.internal_ref_place_some();
                    (block, place_ref, rel_place)
                }
            };
            blocks.push(block);
            place_ref = set_type_addr(self, &mut blocks, place_ref, rel_place);

            place_ref
        };

        for proj in referrals.projs {
            let (BlocksAndResult(added_blocks, new_ref), cur_place) = match proj {
                PlaceReferralProj::Projection(rel_place) => (
                    self.internal_reference_place_projection(
                        place_ref,
                        rel_place.last_projection().unwrap().1,
                    ),
                    rel_place,
                ),
                PlaceReferralProj::SomeProjection(rel_place) => (
                    self.make_bb_for_place_ref_call(
                        sym::ref_place_some_proj,
                        vec![operand::copy_for_local(place_ref)],
                    )
                    .into(),
                    rel_place,
                ),
            };
            blocks.extend(added_blocks);
            place_ref = new_ref;
            place_ref = set_type_addr(self, &mut blocks, place_ref, cur_place);
        }

        BlocksAndResult(blocks, place_ref)
    }

    fn internal_ref_place_some(&mut self) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_helper_call_with_ret(
            self.pri_helper_funcs().ref_place_some_encoded,
            Vec::default(),
        )
    }

    pub(super) fn reference_place_local(&mut self, local: Local) -> BlocksAndResult<'tcx> {
        let (block, mut place_ref) = self.internal_reference_place_local(local);
        let mut blocks = vec![block];

        let ty = self.local_decls()[local].ty;
        if let Some(BlocksAndResult(new_blocks, new_ref)) = self.add_place_type(place_ref, ty) {
            blocks.extend(new_blocks);
            place_ref = new_ref;
        }
        if let Some(BlocksAndResult(new_blocks, new_ref)) =
            self.add_place_addr(place_ref, local.into(), ty)
        {
            blocks.extend(new_blocks);
            place_ref = new_ref;
        }

        BlocksAndResult(blocks, place_ref)
    }

    fn internal_reference_place_local(&mut self, local: Local) -> (BasicBlockData<'tcx>, Local) {
        let kind = self.body().local_kind(local);
        use rustc_middle::mir::LocalKind::*;
        let func = match kind {
            Temp => self.pri_helper_funcs().ref_place_local_encoded,
            Arg => self.pri_helper_funcs().ref_place_argument_encoded,
            ReturnPointer => self.pri_helper_funcs().ref_place_return_value_encoded,
        };
        let args = match kind {
            ReturnPointer => vec![],
            _ => vec![operand::const_from_uint(self.context.tcx(), local.as_u32())],
        };

        self.make_bb_for_helper_call_with_ret(func, args)
    }

    fn internal_reference_place_projection<T>(
        &mut self,
        current_ref: Local,
        proj: ProjectionElem<Local, T>,
    ) -> BlocksAndResult<'tcx> {
        let mut blocks = Vec::new();

        let (func_name, additional_args) = match proj {
            ProjectionElem::Deref => (sym::ref_place_deref, vec![]),
            ProjectionElem::Field(index, _) => (
                sym::ref_place_field,
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    u32::from(index),
                )],
            ),
            ProjectionElem::Index(index) => {
                let BlocksAndResult(additional_blocks, index_ref) =
                    self.internal_reference_place(&Place::from(index));
                blocks.extend(additional_blocks);
                (
                    sym::ref_place_index,
                    vec![operand::copy_for_local(index_ref)],
                )
            }
            ProjectionElem::ConstantIndex {
                offset,
                min_length,
                from_end,
            } => (
                sym::ref_place_constant_index,
                vec![
                    operand::const_from_uint(self.context.tcx(), offset),
                    operand::const_from_uint(self.context.tcx(), min_length),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ],
            ),
            ProjectionElem::Subslice { from, to, from_end } => (
                sym::ref_place_subslice,
                vec![
                    operand::const_from_uint(self.context.tcx(), from),
                    operand::const_from_uint(self.context.tcx(), to),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ],
            ),
            ProjectionElem::Downcast(_, index) => (
                sym::ref_place_downcast,
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    u32::from(index),
                )],
            ),
            ProjectionElem::OpaqueCast(_) => (sym::ref_place_opaque_cast, vec![]),
            ProjectionElem::UnwrapUnsafeBinder(..) => (sym::ref_place_unwrap_unsafe_binder, vec![]),
        };

        let (block, place_ref) = self.make_bb_for_place_ref_call(
            func_name,
            [vec![operand::copy_for_local(current_ref)], additional_args].concat(),
        );
        blocks.push(block);
        BlocksAndResult(blocks, place_ref)
    }

    fn make_bb_for_place_ref_call(
        &mut self,
        func_name: LeafSymbol,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_call_with_ret(func_name, args)
    }

    fn add_place_addr(
        &mut self,
        place_ref: Local,
        place: Place<'tcx>,
        place_ty: Ty<'tcx>,
    ) -> Option<BlocksAndResult<'tcx>> {
        if !self.context.config().place_info_filter.address {
            return None;
        }

        let (stmt, ptr_local) = utils::ptr_to_place(self.tcx(), &mut self.context, place, place_ty);
        let (mut block, place_ref) = {
            self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().place_with_address_typed,
                vec![place_ty.into()],
                vec![
                    operand::copy_for_local(place_ref),
                    operand::move_for_local(ptr_local),
                ],
                None,
            )
        };
        block.statements.push(stmt);
        Some((block, place_ref).into())
    }

    pub(super) fn add_place_size(
        &mut self,
        place_ref: Local,
        place_ty: Ty<'tcx>,
    ) -> Option<BlocksAndResult<'tcx>> {
        if !place_ty.is_sized(self.tcx(), self.current_typing_env()) {
            log_warn!("Encountered unsized type. Skipping size setting.");
            return None;
        }

        let (get_call_block, size_local) = self.make_bb_for_helper_call_with_all(
            self.context.pri_helper_funcs().size_of,
            vec![place_ty.into()],
            Vec::default(),
            None,
        );

        let (set_call_block, place_ref) = self.make_bb_for_call_with_ret(
            sym::place_with_size,
            vec![
                operand::copy_for_local(place_ref),
                operand::move_for_local(size_local),
            ],
        );

        Some(BlocksAndResult(
            vec![get_call_block, set_call_block],
            place_ref,
        ))
    }

    fn add_place_type(
        &mut self,
        mut place_ref: Local,
        ty: Ty<'tcx>,
    ) -> Option<BlocksAndResult<'tcx>> {
        if !self.context.config().place_info_filter.ty {
            return None;
        }

        let mut blocks = vec![];

        if ty.is_primitive() {
            let primitive_ty_local = {
                let (block, local) = self.make_primitive_type_of_bb(ty);
                blocks.push(block);
                local
            };
            let (block, new_ref) = self.make_bb_for_call_with_ret(
                sym::place_with_primitive_type,
                vec![
                    operand::copy_for_local(place_ref),
                    operand::move_for_local(primitive_ty_local),
                ],
            );
            blocks.push(block);
            place_ref = new_ref;
        } else {
            let type_id_local = {
                let (block, id_local) = self.make_type_id_of_bb(ty);
                blocks.push(block);
                id_local
            };

            let (block, new_ref) = self.make_bb_for_call_with_ret(
                sym::place_with_type_id,
                vec![
                    operand::copy_for_local(place_ref),
                    operand::move_for_local(type_id_local),
                ],
            );
            blocks.push(block);
            place_ref = new_ref;
        }

        Some(BlocksAndResult(blocks, place_ref))
    }
}

enum PlaceReferralBase<'tcx> {
    Local(Local),
    SomePlace(MirPlaceRef<'tcx>),
}

enum PlaceReferralProj<'tcx> {
    SomeProjection(MirPlaceRef<'tcx>),
    Projection(MirPlaceRef<'tcx>),
}

struct PlaceReferralChain<'tcx> {
    base: PlaceReferralBase<'tcx>,
    projs: Vec<PlaceReferralProj<'tcx>>,
}

fn filter_and_fold_place<'tcx>(
    config: &PlaceStructureRules<bool>,
    place: &Place<'tcx>,
) -> PlaceReferralChain<'tcx> {
    let mut base = if config.local {
        PlaceReferralBase::Local(place.local)
    } else {
        PlaceReferralBase::SomePlace(MirPlaceRef {
            local: place.local,
            projection: rustc_middle::ty::List::empty(),
        })
    };

    let mut projs = Vec::new();
    for place_ref in (0..place.projection.len()).map(move |i| MirPlaceRef {
        local: place.local,
        projection: &place.projection[..=i],
    }) {
        let include = match place_ref.last_projection().unwrap().1 {
            ProjectionElem::Deref => config.deref,
            ProjectionElem::Field(..) => config.field,
            ProjectionElem::Index(_) => config.index,
            ProjectionElem::ConstantIndex { .. } => config.constant_index,
            ProjectionElem::Subslice { .. } => config.subslice,
            ProjectionElem::Downcast(..) => config.downcast,
            ProjectionElem::OpaqueCast(..) => config.opaque_cast,
            ProjectionElem::UnwrapUnsafeBinder(..) => config.unwrap_unsafe_binder,
        };

        if include {
            projs.push(PlaceReferralProj::Projection(place_ref));
        } else {
            match projs.last_mut() {
                Some(PlaceReferralProj::SomeProjection(prev_ref)) => {
                    *prev_ref = place_ref;
                }
                None if let PlaceReferralBase::SomePlace(base_ref) = &mut base => {
                    *base_ref = place_ref;
                }
                _ => {
                    projs.push(PlaceReferralProj::SomeProjection(place_ref));
                }
            }
        }
    }

    PlaceReferralChain { base, projs }
}

mod utils {
    pub(super) use super::super::utils::{operand, ptr_to_place};
}
use utils::*;
