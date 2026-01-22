//! `libafl_libfuzzer` offers a "permanent" replacement for the now-deprecated libfuzzer
//!
//! ## Usage
//!
//! To use `LibAFL` in place of `LibFuzzer`, change the following line in your `fuzz/Cargo.toml`:
//!
//! ```toml
//! libfuzzer-sys = { version = "*", features = [...] }
//! ```
//!
//! With the following:
//!
//! ```toml
//! libfuzzer-sys = { version = "*", features = [...], package = "libafl_libfuzzer" }
//! ```
//!
//! To use bleeding-edge changes from upstream, use the following:
//!
//! ```toml
//! libfuzzer-sys = { version = "*", features = [...], package = "libafl_libfuzzer", git = "https://github.com/sfu-rsl/leaf " }
//! ```
//!
//! You could also specify a specific git revision using `rev = "..."` in this case.
//!
//!
//! ## Flags
//!
//! You can pass additional flags to the libfuzzer runtime in `cargo-fuzz` like so:
//!
//! ```bash
//! cargo fuzz run fuzz_target -- -extra_flag=1
//! ```
//!
//! You will commonly need this for flags such as `-ignore_crashes=1` and `-timeout=5`. In addition
//! to partial support of libfuzzer flags, `libafl_libfuzzer` offers:
//!
//! - `-dedup=n`, with `n` = 1 enabling deduplication of crashes by stacktrace.
//! - `-grimoire=n`, with `n` set to 0 or 1 disabling or enabling [grimoire] mutations, respectively.
//!   - if not specified explicitly, `libafl_libfuzzer` will select based on whether existing inputs are UTF-8
//!   - you should disable grimoire if your target is not string-like
//! - `-report=n`, with `n` = 1 causing `libafl_libfuzzer` to emit a report on the corpus content.
//! - `-skip_tracing=n`, with `n` = 1 causing `libafl_libfuzzer` to disable cmplog tracing.
//!   - you should do this if your target performs many comparisons on memory sequences which are
//!     not contained in the input
//! - `-tui=n`, with `n` = 1 enabling a graphical terminal interface.
//!   - experimental; some users report inconsistent behaviour with tui enabled
//!
//! [grimoire]: https://www.usenix.org/conference/usenixsecurity19/presentation/blazytko
//!
//! ### Supported flags from libfuzzer
//!
//! - `-merge`
//! - `-minimize_crash`
//! - `-artifact_prefix`
//! - `-timeout`
//!   - unlike libfuzzer, `libafl_libfuzzer` supports partial second timeouts (e.g. `-timeout=.5`)
//! - `-dict`
//! - `-fork` and `-jobs`
//!   - in `libafl_libfuzzer`, these are synonymous
//! - `-ignore_crashes`, `-ignore_ooms`, and `-ignore_timeouts`
//!   - note that setting `-tui=1` enables these flags by default, so you'll need to explicitly mention `-ignore_...=0` to disable them
//! - `-rss_limit_mb` and `-malloc_limit_mb`
//! - `-ignore_remaining_args`
//! - `-shrink`
//! - `-runs`
//! - `-close_fd_mask`
//!
//! ## Important notes
//!
//! This crate only offers sufficient functionality to replace libfuzzer for cargo-fuzz in its
//! current state, but may be expanded to handle other flags in the future.
//!
//! This crate links to a (separately built) internal crate which affords the actual functionality.
//! The internal crate must be built separately to ensure flags from dependent crates are not leaked
//! to the runtime (e.g., to prevent coverage being collected on the runtime).
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]
#![cfg_attr(not(test), warn(
    missing_debug_implementations,
    missing_docs,
    //trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    //unused_results
))]
#![cfg_attr(test, deny(
    missing_debug_implementations,
    missing_docs,
    //trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_must_use,
    unfulfilled_lint_expectations,
    bad_style,
    dead_code,
    improper_ctypes,
    non_shorthand_field_patterns,
    no_mangle_generic_items,
    overflowing_literals,
    path_statements,
    patterns_in_fns_without_body,
    unconditional_recursion,
    unused,
    unused_allocation,
    unused_comparisons,
    unused_parens,
    while_true
    )
)]

use core::ffi::{c_char, c_int};

pub use libfuzzer_sys::*;

unsafe extern "C" {
    /// `LLVMFuzzerRunDriver` allows for harnesses which specify their own main. See: <https://llvm.org/docs/LibFuzzer.html#using-libfuzzer-as-a-library>
    ///
    /// You can call this function inside of a main function in your harness, or specify `#![no_main]`
    /// to accept the default runtime driver.
    pub fn LLVMFuzzerRunDriver(
        argc: *mut c_int,
        argv: *mut *mut *const c_char,
        harness_fn: Option<extern "C" fn(*const u8, usize) -> c_int>,
    ) -> c_int;
}

#[cfg(all(
    feature = "embed-runtime",
    target_family = "unix",
    // Disable when building with clippy, as it will complain about the missing environment
    // variable which is set by the build script, which is not run under clippy.
    not(clippy)
))]
pub const LIBAFL_LIBFUZZER_RUNTIME_LIBRARY: &'static [u8] =
    include_bytes!(env!("LIBAFL_LIBFUZZER_RUNTIME_PATH"));

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "embed-runtime", not(clippy)))]
    #[test]
    fn test_embed_runtime_sized() {
        use crate::LIBAFL_LIBFUZZER_RUNTIME_LIBRARY;

        assert_ne!(
            LIBAFL_LIBFUZZER_RUNTIME_LIBRARY.len(),
            0,
            "Runtime library empty"
        );
    }
}

#[macro_export]
/// Defines a fuzz target for hybrid fuzzing using leaf and `cargo fuzz`.
/// It works as a drop-in replacement for [fuzz_target] macro.
/// Given a closure, it generates two versions of the program:
/// one for the fuzzing as it would work with `cargo fuzz`,
/// and another to be compiled with `leafc` for concolic execution.
/// The latter symbolizes all bytes in the standard input and then gives it to the closure.
/// The versions are exclusively compiled by the config flag `leafc`.
///
/// After replacing [fuzz_target] invocations with [hybrid_fuzz_target], you also
/// need to disable `no_main` feature when the program is compiled with `leafc`. i.e.:
/// ```rust
/// #![cfg_attr(not(leafc), no_main)]
/// ```
macro_rules! hybrid_fuzz_target {
    /* NOTE: We use token tree because fuzz_target has explicit parsing rules
     * for closures (and does not accept expr directly).
    */
    ($($closure:tt)+) => {
        #[cfg(not(leafc))]
        $crate::fuzz_target!($($closure)+);

        #[cfg(leafc)]
        fn main() {
            use leaf::annotations::*;
            use std::io::Read;

            let mut buffer = Vec::new();
            let count = std::io::stdin().read_to_end(&mut buffer).unwrap();

            for i in 0..count {
                buffer[i] = buffer[i].mark_symbolic();
            }

            let f = $($closure)+;
            f(&buffer);
        }
    };
}
