use core::{any::Any, ops::Deref};

use paste::paste;
use rustc_hir::def_id::{self, DefId};
use rustc_middle::ty::TyCtxt;

use delegate::delegate;

use crate::{
    config::rules::{InclusionRules, LogicFormula},
    passes::{Storage, StorageExt},
    utils::rules::{InclusionPredicate, Predicate, ToPredicate},
};

use super::super::config::*;

pub(crate) const KEY_RULES: &str = "instr_rules";
pub(crate) const KEY_BAKED_BODY_RULES: &str = "i_r_b_body";
pub(crate) const KEY_BAKED_DYN_DEF_RULES: &str = "i_r_b_dyn_def";
pub(crate) const KEY_BAKED_PLACE_INFO_RULES: &str = "i_r_b_place_info";
pub(crate) const KEY_BAKED_OPERAND_INFO_RULES: &str = "i_r_b_operand_info";
pub(crate) const KEY_BAKED_CONST_TYPE_RULES: &str = "i_r_b_const_ty";
pub(crate) const KEY_BAKED_ASSIGNMENT_RULES: &str = "i_r_b_assignment";
pub(crate) const KEY_BAKED_ASSIGNMENT_INFO_RULES: &str = "i_r_b_assignment_info";
pub(crate) const KEY_BAKED_STORAGE_LIFETIME_RULES: &str = "i_r_b_storage_lifetime";
pub(crate) const KEY_BAKED_CALL_FLOW_RULES: &str = "i_r_b_call_flow";
pub(crate) const KEY_BAKED_DROP_RULES: &str = "i_r_b_drop";
pub(crate) const KEY_BAKED_SWITCH_RULES: &str = "i_r_b_switch";

type LocationQuery<'tcx> = (TyCtxt<'tcx>, DefId);

type EntityLocationFilterPredicate<'tcx> =
    <LogicFormula<EntityLocationFilter> as ToPredicate<LocationQuery<'tcx>>>::Predicate;

type BakedEntityLocationFilterRules<'tcx> = InclusionPredicate<EntityLocationFilterPredicate<'tcx>>;

type BakedWholeBodyFilterRules<'tcx> = BakedEntityLocationFilterRules<'tcx>;

type BakedMethodDynDefinitionFilterRules<'tcx> = BakedEntityLocationFilterRules<'tcx>;

macro_rules! query_and_baked_aliases {
    ($stem:ident, $part:ty) => {
        paste! {
            type [<$stem Query>]<'tcx> = ($part, LocationQuery<'tcx>);
            type [<Baked $stem FilterRules>]<'tcx> =
                InclusionPredicate<<[<$stem Filter>] as ToPredicate<[<$stem Query>]<'tcx>>>::Predicate>;
        }
    };
}

