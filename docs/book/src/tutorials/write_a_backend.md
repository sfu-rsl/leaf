# Write a Counter Backend

This tutorial shows how to write a minimal Leaf backend and plug it into an instrumented program.

## Goal

You will build a backend that counts assignments and run it through a Leaf-instrumented target.

The backend reports:

- total assignments
- unary assignment counts by `UnaryOp`
- binary assignment counts by `BinaryOp`

You will do this in five steps:

1. Define a minimal `RuntimeBackend`.
2. Define an `AssignmentHandler` that performs counting.
3. Add an `InstanceManager` for the backend.
4. Package the backend as a dynamic library.
5. Run an instrumented sample and verify the output.

## Backend Design

Before starting the implementation, it helps to identify the core pieces of the backend.

The backend reports the total number of assignments, plus binary and unary assignment counts grouped by operation.

### State

The information that a backend maintains about the running program is its *state*.
For this backend, the state looks like this:

```rust
# use std::collections::HashMap;
# use leaf_runtime::abs::{BinaryOp, UnaryOp};
#
# #[derive(Default)]
struct AssignStats {
    total_assignments: u64,
    binary_by_op: HashMap<BinaryOp, u64>,
    unary_by_op: HashMap<UnaryOp, u64>,
}
```


### Updating the State

We need to:

- increment `total_assignments` for every assignment
- increment the appropriate map for every unary or binary assignment

This behavior can be represented as follows:

```rust
struct Counter<'a> {
    stats: &'a mut AssignStats,
}

impl Counter<'_> {
    fn binary_op_between(mut self, op: BinaryOp) {
        *self.stats.binary_by_op.entry(op).or_default() += 1;
        self.some()
    }

    fn unary_op_on(mut self, op: UnaryOp) {
        *self.stats.unary_by_op.entry(op).or_default() += 1;
        self.some()
    }

    fn some(mut self) {
        self.stats.total_assignments += 1;
    }
}
```

This is the core functionality that the backend must provide. Next, we map it to Leaf's backend model.

## Realizing the Backend

### Step 1: Define a Backend

> [!TIP]
> Boilerplate code is hidden by default, which can be displayed by clicking on the eyeball (<i class="fa fa-eye"></i>) button.

An implementation of a backend follows the contract defined by `RuntimeBackend`.
Define the backend as follows:
```rust
# use leaf_runtime::{
#     abs::{AssignmentId, BasicBlockIndex, BinaryOp, PlaceUsage, UnaryOp, backend::Shutdown},
#     pri::fluent::backend::{RuntimeBackend, shared::noop::*},
# };
#
struct CounterBackend {
    // Components that will live during the execution.
}

impl RuntimeBackend for CounterBackend {
    // TODO
#    type PlaceHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type OperandHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type AssignmentHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type MemoryHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type RawMemoryHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type ConstraintHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type CallHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type DropHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type AnnotationHandler<'a> = /* ... */
#    where
#        Self: 'a;
#
#    type PlaceInfo = /* ... */;
#    type Place = /* ... */;
#    type DiscriminablePlace = /* ... */;
#    type Operand = /* ... */;
#
#    fn place<'a>(&'a mut self, _usage: PlaceUsage) -> Self::PlaceHandler<'a> {
#        Default::default()
#    }
#
#    fn operand<'a>(&'a mut self) -> Self::OperandHandler<'a> {
#        Default::default()
#    }
#
#    fn assign_to<'a>(
#        &'a mut self, _id: AssignmentId, _dest: Self::Place,
#    ) -> Self::AssignmentHandler<'a> {
#        Default::default()
#    }
#
#    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a> {
#        Default::default()
#    }
#
#    fn raw_memory<'a>(&'a mut self) -> Self::RawMemoryHandler<'a> {
#        Default::default()
#    }
#
#    fn constraint_at<'a>(&'a mut self, _loc: BasicBlockIndex) -> Self::ConstraintHandler<'a> {
#        Default::default()
#    }
#
#    fn call_control<'a>(&'a mut self) -> Self::CallHandler<'a> {
#        Default::default()
#    }
#
#    fn dropping<'a>(&'a mut self) -> Self::DropHandler<'a> {
#        Default::default()
#    }
#
#    fn annotate<'a>(&'a mut self) -> Self::AnnotationHandler<'a> {
#        Default::default()
#    }
}
```

MIR contains several kinds of events that a backend can handle. This backend only needs assignments, so no-op definitions are sufficient for the other event types.


