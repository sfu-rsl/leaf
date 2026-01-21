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
            op: $crate::abs::expr::BinaryOp,
        ) -> Self::Expr<'a> {
            use $crate::abs::expr::BinaryOp::*;
            match op {
                Add => self.add(operands),
                AddUnchecked => self.add_unchecked(operands),
                AddWithOverflow => self.add_with_overflow(operands),
                AddSaturating => self.add_saturating(operands),
                Sub => self.sub(operands),
                SubUnchecked => self.sub_unchecked(operands),
                SubWithOverflow => self.sub_with_overflow(operands),
                SubSaturating => self.sub_saturating(operands),
                Mul => self.mul(operands),
                MulUnchecked => self.mul_unchecked(operands),
                MulWithOverflow => self.mul_with_overflow(operands),
                Div => self.div(operands),
                DivExact => self.div_exact(operands),
                Rem => self.rem(operands),
                BitXor => self.xor(operands),
                BitAnd => self.and(operands),
                BitOr => self.or(operands),
                Shl => self.shl(operands),
                ShlUnchecked => self.shl_unchecked(operands),
                Shr => self.shr(operands),
                ShrUnchecked => self.shr_unchecked(operands),
                RotateL => self.rotate_left(operands),
                RotateR => self.rotate_right(operands),
                Eq => self.eq(operands),
                Lt => self.lt(operands),
                Le => self.le(operands),
                Ne => self.ne(operands),
                Ge => self.ge(operands),
                Gt => self.gt(operands),
                Cmp => self.cmp(operands),
                Offset(pointee_size) => self.offset(operands, pointee_size),
            }
        }
    };
}

