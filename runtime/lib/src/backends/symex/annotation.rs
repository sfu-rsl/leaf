use std::cell::RefMut;

use common::log_debug;

use crate::pri::fluent::backend::AnnotationHandler;

use super::alias::backend;
use backend::SymExBackend;

const LOG_TAG_TAGS: &str = "tags";

pub(crate) struct SymExAnnotationHandler<'a> {
    tags: RefMut<'a, Vec<common::pri::Tag>>,
}

impl<'a> SymExAnnotationHandler<'a> {
    pub(super) fn new(backend: &'a mut SymExBackend) -> Self {
        Self {
            tags: backend.tags.borrow_mut(),
        }
    }

    fn log_current_tags(&self) {
        log_debug!(target: LOG_TAG_TAGS, "Current tags: [{}]", self.tags.join(", "));
    }
}

impl<'a> AnnotationHandler for SymExAnnotationHandler<'a> {
    fn push_tag(mut self, tag: common::pri::Tag) {
        self.tags.push(tag);
        self.log_current_tags();
    }

    fn pop_tag(mut self) {
        self.tags.pop();
        self.log_current_tags();
    }
}
