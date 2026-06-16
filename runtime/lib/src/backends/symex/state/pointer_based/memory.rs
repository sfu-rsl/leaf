use core::num::NonZero;
use std::ops::Range;

use common::pri::TypeSize;

use crate::utils::{RangeIntersection, byte_offset_from};

pub(super) type Address = common::types::RawAddress;

mod high {
    use common::{log_warn, pri::TypeId, types::PointerOffset};

    use crate::memory::raw_addr::{Memory, MemoryGate as SharedMemoryGate};

    use super::super::backend::{
        expr::SymValueRef,
        implication::{Antecedents, Precondition, PreconditionConstraints, PreconditionQuery},
    };
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
        value_mem: SharedMemoryGate<ValueObject>,
        // FIXME: migrate to the generic memory gate
        #[cfg(feature = "implicit_flow")]
        precondition_mem: Memory<PreconditionObject>,
    }

    impl MemoryGate {
        #[tracing::instrument(level = "debug", skip(self), ret)]
        #[inline]
        pub(crate) fn read_values<'a, 'b>(
            &'a self,
            addr: Address,
            size: TypeSize,
        ) -> Vec<((Address, NonZero<TypeSize>), &'a (SymValueRef, TypeId))> {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZSTs are not stored, thus no values.
                return Default::default();
            };

            self.value_mem.read_objects(addr, size)
        }

        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn erase_values(&mut self, addr: Address, size: TypeSize) {
            let Some(size) = NonZero::<TypeSize>::new(size) else {
                // ZSTs are not stored
                return;
            };
            let _count = self.value_mem.erase_objects(addr, size);
        }

        /// # Panics
        /// If `values` are not ordered by offset.
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn replace_values(
            &mut self,
            addr: Address,
            size: NonZero<TypeSize>,
            values: Vec<((PointerOffset, NonZero<TypeSize>), (SymValueRef, TypeId))>,
        ) {
            self.value_mem.replace_objects(addr, size, values);
        }

        #[cfg(feature = "implicit_flow")]
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
                                "An overlapping object's preconditions fetched. ",
                                "This is probably due to missed deallocations. ",
                                "Skipping precondition retrieval. ",
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
                    let offset =
                        core::intrinsics::saturating_sub(p_addr.addr(), range.start.addr());
                    let end = p_addr
                        .wrapping_byte_add(p_size.get() as usize)
                        .min(range.end);
                    let size = byte_offset_from(end, range.start.wrapping_byte_add(offset));
                    let size = NonZero::new(size as TypeSize).unwrap();
                    preconditions.push((offset as PointerOffset, size, precondition.clone()));
                },
            );

            preconditions
        }

        #[cfg(feature = "implicit_flow")]
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn erase_preconditions_in(&mut self, addr: Address, size: TypeSize) {
            self.inner_erase_preconditions_in(addr, size, false);
        }

        #[cfg(feature = "implicit_flow")]
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
                    // We don't currently guarantee order, so we stick with filter instead of take_while.
                    .filter(|(offset, _, _)| *offset < whole_size.get())
                    .for_each(|(offset, size, precondition)| {
                        let size = size.min(NonZero::new(whole_size.get() - offset).unwrap());
                        insert(offset, size, precondition)
                    }),
            }
        }

        #[cfg(feature = "implicit_flow")]
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

        #[cfg(feature = "implicit_flow")]
        #[tracing::instrument(level = "debug", skip(self, obj_precondition))]
        fn split_erase_preconditions_and_insert(
            &mut self,
            ((obj_addr, obj_size), obj_precondition): (
                (*const (), NonZero<TypeSize>),
                PreconditionObject,
            ),
            range: Range<Address>,
        ) {
            let mut insert = |r: (*const (), *const ())| {
                if let Some(size) = NonZero::new(byte_offset_from(r.1, r.0) as TypeSize) {
                    self.precondition_mem
                        .after_or_at_mut(&r.0)
                        .insert_before(r.0, (size, obj_precondition.clone()))
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
}
pub(super) use high::MemoryGate;

fn range_from(addr: Address, size: NonZero<TypeSize>) -> Range<Address> {
    addr..addr.wrapping_byte_add(size.get() as usize)
}
