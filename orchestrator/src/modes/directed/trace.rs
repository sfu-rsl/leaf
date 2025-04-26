use std::{fs, io::Read, path::Path};

use serde::Deserialize;
use serde_json::Value;

use common::{pri::BasicBlockLocation, types::trace::Constraint, z3::serdes::SmtLibExpr};

type Expression = SmtLibExpr;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SwitchStep<V = Expression, C = Expression> {
    pub location: BasicBlockLocation,
    pub decision: Option<Constraint<V, C>>,
}

pub(crate) type SwitchTrace = Vec<SwitchStep>;

pub(crate) trait TraceReader {
    type Trace;

    fn read_trace(&mut self) -> Self::Trace;
}

pub(crate) fn new_default_trace_reader(
    full_trace_path: &Path,
    sym_trace_path: &Path,
) -> impl TraceReader<Trace = SwitchTrace> {
    json::JsonListTraceReader {
        full_trace_reader: fs::OpenOptions::new()
            .read(true)
            .open(full_trace_path)
            .expect("Failed to open full trace file for reading"),
        sym_trace_reader: fs::OpenOptions::new()
            .read(true)
            .open(sym_trace_path)
            .expect("Failed to open sym trace file for reading"),
    }
}

mod json {
    use super::*;

    pub(super) struct JsonListTraceReader<R> {
        pub full_trace_reader: R,
        pub sym_trace_reader: R,
    }

    impl<R> JsonListTraceReader<R> {
        fn read_full_trace(&mut self) -> Vec<(usize, BasicBlockLocation)>
        where
            R: Read,
        {
            let mut raw_value: Value = serde_json::from_reader(&mut self.full_trace_reader)
                .expect("Failed to read the full trace");
            const FORMAT_MSG: &str = "The trace was not in the expected format: [ { step: { value: { body: [#, #], index: #}, index: # } ... }* ]";
            let raw_trace = raw_value.as_array_mut().expect(FORMAT_MSG);
            raw_trace
                .into_iter()
                .map(|item| {
                    let step = item
                        .as_object_mut()
                        .and_then(|o| o.get_mut("step"))
                        .expect(FORMAT_MSG)
                        .as_object_mut()
                        .expect(FORMAT_MSG);
                    let location = step.remove("value").expect(FORMAT_MSG);
                    let location =
                        serde_json::from_value::<BasicBlockLocation>(location).expect(FORMAT_MSG);
                    let index = step
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .expect(FORMAT_MSG);
                    (index as usize, location)
                })
                .collect()
        }

        fn read_sym_trace(
            &mut self,
        ) -> Vec<(
            usize,
            (BasicBlockLocation, Constraint<Expression, Expression>),
        )>
        where
            R: Read,
        {
            let mut raw_value: Value = serde_json::from_reader(&mut self.sym_trace_reader)
                .expect("Failed to read the full trace");
            const FORMAT_MSG: &str = "The trace was not in the expected format: [ { step: { value: { body: [#, #], index: #}, index: # }, constraint: { discr: ttt, kind: ttt} }* ]";
            let raw_trace = raw_value.as_array_mut().expect(FORMAT_MSG);
            raw_trace
                .into_iter()
                .map(|item| {
                    let item = item.as_object_mut().expect(FORMAT_MSG);
                    let constraint = item.remove("constraint").expect(FORMAT_MSG);
                    let step = item
                        .get_mut("step")
                        .and_then(|v| v.as_object_mut())
                        .expect(FORMAT_MSG);
                    let location = step.remove("value").expect(FORMAT_MSG);
                    let location =
                        serde_json::from_value::<BasicBlockLocation>(location).expect(FORMAT_MSG);
                    let constraint = serde_json::from_value(constraint).expect(FORMAT_MSG);
                    let index = step
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .expect(FORMAT_MSG);
                    (index as usize, (location, constraint))
                })
                .collect()
        }
    }

    impl<R: Read> TraceReader for JsonListTraceReader<R> {
        type Trace = SwitchTrace;

        fn read_trace(&mut self) -> Self::Trace {
            let mut sym_iter = self.read_sym_trace().into_iter().peekable();
            self.read_full_trace()
                .into_iter()
                .map(|(index, loc)| SwitchStep {
                    location: loc,
                    decision: sym_iter
                        .peek()
                        .is_some_and(|(i, _)| *i == index)
                        .then(|| sym_iter.next().unwrap().1)
                        .inspect(|(sym_loc, _)| debug_assert_eq!(&loc, sym_loc))
                        .map(|(_, c)| c),
                })
                .collect()
        }
    }
}

impl core::fmt::Display for SwitchStep {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "@{}: {}",
            self.location,
            self.decision
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or("<Conc>".to_owned())
        )
    }
}
