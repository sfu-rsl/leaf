/// Temporarily-referencing system meant for passing complex data from instructions to runtime.
/// Example: An argument to a function call is an operand which can be a copy of a place corresponding to an element of an array.
/// Passing this complex structure is broken into multiple calls connected by temporary references.
/// The implementation should have minimal overhead.

pub(crate) trait RefManager {
    type Ref = common::pri::Ref;
    type Value;

    fn push(&mut self, value: Self::Value) -> Self::Ref;

    fn take(&mut self, reference: Self::Ref) -> Self::Value;

    fn get_mut(&mut self, reference: Self::Ref) -> &mut Self::Value;
}

mod circular {
    use super::RefManager;

    /* NOTE: As references are just temporary handles, a very small number of them
     * should be alive at any given time. Probably the largest numbers happen for
     * functions calls and aggregate value creation. Unless in exceptional programs,
     * running out of buffer means a bug in the instrumentation.
     * Keeping it small may help caching.
     */
    const BUF_SIZE: usize = 1024;

    pub struct CircularBufferRefManager<V> {
        buffer: [Option<V>; BUF_SIZE],
        next: usize,
    }

    impl<V> Default for CircularBufferRefManager<V> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<V> CircularBufferRefManager<V> {
        pub const fn new() -> Self {
            Self {
                buffer: [const { None }; BUF_SIZE],
                next: 0,
            }
        }
    }

    impl<V> RefManager for CircularBufferRefManager<V> {
        type Ref = u64;
        type Value = V;

        fn push(&mut self, value: V) -> Self::Ref {
            let coerce = |i| i % BUF_SIZE;
            let find_free = || {
                let mut reference = self.next;
                if core::hint::unlikely(self.buffer[reference].is_some()) {
                    let mut counter = 1;
                    loop {
                        reference = coerce(reference + 1);
                        if self.buffer[reference].is_none() {
                            break;
                        }
                        counter += 1;
                        debug_assert!(counter < BUF_SIZE, "Full reference buffer",);
                    }
                }

                reference
            };

            unsafe {
                core::hint::assert_unchecked(self.next < BUF_SIZE);
            }
            let reference = find_free();
            self.next = coerce(reference + 1);
            self.buffer[reference as usize].replace(value);
            reference as Self::Ref
        }

        fn take(&mut self, reference: Self::Ref) -> V {
            self.buffer[reference as usize].take().unwrap()
        }

        fn get_mut(&mut self, reference: Self::Ref) -> &mut V {
            self.buffer[reference as usize].as_mut().unwrap()
        }
    }
}

mod noop {
    use super::RefManager;

    #[derive(Default)]
    pub struct NoOpRefManager<V>(V);

    impl<V> NoOpRefManager<V> {
        pub(crate) const fn new(value: V) -> Self {
            Self(value)
        }
    }

    impl<V: Copy> RefManager for NoOpRefManager<V> {
        type Value = V;

        fn push(&mut self, _value: V) -> Self::Ref {
            0
        }

        fn take(&mut self, _reference: Self::Ref) -> V {
            self.0
        }

        fn get_mut(&mut self, _reference: Self::Ref) -> &mut V {
            &mut self.0
        }
    }
}

pub(crate) type DefaultRefManager<V> = circular::CircularBufferRefManager<V>;
pub(crate) use noop::NoOpRefManager;
