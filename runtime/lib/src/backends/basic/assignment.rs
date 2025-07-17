use core::{iter, ops::DerefMut};

use common::type_info::TagEncodingInfo;

use crate::{
    abs::{
        AssignmentId, BinaryOp, CastKind, FieldIndex, InstanceKindId, IntType, UnaryOp,
        VariantIndex,
        backend::AssignmentHandler,
        expr::{BinaryExprBuilder, CastExprBuilder, TernaryExprBuilder},
    },
    utils::alias::RRef,
};

use crate::backends::basic as backend;
use backend::{
    BasicBackend, BasicValue, BasicValueExprBuilder, CallStackInfo, Implied, PlaceValueRef,
    Precondition, TypeDatabase, TypeLayoutResolver, ValueRef, VariablesState,
    alias::BasicExprBuilder, expr::prelude::*, implication::PreconditionConstruct,
    place::DiscriminantPossiblePlace, type_info::TypeLayoutResolverExt,
};

#[cfg(feature = "implicit_flow")]
use backend::ImplicationInvestigator;

pub(crate) struct BasicAssignmentHandler<'s, EB> {
    #[cfg(feature = "implicit_flow")]
    id: AssignmentId,
    #[cfg(feature = "implicit_flow")]
    current_func: InstanceKindId,
    dest: PlaceValueRef,
    vars_state: &'s mut dyn VariablesState,
    expr_builder: RRef<EB>,
    type_manager: &'s dyn TypeDatabase,
    #[cfg(feature = "implicit_flow")]
    implication_investigator: &'s dyn ImplicationInvestigator,
}

impl<'s> BasicAssignmentHandler<'s, BasicExprBuilder> {
    pub(super) fn new(
        id: AssignmentId,
        dest: PlaceValueRef,
        backend: &'s mut BasicBackend,
    ) -> Self {
        Self {
            #[cfg(feature = "implicit_flow")]
            id,
            #[cfg(feature = "implicit_flow")]
            current_func: backend.call_stack_manager.current_func().body_id,
            dest,
            vars_state: backend.call_stack_manager.top(),
            expr_builder: backend.expr_builder.clone(),
            type_manager: backend.type_manager.as_ref(),
            #[cfg(feature = "implicit_flow")]
            implication_investigator: backend.implication_investigator.as_ref(),
        }
    }
}

