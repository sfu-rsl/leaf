use super::IntType;

impl IntType {
    #[inline]
    pub(crate) fn bit_mask(bit_size: u32) -> u128 {
        u128::MAX >> (u128::BITS - bit_size)
    }

    #[inline]
    pub(crate) fn all_one(&self) -> u128 {
        Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    pub(crate) fn masked(&self, bit_rep: u128) -> u128 {
        bit_rep & Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    pub(crate) fn signed_masked(&self, bit_rep: u128) -> i128 {
        (bit_rep as i128) << (128 - self.bit_size) >> (128 - self.bit_size)
    }
}
