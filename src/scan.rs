use crate::block::TxSize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxClass {
    TwoD,
    Vert,
    Horiz,
}

pub fn get_tx_class(tx_type: usize) -> TxClass {
    match tx_type {
        10 | 12 | 14 => TxClass::Vert,
        11 | 13 | 15 => TxClass::Horiz,
        _ => TxClass::TwoD,
    }
}

pub fn get_scan_for_tx(tx_size: TxSize, tx_type: usize) -> &'static [u16] {
    let tx_class = get_tx_class(tx_type);
    let cw = tx_size.coded_width();
    let ch = tx_size.coded_height();
    match tx_class {
        TxClass::Vert => get_mrow_scan(cw, ch),
        TxClass::Horiz => get_mcol_scan(cw, ch),
        TxClass::TwoD => get_default_scan_table(cw, ch),
    }
}

pub fn get_scan(tx_size: TxSize) -> &'static [u16] {
    get_scan_for_tx(tx_size, 0)
}

fn get_default_scan_table(cw: usize, ch: usize) -> &'static [u16] {
    match (cw, ch) {
        (4, 4) => &DEFAULT_SCAN_4X4,
        (8, 8) => &DEFAULT_SCAN_8X8,
        (16, 16) => &DEFAULT_SCAN_16X16,
        (32, 32) => &DEFAULT_SCAN_32X32,
        _ => get_rect_scan(cw, ch),
    }
}

fn get_rect_scan(coded_w: usize, coded_h: usize) -> &'static [u16] {
    static RECT_SCANS: OnceLock<std::collections::HashMap<(usize, usize), Vec<u16>>> =
        OnceLock::new();
    let map = RECT_SCANS.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for &(w, h) in &[
            (4, 8),
            (8, 4),
            (4, 16),
            (16, 4),
            (8, 16),
            (16, 8),
            (8, 32),
            (32, 8),
            (16, 32),
            (32, 16),
            (4, 32),
            (32, 4),
        ] {
            m.insert((w, h), generate_default_scan(w, h));
        }
        m
    });
    map.get(&(coded_w, coded_h))
        .expect("unsupported rect scan size")
}

#[rustfmt::skip]
pub const DEFAULT_SCAN_4X4: [u16; 16] = [
    0, 4, 1, 8, 5, 2, 12, 9, 6, 3, 13, 10, 7, 14, 11, 15
];

#[rustfmt::skip]
pub const DEFAULT_SCAN_8X8: [u16; 64] = [
    0, 8, 1, 16, 9, 2, 24, 17, 10, 3, 32, 25, 18, 11, 4, 40,
    33, 26, 19, 12, 5, 48, 41, 34, 27, 20, 13, 6, 56, 49, 42, 35,
    28, 21, 14, 7, 57, 50, 43, 36, 29, 22, 15, 58, 51, 44, 37, 30,
    23, 59, 52, 45, 38, 31, 60, 53, 46, 39, 61, 54, 47, 62, 55, 63,
];

#[rustfmt::skip]
pub const DEFAULT_SCAN_16X16: [u16; 256] = [
    0,   16,  1,   32,  17,  2,   48,  33,  18,  3,   64,  49,  34,  19,  4,
    80,  65,  50,  35,  20,  5,   96,  81,  66,  51,  36,  21,  6,   112, 97,
    82,  67,  52,  37,  22,  7,   128, 113, 98,  83,  68,  53,  38,  23,  8,
    144, 129, 114, 99,  84,  69,  54,  39,  24,  9,   160, 145, 130, 115, 100,
    85,  70,  55,  40,  25,  10,  176, 161, 146, 131, 116, 101, 86,  71,  56,
    41,  26,  11,  192, 177, 162, 147, 132, 117, 102, 87,  72,  57,  42,  27,
    12,  208, 193, 178, 163, 148, 133, 118, 103, 88,  73,  58,  43,  28,  13,
    224, 209, 194, 179, 164, 149, 134, 119, 104, 89,  74,  59,  44,  29,  14,
    240, 225, 210, 195, 180, 165, 150, 135, 120, 105, 90,  75,  60,  45,  30,
    15,  241, 226, 211, 196, 181, 166, 151, 136, 121, 106, 91,  76,  61,  46,
    31,  242, 227, 212, 197, 182, 167, 152, 137, 122, 107, 92,  77,  62,  47,
    243, 228, 213, 198, 183, 168, 153, 138, 123, 108, 93,  78,  63,  244, 229,
    214, 199, 184, 169, 154, 139, 124, 109, 94,  79,  245, 230, 215, 200, 185,
    170, 155, 140, 125, 110, 95,  246, 231, 216, 201, 186, 171, 156, 141, 126,
    111, 247, 232, 217, 202, 187, 172, 157, 142, 127, 248, 233, 218, 203, 188,
    173, 158, 143, 249, 234, 219, 204, 189, 174, 159, 250, 235, 220, 205, 190,
    175, 251, 236, 221, 206, 191, 252, 237, 222, 207, 253, 238, 223, 254, 239,
    255,
];

