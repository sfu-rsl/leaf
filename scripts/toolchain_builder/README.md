# Toolchain Builder

Leaf's compilation mechanism requires slight modifications to be applied on the built-in libraries
which are taken care of by `leafc`. Thus, the built-in libraries are compiled by `leafc` to obtain a toolchain
compatible for Leaf's compilation.

This script causes a rebuild of the built-in libraries by building a dummy crate and passing `build-std` flag to cargo.
Consequently the libraries will be built as dependencies and then the script exports them in directory structured
such that to be usable as the sysroot path during compilation.

## How to use the script?
Although currently this script is run automatically by `leafc`, here are some details.

Currently, all the parameters are taken using environment variables listed below.
- `WORK_DIR`: The directory to be used for building the dummy crate.
- `OUT_DIR`: The directory which the built toolchain will be put in.
- `LEAF_WORKSPACE`: The root of Leaf's source code.
- `TARGET`: The target triple, fetched from `rustc -vV` by default.
- `LEAFS_LOG_LEVEL`: Logging level of the script, which is `info` by default.
  Setting it to `debug` and lower, causes the standard output of the executed command (e.g., building the crate)
  to be redirected as well.
- `LEAFS_ADD_LEAF_AS_DEP`: Sets whether the shim library should be added as an external dependency or integrated inside the core library.
- `LEAFS_TOOLCHAIN_MARKER_FILE`: Name of the file to be generated in the toolchain folder.
  The path of the original toolchain will be written to it.
- `LEAFC`: The `leafc` command (similar to `RUSTC` for `cargo`).
- `RUSTUP_TOOLCHAIN`: Although not one of the parameters of the script, you want to set this to fix the toolchain that will be rebuilt
  by the script. Otherwise, the toolchain resolved at `WORK_DIR` will be used.
