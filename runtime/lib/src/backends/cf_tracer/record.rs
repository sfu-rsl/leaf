use common::{log_warn, pri::BasicBlockIndex};
use tracing::{Span, event, span};
use valuable::Valuable;

use crate::abs::{
    BasicBlockLocation, Constant, ConstraintKind, FuncDef,
    backend::{DecisionTraceRecorder, PhasedCallTraceRecorder},
};

use super::tracing_i::{LEVEL, TARGET};

pub(super) struct Recorder {
    stack: Vec<StackedData>,
    ephemeral: EphemeralData,
}

struct StackedData {
    current: FuncDef,
    last_call_site: Option<BasicBlockIndex>,
    span: Span,
}

struct EphemeralData {
    last_ret_point: Option<BasicBlockLocation<FuncDef>>,
}

macro_rules! body_id_to_str {
    ($body:expr) => {
        &format!("{}:{}({})", $body.1.0, $body.1.1, $body.0)
    };
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            ephemeral: EphemeralData {
                last_ret_point: None,
            },
        }
    }
}

impl Recorder {
    fn current_span(&self) -> &Span {
        static NONE_SPAN: Span = Span::none();
        &self
            .stack
            .last()
            .map(|s| &s.span)
            .unwrap_or_else(|| &NONE_SPAN)
    }

    #[inline]
    fn handle_maybe_unfinished_return(
        &mut self,
        last_ret_point: Option<BasicBlockLocation<FuncDef>>,
        current_location: BasicBlockLocation<FuncDef>,
    ) {
        // NOTE: This happens when an external function calls internal ones.
        if let Some(unfinished) = last_ret_point {
            core::hint::cold_path();

            event!(
                name: EVENT_TRANSFER,
                target: TARGET,
                parent: self.current_span(),
                LEVEL,
                { FIELD_FROM_BODY } = body_id_to_str!(unfinished.body.body_id),
                { FIELD_FROM_BLOCK } = unfinished.index,
                { FIELD_TO_BODY } = body_id_to_str!(current_location.body.body_id),
                { FIELD_TO_BLOCK } = current_location.index,
                { FIELD_BROKEN } = true,
            );
        }
    }

    fn ensure_in_current_body(&self, mut location: BasicBlockLocation) -> BasicBlockLocation {
        let current = if let Some(current) = self.stack.last().map(|s| s.current) {
            current.body_id
        } else {
            core::hint::cold_path();
            return location;
        };

        if cfg!(debug_assertions) {
            if location.body != current {
                log_warn!(
                    "Unexpected location in trace: {:?}, expected: {:?}",
                    location,
                    current
                );
            }
        }

        location.body = current;
        location
    }
}

const SPAN_FUNCTION: &str = "func";

const FIELD_BODY: &str = "body";
const FIELD_ADDRESS: &str = "address";
const FIELD_BROKEN: &str = "broken";

const FIELD_FROM_BODY: &str = "from_body";
const FIELD_FROM_BLOCK: &str = "from_block";
const FIELD_TO_BODY: &str = "to_body";
const FIELD_TO_BLOCK: &str = "to_block";
const FIELD_LOCATION_BLOCK: &str = "block_index";
const FIELD_DECISION_KIND: &str = "kind";

const EVENT_TRANSFER_START: &str = "transfer_start";
const EVENT_TRANSFER: &str = "transfer";
const EVENT_DECISION: &str = "decision";

impl PhasedCallTraceRecorder for Recorder {
    fn start_call(&mut self, call_site: BasicBlockLocation<FuncDef>) {
        let last_ret_point = self.ephemeral.last_ret_point.take();
        self.handle_maybe_unfinished_return(last_ret_point, call_site);

        let top = self.stack.last_mut().expect("Inconsistent stack info");
        debug_assert_eq!(top.current, call_site.body);
        top.last_call_site = Some(call_site.index);

        event!(
            name: EVENT_TRANSFER_START,
            target: TARGET,
            parent: self.current_span(),
            LEVEL,
            { FIELD_FROM_BODY } = body_id_to_str!(call_site.body.body_id),
            { FIELD_FROM_BLOCK } = call_site.index,
        );
    }

