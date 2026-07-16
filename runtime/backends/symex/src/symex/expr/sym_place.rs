/// Represents a selection over a `target` using an `index`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Select<I, V> {
    pub index: I,
    pub target: SelectTarget<V, Self>,
}

/// Represents the possible targets of a selection, which can be an array of
/// values or recursively the result of another selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectTarget<V, S> {
    Array(Vec<V>),
    Nested(Box<S>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolicReadTree<I, V> {
    /// A selection over a set of possible values using a symbolic index.
    /// # Remarks
    /// The root of a complete is expected to be from this variant.
    SymRead(Select<I, Self>),
    /// An array of possible values.
    /// # Remarks
    /// This variant is expected to appear as intermediate nodes as possible values for a selection.
    Array(Vec<Self>),
    /// A single value.
    /// # Remarks
    /// This variant is expected to appear as leaf nodes.
    Single(V),
}

impl<I, V> From<Select<I, Self>> for SymbolicReadTree<I, V> {
    fn from(select: Select<I, Self>) -> Self {
        SymbolicReadTree::SymRead(select)
    }
}
