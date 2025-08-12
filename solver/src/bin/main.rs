use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;

use leafsolver::{
    Config, Constraint, Context, SolveResult,
    backend::z3::{AstAndVars, AstNode, Z3Solver, serdes::SmtLibExpr},
    format::{ModelFormat, OutputFormat, SolverOutput},
};

#[derive(Parser, Debug)]
#[command(name = "leafsolver")]
#[command(version = "0.2.0")]
#[command(about = "Leaf SMT Solver - solves constraints from JSONL files")]
struct Args {
    /// Input JSONL file with constraints
    #[arg(short, long, default_value = "sym_decisions.jsonl")]
    input: PathBuf,
    /// Output JSON file with results  
    #[arg(short, long, default_value = "solver_result.json")]
    output: PathBuf,
    /// Output format for the model
    #[arg(long, default_value = "standard")]
    format: OutputFormat,
}

fn main() -> ExitCode {
    let args = Args::parse();

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    println!("Reading constraints from: {}", args.input.display());
    println!("Writing results to: {}", args.output.display());

    let constraint_entries = read_constraint_entries(&args.input)?;
    let constraint_count = constraint_entries.len();
    println!("Loaded {} constraint entries", constraint_count);

    if constraint_entries.is_empty() {
        let output = SolverOutput {
            result: "unsat".to_string(),
            model: None,
        };
        write_result(&args.output, &output)?;
        println!("No constraints found - wrote UNSAT result");
        return Ok(());
    }

    let constraints = constraint_entries;
    let output = solve_constraints(constraints, args.format)?;

    write_result(&args.output, &output)?;

    match output.result.as_str() {
        "sat" => println!(
            "✓ SAT - Solution found and written to {}",
            args.output.display()
        ),
        "unsat" => println!("✗ UNSAT - No solution exists"),
        "unknown" => println!("? UNKNOWN - Could not determine satisfiability"),
        _ => unreachable!("Unexpected result: {}", output.result),
    }

    Ok(())
}

fn read_constraint_entries(
    filename: &Path,
) -> Result<Vec<Constraint<SmtLibExpr, SmtLibExpr>>, Box<dyn std::error::Error>> {
    if !filename.exists() {
        return Err(format!("Input file '{}' not found", filename.display()).into());
    }

    let file = File::open(filename)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        // Parse JSON and extract only the constraint field (ignore step info)
        let mut json_value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(e) => {
                return Err(format!("Error parsing JSON on line {}: {}", line_num + 1, e).into());
            }
        };

        let constraint_value = json_value
            .as_object_mut()
            .and_then(|obj| obj.remove("constraint"))
            .ok_or_else(|| format!("Missing 'constraint' field on line {}", line_num + 1))?;

        match serde_json::from_value::<Constraint<SmtLibExpr, SmtLibExpr>>(constraint_value) {
            Ok(constraint) => entries.push(constraint),
            Err(e) => {
                return Err(
                    format!("Error parsing constraint on line {}: {}", line_num + 1, e).into(),
                );
            }
        }
    }

    Ok(entries)
}

fn solve_constraints(
    constraints: Vec<
        Constraint<
            leafsolver::backend::z3::serdes::SmtLibExpr,
            leafsolver::backend::z3::serdes::SmtLibExpr,
        >,
    >,
    format: OutputFormat,
) -> Result<SolverOutput, Box<dyn std::error::Error>> {
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, String> = Z3Solver::new(&context);

    // Convert serializable constraints to internal format
    let mut variable_map: HashMap<String, AstNode> = HashMap::new();
    let internal_constraints: Vec<Constraint<AstAndVars<'_, String>, AstNode<'_>>> = constraints
        .into_iter()
        .map(|c| convert_constraint(&context, c, &mut variable_map))
        .collect::<Result<Vec<_>, _>>()?;

    println!(
        "Solving {} constraints with {} variables",
        internal_constraints.len(),
        variable_map.len()
    );

    let result = solver.check(internal_constraints.into_iter());
    match result {
        SolveResult::Sat(model) => {
            let model_format = match format {
                OutputFormat::Standard => {
                    let serializable_model: HashMap<
                        String,
                        leafsolver::backend::z3::serdes::SmtLibExpr,
                    > = model
                        .into_iter()
                        .map(|(id, ast_node)| {
                            let expr = leafsolver::backend::z3::serdes::Expr {
                                sort: ast_node.sort(),
                                smtlib_rep: ast_node.to_smtlib2(),
                            };
                            (id, expr.into())
                        })
                        .collect();
                    ModelFormat::Standard(serializable_model)
                }
                OutputFormat::Bytes => {
                    let bytes_model: Result<HashMap<String, u8>, Box<dyn std::error::Error>> =
                        model
                            .into_iter()
                            .map(|(id, ast_node)| {
                                let id_clone = id.clone();
                                convert_ast_node_to_byte(&ast_node)
                                    .map(|byte_val| (id, byte_val))
                                    .map_err(|e| {
                                        format!("Error converting variable {}: {}", id_clone, e)
                                            .into()
                                    })
                            })
                            .collect();
                    ModelFormat::Bytes(bytes_model?)
                }
            };

            Ok(SolverOutput {
                result: "sat".to_string(),
                model: Some(model_format),
            })
        }
        SolveResult::Unsat => Ok(SolverOutput {
            result: "unsat".to_string(),
            model: None,
        }),
        SolveResult::Unknown => Ok(SolverOutput {
            result: "unknown".to_string(),
            model: None,
        }),
    }
}

