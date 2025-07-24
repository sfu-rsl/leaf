#![cfg_attr(feature = "runtime_access_raw_ptr", allow(static_mut_refs))]

use common::type_info::rw::LoadedTypeDatabase;

use super::utils::{DefaultRefManager, RefManager};
use super::{AssignmentId, OperandRef, PlaceHandler, PlaceRef, SwitchInfo};
use crate::abs::{
    BasicBlockIndex, PlaceUsage,
    backend::{
        AssignmentHandler, ConstraintHandler, OperandHandler, PlaceBuilder, RuntimeBackend,
        Shutdown,
    },
};
use crate::backends::basic::BasicPlaceBuilder;
use cfg_if::cfg_if;
use common::log_info;

use std::{cell::RefCell, sync::Once};

type ConfigImpl = crate::backends::basic::BasicBackendConfig;
pub(super) type BackendImpl = crate::backends::basic::BasicBackend;

type PlaceInfo = <BasicPlaceBuilder as PlaceBuilder>::Place;
type PlaceImpl = <<BackendImpl as RuntimeBackend>::PlaceHandler<'static> as PlaceHandler>::Place;
pub(super) type OperandImpl =
    <<BackendImpl as RuntimeBackend>::OperandHandler<'static> as OperandHandler>::Operand;
pub(super) type FieldImpl =
    <<BackendImpl as RuntimeBackend>::AssignmentHandler<'static> as AssignmentHandler>::Field;

static INIT: Once = Once::new();
cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        static mut BACKEND: Option<BackendImpl> = None;
    } else if #[cfg(feature = "runtime_access_mutex")] {
        use std::sync::Mutex;
        static BACKEND: Mutex<Option<BackendImpl>> = Mutex::new(None);
    } else {
        use common::utils::UnsafeSync;
        static BACKEND: UnsafeSync<RefCell<Option<BackendImpl>>> = UnsafeSync::new(RefCell::new(None));
    }
}

cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        static mut PROGRAM_TYPES: Option<LoadedTypeDatabase> = None;
    } else {
        use std::sync::OnceLock;
        static PROGRAM_TYPES: OnceLock<LoadedTypeDatabase> = OnceLock::new();
    }
}

cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        static mut PLACE_REF_MANAGER: DefaultRefManager<PlaceInfo> = DefaultRefManager::new();
    } else {
        thread_local! {
            // Place and operand references are local to functions, so they need not and should not be shared
            static PLACE_REF_MANAGER: RefCell<DefaultRefManager<PlaceInfo>> =
                RefCell::new(DefaultRefManager::new());
        }
    }
}

cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        static mut OPERAND_REF_MANAGER: DefaultRefManager<OperandImpl> = DefaultRefManager::new();
    } else {
        thread_local! {
            static OPERAND_REF_MANAGER: RefCell<DefaultRefManager<OperandImpl>> =
                RefCell::new(DefaultRefManager::new());
        }
    }
}

pub(super) fn init_backend() {
    INIT.call_once(|| {
        crate::init();

        log_info!("Initializing basic backend");
        let config = load_config();
        let config = ConfigImpl::try_from(config).expect("Failed to load config");

        let types_db = common::type_info::rw::read_types_db().expect("Failed to read type info");
        cfg_if! {
            if #[cfg(feature = "runtime_access_raw_ptr")] {
                unsafe { PROGRAM_TYPES = Some(types_db); }
                let types_db = unsafe { PROGRAM_TYPES.as_ref().unwrap() };
            } else {
                let types_db = PROGRAM_TYPES.get_or_init(move || types_db);
            }
        }
        let backend = BackendImpl::new(config, types_db);
        cfg_if! {
            if #[cfg(feature = "runtime_access_raw_ptr")] {
                unsafe { BACKEND = Some(backend); }
            } else if #[cfg(feature = "runtime_access_mutex")] {
                let mut guard = BACKEND.lock().unwrap();
                *guard = Some(backend);
            } else {
                let mut binding = BACKEND.borrow_mut();
                *binding = Some(backend);
            }
        }
        log_info!("Basic backend initialized");
    });
}

pub(super) fn shutdown_backend() {
    // FIXME: We can drop the here.
    perform_on_backend(|r| r.shutdown())
}

const CONFIG_FILENAME: &str = "leaf_config";

fn load_config() -> ::config::Config {
    common::config::load_config(CONFIG_FILENAME, "LEAF", |b| Ok(b))
        .expect("Failed to read configurations")
}

/* FIXME: Make these functions rely on abstract traits rather than concrete types.
 * It may require some wrapper types to make borrowing/not borrowing of inner objects
 * definite.
 */

