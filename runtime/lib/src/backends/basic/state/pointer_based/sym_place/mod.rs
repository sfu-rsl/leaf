mod resolution;
pub(crate) mod strategies;

use core::{cell::RefMut, iter, ops::Bound};

use common::log_debug;

use crate::{
    abs::{
        expr::sym_place::{SymbolicReadResolver, SymbolicReadTreeLeafMutator},
        place::HasMetadata,
    },
    backends::basic::{
        expr::{place::*, SliceIndex},
        place::PlaceMetadata,
        state::proj::IndexResolver,
    },
};

use super::*;

use self::resolution::Select as PlaceSelect;
use crate::backends::basic::expr::MultiValue as ValueSelect;

use self::resolution::{DefaultSymPlaceResolver, SinglePlaceResult};

impl RawPointerVariableState {
    pub(super) fn get_place<'a, 'b>(
        &'a self,
        place: &'b Place,
        mut sym_place_handler: RefMut<'a, SymPlaceHandlerObject>,
    ) -> PlaceValueRef {
        let place_value = self.get_place_iter_raw(
            place.local().metadata(),
            place.projections(),
            place.projs_metadata(),
        );
        if place_value.is_symbolic() {
            sym_place_handler.handle(SymPlaceValueRef::new(place_value), place.metadata())
        } else {
            place_value
        }
    }

    pub(super) fn get_place_iter_raw<'b, I: Iterator<Item = &'b PlaceMetadata>>(
        &self,
        mut host_metadata: &'b PlaceMetadata,
        mut projs: &'b [Projection],
        mut projs_metadata: I,
    ) -> PlaceValueRef {
        /* NOTE: How does this work?
         * The main responsibility of this is function is to check if any non-determinism
         * is present for the given place. The following cases are only expected:
         * 1. Dereferencing a symbolic value.
         * 2. Indexing by a symbolic value.
         * Note that other projections on symbolic values do not cause non-determinism
         * for the place and they correspond to a single deterministic place (but with
         * a symbolic value).
         */

        // 1. Dereferencing a symbolic value.
        let opt_sym_deref = projs.first().is_some_and(|p| matches!(p, Projection::Deref)).then(|| {
            let opt_sym_deref = self.opt_sym_deref(host_metadata);

            // Symbolic or not, pass the deref projection.
            host_metadata = projs_metadata.next().unwrap();
            projs = &projs[(Bound::Excluded(0), Bound::Unbounded)];
            debug_assert!(
                !has_deref(projs.iter().skip(1)),
                "Based on the documentation, Deref can only appear as the first projection after MIR optimizations."
            );

            opt_sym_deref
        }).flatten();

        let value = opt_sym_deref
            .map(Into::into)
            .unwrap_or_else(|| DeterministicPlaceValue::new(host_metadata.clone()).to_value_ref());

        projs
            .iter()
            .zip(
                iter::once(host_metadata)
                    .chain(projs_metadata.by_ref())
                    .map_windows(|[a, b]| (*a, *b)),
            )
            .fold(value, |mut value, (proj, (host_meta, meta))| {
                // 2. Indexing by a symbolic value.
                let opt_sym_index = self.opt_sym_index(&value, host_meta, proj);

                let project_further = || {
                    match PlaceValueRef::make_mut(&mut value) {
                        PlaceValue::Deterministic(deter) => {
                            *deter = DeterministicPlaceValue::new(meta.clone());
                        }
                        PlaceValue::Symbolic(sym) => {
                            sym.proj = Some(to_deterministic_proj(
                                sym.proj.as_ref(),
                                proj,
                                host_meta,
                                meta,
                            ));
                        }
                    }
                    value
                };

                opt_sym_index
                    .map(Into::into)
                    .unwrap_or_else(project_further)
            })
    }

