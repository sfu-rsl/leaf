# Compiler Configuration[^ai-content-note]

This page describes the configuration surface of `leafc`: where configuration
comes from, which compiler passes run, and how instrumentation rules select MIR
entities. It also documents the settings behind Leaf's standard compiler setup
for codegen, sysroot compatibility, and the runtime shim.

## Pass configuration

Each configurable pass is configured in its own `passes.<pass_name>` table. At
minimum, every such table has an `enabled` flag, which defaults to `true` and
enables or disables that pass. Some pass tables have additional fields;
`instrumentation` and `internalization` provide a `rules` field.

| Pass | Additional configuration |
| --- | --- |
| `instrumentation` | `rules` selects MIR entities for instrumentation. |
| `instrumentation_counter` | none |
| `instrumentation_rec_check` | none |
| `internalization` | `rules` selects functions for internalization. |
| `program_map` | none |
| `program_dep` | none |
| `type_export` | none |
| `md_info` | none |

For example, a pass can be disabled as follows and it won't run during compilation:

```toml
[passes.instrumentation]
enabled = false
```

### Instrumentation rules

Each rule is an `include` or `exclude` entry with an `entity` tag. The
available entities and aliases are:

`include` and `exclude` are parallel lists. If any include rule matches an
entity, it is included and takes precedence over every matching exclude rule.
Otherwise, a matching exclude rule excludes it; if neither list matches, the
rules provide no decision and the pass's normal behavior is retained.

| Canonical `entity` | Aliases | Entity-specific field |
| --- | --- | --- |
| `whole_body` | `body` | none |
| `method_dyn_definition` | `dyn_def` | none |
| `place_info` | none | `kind`, then `piece` for `structure` |
| `operand_kind` | none | `kind` |
| `constant_type` | `const_type` | `ty` or alias `type` |
| `assignment` | none | `kind` |
| `assignment_info` | none | `kind` |
| `storage_lifetime_marker` | `storage_lifetime`, `lifetime_marker` | `kind` |
| `call_flow` | none | `kind` |
| `drop` | none | `kind` |
| `switch` | none | `kind` |

Every instrumentation filter has two parts: criteria for the selected entity,
such as `kind` or `piece`, and location criteria named `loc` that describe the
location of the function MIR body being instrumented. `loc` is a field in the configuration
schema, but it is flattened when reading TOML, so its predicates appear next to
the other criteria rather than under a literal `loc` key. The conceptual shape
is:

```text
entity-specific criteria + loc: location expression
```

The TOML form is:

```toml
[[passes.instrumentation.rules.exclude]]
entity = "assignment"
kind = "binary_op"
crate = { name = "my_app" }
```

Here, `kind` is the entity-specific criterion and `crate` is part of `loc`.

The accepted location values are:

