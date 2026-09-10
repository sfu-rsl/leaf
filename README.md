# Leaf
<img align="right" src="docs/book/src/assets/LeafLogo.svg" style="width:8em" alt="The Leaf logo">

**Dynamic analysis for Rust, built around MIR instrumentation.**

Leaf is a framework for dynamic analysis built around [Rust MIR][mir] instrumentation. Its `leafc` compiler wrapper instruments a program at compile time, then routes runtime events to pluggable backends for control-flow tracing, symbolic execution, and other analyses.

With Leaf, you can:

- compile Rust programs through `leafc` with MIR instrumentation;
- connect instrumented programs to runtime analysis backends; and
- mark values or program points for analysis with Rust annotations;
- inspect execution traces and backend-specific analysis output.

The project is under active development. The [Leaf Book](https://sfu-rsl.github.io/leaf) has the broader architecture, tutorials, and reference material.

## Quick start

### Requirements

- Rustup and cargo to install nightly toolchains, `rustc` libraries and building the project.
- Python for helper scripts (e.g., toolchain builder) in the repository

1. Clone the repository and build the compiler:
   ```console
   $ git clone https://github.com/sfu-rsl/leaf.git
   $ cd leaf
   $ cargo install --path ./compiler
   ```

1. Build a runtime backend, for example the control-flow tracer:
   ```console
   $ cargo build -p runtime_backend_cf_tracer
   ```

1. Make the shared library discoverable to the generated program:
   ```console
   $ mkdir -p target/debug/runtime_cf_tracer
   $ ln -sf target/debug/runtime_cf_tracer.so target/debug/runtime_cf_tracer/libleafrt.so
   $ export LD_LIBRARY_PATH="$PWD/target/debug/runtime_cf_tracer:$LD_LIBRARY_PATH"
   ```

1. Compile a sample program with `leafc`:
   ```console
   $ leafc samples/hello_world.rs
   ```

1. Run the instrumented binary with logging enabled:
   ```console
   $ export LEAF_LOG="info"
   $ ./hello_world
   ```

The generated program will emit runtime events through the active backend, which can be inspected through the logging output or any backend-specific artifacts.

<details open>
<summary>Control Flow Tracer Output (with selective instrumentation)</summary>

```jsonl
{"fields":{"from_block":0,"from_body":"0:4(0)","kind":"call"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193382Z"}
{"fields":{"broken":false,"from_block":0,"from_body":"0:4(0)","kind":"call","to_block":0,"to_body":"2:1562(0)"},"level":"INFO","name":"transfer","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193444Z"}
{"fields":{"from_block":1,"from_body":"2:1562(0)","kind":"return"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193546Z"}
{"fields":{"broken":false,"from_block":1,"from_body":"2:1562(0)","kind":"return","to_block":0,"to_body":"0:4(0)"},"level":"INFO","name":"transfer","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193590Z"}
{"fields":{"from_block":1,"from_body":"0:4(0)","kind":"call"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193612Z"}
{"fields":{"block_index":2,"kind":"Decision::NoneOf([])"},"level":"INFO","name":"decision","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193731Z"}
{"fields":{"from_block":3,"from_body":"0:4(0)","kind":"call"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193752Z"}
{"fields":{"broken":false,"from_block":3,"from_body":"0:4(0)","kind":"call","to_block":0,"to_body":"2:13152(0)"},"level":"INFO","name":"transfer","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.193788Z"}
{"fields":{"from_block":0,"from_body":"2:13152(0)","kind":"return"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.194183Z"}
{"fields":{"broken":false,"from_block":0,"from_body":"2:13152(0)","kind":"return","to_block":3,"to_body":"0:4(0)"},"level":"INFO","name":"transfer","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.194223Z"}
{"fields":{"from_block":4,"from_body":"0:4(0)","kind":"call"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.194243Z"}
Hello, world!
{"fields":{"from_block":6,"from_body":"0:4(0)","kind":"return"},"level":"INFO","name":"transfer_start","threadId":"ThreadId(2)","timestamp":"2026-09-10T14:04:14.194363Z"}
```
</details>

<details>
<summary>Symbolic Executor Output</summary>

```log
 2026-09-10 06:55:33  INFO leafrtb_symex Initializing symbolic execution backend
 2026-09-10 06:55:33  INFO leafrtb_symex::instance Initializing symbolic execution backend
 2026-09-10 06:55:33  INFO leafcmn::type_info::rw Finding and reading types db
 2026-09-10 06:55:33  INFO constraint_sanity Trace satisfiability sanity checking will be performed for this run
 2026-09-10 06:55:33  INFO leafcmn::answers::binary Setting up binary output writing to directory: leaf_out
 2026-09-10 06:55:33  WARN leafcmn::answers::binary Output directory has some previous answers, which may may be overwritten: /workspaces/Rust/leaf/leaf/samples/leaf_out
 2026-09-10 06:55:33  INFO constraint_sanity Constraint satisfiability sanity checking will be performed for this run
 2026-09-10 06:55:33  INFO leafcmn::program_dep::rw Finding and reading dependence map
 2026-09-10 06:55:33  INFO leafrtb_symex::instance SymEx backend initialized
 2026-09-10 06:55:33  INFO leafrtb_symex::sym_vars Added a new symbolic variable: <Var1: u8> = 2u8
 2026-09-10 06:55:33  INFO leafrt::trace::log Notified about constraint {<(<Var1: u8>, 5u8)} at step 3: Instance(Def(0:4))[2]
 2026-09-10 06:55:33  INFO leafrt::outgen Found a solution:
   {
       "1": 8u8,
   }
Hello, world!
 2026-09-10 06:55:33  INFO leafrtb_symex Shutting down the backend
```

</details>


## Project layout

- `compiler/`: the `leafc` driver and instrumentation pipeline
- `runtime/lib`: the shared abstraction library for implementing runtime backends
- `runtime/backends/`: concrete backend implementations
- `common/`: shared facilities and definitions used across the project

## Documentation

Further information, tutorials, and technical details are collected in Leaf Book @
[sfu-rsl.github.io/leaf](https://sfu-rsl.github.io/leaf) (WIP).

## Publications
- Leaf: An Instrumentation-based Dynamic Analysis Framework for Rust: [arXiv](https://arxiv.org/abs/2607.15025)

## License

Leaf is licensed under the MIT or Apache-2.0 licenses.

- Apache License, Version 2.0: [LICENSE-APACHE](LICENSE-APACHE)
- MIT License: [LICENSE-MIT](LICENSE-MIT)


[mir]: https://rustc-dev-guide.rust-lang.org/mir/index.html