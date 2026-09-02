# How Leaf Builds a Compatible Compiler Toolchain[^ai-content-note]

Leaf instruments MIR from the program being analyzed. For that instrumentation
to work consistently, the compiler must also be able to **emit and use MIR** for
the Rust libraries that the program depends on, including `core` and `std`.
The standard libraries supplied by rustup are compiled for ordinary Rust
compilation; they are not necessarily compiled with the options Leaf needs.
`leafc` therefore builds a separate, Leaf-compatible sysroot when the current
one is not already suitable.

## When and why Leaf builds a sysroot

For each original Rust toolchain, Leaf builds one compatible toolchain when a
previously built compatible toolchain is not already available. Codegen-all-MIR
mode is enabled by default, and in this mode `leafc` checks the
compiler's current sysroot during compiler configuration. The flow is:

1. If the current sysroot contains `.leafc_toolchain`, it is treated as a
	 Leaf-built sysroot and used as-is.
2. Otherwise, `leafc` looks through the persistent generated-toolchain cache
	 for an artifact whose marker identifies the same original sysroot.
3. If no compatible artifact is found, `leafc` invokes the toolchain builder.
4. The resulting path is installed as the compiler's sysroot.

## How the build works

In essence, the builder rebuilds the Rust sysroot and copies the build results
into a separate directory. The generated toolchain is then selected
automatically for the compilation that needs it.

> [!TIP]
> If a cached toolchain produces unexpected errors, stop any Leaf compilation and
remove the generated toolchain directory, or remove the relevant child
directory inside it. The next compilation will rebuild the sysroot from the
current original sysroot. Removing this cache does not remove or modify the
original rustup toolchain.

## Runtime shim

The runtime shim is needed by instrumented code and is included in the
compatible toolchain automatically. Users normally do not need to choose where
it is built or loaded.

## Important paths

- `leafc_toolchains`: the persistent cache beside the `leafc` executable.
- `.leafc_toolchain`: a marker in a generated toolchain that records the path
	to the original sysroot used to build it.
- `toolchain_builder/build`: the builder script invoked by `leafc`.
- `scripts/toolchain_builder/res/crate_template`: templates for the temporary
	dummy crate and its Cargo configuration.
- `leafc/toolchain_builder/<id>`: the temporary work directory when the crate
	output directory is used; otherwise the work directory is under the system
	temporary directory.

## For contributors

### The original and generated sysroots

It is useful to distinguish two paths:

- The **original sysroot** is the Rust toolchain selected by rustup or supplied
	to the compiler. It provides the Rust sources and tools from which the build
	starts.
- The **generated Leaf sysroot** is a separate directory containing the
	libraries rebuilt for Leaf. `leafc` installs this path as the sysroot for the
	compilation that requested it.

The generated directory is not the original rustup installation modified in
place. This keeps the user's toolchain reusable and gives Leaf a clear place
to cache its artifacts.

The compatibility check canonicalizes both paths before comparing them. This
means that the marker is not a general description of the generated toolchain:
it stores the path to the **original sysroot** from which that generated
toolchain was built. The marker lets a later invocation recognize that
relationship.

This check is skipped while the builder is compiling the standard library.
That exception is controlled by `LEAFC_BUILDING_CORE`, which becomes the
`building_core` compiler configuration flag. Without this recursion guard,
the `leafc` used by Cargo to build `core` could try to build or select another
Leaf sysroot while the current one was still being assembled.

### Build details

The Rust side locates `toolchain_builder/build`, creates a fresh work directory,
and starts the Python script with the original sysroot and output paths. A work
directory is placed below `leafc/toolchain_builder` under the crate output
directory when one is available; otherwise it is placed under the system
temporary directory. The output directory is created beside the `leafc`
executable under `leafc_toolchains`.

Cargo's unstable `-Zbuild-std` support builds the standard library as part of a
crate build. The Python builder creates a small dummy crate from its templates,
then uses Cargo to:

1. ask Cargo for the sysroot selected for the build;
2. build `core`, `std`, panic support, and test support with
	 `-Zbuild-std=core,std,panic_unwind,panic_abort,test`; and
3. collect the resulting libraries from the dummy crate's target directory.

The dummy crate gives Cargo the dependency graph and build context it needs;
it is not part of the generated sysroot. The builder copies the resulting
`lib*.r*` files, excluding the dummy crate's own library, into the
target-specific Rust library directory.

The current builder uses `x86_64-unknown-linux-gnu` and the `release` profile
for this process. Those are implementation constraints of this builder, not
universal requirements of Leaf. A request using another target or profile can
therefore fail or produce artifacts that this builder does not know how to
assemble.

### Runtime shim placement

The runtime shim is needed by instrumented code. The current default is the
**external-dependency mode**: the dummy crate depends on the `runtime_shim`
package, and the generated build places the shim alongside the other built
dependencies. The builder checks the dummy crate's dependency tree in this
mode so that unexpected transitive dependencies do not silently become part
of the sysroot.

There is also an **internal core-patching mode**. In that mode the builder
copies the original toolchain to its work area, copies Leaf's `common` and
runtime-shim sources into the Rust `core` sources, and applies Leaf's core
patches before building. The templates select `core_build` and the in-core shim
location for this path. This mode is deprecated and is not planned to be
maintained; it remains relevant when interpreting existing configuration or
diagnosing an older build.

The `leafc` process used for this Cargo build must understand that it is
building the standard library. The `LEAFC_BUILDING_CORE` setting is the
recursion guard that separates this bootstrap work from normal application
compilation.

### Assembly and caching

After Cargo succeeds, the Python builder creates an output directory with the
sysroot library layout and copies the built libraries into it. It then writes
`.leafc_toolchain` containing the original sysroot path. Finally, the Rust
side prints the output path, removes the temporary work directory, and tries
to move the artifact into the persistent `leafc_toolchains` directory beside
`leafc`.

The persistent directory name is derived from the `libcore` artifact found
under the requested target's library directory. The target triple is used to
find that artifact, so the target participates in the cache identity and
prevents a target's `libcore` from being mistaken for another target's during
assembly. The current cache lookup itself first filters for non-empty
directories and then checks the original-sysroot marker; when maintaining this
code, remember that the target-specific identity and the marker compatibility
check are separate mechanisms.

If two compiler processes build the same artifact concurrently, an existing
persistent destination wins when the Rust side reaches it first. If persistence
fails, the freshly generated output can still be returned for the current
compilation, with a warning logged.

### Diagnosing failures

The failure usually belongs to one of a few boundaries:

- **Builder discovery or process startup:** `leafc` could not find or execute
	the builder. Check the compiler log and the resolved builder location.
- **Rust source availability:** the original toolchain must include the Rust
	source component, because the builder needs `core` sources. A missing source
	directory is reported before the core build begins.
- **Cargo and toolchain selection:** `-Zbuild-std` depends on the nightly
	toolchain and its Cargo support. Inspect the builder log for the exact Cargo
	command and selected sysroot.
- **Shim configuration:** external mode expects the shim dependency in the
	configured dependency location, while internal mode expects the copied core
	sources and patches to apply cleanly.
- **Artifact assembly:** a missing `libcore` file, an unexpected target/profile
	directory, or an incomplete Cargo build prevents the output from becoming a
	usable cached artifact.
- **Cache compatibility:** a generated artifact is associated with the
	original path recorded in `.leafc_toolchain`. Changing or removing that
	toolchain, or relying on a path that no longer canonicalizes, can make a
	previously cached artifact unusable.

For environment variables, logging controls, manual invocation details, and
the names of the builder inputs, see the [toolchain builder README](https://github.com/sfu-rsl/leaf/blob/main/scripts/toolchain_builder/README.md).



[^ai-content-note]: This page was created with AI assistance under the supervision and review of the maintainers.