    fn finish_call(&mut self, entered_func: FuncDef, broken: bool) {
        let call_site = self.stack.last().and_then(|s| {
            s.last_call_site.map(|index| BasicBlockLocation {
                body: s.current,
                index,
            })
        });
        self.stack.push(StackedData {
            current: entered_func,
            last_call_site: None,
            span: span!(
                target: TARGET,
                parent: self.current_span(),
                LEVEL,
                SPAN_FUNCTION,
                { FIELD_BODY } = body_id_to_str!(entered_func.body_id),
                { FIELD_ADDRESS } = entered_func.static_addr as usize,
            ),
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

        event!(
            name: EVENT_TRANSFER,
            target: TARGET,
            parent: self.current_span(),
            LEVEL,
            { FIELD_FROM_BODY } = body_id_to_str!(call_site.body.body_id),
            { FIELD_FROM_BLOCK } = call_site.index,
            { FIELD_TO_BODY } = body_id_to_str!(entered_func.body_id),
            { FIELD_TO_BLOCK } = 0,
            { FIELD_BROKEN } = broken,
        );
    }

    fn start_return(&mut self, ret_point: BasicBlockLocation<FuncDef>) {
        self.stack.pop().expect("Inconsistent stack info");
        let unfinished = self.ephemeral.last_ret_point.replace(ret_point);
        self.handle_maybe_unfinished_return(unfinished, ret_point);

        event!(
            name: EVENT_TRANSFER_START,
            target: TARGET,
            parent: self.current_span(),
            LEVEL,
            { FIELD_FROM_BODY } = body_id_to_str!(ret_point.body.body_id),
            { FIELD_FROM_BLOCK } = ret_point.index,
        );
    }

    fn finish_return(&mut self, broken: bool) -> FuncDef {
        let caller = self
            .stack
            .last()
            .map(|s| BasicBlockLocation {
                body: s.current,
                index: s.last_call_site.expect("Inconsistent stack info"),
            })
            .expect("Inconsistent stack info");
        let Some(ret_point) = self.ephemeral.last_ret_point.take() else {
            if !broken {
                panic!(
                    "Last return point is expected when not broken, current: {}",
                    caller
                )
            } else {
                return caller.body;
            }
        };

        event!(
            name: EVENT_TRANSFER,
            target: TARGET,
            parent: self.current_span(),
            LEVEL,
            { FIELD_FROM_BODY } = body_id_to_str!(ret_point.body.body_id),
            { FIELD_FROM_BLOCK } = ret_point.index,
            { FIELD_TO_BODY } = body_id_to_str!(caller.body.body_id),
            { FIELD_TO_BLOCK } = caller.index,
            { FIELD_BROKEN } = broken,
        );
        caller.body
    }
}

impl DecisionTraceRecorder for Recorder {
    type Case = Constant;

    fn notify_decision(
        &mut self,
        node_location: BasicBlockLocation,
        kind: &ConstraintKind<Self::Case>,
    ) -> usize {
        let node_location = self.ensure_in_current_body(node_location);

        event!(
            name: EVENT_DECISION,
            target: TARGET,
            parent: self.current_span(),
            LEVEL,
            { FIELD_LOCATION_BLOCK } = node_location.index,
            { FIELD_DECISION_KIND } = valuable_serde::Serializable::new(Decision::from(kind)).as_value(),
        );

        Default::default()
    }
}

fn to_value(constant: &Constant) -> Box<dyn tracing::Value> {
    match constant {
        Constant::Bool(b) => Box::new(*b),
        Constant::Int { bit_rep, ty } => {
            if ty.is_signed {
                Box::new(ty.signed_masked(*bit_rep))
            } else {
                Box::new(ty.masked(*bit_rep))
            }
        }
        Constant::Char(c) => Box::new(c.to_string()),
        _ => unreachable!("Unexpected constant value in constraint: {:?}", constant),
    }
}

#[derive(Valuable)]
enum Decision {
    Value(CaseValue),
    NoneOf(Vec<CaseValue>),
}

#[derive(Valuable)]
enum CaseValue {
    Bool(bool),
    Int(String),
    Char(char),
}

impl From<&ConstraintKind<Constant>> for Decision {
    #[inline]
    fn from(value: &ConstraintKind<Constant>) -> Self {
        match value {
            ConstraintKind::True => Self::Value(CaseValue::Bool(true)),
            ConstraintKind::False => Self::Value(CaseValue::Bool(false)),
            ConstraintKind::OneOf(vals) => Self::Value(vals.iter().map(Into::into).next().unwrap()),
            ConstraintKind::NoneOf(vals) => Self::NoneOf(vals.iter().map(Into::into).collect()),
        }
    }
}

impl From<&Constant> for CaseValue {
    #[inline]
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Bool(b) => Self::Bool(*b),
            Constant::Int { bit_rep, ty } => Self::Int(if ty.is_signed {
                ty.signed_masked(*bit_rep).to_string()
            } else {
                ty.masked(*bit_rep).to_string()
            }),
            Constant::Char(c) => Self::Char(*c),
            _ => unreachable!("Unexpected constant value in constraint: {:?}", value),
        }
    }
}
