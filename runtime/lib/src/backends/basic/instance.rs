#![cfg_attr(feature = "runtime_access_raw_ptr", allow(static_mut_refs))]

/// Singleton instance management for the basic backend.
/// Multi-threaded programs are not supported, and we have few options to implement a singleton based on the safety and performance requirements.

#[cfg(not(feature = "runtime_access_raw_ptr"))]
use std::cell::RefCell;
use std::sync::Once;

use common::log_info;
use common::type_info::rw::LoadedTypeDatabase;

use cfg_if::cfg_if;

use crate::pri::{
    fluent::{
        InstanceManager,
        backend::{OperandHandler, RuntimeBackend},
    },
    refs::DefaultRefManager,
};

use super::{BasicBackendConfig, BasicPlaceBuilder};

type BackendImpl = crate::backends::basic::BasicBackend;
type PlaceInfo = <BackendImpl as RuntimeBackend>::PlaceInfo;
type OperandImpl =
    <<BackendImpl as RuntimeBackend>::OperandHandler<'static> as OperandHandler>::Operand;

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

const CONFIG_FILENAME: &str = "leaf_config";
fn load_config() -> ::config::Config {
    common::config::load_config(CONFIG_FILENAME, "LEAF", |b| Ok(b))
        .expect("Failed to read configurations")
}

pub struct BasicInstanceManager;

impl BasicInstanceManager {
    #[inline]
    fn check_and_perform_on_backend<T>(
        backend: &mut Option<BackendImpl>,
        action: impl FnOnce(&mut BackendImpl) -> T,
    ) -> T {
        let backend = if cfg!(debug_assertions) {
            backend.as_mut().expect("Runtime is not initialized.")
        } else {
            unsafe { backend.as_mut().unwrap_unchecked() }
        };
        action(backend)
    }
}

impl InstanceManager for BasicInstanceManager {
    type Backend = BackendImpl;
    type PlaceBuilder = BasicPlaceBuilder;
    type PlaceRefManager = DefaultRefManager<PlaceInfo>;
    type OperandRefManager = DefaultRefManager<OperandImpl>;

    fn init() {
        INIT.call_once(|| {
            crate::init::<crate::utils::logging::IdentityFactory>();

            log_info!("Initializing basic backend");
            let config = load_config();
            let config = BasicBackendConfig::try_from(config).expect("Failed to load config");

            let types_db =
                common::type_info::rw::read_types_db().expect("Failed to read type info");
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

    fn deinit() {}

    #[inline]
    fn perform_on_backend<T>(action: impl for<'a> FnOnce(&'a mut Self::Backend) -> T) -> T {
        cfg_if! {
            if #[cfg(feature = "runtime_access_raw_ptr")] {
                Self::check_and_perform_on_backend(unsafe { &mut BACKEND }, action)
            } else if #[cfg(feature = "runtime_access_mutex")] {
                let mut guard = BACKEND.lock().unwrap();
                Self::check_and_perform_on_backend(&mut guard, action)
            } else {
                let mut binding = BACKEND.borrow_mut();
                Self::check_and_perform_on_backend(&mut binding, action)
            }
        }
    }

    #[inline]
    fn perform_on_place_ref_manager<T>(action: impl FnOnce(&mut Self::PlaceRefManager) -> T) -> T {
        cfg_if! {
            if #[cfg(feature = "runtime_access_raw_ptr")] {
                action(unsafe { &mut PLACE_REF_MANAGER })
            } else {
                PLACE_REF_MANAGER.with_borrow_mut(action)
            }
        }
    }

    #[inline]
    fn perform_on_operand_ref_manager<T>(
        action: impl FnOnce(&mut Self::OperandRefManager) -> T,
    ) -> T {
        cfg_if! {
            if #[cfg(feature = "runtime_access_raw_ptr")] {
                action(unsafe { &mut OPERAND_REF_MANAGER })
            } else {
                OPERAND_REF_MANAGER.with_borrow_mut(action)
            }
        }
    }
}
