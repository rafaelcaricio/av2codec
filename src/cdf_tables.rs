pub const LF_2D_LIM: usize = 4;
pub const LF_2D_LIM_UV: usize = 1;
pub const LF_NUM_BASE_LEVELS: usize = 4;
pub const NUM_BASE_LEVELS: usize = 2;
pub const TX_PAD_HOR: usize = 4;
pub const TX_PAD_HOR_LOG2: usize = 2;
pub const COEFF_BASE_RANGE: usize = 3;
pub const LF_COEFF_BASE_RANGE: usize = 3;
pub const BR_CDF_SIZE: usize = 4;
pub const MAX_BASE_BR_RANGE: usize = COEFF_BASE_RANGE + NUM_BASE_LEVELS + 1;
pub const LF_MAX_BASE_BR_RANGE: usize = LF_COEFF_BASE_RANGE + LF_NUM_BASE_LEVELS + 1;
pub const LF_BASE_SYMBOLS: usize = LF_NUM_BASE_LEVELS + 2;
pub const MAX_VAL_BR_CTX: u8 = (MAX_BASE_BR_RANGE - 1) as u8;
pub const EOB_PT_INDEX_COUNT: usize = 8;

#[rustfmt::skip]
pub const COEFF_BASE_LF_Y_ICDF: [[[u16; 6]; 33]; 2] = [
    [
        [14787,  2505,  1425,   933,   540,     0],
        [32227, 12859,  7074,  1197,    61,     0],
        [26488, 16124, 10571,  5382,  2457,     0],
        [30335, 24414, 17438, 12653,  9530,     0],
        [29709, 27239, 22079, 18098, 16108,     0],
        [30240, 25580, 22209, 20722, 17053,     0],
        [31450, 27784, 25894, 20967, 19936,     0],
        [31027, 28076, 26563, 24292, 21946,     0],
        [31949, 30700, 28828, 27151, 26604,     0],
        [10141,  2661,   814,    73,    36,     0],
        [17960,  5468,  1720,   413,   221,     0],
        [18657,  6928,  3092,  1572,   668,     0],
        [29448, 16116,  9525,  4968,  3158,     0],
        [29993, 22366, 15973, 11348,  8613,     0],
        [31242, 23285, 17472, 12536, 10035,     0],
        [31202, 28560, 25275, 22787, 21461,     0],
        [ 3717,   157,    36,    32,    28,     0],
        [ 9686,  1363,   251,    32,    28,     0],
        [16041,  4294,  1307,   361,    58,     0],
        [25083, 12079,  5215,  2254,  1141,     0],
        [29833, 24132, 19023, 15107, 12253,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
    ],
    [
        [27307, 21845, 16384, 10923,  5461,     0],
        [32161, 29734, 24879, 16991,  9102,     0],
        [32286, 23130, 21685, 21203, 19275,     0],
        [32495, 25122, 24849, 22391, 19115,     0],
        [31949, 31403, 30583, 27034, 26214,     0],
        [32342, 30640, 28512, 27236, 26385,     0],
        [31832, 30583, 29335, 28711, 27463,     0],
        [31638, 25988, 21469, 19209, 16949,     0],
        [31928, 30247, 29827, 28987, 26046,     0],
        [30008, 22884, 13624,  6322,  4007,     0],
        [31746, 27586, 22697, 18245, 12626,     0],
        [31275, 18725, 16147, 13026, 11872,     0],
        [30872, 26810, 21123, 18415, 16519,     0],
        [29991, 27769, 24437, 22216, 19439,     0],
        [30997, 27454, 23912, 20369, 17712,     0],
        [32259, 29033, 24788, 22751, 21053,     0],
        [ 7911,   679,    78,    52,    36,     0],
        [16923,  2479,   216,   108,    36,     0],
        [23002,  8481,  4048,  1799,   386,     0],
        [25141, 14124,  8051,  3249,   989,     0],
        [30317, 24511, 20899, 16212, 13245,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
        [27307, 21845, 16384, 10923,  5461,     0],
    ],
];

#[rustfmt::skip]
pub const COEFF_BASE_LF_EOB_Y_ICDF: [[[u16; 5]; 4]; 2] = [
    [
        [32692, 29107, 19009, 11259,     0],
        [  858,   185,    80,    28,     0],
        [26214, 19661, 13107,  6554,     0],
        [26214, 19661, 13107,  6554,     0],
    ],
    [
        [32470, 32172, 31874, 31576,     0],
        [ 3760,  2157,   278,    85,     0],
        [26214, 19661, 13107,  6554,     0],
        [26214, 19661, 13107,  6554,     0],
    ],
];

#[rustfmt::skip]
pub const COEFF_BASE_Y_ICDF: [[[u16; 4]; 20]; 2] = [
    [
        [ 3309,    92,    27,     0],
        [ 9101,   656,    67,     0],
        [13385,  2108,   204,     0],
        [22693,  7874,  2871,     0],
        [29400, 23370, 18876,     0],
        [ 2227,    56,    20,     0],
        [ 6388,   331,    32,     0],
        [12211,  1487,   295,     0],
        [22163,  6792,  2379,     0],
        [28976, 21634, 16861,     0],
        [ 2360,    20,    16,     0],
        [ 7038,   183,    29,     0],
        [12583,  1692,   304,     0],
        [21061,  6569,  2079,     0],
        [27733, 17838, 11539,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
    ],
    [
        [ 4385,   159,    80,     0],
        [17014,  1980,   180,     0],
        [22218,  8440,   621,     0],
        [23073,  9889,  5817,     0],
        [29341, 22486, 18641,     0],
        [ 1208,   453,   151,     0],
        [12332,  2819,   705,     0],
        [23051,  4633,  1582,     0],
        [25645,  7480,  4274,     0],
        [28727, 21875, 17306,     0],
        [ 4104,    24,    20,     0],
        [ 6060,   189,    26,     0],
        [11935,  1123,   129,     0],
        [19092,  4541,  1047,     0],
        [26976, 17212, 11128,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
        [24576, 16384,  8192,     0],
    ],
];

#[rustfmt::skip]
pub const COEFF_BASE_EOB_Y_ICDF: [[[u16; 3]; 4]; 2] = [
    [
        [21845, 10923,     0],
        [  110,    49,     0],
        [  110,    24,     0],
        [  121,    12,     0],
    ],
    [
        [21845, 10923,     0],
        [  230,    57,     0],
        [  630,   315,     0],
        [  412,   206,     0],
    ],
];

#[rustfmt::skip]
pub const COEFF_BR_LF_Y_ICDF: [[u16; 4]; 14] = [
    [24825, 18575, 11993,     0],
    [18471, 10368,  6530,     0],
    [22211, 14085, 10218,     0],
    [24479, 16700, 14314,     0],
    [27510, 22038, 19059,     0],
    [28835, 24602, 22088,     0],
    [30303, 27443, 26143,     0],
    [21903, 16338, 13077,     0],
    [18197, 10035,  6662,     0],
    [18696,  9747,  6797,     0],
    [21210, 12515,  9533,     0],
    [24165, 16568, 13302,     0],
    [26127, 19682, 16156,     0],
    [28528, 23725, 20822,     0],
];

#[rustfmt::skip]
pub const COEFF_BR_Y_ICDF: [[u16; 4]; 7] = [
    [10463,  4025,  2423,     0],
    [10105,  2820,  1448,     0],
    [12992,  4110,  2333,     0],
    [17332,  7455,  4587,     0],
    [21554, 12097,  7914,     0],
    [24220, 15786, 11002,     0],
    [27039, 20775, 15592,     0],
];

#[rustfmt::skip]
pub const COEFF_BASE_LF_UV_ICDF: [[u16; 6]; 12] = [
    [18692,  6304,  2830,  1460,   940,     0],
    [25248, 11541,  5002,  2456,  1291,     0],
    [28391, 19478, 12957,  8548,  5704,     0],
    [31086, 27629, 24167, 20795, 17722,     0],
    [17533,  4163,  1401,   617,   317,     0],
    [22512,  8182,  2993,  1303,   631,     0],
    [26850, 17139, 10451,  6166,  3667,     0],
    [30753, 27064, 22933, 19063, 15469,     0],
    [ 6348,   813,   456,   338,   242,     0],
    [16394,  3208,  1237,   745,   477,     0],
    [25571, 16814, 11782,  7834,  5031,     0],
    [27948, 23280, 21067, 18703, 16520,     0],
];

#[rustfmt::skip]
pub const COEFF_BASE_LF_EOB_UV_ICDF: [[u16; 5]; 4] = [
    [ 3818,  1325,   759,   511,     0],
    [ 2852,   849,   544,   327,     0],
    [ 3866,  1963,  1189,   952,     0],
    [26214, 19661, 13107,  6554,     0],
];

#[rustfmt::skip]
pub const COEFF_BASE_UV_ICDF: [[u16; 4]; 12] = [
    [ 5864,   666,   170,     0],
    [17019,  3870,  1158,     0],
    [23662, 11439,  5806,     0],
    [27940, 19845, 13785,     0],
    [ 4989,   362,    79,     0],
    [15354,  2691,   743,     0],
    [23540, 10472,  5001,     0],
    [28204, 20034, 13624,     0],
    [ 3530,   279,    75,     0],
    [12949,  1915,   546,     0],
    [23454, 13450,  7422,     0],
    [29708, 22503, 16680,     0],
];

#[rustfmt::skip]
pub const COEFF_BASE_EOB_UV_ICDF: [[u16; 3]; 4] = [
    [21845, 10923,     0],
    [ 1554,   331,     0],
    [  880,   321,     0],
    [ 2156,   695,     0],
];

#[rustfmt::skip]
pub const COEFF_BR_LF_UV_ICDF: [[u16; 4]; 4] = [
    [12754,  6227,  3216,     0],
    [12094,  5088,  2439,     0],
    [16540,  8475,  4454,     0],
    [23188, 16485, 11809,     0],
];

#[rustfmt::skip]
pub const COEFF_BR_UV_ICDF: [[u16; 4]; 4] = [
    [12754,  6227,  3216,     0],
    [12094,  5088,  2439,     0],
    [16540,  8475,  4454,     0],
    [23188, 16485, 11809,     0],
];

#[rustfmt::skip]
pub const EOB_GROUP_START: [u16; 12] = [0, 1, 2, 3, 5, 9, 17, 33, 65, 129, 257, 513];

#[rustfmt::skip]
pub const EOB_OFFSET_BITS: [u8; 12] = [0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

#[rustfmt::skip]
pub const DC_SIGN_UV_ICDF: [u16; 2] = [16384, 0];

#[rustfmt::skip]
pub const EOB_MULTI_1024_ICDF: [[u16; 8]; 3] = [
    [32192, 30815, 24682, 17773, 12762,  8086,  6898,     0],
    [30021, 29749, 28833, 27892, 26717, 24180, 22633,     0],
    [20895, 17389,  6933,  4850,  3675,  2604,  1986,     0],
];

#[rustfmt::skip]
pub const EOB_EXTRA_ICDF: [u16; 2] = [16377,     0];

#[rustfmt::skip]
pub const DC_SIGN_Y_ICDF: [[u16; 2]; 3] = [
    [16937,     0],
    [19136,     0],
    [13727,     0],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icdf_arrays_end_with_zero() {
        for row in COEFF_BASE_LF_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in COEFF_BASE_LF_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in COEFF_BASE_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in COEFF_BASE_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BR_LF_Y_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BR_Y_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BASE_LF_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BASE_LF_EOB_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BASE_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BASE_EOB_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BR_LF_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &COEFF_BR_UV_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        for row in &EOB_MULTI_1024_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
        assert_eq!(*DC_SIGN_UV_ICDF.last().unwrap(), 0);
        assert_eq!(*EOB_EXTRA_ICDF.last().unwrap(), 0);
        for row in &DC_SIGN_Y_ICDF {
            assert_eq!(*row.last().unwrap(), 0);
        }
    }

    #[test]
    fn test_icdf_monotonically_decreasing() {
        fn check_monotonic(arr: &[u16]) {
            for i in 0..arr.len() - 1 {
                assert!(
                    arr[i] >= arr[i + 1],
                    "ICDF not monotonically decreasing: {:?}",
                    arr
                );
            }
        }
        for row in COEFF_BASE_LF_Y_ICDF.iter().flat_map(|t| t.iter()) {
            check_monotonic(row);
        }
        for row in COEFF_BASE_LF_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            check_monotonic(row);
        }
        for row in COEFF_BASE_Y_ICDF.iter().flat_map(|t| t.iter()) {
            check_monotonic(row);
        }
        for row in COEFF_BASE_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            check_monotonic(row);
        }
        for row in &COEFF_BR_LF_Y_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BR_Y_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BASE_LF_UV_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BASE_LF_EOB_UV_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BASE_UV_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BASE_EOB_UV_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BR_LF_UV_ICDF {
            check_monotonic(row);
        }
        for row in &COEFF_BR_UV_ICDF {
            check_monotonic(row);
        }
        for row in &EOB_MULTI_1024_ICDF {
            check_monotonic(row);
        }
        for row in &DC_SIGN_Y_ICDF {
            check_monotonic(row);
        }
    }

    #[test]
    fn test_icdf_first_value_less_than_32768() {
        for row in COEFF_BASE_LF_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert!(row[0] < 32768);
        }
        for row in COEFF_BASE_LF_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert!(row[0] < 32768);
        }
        for row in COEFF_BASE_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert!(row[0] < 32768);
        }
        for row in COEFF_BASE_EOB_Y_ICDF.iter().flat_map(|t| t.iter()) {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BR_LF_Y_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BR_Y_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BASE_LF_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BASE_LF_EOB_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BASE_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BASE_EOB_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BR_LF_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &COEFF_BR_UV_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &EOB_MULTI_1024_ICDF {
            assert!(row[0] < 32768);
        }
        for row in &DC_SIGN_Y_ICDF {
            assert!(row[0] < 32768);
        }
    }
}
