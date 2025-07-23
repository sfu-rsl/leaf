/* What is going on for a symbolic read?
 * In a nutshell a symbolic read corresponds to multiple possible values.
 * In safe Rust, the only point for a symbolic read is a symbolic index over a slice.
 * However, any further projection on the symbolic index is effectively another symbolic read.
 * The difference is that the symbolic index is resolved to multiple possible values
 * but other projections select a single value out of a possible value.
 * In this way, we can separate the symbolic index from other projections and
 * apply other projections on the possible values of the underlying symbolic index (select).
 * Example:
 * If `a` is an array of compound values, `a[x].1` is equivalent to `b[x]`,
 * where `for all i. b[i] == a[i].1`.
 *
 * Symbolic indices over symbolic reads only add another layer of select.
 * So if `a[x]` is represented as `select(a, x)` then `a[x][y]` is represented as
 * `select(select(a, x), y)`.
 * This effectively means that it doesn't matter where the other projections are
 * located. In other words, both `*(a[x].1)[y]` and `*(a[x][y].1)` will be
 * represented by two layers selections.
 *
 * Assuming that symbolic variables only come from primitive types, the start
 * point for a symbolic read is always an index projection over a concrete array.
 * Moreover, if a projection is applied on a symbolic value, it has to be another
 * symbolic read, which may be resolved on demand.
 */

use derive_more::From;

use common::log_debug;

use crate::abs::expr::sym_place::SymbolicReadResolver;

use super::*;

type SymIndex = SymValueRef;
pub(crate) type Select<V = SymbolicPlaceResult> = crate::abs::expr::sym_place::Select<SymIndex, V>;

/// # Remarks about Array (old)
/// Although both `Select` and `SingleResult` can hold array of values, there are semantical
/// and operational differences between this variant and them. This is the result of
/// an expansion where we expect to see multiple values. This variant appears in nested
/// `Select`s. Let's consider `a[x][y].1` where `a` is an array of arrays.
/// We expect to see all the first field of a[_][_] as a possible value. This can be done either
/// by generating a `Select` for each possible value of `a[x]`, or having a nested select with a
/// 2D array of possible values. We choose the second one, which means a series of nested
/// `Select`s will be created with a last one having an array. However, the possible values
/// shape an n-D array which means the leaf nodes of a `Select` should be arrays.
/// Also, let's say we want to apply the last projection (the first field access). For a
/// consistent behavior, intermediate single values that are expected to be array need to be
/// resolved (expanded) to be able to iterate over their values. Again, this variant is
/// necessary to hold the expanded view.
pub(crate) type SymbolicPlaceResult =
    crate::abs::expr::sym_place::SymbolicReadTree<SymIndex, SinglePlaceResult>;

impl From<Vec<Self>> for SymbolicPlaceResult {
    fn from(value: Vec<Self>) -> Self {
        SymbolicPlaceResult::Array(value)
    }
}

impl From<SinglePlaceResult> for SymbolicPlaceResult {
    fn from(value: SinglePlaceResult) -> Self {
        SymbolicPlaceResult::Single(value)
    }
}

#[derive(Debug, Clone, From)]
pub(crate) struct SinglePlaceResult(pub DeterPlaceValueRef);

/// Resolves a symbolic place to the possible places it may refer to.
trait SymbolicPlaceResolver:
    for<'a> SymbolicReadResolver<SymIndex, PossibleValue<'a> = SymbolicPlaceResult>
{
    /// Expands the given value to another representation that is indexable.
    /// # Returns
    /// An expanded version of the given value.
    /// If the value gets resolved to a single possible value, that value must
    /// be indexable and it is expanded to an `Array`,
    /// otherwise a `Select` will be returned to be expanded later.
    /// The returned value is guaranteed not to be a `Single`.
    fn expand<'a>(&self, value: &'a SinglePlaceResult) -> SymbolicPlaceResult;
}

pub(crate) struct DefaultSymPlaceResolver<'a> {
    type_manager: &'a dyn TypeDatabase,
    retriever: &'a dyn RawPointerRetriever,
}

