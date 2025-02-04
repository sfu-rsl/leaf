use std::{
    collections::{
        BTreeMap,
        btree_map::{Cursor, CursorMut, Entry},
    },
    fmt::{self, Debug, Display},
    ops::{Bound, Range},
};

use common::types::RawAddress;

use super::MemoryObject;

pub(super) type Address = RawAddress;
#[derive(Debug, Default)]
pub(super) struct Memory(BTreeMap<Address, MemoryObject>);

impl Memory {
    /// # Remarks
    /// The `prev` node of the returned cursor is the last entry with an address
    /// less than or equal to `addr`.
    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) fn before_or_at(&self, addr: &Address) -> Cursor<'_, Address, MemoryObject> {
        self.0.upper_bound(Bound::Included(addr))
    }

    /// # Remarks
    /// The `next` node of the returned cursor is greater than or equal to `addr`.
    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) fn after_or_at(&self, addr: &Address) -> Cursor<'_, Address, MemoryObject> {
        self.0.lower_bound(Bound::Included(addr))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) fn after_or_at_mut(
        &mut self,
        addr: &Address,
    ) -> CursorMut<'_, Address, MemoryObject> {
        self.0.lower_bound_mut(Bound::Included(addr))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) fn entry_at(&mut self, addr: Address) -> Entry<'_, Address, MemoryObject> {
        self.0.entry(addr)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub(crate) fn remove_at(&mut self, addr: &Address) {
        self.0.remove(addr);
    }

    /// # Remarks
    /// The `next` node of the given cursor is in the range.
    #[tracing::instrument(level = "debug", skip(self, f))]
    pub(crate) fn apply_in_range(
        &self,
        range: Range<Address>,
        mut f: impl FnMut(&Address, &MemoryObject),
    ) {
        let mut cursor = self.after_or_at(&range.start);
        while let Some((addr, obj)) = cursor.peek_next() {
            if !range.contains(addr) {
                break;
            }
            f(addr, obj);
            cursor.next();
        }
    }

    /// # Remarks
    /// The `next` node of the given cursor is in the range.
    #[tracing::instrument(level = "debug", skip(self, f))]
    pub(crate) fn drain_range_and_apply(
        &mut self,
        range: Range<Address>,
        mut f: impl FnMut(Address, MemoryObject),
    ) {
        let mut cursor = self.after_or_at_mut(&range.start);
        while let Some((addr, _)) = cursor.peek_next() {
            if !range.contains(addr) {
                break;
            }
            let element = cursor.remove_next().unwrap();
            f(element.0, element.1);
        }
    }
}

impl Display for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{{")?;
        for (addr, (value, type_id)) in self.0.iter() {
            writeln!(f, "{:p} ->\t{} ({:?})", addr, value, type_id)?;
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}
