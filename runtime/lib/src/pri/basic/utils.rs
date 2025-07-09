use super::Ref;

pub(super) trait RefManager {
    type Ref;
    type Value;

    fn push(&mut self, value: Self::Value) -> Self::Ref;

    fn take(&mut self, reference: Self::Ref) -> Self::Value;

    fn get_mut(&mut self, reference: Self::Ref) -> &mut Self::Value;
}

pub(super) type DefaultRefManager<V> = CircularBufferRefManager<V>;

#[deprecated(note = "Inefficient")]
pub(super) struct ArrayListRefManager<V> {
    counter: Ref,
    refs: Vec<(Ref, V)>,
}

impl<T> ArrayListRefManager<T> {
    pub const fn new() -> Self {
        Self {
            counter: 0,
            refs: Vec::new(),
        }
    }
}

impl<V> RefManager for ArrayListRefManager<V> {
    type Ref = Ref;
    type Value = V;

    fn push(&mut self, value: V) -> Ref {
        self.counter += 1;
        self.refs.push((self.counter, value));
        self.counter
    }

    fn take(&mut self, reference: Ref) -> V {
        let index = self.find(reference).unwrap();
        self.refs.swap_remove(index).1
    }

    fn get_mut(&mut self, reference: Ref) -> &mut V {
        let index = self.find(reference).unwrap();
        &mut self.refs[index].1
    }
}

impl<V> ArrayListRefManager<V> {
    fn find(&self, reference: Ref) -> Option<usize> {
        self.refs.iter().position(|(r, _)| r.eq(&reference))
    }
}

/* NOTE: As references are just temporary handles, a very small number of them
 * should be alive at any given time. Probably the largest numbers happen for
 * functions calls and aggregate value creation. Unless in exceptional programs,
 * running out of buffer means a bug in the instrumentation. */
const DEFAULT_BUFFER_SIZE: usize = 1024;

pub(super) struct CircularBufferRefManager<V> {
    buffer: [Option<V>; DEFAULT_BUFFER_SIZE],
    index: usize,
}

impl<V> CircularBufferRefManager<V> {
    pub const fn new() -> Self {
        Self {
            buffer: [const { None }; DEFAULT_BUFFER_SIZE],
            index: 0,
        }
    }
}

impl<V: core::fmt::Debug> RefManager for CircularBufferRefManager<V> {
    type Ref = Ref;
    type Value = V;

    fn push(&mut self, value: V) -> Ref {
        let coerce = |index| index % self.buffer.len();

        let mut counter = 0;
        while self.buffer[coerce(self.index + counter)].is_some() {
            counter += 1;
            debug_assert!(counter < self.buffer.len(), "Full reference buffer",);
        }

        let reference = coerce(self.index + counter) as Ref;
        self.index = self.index + counter + 1;
        self.buffer[reference as usize].replace(value);
        reference
    }

    fn take(&mut self, reference: Ref) -> V {
        self.buffer[reference as usize].take().unwrap()
    }

    fn get_mut(&mut self, reference: Ref) -> &mut V {
        self.buffer[reference as usize].as_mut().unwrap()
    }
}
