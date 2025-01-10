macro_rules! repeat_macro_for {
    ($macro: ident; $($item: tt)*) => {
        $(
            $macro!($item);
        )*
    };
}

macro_rules! impl_general_binary_op_through_singulars {
    () => {
        fn binary_op<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            op: crate::abs::BinaryOp,
        ) -> Self::Expr<'a> {
            use crate::abs::BinaryOp::*;
            let binop = match op {
                Add => Self::add,
                AddUnchecked => Self::add_unchecked,
                AddWithOverflow => Self::add_with_overflow,
                AddSaturating => Self::add_saturating,
                Sub => Self::sub,
                SubUnchecked => Self::sub_unchecked,
                SubWithOverflow => Self::sub_with_overflow,
                SubSaturating => Self::sub_saturating,
                Mul => Self::mul,
                MulUnchecked => Self::mul_unchecked,
                MulWithOverflow => Self::mul_with_overflow,
                Div => Self::div,
                DivExact => Self::div_exact,
                Rem => Self::rem,
                BitXor => Self::xor,
                BitAnd => Self::and,
                BitOr => Self::or,
                Shl => Self::shl,
                ShlUnchecked => Self::shl_unchecked,
                Shr => Self::shr,
                ShrUnchecked => Self::shr_unchecked,
                RotateL => Self::rotate_left,
                RotateR => Self::rotate_right,
                Eq => Self::eq,
                Lt => Self::lt,
                Le => Self::le,
                Ne => Self::ne,
                Ge => Self::ge,
                Gt => Self::gt,
                Cmp => Self::cmp,
                Offset => Self::offset,
            };
            binop(self, operands)
        }
    };
}