pub(super) fn push_place_info(build: impl FnOnce(BasicPlaceBuilder) -> PlaceInfo) -> PlaceRef {
    let builder = BasicPlaceBuilder::default();
    let place = build(builder);
    perform_on_place_ref_manager(|rm| rm.push(place))
}
pub(super) fn mut_place_info(
    place_ref: PlaceRef,
    mut_place: impl FnOnce(BasicPlaceBuilder, &mut PlaceInfo),
) {
    perform_on_place_ref_manager(|rm| {
        let place = rm.get_mut(place_ref);
        let builder = BasicPlaceBuilder::default();
        mut_place(builder, place);
    });
}
pub(super) fn take_back_place_info(reference: PlaceRef) -> PlaceInfo {
    perform_on_place_ref_manager(|rm| rm.take(reference))
}
pub(super) fn take_place_info_to_read(reference: PlaceRef) -> PlaceImpl {
    let place_info = take_back_place_info(reference);
    get_backend_place(PlaceUsage::Read, |h| h.from_info(place_info))
}
pub(super) fn take_place_info_to_write(reference: PlaceRef) -> PlaceImpl {
    let place_info = take_back_place_info(reference);
    get_backend_place(PlaceUsage::Write, |h| h.from_info(place_info))
}
pub(super) fn take_place_info_to_ref(reference: PlaceRef) -> PlaceImpl {
    let place_info = take_back_place_info(reference);
    get_backend_place(PlaceUsage::Ref, |h| h.from_info(place_info))
}
#[inline]
pub(super) fn get_backend_place<T>(
    usage: PlaceUsage,
    get_place: impl FnOnce(<BackendImpl as RuntimeBackend>::PlaceHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| get_place(r.place(usage)))
}

pub(super) fn assign_to<T>(
    id: AssignmentId,
    dest_ref: PlaceRef,
    assign_action: impl FnOnce(<BackendImpl as RuntimeBackend>::AssignmentHandler<'_>) -> T,
) -> T {
    let dest = take_place_info_to_write(dest_ref);
    assign_to_place(id, dest, assign_action)
}

pub(super) fn assign_to_place<T>(
    id: AssignmentId,
    dest: PlaceImpl,
    assign_action: impl FnOnce(<BackendImpl as RuntimeBackend>::AssignmentHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| assign_action(r.assign_to(id, dest)))
}

pub(super) fn push_operand(
    get_operand: impl FnOnce(<BackendImpl as RuntimeBackend>::OperandHandler<'_>) -> OperandImpl,
) -> OperandRef {
    let operand = perform_on_backend(|r| get_operand(r.operand()));
    perform_on_operand_ref_manager(|rm| rm.push(operand))
}
pub(super) fn take_back_operand(reference: OperandRef) -> OperandImpl {
    perform_on_operand_ref_manager(|rm| rm.take(reference))
}

pub(super) fn memory<T>(
    memory_action: impl FnOnce(<BackendImpl as RuntimeBackend>::MemoryHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| {
        let handler = r.memory();
        memory_action(handler)
    })
}

pub(super) fn constraint_at<T>(
    location: BasicBlockIndex,
    constraint_action: impl FnOnce(<BackendImpl as RuntimeBackend>::ConstraintHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| {
        let handler = r.constraint_at(location);
        constraint_action(handler)
    })
}

pub(super) fn switch<T>(
    info: SwitchInfo,
    switch_action: impl FnOnce(
        <<BackendImpl as RuntimeBackend>::ConstraintHandler<'_> as ConstraintHandler>::SwitchHandler,
    ) -> T,
) -> T {
    constraint_at(info.node_location, |c| {
        let handler = c.switch(take_back_operand(info.discriminant));
        switch_action(handler)
    })
}

pub(super) fn func_control<T>(
    call_action: impl FnOnce(<BackendImpl as RuntimeBackend>::CallHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| {
        let call_control = r.call_control();
        call_action(call_control)
    })
}

pub(super) fn annotate<T>(
    annotate_action: impl FnOnce(<BackendImpl as RuntimeBackend>::AnnotationHandler<'_>) -> T,
) -> T {
    perform_on_backend(|r| {
        let annotate = r.annotate();
        annotate_action(annotate)
    })
}

cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        fn perform_on_backend<T>(action: impl FnOnce(&mut BackendImpl) -> T) -> T {
            check_and_perform_on_backend(unsafe { &mut BACKEND }, action)
        }
    } else if #[cfg(feature = "runtime_access_mutex")] {
        fn perform_on_backend<T>(action: impl FnOnce(&mut BackendImpl) -> T) -> T {
            let mut guard = BACKEND.lock().unwrap();
            check_and_perform_on_backend(&mut guard, action)
        }
    } else {
        fn perform_on_backend<T>(action: impl FnOnce(&mut BackendImpl) -> T) -> T {
            let mut binding = BACKEND.borrow_mut();
            check_and_perform_on_backend(&mut binding, action)
        }
    }
}

fn check_and_perform_on_backend<T>(
    backend: &mut Option<BackendImpl>,
    action: impl FnOnce(&mut BackendImpl) -> T,
) -> T {
    let backend = backend.as_mut().expect("Runtime is not initialized.");
    action(backend)
}
cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        fn perform_on_place_ref_manager<T>(
            action: impl FnOnce(&mut DefaultRefManager<PlaceInfo>) -> T,
        ) -> T {
            action(unsafe { &mut PLACE_REF_MANAGER })
        }
    } else {
        fn perform_on_place_ref_manager<T>(
            action: impl FnOnce(&mut DefaultRefManager<PlaceInfo>) -> T,
        ) -> T {
            PLACE_REF_MANAGER.with_borrow_mut(action)
        }
    }
}

cfg_if! {
    if #[cfg(feature = "runtime_access_raw_ptr")] {
        fn perform_on_operand_ref_manager<T>(
            action: impl FnOnce(&mut DefaultRefManager<OperandImpl>) -> T,
        ) -> T {
            action(unsafe { &mut OPERAND_REF_MANAGER })
        }
    } else {
        fn perform_on_operand_ref_manager<T>(
            action: impl FnOnce(&mut DefaultRefManager<OperandImpl>) -> T,
        ) -> T {
            OPERAND_REF_MANAGER.with_borrow_mut(action)
        }
    }
}
