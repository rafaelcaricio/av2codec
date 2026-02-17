pub const PARTITION_PLOFFSET: usize = 4;
pub const PARTITION_BLOCK_GROUPS: usize = 16;
pub const PARTITION_CONTEXTS: usize = 64;
pub const SQUARE_SPLIT_CONTEXTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockSize {
    Block4x4,
    Block4x8,
    Block8x4,
    Block8x8,
    Block8x16,
    Block16x8,
    Block16x16,
    Block16x32,
    Block32x16,
    Block32x32,
    Block32x64,
    Block64x32,
    Block64x64,
}

impl BlockSize {
    pub fn width(&self) -> usize {
        match self {
            BlockSize::Block4x4 => 4,
            BlockSize::Block4x8 => 4,
            BlockSize::Block8x4 => 8,
            BlockSize::Block8x8 => 8,
            BlockSize::Block8x16 => 8,
            BlockSize::Block16x8 => 16,
            BlockSize::Block16x16 => 16,
            BlockSize::Block16x32 => 16,
            BlockSize::Block32x16 => 32,
            BlockSize::Block32x32 => 32,
            BlockSize::Block32x64 => 32,
            BlockSize::Block64x32 => 64,
            BlockSize::Block64x64 => 64,
        }
    }

    pub fn height(&self) -> usize {
        match self {
            BlockSize::Block4x4 => 4,
            BlockSize::Block4x8 => 8,
            BlockSize::Block8x4 => 4,
            BlockSize::Block8x8 => 8,
            BlockSize::Block8x16 => 16,
            BlockSize::Block16x8 => 8,
            BlockSize::Block16x16 => 16,
            BlockSize::Block16x32 => 32,
            BlockSize::Block32x16 => 16,
            BlockSize::Block32x32 => 32,
            BlockSize::Block32x64 => 64,
            BlockSize::Block64x32 => 32,
            BlockSize::Block64x64 => 64,
        }
    }

    pub fn bwl(&self) -> u32 {
        (self.width() as f64).log2() as u32
    }

    pub fn bhl(&self) -> u32 {
        (self.height() as f64).log2() as u32
    }

    pub fn is_square(&self) -> bool {
        self.width() == self.height()
    }

