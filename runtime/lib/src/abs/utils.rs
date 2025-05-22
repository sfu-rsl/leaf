use common::{
    pri::{BasicBlockIndex, BasicBlockLocation},
    types::InstanceKindId,
};

pub(crate) trait InstanceKindIdExt {
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation
    where
        Self: Copy;
}

impl InstanceKindIdExt for InstanceKindId {
    #[inline]
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation
    where
        Self: Copy,
    {
        BasicBlockLocation { body: self, index }
    }
}