    fn opt_sym_deref(&self, host_metadata: &PlaceMetadata) -> Option<SymPlaceValueRef> {
        let host =
            self.copy_deterministic_place(&DeterministicPlaceValue::new(host_metadata.clone()));

        match host.as_ref() {
            Value::Concrete(_) => None,
            Value::Symbolic(SymValue::Variable(..)) => {
                unreachable!("Unexpected symbolic variable to dereference: {:?}", host)
            }
            Value::Symbolic(SymValue::Expression(Expr::Ref(place))) => Some(place.clone()),
            Value::Symbolic(SymValue::Expression(..)) => {
                log_debug!("Deref of symbolic value observed: {}", host);
                Some(SymPlaceValueRef::new(
                    SymbolicPlaceValue::from_base(DerefSymHostPlace {
                        value: SymValueRef::new(host),
                        metadata: host_metadata.clone(),
                    })
                    .to_value_ref(),
                ))
            }
        }
    }

    fn opt_sym_index<'b>(
        &self,
        host: &PlaceValueRef,
        host_meta: &PlaceMetadata,
        proj: &'b Projection,
    ) -> Option<SymPlaceValueRef> {
        let opt_sym_index_val = match proj {
            Projection::Index(index_local) => {
                let index_val = IndexResolver::get(self, index_local);
                index_val
                    .is_symbolic()
                    .then(|| SymValueRef::new(index_val))
                    .map(|index_val| (index_val, index_local))
            }
            Projection::ConstantIndex {
                offset,
                min_length: _,
                from_end: true,
            } => self
                .opt_sym_index_val_from_end(host.as_ref(), *offset)
                .map(|index_val| {
                    let index_local = todo!("#480: Index metadata is required for concretization");
                    (index_val, index_local)
                }),
            _ => None,
        };

        opt_sym_index_val.map(|(index_val, index_local)| {
            log_debug!("Symbolic index observed: {}", index_val.as_ref());
            SymPlaceValueRef::new(
                SymbolicPlaceValue::from_base(SymIndexedPlace {
                    host: host.clone(),
                    host_metadata: host_meta.clone(),
                    index: index_val,
                    index_metadata: index_local.metadata().clone(),
                })
                .to_value_ref(),
            )
        })
    }

    fn opt_sym_index_val_from_end(&self, host: &PlaceValue, offset: u64) -> Option<SymValueRef> {
        // FIXME: As indices from end refer to only one element, it is more reasonable
        // to introduce a new symbolic place kind and handle them in the resolver properly.

        // NOTE: Only slices may show up here, which will be dereferenced before this projection.
        // https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/enum.ProjectionElem.html#variant.ConstantIndex.field.from_end
        if let PlaceValue::Symbolic(
            sym_host @ SymbolicPlaceValue {
                base: SymbolicPlaceBase::Deref(..),
                ..
            },
        ) = host
        {
            log_debug!(
                "Index from end of a symbolic slice observed: {}, {}",
                host,
                offset
            );
            let len = self.retrieve_len_value(sym_host);
            let index: SymValueRef = todo!("Add expression builder");
            return Some(index);
        }

        None
    }
}

impl RawPointerVariableState {
    pub(super) fn resolve_and_retrieve_symbolic_place(
        &self,
        place_val: &SymbolicPlaceValue,
        type_id: TypeId,
    ) -> SymValueRef {
        let resolved = self.resolve_symbolic_place(place_val);
        let mut copied = self.get_select_sym_place_result(&resolved);
        self.retrieve_multi_value(&mut copied, type_id);
        Expr::Multi(copied).to_value_ref()
    }
}

// Getting Symbolic Place
impl RawPointerVariableState {
    fn resolve_symbolic_place(&self, place_val: &SymbolicPlaceValue) -> PlaceSelect {
        let resolver = DefaultSymPlaceResolver::new(self.type_manager.as_ref(), self);
        resolver.resolve(place_val)
    }

    fn get_select_sym_place_result(&self, select: &PlaceSelect) -> ValueSelect {
        select.map_leaves(
            |index| SliceIndex {
                index: index.clone(),
                from_end: false,
            },
            |place| self.get_single_sym_place_result(place),
        )
    }

