use super::types::{ArchivedDefId, ArchivedInstanceKindId, DefId, InstanceKindId};

impl From<&DefId> for ArchivedDefId {
    fn from(value: &DefId) -> Self {
        (*value).into()
    }
}

impl From<DefId> for ArchivedDefId {
    fn from(value: DefId) -> Self {
        ArchivedDefId(value.0.into(), value.1.into())
    }
}

impl From<&InstanceKindId> for ArchivedInstanceKindId {
    fn from(value: &InstanceKindId) -> Self {
        (*value).into()
    }
}

impl From<InstanceKindId> for ArchivedInstanceKindId {
    fn from(value: InstanceKindId) -> Self {
        ArchivedInstanceKindId(value.0.into(), value.1.into())
    }
}