#[rustfmt::skip]
pub const DEFAULT_SCAN_32X32: [u16; 1024] = [
    0,   32,   1,    64,   33,  2,   96,   65,  34,   3,    128,  97,  66,  35,
    4,   160,  129,  98,   67,  36,  5,    192, 161,  130,  99,   68,  37,  6,
    224, 193,  162,  131,  100, 69,  38,   7,   256,  225,  194,  163, 132, 101,
    70,  39,   8,    288,  257, 226, 195,  164, 133,  102,  71,   40,  9,   320,
    289, 258,  227,  196,  165, 134, 103,  72,  41,   10,   352,  321, 290, 259,
    228, 197,  166,  135,  104, 73,  42,   11,  384,  353,  322,  291, 260, 229,
    198, 167,  136,  105,  74,  43,  12,   416, 385,  354,  323,  292, 261, 230,
    199, 168,  137,  106,  75,  44,  13,   448, 417,  386,  355,  324, 293, 262,
    231, 200,  169,  138,  107, 76,  45,   14,  480,  449,  418,  387, 356, 325,
    294, 263,  232,  201,  170, 139, 108,  77,  46,   15,   512,  481, 450, 419,
    388, 357,  326,  295,  264, 233, 202,  171, 140,  109,  78,   47,  16,  544,
    513, 482,  451,  420,  389, 358, 327,  296, 265,  234,  203,  172, 141, 110,
    79,  48,   17,   576,  545, 514, 483,  452, 421,  390,  359,  328, 297, 266,
    235, 204,  173,  142,  111, 80,  49,   18,  608,  577,  546,  515, 484, 453,
    422, 391,  360,  329,  298, 267, 236,  205, 174,  143,  112,  81,  50,  19,
    640, 609,  578,  547,  516, 485, 454,  423, 392,  361,  330,  299, 268, 237,
    206, 175,  144,  113,  82,  51,  20,   672, 641,  610,  579,  548, 517, 486,
    455, 424,  393,  362,  331, 300, 269,  238, 207,  176,  145,  114, 83,  52,
    21,  704,  673,  642,  611, 580, 549,  518, 487,  456,  425,  394, 363, 332,
    301, 270,  239,  208,  177, 146, 115,  84,  53,   22,   736,  705, 674, 643,
    612, 581,  550,  519,  488, 457, 426,  395, 364,  333,  302,  271, 240, 209,
    178, 147,  116,  85,   54,  23,  768,  737, 706,  675,  644,  613, 582, 551,
    520, 489,  458,  427,  396, 365, 334,  303, 272,  241,  210,  179, 148, 117,
    86,  55,   24,   800,  769, 738, 707,  676, 645,  614,  583,  552, 521, 490,
    459, 428,  397,  366,  335, 304, 273,  242, 211,  180,  149,  118, 87,  56,
    25,  832,  801,  770,  739, 708, 677,  646, 615,  584,  553,  522, 491, 460,
    429, 398,  367,  336,  305, 274, 243,  212, 181,  150,  119,  88,  57,  26,
    864, 833,  802,  771,  740, 709, 678,  647, 616,  585,  554,  523, 492, 461,
    430, 399,  368,  337,  306, 275, 244,  213, 182,  151,  120,  89,  58,  27,
    896, 865,  834,  803,  772, 741, 710,  679, 648,  617,  586,  555, 524, 493,
    462, 431,  400,  369,  338, 307, 276,  245, 214,  183,  152,  121, 90,  59,
    28,  928,  897,  866,  835, 804, 773,  742, 711,  680,  649,  618, 587, 556,
    525, 494,  463,  432,  401, 370, 339,  308, 277,  246,  215,  184, 153, 122,
    91,  60,   29,   960,  929, 898, 867,  836, 805,  774,  743,  712, 681, 650,
    619, 588,  557,  526,  495, 464, 433,  402, 371,  340,  309,  278, 247, 216,
    185, 154,  123,  92,   61,  30,  992,  961, 930,  899,  868,  837, 806, 775,
    744, 713,  682,  651,  620, 589, 558,  527, 496,  465,  434,  403, 372, 341,
    310, 279,  248,  217,  186, 155, 124,  93,  62,   31,   993,  962, 931, 900,
    869, 838,  807,  776,  745, 714, 683,  652, 621,  590,  559,  528, 497, 466,
    435, 404,  373,  342,  311, 280, 249,  218, 187,  156,  125,  94,  63,  994,
    963, 932,  901,  870,  839, 808, 777,  746, 715,  684,  653,  622, 591, 560,
    529, 498,  467,  436,  405, 374, 343,  312, 281,  250,  219,  188, 157, 126,
    95,  995,  964,  933,  902, 871, 840,  809, 778,  747,  716,  685, 654, 623,
    592, 561,  530,  499,  468, 437, 406,  375, 344,  313,  282,  251, 220, 189,
    158, 127,  996,  965,  934, 903, 872,  841, 810,  779,  748,  717, 686, 655,
    624, 593,  562,  531,  500, 469, 438,  407, 376,  345,  314,  283, 252, 221,
    190, 159,  997,  966,  935, 904, 873,  842, 811,  780,  749,  718, 687, 656,
    625, 594,  563,  532,  501, 470, 439,  408, 377,  346,  315,  284, 253, 222,
    191, 998,  967,  936,  905, 874, 843,  812, 781,  750,  719,  688, 657, 626,
    595, 564,  533,  502,  471, 440, 409,  378, 347,  316,  285,  254, 223, 999,
    968, 937,  906,  875,  844, 813, 782,  751, 720,  689,  658,  627, 596, 565,
    534, 503,  472,  441,  410, 379, 348,  317, 286,  255,  1000, 969, 938, 907,
    876, 845,  814,  783,  752, 721, 690,  659, 628,  597,  566,  535, 504, 473,
    442, 411,  380,  349,  318, 287, 1001, 970, 939,  908,  877,  846, 815, 784,
    753, 722,  691,  660,  629, 598, 567,  536, 505,  474,  443,  412, 381, 350,
    319, 1002, 971,  940,  909, 878, 847,  816, 785,  754,  723,  692, 661, 630,
    599, 568,  537,  506,  475, 444, 413,  382, 351,  1003, 972,  941, 910, 879,
    848, 817,  786,  755,  724, 693, 662,  631, 600,  569,  538,  507, 476, 445,
    414, 383,  1004, 973,  942, 911, 880,  849, 818,  787,  756,  725, 694, 663,
    632, 601,  570,  539,  508, 477, 446,  415, 1005, 974,  943,  912, 881, 850,
    819, 788,  757,  726,  695, 664, 633,  602, 571,  540,  509,  478, 447, 1006,
    975, 944,  913,  882,  851, 820, 789,  758, 727,  696,  665,  634, 603, 572,
    541, 510,  479,  1007, 976, 945, 914,  883, 852,  821,  790,  759, 728, 697,
    666, 635,  604,  573,  542, 511, 1008, 977, 946,  915,  884,  853, 822, 791,
    760, 729,  698,  667,  636, 605, 574,  543, 1009, 978,  947,  916, 885, 854,
    823, 792,  761,  730,  699, 668, 637,  606, 575,  1010, 979,  948, 917, 886,
    855, 824,  793,  762,  731, 700, 669,  638, 607,  1011, 980,  949, 918, 887,
    856, 825,  794,  763,  732, 701, 670,  639, 1012, 981,  950,  919, 888, 857,
    826, 795,  764,  733,  702, 671, 1013, 982, 951,  920,  889,  858, 827, 796,
    765, 734,  703,  1014, 983, 952, 921,  890, 859,  828,  797,  766, 735, 1015,
    984, 953,  922,  891,  860, 829, 798,  767, 1016, 985,  954,  923, 892, 861,
    830, 799,  1017, 986,  955, 924, 893,  862, 831,  1018, 987,  956, 925, 894,
    863, 1019, 988,  957,  926, 895, 1020, 989, 958,  927,  1021, 990, 959, 1022,
    991, 1023,
];