impl<EB: BasicValueExprBuilder> AssignmentHandler for BasicAssignmentHandler<'_, EB> {
    type Place = PlaceValueRef;
    type DiscriminablePlace = DiscriminantPossiblePlace;
    type Operand = BasicValue;

    fn use_of(mut self, operand: Self::Operand) {
        self.set(operand)
    }

    fn repeat_of(mut self, operand: Self::Operand, count: usize) {
        self.set_value(operand.map_value(|value| {
            if value.is_symbolic() {
                ConcreteValue::Array(ArrayValue {
                    elements: vec![value; count],
                })
                .into()
            } else {
                UnevalValue::Some.into()
            }
        }))
    }

    fn ref_to(mut self, place: Self::Place, _is_mutable: bool) {
        use backend::expr::place::*;
        match place.as_ref() {
            PlaceValue::Symbolic(sym_place) => match &sym_place.base {
                // Reborrowing
                SymbolicPlaceBase::Deref(DerefSymHostPlace { host, .. }) => {
                    let value = self.expr_builder().transmute(
                        // FIXME: retain antecedents
                        Implied::by_unknown(host.clone().into()),
                        self.dest.type_info().id().unwrap(),
                        self.dest.type_info().clone(),
                    );
                    self.set(value);
                }
                SymbolicPlaceBase::SymIndex(..) => {
                    // FIXME: retain antecedents
                    self.set_value(Implied::by_unknown(
                        Expr::Ref(SymIndexPlaceValueRef::new(place)).into(),
                    ));
                }
            },
            PlaceValue::Deterministic(..) => {
                self.set_value(Implied::always(UnevalValue::Some.into()))
            }
        }
    }

    fn thread_local_ref_to(mut self) {
        // Thread local references cannot refer to symbolic places, so the reference is concrete.
        self.set_value(Implied::by_unknown(UnevalValue::Some.into()))
    }

    fn address_of(self, place: Self::Place, is_mutable: bool) {
        // For symbolic values `ref_to` and `address_of` should have the same behavior.
        self.ref_to(place, is_mutable)
    }

    fn len_of(mut self, place: Self::Place) {
        let value = if place.is_symbolic() {
            Expr::Len(SymPlaceValueRef::new(place)).into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(Implied::by_unknown(value))
    }

    fn cast_of(mut self, operand: Self::Operand, target: CastKind) {
        let cast_value = self
            .expr_builder()
            .cast(operand, target, self.dest.type_info().clone());

        self.set(cast_value)
    }

    fn binary_op_between(
        mut self,
        operator: BinaryOp,
        first: Self::Operand,
        second: Self::Operand,
    ) {
        let result_value = self.expr_builder().binary_op((first, second), operator);
        self.set(result_value)
    }

    fn unary_op_on(mut self, operator: UnaryOp, operand: Self::Operand) {
        let result_value = self.expr_builder().unary_op(operand, operator);
        self.set(result_value)
    }

    fn discriminant_from(mut self, place: Self::DiscriminablePlace) {
        let discr_value = match place {
            DiscriminantPossiblePlace::None => {
                // https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/enum.Rvalue.html#variant.Discriminant
                Implied::always(
                    ConstValue::new_int(0_u128, self.get_int_type(self.dest.type_info()))
                        .to_value_ref(),
                )
            }
            DiscriminantPossiblePlace::SingleVariant { discr_bit_rep } => Implied::always(
                ConstValue::new_int(discr_bit_rep, self.get_int_type(self.dest.type_info()))
                    .to_value_ref(),
            ),
            DiscriminantPossiblePlace::TagPlaceWithInfo(tag_place, tag_encoding) => {
                let tag_value = self.vars_state.copy_place(&tag_place);
                if tag_value.is_symbolic() {
                    tag_value.map_value(|value| {
                        self.build_discriminant_expr(
                            SymValueRef::new(value),
                            tag_encoding,
                            self.dest.type_info(),
                            tag_place.type_info(),
                        )
                        .into()
                    })
                } else {
                    tag_value
                }
            }
        };

        self.set(discr_value)
    }

    #[cfg_attr(not(feature = "implicit_flow"), allow(unused))]
    fn array_from(mut self, elements: impl Iterator<Item = Self::Operand>) {
        let (preconditions, values) = elements
            .map(Implied::into_tuple)
            .unzip::<_, _, Vec<_>, Vec<_>>();

        #[cfg(not(feature = "implicit_flow"))]
        let precondition = Precondition::unknown();
        #[cfg(feature = "implicit_flow")]
        let precondition = self.precondition_of_array(preconditions);

        let value = ConcreteValue::Array(ArrayValue {
            elements: values.into_iter().collect(),
        });

        self.set_no_ant(Implied {
            by: precondition,
            value: value.to_value_ref(),
        })
    }

    fn adt_from(
        mut self,
        fields: impl Iterator<Item = Self::Field>,
        variant: Option<VariantIndex>,
    ) {
        let kind = match variant {
            Some(variant) => AdtKind::Enum { variant },
            None => AdtKind::Struct,
        };
        self.set_adt_value(kind, fields.map(|f| Some(f)))
    }

    fn union_from(mut self, active_field: FieldIndex, value: Self::Field) {
        let fields = (0..active_field)
            .map(|_| None)
            .chain(iter::once(Some(value)));
        self.set_adt_value(AdtKind::Struct, fields.into_iter())
    }

    fn raw_ptr_from(self, data_ptr: Self::Operand, metadata: Self::Operand, _is_mutable: bool) {
        let data_ptr = self.ensure_type_of_ptr_for_raw_ptr(data_ptr);
        self.adt_from([data_ptr, metadata].into_iter(), None)
    }

    // TODO: Need to add support for the Deinit MIR instruction to have this working properly.
    // This solution works for now to avoid crashes when samples are run.
    fn variant_index(mut self, variant_index: VariantIndex) {
        // FIXME: This implementation relies on internals of the VariablesState.
        let value = Value::Concrete(ConcreteValue::Adt(AdtValue {
            kind: AdtKind::Enum {
                variant: variant_index,
            },
            fields: vec![],
        }));
        self.set_value(Implied::always(value))
    }

    fn shallow_init_box_from(self, value: Self::Operand) {
        /* According to the Rust MIR documentation:
         * https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/enum.Rvalue.html#variant.ShallowInitBox
         * > Transmutes a *mut u8 into shallow-initialized Box<T>.
         * BTW, very improbable to have a symbolic value here. */
        let dst_ty_id = self.dest.type_id();
        self.cast_of(value, CastKind::Transmute(dst_ty_id));
    }

    fn use_if_eq(mut self, val: Self::Operand, current: Self::Operand, expected: Self::Operand) {
        let are_eq = self.expr_builder().eq((current.clone(), expected.clone()));
        let are_eq = if !are_eq.is_symbolic() {
            are_eq.map_value(|value| {
                // As it will be abstracted to Some, we need to resolve them explicitly here.
                let current = ConcreteValueRef::new(current.value.clone())
                    .try_resolve_as_const(self.type_manager);
                let expected =
                    ConcreteValueRef::new(expected.value).try_resolve_as_const(self.type_manager);
                match (current, expected) {
                    (Some(current), Some(expected)) => {
                        let are_eq = current == expected;
                        ConstValue::Bool(are_eq).to_value_ref()
                    }
                    _ => value,
                }
            })
        } else {
            are_eq
        };
        let value = self.expr_builder().if_then_else((are_eq, val, current));
        self.set(value)
    }

    fn use_and_check_eq(mut self, val: Self::Operand, expected: Self::Operand) {
        let are_eq = self.expr_builder().eq((val.clone(), expected));
        self.set_adt_value(
            AdtKind::Struct,
            [Some(val.clone()), Some(are_eq)].into_iter(),
        )
    }
}

