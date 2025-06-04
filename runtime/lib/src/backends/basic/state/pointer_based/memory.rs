use core::num::NonZero;
use std::{
    fmt::{self, Debug, Display},
    ops::Range,
};

use common::pri::TypeSize;

use crate::utils::RangeIntersection;

pub(super) type Address = common::types::RawAddress;

mod high {
    use common::{log_debug, log_warn, pri::TypeId, types::PointerOffset};

    use crate::backends::basic::{
        expr::SymValueRef,
        implication::{Antecedents, Precondition, PreconditionConstraints},
    };

    use low::Memory;

    use super::*;

    type ValueObject = (SymValueRef, TypeId);
    /* Granularity of precondition is the MIR assignment level, i.e., preconditions
     * for the fields of composite types are merged. */
    /* Don't wee need the type id?
     * As we approximate the preconditions for the objects, it works correctly,
     * even if the types are not the same.
     * If retrieving the containing object, the preconditions of the fields work
     * for the object as well. And if it is the field being accessed, we approximate
     * with the preconditions of the parent. */
    type PreconditionObject = Antecedents;

    #[derive(Default)]
    pub(crate) struct MemoryGate {
        value_mem: Memory<ValueObject>,
        precondition_mem: Memory<PreconditionObject>,
    }

    impl MemoryGate {
        #[tracing::instrument(level = "debug", skip(self), ret)]
        pub(crate) fn read_values<'a, 'b>(
            &'a self,
            addr: Address,
            size: TypeSize,
        ) -> Vec<(Address, &'a SymValueRef, &'a TypeId)> {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZST symbolic values are not expected.
                return Default::default();
            };

            let range = range_from(addr, size);

            let mut values = Vec::new();
            self.value_mem.apply_in_range(
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
                |addr, _, (value, type_id)| {
                    values.push((*addr, value, type_id));
                },
            );
            values
        }

        #[tracing::instrument(level = "debug", skip(self), ret)]
        pub(crate) fn read_preconditions<'a, 'b>(
            &'a self,
            addr: Address,
            size: TypeSize,
        ) -> Vec<(PointerOffset, NonZero<TypeSize>, PreconditionObject)> {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZST instances are constants, thus no precondition.
                return Default::default();
            };

            let range = range_from(addr, size);

