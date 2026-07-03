use derive_more as dm;
use serde::Deserialize;

use crate::config::rules::{InclusionRules, LogicFormula, PatternMatch};

pub(crate) type InstrumentationRules = InclusionRules<EntityFilter>;

macro_rules! filter_struct {
    // Filter with optional field(s) followed by loc field, with optional attributes.
    ($(#[$attr:meta])* $name:ident { $($field:tt)* }) => {
        $(#[$attr])*
        #[derive(Debug, Clone, Deserialize)]
        pub(crate) struct $name {
            $($field)*
            #[serde(flatten)]
            pub(crate) loc: LogicFormula<EntityLocationFilter>,
        }
    };
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "entity")]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntityFilter {
    #[serde(alias = "body")]
    WholeBody(WholeBodyFilter),
    #[serde(alias = "dyn_def")]
    MethodDynDefinition(MethodDynDefinitionFilter),
    PlaceInfo(PlaceInfoFilter),
    OperandKind(OperandKindFilter),
    #[serde(alias = "const_type")]
    ConstantType(ConstantTypeFilter),
    Assignment(AssignmentFilter),
    AssignmentInfo(AssignmentFilter),
    #[serde(alias = "storage_lifetime", alias = "lifetime_marker")]
    StorageLifetimeMarker(StorageLifetimeMarkerFilter),
    CallFlow(CallFlowFilter),
    Drop(DropFilter),
    Switch(SwitchFilter),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntityLocationFilter {
    #[serde(alias = "def_path")]
    DefPathMatch(PatternMatch),
    DefId(DefIdFilter),
    Crate(CrateFilter),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CrateFilter {
    #[serde(alias = "is_external")]
    Externality(bool),
    Name(PatternMatch),
}

pub(crate) type DefIdFilter = common::types::DefId;

filter_struct! {
    #[derive(dm::From)]
    WholeBodyFilter {}
}

filter_struct!(MethodDynDefinitionFilter {});

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaceInfoFilter {
    #[serde(alias = "struct")]
    Structure(PlaceStructureFilter),
    #[serde(alias = "addr")]
    Address(PlaceAddressFilter),
    #[serde(alias = "ty")]
    Type(PlaceTypeFilter),
}

filter_struct! { PlaceStructureFilter {
    pub(crate) piece: LogicFormula<PlaceStructurePiece>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaceStructurePiece {
    Local,
    Deref,
    Field,
    Index,
    ConstantIndex,
    Subslice,
    Downcast,
    OpaqueCast,
    UnwrapUnsafeBinder,
}

filter_struct! { PlaceAddressFilter {} }

filter_struct! { PlaceTypeFilter {} }

filter_struct! { OperandKindFilter {
    pub(crate) kind: LogicFormula<OperandKind>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperandKind {
    Copy,
    Move,
    #[serde(alias = "const")]
    Constant,
}

filter_struct! { ConstantTypeFilter {
    #[serde(alias = "type")]
    pub(crate) ty: LogicFormula<ConstantType>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstantType {
    Bool,
    Char,
    Int,
    Float,
    Str,
    ByteStr,
    Ptr,
    Zst,
}

filter_struct! { AssignmentFilter {
    pub(crate) kind: LogicFormula<AssignmentKind>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssignmentKind {
    Use,
    Repeat,
    Ref,
    ThreadLocalRef,
    RawPtr,
    Cast,
    BinaryOp,
    UnaryOp,
    Discriminant,
    Aggregate,
    WrapUnsafeBinder,
    IntrinsicBinaryOp,
    IntrinsicUnaryOp,
    IntrinsicMemoryOp,
    AtomicBinaryOp,
    AtomicMemoryOp,
}

filter_struct! { StorageLifetimeMarkerFilter {
    pub(crate) kind: LogicFormula<StorageLifetimeMarkerKind>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageLifetimeMarkerKind {
    Live,
    Dead,
}

filter_struct! { CallFlowFilter {
    pub(crate) kind: LogicFormula<CallFlowPartKind>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallFlowPartKind {
    CallControl,
    #[serde(alias = "call_addr")]
    CallAddress,
    CallInput,
    #[serde(alias = "func_addr")]
    FunctionAddress,
    #[serde(alias = "func_data")]
    FunctionData,
}

filter_struct! { DropFilter {
    pub(crate) kind: LogicFormula<DropPartKind>,
} }

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DropPartKind {
    CallControl,
    #[serde(alias = "call_addr")]
    CallAddress,
    CallInput,
}

filter_struct! { SwitchFilter {
    pub(crate) kind: LogicFormula<SwitchPartKind>,
}}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SwitchPartKind {
    Control,
    Data,
}
