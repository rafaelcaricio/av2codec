const NON_DIRECTIONAL_MODES_COUNT: usize = 5;
const TOTAL_ANGLE_DELTA_COUNT: usize = 7;
const MAX_ANGLE_DELTA: i32 = 3;
pub const LUMA_MODE_COUNT: usize = 61;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PredictionMode {
    DcPred = 0,
    VPred = 1,
    HPred = 2,
    D45Pred = 3,
    D135Pred = 4,
    D113Pred = 5,
    D157Pred = 6,
    D203Pred = 7,
    D67Pred = 8,
    SmoothPred = 9,
    SmoothVPred = 10,
    SmoothHPred = 11,
    PaethPred = 12,
}

impl PredictionMode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::DcPred,
            1 => Self::VPred,
            2 => Self::HPred,
            3 => Self::D45Pred,
            4 => Self::D135Pred,
            5 => Self::D113Pred,
            6 => Self::D157Pred,
            7 => Self::D203Pred,
            8 => Self::D67Pred,
            9 => Self::SmoothPred,
            10 => Self::SmoothVPred,
            11 => Self::SmoothHPred,
            12 => Self::PaethPred,
            _ => Self::DcPred,
        }
    }
}

const REORDERED_Y_MODE: [u8; 13] = [0, 9, 10, 11, 12, 3, 8, 1, 5, 4, 6, 2, 7];

const DEFAULT_MODE_LIST_Y: [usize; 56] = [
    17, 45, 3, 10, 24, 31, 38, 52,
    15, 19, 43, 47, 1, 5, 8, 12, 22, 26, 29, 33, 36, 40, 50, 54,
    16, 18, 44, 46, 2, 4, 9, 11, 23, 25, 30, 32, 37, 39, 51, 53,
    14, 20, 42, 48, 0, 6, 7, 13, 21, 27, 28, 34, 35, 41, 49, 55,
];

pub fn build_y_intra_mode_list(
    bottom_left_joint: Option<u8>,
    above_right_joint: Option<u8>,
    is_small_block: bool,
    is_large_block: bool,
) -> [u8; LUMA_MODE_COUNT] {
    let mut list = [0u8; LUMA_MODE_COUNT];
    let mut selected = [false; LUMA_MODE_COUNT];
    let mut idx = 0;

    for i in 0..NON_DIRECTIONAL_MODES_COUNT {
        list[idx] = i as u8;
        selected[i] = true;
        idx += 1;
    }

    if !is_small_block {
        let left_joint = bottom_left_joint.unwrap_or(0) as usize;
        let above_joint = above_right_joint.unwrap_or(0) as usize;
        let is_left_dir = left_joint >= NON_DIRECTIONAL_MODES_COUNT;
        let is_above_dir = above_joint >= NON_DIRECTIONAL_MODES_COUNT;

        let mut neighbor_joints = [left_joint, above_joint];
        let mut dir_cnt = (is_left_dir as usize) + (is_above_dir as usize);

        if dir_cnt == 2 && neighbor_joints[0] == neighbor_joints[1] {
            dir_cnt = 1;
        }
        if dir_cnt == 1 && !is_left_dir {
            neighbor_joints[0] = neighbor_joints[1];
        }

        for i in 0..dir_cnt {
            let j = neighbor_joints[i];
            if !selected[j] {
                list[idx] = j as u8;
                selected[j] = true;
                idx += 1;
            }
        }

        if is_large_block {
            for offset in 0..4usize {
                for j_idx in 0..dir_cnt {
                    let nj = neighbor_joints[j_idx] as i64;
                    let left_derived =
                        ((nj - offset as i64 + 50).rem_euclid(56)) as usize
                            + NON_DIRECTIONAL_MODES_COUNT;
                    let right_derived =
                        ((nj + offset as i64 - 4).rem_euclid(56)) as usize
                            + NON_DIRECTIONAL_MODES_COUNT;

                    if !selected[left_derived] {
                        list[idx] = left_derived as u8;
                        selected[left_derived] = true;
                        idx += 1;
                    }
                    if !selected[right_derived] {
                        list[idx] = right_derived as u8;
                        selected[right_derived] = true;
                        idx += 1;
                    }
                }
            }
        }
    }

    for &d in &DEFAULT_MODE_LIST_Y {
        if idx >= LUMA_MODE_COUNT {
            break;
        }
        let mode = d + NON_DIRECTIONAL_MODES_COUNT;
        if !selected[mode] {
            list[idx] = mode as u8;
            selected[mode] = true;
            idx += 1;
        }
    }

    list
}