impl<'a> DefaultSymPlaceResolver<'a> {
    pub(crate) fn new(
        type_manager: &'a dyn TypeDatabase,
        retriever: &'a dyn RawPointerRetriever,
    ) -> Self {
        Self {
            type_manager,
            retriever,
        }
    }
}

mod implementation {
    use crate::{
        abs::expr::sym_place::{
            SelectTarget, SymbolicReadResolver, SymbolicReadTreeLeafMutator::*,
        },
        type_info::TypeInfoExt,
    };

    use super::*;
    use common::{log_warn, type_info::ArrayShape};

    impl SymbolicPlaceResolver for DefaultSymPlaceResolver<'_> {
        #[tracing::instrument(level = "debug", skip(self))]
        fn expand<'a>(&self, place: &'a SinglePlaceResult) -> SymbolicPlaceResult {
            SymbolicPlaceResult::Array(
                self.expand_deterministic(place.0.as_ref())
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
        }
    }

    /// # Remarks
    /// If there is concrete host, we know for sure that the result is a select.
    /// However, transmutations turn into symbolic reads if there is a symbolic index.
    impl SymbolicReadResolver<SymIndex> for DefaultSymPlaceResolver<'_> {
        type SymValue<'a> = &'a SymbolicPlaceValue;
        type PossibleValue<'a> = SymbolicPlaceResult;

        #[tracing::instrument(level = "debug", skip(self))]
        fn resolve<'a>(&self, place_value: Self::SymValue<'a>) -> Select<Self::PossibleValue<'a>> {
            let mut base = match &place_value.base {
                SymbolicPlaceBase::Deref(host) => self.resolve_deref_of_sym(host),
                SymbolicPlaceBase::SymIndex(indexed) => self.resolve_sym_indexed(indexed),
            };
            if let Some(proj) = &place_value.proj {
                base.mutate_leaves(
                    Mutator(&mut |p| {
                        p.0 = DeterPlaceValueRef::new(proj.on_deter(p.0.as_ref()).to_value_ref())
                    }),
                    |v| self.expand(v),
                );
            }
            base
        }
    }

    impl DefaultSymPlaceResolver<'_> {
        fn resolve_deref_of_sym(&self, host: &DerefSymHostPlace) -> Select {
            let pointee_type_id = self
                .type_manager
                .get_type(&host.host_type_id)
                .pointee_ty
                .expect("Host type must be a pointer type.");

            self.deref_symbolic(host.host.as_ref(), pointee_type_id)
        }

        fn deref_symbolic(&self, host: &SymValue, pointee_type_id: TypeId) -> Select {
            let unexpected = || unreachable!("Unexpected symbolic host to dereference: {:?}", host);
            let SymValue::Expression(expr) = host else {
                unexpected()
            };
            match expr {
                Expr::Multi(multi) => self.deref_multi(multi, pointee_type_id),
                // Offset
                Expr::Binary(_) => todo!(),
                // Cast
                Expr::Ite { .. } | Expr::Truncation(..) | Expr::Extension(..) => todo!(),
                Expr::Partial(..) => todo!(),
                Expr::Concat(..) => todo!(),
                Expr::Transmutation { .. } => todo!(),
                Expr::Unary { .. }
                | Expr::BinaryBoundCheck { .. }
                | Expr::Ref(_)
                | Expr::Len(_)
                | Expr::PtrMetadata(..) => unexpected(),
            }
        }

        fn deref_multi(&self, multi: &MultiValue, pointee_type_id: TypeId) -> Select {
            multi.map_expand(
                |index| {
                    if index.from_end {
                        todo!("Index from end is not supported yet.")
                    }
                    index.index.clone()
                },
                |value: &ValueRef| match value.as_ref() {
                    Value::Concrete(host) => SymbolicPlaceResult::Single(
                        DeterPlaceValueRef::new(
                            self.deref_concrete(host, pointee_type_id).to_value_ref(),
                        )
                        .into(),
                    ),
                    Value::Symbolic(host) => {
                        SymbolicPlaceResult::SymRead(self.deref_symbolic(host, pointee_type_id))
                    }
                },
            )
        }

        #[tracing::instrument(level = "debug", skip(self))]
        fn deref_concrete(
            &self,
            host: &ConcreteValue,
            pointee_type_id: TypeId,
        ) -> DeterministicPlaceValue {
            match host {
                ConcreteValue::Const(host) => self.deref_const(host, pointee_type_id),
                ConcreteValue::FatPointer(host) => {
                    host.deref(self.type_manager, self.retriever).into()
                }
                ConcreteValue::Unevaluated(UnevalValue::Lazy(host)) => {
                    panic!(
                        "Lazy unevaluated value should be retrieved before: {:?}",
                        host
                    )
                }
                _ => {
                    unreachable!("Unexpected concrete value to dereference: {:?}", host)
                }
            }
        }

        #[tracing::instrument(level = "debug", skip(self))]
        fn deref_const(
            &self,
            host: &ConstValue,
            pointee_type_id: TypeId,
        ) -> DeterministicPlaceValue {
            match host {
                ConstValue::Addr(addr) => {
                    DeterministicPlaceValue::from_addr_type(*addr, pointee_type_id)
                }
                _ => unreachable!("Unexpected constant value to dereference: {:?}", host),
            }
        }

        fn resolve_sym_indexed(&self, indexed: &SymIndexedPlace) -> Select {
            let target = match indexed.host.as_ref() {
                PlaceValue::Deterministic(..) => self.expand(&SinglePlaceResult(
                    DeterPlaceValueRef::new(indexed.host.clone()),
                )),
                PlaceValue::Symbolic(sym_host) => {
                    let mut resolved = self.resolve(sym_host);
                    resolved.mutate_leaves(Replacer(&mut |p| self.expand(p)), |v| {
                        // This should not happen anymore in this algorithm
                        log_warn!("Unexpanded value in the new algorithm: {:?}", v);
                        self.expand(v)
                    });
                    resolved.into()
                }
            };
            let target = match target {
                SymbolicPlaceResult::SymRead(current) => SelectTarget::Nested(Box::new(current)),
                SymbolicPlaceResult::Array(values) => SelectTarget::Array(values),
                SymbolicPlaceResult::Single(..) => {
                    unreachable!("The single value is expected to be expanded at this point.")
                }
            };
            Select {
                index: indexed.index.clone(),
                target,
            }
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(super) fn expand_deterministic(
            &self,
            place: &DeterministicPlaceValue,
        ) -> Vec<SinglePlaceResult> {
            let ty = place.type_info().get_type(self.type_manager).unwrap();
            let array = ty.expect_array();
            debug_assert!(
                !ty.is_slice(),
                "Slice should be replaced by a pseudo-array."
            );
            self.expand_array(place.address(), array)
        }

        #[tracing::instrument(level = "debug", skip(self))]
        fn expand_array(
            &self,
            base_address: RawAddress,
            shape: &ArrayShape,
        ) -> Vec<SinglePlaceResult> {
            let item_ty = self.type_manager.get_type(&shape.item_ty);
            let mut result = Vec::with_capacity(shape.len as usize);
            for i in 0..shape.len {
                /* NOTE: Wait, shouldn't we pay attention to `align` here?
                 * https://doc.rust-lang.org/reference/type-layout.html#size-and-alignment
                 * > The size of a value is the offset in bytes between successive elements in an array
                 * with that item type including alignment padding.
                 */
                let item_addr = base_address.wrapping_byte_offset((i * item_ty.size) as isize);
                let place = DeterministicPlaceValue::from_addr_type(item_addr, item_ty.id);
                result.push(DeterPlaceValueRef::new(place.to_value_ref()).into());
            }
            result
        }

        pub(super) fn expand_symbolic(&self, value: &SymValue) -> SymbolicPlaceResult {
            todo!()
        }
    }

    pub(super) mod utils {}
}
