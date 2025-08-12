use leafsolver::{
    Ast, AstAndVars, AstNode, BVNode, Config, Constraint, ConstraintKind, Context, SolveResult,
    Z3Solver, ast,
};

/// Tests based on real constraint patterns from `samples/` directory

#[test]
fn test_arithmetic_operations() {
    // Based on samples/assignment/bin_op/main.rs
    // Test: a + b == 15 where a = 10, should find b = 5
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let a = ast::BV::new_const(&context, "a", 32);
    let b = ast::BV::new_const(&context, "b", 32);
    let ten = ast::BV::from_i64(&context, 10, 32);
    let fifteen = ast::BV::from_i64(&context, 15, 32);

    let sum = a.bvadd(&b);
    let eq_constraint = sum._eq(&fifteen);
    let a_eq_ten = a._eq(&ten);

    let variables = vec![
        (1, AstNode::BitVector(BVNode::new(a, true))),
        (2, AstNode::BitVector(BVNode::new(b, true))),
    ];

    let constraints = vec![
        Constraint {
            discr: AstAndVars {
                value: AstNode::Bool(eq_constraint),
                variables: variables.clone(),
            },
            kind: ConstraintKind::True,
        },
        Constraint {
            discr: AstAndVars {
                value: AstNode::Bool(a_eq_ten),
                variables,
            },
            kind: ConstraintKind::True,
        },
    ];

    let result = solver.check(constraints.into_iter());

    match result {
        SolveResult::Sat(model) => {
            let a_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_i64(),
                    _ => None,
                })
                .unwrap_or(0);
            let b_val = model
                .get(&2)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_i64(),
                    _ => None,
                })
                .unwrap_or(0);

            assert_eq!(a_val, 10);
            assert_eq!(b_val, 5);
            assert_eq!(a_val + b_val, 15);
        }
        _ => panic!("Expected SAT result for arithmetic constraint"),
    }
}

#[test]
fn test_modulo_branching() {
    // Based on samples/branching/match_basic/main.rs
    // Test: x % 3 == 2
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let x = ast::BV::new_const(&context, "x", 32);
    let three = ast::BV::from_i64(&context, 3, 32);
    let two = ast::BV::from_i64(&context, 2, 32);

    let mod_result = x.bvsrem(&three);
    let eq_two = mod_result._eq(&two);

    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::Bool(eq_two),
            variables: vec![(1, AstNode::BitVector(BVNode::new(x, true)))],
        },
        kind: ConstraintKind::True,
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            let x_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_i64(),
                    _ => None,
                })
                .unwrap_or(0);

            assert_eq!(x_val % 3, 2, "Expected x % 3 == 2, got x = {}", x_val);
        }
        _ => panic!("Expected SAT result for modulo constraint"),
    }
}

#[test]
fn test_range_constraints() {
    // Based on samples/branching/match_range/main.rs
    // Test: x in range [1, 3] using OneOf constraint
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let x = ast::BV::new_const(&context, "x", 8);
    let one = ast::BV::from_u64(&context, 1, 8);
    let two = ast::BV::from_u64(&context, 2, 8);
    let three = ast::BV::from_u64(&context, 3, 8);

    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::BitVector(BVNode::new(x.clone(), false)),
            variables: vec![(1, AstNode::BitVector(BVNode::new(x, false)))],
        },
        kind: ConstraintKind::OneOf(vec![
            AstNode::BitVector(BVNode::new(one, false)),
            AstNode::BitVector(BVNode::new(two, false)),
            AstNode::BitVector(BVNode::new(three, false)),
        ]),
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            let x_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);

            assert!(
                x_val >= 1 && x_val <= 3,
                "Expected x ∈ [1, 3], got {}",
                x_val
            );
        }
        _ => panic!("Expected SAT result for range constraint"),
    }
}