pub fn decode_prediction_mode(joint: u8) -> (PredictionMode, i32) {
    let j = joint as usize;
    if j < NON_DIRECTIONAL_MODES_COUNT {
        (PredictionMode::from_u8(REORDERED_Y_MODE[j]), 0)
    } else {
        let dir_group =
            (j - NON_DIRECTIONAL_MODES_COUNT) / TOTAL_ANGLE_DELTA_COUNT;
        let angle_delta = ((j - NON_DIRECTIONAL_MODES_COUNT)
            % TOTAL_ANGLE_DELTA_COUNT) as i32
            - MAX_ANGLE_DELTA;
        let canonical_idx = dir_group + NON_DIRECTIONAL_MODES_COUNT;
        (
            PredictionMode::from_u8(REORDERED_Y_MODE[canonical_idx]),
            angle_delta,
        )
    }
}

const BASE_DC: u8 = 128;
const DEFAULT_ABOVE: u8 = 127;
const DEFAULT_LEFT: u8 = 129;
const DEFAULT_TOP_LEFT: u8 = 129;

pub fn predict_intra(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    mode: PredictionMode,
    _angle_delta: i32,
) -> Vec<u8> {
    let has_above = y > 0;
    let has_left = x > 0;

    match mode {
        PredictionMode::DcPred => predict_dc(recon, stride, x, y, w, h, has_above, has_left),
        PredictionMode::VPred => predict_v(recon, stride, x, y, w, h, has_above, has_left),
        PredictionMode::HPred => predict_h(recon, stride, x, y, w, h, has_above, has_left),
        PredictionMode::D45Pred | PredictionMode::D67Pred => {
            predict_zone1(recon, stride, x, y, w, h, has_above, has_left)
        }
        PredictionMode::D203Pred => {
            predict_zone3(recon, stride, x, y, w, h, has_above, has_left)
        }
        PredictionMode::D113Pred | PredictionMode::D135Pred | PredictionMode::D157Pred => {
            predict_zone2(recon, stride, x, y, w, h, has_above, has_left)
        }
        PredictionMode::PaethPred => {
            predict_paeth(recon, stride, x, y, w, h, has_above, has_left)
        }
        PredictionMode::SmoothPred | PredictionMode::SmoothVPred | PredictionMode::SmoothHPred => {
            predict_smooth(recon, stride, x, y, w, h, has_above, has_left)
        }
    }
}

fn predict_dc(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    let val = match (has_above, has_left) {
        (false, false) => BASE_DC,
        (true, false) => {
            let sum: u32 = (0..w).map(|i| recon[(y - 1) * stride + x + i] as u32).sum();
            ((sum + w as u32 / 2) / w as u32) as u8
        }
        (false, true) => {
            let sum: u32 = (0..h).map(|j| recon[(y + j) * stride + x - 1] as u32).sum();
            ((sum + h as u32 / 2) / h as u32) as u8
        }
        (true, true) => {
            let sum_above: u32 =
                (0..w).map(|i| recon[(y - 1) * stride + x + i] as u32).sum();
            let sum_left: u32 =
                (0..h).map(|j| recon[(y + j) * stride + x - 1] as u32).sum();
            ((sum_above + sum_left + (w + h) as u32 / 2) / (w + h) as u32) as u8
        }
    };
    vec![val; w * h]
}

fn predict_v(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    if !has_above {
        return if has_left {
            vec![recon[y * stride + x - 1]; w * h]
        } else {
            vec![DEFAULT_ABOVE; w * h]
        };
    }
    let mut pred = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            pred[row * w + col] = recon[(y - 1) * stride + x + col];
        }
    }
    pred
}

fn predict_h(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    if !has_left {
        return if has_above {
            vec![recon[(y - 1) * stride + x]; w * h]
        } else {
            vec![DEFAULT_LEFT; w * h]
        };
    }
    let mut pred = vec![0u8; w * h];
    for row in 0..h {
        let left_val = recon[(y + row) * stride + x - 1];
        for col in 0..w {
            pred[row * w + col] = left_val;
        }
    }
    pred
}

