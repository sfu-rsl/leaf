use std::collections::HashSet;

use delegate::delegate;

use common::log_warn;
use common::type_info::TypeDatabase;

use leaf_runtime::abs::TypeId;

use super::MdTypeProvider;

type SetImpl<T> = HashSet<T>;
pub(super) struct MdSanTypeDb<D: 'static> {
    inner: D,
    md_types: SetImpl<TypeId>,
    md_container_types: SetImpl<TypeId>,
}

impl<D: 'static> MdSanTypeDb<D> {
    pub(super) fn new(inner: D) -> Self
    where
        D: TypeDatabase<'static>,
    {
        const KEY_MD_TYPES: &str = "md_types";
        const KEY_MD_CONTAINER_TYPES: &str = "md_container_types";

        let md_types: SetImpl<TypeId> = inner
            .get_metadata(KEY_MD_TYPES)
            .and_then(|value| value.as_array())
            .map(|array| {
                array
                    .iter()
                    .map(|item| TypeId::new(item.as_number().unwrap()).unwrap())
                    .collect()
            })
            .unwrap_or_default();
        let md_container_types: SetImpl<TypeId> = inner
            .get_metadata(KEY_MD_CONTAINER_TYPES)
            .and_then(|value| value.as_array())
            .map(|array| {
                array
                    .iter()
                    .map(|item| TypeId::new(item.as_number().unwrap()).unwrap())
                    .collect()
            })
            .unwrap_or_default();

        if md_container_types.len() < md_types.len() || md_types.len() == 0 {
            log_warn!(
                "MdSan type database metadata is missing or incomplete. This may lead to incorrect analysis results."
            );
        }
        Self {
            inner,
            md_types,
            md_container_types,
        }
    }
}

impl<T> MdTypeProvider for MdSanTypeDb<T> {
    fn is_md_container_type(&self, type_id: &TypeId) -> bool {
        self.md_container_types.contains(type_id)
    }

    fn is_md_type(&self, type_id: &TypeId) -> bool {
        self.md_types.contains(type_id)
    }
}

impl<D: TypeDatabase<'static> + 'static> TypeDatabase<'static> for MdSanTypeDb<D> {
    delegate! {
        to self.inner {
            fn opt_get_type(&self, key: &TypeId) -> Option<&'static common::type_info::TypeInfo>;

            fn core_types(&self) -> &common::type_info::CoreTypes<TypeId>;

            fn get_metadata(&self, key: &str) -> Option<&common::type_info::MetadataValue>;
        }
    }
}
