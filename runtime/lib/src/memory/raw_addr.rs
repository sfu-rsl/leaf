use core::{
    fmt::{self, Debug, Display},
    num::NonZero,
    ops::Range,
};

use common::{pri::TypeSize, types::RawAddress};

use crate::utils::RangeIntersection;

type Address = RawAddress;

mod high {
    use common::{log_debug, log_warn, types::PointerOffset};

    use super::low::Memory;

    use super::*;

    pub(crate) struct MemoryGate<O> {
        mem: Memory<O>,
    }

    impl<O> Default for MemoryGate<O> {
        fn default() -> Self {
            Self {
                mem: Default::default(),
            }
        }
    }

    impl<O: Debug> MemoryGate<O> {
        #[tracing::instrument(level = "debug", skip(self), ret)]
        pub(crate) fn read_objects<'a, 'b>(
            &'a self,
            addr: Address,
            size: NonZero<TypeSize>,
        ) -> Vec<((Address, NonZero<TypeSize>), &'a O)> {
            let range = range_from(addr, size);

            let mut objs = Vec::new();
            self.mem.apply_in_range(
                &range,
                |addr, size, _| {
                    let obj_range = range_from(*addr, *size);
                    // Overlapping but not contained
                    if !RangeIntersection::contains(&range, &obj_range) {
                        log_warn!(
                            concat!(
                                "Object boundary/alignment assumption does not hold. ",
                                "An overlapping object / symbolic container found. ",
                                "This is probably due to missed deallocations. ",
                                "Skipping value retrieval. ",
                                "Query: {:?}, Object: {:?}"
                            ),
                            range,
                            obj_range,
                        );
                        false
                    } else {
                        true
                    }
                },
                |addr, obj_size, obj| {
                    objs.push(((*addr, *obj_size), obj));
                },
            );
            objs
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn erase_objects(&mut self, addr: Address, size: NonZero<TypeSize>) -> usize {
            let range = range_from(addr, size);

            self.mem.drain_range_and_apply(
                &range,
                |addr, size, _| {
                    let obj_range = range_from(*addr, *size);
                    // Overlapping but not contained
                    if !RangeIntersection::contains(&range, &obj_range) {
                        log_warn!(
                            concat!(
                                "Object boundary/alignment assumption does not hold. ",
                                "An overlapping object / symbolic container found. ",
                                "This is probably due to missed deallocations. ",
                                "Erasing the overlapping object. ",
                                "Query: {:?}, Object: {:?}"
                            ),
                            range,
                            obj_range,
                        );
                    }
                    true
                },
                |_, _, _| {},
            )
        }

        /// # Panics
        /// If `values` are not ordered by offset.
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn replace_objects(
            &mut self,
            addr: Address,
            size: NonZero<TypeSize>,
            objs: Vec<((PointerOffset, NonZero<TypeSize>), O)>,
        ) {
            let range = range_from(addr, size);

            self.erase_objects(addr, size);

            let mut cursor = self.mem.after_or_at_mut(&addr);
            for ((offset, obj_size), obj) in objs {
                let value_addr = addr.wrapping_byte_add(offset as usize);
                let value_size = obj_size;
                let value_range = range_from(value_addr, value_size);
                debug_assert!(
                    RangeIntersection::contains(&range, &value_range),
                    "Value out of bound {:?} {:?}",
                    range,
                    value_range,
                );
                log_debug!("Inserting: {:?} = ({:?})", value_range, &obj);
                cursor
                    .insert_before(value_addr, (value_size, obj))
                    .expect("Unordered symbolic values passed");
            }
        }

        pub(crate) fn get_containing(&self, addr: Address) -> Option<&O> {
            if let Some((obj_addr, (obj_size, obj))) = self.mem.before_or_at(&addr).peek_prev() {
                let obj_range = range_from(*obj_addr, *obj_size);
                if obj_range.contains(&addr) {
                    return Some(obj);
                }
            }
            None
        }

        pub(crate) fn update_containing(&mut self, addr: Address, value: O) -> Option<O> {
            if let Some((obj_addr, (obj_size, obj))) = self.mem.before_or_at_mut(&addr).peek_prev()
            {
                let obj_range = range_from(*obj_addr, *obj_size);
                if obj_range.contains(&addr) {
                    return Some(core::mem::replace(obj, value));
                }
            }
            None
        }
    }
}

mod low {
    use std::{
        borrow::Borrow,
        collections::{
            BTreeMap,
            btree_map::{Cursor, CursorMut},
        },
        ops::Bound,
    };

    use super::*;

    type MemoryElement<O> = (NonZero<TypeSize>, O);

    /// Sparse raw-addressed object memory.
    ///
    /// Stores non-zero-sized objects keyed by their start address. Each entry keeps
    /// the object size and payload, while range helpers provide overlap-aware read,
    /// mutate, and drain operations.
    #[derive(Debug)]
    pub(crate) struct Memory<O>(BTreeMap<Address, MemoryElement<O>>);

    impl<O> Default for Memory<O> {
        fn default() -> Self {
            Self(Default::default())
        }
    }

    impl<O> Memory<O> {
        /// # Remarks
        /// The `prev` node of the returned cursor is the last entry with an address
        /// less than or equal to `addr`.
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn before_or_at(&self, addr: &Address) -> Cursor<'_, Address, MemoryElement<O>> {
            self.0.upper_bound(Bound::Included(addr))
        }