```rust
type PlaceHandler<'a>
    = NoOpPlaceHandler
where
    Self: 'a;

// Similar for other associated types
# type OperandHandler<'a>
#     = NoOpOperandHandler
# where
#     Self: 'a;
# 
# type AssignmentHandler<'a>
#     = NoOpAssignmentHandler
# where
#     Self: 'a;
# 
# type MemoryHandler<'a>
#     = NoOpLifetimeHandler
# where
#     Self: 'a;
# 
# type RawMemoryHandler<'a>
#     = NoOpRawMemoryHandler
# where
#     Self: 'a;
# 
# type ConstraintHandler<'a>
#     = NoOpConstraintHandler
# where
#     Self: 'a;
# 
# type CallHandler<'a>
#     = NoOpCallHandler
# where
#     Self: 'a;
# 
# type DropHandler<'a>
#     = NoOpDropHandler
# where
#     Self: 'a;
# 
# type AnnotationHandler<'a>
#     = NoOpAnnotationHandler
# where
#     Self: 'a;
# 
# 
# type PlaceInfo = NullPlaceInfo;
# type Place = NullPlace;
# type DiscriminablePlace = NullPlace;
# 
# type Operand = NullOperand;
```

> [!NOTE]
> We explain each element elsewhere in the book. To keep this tutorial focused, treat these associated types and methods as holes filled by no-op definitions.

The backend instance owns the state, so add it:

```rust
#[derive(Default)]
struct CounterBackend { 
    stats: AssignStats,
}
```

Print the statistics when the runtime shuts down:

```rust
impl Shutdown for CounterBackend {
    fn shutdown(&mut self) {
        println!(
            "total assignments: {}\nbinary: {:?}\nunary: {:?}",
            self.stats.total_assignments, self.stats.binary_by_op, self.stats.unary_by_op,
        );
    }
}
```


### Step 2: Handling Assignments

Now we add the counting behavior shown earlier as an implementation for `AssignmentHandler`.

```rust
struct CounterAssignmentHandler<'a> {
  stats: &'a mut AssignStats,
}

impl AssignmentHandler for CounterAssignmentHandler<'_> {
  type Place = NullPlace;
  type Operand = NullOperand;

    fn binary_op_between(self, op: BinaryOp, _a: Self::Operand, _b: Self::Operand) {
    *self.stats.binary_by_op.entry(op).or_default() += 1;
    self.some()
  }

    fn unary_op_on(self, op: UnaryOp, _operand: Self::Operand) {
    *self.stats.unary_by_op.entry(op).or_default() += 1;
    self.some()
  }

  // Catch-all for other assignment forms.
    fn some(self) {
    self.stats.total_assignments += 1;
  }
}
```

Then installing it in the backend:

```rust
impl RuntimeBackend for CounterBackend {
    type AssignmentHandler<'a>
        = CounterAssignmentHandler<'a>
    where
        Self: 'a;

    fn assign_to<'a>(
        &'a mut self,
        _id: AssignmentId,
        _dest: Self::Place,
    ) -> CounterAssignmentHandler<'a> {
        CounterAssignmentHandler { stats: &mut self.stats }
    }
}
```

> A few details are worth mentioning, although they are not specific to this backend:
> * The traits to implement for a backend and its handlers are defined for working in `FluentPri`.
> * The parameters given in an interface call chain provide the representation of the pieces in the original MIR event. For instance, in an assignment based on a unary operation like `_5 = Neg(move _4);`, a call chain of `assign_to(ID_X, p_dest).unary_op_on(UnaryOp::Neg, p_operand)` is expected where `ID_X` corresponds to the unique id for this assignment in its parent body, `p_dest` and `p_operand` correspond to place representations for locals `_5` and `_4`.
> * Handler components of a backend are designed to be short-lived instances that provide the expected interface. All durable information (e.g., program state) should be owned by the backend itself and borrowed by the handlers.

## Step 3: Add an `InstanceManager` and `Pri`

### `InstanceManager`
An instance manager constructs, provides access to, and destroys backend instances for probes. Probes can run at any point during execution and in any function in the program. In many cases, a simple instance manager that wraps a globally allocated backend instance suffices.

This tutorial does not explain the lower-level details of this trait. Use the following implementation as a template, and see the rest of the book for details.

