const TAG: &str = "translators";

pub(crate) mod z3 {
    use std::{
        collections::HashMap,
        default::Default,
        mem::{discriminant, size_of},
        ops::Not,
    };

    use z3::{
        ast::{self, Ast},
        Context,
    };

    use crate::{
        abs::{expr::sym_place::SelectTarget, IntType, ValueType},
        backends::basic::expr::{prelude::*, OverflowingBinaryOp, SymBinaryOperands, SymVarId},
        solvers::z3::{ArrayNode, ArraySort, AstNodeSort, BVExt, BVNode, BVSort},
    };

    use common::log_debug;

    use crate::solvers::z3::{AstNode, TranslatedConstraint};

    use super::TAG;

    const CHAR_BIT_SIZE: u32 = size_of::<char>() as u32 * 8;
    const USIZE_BIT_SIZE: u32 = size_of::<usize>() as u32 * 8;
    const POSSIBLE_VALUES_PREFIX: &str = "pvs";

    pub(crate) struct Z3ValueTranslator<'ctx> {
        context: &'ctx Context,
        variables: HashMap<SymVarId, AstNode<'ctx>>,
        constraints: Vec<ast::Bool<'ctx>>,
    }

    impl<'ctx> Z3ValueTranslator<'ctx> {
        pub(crate) fn new(context: &'ctx Context) -> Self {
            Self {
                context,
                variables: Default::default(),
                // Additional constraints are not common.
                constraints: Vec::with_capacity(0),
            }
        }
    }

    impl<'ctx> FnOnce<(&ValueRef,)> for Z3ValueTranslator<'ctx> {
        type Output = TranslatedConstraint<'ctx, SymVarId>;
        extern "rust-call" fn call_once(mut self, (value,): (&ValueRef,)) -> Self::Output {
            self.translate(value)
        }
    }

    impl<'ctx> FnMut<(&ValueRef,)> for Z3ValueTranslator<'_> {
        extern "rust-call" fn call_mut(&mut self, (value,): (&ValueRef,)) -> Self::Output {
            self.translate(value)
        }
    }

    impl<'ctx> Z3ValueTranslator<'ctx> {
        fn translate(&mut self, value: &ValueRef) -> TranslatedConstraint<'ctx, SymVarId> {
            log_debug!(target: TAG, "Translating value: {}", value);
            let ast = self.translate_value(value);
            match ast {
                AstNode::Bool(ast) => TranslatedConstraint {
                    constraint: ast,
                    variables: self.variables.drain().collect(),
                    extra: self.constraints.drain(..).collect(),
                },
                _ => panic!("Expected the value to be a boolean expression but it is a {ast:#?}.",),
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
                ConcreteValue::Adt(_) => {
                    unimplemented!("Expressions involving ADTs directly are not supported.")
                }
                ConcreteValue::Array(array) => AstNode::Array(self.translate_array(array)),
                ConcreteValue::FatPointer(_) => {
                    panic!("Pointer value should not exist at this phase.")
                }
                ConcreteValue::Unevaluated(unevaluated) => {
                    panic!(
                        "Unevaluated value should not exist at this phase. {:?}",
                        unevaluated
                    )
                }
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
                    ty:
                        IntType {
                            bit_size,
                            is_signed,
                        },
                } => {
                    let size = (*bit_size).try_into().unwrap();
                    let ast = if *bit_size <= 64 {
                        if !*is_signed {
                            ast::BV::from_u64(self.context, bit_rep.0 as u64, size)
                        } else {
                            ast::BV::from_i64(self.context, bit_rep.0 as i64, size)
                        }
                    } else {
                        ast::BV::from_str(&self.context, size, &bit_rep.to_string()).unwrap()
                    };
                    BVNode::new(ast, *is_signed).into()
                }
                ConstValue::Float { .. } => todo!(),
                ConstValue::Func(_) => unreachable!(concat!(
                    "Function values are not supposed to appear in symbolic expressions.",
                    "Symbolic function pointers are not expected to appear as function constants."
                )),
                ConstValue::Addr(..) => {
                    unreachable!(
                        "Raw address values are not supposed to appear in symbolic expressions."
                    )
                }
            }
        }

        fn translate_array(&mut self, array: &ArrayValue) -> ArrayNode<'ctx> {
            self.translate_array_of_values("arr", array.elements.iter(), Self::translate_value)
        }

        fn translate_symbolic(&mut self, symbolic: &SymValue) -> AstNode<'ctx> {
            match symbolic {
                SymValue::Variable(var) => self.translate_symbolic_var(var),
                SymValue::Expression(expr) => self.translate_symbolic_expr(expr),
            }
        }

        fn translate_symbolic_var(&mut self, var: &SymbolicVar) -> AstNode<'ctx> {
            self.translate_symbolic_var_and_record(var)
        }

        fn translate_symbolic_var_and_record(&mut self, var: &SymbolicVar) -> AstNode<'ctx> {
            let node = match var.ty {
                ValueType::Bool => ast::Bool::new_const(self.context, var.id).into(),
                ValueType::Char => {
                    AstNode::from_ubv(ast::BV::new_const(self.context, var.id, CHAR_BIT_SIZE))
                }
                ValueType::Int(IntType {
                    bit_size,
                    is_signed,
                }) => {
                    let ast = ast::BV::new_const(self.context, var.id, bit_size as u32);
                    BVNode::new(ast, is_signed).into()
                }
                ValueType::Float { .. } => todo!(),
            };
            self.variables.insert(var.id, node.clone());
            node
        }

        fn translate_symbolic_expr(&mut self, expr: &Expr) -> AstNode<'ctx> {
            log_debug!(target: TAG, "Translating symbolic expression: {}", expr);
            use Expr::*;
            match expr {
                Unary { operator, operand } => {
                    let operand = self.translate_symbolic(operand);
                    self.translate_unary_expr(operator, operand)
                }
                Binary(BinaryExpr { operator, operands }) => {
                    let (left, right) = self.translate_binary_operands(operands);
                    self.translate_binary_expr(*operator, left, right)
                }
                BinaryBoundCheck {
                    bin_expr: BinaryExpr { operator, operands },
                    is_overflow,
                } => {
                    let (left, right) = self.translate_binary_operands(operands);
                    self.translate_binary_bound_check(*operator, left, right, *is_overflow)
                }
                Extension(ExtensionExpr {
                    source,
                    is_zero_ext,
                    bits_to_add,
                    ty,
                }) => {
                    let source = self.translate_symbolic(source);
                    self.translate_extension_expr(
                        source,
                        *is_zero_ext,
                        (*bits_to_add).into(),
                        ty.is_signed(),
                    )
                }
                Truncation(TruncationExpr { source, ty }) => {
                    let source = self.translate_symbolic(source);
                    self.translate_truncation_expr(source, ty.bit_size as u32, ty.is_signed)
                }
                Ite {
                    condition,
                    if_target,
                    else_target,
                } => {
                    let condition = self.translate_symbolic(condition);
                    let if_target = self.translate_value(if_target);
                    let else_target = self.translate_value(else_target);
                    self.translate_ite_expr(condition, if_target, else_target)
                }
                Transmutation { source, dst_ty } => {
                    let ast = self.translate_symbolic(source);
                    let ast = if let Some(bv_sort) = ValueType::try_from(dst_ty)
                        .ok()
                        .and_then(|value_ty| BVSort::try_from(value_ty).ok())
                    {
                        ast.transmute(bv_sort)
                    } else {
                        ast
                    };
                    ast
                }
                Multi(select) => self.translate_select(select, None),
                Ref(..) | Len(..) | PtrMetadata(..) => {
                    unreachable!(
                        "Projection expressions should be resolved before translation. Got: {expr}"
                    )
                }
                Partial(..) => {
                    unreachable!(
                        "Partial expressions are expected to be converted to masked values before translation."
                    )
                }
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
                    AstNode::BitVector(bv) => bv.map(ast::BV::bvnot).into(),
                    _ => unreachable!("Not is not supported for this operand: {operand:#?}"),
                },
                UnaryOp::Neg => match operand {
                    AstNode::BitVector(bv @ BVNode(_, BVSort { is_signed: true })) => {
                        bv.map(ast::BV::bvneg).into()
                    }
                    _ => unreachable!("Neg is not supported for this operand: {operand:#?}"),
                },
                UnaryOp::BitReverse => match operand {
                    AstNode::BitVector(bv) => self.translate_bitreverse_expr(bv),
                    _ => unreachable!("BitReverse is not supported for this operand: {operand:#?}"),
                },
                UnaryOp::TrailingZeros => match operand {
                    AstNode::BitVector(bv) => self.translate_trailing_zeros_expr(bv),
                    _ => unreachable!(
                        "TrailingZeros is not supported for this operand: {operand:#?}"
                    ),
                },
            }
        }

        fn translate_binary_operands(
            &mut self,
            operands: &SymBinaryOperands,
        ) -> (AstNode<'ctx>, AstNode<'ctx>) {
            (
                self.translate_value(operands.first()),
                self.translate_value(operands.second()),
            )
        }

        fn translate_binary_expr(
            &mut self,
            operator: BinaryOp,
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
                        _ => {
                            unreachable!(
                                "Bool can only used with logical operators. {:?}",
                                operator
                            )
                        }
                    }
                    .into()
                }
                AstNode::BitVector(ref left_node) => {
                    let right = match operator {
                        // Z3 requires that the operands in a left or right shift operation are the same size.
                        // Thus, if two operands are of different sizes, we cast the right operand to the same type of left operand.
                        // Casting from a larger type to a smaller one will truncate, whereas the reverse will zero-extend
                        BinaryOp::Shl | BinaryOp::Shr | BinaryOp::RotateL | BinaryOp::RotateR
                            if left.z3_sort() != right.z3_sort() =>
                        {
                            let left_size = left_node.size();
                            let right_size = right.as_bit_vector().get_size();
                            if right_size > left_size {
                                // FIXME: This may cause problems with large numbers.
                                self.translate_truncation_expr(
                                    right,
                                    left_size,
                                    left_node.is_signed(),
                                )
                            } else {
                                self.translate_extension_expr(
                                    right,
                                    true,
                                    left_size - right_size,
                                    false,
                                )
                            }
                        }
                        _ => right,
                    };
                    debug_assert_eq!(left.z3_sort(), right.z3_sort());
                    let right_bv = right.as_bit_vector();
                    let is_signed = left_node.is_signed();

                    let handle_ar_op = || {
                        let f: Option<fn(&_, &_) -> ast::BV<'ctx>> = match (operator, is_signed) {
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
                            (BinaryOp::Shr, true) => Some(ast::BV::bvashr),
                            (BinaryOp::Shr, false) => Some(ast::BV::bvlshr),
                            (BinaryOp::RotateL, _) => Some(ast::BV::bvrotl),
                            (BinaryOp::RotateR, _) => Some(ast::BV::bvrotr),
                            (BinaryOp::Offset, _) => Some(todo!()),
                            _ => None,
                        };
                        f.map(|f| left_node.map(|left| f(left, right_bv)).into())
                    };
                    let handle_logical_op = || {
                        let f: Option<fn(&_, &_) -> ast::Bool<'ctx>> = match (operator, is_signed) {
                            (BinaryOp::Eq, _) => Some(ast::BV::_eq),
                            (BinaryOp::Ne, _) => Some(|l, r| ast::BV::_eq(l, r).not()),
                            (BinaryOp::Lt, true) => Some(ast::BV::bvslt),
                            (BinaryOp::Lt, false) => Some(ast::BV::bvult),
                            (BinaryOp::Le, true) => Some(ast::BV::bvsle),
                            (BinaryOp::Le, false) => Some(ast::BV::bvule),
                            (BinaryOp::Ge, true) => Some(ast::BV::bvsge),
                            (BinaryOp::Ge, false) => Some(ast::BV::bvuge),
                            (BinaryOp::Gt, true) => Some(ast::BV::bvsgt),
                            (BinaryOp::Gt, false) => Some(ast::BV::bvugt),
                            _ => None,
                        };
                        f.map(|f| f(&left_node.0, right_bv).into())
                    };
                    let handle_other_func = || {
                        let f: Option<
                            Box<dyn FnOnce(AstNode<'ctx>, AstNode<'ctx>) -> AstNode<'ctx>>,
                        > = match (operator, is_signed) {
                            (BinaryOp::Cmp, _) => Some(Box::new(|left, right| {
                                use core::cmp::Ordering::*;
                                let lt_check = self.translate_binary_expr(
                                    BinaryOp::Lt,
                                    left.clone(),
                                    right.clone(),
                                );
                                let gt_check =
                                    self.translate_binary_expr(BinaryOp::Gt, left, right);
                                let less = self.translate_const(&(Less as i8).into());
                                let greater = self.translate_const(&(Greater as i8).into());
                                let equal = self.translate_const(&(Equal as i8).into());
                                let gt = self.translate_ite_expr(gt_check, greater, equal);
                                self.translate_ite_expr(lt_check, less, gt)
                            })),
                            _ => None,
                        };
                        f.map(|f| f(left.clone(), right.clone()))
                    };

                    None.or_else(handle_ar_op)
                        .or_else(handle_logical_op)
                        .or_else(handle_other_func)
                        .unwrap()
                }
                _ => unreachable!("Binary expressions are not supported for this type: {left:#?}"),
            }
        }

        fn translate_extension_expr(
            &mut self,
            source: AstNode<'ctx>,
            is_zero_ext: bool,
            bits_to_add: u32,
            is_signed: bool,
        ) -> AstNode<'ctx> {
            match source {
                AstNode::BitVector(BVNode(ast, _)) => {
                    let ast = if is_zero_ext {
                        ast.zero_ext(bits_to_add)
                    } else {
                        ast.sign_ext(bits_to_add)
                    };
                    BVNode::new(ast, is_signed).into()
                }
                _ => unreachable!("Invalid extension expression for {:?}.", source),
            }
        }

        fn translate_truncation_expr(
            &mut self,
            source: AstNode<'ctx>,
            high_exclusive: u32,
            is_signed: bool,
        ) -> AstNode<'ctx> {
            match source {
                AstNode::BitVector(BVNode(ast, _)) => {
                    BVNode::new(ast.extract(high_exclusive - 1, 0), is_signed).into()
                }
                _ => unreachable!("Invalid extraction expression for {:?}.", source),
            }
        }

        fn translate_ite_expr(
            &mut self,
            condition: AstNode<'ctx>,
            if_target: AstNode<'ctx>,
            else_target: AstNode<'ctx>,
        ) -> AstNode<'ctx> {
            match condition {
                AstNode::Bool(_) => {
                    let condition = condition.as_bool();
                    let ast = condition.ite(&if_target.ast(), &else_target.ast());
                    // NOTE: the sort of operands must be the same for ITE, so either sort can be picked for the sort of the result
                    debug_assert_eq!(if_target.sort(), else_target.sort());
                    AstNode::from_ast(ast, &if_target.sort())
                }
                _ => unreachable!("Invalid ITE expression for {:?}", condition),
            }
        }

        fn translate_select(
            &mut self,
            select: &MultiValue,
            const_prefix: Option<&str>,
        ) -> AstNode<'ctx> {
            let index = self.translate_symbolic(&select.index.index);
            let index = if select.index.from_end {
                todo!("#485")
            } else {
                index
            };
            debug_assert_eq!(
                index.z3_sort(),
                z3::Sort::bitvector(self.context, USIZE_BIT_SIZE)
            );

            /* NOTE: Do we need to add constraint that index is within bounds?
             * This code is meant for safe Rust. Thus,
             * Bound constraints are automatically implied by the bound checks compiler adds.
             * Also, we don't need to worry about the empty arrays for the same reason. */

            match &select.target {
                SelectTarget::Array(possible_values) => {
                    let ArrayNode(
                        ast,
                        ArraySort {
                            range: box elem_sort,
                        },
                    ) = self.translate_array_of_values(
                        const_prefix.unwrap_or(POSSIBLE_VALUES_PREFIX),
                        possible_values.iter(),
                        |this, r| this.translate_multi_value_tree(r, const_prefix),
                    );

                    let result =
                        AstNode::from_ast(ast::Array::select(&ast, &index.ast()), &elem_sort);
                    result
                }
                SelectTarget::Nested(box inner) => {
                    let inner = self.translate_select(inner, const_prefix);
                    if let AstNode::Array(ArrayNode(array, ArraySort { range: range_sort })) = inner
                    {
                        AstNode::from_ast(ast::Array::select(&array, &index.ast()), &range_sort)
                    } else {
                        unreachable!("Nested select result should be an array.")
                    }
                }
            }
        }

        fn translate_array_of_values<'a, V: 'a>(
            &mut self,
            const_prefix: &str,
            values: impl Iterator<Item = &'a V>,
            translate: impl Fn(&mut Self, &V) -> AstNode<'ctx>,
        ) -> ArrayNode<'ctx>
        where
            'ctx: 'a,
        {
            let context = self.context;

            let mut values = values.map(|v| translate(self, v));
            let first = values
                .next()
                .expect("Indices on zero-sized arrays should be prevented by the bound checks.");
            let element_sort = first.sort().clone();

            let mut array = ast::Array::fresh_const(
                context,
                const_prefix,
                &z3::Sort::bitvector(context, USIZE_BIT_SIZE),
                &first.z3_sort(),
            );

            for (i, value) in std::iter::once(first).chain(values).enumerate() {
                array = array.store(
                    &ast::BV::from_u64(context, i as u64, USIZE_BIT_SIZE),
                    &value.ast(),
                );
            }

            ArrayNode(
                array,
                ArraySort {
                    range: Box::new(element_sort),
                },
            )
        }

        fn translate_multi_value_leaf(&mut self, leaf: &MultiValueLeaf) -> AstNode<'ctx> {
            self.translate_value(leaf)
        }

        fn translate_multi_value_tree(
            &mut self,
            tree: &MultiValueTree,
            const_prefix: Option<&str>,
        ) -> AstNode<'ctx> {
            match tree {
                MultiValueTree::Single(single) => self.translate_multi_value_leaf(single),
                MultiValueTree::Array(values) => {
                    let const_prefix = const_prefix.unwrap_or(POSSIBLE_VALUES_PREFIX);
                    self.translate_array_of_values(const_prefix, values.iter(), |this, v| {
                        this.translate_multi_value_tree(v, Some(const_prefix))
                    })
                    .into()
                }
                MultiValueTree::SymRead(select) => self.translate_select(select, const_prefix),
            }
        }

        fn translate_binary_bound_check(
            &mut self,
            operator: OverflowingBinaryOp,
            left: AstNode<'ctx>,
            right: AstNode<'ctx>,
            is_overflow: bool,
        ) -> AstNode<'ctx> {
            // debug_assert!(operator.is_with_overflow());
            let AstNode::BitVector(BVNode(_, BVSort { is_signed })) = left else {
                unreachable!("Overflow only applies to numerical arithmetic operations.")
            };

            let left = left.as_bit_vector();
            let right = right.as_bit_vector();

            use OverflowingBinaryOp::*;
            let in_bounds = match (is_overflow, operator, is_signed) {
                (true, Add, _) => ast::BV::bvadd_no_overflow(left, right, is_signed),
                (true, Sub, true) => ast::BV::bvsub_no_overflow(left, right),
                // Impossible. Largest case: MAX - 0
                (true, Sub, false) => ast::Bool::from_bool(left.get_ctx(), true),
                (true, Mul, _) => ast::BV::bvmul_no_overflow(left, right, is_signed),
                (false, Add, true) => ast::BV::bvadd_no_underflow(left, right),
                // Impossible. Smallest case: 0 . 0
                (false, Add | Mul, false) => ast::Bool::from_bool(left.get_ctx(), true),
                (false, Sub, _) => ast::BV::bvsub_no_underflow(left, right, is_signed),
                (false, Mul, true) => ast::BV::bvmul_no_underflow(left, right),
            };
            ast::Bool::not(&in_bounds).into()
        }

        fn translate_bitreverse_expr(&mut self, bv: BVNode<'ctx>) -> AstNode<'ctx> {
            let size = bv.size();
            // Reverse a bit vector expression by extracting and concatenating the bits in reverse order.
            let mut reversed_bv = bv.0.extract(size - 1, size - 1);
            for idx in (0..(size - 1)).rev() {
                reversed_bv = reversed_bv.concat(&bv.0.extract(idx, idx));
            }
            BVNode::new(reversed_bv, bv.is_signed()).into()
        }

        fn translate_trailing_zeros_expr(&mut self, bv: BVNode<'ctx>) -> AstNode<'ctx> {
            let size = bv.size();
            let ctx = bv.0.get_ctx();
            let zero_bit: ast::BV<'_> = ast::BV::from_u64(ctx, 0, 1);
            let mut trailing_zeros = ast::BV::from_u64(ctx, 0, size);

            for idx in (0..size).rev() {
                let bit = bv.0.extract(idx, idx);
                trailing_zeros = bit.bvugt(&zero_bit).ite(
                    &trailing_zeros, // Current bit is 1
                    &trailing_zeros // Current bit is 0
                        /* If `trailing_zeros` is less than `(size - 1 -idx)`, bit 1 has already been encountered,
                         * so regardless of the current bit, `trailing_zeros` will not change.
                         */
                        .bvult(&ast::BV::from_u64(ctx, (size - 1 - idx).into(), size))
                        .ite(
                            &trailing_zeros,
                            &trailing_zeros.bvadd(&ast::BV::from_u64(ctx, 1, size)),
                        ),
                );
            }
            BVNode::new(trailing_zeros, false).into()
        }
    }

    impl<'ctx> AstNode<'ctx> {
        fn transmute(mut self, to_sort: BVSort) -> Self {
            match &mut self {
                AstNode::BitVector(BVNode(_, sort)) => *sort = to_sort,
                AstNode::Array(ArrayNode(_, sort)) => *sort.range = sort.range.transmute(to_sort),
                AstNode::Bool(..) => panic!("Transmutation of boolean sort is not expected."),
            };
            self
        }
    }

    impl AstNodeSort {
        fn transmute(&self, to_sort: BVSort) -> Self {
            match self {
                Self::BitVector(_) => to_sort.into(),
                Self::Array(arr) => Self::Array(Box::new(arr.range.transmute(to_sort)).into()),
                Self::Bool => panic!("Transmutation of boolean sort is not expected."),
            }
        }
    }

    impl TryFrom<ValueType> for BVSort {
        type Error = ValueType;

        fn try_from(value_type: ValueType) -> Result<Self, Self::Error> {
            match value_type {
                ValueType::Int(IntType {
                    bit_size: _,
                    is_signed,
                }) => Ok(Self { is_signed }),
                _ => Err(value_type),
            }
        }
    }

    impl<'ctx> From<AstNode<'ctx>> for ValueRef {
        fn from(ast: AstNode<'ctx>) -> Self {
            match ast {
                AstNode::Bool(ast) => super::super::ConstValue::Bool(ast.as_bool().unwrap()),
                AstNode::BitVector(BVNode(ast, BVSort { is_signed })) => {
                    super::super::ConstValue::new_int(
                        ast.as_u128().unwrap(),
                        IntType {
                            bit_size: ast.get_size() as u64,
                            is_signed,
                        },
                    )
                }
                AstNode::Array(_) => {
                    unimplemented!("Symbolic arrays are not supported by this converter.")
                }
            }
            .to_value_ref()
        }
    }
}
