use common::{
    pri::{BasicBlockIndex, BasicBlockLocation},
    types::InstanceKindId,
};

pub(crate) trait BasicBlockLocationExt {
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation<Self>
    where
        Self: Copy;
}

impl<T> BasicBlockLocationExt for T {
    #[inline]
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation<Self>
    where
        Self: Copy,
    {
        BasicBlockLocation { body: self, index }
    }
}
