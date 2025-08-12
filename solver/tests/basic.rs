use std::collections::HashMap;

use leafsolver::{
    Ast, AstAndVars, AstNode, BVNode, Config, Constraint, ConstraintKind, Context, SolveResult,
    Z3Solver, ast,
};

#[test]
fn test_constraint_creation() {
    let constraint = Constraint::equality("x", 5);

    assert_eq!(constraint.discr, "x");
    match constraint.kind {
        ConstraintKind::OneOf(ref values) => {
            assert_eq!(values.len(), 1);
            assert_eq!(values[0], 5);
        }
        _ => panic!("Expected OneOf constraint"),
    }
}

#[test]
fn test_constraint_negation() {
    let constraint = Constraint::equality("x", 5);
    let negated = constraint.not();

    match negated.kind {
        ConstraintKind::NoneOf(ref values) => {
            assert_eq!(values.len(), 1);
            assert_eq!(values[0], 5);
        }
        _ => panic!("Expected NoneOf constraint after negation"),
    }
}

#[test]
fn test_constraint_kind_operations() {
    assert_eq!(
        ConstraintKind::<i32>::True.not(),
        ConstraintKind::<i32>::False
    );
    assert_eq!(
        ConstraintKind::<i32>::False.not(),
        ConstraintKind::<i32>::True
    );

    let one_of = ConstraintKind::OneOf(vec![1, 2, 3]);
    let negated = one_of.not();
    match negated {
        ConstraintKind::NoneOf(values) => {
            assert_eq!(values, vec![1, 2, 3]);
        }
        _ => panic!("Expected NoneOf after negating OneOf"),
    }
}

#[test]
fn test_solve_result_enum() {
    let sat_result: SolveResult<HashMap<i32, i32>> = SolveResult::Sat(HashMap::new());
    let unsat_result: SolveResult<HashMap<i32, i32>> = SolveResult::Unsat;
    let unknown_result: SolveResult<HashMap<i32, i32>> = SolveResult::Unknown;

    match sat_result {
        SolveResult::Sat(_) => (),
        _ => panic!("Expected Sat"),
    }

    match unsat_result {
        SolveResult::Unsat => (),
        _ => panic!("Expected Unsat"),
    }

    match unknown_result {
        SolveResult::Unknown => (),
        _ => panic!("Expected Unknown"),
    }
}

#[test]
fn test_z3_solver_basic() {
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    // Simple boolean constraint: true
    let true_ast = ast::Bool::from_bool(&context, true);
    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::Bool(true_ast),
            variables: vec![],
        },
        kind: ConstraintKind::True,
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            assert!(model.is_empty());
        }
        _ => panic!("Expected SAT result for true constraint"),
    }
}

#[test]
fn test_bitvector_constraint() {
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let x_var = ast::BV::new_const(&context, "x", 8);
    let eq_ast = x_var._eq(&x_var); // x == x (always true)

    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::Bool(eq_ast),
            variables: vec![(1, AstNode::BitVector(BVNode::new(x_var, false)))],
        },
        kind: ConstraintKind::True,
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            assert!(model.contains_key(&1));
            if let Some(AstNode::BitVector(_)) = model.get(&1) {
                // The only correct case
            } else {
                panic!("Expected BitVector solution for variable 1");
            }
        }
        _ => panic!("Expected SAT result for x == x constraint"),
    }
}
