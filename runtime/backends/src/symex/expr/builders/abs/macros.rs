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
            op: $crate::symex::expr::builders::abs::BinaryOp,
        ) -> Self::Expr<'a> {
            use $crate::symex::expr::builders::abs::BinaryOp::*;
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
                CarrylessMul => self.carryless_mul(operands),
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
            (add = $crate::symex::expr::builders::abs::BinaryOp::Add)
            (add_unchecked = $crate::symex::expr::builders::abs::BinaryOp::AddUnchecked)
            (add_with_overflow = $crate::symex::expr::builders::abs::BinaryOp::AddWithOverflow)
            (add_saturating = $crate::symex::expr::builders::abs::BinaryOp::AddSaturating)
            (sub = $crate::symex::expr::builders::abs::BinaryOp::Sub)
            (sub_unchecked = $crate::symex::expr::builders::abs::BinaryOp::SubUnchecked)
            (sub_with_overflow = $crate::symex::expr::builders::abs::BinaryOp::SubWithOverflow)
            (sub_saturating = $crate::symex::expr::builders::abs::BinaryOp::SubSaturating)
            (mul = $crate::symex::expr::builders::abs::BinaryOp::Mul)
            (mul_unchecked = $crate::symex::expr::builders::abs::BinaryOp::MulUnchecked)
            (mul_with_overflow = $crate::symex::expr::builders::abs::BinaryOp::MulWithOverflow)
            (div = $crate::symex::expr::builders::abs::BinaryOp::Div)
            (div_exact = $crate::symex::expr::builders::abs::BinaryOp::DivExact)
            (rem = $crate::symex::expr::builders::abs::BinaryOp::Rem)
            (xor = $crate::symex::expr::builders::abs::BinaryOp::BitXor)
            (and = $crate::symex::expr::builders::abs::BinaryOp::BitAnd)
            (or = $crate::symex::expr::builders::abs::BinaryOp::BitOr)
            (shl = $crate::symex::expr::builders::abs::BinaryOp::Shl)
            (shl_unchecked = $crate::symex::expr::builders::abs::BinaryOp::ShlUnchecked)
            (shr = $crate::symex::expr::builders::abs::BinaryOp::Shr)
            (shr_unchecked = $crate::symex::expr::builders::abs::BinaryOp::ShrUnchecked)
            (rotate_left = $crate::symex::expr::builders::abs::BinaryOp::RotateL)
            (rotate_right = $crate::symex::expr::builders::abs::BinaryOp::RotateR)
            (carryless_mul = $crate::symex::expr::builders::abs::BinaryOp::CarrylessMul)
            (eq = $crate::symex::expr::builders::abs::BinaryOp::Eq)
            (lt = $crate::symex::expr::builders::abs::BinaryOp::Lt)
            (le = $crate::symex::expr::builders::abs::BinaryOp::Le)
            (ne = $crate::symex::expr::builders::abs::BinaryOp::Ne)
            (ge = $crate::symex::expr::builders::abs::BinaryOp::Ge)
            (gt = $crate::symex::expr::builders::abs::BinaryOp::Gt)
            (cmp = $crate::symex::expr::builders::abs::BinaryOp::Cmp)
            (offset + pointee_size: TypeSize = $crate::symex::expr::builders::abs::BinaryOp::Offset(pointee_size))
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_unary_op_through_singulars {
    () => {
        fn unary_op<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            op: $crate::symex::expr::builders::abs::UnaryOp,
        ) -> Self::Expr<'a> {
            use $crate::symex::expr::builders::abs::UnaryOp::*;
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
            (no_op = $crate::symex::expr::builders::abs::UnaryOp::NoOp)
            (not = $crate::symex::expr::builders::abs::UnaryOp::Not)
            (neg = $crate::symex::expr::builders::abs::UnaryOp::Neg)
            (ptr_metadata = $crate::symex::expr::builders::abs::UnaryOp::PtrMetadata)
            (bit_reverse = $crate::symex::expr::builders::abs::UnaryOp::BitReverse)
            (trailing_zeros + non_zero: bool =
                if non_zero {
                    $crate::symex::expr::builders::abs::UnaryOp::NonZeroTrailingZeros
                } else {
                    $crate::symex::expr::builders::abs::UnaryOp::TrailingZeros
                })
            (count_ones = $crate::symex::expr::builders::abs::UnaryOp::CountOnes)
            (leading_zeros + non_zero: bool =
                if non_zero {
                    $crate::symex::expr::builders::abs::UnaryOp::NonZeroLeadingZeros
                } else {
                    $crate::symex::expr::builders::abs::UnaryOp::LeadingZeros
                })
            (byte_swap = $crate::symex::expr::builders::abs::UnaryOp::ByteSwap)
        );
    };
}

#[allow(unused_macros)]
macro_rules! impl_general_ternary_op_through_singulars {
    () => {
        fn ternary_op<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            op: $crate::symex::expr::builders::abs::TernaryOp,
        ) -> Self::Expr<'a> {
            use $crate::symex::expr::builders::abs::TernaryOp::*;
            match op {
                IfThenElse => self.if_then_else(operands),
                FunnelShl => self.funnel_shl(operands),
                FunnelShr => self.funnel_shr(operands),
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
            (if_then_else = $crate::symex::expr::builders::abs::TernaryOp::IfThenElse)
            (funnel_shl = $crate::symex::expr::builders::abs::TernaryOp::FunnelShl)
            (funnel_shr = $crate::symex::expr::builders::abs::TernaryOp::FunnelShr)
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
