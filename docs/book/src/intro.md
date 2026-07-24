# Introduction

Leaf is a framework for dynamic analysis of Rust programs built around MIR instrumentation. Instead of analyzing source code directly, Leaf compiles a program through `leafc`, rewrites the relevant MIR, and runs the resulting binary with a runtime backend attached.

That split keeps the responsibilities clear:

- the compiler frontend prepares the program for analysis,
- the runtime receives execution events and records or reacts to them, and
- higher-level tooling can orchestrate repeated runs or more advanced workflows.

At a high level, the workflow looks like this:

1. write or choose a Rust program,
2. compile it with Leaf’s instrumentation pipeline,
3. run the instrumented binary with the desired backend, and
4. inspect the emitted traces or analysis results.

This book starts with a practical getting-started guide, then moves into recipes and configuration details for common workflows. Use it as a reference when you need to understand how Leaf’s compiler, runtime, and orchestration pieces fit together.