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
    Tx4x8,
    Tx8x4,
    Tx4x16,
    Tx16x4,
    Tx8x16,
    Tx16x8,
    Tx8x32,
    Tx32x8,
    Tx16x32,
    Tx32x16,
    Tx16x64,
    Tx64x16,
    Tx32x64,
    Tx64x32,
}

const AV2_TXFORM_COEFF_LIMIT: usize = 32;

impl TxSize {
    pub fn from_block_size(bs: BlockSize) -> TxSize {
        max_txsize_rect_for_block(bs.width(), bs.height())
    }

    pub fn width(&self) -> usize {
        match self {
            TxSize::Tx4x4 => 4,
            TxSize::Tx8x8 => 8,
            TxSize::Tx16x16 => 16,
            TxSize::Tx32x32 => 32,
            TxSize::Tx64x64 => 64,
            TxSize::Tx4x8 | TxSize::Tx4x16 => 4,
            TxSize::Tx8x4 | TxSize::Tx8x16 | TxSize::Tx8x32 => 8,
            TxSize::Tx16x4 | TxSize::Tx16x8 | TxSize::Tx16x32 | TxSize::Tx16x64 => 16,
            TxSize::Tx32x8 | TxSize::Tx32x16 | TxSize::Tx32x64 => 32,
            TxSize::Tx64x16 | TxSize::Tx64x32 => 64,
        }
    }

    pub fn height(&self) -> usize {
        match self {
            TxSize::Tx4x4 => 4,
            TxSize::Tx8x8 => 8,
            TxSize::Tx16x16 => 16,
            TxSize::Tx32x32 => 32,
            TxSize::Tx64x64 => 64,
            TxSize::Tx8x4 | TxSize::Tx16x4 => 4,
            TxSize::Tx4x8 | TxSize::Tx16x8 | TxSize::Tx32x8 => 8,
            TxSize::Tx4x16 | TxSize::Tx8x16 | TxSize::Tx32x16 | TxSize::Tx64x16 => 16,
            TxSize::Tx8x32 | TxSize::Tx16x32 | TxSize::Tx64x32 => 32,
            TxSize::Tx16x64 | TxSize::Tx32x64 => 64,
        }
    }

    pub fn coded_width(&self) -> usize {
        self.width().min(AV2_TXFORM_COEFF_LIMIT)
    }

    pub fn coded_height(&self) -> usize {
        self.height().min(AV2_TXFORM_COEFF_LIMIT)
    }

    pub fn bwl(&self) -> u32 {
        match self.coded_width() {
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            _ => unreachable!(),
        }
    }

    pub fn num_coeffs(&self) -> usize {
        self.coded_width() * self.coded_height()
    }

    pub fn log_scale(&self) -> u32 {
        let els = self.width() * self.height();
        (els > 256) as u32 + (els > 1024) as u32
    }

    pub fn txs_ctx(&self) -> usize {
        let max_dim = self.width().max(self.height());
        match max_dim {
            4 => 0,
            8 => 1,
            16 => 2,
            32 => 3,
            64 => 4,
            _ => unreachable!(),
        }
    }

    pub fn inv_shifts(&self) -> (u32, u32) {
        match self {
            TxSize::Tx4x4 => (7, 10),
            TxSize::Tx8x8 => (7, 11),
            TxSize::Tx16x16 => (6, 13),
            TxSize::Tx32x32 => (6, 13),
            TxSize::Tx64x64 => (6, 13),
            TxSize::Tx4x8 | TxSize::Tx8x4 => (7, 10),
            TxSize::Tx8x16 | TxSize::Tx16x8 => (7, 11),
            TxSize::Tx16x32 | TxSize::Tx32x16 => (6, 12),
            TxSize::Tx32x64 | TxSize::Tx64x32 => (6, 12),
            TxSize::Tx4x16 | TxSize::Tx16x4 => (6, 12),
            TxSize::Tx8x32 | TxSize::Tx32x8 => (6, 13),
            TxSize::Tx16x64 | TxSize::Tx64x16 => (6, 13),
        }
    }

    pub fn needs_sqrt2(&self) -> bool {
        let log2w = (self.width() as f64).log2() as u32;
        let log2h = (self.height() as f64).log2() as u32;
        (log2w + log2h) & 1 != 0
    }
}

pub fn max_txsize_rect_for_block(width: usize, height: usize) -> TxSize {
    match (width, height) {
        (4, 4) => TxSize::Tx4x4,
        (4, 8) => TxSize::Tx4x8,
        (8, 4) => TxSize::Tx8x4,
        (8, 8) => TxSize::Tx8x8,
        (8, 16) => TxSize::Tx8x16,
        (16, 8) => TxSize::Tx16x8,
        (16, 16) => TxSize::Tx16x16,
        (16, 32) => TxSize::Tx16x32,
        (32, 16) => TxSize::Tx32x16,
        (32, 32) => TxSize::Tx32x32,
        (32, 64) => TxSize::Tx32x64,
        (64, 32) => TxSize::Tx64x32,
        (64, 64) | (128, 128) | (64, 128) | (128, 64) => TxSize::Tx64x64,
        (4, 16) => TxSize::Tx4x16,
        (16, 4) => TxSize::Tx16x4,
        (8, 32) => TxSize::Tx8x32,
        (32, 8) => TxSize::Tx32x8,
        (16, 64) => TxSize::Tx16x64,
        (64, 16) => TxSize::Tx64x16,
        _ => {
            let w = width.min(64);
            let h = height.min(64);
            if w == width && h == height {
                if h > w {
                    max_txsize_rect_for_block(w, h / 2)
                } else {
                    max_txsize_rect_for_block(w / 2, h)
                }
            } else {
                max_txsize_rect_for_block(w, h)
            }
        }
    }
}

pub fn tx_size_for_block(width: usize, height: usize) -> TxSize {
    max_txsize_rect_for_block(width, height)
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