#[test]
fn test_array_bounds_check() {
    // Based on samples/branching/assert/boundscheck.rs
    // Test: index < array_length for array access
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let index = ast::BV::new_const(&context, "index", 8);
    let array_len = ast::BV::from_u64(&context, 5, 8);

    let valid_constraint = index.bvult(&array_len);

    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::Bool(valid_constraint),
            variables: vec![(1, AstNode::BitVector(BVNode::new(index, false)))],
        },
        kind: ConstraintKind::True,
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            let index_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);

            assert!(index_val < 5, "Expected valid index < 5, got {}", index_val);
        }
        _ => panic!("Expected SAT result for bounds check constraint"),
    }
}

#[test]
fn test_overflow_detection() {
    // Based on samples/assignment/bin_op/with_overflow_ops/
    // Test: detect when a + b overflows u8
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let a = ast::BV::new_const(&context, "a", 8);
    let b = ast::BV::new_const(&context, "b", 8);

    let sum = a.bvadd(&b);
    // Overflow occurred if sum < a (wraparound happened)
    let overflow_condition = sum.bvult(&a);

    let variables = vec![
        (1, AstNode::BitVector(BVNode::new(a, false))),
        (2, AstNode::BitVector(BVNode::new(b, false))),
    ];

    let constraint = Constraint {
        discr: AstAndVars {
            value: AstNode::Bool(overflow_condition),
            variables,
        },
        kind: ConstraintKind::True,
    };

    let result = solver.check(std::iter::once(constraint));

    match result {
        SolveResult::Sat(model) => {
            let a_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);
            let b_val = model
                .get(&2)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);

            // Verify that adding these would indeed overflow u8
            assert!(
                a_val + b_val > 255,
                "Expected overflow: {} + {} = {} > 255",
                a_val,
                b_val,
                a_val + b_val
            );
        }
        _ => panic!("Expected SAT result for overflow constraint"),
    }
}

#[test]
fn test_multiple_variables() {
    // Based on samples/misc/multiple_vars/main.rs
    // Test: complex interactions between multiple symbolic variables
    let context = Context::new(&Config::new());
    let solver: Z3Solver<'_, i32> = Z3Solver::new(&context);

    let x = ast::BV::new_const(&context, "x", 32);
    let y = ast::BV::new_const(&context, "y", 32);

    // Constraint: (x + 5) == 15 AND (y + 3) == 25
    // Therefore: x == 10 AND y == 22
    let five = ast::BV::from_u64(&context, 5, 32);
    let three = ast::BV::from_u64(&context, 3, 32);
    let fifteen = ast::BV::from_u64(&context, 15, 32);
    let twenty_five = ast::BV::from_u64(&context, 25, 32);

    let calc_x = x.bvadd(&five);
    let calc_y = y.bvadd(&three);
    let eq1 = calc_x._eq(&fifteen);
    let eq2 = calc_y._eq(&twenty_five);

    let variables = vec![
        (1, AstNode::BitVector(BVNode::new(x, false))),
        (2, AstNode::BitVector(BVNode::new(y, false))),
    ];

    let constraints = vec![
        Constraint {
            discr: AstAndVars {
                value: AstNode::Bool(eq1),
                variables: variables.clone(),
            },
            kind: ConstraintKind::True,
        },
        Constraint {
            discr: AstAndVars {
                value: AstNode::Bool(eq2),
                variables,
            },
            kind: ConstraintKind::True,
        },
    ];

    let result = solver.check(constraints.into_iter());

    match result {
        SolveResult::Sat(model) => {
            let x_val = model
                .get(&1)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);
            let y_val = model
                .get(&2)
                .and_then(|v| match v {
                    AstNode::BitVector(bv) => bv.0.as_u64(),
                    _ => None,
                })
                .unwrap_or(0);

            assert_eq!(x_val, 10, "Expected x = 10, got {}", x_val);
            assert_eq!(y_val, 22, "Expected y = 22, got {}", y_val);
        }
        _ => panic!("Expected SAT result for multiple variable constraints"),
    }
}
