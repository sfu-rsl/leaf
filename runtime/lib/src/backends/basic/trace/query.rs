use std::{borrow::Borrow, ops::Deref};

use common::types::{InstanceKindId, trace::ExeTraceRecord};

use crate::{
    backends::basic::{ExeTraceStorage, TraceQuerier},
    utils::RefView,
};

type TraceView<T> = RefView<Vec<T>>;

use super::*;

// Let's avoid complexity by introducing generics, but rely on type aliases for the actual types.

type BasicExeTraceRecord = <record::BasicExeTraceRecorder as ExeTraceStorage>::Record;
type BasicConstraintTraceStep = Indexed<Step>;
type BasicConstraint = super::super::Constraint;

struct BasicTraceQuerier
where
    BasicExeTraceRecord: HasIndex + ExeRecord,
    BasicConstraintTraceStep: HasIndex + Borrow<Step>,
{
    pub exe_records: TraceView<BasicExeTraceRecord>,
    pub constraint_steps: TraceView<BasicConstraintTraceStep>,
    pub constraints: TraceView<BasicConstraint>,
}

pub(crate) fn default_trace_querier(
    exe_records: TraceView<BasicExeTraceRecord>,
    constraint_steps: TraceView<BasicConstraintTraceStep>,
    constraints: TraceView<BasicConstraint>,
) -> impl super::super::alias::BasicTraceQuerier {
    BasicTraceQuerier {
        exe_records,
        constraint_steps,
        constraints,
    }
}

/// The set of properties used for querying.
trait ExeRecord {
    fn is_call(&self, callee: InstanceKindId) -> bool;

    fn is_in(&self, body_id: InstanceKindId) -> bool;
}

impl TraceQuerier for BasicTraceQuerier {
    type Record = BasicExeTraceRecord;
    type Constraint = BasicConstraint;

    fn find_in_latest_call_of<'a>(
        &'a self,
        body_id: InstanceKindId,
        mut predicate: impl FnMut(&Self::Record, &Self::Constraint) -> bool,
    ) -> Option<impl AsRef<Self::Record> + AsRef<Self::Constraint>> {
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
            .take_while(|((_, r), _)| !r.is_call(body_id))
            .filter(|((_, r), _)| r.is_in(body_id));

        let record_of_interest = latest_records_in_body
            .filter_map(|(r, opt_c)| opt_c.map(|c| (r, c)))
            .find(|((_, r), (_, c))| predicate(r, c));

        record_of_interest.map(|((r_i, _), (c_i, _))| self.create_view(r_i, c_i))
    }
}

impl BasicTraceQuerier {
    fn create_view<'a>(
        &'a self,
        record_index: usize,
        constraint_index: usize,
    ) -> impl AsRef<BasicExeTraceRecord> + AsRef<BasicConstraint> + 'a {
        QuerierStepView {
            record: self.exe_records.borrow_map(move |rs| &rs[record_index]),
            constraint: self.constraints.borrow_map(move |cs| &cs[constraint_index]),
        }
    }
}

mod helpers {
    use super::*;

    pub(super) struct QuerierStepView<R, C> {
        pub record: R,
        pub constraint: C,
    }

    impl<R, C> AsRef<BasicExeTraceRecord> for QuerierStepView<R, C>
    where
        R: Deref<Target = BasicExeTraceRecord>,
    {
        fn as_ref(&self) -> &BasicExeTraceRecord {
            self.record.deref()
        }
    }

    impl<R, C> AsRef<BasicConstraint> for QuerierStepView<R, C>
    where
        C: Deref<Target = BasicConstraint>,
    {
        fn as_ref(&self) -> &BasicConstraint {
            self.constraint.deref()
        }
    }

    impl ExeRecord for <record::BasicExeTraceRecorder as ExeTraceStorage>::Record {
        fn is_call(&self, callee: InstanceKindId) -> bool {
            match self.borrow() {
                ExeTraceRecord::Call { to, .. } if to.eq(&callee) => true,
                _ => false,
            }
        }

        fn is_in(&self, body_id: InstanceKindId) -> bool {
            ExeTraceRecord::location(self.borrow()).body == body_id
        }
    }
}
use helpers::QuerierStepView;
