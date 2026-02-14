use std::cell::RefMut;

use crate::pri::fluent::backend::AnnotationHandler;

use crate::backends::basic as backend;
use backend::BasicBackend;
use common::log_debug;

const LOG_TAG_TAGS: &str = "tags";

pub(crate) struct BasicAnnotationHandler<'a> {
    tags: RefMut<'a, Vec<common::pri::Tag>>,
}

impl<'a> BasicAnnotationHandler<'a> {
    pub(super) fn new(backend: &'a mut BasicBackend) -> Self {
        Self {
            tags: backend.tags.borrow_mut(),
        }
    }

    fn log_current_tags(&self) {
        log_debug!(target: LOG_TAG_TAGS, "Current tags: [{}]", self.tags.join(", "));
    }
}

impl<'a> AnnotationHandler for BasicAnnotationHandler<'a> {
    fn push_tag(mut self, tag: common::pri::Tag) {
        self.tags.push(tag);
        self.log_current_tags();
    }

    fn pop_tag(mut self) {
        self.tags.pop();
        self.log_current_tags();
    }
}
