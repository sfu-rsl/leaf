# Leaf Solver

A standalone SMT solver crate built on Z3

## Example Usage

```rust
use leafsolver::{
    AstAndVars, AstNode, BVNode, Constraint, ConstraintKind, Z3Solver,
    Config, Context, SolveResult, ast, Ast
};

// Create a Z3 context and solver
let context = Context::new(&Config::new());
let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

// Create symbolic variables: a + b == 15, a == 10
let a = ast::BV::new_const(&context, "a", 32);
let b = ast::BV::new_const(&context, "b", 32);
let ten = ast::BV::from_i64(&context, 10, 32);
let fifteen = ast::BV::from_i64(&context, 15, 32);

let sum = a.bvadd(&b);
let eq_constraint = sum._eq(&fifteen);  // a + b == 15
let a_eq_ten = a._eq(&ten);             // a == 10

let variables = vec![
    (1, AstNode::BitVector(BVNode::new(a, true))),
    (2, AstNode::BitVector(BVNode::new(b, true))),
];

let constraints = vec![
    Constraint {
        discr: AstAndVars { value: AstNode::Bool(eq_constraint), variables: variables.clone() },
        kind: ConstraintKind::True,
    },
    Constraint {
        discr: AstAndVars { value: AstNode::Bool(a_eq_ten), variables },
        kind: ConstraintKind::True,
    },
];

// Solve
let result = solver.check(constraints.into_iter());
match result {
    SolveResult::Sat(model) => {
        // Solver found: a = 10, b = 5
        println!("Solution found!");
    }
    SolveResult::Unsat => println!("No solution exists"),
    SolveResult::Unknown => println!("Could not determine"),
}
```

> See the [`tests/`](tests/) directory for more examples

## CLI

The solver provides a standalone binary for solving constraints from JSONL files

### Installation

From the root of the repository, run:

```bash
cargo install --path solver --bin leafsolver
```

### Usage

```bash
leafsolver [OPTIONS]
```

**Options:**
- `-i, --input <FILE>` - Input JSONL file with constraints (default: `sym_decisions.jsonl`)
- `-o, --output <FILE>` - Output JSON file with results (default: `solver_result.json`)
- `--format <FORMAT>` - Output format for the model: `standard` or `bytes` (default: `standard`)

**Output Formats:**
- `standard`: Full SmtLibExpr format with SMT-LIB representation
- `bytes`: Raw byte values only (u8)

> The binary is currently tested using constraint files generated from the `samples/` directory. The `test_data/` folder contains constraint files produced by running Leaf on sample programs. These files are used in the test suite.

Example usage with the `is_sorted` test case:
```bash
# Standard format (default)
leafsolver -i test_data/is_sorted_complex.jsonl -o result.json

# Bytes format for direct byte values
leafsolver -i test_data/hello_world_simple.jsonl -o result.json --format bytes
```

### Input Format

The binary expects JSONL format where each line contains a constraint entry:

```json
{
  "step": {
    "value": "0:4:2",
    "index": 3
  },
  "constraint": {
    "discr": {
      "decls": {
        "1": {
          "name": "k!1",
          "sort": {"BitVector": {"is_signed": false}},
          "smtlib_rep": "(declare-fun k!1 () (_ BitVec 8))"
        }
      },
      "sort": "Bool",
      "smtlib_rep": "(bvult k!1 #x05)"
    },
    "kind": "False"
  }
}
```

### Output Format

#### Standard Format (default)

```json
{
  "result": "sat",
  "model": {
    "1": {
      "decls": {},
      "sort": {"BitVector": {"is_signed": false}},
      "smtlib_rep": "#x08"
    }
  }
}
```

#### Bytes Format

```json
{
  "result": "sat",
  "model": {
    "1": 8
  }
}
```
> The bytes output format only supports 8-bit `BitVector` variables. Other sorts (Bool, Int, etc.) will cause an error.
> 
> Byte values must be in the range 0-255 (u8). Larger BitVectors are not supported in bytes format
