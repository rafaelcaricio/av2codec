const CDF_PROB_TOP: u32 = 32768;
const EC_PROB_SHIFT: u32 = 7;

#[rustfmt::skip]
static PROB_INC_TBL: [[u16; 16]; 15] = [
    [8,  0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [10, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [12, 8, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [12, 9, 6, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [13,10, 8, 5, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [13,11, 9, 6, 4, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [14,12,10, 8, 6, 4, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [14,12,10, 8, 7, 5, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [14,12,11, 9, 8, 6, 4, 3, 1, 0, 0, 0, 0, 0, 0, 0],
    [14,13,11,10, 8, 7, 5, 4, 2, 1, 0, 0, 0, 0, 0, 0],
    [14,13,12,10, 9, 8, 6, 5, 4, 2, 1, 0, 0, 0, 0, 0],
    [14,13,12,11, 9, 8, 7, 6, 4, 3, 2, 1, 0, 0, 0, 0],
    [14,13,12,11,10, 9, 8, 6, 5, 4, 3, 2, 1, 0, 0, 0],
    [14,13,12,11,10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0],
    [15,14,13,12,11,10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
];

fn ilog_nz(x: u32) -> u32 {
    debug_assert!(x > 0);
    32 - x.leading_zeros()
}

fn prob_scale(p: u16, r: u32, n: usize, nsym: usize) -> u32 {
    let rr = r >> 8;
    let mut pp = (u32::from(p) >> EC_PROB_SHIFT) << 4;
    pp += u32::from(PROB_INC_TBL[nsym - 2][n]);
    ((rr * pp) >> 7) << 3
}

pub struct DaalaCoder {
    precarry: Vec<u16>,
    low: u64,
    rng: u32,
    cnt: i32,
}

impl Default for DaalaCoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DaalaCoder {
    pub fn new() -> Self {
        Self {
            precarry: Vec::new(),
            low: 0,
            rng: 0x8000,
            cnt: -9,
        }
    }

    fn normalize(&mut self, mut low: u64, rng: u32, n_bypass: i32) {
        debug_assert!(rng <= 65535);

        let (c_init, d) = if n_bypass > 0 {
            (self.cnt + n_bypass, 0)
        } else {
            (self.cnt, 16 - ilog_nz(rng) as i32)
        };

        let mut s = c_init + d;

        if s >= 0 {
            let mut c = c_init + 16;
            let mut m: u64 = (1u64 << c) - 1;

            if s >= 8 {
                self.precarry.push((low >> c) as u16);
                low &= m;
                c -= 8;
                m >>= 8;
            }

            self.precarry.push((low >> c) as u16);
            s = c + d - 24;
            low &= m;
        }

        self.low = low << d as u32;
        self.rng = rng << d as u32;
        self.cnt = s;
    }

    fn encode_q15(&mut self, fl: u32, fh: u32, s: usize, nsyms: usize) {
        let l = self.low;
        let r = self.rng;
        debug_assert!(r & 1 == 0);
        debug_assert!(r >= 0x8000);
        debug_assert!(fh <= fl);
        debug_assert!(fl <= CDF_PROB_TOP);

        let (new_l, new_r) = if fl < CDF_PROB_TOP {
            let u = prob_scale(fl as u16, r, s - 1, nsyms);
            let v = prob_scale(fh as u16, r, s, nsyms);
            (l + (r - u) as u64, u - v)
        } else {
            let v = prob_scale(fh as u16, r, s, nsyms);
            (l, r - v)
        };

        self.normalize(new_l, new_r, 0);
    }

    pub fn encode_bool(&mut self, val: bool, f: u16) {
        debug_assert!(f > 0 && f < 32768);
        let l = self.low;
        let r = self.rng;
        debug_assert!(r >= 0x8000);

        let v = prob_scale(f, r, 0, 2);
        let (new_l, new_r) = if val {
            (l + (r - v) as u64, v)
        } else {
            (l, r - v)
        };

        self.normalize(new_l, new_r, 0);
    }

    pub fn encode_symbol(&mut self, s: usize, icdf: &[u16]) {
        let nsyms = icdf.len();
        debug_assert!(s < nsyms);
        debug_assert_eq!(icdf[nsyms - 1], 0);

        let fl = if s > 0 {
            u32::from(icdf[s - 1])
        } else {
            CDF_PROB_TOP
        };
        let fh = u32::from(icdf[s]);
        self.encode_q15(fl, fh, s, nsyms);
    }

    pub fn encode_literal(&mut self, val: u32, n_bits: u32) {
        let l = self.low;
        let r = self.rng;
        debug_assert!(r >= 0x8000);
        debug_assert!(r & 1 == 0);

        let new_l = (l << n_bits) + u64::from(r) * u64::from(val);
        self.normalize(new_l, r, n_bits as i32);
    }

    pub fn tell(&self) -> i32 {
        (self.cnt + 10) + (self.precarry.len() as i32) * 8
    }

    pub fn done(mut self) -> Vec<u8> {
        let l = self.low;
        let mut c = self.cnt;
        let m: u64 = 0x3FFF;
        let mut e = ((l + m) & !m) | (m + 1);
        let mut s = 10 + c;

        if s > 0 {
            let mut n: u64 = (1u64 << (c + 16)) - 1;
            while s > 0 {
                self.precarry.push((e >> (c + 16)) as u16);
                e &= n;
                s -= 8;
                c -= 8;
                n >>= 8;
            }
        }

        let offs = self.precarry.len();
        let mut out = vec![0u8; offs];
        let mut carry: u32 = 0;
        for i in (0..offs).rev() {
            carry += u32::from(self.precarry[i]);
            out[i] = carry as u8;
            carry >>= 8;
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_encoder() {
        let enc = DaalaCoder::new();
        let out = enc.done();
        assert_eq!(out, vec![0x80]);
    }

    #[test]
    fn bool_false_5050() {
        let mut enc = DaalaCoder::new();
        enc.encode_bool(false, 16384);
        let out = enc.done();
        assert_eq!(out, vec![0x20]);
    }

    #[test]
    fn bool_true_5050() {
        let mut enc = DaalaCoder::new();
        enc.encode_bool(true, 16384);
        let out = enc.done();
        assert_eq!(out, vec![0xC0]);
    }

    #[test]
    fn symbol_first_of_two() {
        let mut enc = DaalaCoder::new();
        enc.encode_symbol(0, &[12276, 0]);
        let out = enc.done();
        assert!(!out.is_empty());
    }

    #[test]
    fn symbol_second_of_two() {
        let mut enc = DaalaCoder::new();
        enc.encode_symbol(1, &[12276, 0]);
        let out = enc.done();
        assert!(!out.is_empty());
    }

    #[test]
    fn tell_increases() {
        let mut enc = DaalaCoder::new();
        let t0 = enc.tell();
        enc.encode_bool(false, 16384);
        let t1 = enc.tell();
        assert!(t1 > t0);
    }

    #[test]
    fn literal_bypass() {
        let mut enc = DaalaCoder::new();
        enc.encode_literal(42, 8);
        let out = enc.done();
        assert!(!out.is_empty());
    }

    #[test]
    fn multiple_symbols_reasonable_size() {
        let mut enc = DaalaCoder::new();
        enc.encode_symbol(0, &[12276, 0]);
        enc.encode_symbol(1, &[6903, 0]);
        enc.encode_symbol(0, &[3905, 1746, 1044, 0]);
        enc.encode_symbol(0, &[17593, 12693, 11040, 8670, 6363, 5113, 3908, 0]);
        enc.encode_symbol(0, &[23405, 11811, 9903, 8015, 6357, 4785, 2340, 0]);
        let out = enc.done();
        assert!(out.len() <= 10);
    }

    #[test]
    fn deterministic() {
        let encode = || {
            let mut enc = DaalaCoder::new();
            enc.encode_bool(true, 8000);
            enc.encode_symbol(0, &[3905, 1746, 1044, 0]);
            enc.encode_literal(7, 3);
            enc.done()
        };
        assert_eq!(encode(), encode());
    }
}
