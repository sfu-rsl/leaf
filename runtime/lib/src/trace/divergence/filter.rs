use crate::utils::alias::RRef;

use super::Constraint;

pub(crate) trait DivergenceFilter<S, V, C> {
    fn should_find(&mut self, trace: &[S], constraints: &[Constraint<V, C>]) -> bool;
}

impl<S, V, C> DivergenceFilter<S, V, C> for Box<dyn DivergenceFilter<S, V, C> + '_> {
    fn should_find(&mut self, trace: &[S], constraints: &[Constraint<V, C>]) -> bool {
        self.as_mut().should_find(trace, constraints)
    }
}

impl<S, V, C, F: DivergenceFilter<S, V, C>> DivergenceFilter<S, V, C> for RRef<F> {
    fn should_find(&mut self, trace: &[S], constraints: &[Constraint<V, C>]) -> bool {
        self.borrow_mut().should_find(trace, constraints)
    }
}

pub(crate) fn all<'a, S: 'a, V: 'a, C: 'a>(
    filters: Vec<Box<dyn DivergenceFilter<S, V, C> + 'a>>,
) -> impl DivergenceFilter<S, V, C> + 'a {
    struct All<'a, S, V, C>(Vec<Box<dyn DivergenceFilter<S, V, C> + 'a>>);

    impl<'a, S, V, C> DivergenceFilter<S, V, C> for All<'a, S, V, C> {
        fn should_find(&mut self, trace: &[S], constraints: &[Constraint<V, C>]) -> bool {
            self.0.iter_mut().all(|f| f.should_find(trace, constraints))
        }
    }

    All(filters)
}

pub(crate) trait DivergenceFilterExt<S, V, C> {
    fn and_then<F>(self, f: F) -> impl DivergenceFilter<S, V, C>
    where
        Self: Sized,
        F: DivergenceFilter<S, V, C>;
}

impl<S, V, C, T: DivergenceFilter<S, V, C>> DivergenceFilterExt<S, V, C> for T {
    fn and_then<F>(self, f: F) -> impl DivergenceFilter<S, V, C>
    where
        F: DivergenceFilter<S, V, C>,
    {
        struct And<T, F> {
            first: T,
            second: F,
        }

        impl<S, V, C, T, F> DivergenceFilter<S, V, C> for And<T, F>
        where
            T: DivergenceFilter<S, V, C>,
            F: DivergenceFilter<S, V, C>,
        {
            fn should_find<'a, 'b>(
                &'a mut self,
                trace: &'b [S],
                constraints: &'b [Constraint<V, C>],
            ) -> bool {
                self.first.should_find(trace, constraints)
                    && self.second.should_find(trace, constraints)
            }
        }

        And {
            first: self,
            second: f,
        }
    }
}