fn predict_zone1(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    if !has_above {
        return if has_left {
            vec![recon[y * stride + x - 1]; w * h]
        } else {
            vec![DEFAULT_ABOVE; w * h]
        };
    }
    let total = w + h;
    let mut above = Vec::with_capacity(total);
    for i in 0..total {
        if x + i < stride {
            above.push(recon[(y - 1) * stride + x + i]);
        } else if let Some(&last) = above.last() {
            above.push(last);
        } else {
            above.push(DEFAULT_ABOVE);
        }
    }
    let mut pred = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let idx = (row + col + 1).min(total - 1);
            pred[row * w + col] = above[idx];
        }
    }
    pred
}

fn predict_zone3(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    if !has_left {
        return if has_above {
            vec![recon[(y - 1) * stride + x]; w * h]
        } else {
            vec![DEFAULT_LEFT; w * h]
        };
    }
    let total = h + w;
    let frame_h = recon.len() / stride;
    let mut left = Vec::with_capacity(total);
    for j in 0..total {
        if y + j < frame_h {
            left.push(recon[(y + j) * stride + x - 1]);
        } else if let Some(&last) = left.last() {
            left.push(last);
        } else {
            left.push(DEFAULT_LEFT);
        }
    }
    let mut pred = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let idx = (row + col + 1).min(total - 1);
            pred[row * w + col] = left[idx];
        }
    }
    pred
}

fn predict_zone2(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    if !has_above && !has_left {
        let (above, left, tl) = default_ref_samples(w, h);
        return predict_paeth_from_refs(&above, &left, tl, w, h);
    }
    predict_dc(recon, stride, x, y, w, h, has_above, has_left)
}

fn predict_paeth(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    let (above, left, tl) = if !has_above && !has_left {
        default_ref_samples(w, h)
    } else {
        ref_samples(recon, stride, x, y, w, h, has_above, has_left)
    };
    predict_paeth_from_refs(&above, &left, tl, w, h)
}

