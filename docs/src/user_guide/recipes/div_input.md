# Diverging Input Generation

Each instance of concolic execution of a program, records a trace of the constraints put on symbolic variables at each
step of the execution.
Conditional branches are the major source of these constraints and whether they are held or not
determines the target of the branch.
Therefore, concolic execution can be used to find concrete values for the symbolic variables
such that the execution diverges at conditional branches compared to the previous execution.
We refer to these concrete values as diverging inputs, counterexamples, or generally answers.

The current default configuration of Leaf tries to find a diverging input whenever
a new constraint is observed.
For instance, in the following program, the input (`x = 10`) does not satisfy
the branch condition (`x < 5`), which the backend reports as `{!(<(<Var1: u8>, 5u8))}`.
It tries to find an input that would satisfy it (so the execution would diverge at this point),
and reports back value `0u8` as a possible one.

## Diverging Standard Input Generation

If all symbolic variables are from `u8`, the default configuration puts the found answers
in binary files in which each byte corresponds to a symbolic variable ordered by
when they were marked as symbolic.

This is mainly meant for situations where we want to mark a file symbolic, e.g., standard input.

Although the backend (and execution of the instrumented program) is enough to obtain the diverging standard input,
the one-time orchestrator is provided to facilitate this process with further control.

You can install it by running the following command in Leaf's root folder.
```console
leaf$ cargo install --path ./orchestrator
```
Then you provide the path to the instrumented program and the desired path to put the diverging inputs at.
For example,
```console
$ leafo_onetime --program ./hello_world --outdir ./next
next/diverging_0.bin
```
It runs the target program, and prints the names of the files generated as diverging input.
