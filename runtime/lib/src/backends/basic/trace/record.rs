use serde::Serialize;
use serde_json::Serializer as JsonSerializer;

use common::log_debug;

use crate::{
    abs::{
        BasicBlockLocation, ConstraintKind, FuncDef,
        backend::{CallTraceRecorder, DecisionTraceRecorder, PhasedCallTraceRecorder},
    },
    utils::{Indexed, RefView, alias::RRef, file::JsonLinesFormatter},
};

use super::{StepCounter, backend};
use backend::{ConstValue, ExeTraceStorage, config::OutputConfig, ExeTraceRecorder};

type ExeTraceRecord = crate::abs::ExeTraceRecord<ConstValue>;

pub(crate) struct BasicExeTraceRecorder {
    pub(in super::super) counter: StepCounter,
    records: RRef<Vec<Indexed<ExeTraceRecord>>>,
    stack: Vec<FuncDef>,
    last_call_site: Option<BasicBlockLocation>,
    last_ret_point: Option<BasicBlockLocation>,
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
            counter: RRef::new(0.into()),
            records: Default::default(),
            stack: Default::default(),
            last_call_site: Default::default(),
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
    fn notify_call(&mut self, call_site: BasicBlockLocation, entered_func: FuncDef, broken: bool) {
        self.notify_step(ExeTraceRecord::Call {
            from: call_site,
            to: entered_func.body_id,
            broken,
        });
    }

    fn notify_return(&mut self, ret_point: BasicBlockLocation, caller_func: FuncDef, broken: bool) {
        self.notify_step(ExeTraceRecord::Return {
            from: ret_point,
            to: caller_func.body_id,
            broken,
        });
    }
}

impl PhasedCallTraceRecorder for BasicExeTraceRecorder {
    #[tracing::instrument(level = "debug", skip(self))]
    fn start_call(&mut self, call_site: BasicBlockLocation) {
        self.last_call_site = Some(call_site);
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn finish_call(&mut self, entered_func: FuncDef, broken: bool) {
        self.stack.push(entered_func);
        let Some(call_site) = self.last_call_site.take() else {
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
    fn start_return(&mut self, ret_point: BasicBlockLocation) {
        self.stack.pop().expect("Inconsistent stack info");
        self.last_ret_point = Some(ret_point);
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn finish_return(&mut self, broken: bool) {
        let current = self.stack.last().copied().expect("Inconsistent stack info");
        let Some(ret_point) = self.last_ret_point.take() else {
            if !broken {
                panic!(
                    "Last return point is expected when not broken, current: {}",
                    current
                )
            } else {
                return;
            }
        };

        self.notify_return(ret_point, current, broken);
    }
}

impl DecisionTraceRecorder for BasicExeTraceRecorder {
    type Case = ConstValue;

    fn notify_decision(
        &mut self,
        node_location: BasicBlockLocation,
        kind: &ConstraintKind<Self::Case>,
    ) {
        self.notify_step(ExeTraceRecord::Branch {
            location: node_location,
            kind: kind.clone(),
        });
    }
}

impl ExeTraceStorage for BasicExeTraceRecorder {
    type Record = Indexed<ExeTraceRecord>;

    fn records(&self) -> RefView<Vec<Self::Record>> {
        self.records.clone().into()
    }
}

impl BasicExeTraceRecorder {
    #[tracing::instrument(level = "debug", skip(self), fields(index = *self.counter.as_ref().borrow()))]
    fn notify_step(&mut self, record: ExeTraceRecord) {
        let counter = RRef::as_ref(&self.counter);
        *counter.borrow_mut() += 1;
        self.records.borrow_mut().push(Indexed {
            value: record,
            index: *counter.borrow(),
        });
        self.append_last_to_file();
    }

    fn append_last_to_file(&mut self) {
        let Some(serializer) = self.serializer.as_mut() else {
            return;
        };
        let _ = self
            .records
            .as_ref()
            .borrow()
            .last()
            .unwrap()
            .serialize(serializer)
            .inspect_err(|e| log_debug!("Failed to dump trace: {}", e));
    }
}
