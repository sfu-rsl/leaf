# Getting Started

Leaf is a Rust-oriented framework for dynamic analysis built around MIR instrumentation. The workflow is:

1. compile and instrument a target program with `leafc`, and
1. provide a runtime backend that receives callbacks from the instrumented program,
4. run the instrumented program with the backend plugged in.

## Requirements

- Rust
- Python
- A working C toolchain and linker

## Installing Leaf

1. Clone the repository and enter the workspace.
   ```console
   $ git clone https://github.com/sfu-rsl/leaf.git
   $ cd leaf
   ```

1. Install the compiler frontend.
   ```console
   $ cargo install --path ./compiler
   ```

## Preparing Dynamic Analysis

1. Build a runtime backend, for example symbolic execution.
   ```console
   $ cargo build -p runtime_symex
   ```

1. Make the runtime shared library discoverable to the instrumented binary.
   ```console
   $ mkdir -p target/debug/runtime_symex
   $ ln -sf "$(find target/debug -maxdepth 1 -name 'libleafrt*.so' | head -n 1)" target/debug/runtime_symex/libleafrt.so
   $ export LD_LIBRARY_PATH="$PWD/target/debug/runtime_symex:$LD_LIBRARY_PATH"
   ```

## Analyzing a Program

Leaf ships with sample programs under the `samples/` directory. A minimal example is the `hello_world` sample.
```rust
fn main() {
    let x: u8 = core::hint::black_box(10);
    #[cfg(leafc)]
    let x: u8 = {
        use leaf::annotations::*;
        x.mark_symbolic()
    };

    if x < 5 {
        println!("Hello, world!");
    }
}
```

1. Compile the sample with `leafc`.
   ```console
   $ leafc samples/hello_world.rs
   ```

2. Enable logging for the runtime.
   ```console
   $ export LEAF_LOG="info"
   ```

3. Run the generated binary.
   ```console
   $ ./hello_world
   ```

You should see runtime events emitted by the active backend. The exact output depends on the chosen backend and logging configuration, but the execution should complete and produce instrumentation traces or analysis data. With the example symbolic execution backend in effect, an output similar to the following is expected.
```log
2024-12-10 00:40:55  INFO leafrt Initializing runtime library
2024-12-10 00:40:55  INFO leafrt::pri::basic::instance Initializing basic backend
2024-12-10 00:40:55  INFO leafrt::backends::basic::outgen Setting up binary output writing to directory: output
2024-12-10 00:40:55  INFO leafrt::pri::basic::instance Basic backend initialized
2024-12-10 00:40:55  INFO leafrt::backends::basic::sym_vars Added a new symbolic variable: <Var1: u8> = 10u8
2024-12-10 00:40:55  INFO leafrt::trace::log Notified about constraint {!(<(<Var1: u8>, 5u8))} at step Def(0:5)[2]
2024-12-10 00:40:55  INFO leafrt::outgen Found a solution:
{
    "1": 0u8,
}
```

## Next steps

The rest of the book covers the compiler pipeline, runtime backends, and more advanced analysis workflows in greater detail.