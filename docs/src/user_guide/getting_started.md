# Getting Started
Although currently working with the first releases of Leaf, we aim to provide a rather straightforward workflow for the users.

Please follow the instruction below to try Leaf in your environment.

## Installing Leaf

### Requirements
- Rust (`rustup`)
- Python

You can install Leaf using `cargo` and by building the source code.

1. Clone the repository.
    ```console
    $ git clone https://github.com/sfu-rsl/leaf.git
    $ cd leaf
    ```
1. Install Leaf's compiler named as `leafc` using
    ```console
    $ cargo install --path ./compiler
    ```
## Performing Concolic Execution
As an instrumentation-based dynamic analyzer, Leaf instruments programs such that
they expose information about their behavior during each execution.
Therefore, concolic execution is achieved by compiling the target program and
running the executable.

1. Pick a program you want to do concolic execution for. Some are available in `samples` directory of the source tree.
    ```rust
    fn main() {
        let x: u8 = 10;
        if x < 5 {
            println!("Hello, world!");
        }
    }
    ```

1. Mark a variable interest as symbolic.
    ```rust
    // samples/hello_world.rs

    use leaf::annotations::*;
    fn main() {
        let x: u8 = 10.mark_symbolic();
        if x < 5 {
            println!("Hello, world!");
        }
    }
    ```

1. Compile your target program using `leafc` as you would do with `rustc`. (The first compilation takes longer, bear with it!)
    ```console
    $ leafc ./hello_world.rs
    ```

1. Prepare the environment to observe the execution through the standard error
   by setting `LEAF_LOG` variable. e.g.,
    ```bash
    export LEAF_LOG="info"
    ```
1. Execute the compiled program.
    ```console
    $ ./hello_world 
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
    The logs include information about the constraints put on the variables marked as symbolic for the path taken in the execution.

1. Given the current default configuration, a folder named `leaf_out` also gets generated
   which contains alternative values for the symbolic variables that should cause
   the execution take other paths than the one taken, which we call diverging inputs.
   ```console
   $ ls ./leaf_out
   0.bin
   ```

-----------

More details about each step is provided in the rest of the book.