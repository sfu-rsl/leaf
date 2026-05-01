use std::{assert_matches::debug_assert_matches, borrow::Borrow, ops::Deref};

use common::{
    pri::BasicBlockIndex,
    types::{InstanceKindId, trace::ExeTraceRecord},
};

use crate::utils::{HasIndex, Indexed, RefView};

use super::{Step, backend};
use backend::{ExeTraceStorage, GenericTraceQuerier, SymExConstraint, SymExExeTraceRecorder};

type TraceView<T> = RefView<Vec<T>>;

// Let's avoid complexity by introducing generics, but rely on type aliases for the actual types.

type SymExExeTraceRecord = <SymExExeTraceRecorder as ExeTraceStorage>::Record;
type SymExConstraintTraceStep = Indexed<Step>;

struct DefaultTraceQuerier
where
    SymExExeTraceRecord: HasIndex + ExeRecord,
    SymExConstraintTraceStep: HasIndex + Borrow<Step>,
{
    pub exe_records: TraceView<SymExExeTraceRecord>,
    pub constraint_steps: TraceView<SymExConstraintTraceStep>,
    pub constraints: TraceView<SymExConstraint>,
    pub sym_dependent_step_indices: TraceView<usize>,
}

pub(crate) fn default_trace_querier(
    exe_records: TraceView<SymExExeTraceRecord>,
    constraint_steps: TraceView<SymExConstraintTraceStep>,
    constraints: TraceView<SymExConstraint>,
    sym_dependent_step_indices: TraceView<usize>,
) -> impl super::super::alias::TraceQuerier {
    DefaultTraceQuerier {
        exe_records,
        constraint_steps,
        constraints,
        sym_dependent_step_indices,
    }
}

/// The set of properties used for querying.
trait ExeRecord {
    fn is_call(&self, callee: InstanceKindId) -> bool;

    fn is_in(&self, body_id: InstanceKindId) -> bool;

    fn depth(&self) -> usize;
}

impl GenericTraceQuerier for DefaultTraceQuerier {
    type Record = SymExExeTraceRecord;
    type Constraint = SymExConstraint;

    fn any_sym_dependent_in_current_call(&self, body_id: InstanceKindId) -> bool {
        let records = self.exe_records.borrow();
        let Some((diverged_count, current_depth)) = records
            .iter()
            .rev()
            .enumerate()
            // Except with external calls in between, the last record is always in the current body
            .filter(|(_, r)| r.is_in(body_id))
            .map(|(i, r)| (i, r.depth()))
            .next()
        else {
            return false;
        };
        let sym_dependent_indices = self.sym_dependent_step_indices.borrow();
        let Some(latest_sym_dependent) = sym_dependent_indices.last().copied() else {
            return false;
        };
        let records = records.iter();
        let latest_records_in_body = records
            .rev()
            .skip(diverged_count)
            .take_while(|r| r.depth() >= current_depth)
            .filter(|r| r.depth() == current_depth);
        let latest_records_before_latest =
            latest_records_in_body.skip_while(|r| r.index() > latest_sym_dependent);

        itertools::merge_join_by(
            latest_records_before_latest,
            sym_dependent_indices.iter().rev(),
            |r, i| r.index().cmp(i).reverse(),
        )
        .any(|either| either.is_both())
    }

