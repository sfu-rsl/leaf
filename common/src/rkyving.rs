use crate::types::ArchivedDefId;

use super::types::DefId;

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
