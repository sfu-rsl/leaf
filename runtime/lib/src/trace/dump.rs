use std::fs::File;

use serde::{Serialize, Serializer, ser::SerializeStruct};

use crate::utils::file::JsonLinesFormatter;

use super::{Constraint, StepInspector};

pub(crate) struct StreamDumperStepInspector<S: Serialize, V: Serialize, C: Serialize, Ser> {
    serializer: Ser,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<S: Serialize, V: Serialize, C: Serialize, Ser> StreamDumperStepInspector<S, V, C, Ser> {
    pub fn new(serializer: Ser) -> Self {
        Self {
            serializer,
            _phantom: Default::default(),
        }
    }
}
impl<S: Serialize, V: Serialize, C: Serialize>
    StreamDumperStepInspector<S, V, C, serde_json::Serializer<std::fs::File, JsonLinesFormatter>>
{
    pub fn json_lines(stream_file: File) -> Self {
        Self::new(serde_json::Serializer::with_formatter(
            stream_file,
            JsonLinesFormatter::default(),
        ))
    }
}

impl<S: Serialize, V: Serialize, C: Serialize, Ser> StepInspector<S, V, C>
    for StreamDumperStepInspector<S, V, C, Ser>
where
    for<'a> &'a mut Ser: Serializer,
{
    fn inspect(&mut self, step: &S, constraint: Constraint<&V, &C>) {
        let serializer = &mut self.serializer;
        serializer
            .serialize_struct("Record", 2)
            .and_then(|mut rec_ser| {
                rec_ser.serialize_field(stringify!(step), step)?;
                rec_ser.serialize_field(stringify!(constraint), &constraint)?;
                rec_ser.end()
            })
            .unwrap_or_else(|e| panic!("Could not dump step: {e}"));
    }
}