/// Converts serialized constraints from JSONL format to live Z3 constraint objects.
///
/// This conversion is needed because:
/// - `SmtLibExpr` contains serialized constraint data (strings, metadata) that can be stored in files
/// - `AstAndVars`/`AstNode` are live Z3 AST objects bound to a specific Z3 Context
/// - Z3Solver.check() requires live Z3 objects, not serialized strings
fn convert_constraint<'ctx>(
    context: &'ctx Context,
    constraint: Constraint<
        leafsolver::backend::z3::serdes::SmtLibExpr,
        leafsolver::backend::z3::serdes::SmtLibExpr,
    >,
    variable_map: &mut HashMap<String, AstNode<'ctx>>,
) -> Result<Constraint<AstAndVars<'ctx, String>, AstNode<'ctx>>, Box<dyn std::error::Error>> {
    // Parse the discriminant (the expression being constrained)
    let discr = constraint.discr.parse(context, variable_map);

    // Convert the constraint kind
    let kind = match constraint.kind {
        leafsolver::ConstraintKind::True => leafsolver::ConstraintKind::True,
        leafsolver::ConstraintKind::False => leafsolver::ConstraintKind::False,
        leafsolver::ConstraintKind::OneOf(cases) => {
            let parsed_cases: Result<Vec<_>, _> = cases
                .into_iter()
                .map(|case| {
                    case.parse_as_const(context)
                        .ok_or_else(|| "Case contains variables - not supported as constant")
                })
                .collect();
            leafsolver::ConstraintKind::OneOf(parsed_cases?)
        }
        leafsolver::ConstraintKind::NoneOf(cases) => {
            let parsed_cases: Result<Vec<_>, _> = cases
                .into_iter()
                .map(|case| {
                    case.parse_as_const(context)
                        .ok_or_else(|| "Case contains variables - not supported as constant")
                })
                .collect();
            leafsolver::ConstraintKind::NoneOf(parsed_cases?)
        }
    };

    Ok(Constraint { discr, kind })
}

/// Converts a Z3 AST node to a byte value
/// Only supports u8 bit vectors - errors for other types
fn convert_ast_node_to_byte(ast_node: &AstNode) -> Result<u8, Box<dyn std::error::Error>> {
    match ast_node {
        AstNode::BitVector(bv_node) => {
            let bv = &bv_node.0;
            let size = bv.get_size();

            if size != 8 {
                return Err(format!(
                    "Expected 8-bit value for byte conversion, got {}-bit value",
                    size
                )
                .into());
            }

            match bv.as_u64() {
                Some(value) if value <= u8::MAX as u64 => Ok(value as u8),
                Some(value) => {
                    Err(format!("Value {} is too large for u8 (max: {})", value, u8::MAX).into())
                }
                None => Err("Could not convert bit vector to integer".into()),
            }
        }
        _ => Err(format!(
            "Cannot convert {:?} to byte - only BitVector supported",
            ast_node.sort()
        )
        .into()),
    }
}

fn write_result(filename: &Path, result: &SolverOutput) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(result)?;
    let mut file = File::create(filename)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}