| Location field | Values | Description |
| --- | --- | --- |
| `crate.name` | regex string | The [name](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/context/struct.TyCtxt.html#method.crate_name) of the parent crate of the body. |
| `crate.is_external` | `true` or `false` | Whether the parent crate of the body is external (a dependency) |
| `def_path` (alias `def_path_match`) | regex string | [MIR's `DefPath`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/context/struct.TyCtxt.html#method.def_path) of the body. |
| `def_id` | `"crate-number:index"` | [MIR's `DefId`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefId.html) of the function body.  |

The accepted entity-specific values are:

<!-- FIXME: Add the corresponding MIR element in the table. -->

| Entity kind | Field | Values | Expressions |
| --- | --- | --- | --- |
| `place_info` | `kind` | `structure` (alias `struct`), `address` (alias `addr`), `type` (alias `ty`) | no |
| `place_info` | `piece` | `local`, `deref`, `field`, `index`, `constant_index`, `subslice`, `downcast`, `opaque_cast`, `unwrap_unsafe_binder` | yes |
| `operand_kind` | `kind` | `copy`, `move`, `constant` (alias `const`) | yes |
| `constant_type` | `ty` | `bool`, `char`, `int`, `float`, `str`, `byte_str`, `ptr`, `zst` | yes |
| `assignment` / `assignment_info` | `kind` | `use`, `repeat`, `ref`, `thread_local_ref`, `raw_ptr`, `cast`, `binary_op`, `unary_op`, `discriminant`, `aggregate`, `wrap_unsafe_binder`, `intrinsic_unary_op`, `intrinsic_binary_op`, `intrinsic_ternary_op`, `intrinsic_misc_op`, `intrinsic_memory_op`, `atomic_binary_op`, `atomic_memory_op` | yes |
| `storage_lifetime_marker` | `kind` | `live`, `dead` | yes |
| `call_flow` | `kind` | `call_control`, `call_address` (alias `call_addr`), `call_input`, `function_address` (alias `func_addr`), `function_data` (alias `func_data`) | yes |
| `drop` | `kind` | `call_control`, `call_address` (alias `call_addr`), `call_input` | yes |
| `switch` | `kind` | `control`, `data` | yes |

### Writing rule expressions

Where supported, a field can be a boolean expression rather than a single
value. The tables above mark the fields that support expressions. An atomic
value is a string, enum value, boolean, or nested table accepted by the field.
An expression can also be a table with a single field of `not`, `any`, or
`all`:

```toml
[[passes.instrumentation.rules.exclude]]
entity = "operand_kind"
kind = { any = ["move", "copy"] }
all = [
    { crate = { is_external = true } },
    { not = { crate = { name = "std" } } },
]
```

`any` is true when at least one child is true; `all` requires every child;
`not` negates one child. An empty formula table, `{}`, is a valid empty formula
and evaluates false. This is useful when a rule intentionally supplies no
location restriction, as in `all = []`: the `all` formula itself is true, but
an empty atomic formula `{}` is false. Keep the distinction explicit in
configuration.

Invalid regex syntax is an error when the rules are converted for use.

It's worth mentioning that two top-level aliases, `instr_rules` and `rules`, are available for
`passes.instrumentation.rules`. While we do not guarantee stability over this shortcut, it is important that you do not provide configurations in both locations as it is an error.

## Advanced settings

Leaf has internal procedures for instrumenting the standard library and linking
the runtime shim. The defaults below are the recommended settings for that
workflow. Customize them when you need to control how Leaf builds or selects a
compatible toolchain, uses a different runtime shim, or changes codegen
behavior.

### Compiler marker

`marker_cfg_name` defaults to `"leafc"` and adds that cfg to the crate it compiles. Set it
to another valid cfg name to use a different marker, or to `""` to omit the
marker.

<!-- FIXME: Explain in a separate page and just provide a link here. -->
For example, the default marker lets a sample select code that is compiled
only by `leafc`:

```rust
#[cfg(leafc)]
let x: u8 = {
  use leaf::annotations::*;
  x.mark_symbolic()
};
```

### Runtime shim

The default is equivalent to:

```toml
[runtime_shim.location.external]
crate_name = "leaf"
search_path = "sysroot"
```

Use the core library as the shim with either spelling:

```toml
[runtime_shim]
location = "core_lib" # `core` is an alias
```

For an external shim, `crate_name` defaults to `"leaf"`. `search_path` accepts:

| Value | Meaning |
| --- | --- |
| `"sysroot"` or `"default"` | Find the crate through the normal library search paths. This is the default and is intended for Leaf-compatible sysroots. |
| `"crate_deps"` or `"deps"` | Treat the shim as a normal dependency found in the crate's sysroot or supplied search paths. |
| `"compiler"` | Use Leaf's compiler-side `runtime_shim/libleafrtsh.rlib` and its adjacent `deps` directory. |
| `{ exact = "..." }` or `{ exact = "/absolute/path/to/libleafrtsh.rlib" }` | Use an exact runtime-shim library path and its adjacent dependency directory. `"exact"` is also the enum alias, but the value must be a path string. |

For example:

```toml
[runtime_shim.location.external]
crate_name = "my_leaf_shim"
search_path = "deps"
```

The external form adds the configured crate as a forced prelude dependency;
the core-library form expects the shim to be part of `core` and adds no
external dependency.

### Sysroot and codegen

| Key | Default | Effect |
| --- | --- | --- |
| `codegen_all_mir` | `true` | Enables MIR/codegen for all items, i.e., including items from upstream dependencies. It also forces one codegen unit, disables LTO when necessary, and uses unwind panic strategy when required. |
| `override_sysroot` | `true` | Allows codegen-all mode to find or build a Leaf-compatible sysroot when the current sysroot is incompatible. |
| `building_core` | `false` | Identifies a core-library build. Used when building a compatible toolchain. See [How Leaf Builds a Compatible Compiler Toolchain](../../explanations/dependencies.md). |

`codegen_all_mir` and `override_sysroot` are usually left at their defaults.

`codegen_all_mir = false` selects the non-codegen-all pipeline. However,
`runtime_shim.location = "external"` with `search_path = "sysroot"` requires
codegen-all mode and enables it with a warning. When codegen-all mode is on,
Leaf may override the current sysroot unless `override_sysroot` is false.
Disabling that override is intended for special build scenarios and can cause
errors if the sysroot does not contain MIR for all required libraries.

### Selecting functions for internalization

The internalization pass is configured under `passes.internalization`. Its
`enabled` flag controls the pass, and its `rules` select which generated
functions are made internal. Each include or exclude entry matches a generated
function's symbol name with a regex and can use the same boolean expression
forms as instrumentation.

```toml
[passes.internalization.rules]
include = [
    { any = ["my_crate::public_api::.*", "leafrtsh::.*"] },
]
exclude = [
    { not = "my_crate::keep_exported" },
]
```

The same `not`, `any`, `all`, empty-formula, and regex rules apply. A matching
include selects the symbol for internalization. An unmatched symbol follows
the compiler's default of internalizing it unless it is a required public C
export, weak lang item, Rust compiler symbol, or proc-macro export.

## Loading and precedence

`leafc` relies on the [config](https://docs.rs/config/latest/config/) to load configurations. The sources are applied in this order:

1. schema defaults,
2. `leafc_config.<ext>` file: searched in the current working directory, the compiler executable's directory, including their ancestors in order,
3. an inline configuration string, and
4. `LEAFC` environment variables.

Later sources override earlier values.

### Inline configuration

An inline configuration is enabled only when both variables are set:

```sh
export LEAFC_CONFIG_STR=$'[passes.instrumentation]\nenabled = false'
export LEAFC_CONFIG_STR_FMT=toml
```

The format name may be a supported file extension: `toml`, `json`, `json5`,
`yaml`, `ron`, or `ini`. An unknown format is ignored with a warning.

### Environment variables

`LEAFC` is the environment-variable prefix used by `leafc`. The loader uses
`_` between the prefix and a top-level key, and `__` for nested keys. For
example:

```sh
export LEAFC_PASSES__INSTRUMENTATION__ENABLED=false
export LEAFC_MARKER_CFG_NAME=my_leafc_marker
```

The file and inline string use the `config` crate's normal source merging, so
the names and value types must match the schema above.

## Example

```toml
codegen_all_mir = true
override_sysroot = true
marker_cfg_name = "leafc"

[runtime_shim.location.external]
crate_name = "leaf"
search_path = "sysroot"

[passes.instrumentation]
enabled = true

[[passes.instrumentation.rules.exclude]]
entity = "whole_body"
crate = { is_external = true }

[[passes.instrumentation.rules.include]]
entity = "assignment"
kind = { any = ["binary_op", "atomic_binary_op"] }
crate = { name = "my_app" }

[passes.internalization]
enabled = true

[passes.internalization.rules]
exclude = ["LLVMFuzzerInitialize"]

[passes.md_info]
enabled = false
```

## Troubleshooting

- **`Failed to read configurations`**: check TOML syntax, environment value
  types, table nesting, and inline format. An inline string requires both
  `LEAFC_CONFIG_STR` and `LEAFC_CONFIG_STR_FMT`.
- **Unknown `search_path` value**: use `sysroot`/`default`, `crate_deps`/`deps`,
  `compiler`, or `{ exact = "..." }`. The external form also needs a
  `crate_name` if it is not `leaf`.
- **Runtime shim not found**: use `core_lib` only when the shim is in the core
  library; otherwise verify the selected library path and its adjacent `deps`
  directory.
- **Sysroot or MIR errors**: `sysroot` shim lookup and codegen-all mode are
  coupled. Use a Leaf-compatible sysroot, leave `override_sysroot = true`, or
  choose another shim location for a special non-codegen-all build.
- **Rules appear ignored**: use `passes.instrumentation.rules` for new files,
  and do not populate it together with non-empty top-level `instr_rules` or
  `rules`.
- **A rule fails to parse**: verify the entity tag, its field (`ty` versus
  `kind`), aliases, enum spelling, and formula shape. `def_id` must be a
  `crate-number:index` string and pattern values must be valid regexes.

## Implementation references

- [`common/src/config.rs`](/common/src/config.rs): file discovery, source precedence, inline formats,
  and environment naming.
- [`compiler/src/config.rs`](/compiler/src/config.rs): compiler schema, defaults, runtime-shim values,
  pass gates, formulas, and legacy-rule migration.
- [`compiler/src/passes/instr/config.rs`](/compiler/src/passes/instr/config.rs): instrumentation entities, aliases,
  fields, and enum values.
- [`compiler/src/passes/codegen.rs`](/compiler/src/passes/codegen.rs): internalization rule shape and behavior.

[^ai-content-note]: This page was created with AI assistance under the supervision and review of the maintainers.

[crate_name]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/context/struct.TyCtxt.html#method.crate_name
[def_path]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/context/struct.TyCtxt.html#method.def_path
[def_id]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefId.html