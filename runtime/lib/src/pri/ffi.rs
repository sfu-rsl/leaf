use crate::abs;

impl From<common::pri::BinaryOp> for abs::BinaryOp {
    #[inline(always)]
    fn from(value: common::pri::BinaryOp) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<common::pri::UnaryOp> for abs::UnaryOp {
    #[inline(always)]
    fn from(value: common::pri::UnaryOp) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<common::pri::AtomicOrdering> for abs::AtomicOrdering {
    #[inline(always)]
    fn from(value: common::pri::AtomicOrdering) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<common::pri::AtomicBinaryOp> for abs::AtomicBinaryOp {
    #[inline(always)]
    fn from(value: common::pri::AtomicBinaryOp) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}
