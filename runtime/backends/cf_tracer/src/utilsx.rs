use crate::abs::IntType;

pub(crate) trait IntTypeExt {
    fn bit_mask(bit_size: u32) -> u128;
    fn all_one(&self) -> u128;
    fn masked(&self, bit_rep: u128) -> u128;
    fn signed_masked(&self, bit_rep: u128) -> i128;
}

impl IntTypeExt for IntType {
    #[inline]
    fn bit_mask(bit_size: u32) -> u128 {
        u128::MAX >> (u128::BITS - bit_size)
    }

    #[inline]
    fn all_one(&self) -> u128 {
        Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    fn masked(&self, bit_rep: u128) -> u128 {
        bit_rep & Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    fn signed_masked(&self, bit_rep: u128) -> i128 {
        (bit_rep as i128) << (128 - self.bit_size) >> (128 - self.bit_size)
    }
}
