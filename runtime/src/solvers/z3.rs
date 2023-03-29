use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use crate::{
    abs::{
        backend::{self, ValueTranslator},
        Constraint,
    },
    backends::basic::{self, expr::ValueRef},
};
use z3::{
    self,
    ast::{self, Dynamic},
    Context, SatResult, Solver,
};

/* NOTE: Why not using `Dynamic`?
 * In this way we have a little more freedom to include our information such
 * as whether the bit vector is signed or not.
 */
#[derive(Debug, Clone)]
pub(crate) enum AstNode<'ctx> {
    Bool(ast::Bool<'ctx>),
    BitVector { ast: ast::BV<'ctx>, is_signed: bool },
}

impl<'ctx> From<ast::Bool<'ctx>> for AstNode<'ctx> {
    fn from(ast: ast::Bool<'ctx>) -> Self {
        Self::Bool(ast)
    }
}

impl<'ctx> AstNode<'ctx> {
    fn from_ubv(ast: ast::BV<'ctx>) -> Self {
        Self::from_bv(ast, false)
    }

    fn from_bv(ast: ast::BV<'ctx>, is_signed: bool) -> Self {
        Self::BitVector { ast, is_signed }
    }
}

impl<'ctx> AstNode<'ctx> {
    fn as_bool(&self) -> &ast::Bool<'ctx> {
        match self {
            Self::Bool(ast) => ast,
            _ => panic!("Expected the value to be a boolean expression."),
        }
    }

    fn as_bit_vector(&self) -> &ast::BV<'ctx> {
        match self {
            Self::BitVector { ast, .. } => ast,
            _ => panic!("Expected the value to be a bit vector."),
        }
    }

    fn ast(&self) -> Dynamic<'ctx> {
        match self {
            Self::Bool(ast) => Dynamic::from_ast(ast),
            Self::BitVector { ast, .. } => Dynamic::from_ast(ast),
        }
    }
}

impl Into<ValueRef> for AstNode<'_> {
    fn into(self) -> ValueRef {
        match self {
            Self::Bool(ast) => basic::expr::Value::Concrete(basic::expr::ConcreteValue::Const(
                basic::expr::ConstValue::Bool(ast.as_bool().unwrap()),
            ))
            .into(),
            Self::BitVector { ast, is_signed } => {
                // TODO: Add support for up to 128-bit integers.
                let value = if is_signed {
                    let bytes = ast.as_i64().unwrap().to_be_bytes();
                    let mut extended = [0 as u8; 16];
                    extended[8..].copy_from_slice(&bytes);
                    u128::from_be_bytes(extended)
                } else {
                    ast.as_u64().unwrap() as u128
                };
                ValueRef::new(basic::expr::Value::Concrete(
                    basic::expr::ConcreteValue::Const(basic::expr::ConstValue::Int {
                        is_signed,
                        bit_rep: value,
                        size: ast.get_size() as u64,
                    }),
                ))
            }
        }
    }
}

type VariableId = u32;
type AstPair<'ctx> = (ast::Bool<'ctx>, HashMap<VariableId, AstNode<'ctx>>);

pub(crate) struct Z3Solver<'ctx, V> {
    solver: Solver<'ctx>,
    translator: Box<dyn ValueTranslator<V, AstPair<'ctx>>>,
    phantom: PhantomData<V>,
}

impl<'ctx, V> Z3Solver<'ctx, V> {
    pub fn new(
        solver: Solver<'ctx>,
        translator: Box<dyn ValueTranslator<V, AstPair<'ctx>>>,
    ) -> Self {
        Self {
            solver,
            translator,
            phantom: PhantomData,
        }
    }
}

impl<'ctx, V> backend::Solver for Z3Solver<'ctx, V>
where
    V: From<AstNode<'ctx>>,
{
    type SymVarId = u32;

    type Value = V;

    fn check(
        &mut self,
        constraints: &[crate::abs::Constraint<Self::Value>],
    ) -> backend::SolveResult<Self::SymVarId, Self::Value> {
        self.solver.reset();

        let mut all_vars = HashMap::<VariableId, AstNode>::new();
        for constraint in constraints {
            let (value, is_negated) = constraint.destruct();
            let (ast, variables) = self.translator.translate(value);
            all_vars.extend(variables);
            self.solver
                .assert(&(if is_negated { ast.not() } else { ast }));
        }

        match self.solver.check() {
            SatResult::Sat => {
                let model = self.solver.get_model().unwrap();
                let mut values = HashMap::new();
                for (id, node) in all_vars {
                    let value = match node {
                        AstNode::Bool(ast) => AstNode::Bool(model.eval(&ast, true).unwrap()),
                        AstNode::BitVector { ast, is_signed } => AstNode::BitVector {
                            ast: model.eval(&ast, true).unwrap(),
                            is_signed,
                        },
                    };
                    values.insert(id, value.into());
                }
                backend::SolveResult::Sat(values)
            }
            SatResult::Unsat => backend::SolveResult::Unsat,
            SatResult::Unknown => backend::SolveResult::Unknown,
        }
    }
}

