# Samples Reference

## Map

Here is a map of sample programs put under `samples` in the work tree of the project.

| Sample | Targets |
|---|---|
| `assignment/addr_of` | `Rvalue::RawPtr` |
| `assignment/aggregate` | `Rvalue::Aggregate` |
| `assignment/bin_op` | `Rvalue::BinaryOp` |
| `const` | `Operand::Constant::*` |
| `assignment/discr` | `Rvalue::Discriminant` |
| `assignment/ref` | `Rvalue::Ref` |
| `assignment/repeat_array` | `Rvalue::Repeat` |
| `assignment/set_discr` | `StatementKind::SetDiscriminant` |
| `assignment/thread_local_ref` | `Rvalue::ThreadLocalRef` |
| `assignment/un_op` | `Rvalue::UnaryOp` |
| `branching/assert` | `TerminatorKind::Assert` |
| `branching/if_basic` | `TerminatorKind::SwitchInt`, `if` |
| `branching/if_else` | `TerminatorKind::SwitchInt`, `if`, `else if`, `else` |
| `branching/if_let` | `TerminatorKind::SwitchInt`, `if let` |
| `branching/match_basic` | `TerminatorKind::SwitchInt`, `match` |
| `branching/match_enum` | `TerminatorKind::SwitchInt`, `Rvalue::Discriminant`, `match <enum>` |
| `casting/numeric` | `Rvalue::Cast`, `CastKind::IntTo*`, `CastKind::FloatTo*` |
| `casting/pointer` | `Rvalue::Cast`, `CastKind::PtrToPtr`, `CastKind::PointerCoercion`, `PointerCoercion::*` |
| `casting/subtype` | `Rvalue::Cast`, `CastKind::Subtype` |
| `casting/transmute` | `Rvalue::Cast`, `CastKind::Transmute` |
| `drop` | `TerminatorKind::Drop`, `intrinsics::drop_glue` |
| `function/async` | Async Functions, `TyKind::CoroutineClosure` |
| `function/call_basic` | `TerminatorKind::Call` |
| `function/closures` | `TyKind::Closure`, `Fn*` traits, tupling/untupling arguments |
| `function/coroutines` | `TyKind::Coroutine` |
| `function/shims` | `ShimKind` |
| `intrinsics/atomic` | `intrinsics::atomic_*` |
| `intrinsics/memory` | (Raw) Memory-related intrinsics |
| `intrinsics/operators` | Intrinsic (arithmetic) operators |
| `misc/intrinsics` | Misc intrinsic usage |
| `misc/leaf_attr` | Using Leaf-specific attributes, `#[leaf_attr::instrument]` |
| `misc/no_diverge` | Pushing/popping tags |
| `misc/promoted` | Promoted bodies |
| `misc/static` | Static items and accesses |
| `place/deref_mut` | Dereferencing mutable references |
| `place/projection/downcast` | `PlaceElem::Downcast` |
| `place/projection/field` | `PlaceElem::Field` |
| `place/projection/index` | `PlaceElem::Index` |
| `place/projection/unwrap_unsafe_binder` | `PlaceElem::UnwrapUnsafeBinder` |
|||
| `sym_place/read` | #SymEx Reading symbolic places |
| `sym_place/write` | #SymEx Writing to symbolic places |
| `function/sym_*` | #SymEx Symbolic values transferred between functions |
| `basic` | Basic algorithm implementations |
| `crates/multi_file_bin` | Multi-file crate compilation |
| `crates/single_file_bin` | Single-file crate baseline |
| `crates/with_dep` | Crate with dependencies |
| `crates/with_shared_dep` | Crate with shared transitive dependency |