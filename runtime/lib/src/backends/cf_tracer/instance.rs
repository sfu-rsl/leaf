use std::{cell::RefCell, sync::Once};

use crate::{
    backends::cf_tracer::{NullOperand, NullPlace},
    pri::{
        fluent::{InstanceManager, backend::shared::noop::NoOpPlaceBuilder},
        refs::NoOpRefManager,
    },
};

use super::CftBackend;

thread_local! {
    static BACKEND: RefCell<Option<CftBackend>> = RefCell::new(None);
}
static mut PLACE_REF_MANAGER: NoOpRefManager<NullPlace> = NoOpRefManager::new(());
static mut OPERAND_REF_MANAGER: NoOpRefManager<NullOperand> = NoOpRefManager::new(());

static INIT: Once = Once::new();

pub struct CftInstanceManager;

impl InstanceManager for CftInstanceManager {
    type Backend = CftBackend;

    type PlaceBuilder = NoOpPlaceBuilder<NullPlace, NullPlace>;

    type PlaceRefManager = NoOpRefManager<NullPlace>;

    type OperandRefManager = NoOpRefManager<NullOperand>;

    fn init() {
        INIT.call_once(|| {
            crate::init::<super::tracing_i::LayerFactory>();
            BACKEND.set(Some(CftBackend::new()));
        });
    }

    fn deinit() {}

    fn perform_on_backend<T>(action: impl for<'a> FnOnce(&'a mut Self::Backend) -> T) -> T {
        BACKEND.with_borrow_mut(|b| {
            let backend = if cfg!(debug_assertions) {
                b.as_mut().expect("Runtime is not initialized.")
            } else {
                unsafe { b.as_mut().unwrap_unchecked() }
            };
            action(backend)
        })
    }

    #[allow(static_mut_refs)]
    fn perform_on_place_ref_manager<T>(action: impl FnOnce(&mut Self::PlaceRefManager) -> T) -> T {
        action(unsafe { &mut PLACE_REF_MANAGER })
    }

    #[allow(static_mut_refs)]
    fn perform_on_operand_ref_manager<T>(
        action: impl FnOnce(&mut Self::OperandRefManager) -> T,
    ) -> T {
        action(unsafe { &mut OPERAND_REF_MANAGER })
    }
}