mod translators {
    use std::{
        clone,
        collections::HashMap,
        mem::{discriminant, size_of},
        ops::Not,
    };

    use z3::{
        ast::{self, Ast, Dynamic},
        Context,
    };

    use crate::{
        abs::{backend::ValueTranslator, BinaryOp, UnaryOp},
        backends::basic::expr::{
            ConcreteValue, ConstValue, Expr, SymValue, SymbolicVar, SymbolicVarType, Value,
            ValueRef,
        },
    };

    use super::{AstNode, AstPair};

    const CHAR_BIT_SIZE: u32 = size_of::<char>() as u32 * 8;

    pub(crate) struct Z3ValueTranslator<'ctx> {
        context: &'ctx Context,
        variables: HashMap<u32, AstNode<'ctx>>,
    }

    impl<'ctx> ValueTranslator<ValueRef, AstPair<'ctx>> for Z3ValueTranslator<'ctx> {
        fn translate(&mut self, value: &ValueRef) -> AstPair<'ctx> {
            let ast = self.translate_value(value);
            match ast {
                AstNode::Bool(ast) => (ast, self.variables.drain().collect()),
                _ => panic!(
                    "Expected the value to be a boolean expression but it is a {:#?}.",
                    ast
                ),
            }
        }
    }

    impl<'ctx> Z3ValueTranslator<'ctx> {
        fn translate_value(&mut self, value: &ValueRef) -> AstNode<'ctx> {
            match value.as_ref() {
                Value::Concrete(c) => self.translate_concrete(c),
                Value::Symbolic(s) => self.translate_symbolic(s),
            }
        }

        fn translate_concrete(&mut self, concrete: &ConcreteValue) -> AstNode<'ctx> {
            match concrete {
                ConcreteValue::Const(c) => self.translate_const(c),
                ConcreteValue::Adt(a) => todo!(),
                ConcreteValue::Array(a) => todo!(),
                ConcreteValue::Ref(r) => todo!(),
            }
        }

        fn translate_const(&mut self, const_value: &ConstValue) -> AstNode<'ctx> {
            match const_value {
                ConstValue::Bool(b) => ast::Bool::from_bool(self.context, *b).into(),
                ConstValue::Char(c) => {
                    AstNode::from_ubv(ast::BV::from_u64(self.context, *c as u64, CHAR_BIT_SIZE))
                }
                ConstValue::Int {
                    bit_rep,
                    size,
                    is_signed: false,
                } => {
                    // TODO: Add support for 128 bit integers.
                    AstNode::from_bv(
                        ast::BV::from_u64(
                            self.context,
                            *bit_rep as u64,
                            (*size).try_into().expect("Size is too large."),
                        ),
                        false,
                    )
                }
                ConstValue::Int {
                    bit_rep,
                    size,
                    is_signed: true,
                } => {
                    // TODO: Add support for 128 bit integers.
                    AstNode::from_bv(
                        ast::BV::from_i64(
                            self.context,
                            *bit_rep as i64,
                            (*size).try_into().expect("Size is too large."),
                        ),
                        true,
                    )
                }
                ConstValue::Float {
                    bit_rep,
                    ebits,
                    sbits,
                } => todo!(),
                ConstValue::Str(s) => todo!(),
                ConstValue::Func(_) => todo!(),
            }
        }

        fn translate_symbolic(&mut self, symbolic: &SymValue) -> AstNode<'ctx> {
            match symbolic {
                SymValue::Variable(var) => self.translate_symbolic_var(var),
                SymValue::Expression(expr) => self.translate_symbolic_expr(expr),
            }
        }

        fn translate_symbolic_var(&mut self, var: &SymbolicVar) -> AstNode<'ctx> {
            let node = match var.ty {
                SymbolicVarType::Bool => ast::Bool::new_const(self.context, var.id).into(),
                SymbolicVarType::Char => {
                    AstNode::from_ubv(ast::BV::new_const(self.context, var.id, CHAR_BIT_SIZE))
                }
                SymbolicVarType::Int { size, is_signed } => AstNode::from_bv(
                    ast::BV::new_const(self.context, var.id, size as u32),
                    is_signed,
                ),
                SymbolicVarType::Float { ebits, sbits } => todo!(),
            };
            self.variables.insert(var.id, node.clone());
            node
        }

        fn translate_symbolic_expr(&mut self, expr: &Expr) -> AstNode<'ctx> {
            match expr {
                Expr::Unary { operator, operand } => {
                    let operand = self.translate_symbolic(operand);
                    self.translate_unary_expr(operator, operand)
                }
                Expr::Binary {
                    operator,
                    first,
                    second,
                    is_flipped,
                } => {
                    let first = self.translate_symbolic(first);
                    let second = self.translate_value(second);
                    let (left, right) = if *is_flipped {
                        (second, first)
                    } else {
                        (first, second)
                    };
                    self.translate_binary_expr(operator, left, right)
                }
                Expr::Cast() => todo!(),
                Expr::AddrOf() => todo!(),
                Expr::Deref(_) => todo!(),
                Expr::Index {
                    on,
                    index,
                    from_end,
                } => todo!(),
                Expr::Slice {
                    of,
                    from,
                    to,
                    from_end,
                } => todo!(),
            }
        }

        fn translate_unary_expr(
            &mut self,
            operator: &UnaryOp,
            operand: AstNode<'ctx>,
        ) -> AstNode<'ctx> {
            match operator {
                UnaryOp::Not => match operand {
                    AstNode::Bool(ast) => ast.not().into(),
                    AstNode::BitVector { ast, is_signed } => {
                        AstNode::from_bv(ast.bvnot(), is_signed)
                    }
                    _ => unreachable!("Not is only supposed to be applied to bools and ints."),
                },
                UnaryOp::Neg => match operand {
                    AstNode::BitVector {
                        ast,
                        is_signed: true,
                    } => AstNode::from_bv(ast.bvneg(), true),
                    _ => unreachable!("Neg is only supposed to be applied to signed numbers."),
                },
            }
        }

        fn translate_binary_expr(
            &mut self,
            operator: &BinaryOp,
            left: AstNode<'ctx>,
            right: AstNode<'ctx>,
        ) -> AstNode<'ctx> {
            assert_eq!(discriminant(&left), discriminant(&right));

            match left {
                AstNode::Bool(_) => {
                    let left = left.as_bool();
                    let right = right.as_bool();
                    match operator {
                        BinaryOp::Eq => ast::Bool::_eq(left, right),
                        BinaryOp::Ne => ast::Bool::_eq(left, right).not(),
                        BinaryOp::BitAnd => ast::Bool::and(left.get_ctx(), &[left, right]),
                        BinaryOp::BitOr => ast::Bool::or(left.get_ctx(), &[left, right]),
                        BinaryOp::BitXor => ast::Bool::xor(left, right),
                        _ => unreachable!("Bool can only be compared with eq, ne, lt, le, gt, ge."),
                    }
                    .into()
                }
                AstNode::BitVector { is_signed, .. } => {
                    let left = left.as_bit_vector();
                    let right = right.as_bit_vector();
                    let ar_func: Option<fn(&ast::BV<'ctx>, &ast::BV<'ctx>) -> ast::BV<'ctx>> =
                        match (operator, is_signed) {
                            (BinaryOp::Add, _) => Some(ast::BV::bvadd),
                            (BinaryOp::Sub, _) => Some(ast::BV::bvsub),
                            (BinaryOp::Mul, _) => Some(ast::BV::bvmul),
                            (BinaryOp::Div, true) => Some(ast::BV::bvsdiv),
                            (BinaryOp::Div, false) => Some(ast::BV::bvudiv),
                            (BinaryOp::Rem, true) => Some(ast::BV::bvsrem),
                            (BinaryOp::Rem, false) => Some(ast::BV::bvurem),
                            (BinaryOp::BitXor, _) => Some(ast::BV::bvxor),
                            (BinaryOp::BitAnd, _) => Some(ast::BV::bvand),
                            (BinaryOp::BitOr, _) => Some(ast::BV::bvor),
                            (BinaryOp::Shl, _) => Some(ast::BV::bvshl),
                            /* Shift right function obtained from documentation
                             * https://doc.rust-lang.org/reference/expressions/operator-expr.html#arithmetic-and-logical-binary-operators
                             */
                            (BinaryOp::Shr, true) => Some(ast::BV::bvashr),
                            (BinaryOp::Shr, false) => Some(ast::BV::bvlshr),
                            _ => None,
                        };

                    if let Some(func) = ar_func {
                        AstNode::from_bv(func(left, right), is_signed)
                    } else {
                        let logical_func: fn(&ast::BV<'ctx>, &ast::BV<'ctx>) -> ast::Bool<'ctx> =
                            match (operator, is_signed) {
                                (BinaryOp::Eq, _) => ast::BV::_eq,
                                (BinaryOp::Ne, _) => |l, r| ast::BV::_eq(l, r).not(),
                                (BinaryOp::Lt, true) => ast::BV::bvslt,
                                (BinaryOp::Lt, false) => ast::BV::bvult,
                                (BinaryOp::Le, true) => ast::BV::bvsle,
                                (BinaryOp::Le, false) => ast::BV::bvule,
                                (BinaryOp::Ge, true) => ast::BV::bvsge,
                                (BinaryOp::Ge, false) => ast::BV::bvuge,
                                (BinaryOp::Gt, true) => ast::BV::bvsgt,
                                (BinaryOp::Gt, false) => ast::BV::bvugt,
                                (BinaryOp::Offset, _) => todo!(),
                                _ => unreachable!(),
                            };
                        logical_func(left, right).into()
                    }
                }
            }
        }
    }
}
