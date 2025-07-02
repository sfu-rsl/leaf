use core::borrow::Borrow;

use derive_more as dm;
use serde::{Serialize, Serializer};
use serde_json::Serializer as JsonSerializer;

use common::{directed::RawCaseValue, log_debug, types::trace::BranchRecord};

use crate::{
    abs::{
        BasicBlockLocation, ConstraintKind, FuncDef,
        backend::{CallTraceRecorder, DecisionTraceRecorder, PhasedCallTraceRecorder},
    },
    utils::{HasIndex, Indexed, RefView, alias::RRef, file::JsonLinesFormatter},
};

use super::backend;
use backend::{ConstValue, ExeTraceRecorder, ExeTraceStorage, config::OutputConfig};

type ExeTraceRecord = crate::abs::ExeTraceRecord<ConstValue>;

#[derive(Debug, Serialize, dm::Deref)]
pub(crate) struct Record {
    #[deref]
    record: Indexed<crate::abs::ExeTraceRecord<ConstValue>>,
    pub(super) depth: usize,
}

impl HasIndex for Record {
    fn index(&self) -> usize {
        self.record.index
    }
}

impl Borrow<crate::abs::ExeTraceRecord<ConstValue>> for Record {
    fn borrow(&self) -> &crate::abs::ExeTraceRecord<ConstValue> {
        &self.record.value
    }
}

pub(crate) struct BasicExeTraceRecorder {
    counter: usize,
    records: RRef<Vec<Record>>,
    stack: Vec<BasicBlockLocation<FuncDef>>,
    last_ret_point: Option<BasicBlockLocation<FuncDef>>,
    serializer: Option<JsonSerializer<std::fs::File, JsonLinesFormatter>>,
}

impl BasicExeTraceRecorder {
    fn new(config: Option<&OutputConfig>) -> Self {
        let file = config
            .and_then(|c| match c {
                OutputConfig::File(file) => Some(file),
            })
            .filter(|c| matches!(c.format, crate::utils::file::FileFormat::JsonLines))
            .map(|c| {
                c.open_or_create_single("exe_trace", true)
                    .unwrap_or_else(|e| panic!("Could not create file for trace recording: {e}"))
            });

        Self {
            serializer: file
                .map(|f| JsonSerializer::with_formatter(f, JsonLinesFormatter::default())),
            counter: 0,
            records: Default::default(),
            stack: Default::default(),
            last_ret_point: Default::default(),
        }
    }
}

pub(crate) fn create_trace_recorder(config: Option<&OutputConfig>) -> BasicExeTraceRecorder
where
    BasicExeTraceRecorder: ExeTraceRecorder,
{
    BasicExeTraceRecorder::new(config)
}

impl CallTraceRecorder for BasicExeTraceRecorder {
    fn notify_call(
        &mut self,
        call_site: BasicBlockLocation<FuncDef>,
        entered_func: FuncDef,
        broken: bool,
    ) {
        self.notify_step(ExeTraceRecord::Call {
            from: call_site.into(),
            to: entered_func.body_id,
            broken,
        });
    }

    fn notify_return(
        &mut self,
        ret_point: BasicBlockLocation<FuncDef>,
        caller_func: FuncDef,
        broken: bool,
    ) {
        self.notify_step(ExeTraceRecord::Return {
            from: ret_point.into(),
            to: caller_func.body_id,
            broken,
        });
    }
}

impl PhasedCallTraceRecorder for BasicExeTraceRecorder {
    #[tracing::instrument(level = "debug", skip(self))]
    fn start_call(&mut self, call_site: BasicBlockLocation<FuncDef>) {
        *self
            .stack
            .last_mut()
            .inspect(|l| debug_assert_eq!(&l.body, &call_site.body))
            .expect("Inconsistent stack info") = call_site;
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn finish_call(&mut self, entered_func: FuncDef, broken: bool) {
        let call_site = self.stack.last().copied();
        self.stack.push(BasicBlockLocation {
            body: entered_func,
            index: 0,
        });
        let Some(call_site) = call_site else {
            if !broken {
                panic!(
                    "Last call site is expected when not broken, current: {}",
                    entered_func
                )
            } else {
                return;
            }
        };

        self.notify_call(call_site, entered_func, broken);
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn start_return(&mut self, ret_point: BasicBlockLocation<FuncDef>) {
        self.stack.pop().expect("Inconsistent stack info");
        self.last_ret_point = Some(ret_point);
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn finish_return(&mut self, broken: bool) -> BasicBlockLocation<FuncDef> {
        let call_site = self.stack.last().copied().expect("Inconsistent stack info");
        let Some(ret_point) = self.last_ret_point.take() else {
            if !broken {
                panic!(
                    "Last return point is expected when not broken, current: {}",
                    call_site
                )
            } else {
                return call_site;
            }
        };

        self.notify_return(ret_point, call_site.body, broken);
        call_site
    }
}

impl DecisionTraceRecorder for BasicExeTraceRecorder {
    type Case = ConstValue;

    fn notify_decision(
        &mut self,
        node_location: BasicBlockLocation,
        kind: &ConstraintKind<Self::Case>,
    ) -> usize {
        self.notify_step(ExeTraceRecord::Branch(BranchRecord {
            location: node_location,
            decision: kind.clone(),
        }))
    }
}

impl ExeTraceStorage for BasicExeTraceRecorder {
    type Record = Record;

    fn records(&self) -> RefView<Vec<Self::Record>> {
        self.records.clone().into()
    }
}

impl BasicExeTraceRecorder {
    #[tracing::instrument(level = "debug", skip(self), fields(index = self.counter + 1))]
    fn notify_step(&mut self, record: ExeTraceRecord) -> usize {
        let index = {
            let index = self.counter + 1;
            self.counter = index;
            index
        };
        self.records.borrow_mut().push(Record {
            record: Indexed {
                value: record,
                index,
            },
            depth: self.stack.len(),
        });
        self.append_last_to_file();
        index
    }

    fn append_last_to_file(&mut self) {
        let Some(serializer) = self.serializer.as_mut() else {
            return;
        };
        let _ = serialize_rec(self.records.as_ref().borrow().last().unwrap(), serializer)
            .inspect_err(|e| log_debug!("Failed to dump trace: {}", e));
    }
}

fn serialize_rec<S: Serializer>(record: &Record, serializer: S) -> Result<S::Ok, S::Error> {
    let Record {
        record: Indexed {
            ref value,
            ref index,
        },
        ..
    } = record;

    use crate::abs::ExeTraceRecord::*;
    match value {
        Branch(ref branch) => Indexed {
            value: Branch(to_raw_case(branch)),
            index: *index,
        }
        .serialize(serializer),
        _ => record.serialize(serializer),
    }
}

fn to_raw_case(branch: &BranchRecord<ConstValue>) -> BranchRecord<RawCaseValue> {
    BranchRecord {
        location: branch.location,
        decision: branch
            .decision
            .as_ref()
            .map(|c| c.try_to_bit_rep().unwrap()),
    }
}