macro_rules! impl_general_binary_op_for {
    (($method:ident $(+ $($arg: ident : $arg_type: ty),*)? = $op:expr)) => {
        #[inline(always)]
        fn $method<'a>(
            &mut self,
            operands: <Self as BinaryExprBuilder>::ExprRefPair<'a>,
            $($($arg: $arg_type,)*)?
        ) -> <Self as BinaryExprBuilder>::Expr<'a> {
            self.binary_op(operands, $op)
        }
    };
}
macro_rules! impl_singular_binary_ops_through_general {
    () => {
        repeat_macro_for!(
            impl_general_binary_op_for;
            (add = $crate::abs::expr::BinaryOp::Add)
            (add_unchecked = $crate::abs::expr::BinaryOp::AddUnchecked)
            (add_with_overflow = $crate::abs::expr::BinaryOp::AddWithOverflow)
            (add_saturating = $crate::abs::expr::BinaryOp::AddSaturating)
            (sub = $crate::abs::expr::BinaryOp::Sub)
            (sub_unchecked = $crate::abs::expr::BinaryOp::SubUnchecked)
            (sub_with_overflow = $crate::abs::expr::BinaryOp::SubWithOverflow)
            (sub_saturating = $crate::abs::expr::BinaryOp::SubSaturating)
            (mul = $crate::abs::expr::BinaryOp::Mul)
            (mul_unchecked = $crate::abs::expr::BinaryOp::MulUnchecked)
            (mul_with_overflow = $crate::abs::expr::BinaryOp::MulWithOverflow)
            (div = $crate::abs::expr::BinaryOp::Div)
            (div_exact = $crate::abs::expr::BinaryOp::DivExact)
            (rem = $crate::abs::expr::BinaryOp::Rem)
            (xor = $crate::abs::expr::BinaryOp::BitXor)
            (and = $crate::abs::expr::BinaryOp::BitAnd)
            (or = $crate::abs::expr::BinaryOp::BitOr)
            (shl = $crate::abs::expr::BinaryOp::Shl)
            (shl_unchecked = $crate::abs::expr::BinaryOp::ShlUnchecked)
            (shr = $crate::abs::expr::BinaryOp::Shr)
            (shr_unchecked = $crate::abs::expr::BinaryOp::ShrUnchecked)
            (rotate_left = $crate::abs::expr::BinaryOp::RotateL)
            (rotate_right = $crate::abs::expr::BinaryOp::RotateR)
            (eq = $crate::abs::expr::BinaryOp::Eq)
            (lt = $crate::abs::expr::BinaryOp::Lt)
            (le = $crate::abs::expr::BinaryOp::Le)
            (ne = $crate::abs::expr::BinaryOp::Ne)
            (ge = $crate::abs::expr::BinaryOp::Ge)
            (gt = $crate::abs::expr::BinaryOp::Gt)
            (cmp = $crate::abs::expr::BinaryOp::Cmp)
            (offset + pointee_size: TypeSize = $crate::abs::expr::BinaryOp::Offset(pointee_size))
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_unary_op_through_singulars {
    () => {
        fn unary_op<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            op: $crate::abs::expr::UnaryOp,
        ) -> Self::Expr<'a> {
            use $crate::abs::expr::UnaryOp::*;
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
                ByteSwap => self.byte_swap(operand),
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
            (no_op = $crate::abs::expr::UnaryOp::NoOp)
            (not = $crate::abs::expr::UnaryOp::Not)
            (neg = $crate::abs::expr::UnaryOp::Neg)
            (ptr_metadata = $crate::abs::expr::UnaryOp::PtrMetadata)
            (bit_reverse = $crate::abs::expr::UnaryOp::BitReverse)
            (trailing_zeros + non_zero: bool =
                if non_zero {
                    $crate::abs::expr::UnaryOp::NonZeroTrailingZeros
                } else {
                    $crate::abs::expr::UnaryOp::TrailingZeros
                })
            (count_ones = $crate::abs::expr::UnaryOp::CountOnes)
            (leading_zeros + non_zero: bool =
                if non_zero {
                    $crate::abs::expr::UnaryOp::NonZeroLeadingZeros
                } else {
                    $crate::abs::expr::UnaryOp::LeadingZeros
                })
            (byte_swap = $crate::abs::expr::UnaryOp::ByteSwap)
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_ternary_op_through_singulars {
    () => {
        fn ternary_op<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            op: $crate::abs::expr::TernaryOp,
        ) -> Self::Expr<'a> {
            use $crate::abs::expr::TernaryOp::*;
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
            (if_then_else = $crate::abs::expr::TernaryOp::IfThenElse)
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_cast_through_singulars {
    () => {
        fn cast<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            target: $crate::abs::CastKind<
                Self::IntType,
                Self::FloatType,
                Self::PtrType,
                Self::GenericType,
            >,
            metadata: Self::Metadata<'a>,
        ) -> Self::Expr<'a> {
            use $crate::abs::CastKind;
            match target {
                CastKind::ToChar => self.to_char(operand, metadata),
                CastKind::ToInt(ty) => self.to_int(operand, ty, metadata),
                CastKind::ToFloat(ty) => self.to_float(operand, ty, metadata),
                CastKind::ToPointer(ty) => self.to_ptr(operand, ty, metadata),
                CastKind::PointerUnsize => self.ptr_unsize(operand, metadata),
                CastKind::ExposeProvenance => self.expose_prov(operand, metadata),
                CastKind::Transmute(ty) => self.transmute(operand, ty, metadata),
                CastKind::Subtype(ty) => self.subtype(operand, ty, metadata),
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
            (to_char = $crate::abs::CastKind::ToChar)
            (to_int + ty: Self::IntType = $crate::abs::CastKind::ToInt(ty))
            (to_float + ty: Self::FloatType = $crate::abs::CastKind::ToFloat(ty))
            (to_ptr + ty: Self::PtrType = $crate::abs::CastKind::ToPointer(ty))
            (ptr_unsize = $crate::abs::CastKind::PointerUnsize)
            (expose_prov = $crate::abs::CastKind::ExposeProvenance)
            (transmute + ty: Self::GenericType = $crate::abs::CastKind::Transmute(ty))
            (subtype + ty: Self::GenericType = $crate::abs::CastKind::Subtype(ty))
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
    impl_general_unary_op_through_singulars, impl_singular_binary_ops_through_general,
    impl_singular_cast_through_general, impl_singular_casts_through_general,
    impl_singular_ternary_op_through_general, impl_singular_ternary_ops_through_general,
    impl_singular_unary_op_through_general, impl_singular_unary_ops_through_general,
    macro_rules_method_with_optional_args, repeat_macro_for,
};