            let mut preconditions = Vec::new();
            self.precondition_mem.apply_in_range(
                &range,
                |addr, size, _| {
                    let obj_range = range_from(*addr, *size);
                    // Overlapping but not contained
                    if !RangeIntersection::contains(&range, &obj_range)
                        && !RangeIntersection::contains(&obj_range, &range)
                    {
                        log_warn!(
                            concat!(
                                "Object boundary/alignment assumption does not hold. ",
                                "An overlapping object's preconditions fetched. Skipping. ",
                                "This is probably due to missed deallocations. ",
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
                |p_addr, p_size, precondition| {
                    let offset = if *p_addr > range.start {
                        offset_of(range.start, *p_addr)
                    } else {
                        0
                    };
                    let size = (*p_size).min(NonZero::new(size.get() - offset).unwrap());
                    preconditions.push((offset, size, precondition.clone()));
                },
            );

            preconditions
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn erase_values(&mut self, addr: Address, size: TypeSize) {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZSTs are not stored to be erased
                return;
            };

            let range = range_from(addr, size);

            self.value_mem.drain_range_and_apply(
                &range,
                |_, _, _| {
                    let obj_range = range_from(addr, size);
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
            );
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn erase_preconditions_in(&mut self, addr: Address, size: TypeSize) {
            self.inner_erase_preconditions_in(addr, size, false);
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn replace_values(
            &mut self,
            addr: Address,
            size: TypeSize,
            values: Vec<(PointerOffset, TypeSize, SymValueRef, TypeId)>,
        ) {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZSTs are not stored.
                debug_assert!(values.is_empty());
                return;
            };

            let range = range_from(addr, size);

            self.erase_values(addr, size.get());

            let mut cursor = self.value_mem.after_or_at_mut(&addr);
            for (offset, size, value, type_id) in values {
                let value_addr = addr.wrapping_byte_add(offset as usize);
                let value_size = NonZero::new(size).expect("ZST symbolic value observed");
                let value_range = range_from(value_addr, value_size);
                debug_assert!(
                    RangeIntersection::contains(&range, &value_range),
                    "Value out of bound {:?} {:?}",
                    range,
                    value_range,
                );
                log_debug!("Inserting: {:?} = ({}, {})", value_range, &value, &type_id);
                cursor
                    .insert_before(value_addr, (value_size, (value, type_id)))
                    .unwrap();
            }
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn replace_preconditions(
            &mut self,
            addr: Address,
            size: TypeSize,
            precondition: Precondition,
        ) {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZST instances are constant.
                // debug_assert_matches!(precondition, Precondition::True);
                return;
            };

            self.inner_erase_preconditions_in(addr, size.get(), true);

            let Some(constraints) = precondition.take_constraints() else {
                return;
            };

            let base_addr = addr;
            let whole_size = size;
            let mut insert = |offset, size: NonZero<TypeSize>, precondition| {
                let addr = base_addr.wrapping_byte_add(offset as usize);
                debug_assert!(addr as u64 + size.get() <= base_addr as u64 + whole_size.get());
                self.precondition_mem
                    .after_or_at_mut(&addr)
                    .insert_before(addr, (size, precondition))
                    .unwrap()
            };
            match constraints {
                PreconditionConstraints::Whole(constraints) => insert(0, size, constraints),
                PreconditionConstraints::Refined(items) => items
                    .get()
                    .into_iter()
                    .for_each(|(offset, size, precondition)| insert(offset, size, precondition)),
            }
        }

        #[tracing::instrument(level = "debug", skip(self))]
        fn inner_erase_preconditions_in(
            &mut self,
            addr: Address,
            size: TypeSize,
            expect_container: bool,
        ) {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZSTs are constants and don't have preconditions
                return;
            };

            let range = range_from(addr, size);
            let mut container = false;
            let mut last_erased = None;
            self.precondition_mem.drain_range_and_apply(
                &range,
                |addr, size, _| {
                    let obj_range = range_from(*addr, *size);
                    if obj_range == range {
                        true
                    } else if RangeIntersection::contains(&range, &obj_range) {
                        true
                    }
                    // Container
                    else if RangeIntersection::contains(&obj_range, &range) {
                        if !expect_container {
                            log_warn!(
                                concat!(
                                    "Object boundary/alignment assumption does not hold. ",
                                    "A contained object is being erased before the container. ",
                                    "This is probably due to missed deallocations. ",
                                    "Breaking the preconditions of the container object anyway. ",
                                    "Query: {:?}, Object: {:?}"
                                ),
                                range,
                                obj_range,
                            );
                        }
                        container = true;
                        true
                    }
                    // Overlapping but not contained
                    else {
                        log_warn!(
                            concat!(
                                "Object boundary/alignment assumption does not hold. ",
                                "An overlapping object / symbolic container found. ",
                                "This is probably due to missed deallocations. ",
                                "Erasing the preconditions of the overlapping object. ",
                                "Query: {:?}, Object: {:?}"
                            ),
                            range,
                            obj_range,
                        );
                        true
                    }
                },
                |addr, size, precondition| {
                    last_erased = Some(((addr, size), precondition));
                },
            );

            // FIXME: We can do better with cursors, but let's keep it simple for now.
            if container {
                self.split_erase_preconditions_and_insert(last_erased.unwrap(), range);
            }
        }

        #[tracing::instrument(level = "debug", skip(self, obj_precondition))]
        fn split_erase_preconditions_and_insert(
            &mut self,
            ((obj_addr, obj_size), obj_precondition): (
                (*const (), NonZero<TypeSize>),
                PreconditionObject,
            ),
            range: Range<Address>,
        ) {
            let mut insert = |part: (*const (), *const ())| {
                if let Some(size) = NonZero::new(
                    unsafe { part.1.byte_offset_from(part.0) }
                        .try_into()
                        .unwrap(),
                ) {
                    self.precondition_mem
                        .after_or_at_mut(&part.0)
                        .insert_before(part.0, (size, obj_precondition.clone()))
                        .unwrap();
                }
            };

            let obj_range = range_from(obj_addr, obj_size);
            let first_part = (obj_range.start, range.start);
            let second_part = (range.end, obj_range.end);
            if first_part.0 < first_part.1 {
                insert(first_part);
            }
            if second_part.0 < second_part.1 {
                insert(second_part);
            }
        }
    }

    #[inline]
    fn offset_of(base_addr: Address, inner_addr: Address) -> PointerOffset {
        unsafe { inner_addr.byte_offset_from(base_addr) }
            .try_into()
            .unwrap()
    }
}
pub(super) use high::MemoryGate;

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
    #[derive(Debug)]
    pub(super) struct Memory<O>(BTreeMap<Address, MemoryElement<O>>);

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