    fn find_map_in_current_func<'a, T>(
        &'a self,
        body_id: InstanceKindId,
        mut f: impl FnMut(BasicBlockIndex, &Self::Constraint) -> Option<T>,
    ) -> Option<(
        impl AsRef<BasicBlockIndex> + HasIndex + AsRef<Self::Constraint>,
        T,
    )> {
        // (Indexed<...>s, Constraints) -> (Indexed<Constraint>s)
        let constraint_steps = self.constraint_steps.borrow();
        let constraint_indices = constraint_steps.iter().map(HasIndex::index);
        let constraints = self.constraints.borrow();
        let constraints = constraints.iter();
        let indexed_constraints = constraint_indices
            .zip(constraints)
            .map(|(index, c)| Indexed { value: c, index })
            .enumerate();

        let records = self.exe_records.borrow();
        let (diverged_count, current_depth) = records
            .iter()
            .rev()
            .enumerate()
            .filter(|(_, r)| r.is_in(body_id))
            .map(|(i, r)| (i, r.depth()))
            .next()?;
        let records = records.iter().enumerate();
        let records_with_constraints = itertools::merge_join_by(
            records.rev(),
            indexed_constraints.rev(),
            |(_, r), (_, c)| r.index().cmp(&c.index()).reverse(),
        )
        .map(|either| {
            let (r, c) = either.left_and_right();
            (r.expect("Records must be the complete set"), c)
        });

        let latest_records_in_body = records_with_constraints
            .skip(diverged_count)
            .take_while(|((_, r), _)| r.depth() >= current_depth)
            .filter(|((_, r), _)| r.depth() == current_depth);

        let record_of_interest = latest_records_in_body
            .filter_map(|(r, opt_c)| opt_c.map(|c| (r, c)))
            .filter(|((_, r), _)| r.is_in(body_id))
            .find_map(|pair @ ((_, r), (_, c))| {
                f(*helpers::branch_rec_block_index(r), &c).map(|v| (pair, v))
            });

        record_of_interest.map(|(((r_i, _), (c_i, _)), v)| (self.create_view(r_i, c_i), v))
    }
}

impl DefaultTraceQuerier {
    fn create_view<'a>(
        &'a self,
        record_index: usize,
        constraint_index: usize,
    ) -> impl AsRef<BasicBlockIndex> + HasIndex + AsRef<SymExConstraint> + 'a {
        let view = QuerierStepView {
            record: self.exe_records.borrow_map(move |rs| &rs[record_index]),
            constraint: self.constraints.borrow_map(move |cs| &cs[constraint_index]),
        };
        debug_assert_matches!(view.record.value, ExeTraceRecord::Branch(..));
        view
    }
}

mod helpers {
    use common::{
        pri::{BasicBlockIndex, BasicBlockLocation},
        types::trace::BranchRecord,
    };

    use super::*;

    pub(super) struct QuerierStepView<R, C> {
        pub record: R,
        pub constraint: C,
    }

    impl<R, C> AsRef<BasicBlockIndex> for QuerierStepView<R, C>
    where
        R: Deref<Target = SymExExeTraceRecord>,
    {
        fn as_ref(&self) -> &BasicBlockIndex {
            branch_rec_block_index(self.record.deref())
        }
    }

    impl<R, C> HasIndex for QuerierStepView<R, C>
    where
        R: Deref<Target = SymExExeTraceRecord>,
    {
        fn index(&self) -> usize {
            self.record.index
        }
    }

    impl<R, C> AsRef<SymExConstraint> for QuerierStepView<R, C>
    where
        C: Deref<Target = SymExConstraint>,
    {
        fn as_ref(&self) -> &SymExConstraint {
            self.constraint.deref()
        }
    }

    impl ExeRecord for SymExExeTraceRecord {
        fn is_call(&self, callee: InstanceKindId) -> bool {
            match self.borrow() {
                ExeTraceRecord::Call { to, .. } if to.eq(&callee) => true,
                _ => false,
            }
        }

        fn is_in(&self, body_id: InstanceKindId) -> bool {
            match self.borrow() {
                ExeTraceRecord::Call { to, .. } => to,
                ExeTraceRecord::Return { to, .. } => to,
                ExeTraceRecord::Branch(BranchRecord {
                    location: BasicBlockLocation { body, .. },
                    ..
                }) => body,
            }
            .eq(&body_id)
        }

        fn depth(&self) -> usize {
            self.depth
        }
    }

    pub(super) fn branch_rec_block_index(record: &SymExExeTraceRecord) -> &BasicBlockIndex {
        match record.borrow() {
            ExeTraceRecord::Branch(BranchRecord {
                location: BasicBlockLocation { ref index, .. },
                ..
            }) => index,
            _ => unreachable!("Expected a branch record"),
        }
    }
}
use helpers::QuerierStepView;