impl<EB: BasicValueExprBuilder> BasicAssignmentHandler<'_, EB> {
    #[inline]
    fn set_value(&mut self, value: Implied<Value>) {
        self.set(value.map_value(Value::to_value_ref));
    }

    fn set(&mut self, mut value: BasicValue) {
        #[cfg(feature = "implicit_flow")]
        self.add_antecedent(&mut value);
        self.set_no_ant(value);
    }

    #[inline]
    fn set_no_ant(&mut self, value: BasicValue) {
        self.vars_state.set_place(&self.dest, value);
    }

    fn expr_builder(&self) -> impl DerefMut<Target = EB> + '_ {
        self.expr_builder.as_ref().borrow_mut()
    }

    #[cfg_attr(not(feature = "implicit_flow"), allow(unused))]
    fn set_adt_value(
        &mut self,
        kind: AdtKind,
        fields: impl Iterator<Item = Option<<Self as AssignmentHandler>::Field>>,
    ) {
        let (preconditions, values) = fields
            .map(|f| -> (Option<Precondition>, Option<ValueRef>) {
                f.map(Implied::into_tuple).unzip()
            })
            .unzip::<_, _, Vec<_>, Vec<_>>();

        #[cfg(not(feature = "implicit_flow"))]
        let precondition = Precondition::unknown();
        #[cfg(feature = "implicit_flow")]
        let precondition = self.precondition_of_adt(&kind, preconditions);

        let value = ConcreteValue::Adt(AdtValue {
            kind,
            fields: values.into_iter().map(|f| AdtField { value: f }).collect(),
        });

        self.set_no_ant(Implied {
            by: precondition,
            value: value.to_value_ref(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn build_discriminant_expr(
        &self,
        tag_value: SymValueRef,
        tag_encoding: &TagEncodingInfo,
        discr_ty_info: &LazyTypeInfo,
        tag_ty_info: &LazyTypeInfo,
    ) -> SymValueRef {
        use TagEncodingInfo::*;
        match tag_encoding {
            Direct => tag_value,
            Niche {
                non_niche_value,
                niche_value_range,
                tag_value_start,
            } => {
                let discr_ty = self.get_int_type(discr_ty_info);
                let tag_ty = self.get_int_type(tag_ty_info);

                let into_tag_value = |v: u128| ConstValue::new_int(v, tag_ty).to_value_ref();
                let into_discr_value = |v: u128| ConstValue::new_int(v, discr_ty).to_value_ref();

                // Based on: `rustc_codegen_ssa::mir::place::PlaceRef::codegen_get_discr`
                let niche_start = *niche_value_range.start();
                let tag_value: ValueRef = tag_value.into();
                let relative_max = niche_value_range.end() - niche_start;
                let (is_niche, tagged_discr) = if relative_max == 0 {
                    let is_niche = SymValueRef::new(
                        self.expr_builder()
                            .inner()
                            .eq((tag_value.clone(), into_tag_value(*tag_value_start)).into())
                            .into(),
                    );
                    let tagged_discr = into_discr_value(niche_start);
                    (is_niche, tagged_discr)
                } else {
                    let relative_tag_value: ValueRef = self
                        .expr_builder()
                        .inner()
                        .sub((tag_value.clone(), into_tag_value(*tag_value_start)).into())
                        .into();
                    let is_niche = SymValueRef::new(
                        self.expr_builder()
                            .inner()
                            .le((relative_tag_value.clone(), into_tag_value(relative_max)).into())
                            .into(),
                    );
                    let relative_discr_value = self
                        .expr_builder()
                        .inner()
                        .to_int(relative_tag_value.into(), discr_ty, discr_ty_info.clone())
                        .into();
                    let tagged_discr = self
                        .expr_builder()
                        .inner()
                        .add((relative_discr_value, into_discr_value(niche_start)).into())
                        .into();
                    let tagged_discr = self.expr_builder().inner().to_int(
                        tagged_discr,
                        discr_ty,
                        discr_ty_info.clone(),
                    );
                    debug_assert!(tagged_discr.is_symbolic());
                    (is_niche, tagged_discr)
                };

                let discr_value = self.expr_builder().inner().if_then_else((
                    is_niche.into(),
                    tagged_discr.into(),
                    into_discr_value(*non_niche_value),
                ));
                SymValueRef::new(discr_value)
            }
        }
    }

    fn ensure_type_of_ptr_for_raw_ptr(&self, data_ptr: BasicValue) -> BasicValue {
        if !data_ptr.is_symbolic() {
            return data_ptr;
        }

        let field_ty = self
            .type_manager
            .layouts()
            .resolve_adt_fields(self.dest.type_id(), None)
            .next()
            .unwrap()
            .0;

        self.expr_builder()
            .transmute(data_ptr, field_ty, LazyTypeInfo::from(field_ty))
    }

    fn get_int_type(&self, ty_info: &LazyTypeInfo) -> IntType {
        let ty = self
            .type_manager
            .try_to_value_type(ty_info.clone())
            .expect("Expected the type of the discriminant raw value to be a primitive");
        *ty.as_int()
            // https://doc.rust-lang.org/reference/type-layout.html#primitive-representations
            .unwrap_or_else(|| panic!("Expected the type of the tag to be a int type: {:?}", ty))
    }
}

#[cfg(feature = "implicit_flow")]
pub(super) mod precondition {
    use std::{borrow::Cow, num::NonZero};

    use common::type_info::TagInfo;

    use crate::utils::RangeIntersection;

    use crate::backends::basic as backend;
    use backend::{
        TypeLayoutResolver,
        implication::{PreconditionConstraints, PreconditionQuery},
        type_info::TypeLayoutResolverExt,
    };

    use super::*;

    impl<EB> BasicAssignmentHandler<'_, EB> {
        pub(super) fn add_antecedent(&self, value: &mut BasicValue) {
            add_antecedent(
                self.implication_investigator,
                self.type_manager,
                &self.dest,
                (self.current_func, self.id),
                value,
            );
        }

        pub(super) fn precondition_of_array(&self, elements: Vec<Precondition>) -> Precondition {
            let for_elements = self
                .implication_investigator
                .antecedent_of_latest_assignment((self.current_func, self.id));

            // If non of the elements have preconditions, don't bother with refined information.
            let elements = if elements.iter().any(|p| p.is_some()) {
                let element_intervals = self
                    .type_manager
                    .layouts()
                    .resolve_array_elements(self.dest.type_id())
                    .1;
                let elements = elements
                    .into_iter()
                    .zip(element_intervals)
                    .map(|(mut p, (offset, size))| {
                        if let Some(for_elements) = for_elements.as_ref() {
                            p = p.add(Cow::Borrowed(for_elements), || size);
                        }
                        (p, (offset, size))
                    })
                    .flat_map(|(p, (offset, size))| {
                        p.take_constraints()
                            .zip(NonZero::new(size).map(|s| (offset, s)))
                    })
                    .flat_map(|(c, (offset, size))| c.at_loc(offset, size));
                PreconditionConstraints::refined(elements)
            } else {
                None
            };

            Precondition::new(elements.or(for_elements.map(Into::into)))
        }

        pub(super) fn precondition_of_adt(
            &self,
            kind: &AdtKind,
            fields: Vec<Option<Precondition>>,
        ) -> Precondition {
            let opt_tag_interval = self
                .dest
                .type_info()
                .get_type(self.type_manager)
                .unwrap()
                .tag
                .as_ref()
                .and_then(|tag| {
                    if let TagInfo::Regular { as_field, encoding } = tag {
                        let tag_ty = self.type_manager.get_type(&as_field.ty);
                        Some((
                            (
                                as_field.offset,
                                tag_ty.size().and_then(NonZero::new).unwrap(),
                            ),
                            matches!(encoding, TagEncodingInfo::Niche { .. }),
                        ))
                    } else {
                        None
                    }
                });

            let (for_fields, for_tag) = if opt_tag_interval.is_some() {
                let antecedents = self
                    .implication_investigator
                    .antecedent_of_latest_enum_assignment((self.current_func, self.id));

                let (for_fields, for_tag) = antecedents.map(|a| (a.fields, a.tag)).unzip();
                (for_fields.flatten(), for_tag)
            } else {
                // If not enum, work as regular.
                let for_fields = self
                    .implication_investigator
                    .antecedent_of_latest_assignment((self.current_func, self.id));
                (for_fields, None)
            };

            let mut fields = if fields.iter().flatten().any(|p| p.is_some()) {
                let field_intervals = self
                    .type_manager
                    .layouts()
                    .resolve_adt_fields(self.dest.type_id(), kind.variant_index())
                    .map(|(_, offset, size)| (offset, size));
                fields
                    .into_iter()
                    .zip(field_intervals)
                    .flat_map(|(p, interval)| p.map(|p| (p, interval)))
                    .map(|(mut p, (offset, size))| {
                        if let Some(for_fields) = for_fields.as_ref() {
                            p = p.add(Cow::Borrowed(for_fields), || size);
                        }
                        (p, (offset, size))
                    })
                    .flat_map(|(p, (offset, size))| {
                        p.take_constraints()
                            .zip(NonZero::new(size).map(|s| (offset, s)))
                    })
                    .flat_map(|(c, (offset, size))| c.at_loc(offset, size))
                    .collect::<Vec<_>>()
            } else {
                Vec::with_capacity(for_tag.is_some() as usize)
            };

            if let Some(antecedents) = for_tag {
                let ((tag_offset, tag_size), is_niche) = opt_tag_interval.unwrap();
                if is_niche {
                    // If the tag is niche, and it is the niche variant assignment
                    let tag_range = tag_offset..(tag_offset + tag_size.get());
                    if let Some((offset, size, existing)) =
                        fields.iter_mut().find(|(offset, size, _)| {
                            RangeIntersection::is_overlapping(
                                &(*offset..(offset + size.get())),
                                &tag_range,
                            )
                        })
                    {
                        // Then it should fall into one of the fields.
                        debug_assert!(RangeIntersection::contains(
                            &(*offset..(*offset + size.get())),
                            &tag_range,
                        ));
                        existing.extend_with(&antecedents);
                    } else {
                        fields.push((tag_offset, tag_size, antecedents));
                    }
                } else {
                    fields.push((tag_offset, tag_size, antecedents));
                }
            }

            Precondition::new(
                PreconditionConstraints::refined(fields).or(for_fields.map(Into::into)),
            )
        }
    }

    pub(crate) fn add_antecedent(
        implication_investigator: &(impl ImplicationInvestigator + ?Sized),
        type_manager: &dyn TypeDatabase,
        dest: &PlaceValueRef,
        assignment_id: (InstanceKindId, AssignmentId),
        value: &mut BasicValue,
    ) {
        let Some(antecedent) =
            implication_investigator.antecedent_of_latest_assignment(assignment_id)
        else {
            return;
        };

        value.add_antecedents(Cow::Owned(antecedent), || {
            dest.type_info().get_size(type_manager).unwrap()
        });
    }
}