```rust
mod instance {
    use std::sync::{Mutex, Once};

    use leaf_runtime::pri::{fluent::InstanceManager, refs::NoOpRefManager};

    use super::*;

    static BACKEND: Mutex<Option<CounterBackend>> = Mutex::new(None);
    static mut PLACE_REF_MANAGER: NoOpRefManager<NullPlace> = NoOpRefManager::new(());
    static mut OPERAND_REF_MANAGER: NoOpRefManager<NullOperand> = NoOpRefManager::new(());

    static INIT: Once = Once::new();

    pub(crate) struct CounterInstanceManager;

    impl InstanceManager for CounterInstanceManager {
        type PlaceInfo = NullPlace;
        type Place = NullPlace;
        type Operand = NullOperand;

        type Backend = CounterBackend;

        type PlaceBuilder = NoOpPlaceBuilder<NullPlace, NullPlace>;

        type PlaceRefManager = NoOpRefManager<NullPlace>;

        type OperandRefManager = NoOpRefManager<NullOperand>;

        fn init() {
            INIT.call_once(|| {
                let mut guard = BACKEND.lock().unwrap();
                let backend = CounterBackend::default();
                *guard = Some(backend);
            });
        }

        fn deinit() {}

        fn perform_on_backend<T>(action: impl for<'a> FnOnce(&'a mut Self::Backend) -> T) -> T {
            let mut guard = BACKEND.lock().unwrap();
            let backend = guard.as_mut().expect("Runtime is not initialized.");
            action(backend)
        }

        #[allow(static_mut_refs)]
        fn perform_on_place_ref_manager<T>(
            action: impl FnOnce(&mut Self::PlaceRefManager) -> T,
        ) -> T {
            action(unsafe { &mut PLACE_REF_MANAGER })
        }

        #[allow(static_mut_refs)]
        fn perform_on_operand_ref_manager<T>(
            action: impl FnOnce(&mut Self::OperandRefManager) -> T,
        ) -> T {
            action(unsafe { &mut OPERAND_REF_MANAGER })
        }
    }
}
```

### Exporting a PRI

The final step in the backend crate is to define the PRI implementation that the flavor exports through Leaf's C ABI.

```rust
pub mod interface {
    use leaf_runtime::pri::fluent::FluentPri;

    type CounterPri = FluentPri<super::instance::CounterInstanceManager>;

    leaf_runtime::make_late_init_pri_of!(CounterPri);

    pub type DefaultPri = CounterPriLateInit;
}
```

## Step 4: Package as a Dynamic Library

To publish the backend as `libleafrt.so`, which can be loaded by an instrumented program, define a *flavor* as a separate crate and use the project template.

For this tutorial, copy an existing flavor under `runtime/flavors` and point its `backend` dependency at the backend crate in `Cargo.toml`.

```toml
[package]
name = "runtime_counter"
license = { workspace = true }
version = { workspace = true }
edition = "2021"

[lib]
name = "leafrt_counter"
crate-type = ["cdylib"]

[dependencies]
common = { workspace = true }
backend = { path = "../../backends/counter", package = "runtime_backend_counter" }
```

The flavor's `build.rs` sets the shared library's SONAME to `libleafrt.so`, and its `src/lib.rs` exports the backend through the common FFI template:

```rust
include!("../shared_build.rs");

fn main() {
    set_so_name();
}
```

```rust
type PriImpl = backend::interface::DefaultPri;

include!("../../ffi_template.rs");
```

### Step 5: Run and inspect output
Build the flavor from the repository root:

```console
$ cargo build -p runtime_counter
```
Follow the same steps as in the [other tutorial](./first_dynamic_analysis.md) to load the dynamic library.

Now compile and run the instrumented sample with `leafc`:

For example, instrument the following program.
```rust
# fn main() {
let mut x: i8 = core::hint::black_box(20);

if x < 5 {
    x += 1;
} else {
    x -= 1;
}
x = -x;

core::hint::black_box(x);
# }
```

Save the example as `counter_sample.rs`, then run:

```console
$ leafc counter_sample.rs
$ ./counter_sample
```
<details>
<summary>MIR</summary>

```txt
fn main() -> () {
    let mut _0: ();
    let mut _1: i8;
    let mut _2: bool;
    let mut _3: i8;
    let mut _4: i8;
    let _5: i8;
    scope 1 {
        debug x => _1;
        scope 3 (inlined std::hint::black_box::<i8>) {
            debug dummy => _1;
        }
    }
    scope 2 (inlined std::hint::black_box::<i8>) {
        debug dummy => const 20_i8;
    }

    bb0: {
        _1 = std::intrinsics::black_box::<i8>(const 20_i8) -> [return: bb4, unwind unreachable];
    }

    bb1: {
        StorageDead(_3);
        _1 = Add(copy _1, const 1_i8);
        goto -> bb3;
    }

    bb2: {
        StorageDead(_3);
        _1 = Sub(copy _1, const 1_i8);
        goto -> bb3;
    }

    bb3: {
        StorageDead(_2);
        StorageLive(_4);
        _4 = copy _1;
        _1 = Neg(move _4);
        StorageDead(_4);
        StorageLive(_5);
        _5 = std::intrinsics::black_box::<i8>(move _1) -> [return: bb5, unwind unreachable];
    }

    bb4: {
        StorageLive(_2);
        StorageLive(_3);
        _3 = copy _1;
        _2 = Lt(move _3, const 5_i8);
        switchInt(move _2) -> [0: bb2, otherwise: bb1];
    }

    bb5: {
        StorageDead(_5);
        return;
    }
}
```

</details>

Then run it with the counter backend. For this input, the output is similar to:
```txt
total assignment: 7
binary: {Lt: 1, Sub: 1}
unary: {Neg: 1}
```