fn get_mrow_scan(coded_w: usize, coded_h: usize) -> &'static [u16] {
    static MROW_SCANS: OnceLock<std::collections::HashMap<(usize, usize), Vec<u16>>> =
        OnceLock::new();
    let map = MROW_SCANS.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for &(w, h) in &ALL_SCAN_SIZES {
            m.insert((w, h), generate_mrow_scan(w, h));
        }
        m
    });
    map.get(&(coded_w, coded_h))
        .expect("unsupported mrow scan size")
}

fn get_mcol_scan(coded_w: usize, coded_h: usize) -> &'static [u16] {
    static MCOL_SCANS: OnceLock<std::collections::HashMap<(usize, usize), Vec<u16>>> =
        OnceLock::new();
    let map = MCOL_SCANS.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for &(w, h) in &ALL_SCAN_SIZES {
            m.insert((w, h), generate_mcol_scan(w, h));
        }
        m
    });
    map.get(&(coded_w, coded_h))
        .expect("unsupported mcol scan size")
}

const ALL_SCAN_SIZES: [(usize, usize); 16] = [
    (4, 4),
    (8, 8),
    (16, 16),
    (32, 32),
    (4, 8),
    (8, 4),
    (4, 16),
    (16, 4),
    (8, 16),
    (16, 8),
    (8, 32),
    (32, 8),
    (16, 32),
    (32, 16),
    (4, 32),
    (32, 4),
];

