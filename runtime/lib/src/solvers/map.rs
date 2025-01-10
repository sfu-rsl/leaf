use core::hash::Hash;

use crate::abs::backend::{Model, SolveResult};

use super::Solver;

/// Maps the model found by a solver.
pub(crate) struct MappedSolver<F, MTo, S: Solver> {
    inner: S,
    f: F,
    _phantom: core::marker::PhantomData<MTo>,
}

impl<F, MTo, S: Solver> Solver for MappedSolver<F, MTo, S>
where
    F: FnMut(S::Model) -> MTo,
{
    type Value = S::Value;
    type Case = S::Case;
    type Model = MTo;

    fn check<'b>(
        &mut self,
        constraints: impl Iterator<Item = &'b crate::abs::Constraint<Self::Value, Self::Case>>,
    ) -> SolveResult<Self::Model>
    where
        Self: 'b,
    {
        self.inner.check(constraints).map(&mut self.f)
    }
}

impl<MFrom> SolveResult<MFrom> {
    fn map<MTo, F>(self, f: F) -> SolveResult<MTo>
    where
        F: FnOnce(MFrom) -> MTo,
    {
        match self {
            SolveResult::Sat(model) => SolveResult::Sat(f(model)),
            SolveResult::Unsat => SolveResult::Unsat,
            SolveResult::Unknown => SolveResult::Unknown,
        }
    }
}

pub(crate) trait SolverExt: Solver {
    fn map<MTo>(
        self,
        f: impl FnMut(Self::Model) -> MTo,
    ) -> impl Solver<Value = Self::Value, Case = Self::Case, Model = MTo>;

    fn map_answers<I: Eq + Hash, AFrom, ATo>(
        self,
        mut f: impl FnMut(AFrom) -> ATo,
    ) -> impl Solver<Value = Self::Value, Case = Self::Case, Model = Model<I, ATo>>
    where
        Self: Solver<Model = Model<I, AFrom>> + Sized,
    {
        self.map(move |model| model.into_iter().map(|(id, ans)| (id, f(ans))).collect())
    }
}
impl<S: Solver> SolverExt for S {
    fn map<MTo>(
        self,
        f: impl FnMut(S::Model) -> MTo,
    ) -> impl Solver<Value = S::Value, Case = S::Case, Model = MTo> {
        MappedSolver {
            inner: self,
            f,
            _phantom: core::marker::PhantomData,
        }
    }
}
