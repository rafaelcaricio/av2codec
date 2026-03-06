const NON_DIRECTIONAL_MODES_COUNT: usize = 5;
const TOTAL_ANGLE_DELTA_COUNT: usize = 7;
const MAX_ANGLE_DELTA: i32 = 3;
pub const LUMA_MODE_COUNT: usize = 61;
const ANGLE_STEP: i32 = 3;
const IBP_WEIGHT_SHIFT: u32 = 7;
const IBP_WEIGHT_MAX: u32 = 1 << IBP_WEIGHT_SHIFT;
const IBP_WEIGHT_REF: u32 = IBP_WEIGHT_MAX;
const IBP_WEIGHT_SIZE_LOG2: usize = 4;
const IBP_WEIGHT_SIZE: usize = 1 << IBP_WEIGHT_SIZE_LOG2;

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
    pub fn from_u8(v: u8) -> Self {
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

    fn base_angle(self) -> i32 {
        MODE_TO_ANGLE_MAP[self as usize] as i32
    }

    fn is_directional(self) -> bool {
        (self as u8) >= 1 && (self as u8) <= 8
    }
}

const MODE_TO_ANGLE_MAP: [u16; 13] = [0, 90, 180, 45, 135, 113, 157, 203, 67, 0, 0, 0, 0];

#[rustfmt::skip]
const DR_INTRA_DERIVATIVE: [i32; 90] = [
    0, 4096, 2048,
    1365, 1024, 819,
    682, 585, 512,
    455, 409, 409, 409, 372,
    341, 292, 273,
    256, 227, 215,
    204, 186, 178,
    170, 157, 151,
    146, 136, 132,
    128, 117, 110,
    107, 99, 97, 97,
    93, 87, 83,
    81, 77, 74,
    73, 69, 66,
    64, 62, 59,
    56, 55, 53,
    50, 49, 47,
    44, 42, 42, 41,
    38, 37, 35,
    32, 31, 30,
    28, 27, 26,
    24, 23, 22,
    20, 19, 18,
    16, 15, 14,
    12, 11, 10, 10, 10,
    9, 8, 7,
    6, 5, 4,
    3, 2, 1,
];

fn get_dx(angle: i32) -> i32 {
    if angle > 0 && angle < 90 {
        DR_INTRA_DERIVATIVE[angle as usize]
    } else if angle > 90 && angle < 180 {
        DR_INTRA_DERIVATIVE[(180 - angle) as usize]
    } else {
        1
    }
}

