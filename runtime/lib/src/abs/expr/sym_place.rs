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

/// Resolves a symbolic read to a selection.
/// It may recursively resolve the possible values of the selection.
pub(crate) trait SymbolicReadResolver<I> {
    type SymValue<'a>;
    type PossibleValue<'a>;

    fn resolve<'a>(&self, sym_value: Self::SymValue<'a>) -> Select<I, Self::PossibleValue<'a>>;
}

// FIXME: Should be replaced with separate expand/leaves functions.
pub(crate) enum SymbolicReadTreeLeafMutator<'m, I, V> {
    Mutator(&'m mut dyn FnMut(&mut V)),
    Replacer(&'m mut dyn FnMut(&mut V) -> SymbolicReadTree<I, V>),
}
use SymbolicReadTreeLeafMutator::*;

impl<I, V> Select<I, SymbolicReadTree<I, V>> {
    pub(crate) fn first_leaf(&self) -> &V {
        match &self.target {
            SelectTarget::Array(values) => values.first().unwrap().first_leaf(),
            SelectTarget::Nested(box nested) => nested.first_leaf(),
        }
    }

    pub(crate) fn map<'m, J, W>(
        &self,
        f_index: impl Fn(&I) -> J,
        mut f: impl FnMut(&SymbolicReadTree<I, V>) -> SymbolicReadTree<J, W>,
    ) -> Select<J, SymbolicReadTree<J, W>> {
        Select {
            index: f_index(&self.index),
            target: match &self.target {
                SelectTarget::Array(values) => {
                    SelectTarget::Array(values.iter().map(|v| f(v)).collect())
                }
                SelectTarget::Nested(box nested) => {
                    SelectTarget::Nested(Box::new(nested.map(f_index, f)))
                }
            },
        }
    }

    #[inline]
    pub(crate) fn map_expand<'m, J, W>(
        &self,
        f_index: impl Fn(&I) -> J,
        mut f: impl FnMut(&V) -> SymbolicReadTree<J, W>,
    ) -> Select<J, SymbolicReadTree<J, W>> {
        self.map(&f_index, |v| v.map_expand(&f_index, &mut f))
    }

    #[inline]
    pub(crate) fn map_leaves<'m, J, W>(
        &self,
        f_index: impl Fn(&I) -> J,
        mut f: impl FnMut(&V) -> W,
    ) -> Select<J, SymbolicReadTree<J, W>>
    where
        I: Clone,
    {
        self.map(&f_index, |v| v.map_leaves(&f_index, &mut f))
    }

    #[inline]
    pub(crate) fn mutate_leaves<'m>(
        &mut self,
        mut f: SymbolicReadTreeLeafMutator<'m, I, V>,
        expander: impl Fn(&V) -> SymbolicReadTree<I, V> + Copy,
    ) {
        self.internal_mutate_leaves(1, &mut f, expander)
    }

    /// # Arguments
    /// - `expected_dim`: The expected dimension of the array this is selecting on.
    /// Effectively this corresponds to one more than the number of `Select` values wrapping this.
    fn internal_mutate_leaves<'m>(
        &mut self,
        array_dim: usize,
        f: &mut SymbolicReadTreeLeafMutator<'m, I, V>,
        expander: impl Fn(&V) -> SymbolicReadTree<I, V> + Copy,
    ) {
        match &mut self.target {
            SelectTarget::Array(ref mut values) => {
                values
                    .iter_mut()
                    .for_each(|v| v.internal_mutate_leaves(array_dim - 1, f, expander));
            }
            SelectTarget::Nested(box nested) => {
                Self::internal_mutate_leaves(nested, array_dim + 1, f, expander)
            }
        }
    }
}

impl<I, V> SymbolicReadTree<I, V> {
    pub(crate) fn first_leaf(&self) -> &V {
        match self {
            SymbolicReadTree::SymRead(select) => select.first_leaf(),
            SymbolicReadTree::Array(values) => values.first().unwrap().first_leaf(),
            SymbolicReadTree::Single(value) => value,
        }
    }

    // NOTE: Dynamic objects used here are meant for breaking the recursion in type resolution.

    pub(crate) fn map<'m, J, W>(
        &self,
        f_index: &dyn Fn(&I) -> J,
        f: &mut dyn FnMut(&Self) -> SymbolicReadTree<J, W>,
    ) -> SymbolicReadTree<J, W> {
        match self {
            SymbolicReadTree::SymRead(select) => SymbolicReadTree::SymRead(select.map(f_index, f)),
            SymbolicReadTree::Array(values) => {
                let values = values.iter().map(|v| v.map(f_index, f)).collect();
                SymbolicReadTree::Array(values)
            }
            SymbolicReadTree::Single(..) => f(self),
        }
    }

    #[inline]
    pub(crate) fn map_expand<'m, J, W>(
        &self,
        f_index: &dyn Fn(&I) -> J,
        mut f: &mut dyn FnMut(&V) -> SymbolicReadTree<J, W>,
    ) -> SymbolicReadTree<J, W> {
        self.map(f_index, &mut |v| match v {
            SymbolicReadTree::SymRead(select) => {
                SymbolicReadTree::SymRead(select.map_expand(f_index, &mut f))
            }
            SymbolicReadTree::Array(values) => {
                let values = values.iter().map(|v| v.map_expand(f_index, f)).collect();
                SymbolicReadTree::Array(values)
            }
            SymbolicReadTree::Single(value) => f(value),
        })
    }

    #[inline]
    pub(crate) fn map_leaves<'m, J, W>(
        &self,
        f_index: impl (Fn(&I) -> J),
        mut f: impl FnMut(&V) -> W,
    ) -> SymbolicReadTree<J, W>
    where
        I: Clone,
    {
        self.map_expand(&f_index, &mut |v| SymbolicReadTree::Single(f(v)))
    }

    pub(crate) fn mutate_leaves<'m>(
        &mut self,
        mut f: SymbolicReadTreeLeafMutator<'m, I, V>,
        expander: impl Fn(&V) -> Self + Copy,
    ) {
        match self {
            Self::SymRead(select) => select.mutate_leaves(f, expander),
            Self::Array(values) => values
                .iter_mut()
                .for_each(|v| v.internal_mutate_leaves(0, &mut f, &expander)),
            Self::Single(value) => match f {
                Mutator(f) => f(value),
                Replacer(f) => *self = f(value),
            },
        }
    }

    /// Applies the given function on the leaves (single value(s)) of this tree.
    /// - Expands the value if the expected dimension is more than one.
    /// - Recurses over subtrees.
    /// - Iterates over the arrays.
    ///
    /// # Arguments
    /// - `dim`: The expected dimension of the value(s).
    ///   Effectively this corresponds to one less than the number of `Select` values wrapping this.
    /// - `f`: The function to apply on the value(s).
    /// - `expander`: Used to expand the single values if they appear in the traversal
    ///   (before reaching the desired dimension).
    fn internal_mutate_leaves<'m>(
        &mut self,
        dim: usize,
        f: &mut SymbolicReadTreeLeafMutator<'m, I, V>,
        expander: impl Fn(&V) -> Self + Copy,
    ) {
        // Expand if expected.
        if dim > 0 {
            if let Self::Single(single) = self {
                *self = expander(single);
            }
        }

        match self {
            Self::SymRead(select) => select.internal_mutate_leaves(dim + 1, f, expander),
            Self::Array(values) => values
                .iter_mut()
                .for_each(|v| v.internal_mutate_leaves(dim - 1, f, expander)),
            Self::Single(value) => match f {
                Mutator(f) => f(value),
                Replacer(f) => *self = f(value),
            },
        }
    }
}