        /// # Remarks
        /// The `prev` node of the returned cursor is the last entry with an address
        /// less than or equal to `addr`.
        // FIXME: Guard against insertion of overlapping elements
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn before_or_at_mut<'a>(
            &'a mut self,
            addr: &Address,
        ) -> CursorMut<'a, Address, MemoryElement<O>> {
            self.0.upper_bound_mut(Bound::Included(addr))
        }

        /// # Remarks
        /// The `next` node of the returned cursor is greater than or equal to `addr`.
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn after_or_at(&self, addr: &Address) -> Cursor<'_, Address, MemoryElement<O>> {
            self.0.lower_bound(Bound::Included(addr))
        }

        /// # Remarks
        /// The `next` node of the returned cursor is greater than or equal to `addr`.
        // FIXME: Guard against insertion of overlapping elements
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn after_or_at_mut(
            &mut self,
            addr: &Address,
        ) -> CursorMut<'_, Address, MemoryElement<O>> {
            self.0.lower_bound_mut(Bound::Included(addr))
        }

        /// # Remarks
        /// Calls the function for all objects overlapping with the range.
        #[tracing::instrument(level = "debug", skip_all, fields(range = ?range.borrow()))]
        pub(crate) fn apply_in_range<'a>(
            &'a self,
            range: impl Borrow<Range<Address>>,
            mut predicate: impl FnMut(&'a Address, &'a NonZero<TypeSize>, &'a O) -> bool,
            mut f: impl FnMut(&'a Address, &'a NonZero<TypeSize>, &'a O),
        ) {
            let range = range.borrow();
            let mut cursor = self.before_or_at(&range.start);
            if let Some((addr, (size, obj))) = cursor
                .peek_prev()
                .filter(|(addr, (size, _))| range_from(**addr, *size).is_overlapping(range))
                .filter(|(addr, (size, obj))| predicate(addr, size, obj))
            {
                f(addr, size, obj)
            }
            while let Some((addr, (size, obj))) = cursor.peek_next() {
                if !range.contains(addr) {
                    break;
                }

                if predicate(addr, size, obj) {
                    f(addr, size, obj);
                }

                cursor.next();
            }
        }

        /// # Remarks
        /// Calls the function for all objects overlapping with the range.
        #[tracing::instrument(level = "debug", skip_all, fields(range = ?range.borrow()), ret)]
        pub(crate) fn apply_in_range_mut<'a>(
            &'a mut self,
            range: impl Borrow<Range<Address>>,
            mut predicate: impl FnMut(&'_ Address, &'_ NonZero<TypeSize>, &'_ O) -> bool,
            mut f: impl FnMut(&'_ Address, &'_ mut NonZero<TypeSize>, &'_ mut O),
        ) -> usize {
            let mut matched = 0;

            let range = range.borrow();
            let mut cursor = self.before_or_at_mut(&range.start);
            if let Some((addr, (size, obj))) = cursor
                .peek_prev()
                .filter(|(addr, (size, _))| range_from(**addr, *size).is_overlapping(range))
                .filter(|(addr, (size, obj))| predicate(addr, size, obj))
            {
                f(addr, size, obj);
                matched += 1;
            }
            while let Some((addr, (size, obj))) = cursor.peek_next() {
                if !range.contains(addr) {
                    break;
                }

                if predicate(addr, size, obj) {
                    f(addr, size, obj);
                    matched += 1;
                }

                cursor.next();
            }

            matched
        }

        /// # Remarks
        /// The `next` node of the given cursor is in the range.
        #[tracing::instrument(level = "debug", skip_all, fields(range = ?range.borrow()), ret)]
        pub(crate) fn drain_range_and_apply<'a>(
            &'a mut self,
            range: impl Borrow<Range<Address>>,
            mut predicate: impl FnMut(&'_ Address, &'_ NonZero<TypeSize>, &'_ O) -> bool,
            mut f: impl FnMut(Address, NonZero<TypeSize>, O),
        ) -> usize {
            let mut matched = 0;

            let range = range.borrow();
            let mut cursor = self.before_or_at_mut(&range.start);
            if cursor
                .peek_prev()
                .filter(|(addr, (size, _))| range_from(**addr, *size).is_overlapping(range))
                .is_some_and(|(addr, (size, obj))| predicate(addr, size, obj))
            {
                let entry = cursor.remove_prev().unwrap();
                f(entry.0, entry.1.0, entry.1.1);
                matched += 1;
            }

            while let Some((addr, (size, obj))) = cursor.peek_next() {
                if !range.contains(addr) {
                    break;
                }

                if predicate(addr, size, obj) {
                    let entry = cursor.remove_next().unwrap();
                    f(entry.0, entry.1.0, entry.1.1);
                    matched += 1;
                } else {
                    cursor.next();
                }
            }

            matched
        }
    }

    impl<O: Debug> Display for Memory<O> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            writeln!(f, "{{")?;
            for (addr, (size, obj)) in self.0.iter() {
                writeln!(
                    f,
                    "[{:p}..{:p}] -> ({:?})",
                    addr,
                    addr.wrapping_byte_add(size.get() as usize),
                    obj
                )?;
            }
            writeln!(f, "}}")?;
            Ok(())
        }
    }
}

fn range_from(addr: Address, size: NonZero<TypeSize>) -> Range<Address> {
    addr..addr.wrapping_byte_add(size.get() as usize)
}

pub(crate) use high::MemoryGate;
pub(crate) use low::Memory;
