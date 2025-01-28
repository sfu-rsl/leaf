use rustc_ast::{DUMMY_NODE_ID, Item, ItemKind, Visibility, VisibilityKind, ptr::P};
use rustc_session::Session;
use rustc_span::{
    DUMMY_SP,
    symbol::{Ident, Symbol},
};

use crate::pri_utils::sym::RUNTIME_LIB_CRATE;

use super::CompilationPass;
use common::log_info;

/// A pass that adds the runtime library as an extern crate to the program.
#[derive(Clone)]
pub(crate) struct RuntimeExternCrateAdder {
    enabled: bool,
    crate_name: Option<String>,
}

impl RuntimeExternCrateAdder {
    pub fn new(enabled: bool, crate_name: Option<String>) -> Self {
        Self {
            crate_name,
            enabled,
        }
    }
}

impl CompilationPass for RuntimeExternCrateAdder {
    fn transform_ast(
        &mut self,
        _session: &Session,
        krate: &mut rustc_ast::Crate,
        _storage: &mut dyn super::Storage,
    ) {
        if !self.enabled {
            return;
        }

        let crate_name = self
            .crate_name
            .as_ref()
            .map(|n| n.as_str())
            .unwrap_or(*RUNTIME_LIB_CRATE);

        // extern crate runtime as `crate_name`;
        let item = Item {
            attrs: Default::default(),
            id: DUMMY_NODE_ID,
            span: DUMMY_SP,
            vis: Visibility {
                kind: VisibilityKind::Inherited,
                span: DUMMY_SP,
                tokens: None,
            },
            ident: Ident::with_dummy_span(Symbol::intern(crate_name)),
            kind: ItemKind::ExternCrate(Some(Symbol::intern(*RUNTIME_LIB_CRATE))),
            tokens: None,
        };
        krate.items.insert(0, P(item));

        log_info!(
            "Added extern crate statement for the runtime library: extern crate {} as {};",
            *RUNTIME_LIB_CRATE,
            crate_name
        );
    }
}