fn get_dy(angle: i32) -> i32 {
    if angle > 90 && angle < 180 {
        DR_INTRA_DERIVATIVE[(angle - 90) as usize]
    } else if angle > 180 && angle < 270 {
        DR_INTRA_DERIVATIVE[(270 - angle) as usize]
    } else {
        1
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
const DEFAULT_TOP_LEFT: u8 = 128;

const MRL_INDEX_TO_DELTA: [i32; 4] = [0, 1, -1, 0];

const IBP_WEIGHTS: [[u8; 16]; 5] = [
    [96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [86, 107, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [77, 90, 102, 115, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [71, 78, 86, 92, 100, 107, 114, 121, 0, 0, 0, 0, 0, 0, 0, 0],
    [68, 72, 76, 79, 83, 87, 90, 94, 98, 102, 106, 109, 113, 117, 121, 124],
];

const IBP_SIZE_TO_WEIGHTS_INDEX: [usize; 9] = [0, 1, 2, 0, 3, 0, 0, 0, 4];

const ANGLE_TO_MODE_INDEX: [u8; 90] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 0, 0, 14, 0, 0, 13,
    0, 0, 12, 0, 0, 11, 0, 0, 10, 0, 0, 0, 9, 0, 0, 8, 0, 0, 7, 0, 0, 6, 0, 0, 5, 0, 0, 4, 0,
    0, 3, 0, 0, 0, 0, 2, 0, 0, 1, 0, 0, 0, 0, 0,
];

const IS_IBP_ENABLED: [bool; 16] = [
    false, true, false, false, true, false, true, false, true, false, false, true, false, true,
    false, true,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntraPredictControl {
    pub plane: usize,
    pub enable_ibp: bool,
    pub is_cfl: bool,
}

pub fn predict_intra(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    mode: PredictionMode,
    angle_delta: i32,
    mrl_index: u8,
    sb_y: usize,
) -> Vec<u8> {
    predict_intra_with_control(
        recon,
        stride,
        x,
        y,
        w,
        h,
        mode,
        angle_delta,
        mrl_index,
        sb_y,
        IntraPredictControl::default(),
    )
}

pub fn predict_intra_with_control(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    mode: PredictionMode,
    angle_delta: i32,
    mrl_index: u8,
    sb_y: usize,
    control: IntraPredictControl,
) -> Vec<u8> {
    let has_above = y > 0;
    let has_left = x > 0;
    let apply_ibp = control.enable_ibp && !(w == 4 && h == 4);

    if mode.is_directional() {
        let mut p_angle = mode.base_angle() + angle_delta * ANGLE_STEP;
        p_angle += MRL_INDEX_TO_DELTA[mrl_index as usize];
        return predict_directional(
            recon,
            stride,
            x,
            y,
            w,
            h,
            has_above,
            has_left,
            p_angle,
            mrl_index,
            sb_y,
            apply_ibp && control.plane == 0 && mrl_index == 0 && angle_delta % 2 == 0,
        );
    }

    match mode {
        PredictionMode::DcPred => {
            let mut pred = predict_dc(recon, stride, x, y, w, h, has_above, has_left);
            if apply_ibp && (control.plane == 0 || !control.is_cfl) && (has_above || has_left) {
                let (above, left, _) = ref_samples(recon, stride, x, y, w, h, has_above, has_left);
                apply_ibp_dc(&mut pred, w, h, &above, &left, has_above, has_left);
            }
            pred
        }
        PredictionMode::PaethPred => {
            predict_paeth(recon, stride, x, y, w, h, has_above, has_left)
        }
        PredictionMode::SmoothPred | PredictionMode::SmoothVPred | PredictionMode::SmoothHPred => {
            predict_smooth(recon, stride, x, y, w, h, has_above, has_left, mode)
        }
        _ => vec![BASE_DC; w * h],
    }
}

fn predict_directional(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
    p_angle: i32,
    mrl_index: u8,
    sb_y: usize,
    apply_ibp: bool,
) -> Vec<u8> {
    if p_angle == 90 && mrl_index == 0 {
        return predict_v(recon, stride, x, y, w, h, has_above, has_left);
    }
    if p_angle == 180 && mrl_index == 0 {
        return predict_h(recon, stride, x, y, w, h, has_above, has_left);
    }

    let above_mrl = if y == sb_y { 0 } else { mrl_index };
    let (above_ext, left_ext, tl) =
        extended_ref_samples_mrl(recon, stride, x, y, w, h, has_above, has_left, above_mrl, mrl_index);

    let mrl = mrl_index as usize;
    let mut pred = if p_angle > 0 && p_angle < 90 {
        predict_zone1_angle(&above_ext, w, h, get_dx(p_angle), mrl)
    } else if p_angle == 90 {
        predict_v_mrl(&above_ext, w, h)
    } else if p_angle > 90 && p_angle < 180 {
        predict_zone2_angle(&above_ext, &left_ext, tl, w, h, get_dx(p_angle), get_dy(p_angle), mrl)
    } else if p_angle == 180 {
        predict_h_mrl(&left_ext, w, h)
    } else if p_angle > 180 && p_angle < 270 {
        predict_zone3_angle(&left_ext, w, h, get_dy(p_angle), mrl)
    } else {
        vec![BASE_DC; w * h]
    };

    if apply_ibp {
        if let Some(mode_index) = ibp_mode_index(p_angle) {
            if p_angle < 90 {
                let second = predict_zone3_angle(&left_ext, w, h, DR_INTRA_DERIVATIVE[(90 - p_angle) as usize], 0);
                apply_ibp_directional_zone1(&mut pred, &second, w, h, mode_index, p_angle);
            } else if p_angle > 180 {
                let second =
                    predict_zone1_angle(&above_ext, w, h, DR_INTRA_DERIVATIVE[(p_angle - 180) as usize], 0);
                apply_ibp_directional_zone3(&mut pred, &second, w, h, mode_index, 270 - p_angle);
            }
        }
    }

    pred
}

fn ibp_mode_index(angle: i32) -> Option<usize> {
    let lookup = if angle > 0 && angle < 90 {
        angle as usize
    } else if angle > 180 && angle < 270 {
        (270 - angle) as usize
    } else {
        return None;
    };
    let mode_index = ANGLE_TO_MODE_INDEX[lookup] as usize;
    IS_IBP_ENABLED[mode_index].then_some(mode_index)
}

fn ibp_directional_weight(angle: i32, row_idx: usize, col_idx: usize) -> u32 {
    let y = DR_INTRA_DERIVATIVE[(90 - angle) as usize] * (col_idx as i32 + 1);
    let dist = ((row_idx as i32 + 1) << 6) + y;
    ((y * IBP_WEIGHT_MAX as i32 + (dist >> 1)) / dist) as u32
}

fn apply_ibp_directional_zone1(
    pred: &mut [u8],
    second: &[u8],
    w: usize,
    h: usize,
    _mode_index: usize,
    angle: i32,
) {
    let col_shift = w >> (IBP_WEIGHT_SIZE_LOG2 + 1);
    let row_shift = h >> (IBP_WEIGHT_SIZE_LOG2 + 1);

    for r in 0..h {
        let row_idx = (r >> row_shift).min(IBP_WEIGHT_SIZE - 1);
        for c in 0..w {
            let col_idx = (c >> col_shift).min(IBP_WEIGHT_SIZE - 1);
            let weight = ibp_directional_weight(angle, row_idx, col_idx);
            let idx = r * w + c;
            pred[idx] = ((pred[idx] as u32 * weight
                + second[idx] as u32 * (IBP_WEIGHT_MAX - weight)
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }
}

fn apply_ibp_directional_zone3(
    pred: &mut [u8],
    second: &[u8],
    w: usize,
    h: usize,
    _mode_index: usize,
    angle: i32,
) {
    let col_shift = w >> (IBP_WEIGHT_SIZE_LOG2 + 1);
    let row_shift = h >> (IBP_WEIGHT_SIZE_LOG2 + 1);

    for c in 0..w {
        let col_idx = (c >> col_shift).min(IBP_WEIGHT_SIZE - 1);
        for r in 0..h {
            let row_idx = (r >> row_shift).min(IBP_WEIGHT_SIZE - 1);
            let weight = ibp_directional_weight(angle, col_idx, row_idx);
            let idx = r * w + c;
            pred[idx] = ((pred[idx] as u32 * weight
                + second[idx] as u32 * (IBP_WEIGHT_MAX - weight)
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }
}

fn apply_ibp_dc(pred: &mut [u8], w: usize, h: usize, above: &[u8], left: &[u8], has_above: bool, has_left: bool) {
    match (has_above, has_left) {
        (true, true) => apply_ibp_dc_both(pred, w, h, above, left),
        (true, false) => apply_ibp_dc_top(pred, w, h, above),
        (false, true) => apply_ibp_dc_left(pred, w, h, left),
        (false, false) => {}
    }
}

fn apply_ibp_dc_left(pred: &mut [u8], w: usize, h: usize, left: &[u8]) {
    let len = w >> 2;
    let weights = &IBP_WEIGHTS[IBP_SIZE_TO_WEIGHTS_INDEX[w >> 3]];
    for r in 0..h {
        let row_base = r * w;
        for c in 0..len {
            pred[row_base + c] = ((left[r] as u32 * (IBP_WEIGHT_REF - weights[c] as u32)
                + pred[row_base + c] as u32 * weights[c] as u32
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }
}

fn apply_ibp_dc_top(pred: &mut [u8], w: usize, h: usize, above: &[u8]) {
    let len = h >> 2;
    let weights = &IBP_WEIGHTS[IBP_SIZE_TO_WEIGHTS_INDEX[h >> 3]];
    for r in 0..len {
        let row_base = r * w;
        for c in 0..w {
            pred[row_base + c] = ((above[c] as u32 * (IBP_WEIGHT_REF - weights[r] as u32)
                + pred[row_base + c] as u32 * weights[r] as u32
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }
}

fn apply_ibp_dc_both(pred: &mut [u8], w: usize, h: usize, above: &[u8], left: &[u8]) {
    let len_h = h >> 2;
    let len_w = w >> 2;
    let row_start = if w >= h { len_h } else { 0 };
    let col_start = if w >= h { 0 } else { len_w };

    let top_weights = &IBP_WEIGHTS[IBP_SIZE_TO_WEIGHTS_INDEX[h >> 3]];
    for r in 0..len_h {
        let row_base = r * w;
        for c in col_start..w {
            pred[row_base + c] = ((above[c] as u32 * (IBP_WEIGHT_REF - top_weights[r] as u32)
                + pred[row_base + c] as u32 * top_weights[r] as u32
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }

    let left_weights = &IBP_WEIGHTS[IBP_SIZE_TO_WEIGHTS_INDEX[w >> 3]];
    for r in row_start..h {
        let row_base = r * w;
        for c in 0..len_w {
            pred[row_base + c] = ((left[r] as u32 * (IBP_WEIGHT_REF - left_weights[c] as u32)
                + pred[row_base + c] as u32 * left_weights[c] as u32
                + (1 << (IBP_WEIGHT_SHIFT - 1)))
                >> IBP_WEIGHT_SHIFT) as u8;
        }
    }
}

fn extended_ref_samples(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
) -> (Vec<u8>, Vec<u8>, u8) {
    let above_need = w + h;
    let left_need = h + w;
    let frame_h = recon.len() / stride;

    match (has_above, has_left) {
        (true, true) => {
            let tl = recon[(y - 1) * stride + x - 1];
            let mut above = Vec::with_capacity(above_need);
            for i in 0..above_need {
                if x + i < stride && (y > 0) {
                    above.push(recon[(y - 1) * stride + x + i]);
                } else if !above.is_empty() {
                    above.push(*above.last().unwrap());
                } else {
                    above.push(DEFAULT_ABOVE);
                }
            }
            let mut left = Vec::with_capacity(left_need);
            for j in 0..left_need {
                if y + j < frame_h && x > 0 {
                    left.push(recon[(y + j) * stride + x - 1]);
                } else if !left.is_empty() {
                    left.push(*left.last().unwrap());
                } else {
                    left.push(DEFAULT_LEFT);
                }
            }
            (above, left, tl)
        }
        (true, false) => {
            let mut above = Vec::with_capacity(above_need);
            for i in 0..above_need {
                if x + i < stride {
                    above.push(recon[(y - 1) * stride + x + i]);
                } else if !above.is_empty() {
                    above.push(*above.last().unwrap());
                } else {
                    above.push(DEFAULT_ABOVE);
                }
            }
            let fill = above[0];
            let left = vec![fill; left_need];
            (above, left, fill)
        }
        (false, true) => {
            let mut left = Vec::with_capacity(left_need);
            for j in 0..left_need {
                if y + j < frame_h {
                    left.push(recon[(y + j) * stride + x - 1]);
                } else if !left.is_empty() {
                    left.push(*left.last().unwrap());
                } else {
                    left.push(DEFAULT_LEFT);
                }
            }
            let fill = left[0];
            let above = vec![fill; above_need];
            (above, left, fill)
        }
        (false, false) => {
            (vec![DEFAULT_ABOVE; above_need], vec![DEFAULT_LEFT; left_need], DEFAULT_TOP_LEFT)
        }
    }
}

fn extended_ref_samples_mrl(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    has_above: bool,
    has_left: bool,
    above_mrl: u8,
    left_mrl: u8,
) -> (Vec<u8>, Vec<u8>, u8) {
    if above_mrl == 0 && left_mrl == 0 {
        return extended_ref_samples(recon, stride, x, y, w, h, has_above, has_left);
    }

    let mrl_extra = (above_mrl.max(left_mrl) as usize) << 1;
    let above_need = w + h + mrl_extra;
    let left_need = h + w + mrl_extra;
    let frame_h = recon.len() / stride;
    let above_off = above_mrl as usize + 1;
    let left_off = left_mrl as usize + 1;

    let has_above_mrl = has_above && y >= above_off;
    let has_left_mrl = has_left && x >= left_off;

    match (has_above_mrl, has_left_mrl) {
        (true, true) => {
            let tl = recon[(y - above_off) * stride + x - left_off];
            let mut above = Vec::with_capacity(above_need);
            for i in 0..above_need {
                let col = x + i;
                if col < stride {
                    above.push(recon[(y - above_off) * stride + col]);
                } else if !above.is_empty() {
                    above.push(*above.last().unwrap());
                } else {
                    above.push(DEFAULT_ABOVE);
                }
            }
            let mut left = Vec::with_capacity(left_need);
            for j in 0..left_need {
                let row = y + j;
                if row < frame_h {
                    left.push(recon[row * stride + x - left_off]);
                } else if !left.is_empty() {
                    left.push(*left.last().unwrap());
                } else {
                    left.push(DEFAULT_LEFT);
                }
            }
            (above, left, tl)
        }
        (true, false) => {
            let mut above = Vec::with_capacity(above_need);
            for i in 0..above_need {
                let col = x + i;
                if col < stride {
                    above.push(recon[(y - above_off) * stride + col]);
                } else if !above.is_empty() {
                    above.push(*above.last().unwrap());
                } else {
                    above.push(DEFAULT_ABOVE);
                }
            }
            let fill = above[0];
            let left = vec![fill; left_need];
            (above, left, fill)
        }
        (false, true) => {
            let mut left = Vec::with_capacity(left_need);
            for j in 0..left_need {
                let row = y + j;
                if row < frame_h {
                    left.push(recon[row * stride + x - left_off]);
                } else if !left.is_empty() {
                    left.push(*left.last().unwrap());
                } else {
                    left.push(DEFAULT_LEFT);
                }
            }
            let fill = left[0];
            let above = vec![fill; above_need];
            (above, left, fill)
        }
        (false, false) => {
            (vec![DEFAULT_ABOVE; above_need], vec![DEFAULT_LEFT; left_need], DEFAULT_TOP_LEFT)
        }
    }
}

fn predict_v_mrl(above: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    for r in 0..h {
        for c in 0..w {
            pred[r * w + c] = above[c];
        }
    }
    pred
}

fn predict_h_mrl(left: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    for r in 0..h {
        for c in 0..w {
            pred[r * w + c] = left[r];
        }
    }
    pred
}

fn predict_zone1_angle(above: &[u8], w: usize, h: usize, dx: i32, mrl: usize) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    let max_base_x = (w + h) as i32 - 1 + (mrl as i32) * 2;
    let mut pos_x = dx * (1 + mrl as i32);

    for r in 0..h {
        let base = pos_x >> 6;
        let shift = ((pos_x & 0x3F) >> 1) as u32;

        if base >= max_base_x {
            let fill = above[max_base_x as usize];
            for i in r..h {
                for c in 0..w {
                    pred[i * w + c] = fill;
                }
            }
            return pred;
        }

        for c in 0..w {
            let b = base + c as i32;
            if b < max_base_x {
                let val = above[b as usize] as u32 * (32 - shift)
                    + above[(b + 1) as usize] as u32 * shift;
                pred[r * w + c] = ((val + 16) >> 5) as u8;
            } else {
                pred[r * w + c] = above[max_base_x as usize];
            }
        }
        pos_x += dx;
    }
    pred
}

fn predict_zone2_angle(
    above: &[u8],
    left: &[u8],
    tl: u8,
    w: usize,
    h: usize,
    dx: i32,
    dy: i32,
    mrl: usize,
) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    let min_base_x = -(1 + mrl as i32);
    let min_base_y = -(1 + mrl as i32);

    for r in 0..h {
        for c in 0..w {
            let x = ((c as i32) << 6) - ((r as i32) + 1 + mrl as i32) * dx;
            let base_x = x >> 6;

            if base_x >= min_base_x {
                let shift = ((x & 0x3F) >> 1) as u32;
                let (s0, s1) = if base_x < 0 {
                    (tl as u32, above[0] as u32)
                } else {
                    (above[base_x as usize] as u32, above[(base_x + 1) as usize] as u32)
                };
                let val = s0 * (32 - shift) + s1 * shift;
                pred[r * w + c] = ((val + 16) >> 5) as u8;
            } else {
                let y = ((r as i32) << 6) - ((c as i32) + 1 + mrl as i32) * dy;
                let base_y = y >> 6;
                let shift = ((y & 0x3F) >> 1) as u32;
                let (s0, s1) = if base_y < min_base_y {
                    (tl as u32, left[0] as u32)
                } else if base_y < 0 {
                    (tl as u32, left[0] as u32)
                } else {
                    let by = base_y as usize;
                    (left[by.min(left.len() - 1)] as u32,
                     left[(by + 1).min(left.len() - 1)] as u32)
                };
                let val = s0 * (32 - shift) + s1 * shift;
                pred[r * w + c] = ((val + 16) >> 5) as u8;
            }
        }
    }
    pred
}

fn predict_zone3_angle(left: &[u8], w: usize, h: usize, dy: i32, mrl: usize) -> Vec<u8> {
    let mut pred = vec![0u8; w * h];
    let max_base_y = (w + h) as i32 - 1 + (mrl as i32) * 2;
    let mut pos_y = dy * (1 + mrl as i32);

    for c in 0..w {
        let base = pos_y >> 6;
        let shift = ((pos_y & 0x3F) >> 1) as u32;

        for r in 0..h {
            let b = base + r as i32;
            if b < max_base_y {
                let val = left[b as usize] as u32 * (32 - shift)
                    + left[(b + 1) as usize] as u32 * shift;
                pred[r * w + c] = ((val + 16) >> 5) as u8;
            } else {
                for ri in r..h {
                    pred[ri * w + c] = left[max_base_y as usize];
                }
                break;
            }
        }
        pos_y += dy;
    }
    pred
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

#[rustfmt::skip]
const SM_WEIGHT_ARRAYS: [u8; 128] = [
    // offset 0: unused (2 bytes)
    0, 0,
    // offset 2: bs=2
    255, 128,
    // offset 4: bs=4
    255, 149, 85, 64,
    // offset 8: bs=8
    255, 197, 146, 105, 73, 50, 37, 32,
    // offset 16: bs=16
    255, 225, 196, 170, 145, 123, 102, 84, 68, 54, 43, 33, 26, 20, 17, 16,
    // offset 32: bs=32
    255, 240, 225, 210, 196, 182, 169, 157, 145, 133, 122, 111, 101, 92, 83, 74,
    66, 59, 52, 45, 39, 34, 29, 25, 21, 17, 14, 12, 10, 9, 8, 8,
    // offset 64: bs=64
    255, 248, 240, 233, 225, 218, 210, 203, 196, 189, 182, 176, 169, 163, 156, 150,
    144, 138, 133, 127, 121, 116, 111, 106, 101, 96, 91, 86, 82, 77, 73, 69,
    65, 61, 57, 54, 50, 47, 44, 41, 38, 35, 32, 29, 27, 25, 22, 20,
    18, 16, 15, 13, 12, 10, 9, 8, 7, 6, 6, 5, 5, 4, 4, 4,
];

fn smooth_weight_row(size: usize) -> &'static [u8] {
    &SM_WEIGHT_ARRAYS[size..size * 2]
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
    mode: PredictionMode,
) -> Vec<u8> {
    let (above, left, _tl) = if !has_above && !has_left {
        default_ref_samples(w, h)
    } else {
        ref_samples(recon, stride, x, y, w, h, has_above, has_left)
    };

    let below_pred = left[h - 1] as u32;
    let right_pred = above[w - 1] as u32;
    let sm_weights_h = smooth_weight_row(h);
    let sm_weights_w = smooth_weight_row(w);
    let mut pred = vec![0u8; w * h];

    match mode {
        PredictionMode::SmoothVPred => {
            for row in 0..h {
                let wt = sm_weights_h[row] as u32;
                for col in 0..w {
                    let top = above[col] as u32;
                    let val = wt * top + (256 - wt) * below_pred;
                    pred[row * w + col] = ((val + 128) >> 8) as u8;
                }
            }
        }
        PredictionMode::SmoothHPred => {
            for row in 0..h {
                for col in 0..w {
                    let wl = sm_weights_w[col] as u32;
                    let lft = left[row] as u32;
                    let val = wl * lft + (256 - wl) * right_pred;
                    pred[row * w + col] = ((val + 128) >> 8) as u8;
                }
            }
        }
        _ => {
            for row in 0..h {
                let wt = sm_weights_h[row] as u32;
                for col in 0..w {
                    let top = above[col] as u32;
                    let lft = left[row] as u32;
                    let wl = sm_weights_w[col] as u32;
                    let vert = wt * top + (256 - wt) * below_pred;
                    let horz = wl * lft + (256 - wl) * right_pred;
                    pred[row * w + col] = ((vert + horz + 256) >> 9) as u8;
                }
            }
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
    match (has_above, has_left) {
        (true, true) => {
            let mut above = vec![0u8; w];
            let mut left = vec![0u8; h];
            for i in 0..w {
                above[i] = recon[(y - 1) * stride + x + i];
            }
            for j in 0..h {
                left[j] = recon[(y + j) * stride + x - 1];
            }
            let tl = recon[(y - 1) * stride + x - 1];
            (above, left, tl)
        }
        (true, false) => {
            let mut above = vec![0u8; w];
            for i in 0..w {
                above[i] = recon[(y - 1) * stride + x + i];
            }
            let fill = above[0];
            let left = vec![fill; h];
            (above, left, fill)
        }
        (false, true) => {
            let mut left = vec![0u8; h];
            for j in 0..h {
                left[j] = recon[(y + j) * stride + x - 1];
            }
            let fill = left[0];
            let above = vec![fill; w];
            (above, left, fill)
        }
        (false, false) => {
            (vec![DEFAULT_ABOVE; w], vec![DEFAULT_LEFT; h], DEFAULT_TOP_LEFT)
        }
    }
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
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::DcPred, 0, 0, 0);
        assert!(pred.iter().all(|&v| v == 128));
    }

    #[test]
    fn d45_no_neighbors_gives_127() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::D45Pred, 0, 0, 0);
        assert!(pred.iter().all(|&v| v == 127));
    }

    #[test]
    fn v_pred_no_neighbors_gives_127() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::VPred, 0, 0, 0);
        assert!(pred.iter().all(|&v| v == 127));
    }

    #[test]
    fn h_pred_no_neighbors_gives_129() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::HPred, 0, 0, 0);
        assert!(pred.iter().all(|&v| v == 129));
    }

    #[test]
    fn paeth_no_neighbors_gives_128() {
        let recon = vec![0u8; 64 * 64];
        let pred = predict_intra(&recon, 64, 0, 0, 64, 64, PredictionMode::PaethPred, 0, 0, 0);
        assert!(pred.iter().all(|&v| v == 128));
    }

    #[test]
    fn dc_with_above() {
        let mut recon = vec![0u8; 64 * 64];
        recon[..64].fill(200);
        let pred = predict_intra(&recon, 64, 0, 1, 64, 63, PredictionMode::DcPred, 0, 0, 0);
        assert_eq!(pred[0], 200);
    }

    #[test]
    fn v_pred_with_above() {
        let mut recon = vec![0u8; 128 * 128];
        for (i, pixel) in recon[..64].iter_mut().enumerate() {
            *pixel = 100 + i as u8;
        }
        let pred = predict_intra(&recon, 128, 0, 1, 64, 64, PredictionMode::VPred, 0, 0, 0);
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

    #[test]
    fn zone1_d45_uniform_above() {
        let mut recon = vec![200u8; 128 * 128];
        recon[..128].fill(100);
        let pred = predict_intra(&recon, 128, 0, 1, 8, 8, PredictionMode::D45Pred, 0, 0, 0);
        for &v in &pred {
            assert_eq!(v, 100, "D45 with uniform above should produce 100 everywhere");
        }
    }

    #[test]
    fn zone3_d203_uniform_left() {
        let mut recon = vec![200u8; 128 * 128];
        for r in 0..128 {
            recon[r * 128] = 100;
        }
        let pred = predict_intra(&recon, 128, 1, 0, 8, 8, PredictionMode::D203Pred, 0, 0, 0);
        for &v in &pred {
            assert_eq!(v, 100, "D203 with uniform left should produce 100 everywhere");
        }
    }

    #[test]
    fn dc_ibp_blends_top_and_left_regions() {
        let stride = 16;
        let mut recon = vec![0u8; stride * stride];
        recon[..stride].fill(64);
        for r in 0..stride {
            recon[r * stride] = 192;
        }

        let pred = predict_intra_with_control(
            &recon,
            stride,
            1,
            1,
            8,
            8,
            PredictionMode::DcPred,
            0,
            0,
            0,
            IntraPredictControl {
                plane: 0,
                enable_ibp: true,
                is_cfl: false,
            },
        );

        assert_eq!(pred[0], 107);
        assert_eq!(pred[2 * 8], 149);
        assert_eq!(pred[3 * 8 + 3], 128);
    }

    #[test]
    fn dc_ibp_skips_cfl_chroma() {
        let stride = 16;
        let mut recon = vec![0u8; stride * stride];
        recon[..stride].fill(64);
        for r in 0..stride {
            recon[r * stride] = 192;
        }

        let base = predict_intra(&recon, stride, 1, 1, 8, 8, PredictionMode::DcPred, 0, 0, 0);
        let pred = predict_intra_with_control(
            &recon,
            stride,
            1,
            1,
            8,
            8,
            PredictionMode::DcPred,
            0,
            0,
            0,
            IntraPredictControl {
                plane: 1,
                enable_ibp: true,
                is_cfl: true,
            },
        );

        assert_eq!(pred, base);
    }

    #[test]
    fn directional_ibp_requires_even_delta_luma() {
        let stride = 16;
        let mut recon = vec![0u8; stride * stride];
        for c in 0..stride {
            recon[c] = 32 + c as u8;
        }
        for r in 0..stride {
            recon[r * stride] = 160;
        }

        let base = predict_intra(&recon, stride, 1, 1, 8, 8, PredictionMode::D45Pred, 0, 0, 0);
        let enabled = predict_intra_with_control(
            &recon,
            stride,
            1,
            1,
            8,
            8,
            PredictionMode::D45Pred,
            0,
            0,
            0,
            IntraPredictControl {
                plane: 0,
                enable_ibp: true,
                is_cfl: false,
            },
        );
        let odd_delta = predict_intra_with_control(
            &recon,
            stride,
            1,
            1,
            8,
            8,
            PredictionMode::D45Pred,
            1,
            0,
            0,
            IntraPredictControl {
                plane: 0,
                enable_ibp: true,
                is_cfl: false,
            },
        );

        assert_ne!(enabled, base);
        assert_eq!(odd_delta, predict_intra(&recon, stride, 1, 1, 8, 8, PredictionMode::D45Pred, 1, 0, 0));
    }
}
