# Leaf

Concolic execution for Rust through MIR instrumentation.


## Table of Contents
- [Getting Started](#getting-started)
- [Documentation](#documentation)

## Getting Started
1. Clone the repository.
1. Install `leafc` using
    ```
    cargo install --path ./compiler
    ```
1. Compile target programs using `leafc`, e.g.,
    ```
    leafc samples/hello_world.rs
    ```
1. Enable logging for Leaf's backend using `LEAF_LOG` environment variable.
    ```
    export LEAF_LOG="info"
    ```
1. Run the compiled program.
    ```
    hello_world
    ```
1. An output similar to the following is expected from the execution.
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

This was a demonstration of the basic workflow to perform dynamic symbolic execution using Leaf.

Leaf comes with a compiler (`leafc`) that instruments programs.
It is a wrapper around the Rust compiler and should be usable in any existing command
in place of `rustc`.
The instrumented program calls the backend during runtime, providing the information about the events inside the program
including the constraints on the variables for the path currently being taken.
By marking a variables of interest as symbolic, the system records the constraints on them, and later provides output
based on them, e.g., concrete values for them which cause the program take paths different from the current one.
For further information please refer to documentations.

## Documentation

Further information, tutorials, and technical details are collected in Leaf Book hosted at:
[sfu-rsl.github.io/leaf](https://sfu-rsl.github.io/leaf).

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