    pub fn tx_size(&self) -> TxSize {
        TxSize::from_block_size(*self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxSize {
    Tx4x4,
    Tx8x8,
    Tx16x16,
    Tx32x32,
    Tx64x64,
}

impl TxSize {
    pub fn from_block_size(bs: BlockSize) -> TxSize {
        let min_dim = bs.width().min(bs.height());
        match min_dim {
            4 => TxSize::Tx4x4,
            8 => TxSize::Tx8x8,
            16 => TxSize::Tx16x16,
            32 => TxSize::Tx32x32,
            64 => TxSize::Tx64x64,
            _ => unreachable!(),
        }
    }

    pub fn width(&self) -> usize {
        match self {
            TxSize::Tx4x4 => 4,
            TxSize::Tx8x8 => 8,
            TxSize::Tx16x16 => 16,
            TxSize::Tx32x32 => 32,
            TxSize::Tx64x64 => 64,
        }
    }

    pub fn height(&self) -> usize {
        self.width()
    }

    pub fn bwl(&self) -> u32 {
        match self {
            TxSize::Tx4x4 => 2,
            TxSize::Tx8x8 => 3,
            TxSize::Tx16x16 => 4,
            TxSize::Tx32x32 => 5,
            TxSize::Tx64x64 => 6,
        }
    }

    pub fn num_coeffs(&self) -> usize {
        match self {
            TxSize::Tx4x4 => 16,
            TxSize::Tx8x8 => 64,
            TxSize::Tx16x16 => 256,
            TxSize::Tx32x32 => 1024,
            TxSize::Tx64x64 => 1024,
        }
    }

    pub fn log_scale(&self) -> u32 {
        match self {
            TxSize::Tx4x4 => 0,
            TxSize::Tx8x8 => 0,
            TxSize::Tx16x16 => 0,
            TxSize::Tx32x32 => 1,
            TxSize::Tx64x64 => 2,
        }
    }

    pub fn txs_ctx(&self) -> usize {
        match self {
            TxSize::Tx4x4 => 0,
            TxSize::Tx8x8 => 1,
            TxSize::Tx16x16 => 2,
            TxSize::Tx32x32 => 3,
            TxSize::Tx64x64 => 4,
        }
    }
}

pub fn tx_size_for_block(width: usize, height: usize) -> TxSize {
    let min_dim = width.min(height);
    if min_dim >= 64 {
        TxSize::Tx64x64
    } else if min_dim >= 32 {
        TxSize::Tx32x32
    } else if min_dim >= 16 {
        TxSize::Tx16x16
    } else if min_dim >= 8 {
        TxSize::Tx8x8
    } else {
        TxSize::Tx4x4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PartitionType {
    None = 0,
    Horz = 1,
    Vert = 2,
    Horz3 = 3,
    Vert3 = 4,
    Horz4A = 5,
    Horz4B = 6,
    Vert4A = 7,
    Vert4B = 8,
    Split = 9,
}

pub fn subsize(partition: PartitionType, bsize: BlockSize) -> Option<BlockSize> {
    match partition {
        PartitionType::None => Some(bsize),
        PartitionType::Split => match bsize {
            BlockSize::Block64x64 => Some(BlockSize::Block32x32),
            BlockSize::Block32x32 => Some(BlockSize::Block16x16),
            BlockSize::Block16x16 => Some(BlockSize::Block8x8),
            BlockSize::Block8x8 => Some(BlockSize::Block4x4),
            _ => Option::None,
        },
        PartitionType::Horz => match bsize {
            BlockSize::Block64x64 => Some(BlockSize::Block64x32),
            BlockSize::Block32x32 => Some(BlockSize::Block32x16),
            BlockSize::Block16x16 => Some(BlockSize::Block16x8),
            BlockSize::Block8x8 => Some(BlockSize::Block8x4),
            _ => Option::None,
        },
        PartitionType::Vert => match bsize {
            BlockSize::Block64x64 => Some(BlockSize::Block32x64),
            BlockSize::Block32x32 => Some(BlockSize::Block16x32),
            BlockSize::Block16x16 => Some(BlockSize::Block8x16),
            BlockSize::Block8x8 => Some(BlockSize::Block4x8),
            _ => Option::None,
        },
        PartitionType::Horz3
        | PartitionType::Vert3
        | PartitionType::Horz4A
        | PartitionType::Horz4B
        | PartitionType::Vert4A
        | PartitionType::Vert4B => Option::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_dimensions() {
        assert_eq!(BlockSize::Block4x4.width(), 4);
        assert_eq!(BlockSize::Block4x4.height(), 4);
        assert_eq!(BlockSize::Block64x64.width(), 64);
        assert_eq!(BlockSize::Block64x64.height(), 64);
        assert_eq!(BlockSize::Block32x16.width(), 32);
        assert_eq!(BlockSize::Block32x16.height(), 16);
    }

    #[test]
    fn block_size_bwl() {
        assert_eq!(BlockSize::Block4x4.bwl(), 2);
        assert_eq!(BlockSize::Block8x8.bwl(), 3);
        assert_eq!(BlockSize::Block16x16.bwl(), 4);
        assert_eq!(BlockSize::Block32x32.bwl(), 5);
        assert_eq!(BlockSize::Block64x64.bwl(), 6);
    }

    #[test]
    fn tx_size_from_block() {
        assert_eq!(TxSize::from_block_size(BlockSize::Block4x4), TxSize::Tx4x4);
        assert_eq!(TxSize::from_block_size(BlockSize::Block8x8), TxSize::Tx8x8);
        assert_eq!(
            TxSize::from_block_size(BlockSize::Block16x16),
            TxSize::Tx16x16
        );
        assert_eq!(
            TxSize::from_block_size(BlockSize::Block32x32),
            TxSize::Tx32x32
        );
        assert_eq!(
            TxSize::from_block_size(BlockSize::Block64x64),
            TxSize::Tx64x64
        );
    }

    #[test]
    fn tx_size_params() {
        assert_eq!(TxSize::Tx4x4.width(), 4);
        assert_eq!(TxSize::Tx4x4.height(), 4);
        assert_eq!(TxSize::Tx4x4.log_scale(), 0);
        assert_eq!(TxSize::Tx4x4.num_coeffs(), 16);

        assert_eq!(TxSize::Tx8x8.width(), 8);
        assert_eq!(TxSize::Tx8x8.log_scale(), 0);
        assert_eq!(TxSize::Tx8x8.num_coeffs(), 64);

        assert_eq!(TxSize::Tx16x16.width(), 16);
        assert_eq!(TxSize::Tx16x16.log_scale(), 0);
        assert_eq!(TxSize::Tx16x16.num_coeffs(), 256);

        assert_eq!(TxSize::Tx32x32.width(), 32);
        assert_eq!(TxSize::Tx32x32.log_scale(), 1);
        assert_eq!(TxSize::Tx32x32.num_coeffs(), 1024);

        assert_eq!(TxSize::Tx64x64.width(), 64);
        assert_eq!(TxSize::Tx64x64.log_scale(), 2);
        assert_eq!(TxSize::Tx64x64.num_coeffs(), 1024);
    }

    #[test]
    fn subsize_split() {
        assert_eq!(
            subsize(PartitionType::Split, BlockSize::Block64x64),
            Some(BlockSize::Block32x32)
        );
        assert_eq!(
            subsize(PartitionType::Split, BlockSize::Block32x32),
            Some(BlockSize::Block16x16)
        );
        assert_eq!(
            subsize(PartitionType::Split, BlockSize::Block16x16),
            Some(BlockSize::Block8x8)
        );
        assert_eq!(
            subsize(PartitionType::Split, BlockSize::Block8x8),
            Some(BlockSize::Block4x4)
        );
        assert_eq!(
            subsize(PartitionType::Split, BlockSize::Block4x4),
            Option::None
        );
    }

    #[test]
    fn subsize_none() {
        assert_eq!(
            subsize(PartitionType::None, BlockSize::Block64x64),
            Some(BlockSize::Block64x64)
        );
        assert_eq!(
            subsize(PartitionType::None, BlockSize::Block4x4),
            Some(BlockSize::Block4x4)
        );
        assert_eq!(
            subsize(PartitionType::None, BlockSize::Block32x16),
            Some(BlockSize::Block32x16)
        );
    }

    #[test]
    fn subsize_horz() {
        assert_eq!(
            subsize(PartitionType::Horz, BlockSize::Block64x64),
            Some(BlockSize::Block64x32)
        );
        assert_eq!(
            subsize(PartitionType::Horz, BlockSize::Block32x32),
            Some(BlockSize::Block32x16)
        );
        assert_eq!(
            subsize(PartitionType::Horz, BlockSize::Block16x16),
            Some(BlockSize::Block16x8)
        );
        assert_eq!(
            subsize(PartitionType::Horz, BlockSize::Block8x8),
            Some(BlockSize::Block8x4)
        );
    }

    #[test]
    fn subsize_vert() {
        assert_eq!(
            subsize(PartitionType::Vert, BlockSize::Block64x64),
            Some(BlockSize::Block32x64)
        );
        assert_eq!(
            subsize(PartitionType::Vert, BlockSize::Block32x32),
            Some(BlockSize::Block16x32)
        );
        assert_eq!(
            subsize(PartitionType::Vert, BlockSize::Block16x16),
            Some(BlockSize::Block8x16)
        );
        assert_eq!(
            subsize(PartitionType::Vert, BlockSize::Block8x8),
            Some(BlockSize::Block4x8)
        );
    }

    #[test]
    fn block_is_square() {
        assert!(BlockSize::Block4x4.is_square());
        assert!(BlockSize::Block8x8.is_square());
        assert!(BlockSize::Block16x16.is_square());
        assert!(BlockSize::Block32x32.is_square());
        assert!(BlockSize::Block64x64.is_square());

        assert!(!BlockSize::Block4x8.is_square());
        assert!(!BlockSize::Block8x4.is_square());
        assert!(!BlockSize::Block8x16.is_square());
        assert!(!BlockSize::Block16x8.is_square());
        assert!(!BlockSize::Block16x32.is_square());
        assert!(!BlockSize::Block32x16.is_square());
        assert!(!BlockSize::Block32x64.is_square());
        assert!(!BlockSize::Block64x32.is_square());
    }

    #[test]
    fn partition_type_values() {
        assert_eq!(PartitionType::None as u8, 0);
        assert_eq!(PartitionType::Horz as u8, 1);
        assert_eq!(PartitionType::Vert as u8, 2);
        assert_eq!(PartitionType::Horz3 as u8, 3);
        assert_eq!(PartitionType::Vert3 as u8, 4);
        assert_eq!(PartitionType::Horz4A as u8, 5);
        assert_eq!(PartitionType::Horz4B as u8, 6);
        assert_eq!(PartitionType::Vert4A as u8, 7);
        assert_eq!(PartitionType::Vert4B as u8, 8);
        assert_eq!(PartitionType::Split as u8, 9);
    }
}