macro_rules! impl_general_binary_op_for {
    ($($method:ident = $op:expr)*) => {
        $(
            #[inline(always)]
            fn $method<'a>(
                &mut self,
                operands: <Self as BinaryExprBuilder>::ExprRefPair<'a>,
            ) -> <Self as BinaryExprBuilder>::Expr<'a> {
                self.binary_op(operands, $op)
            }
        )*
    };
    ($($method:ident = $op:expr)* , $arg: ident : $arg_type: ty) => {
        $(
            #[inline(always)]
            fn $method<'a>(
                &mut self,
                operands: <Self as BinaryExprBuilder>::ExprRefPair<'a>,
                $arg: $arg_type,
            ) -> <Self as BinaryExprBuilder>::Expr<'a> {
                self.binary_op(operands, $op, $arg)
            }
        )*
    };
}
macro_rules! impl_singular_binary_ops_through_general {
    () => {
        impl_general_binary_op_for!(
            add = abs::BinaryOp::Add
            add_unchecked = abs::BinaryOp::AddUnchecked
            add_with_overflow = abs::BinaryOp::AddWithOverflow
            add_saturating = abs::BinaryOp::AddSaturating
            sub = abs::BinaryOp::Sub
            sub_unchecked = abs::BinaryOp::SubUnchecked
            sub_with_overflow = abs::BinaryOp::SubWithOverflow
            sub_saturating = abs::BinaryOp::SubSaturating
            mul = abs::BinaryOp::Mul
            mul_unchecked = abs::BinaryOp::MulUnchecked
            mul_with_overflow = abs::BinaryOp::MulWithOverflow
            div = abs::BinaryOp::Div
            div_exact = abs::BinaryOp::DivExact
            rem = abs::BinaryOp::Rem
            xor = abs::BinaryOp::BitXor
            and = abs::BinaryOp::BitAnd
            or = abs::BinaryOp::BitOr
            shl = abs::BinaryOp::Shl
            shl_unchecked = abs::BinaryOp::ShlUnchecked
            shr = abs::BinaryOp::Shr
            shr_unchecked = abs::BinaryOp::ShrUnchecked
            rotate_left = abs::BinaryOp::RotateL
            rotate_right = abs::BinaryOp::RotateR
            eq = abs::BinaryOp::Eq
            lt = abs::BinaryOp::Lt
            le = abs::BinaryOp::Le
            ne = abs::BinaryOp::Ne
            ge = abs::BinaryOp::Ge
            gt = abs::BinaryOp::Gt
            cmp = abs::BinaryOp::Cmp
            offset = abs::BinaryOp::Offset
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_unary_op_through_singulars {
    () => {
        fn unary_op<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            op: crate::abs::UnaryOp,
        ) -> Self::Expr<'a> {
            use crate::abs::UnaryOp::*;
            match op {
                NoOp => self.no_op(operand),
                Not => self.not(operand),
                Neg => self.neg(operand),
                PtrMetadata => self.ptr_metadata(operand),
                BitReverse => self.bit_reverse(operand),
                NonZeroTrailingZeros => self.trailing_zeros(operand, true),
                TrailingZeros => self.trailing_zeros(operand, false),
                CountOnes => self.count_ones(operand),
                NonZeroLeadingZeros => self.leading_zeros(operand, true),
                LeadingZeros => self.leading_zeros(operand, false),
            }
        }
    };
}

macro_rules! impl_singular_unary_op_through_general {
    (($method:ident $(+ $($arg: ident : $arg_type: ty),*)? = $op:expr)) => {
        #[inline(always)]
        fn $method<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($($arg: $arg_type,)*)?
        ) -> Self::Expr<'a> {
            self.unary_op(operand, $op)
        }
    };
}
macro_rules! impl_singular_unary_ops_through_general {
    () => {
        repeat_macro_for!(
            impl_singular_unary_op_through_general;
            (no_op = crate::abs::UnaryOp::NoOp)
            (not = crate::abs::UnaryOp::Not)
            (neg = crate::abs::UnaryOp::Neg)
            (ptr_metadata = crate::abs::UnaryOp::PtrMetadata)
            (bit_reverse = crate::abs::UnaryOp::BitReverse)
            (trailing_zeros + non_zero: bool =
                if non_zero {
                    crate::abs::UnaryOp::NonZeroTrailingZeros
                } else {
                    crate::abs::UnaryOp::TrailingZeros
                })
            (count_ones = crate::abs::UnaryOp::CountOnes)
            (leading_zeros + non_zero: bool =
                if non_zero {
                    crate::abs::UnaryOp::NonZeroLeadingZeros
                } else {
                    crate::abs::UnaryOp::LeadingZeros
                })
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_ternary_op_through_singulars {
    () => {
        fn ternary_op<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            op: crate::abs::TernaryOp,
        ) -> Self::Expr<'a> {
            use crate::abs::TernaryOp::*;
            match op {
                IfThenElse => self.if_then_else(operands),
            }
        }
    };
}

macro_rules! impl_singular_ternary_op_through_general {
    (($method:ident $(+ $($arg: ident : $arg_type: ty),*)? = $op:expr)) => {
        #[inline(always)]
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            $($($arg: $arg_type,)*)?
        ) -> Self::Expr<'a> {
            self.ternary_op(operands, $op)
        }
    };
}
macro_rules! impl_singular_ternary_ops_through_general {
    () => {
        repeat_macro_for!(
            impl_singular_ternary_op_through_general;
            (if_then_else = crate::abs::TernaryOp::IfThenElse)
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_cast_through_singulars {
    () => {
        fn cast<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            target: crate::abs::CastKind<
                Self::IntType,
                Self::FloatType,
                Self::PtrType,
                Self::GenericType,
            >,
            metadata: Self::Metadata<'a>,
        ) -> Self::Expr<'a> {
            use crate::abs::CastKind::*;
            match target {
                ToChar => self.to_char(operand, metadata),
                ToInt(ty) => self.to_int(operand, ty, metadata),
                ToFloat(ty) => self.to_float(operand, ty, metadata),
                ToPointer(ty) => self.to_ptr(operand, ty, metadata),
                PointerUnsize => self.ptr_unsize(operand, metadata),
                ExposeProvenance => self.expose_prov(operand, metadata),
                SizedDynamize => self.sized_dyn(operand, metadata),
                Transmute(ty) => self.transmute(operand, ty, metadata),
            }
        }
    };
}

macro_rules! impl_singular_cast_through_general {
    (($method:ident $(+ $($arg: ident : $arg_type: ty),*)? = $op:expr)) => {
        #[inline(always)]
        fn $method<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($($arg: $arg_type,)*)?
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            self.cast(operand, $op, metadata)
        }
    };
}
macro_rules! impl_singular_casts_through_general {
    () => {
        repeat_macro_for!(
            impl_singular_cast_through_general;
            (to_char = crate::abs::CastKind::ToChar)
            (to_int + ty: Self::IntType = crate::abs::CastKind::ToInt(ty))
            (to_float + ty: Self::FloatType = crate::abs::CastKind::ToFloat(ty))
            (to_ptr + ty: Self::PtrType = crate::abs::CastKind::ToPointer(ty))
            (ptr_unsize = crate::abs::CastKind::PointerUnsize)
            (expose_prov = crate::abs::CastKind::ExposeProvenance)
            (sized_dyn = crate::abs::CastKind::SizedDynamize)
            (transmute + ty: Self::GenericType = crate::abs::CastKind::Transmute(ty))
        );
    };
}

/// Takes a macro rule with the input of a single method name and many arguments
/// and extends it with two additional patterns for multiple method names and
/// respectively zero and one extra arguments.
macro_rules! macro_rules_method_with_optional_args {
    ($name:ident { $($rule:tt)* }) => {
        macro_rules! $name {
            ($$($$method:ident)*) => {
                $$($name!($$method +);)*
            };
            ($$($$method:ident)* + $$arg: ident : $$arg_type: ty) => {
                $$($name!($$method + $$arg: $$arg_type,);)*
            };
            $($rule)*
        }
    };
}

#[allow(unused_imports)]
pub(crate) use {
    impl_general_binary_op_for, impl_general_binary_op_through_singulars,
    impl_general_cast_through_singulars, impl_general_ternary_op_through_singulars,
    impl_singular_binary_ops_through_general, impl_singular_cast_through_general,
    impl_singular_casts_through_general, impl_singular_ternary_op_through_general,
    impl_singular_ternary_ops_through_general, impl_singular_unary_op_through_general,
    impl_singular_unary_ops_through_general, macro_rules_method_with_optional_args,
    repeat_macro_for,
};