    fn get_single_sym_place_result(&self, single: &SinglePlaceResult) -> ValueRef {
        self.copy_deterministic_place(single.0.as_ref())
    }
}

// Retrieving (Raw) Values
impl RawPointerVariableState {
    /// Retrieves the memory content for the given symbolic value.
    /// It makes sure that the result value can live independently with no
    /// lazily-evaluated parts.
    /// In fact checks if the symbolic value is a multi expression then
    /// retrieves all of its possible values recursively.
    pub(super) fn retrieve_sym_value(
        &self,
        mut value: SymValueRef,
        type_id: TypeId,
    ) -> SymValueRef {
        match value.as_ref() {
            SymValue::Expression(expr) => match expr {
                Expr::Multi(..) => {
                    let SymValue::Expression(Expr::Multi(select)) =
                        SymValueRef::make_mut(&mut value)
                    else {
                        unreachable!()
                    };
                    self.retrieve_multi_value(select, type_id);
                    value
                }
                Expr::Partial(porter) => self.retrieve_porter_value(porter).to_value_ref(),
                Expr::Len(place) => self.retrieve_len_value(place),
                Expr::Projection(ProjExpr::SymHost(SymHostProj {
                    host,
                    kind: ProjKind::Field(FieldAccessKind::PtrMetadata),
                    metadata: _,
                })) => self.retrieve_ptr_metadata(host.as_ref()),
                _ => value,
            },
            _ => value,
        }
    }

    fn retrieve_multi_value(&self, select: &mut MultiValue, type_id: TypeId) {
        log_debug!(
            "Retrieving select projection result: {} with type {type_id}",
            select
        );
        select.mutate_leaves(
            SymbolicReadTreeLeafMutator::Replacer(&mut |value| {
                let retrieved = self.retrieve_value(value.clone(), type_id);
                match retrieved.as_ref() {
                    Value::Symbolic(SymValue::Expression(Expr::Multi(..))) => {
                        let Value::Symbolic(SymValue::Expression(Expr::Multi(multi))) =
                            ValueRef::unwrap_or_clone(retrieved)
                        else {
                            unreachable!()
                        };
                        MultiValueTree::SymRead(multi)
                    }
                    _ => MultiValueTree::Single(retrieved),
                }
            }),
            |_| unreachable!("No expansion is expected here."),
        )
    }

    fn retrieve_value(&self, value: ValueRef, type_id: TypeId) -> ValueRef {
        match value.as_ref() {
            Value::Concrete(_) => self
                .retrieve_conc_value(ConcreteValueRef::new(value), type_id)
                .into(),
            Value::Symbolic(_) => self
                .retrieve_sym_value(SymValueRef::new(value), type_id)
                .into(),
        }
    }