fn generate_mrow_scan(coded_w: usize, coded_h: usize) -> Vec<u16> {
    let mut scan = Vec::with_capacity(coded_w * coded_h);
    for row in 0..coded_h {
        for col in 0..coded_w {
            scan.push((row * coded_w + col) as u16);
        }
    }
    scan
}

fn generate_mcol_scan(coded_w: usize, coded_h: usize) -> Vec<u16> {
    let mut scan = Vec::with_capacity(coded_w * coded_h);
    for col in 0..coded_w {
        for row in 0..coded_h {
            scan.push((row * coded_w + col) as u16);
        }
    }
    scan
}

fn generate_default_scan(coded_w: usize, coded_h: usize) -> Vec<u16> {
    let mut scan = Vec::with_capacity(coded_w * coded_h);
    for sum in 0..(coded_w + coded_h - 1) {
        let x_start = if sum < coded_h { 0 } else { sum - coded_h + 1 };
        let x_end = sum.min(coded_w - 1);
        for x in x_start..=x_end {
            let y = sum - x;
            scan.push((y * coded_w + x) as u16);
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_is_first() {
        assert_eq!(DEFAULT_SCAN_32X32[0], 0);
    }

    #[test]
    fn all_entries_unique() {
        let mut seen = [false; 1024];
        for &val in &DEFAULT_SCAN_32X32 {
            assert!(!seen[val as usize], "duplicate entry: {}", val);
            seen[val as usize] = true;
        }
    }

    #[test]
    fn all_entries_in_range() {
        for &val in &DEFAULT_SCAN_32X32 {
            assert!(val < 1024, "out of range: {}", val);
        }
    }

    #[test]
    fn scan_second_entry_is_col0_row1() {
        assert_eq!(DEFAULT_SCAN_32X32[1], 32);
    }

    #[test]
    fn scan_third_entry_is_col1_row0() {
        assert_eq!(DEFAULT_SCAN_32X32[2], 1);
    }

    #[test]
    fn scan_4x4_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_4X4[0], 0);
    }

    #[test]
    fn scan_4x4_all_unique() {
        let mut seen = [false; 16];
        for &val in &DEFAULT_SCAN_4X4 {
            assert!(!seen[val as usize], "dup: {}", val);
            seen[val as usize] = true;
        }
    }

    #[test]
    fn scan_4x4_all_in_range() {
        for &val in &DEFAULT_SCAN_4X4 {
            assert!(val < 16, "out of range: {}", val);
        }
    }

    #[test]
    fn scan_4x4_diagonal_order() {
        assert_eq!(DEFAULT_SCAN_4X4[1], 4);
        assert_eq!(DEFAULT_SCAN_4X4[2], 1);
        assert_eq!(DEFAULT_SCAN_4X4[15], 15);
    }

    #[test]
    fn scan_8x8_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_8X8[0], 0);
    }

    #[test]
    fn scan_8x8_all_unique() {
        let mut seen = [false; 64];
        for &val in &DEFAULT_SCAN_8X8 {
            assert!(!seen[val as usize], "dup: {}", val);
            seen[val as usize] = true;
        }
    }

    #[test]
    fn scan_8x8_all_in_range() {
        for &val in &DEFAULT_SCAN_8X8 {
            assert!(val < 64, "out of range: {}", val);
        }
    }

    #[test]
    fn scan_16x16_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_16X16[0], 0);
    }

    #[test]
    fn scan_16x16_all_unique() {
        let mut seen = [false; 256];
        for &val in &DEFAULT_SCAN_16X16 {
            assert!(!seen[val as usize], "dup: {}", val);
            seen[val as usize] = true;
        }
    }

    #[test]
    fn scan_16x16_all_in_range() {
        for &val in &DEFAULT_SCAN_16X16 {
            assert!(val < 256, "out of range: {}", val);
        }
    }

    #[test]
    fn scan_16x16_second_entry() {
        assert_eq!(DEFAULT_SCAN_16X16[1], 16);
    }

    #[test]
    fn get_scan_dispatch() {
        use crate::block::TxSize;
        assert_eq!(get_scan(TxSize::Tx4x4).len(), 16);
        assert_eq!(get_scan(TxSize::Tx8x8).len(), 64);
        assert_eq!(get_scan(TxSize::Tx16x16).len(), 256);
        assert_eq!(get_scan(TxSize::Tx32x32).len(), 1024);
        assert_eq!(get_scan(TxSize::Tx64x64).len(), 1024);
    }

    #[test]
    fn rect_scan_sizes() {
        use crate::block::TxSize;
        assert_eq!(get_scan(TxSize::Tx4x8).len(), 32);
        assert_eq!(get_scan(TxSize::Tx8x4).len(), 32);
        assert_eq!(get_scan(TxSize::Tx8x16).len(), 128);
        assert_eq!(get_scan(TxSize::Tx16x8).len(), 128);
        assert_eq!(get_scan(TxSize::Tx16x32).len(), 512);
        assert_eq!(get_scan(TxSize::Tx32x16).len(), 512);
        assert_eq!(get_scan(TxSize::Tx32x64).len(), 1024);
        assert_eq!(get_scan(TxSize::Tx64x32).len(), 1024);
    }

    #[test]
    fn generated_scan_matches_4x4() {
        let scan = generate_default_scan(4, 4);
        assert_eq!(scan.as_slice(), &DEFAULT_SCAN_4X4[..]);
    }

    #[test]
    fn generated_scan_matches_8x8() {
        let scan = generate_default_scan(8, 8);
        assert_eq!(scan.as_slice(), &DEFAULT_SCAN_8X8[..]);
    }
}