macro_rules! rules_struct_from_enum {
    (
        $(#[$meta:meta])*
        $name:ident match $enum:path {
            $(
                $variant:ident => $field:ident
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        pub(crate) struct $name<T> {
            $(
                pub $field: T,
            )+
        }

        impl<T> $name<T> {
            #[allow(dead_code)]
            pub(crate) fn accept(mut f: impl FnMut($enum) -> T) -> Self {
                Self {
                    $(
                        $field: f(<$enum>::$variant),
                    )+
                }
            }

            #[allow(unused)]
            pub(crate) fn map<U>(self, f: impl Fn(T) -> U) -> $name<U> {
                $name {
                    $($field: f(self.$field),)+
                }
            }
        }

        // Ensuring exhaustiveness
        const _: fn($enum) = |value| match value {
            $(
                <$enum>::$variant => (),
            )+
        };
    };
}

/// Defines a filter-rule group from an enum.
///
/// For a given `$stem` and enum mapping, this macro generates:
/// - `{$stem}Rules<T>`: a struct with one field per mapping entry.
/// - `{$stem}Query<'tcx>`: `(EnumVariantType, LocationQuery<'tcx>)`.
/// - `Baked{$stem}FilterRules<'tcx>`: baked inclusion predicate rules.
///
/// Example:
///
/// ```rust,ignore
/// define_filter_rule_group!(
///     Switch match SwitchPartKind {
///         Control => control,
///         Data => data,
///     }
/// );
///
/// // Generated (conceptually):
/// // pub(crate) struct SwitchRules<T> { pub control: T, pub data: T }
/// // type SwitchQuery<'tcx> = (SwitchPartKind, LocationQuery<'tcx>);
/// // type BakedSwitchFilterRules<'tcx> = InclusionPredicate<<SwitchFilter as ToPredicate<SwitchQuery<'tcx>>>::Predicate>;
/// ```
macro_rules! define_filter_rule_group {
    (
        $(#[$meta:meta])*
        $stem:ident match $enum:path {
            $(
                $variant:ident => $field:ident
            ),+ $(,)?
        }
    ) => {
        paste! {
            rules_struct_from_enum!(
                $(#[$meta])*
                [<$stem Rules>] match $enum {
                    $(
                        $variant => $field
                    ),+
                }
            );
        }
        query_and_baked_aliases!($stem, $enum);
    };
}

pub(crate) struct PlaceInfoRules<TS, TA = TS, TT = TA> {
    pub structure: TS,
    pub address: TA,
    pub ty: TT,
}

type BakedPlaceInfoFilterRules<'tcx> =
    PlaceInfoRules<BakedPlaceStructureFilterRules<'tcx>, BakedEntityLocationFilterRules<'tcx>>;

define_filter_rule_group!(
    PlaceStructure match PlaceStructurePiece {
        Local => local,
        Deref => deref,
        Field => field,
        Index => index,
        ConstantIndex => constant_index,
        Subslice => subslice,
        Downcast => downcast,
        OpaqueCast => opaque_cast,
        UnwrapUnsafeBinder => unwrap_unsafe_binder,
    }
);

pub(crate) struct OperandKindRules<T, TC = T> {
    pub copy: T,
    pub mov: T,
    pub constant: TC,
}

query_and_baked_aliases!(OperandKind, OperandKind);

define_filter_rule_group!(
    ConstantType match ConstantType {
        Bool => bool,
        Char => char,
        Int => int,
        Float => float,
        Str => str,
        ByteStr => byte_str,
        Ptr => ptr,
        Zst => zst,
    }
);

define_filter_rule_group!(
    Assignment match AssignmentKind {
        Use => use_,
        Repeat => repeat,
        Ref => ref_,
        ThreadLocalRef => thread_local_ref,
        RawPtr => raw_ptr,
        Cast => cast,
        BinaryOp => binary_op,
        UnaryOp => unary_op,
        Discriminant => discriminant,
        Aggregate => aggregate,
        WrapUnsafeBinder => wrap_unsafe_binder,
        IntrinsicBinaryOp => intrinsic_binary_op,
        IntrinsicUnaryOp => intrinsic_unary_op,
        IntrinsicMemoryOp => intrinsic_memory_op,
        AtomicBinaryOp => atomic_binary_op,
        AtomicMemoryOp => atomic_memory_op,
    }
);
type BakedAssignmentInfoFilterRules<'tcx> = BakedAssignmentFilterRules<'tcx>;

define_filter_rule_group!(
    StorageLifetimeMarker match StorageLifetimeMarkerKind {
        Live => live,
        Dead => dead,
    }
);

define_filter_rule_group!(
    CallFlow match CallFlowPartKind {
        CallControl => call_control,
        CallInput => call_input,
        CallAddress => call_address,
        FunctionData => func_data,
        FunctionAddress => func_address,
    }
);

define_filter_rule_group!(
    Drop match DropPartKind {
        CallControl => control,
        CallInput => input,
        CallAddress => call_address,
    }
);

define_filter_rule_group!(
    Switch match SwitchPartKind {
        Control => control,
        Data => data,
    }
);

pub(crate) fn get_baked_body_rules<'tcx>(
    storage: &mut dyn Storage,
) -> impl Deref<Target = BakedEntityLocationFilterRules<'tcx>> + '_ {
    get_baked_rules(storage, KEY_BAKED_BODY_RULES)
}

fn get_baked_rules<'a, T: Any>(
    storage: &'a mut dyn Storage,
    key: &str,
) -> impl Deref<Target = T> + 'a {
    let key = key.to_owned();
    storage
        .get_mut::<T>(&key)
        .expect("Filter rules are expected to be baked at this point.")
}

pub(crate) fn accept_dyn_def_filter_rules<'tcx, I>(
    storage: &mut dyn Storage,
    item: &I,
) -> Option<bool>
where
    EntityLocationFilterPredicate<'tcx>: Predicate<I>,
{
    get_baked_rules::<BakedEntityLocationFilterRules<'tcx>>(storage, KEY_BAKED_DYN_DEF_RULES)
        .accept(item)
}

pub(crate) type PlaceInfoFilterResult =
    PlaceInfoRules<PlaceStructureRules<Option<bool>>, Option<bool>>;

pub(crate) fn accept_place_info_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> PlaceInfoFilterResult {
    let rules =
        get_baked_rules::<BakedPlaceInfoFilterRules<'tcx>>(storage, KEY_BAKED_PLACE_INFO_RULES);

    PlaceInfoRules {
        structure: PlaceStructureRules::accept(|piece| rules.structure.accept(&(piece, *item))),
        address: rules.address.accept(item),
        ty: rules.ty.accept(item),
    }
}

pub(crate) type OperandKindFilterResult = OperandKindRules<Option<bool>>;

pub(crate) fn accept_operand_info_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> OperandKindFilterResult {
    let rules =
        get_baked_rules::<BakedOperandKindFilterRules<'tcx>>(storage, KEY_BAKED_OPERAND_INFO_RULES);

    use OperandKind::*;
    OperandKindRules {
        copy: rules.accept(&(Copy, *item)),
        mov: rules.accept(&(Move, *item)),
        constant: rules.accept(&(Constant, *item)),
    }
}

pub(crate) type ConstantTypeFilterResult = ConstantTypeRules<Option<bool>>;

pub(crate) fn accept_constant_type_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> ConstantTypeFilterResult {
    let rules =
        get_baked_rules::<BakedConstantTypeFilterRules<'tcx>>(storage, KEY_BAKED_CONST_TYPE_RULES);

    ConstantTypeRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(crate) type AssignmentFilterResult = AssignmentRules<Option<bool>>;

pub(crate) fn accept_assignment_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
    info: bool,
) -> AssignmentFilterResult {
    let key = if !info {
        KEY_BAKED_ASSIGNMENT_RULES
    } else {
        KEY_BAKED_ASSIGNMENT_INFO_RULES
    };
    let rules = get_baked_rules::<BakedAssignmentFilterRules<'tcx>>(storage, key);

    AssignmentRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(crate) type StorageLifetimeFilterResult = StorageLifetimeMarkerRules<Option<bool>>;

pub(crate) fn accept_storage_lifetime_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> StorageLifetimeFilterResult {
    let rules = get_baked_rules::<BakedStorageLifetimeMarkerFilterRules<'tcx>>(
        storage,
        KEY_BAKED_STORAGE_LIFETIME_RULES,
    );

    StorageLifetimeMarkerRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(crate) type CallFlowFilterResult = CallFlowRules<Option<bool>>;

pub(crate) fn accept_call_flow_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> CallFlowFilterResult {
    let rules =
        get_baked_rules::<BakedCallFlowFilterRules<'tcx>>(storage, KEY_BAKED_CALL_FLOW_RULES);

    CallFlowRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(crate) type DropFilterResult = DropRules<Option<bool>>;

pub(crate) fn accept_drop_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> DropFilterResult {
    let rules = get_baked_rules::<BakedDropFilterRules<'tcx>>(storage, KEY_BAKED_DROP_RULES);

    DropRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(crate) type SwitchFilterResult = SwitchRules<Option<bool>>;

pub(crate) fn accept_switch_rules<'tcx>(
    storage: &mut dyn Storage,
    item: &LocationQuery<'tcx>,
) -> SwitchFilterResult {
    let rules = get_baked_rules::<BakedSwitchFilterRules<'tcx>>(storage, KEY_BAKED_SWITCH_RULES);

    SwitchRules::accept(|kind| rules.accept(&(kind, *item)))
}

pub(super) fn bake_rules(
    storage: &mut dyn Storage,
    additional_exclusions: impl FnOnce() -> Vec<WholeBodyFilter>,
) {
    macro_rules! bake_entity_filter_rules {
        (
            $key:expr,
            $variant:ident,
            $baked:ty,
            |$rules:ident| $body:block
        ) => {
            // We use explicit types to ensure not using the wrong type by mistake.
            let _ = storage.get_or_insert_with_acc($key.to_owned(), |storage| -> $baked {
                let all_rules = storage.get_or_default::<InstrumentationRules>(KEY_RULES.to_owned());
                #[allow(unused_mut)]
                let mut $rules: InclusionRules<_> = all_rules.clone().filter_map(|r| match r {
                    EntityFilter::$variant(filter) => Some(filter),
                    _ => None,
                });
                $body;
                $rules.to_baked()
            });
        };
        ($key:expr, $variant:ident, |$rules:ident| $body:block $(,)?) => {
            paste! { bake_entity_filter_rules!($key, $variant, [<Baked $variant FilterRules>]<'_>, |$rules| $body); }
        };
        ($key:expr, $variant:ident $(,)?) => {
            bake_entity_filter_rules!($key, $variant, |rules| {});
        };
    }

    bake_entity_filter_rules!(KEY_BAKED_BODY_RULES, WholeBody, |rules| {
        rules.exclude.extend(additional_exclusions());
    });
    bake_entity_filter_rules!(KEY_BAKED_DYN_DEF_RULES, MethodDynDefinition);
    bake_entity_filter_rules!(KEY_BAKED_OPERAND_INFO_RULES, OperandKind);
    bake_entity_filter_rules!(KEY_BAKED_CONST_TYPE_RULES, ConstantType);
    bake_entity_filter_rules!(KEY_BAKED_ASSIGNMENT_RULES, Assignment);
    bake_entity_filter_rules!(KEY_BAKED_ASSIGNMENT_INFO_RULES, AssignmentInfo);
    bake_entity_filter_rules!(KEY_BAKED_STORAGE_LIFETIME_RULES, StorageLifetimeMarker);
    bake_entity_filter_rules!(KEY_BAKED_CALL_FLOW_RULES, CallFlow);
    bake_entity_filter_rules!(KEY_BAKED_DROP_RULES, Drop);
    bake_entity_filter_rules!(KEY_BAKED_SWITCH_RULES, Switch);

    // Place is a bit structurally different
    let _ = storage.get_or_insert_with_acc(
        KEY_BAKED_PLACE_INFO_RULES.to_owned(),
        |storage| -> BakedPlaceInfoFilterRules<'_> {
            let rules = storage.get_or_default::<InstrumentationRules>(KEY_RULES.to_owned());
            let rules = rules.clone().filter_map(|r| match r {
                EntityFilter::PlaceInfo(filter) => Some(filter),
                _ => None,
            });
            BakedPlaceInfoFilterRules {
                structure: rules
                    .clone()
                    .filter_map(|r| match r {
                        PlaceInfoFilter::Structure(filter) => Some(filter),
                        _ => None,
                    })
                    .to_baked(),
                address: rules
                    .clone()
                    .filter_map(|r| match r {
                        PlaceInfoFilter::Address(filter) => Some(filter),
                        _ => None,
                    })
                    .to_baked(),
                ty: rules
                    .clone()
                    .filter_map(|r| match r {
                        PlaceInfoFilter::Type(filter) => Some(filter),
                        _ => None,
                    })
                    .to_baked(),
            }
        },
    );
}

impl ToPredicate<LocationQuery<'_>> for EntityLocationFilter {
    type Predicate = Box<dyn Fn(&LocationQuery<'_>) -> bool>;

    fn to_predicate(&self) -> Self::Predicate {
        match self {
            EntityLocationFilter::Crate(crate_filter) => match crate_filter.clone() {
                CrateFilter::Externality(is_external) => {
                    Box::new(move |(_, def_id)| def_id.is_local() != is_external)
                }
                CrateFilter::Name(name) => {
                    let pred = name.to_predicate();
                    Box::new(move |(tcx, def_id)| {
                        pred.accept(tcx.crate_name(def_id.krate).as_str())
                    })
                }
            },
            EntityLocationFilter::DefPathMatch(pattern) => {
                let pred = pattern.to_predicate();
                Box::new(move |(tcx, def_id)| {
                    let def_path = tcx.def_path_str(*def_id);
                    pred.accept(&def_path)
                })
            }
            EntityLocationFilter::DefId(def_id) => {
                let def_id = *def_id;
                Box::new(move |(_, query_def_id)| {
                    query_def_id.krate.as_u32() == def_id.0
                        && query_def_id.index.as_u32() == def_id.1
                })
            }
        }
    }
}

macro_rules! impl_to_predicate_for_loc_type {
    ($name:ty, $field:ident, $query:ty, $filter:ty) => {
        impl<'tcx> ToPredicate<$query> for $name {
            type Predicate = <$filter as ToPredicate<$query>>::Predicate;

            delegate! {
                to self.$field {
                    fn to_predicate(&self) -> Self::Predicate;
                }
            }
        }
    };

    ($($name:ty),+$(,)?) => {
        $(
            impl_to_predicate_for_loc_type!($name, loc, LocationQuery<'tcx>, LogicFormula<EntityLocationFilter>);
        )*
    };
}

impl_to_predicate_for_loc_type!(
    WholeBodyFilter,
    MethodDynDefinitionFilter,
    PlaceAddressFilter,
    PlaceTypeFilter,
);

macro_rules! impl_to_predicate_by_eq {
    ($($name:ty),+$(,)?) => {
        $(
             impl ToPredicate<$name> for $name {
                 type Predicate = Box<dyn Fn(&Self) -> bool>;

                 fn to_predicate(&self) -> Self::Predicate {
                     let this = *self;
                     Box::new(move |other| this.eq(other))
                 }
             }
        )*
    };
}

impl_to_predicate_by_eq!(
    PlaceStructurePiece,
    OperandKind,
    ConstantType,
    AssignmentKind,
    StorageLifetimeMarkerKind,
    CallFlowPartKind,
    DropPartKind,
    SwitchPartKind,
);

macro_rules! impl_to_predicate_for_filter {
    ($filter:ty, $query:ty, $other:ident) => {
        impl ToPredicate<$query> for $filter {
            type Predicate = Box<dyn Fn(&$query) -> bool>;

            fn to_predicate(&self) -> Self::Predicate {
                let pred = (
                    self.$other.to_predicate(),
                    self.loc.to_predicate(),
                );
                Box::new(move |(q0, q1)| {
                    pred.0.accept(q0) && pred.1.accept(q1)
                })
            }
        }
    };

    ($filter:ty, $query:ty) => { impl_to_predicate_for_filter!($filter, $query, kind); };

    ($({ $filter:ty, $query:ty $(,$other:ident)? }),+ $(,)?) => {
        $(impl_to_predicate_for_filter!($filter, $query $(, $other)?);)+
    };
}
impl_to_predicate_for_filter!(
    { PlaceStructureFilter, PlaceStructureQuery<'_>, piece },
    { OperandKindFilter,  OperandKindQuery<'_> },
    { ConstantTypeFilter, ConstantTypeQuery<'_>, ty },
    { AssignmentFilter, AssignmentQuery<'_> },
    { StorageLifetimeMarkerFilter, StorageLifetimeMarkerQuery<'_> },
    { CallFlowFilter, CallFlowQuery<'_> },
    { DropFilter, DropQuery<'_> },
    { SwitchFilter, SwitchQuery<'_> },
);