    fn retrieve_conc_value(&self, value: ConcreteValueRef, type_id: TypeId) -> ConcreteValueRef {
        ConcreteValueRef::new(match value.as_ref() {
            ConcreteValue::Array(array) => {
                let item_type_id = self.get_type(type_id).expect_array().item_ty;
                ArrayValue {
                    elements: array
                        .elements
                        .iter()
                        .map(|element| self.retrieve_value(element.clone(), item_type_id))
                        .collect(),
                }
                .to_value_ref()
            }
            ConcreteValue::Adt(adt) => {
                let variant_index = match adt.kind {
                    AdtKind::Enum { variant } => Some(variant),
                    _ => None,
                };
                AdtValue {
                    kind: adt.kind.clone(),
                    fields: adt
                        .fields
                        .iter()
                        .zip(self.get_type(type_id).child_type_ids(variant_index))
                        .map(|(field, type_id)| AdtField {
                            value: field
                                .value
                                .as_ref()
                                .map(|value| self.retrieve_value(value.clone(), type_id)),
                        })
                        .collect(),
                }
                .to_value_ref()
            }
            ConcreteValue::FatPointer(fat_ptr) => {
                debug_assert_eq!(fat_ptr.ty, type_id, "Type ids are not consistent.");
                let field_type_ids = self.get_type(type_id).child_type_ids(None);
                debug_assert_eq!(
                    field_type_ids.len(),
                    2,
                    "A fat pointer is expected to have two fields."
                );
                // FIXME: Implicit assumption about the order of fields.
                FatPtrValue {
                    address: self.retrieve_conc_value(fat_ptr.address.clone(), field_type_ids[0]),
                    metadata: self.retrieve_conc_value(fat_ptr.metadata.clone(), field_type_ids[1]),
                    ty: fat_ptr.ty,
                }
                .to_value_ref()
            }
            ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
                let raw = if let Some(id) = raw.2.id() {
                    debug_assert_eq!(id, type_id, "The type id is not consistent.");
                    raw.clone()
                } else {
                    RawConcreteValue(raw.0, raw.1.clone(), LazyTypeInfo::Id(type_id))
                };
                let retrieved = unsafe { raw.retrieve(self.type_manager.as_ref(), self) }.unwrap();
                // Possible to introduce retrievable values (e.g., arrays) again.
                self.retrieve_conc_value(retrieved, type_id).into()
            }
            _ => value.into(),
        })
    }

    pub(super) fn retrieve_porter_value(&self, porter: &PorterValue) -> PorterValue {
        PorterValue {
            as_concrete: porter.as_concrete.clone(),
            sym_values: porter
                .sym_values
                .iter()
                .map(|(index, type_id, sym_value)| {
                    (
                        *index,
                        *type_id,
                        self.retrieve_sym_value(sym_value.clone(), *type_id),
                    )
                })
                .collect(),
        }
    }

    fn retrieve_len_value(&self, place: &SymbolicPlaceValue) -> SymValueRef {
        let SymbolicPlaceValue {
            base: SymbolicPlaceBase::Deref(host),
            proj: None,
        } = place
        else {
            unreachable!(
                "Len place is expected to be applied on a dereference of slice pointer. Got: {:?}",
                place
            )
        };

        // Equivalent to accessing the pointer's metadata.
        self.retrieve_ptr_metadata(host.value.as_ref())
    }

    fn retrieve_ptr_metadata(&self, host: &SymValue) -> SymValueRef {
        match host {
            SymValue::Expression(Expr::Multi(multi)) => Expr::from(multi.map_leaves(
                Clone::clone,
                |value| match value.as_ref() {
                    Value::Symbolic(host) => self.retrieve_ptr_metadata(host).into(),
                    Value::Concrete(ConcreteValue::FatPointer(fat_ptr)) => {
                        fat_ptr.metadata.0.clone()
                    }
                    _ => unreachable!(
                        "Only (retrieved) fat pointers are expected to appear. Got: {:?}",
                        value
                    ),
                },
            ))
            .to_value_ref(),
            SymValue::Expression(Expr::Projection(ProjExpr::SymHost(SymHostProj {
                kind: ProjKind::Downcast(DowncastKind::Transmutation(..)),
                ..
            }))) => {
                todo!("#443, #454: PtrMetadata from transmuted value is not supported yet.")
            }
            _ => {
                unreachable!(
                    "Only retrieved multi values are expected to retrieve pointer metadata from. Got: {:?}",
                    host
                )
            }
        }
    }
}

#[inline]
fn has_deref<'p>(mut projs: impl Iterator<Item = &'p Projection>) -> bool {
    projs.any(|p| matches!(p, Projection::Deref))
}

fn to_deterministic_proj<'a>(
    current: Option<&DeterministicProjection>,
    proj: &'a Projection,
    host_meta: &PlaceMetadata,
    meta: &PlaceMetadata,
) -> DeterministicProjection {
    debug_assert!(!matches!(proj, Projection::Deref));

    let offset = unsafe { meta.address().byte_offset_from(host_meta.address()) };
    let offset = current.map_or(0, |p| p.offset) + PointerOffset::try_from(offset).unwrap();

    DeterministicProjection {
        offset,
        ty_id: meta.unwrap_type_id(),
    }
}
