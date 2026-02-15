use crate::entropy::DaalaCoder;

// Default ICDF tables for the simplest intra-only KEY_FRAME
// Config: enable_ext_partitions=0, segmentation=0, intrabc=0, enable_gdf=0,
// enable_cdef=0, enable_ccso=0, delta_q_present=0, enable_cfl_intra=0
// base_qindex <= 90 (coef CDF category 0)

// do_split CDF: PARTITION_STRUCTURE_0, context 12 (BLOCK_64X64, no neighbors)
// AVM_CDF2(20492)
const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];

// Y mode set CDF (no context dimension)
// AVM_CDF4(28863, 31022, 31724)
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];

// Y mode index CDF, context 0 (no angular neighbors)
// AVM_CDF8(15175, 20075, 21728, 24098, 26405, 27655, 28860)
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];

// UV mode CDF, context 0 (Y mode is non-directional)
// AVM_CDF8(9363, 20957, 22865, 24753, 26411, 27983, 30428)
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];

// Coefficient txb_skip CDFs (coef category 0, pred_mode_ctx=0 intra)
// Y: TX_64X64 (txs_ctx=4, txb_skip_ctx=0) — AVM_CDF2(2800)
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];

// U: TX_32X32 (txs_ctx=3, txb_skip_ctx=0) — AVM_CDF2(7809)
const TXB_SKIP_U_ICDF: [u16; 2] = [24959, 0];

// V: v_txb_skip (ctx=0, U had eob=0) — AVM_CDF2(1439)
const TXB_SKIP_V_ICDF: [u16; 2] = [31329, 0];

pub fn encode_tile_data() -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    // Partition: do_split = 0 (PARTITION_NONE for 64x64 SB)
    ec.encode_symbol(0, &DO_SPLIT_ICDF);

    // Intra luma mode: set 0 (DC_PRED is in first mode set)
    ec.encode_symbol(0, &Y_MODE_SET_ICDF);

    // Intra luma mode: index 0 within set (DC_PRED)
    ec.encode_symbol(0, &Y_MODE_IDX_ICDF);

    // Intra chroma mode: index 0 (DC_PRED, no CFL since enable_cfl_intra=0)
    ec.encode_symbol(0, &UV_MODE_ICDF);

    // Coefficients: all zero (eob==0 encoded as symbol 1)
    // Y plane (1 TU of TX_64X64)
    ec.encode_symbol(1, &TXB_SKIP_Y_ICDF);

    // U plane (1 TU of TX_32X32)
    ec.encode_symbol(1, &TXB_SKIP_U_ICDF);

    // V plane (1 TU of TX_32X32)
    ec.encode_symbol(1, &TXB_SKIP_V_ICDF);

    ec.done()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_data_is_nonempty() {
        let data = encode_tile_data();
        assert!(!data.is_empty());
    }

    #[test]
    fn tile_data_reasonable_size() {
        let data = encode_tile_data();
        assert!(
            data.len() <= 20,
            "tile data too large: {} bytes",
            data.len()
        );
    }
}