fn predict_smooth(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> Vec<u8> {
    let (above, left, _tl) = if !has_above && !has_left {
        default_ref_samples(w, h)
    } else {
        ref_samples(recon, stride, x, y, w, h, has_above, has_left)
    };
    let below_pred = left[h - 1] as u32;
    let right_pred = above[w - 1] as u32;
    let mut pred = vec![0u8; w * h];
    for row in 0..h {
        for col in 0..w {
            let top = above[col] as u32;
            let lft = left[row] as u32;
            let w_top = ((h - 1 - row) * 256 / h.max(1)) as u32;
            let w_lft = ((w - 1 - col) * 256 / w.max(1)) as u32;
            let vert = w_top * top + (256 - w_top) * below_pred;
            let horz = w_lft * lft + (256 - w_lft) * right_pred;
            pred[row * w + col] = ((vert + horz + 256) >> 9).min(255) as u8;
        }
    }
    pred
}

fn default_ref_samples(w: usize, h: usize) -> (Vec<u8>, Vec<u8>, u8) {
    (vec![DEFAULT_ABOVE; w], vec![DEFAULT_LEFT; h], DEFAULT_TOP_LEFT)
}

fn ref_samples(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> (Vec<u8>, Vec<u8>, u8) {
    let mut above = vec![DEFAULT_ABOVE; w];
    let mut left = vec![DEFAULT_LEFT; h];

    if has_above {
        for i in 0..w {
            above[i] = recon[(y - 1) * stride + x + i];
        }
    }
    if has_left {
        for j in 0..h {
            left[j] = recon[(y + j) * stride + x - 1];
        }
    }

    let tl = if has_above && has_left {
        recon[(y - 1) * stride + x - 1]
    } else if has_left {
        left[0]
    } else {
        DEFAULT_TOP_LEFT
    };

    (above, left, tl)
}

fn predict_paeth_from_refs(above: &[u8], left: &[u8], tl: u8, w: usize, h: usize) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    let top_left = tl as i32;
    for row in 0..h {
        for col in 0..w {
            let top = above[col] as i32;
            let lft = left[row] as i32;
            let p = top + lft - top_left;
            let p_top = (p - top).abs();
            let p_left = (p - lft).abs();
            let p_tl = (p - top_left).abs();
            pred[row * w + col] = if p_top <= p_left && p_top <= p_tl {
                top as u8
            } else if p_left <= p_tl {
                lft as u8
            } else {
                tl
            };
        }
    }
    pred
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_list_block0() {
        let list = build_y_intra_mode_list(None, None, false, true);
        assert_eq!(list[0], 0);
        assert_eq!(list[1], 1);
        assert_eq!(list[2], 2);
        assert_eq!(list[3], 3);
        assert_eq!(list[4], 4);
        assert_eq!(list[7], 8);
    }

    #[test]
    fn mode_idx7_maps_to_d45() {
        let list = build_y_intra_mode_list(None, None, false, true);
        let joint = list[7];
        assert_eq!(joint, 8);
        let (mode, delta) = decode_prediction_mode(joint);
        assert_eq!(mode, PredictionMode::D45Pred);
        assert_eq!(delta, 0);
    }

    #[test]
    fn mode_idx0_maps_to_dc() {
        let list = build_y_intra_mode_list(None, None, false, true);
        let joint = list[0];
        assert_eq!(joint, 0);
        let (mode, delta) = decode_prediction_mode(joint);
        assert_eq!(mode, PredictionMode::DcPred);
        assert_eq!(delta, 0);
    }

    #[test]
    fn dc_no_neighbors_gives_128() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::DcPred, 0);
        assert!(pred.iter().all(|&v| v == 128));
    }

    #[test]
    fn d45_no_neighbors_gives_127() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::D45Pred, 0);
        assert!(pred.iter().all(|&v| v == 127));
    }

    #[test]
    fn v_pred_no_neighbors_gives_127() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::VPred, 0);
        assert!(pred.iter().all(|&v| v == 127));
    }

    #[test]
    fn h_pred_no_neighbors_gives_129() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::HPred, 0);
        assert!(pred.iter().all(|&v| v == 129));
    }

    #[test]
    fn paeth_no_neighbors_gives_127() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::PaethPred, 0);
        assert!(pred.iter().all(|&v| v == 127));
    }

    #[test]
    #[test]
    fn dc_with_above() {
        let mut recon = vec![0u8; 64 * 64];
        recon[..64].fill(200);
        let pred = predict_intra(&recon, 64, 0, 1, 64, 63, PredictionMode::DcPred, 0);
        assert_eq!(pred[0], 200);
    }

    #[test]
    fn v_pred_with_above() {
        let mut recon = vec![0u8; 128 * 128];
        for (i, pixel) in recon[..64].iter_mut().enumerate() {
            *pixel = 100 + i as u8;
        }
        let pred = predict_intra(&recon, 128, 0, 1, 64, 64, PredictionMode::VPred, 0);
        for row in 0..64 {
            for col in 0..64 {
                assert_eq!(pred[row * 64 + col], 100 + col as u8);
            }
        }
    }

    #[test]
    fn decode_all_non_directional_modes() {
        for joint in 0..5u8 {
            let (mode, delta) = decode_prediction_mode(joint);
            assert_eq!(delta, 0);
            match joint {
                0 => assert_eq!(mode, PredictionMode::DcPred),
                1 => assert_eq!(mode, PredictionMode::SmoothPred),
                2 => assert_eq!(mode, PredictionMode::SmoothVPred),
                3 => assert_eq!(mode, PredictionMode::SmoothHPred),
                4 => assert_eq!(mode, PredictionMode::PaethPred),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn decode_directional_d45_with_deltas() {
        for delta_idx in 0..7usize {
            let joint = (5 + delta_idx) as u8;
            let (mode, delta) = decode_prediction_mode(joint);
            assert_eq!(mode, PredictionMode::D45Pred);
            assert_eq!(delta, delta_idx as i32 - 3);
        }
    }

    #[test]
    fn mode_list_completeness() {
        let list = build_y_intra_mode_list(None, None, false, true);
        let mut seen = [false; LUMA_MODE_COUNT];
        for &v in &list {
            let v = v as usize;
            assert!(v < LUMA_MODE_COUNT);
            assert!(!seen[v], "duplicate entry {}", v);
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s), "not all modes covered");
    }
}
