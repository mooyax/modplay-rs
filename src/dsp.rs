//! DSP building blocks ported 1:1 from pt2-clone:
//! - pt2_blep.c        (minBLEP synthesis, by aciddose)
//! - pt2_rcfilters.c   (Amiga RC / Sallen-Key filters)
//! - pt2_downsample2x.c (half-band 2x downsampler used by the mixer)
//!
//! Floating point operations are kept in the same order and precision as the
//! C code so that the output is bit-identical.

use crate::tables::MIN_BLEP_DATA;
use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// BLEP
// ---------------------------------------------------------------------------

pub const BLEP_ZC: usize = 16;
pub const BLEP_OS: usize = 16;
pub const BLEP_SP: usize = 16;
pub const BLEP_NS: usize = BLEP_ZC * BLEP_OS / BLEP_SP;
pub const BLEP_RNS: usize = 31;

#[derive(Clone, Copy)]
pub struct Blep {
    pub index: i32,
    pub samples_left: i32,
    pub buffer: [f32; BLEP_RNS + 1],
    pub last_value: f32,
}

impl Default for Blep {
    fn default() -> Self {
        Blep { index: 0, samples_left: 0, buffer: [0.0; BLEP_RNS + 1], last_value: 0.0 }
    }
}

impl Blep {
    #[inline]
    pub fn add(&mut self, offset: f64, amplitude: f32) {
        debug_assert!((0.0..1.0).contains(&offset));

        let phase = offset * BLEP_SP as f64;
        let phase_int = phase as i32;
        let phase_frac = (phase - phase_int as f64) as f32;

        let mut src = phase_int as usize;
        let mut i = self.index as usize;
        for _ in 0..BLEP_NS {
            let p1 = MIN_BLEP_DATA[src];
            let p2 = MIN_BLEP_DATA[src + 1];
            self.buffer[i] += amplitude * (p1 + ((p2 - p1) * phase_frac));
            src += BLEP_SP;
            i = (i + 1) & BLEP_RNS;
        }

        self.samples_left = BLEP_NS as i32;
    }

    #[inline]
    pub fn run(&mut self, input: f32) -> f32 {
        let idx = self.index as usize;
        let out = input + self.buffer[idx];
        self.buffer[idx] = 0.0;
        self.index = ((idx + 1) & BLEP_RNS) as i32;
        self.samples_left -= 1;
        out
    }
}

// ---------------------------------------------------------------------------
// RC filters
// ---------------------------------------------------------------------------

const SMALL_NUMBER: f64 = 1e-4;

#[derive(Clone, Copy, Default)]
pub struct OnePoleFilter {
    tmp_l: f32,
    tmp_r: f32,
    a0: f32,
    b1: f32,
}

impl OnePoleFilter {
    pub fn setup(&mut self, audio_rate: f64, mut cut_off: f64) {
        if cut_off >= audio_rate / 2.0 {
            cut_off = (audio_rate / 2.0) - SMALL_NUMBER;
        }
        let b1 = ((-2.0 * PI) * cut_off / audio_rate).exp();
        let a0 = 1.0 - b1;
        self.b1 = b1 as f32;
        self.a0 = a0 as f32;
    }

    pub fn clear(&mut self) {
        self.tmp_l = 0.0;
        self.tmp_r = 0.0;
    }

    #[inline]
    pub fn lp_stereo(&mut self, io: &mut [f32; 2]) {
        self.tmp_l = (io[0] * self.a0) + (self.tmp_l * self.b1);
        io[0] = self.tmp_l;
        self.tmp_r = (io[1] * self.a0) + (self.tmp_r * self.b1);
        io[1] = self.tmp_r;
    }

    #[inline]
    pub fn hp_stereo(&mut self, io: &mut [f32; 2]) {
        self.tmp_l = (io[0] * self.a0) + (self.tmp_l * self.b1);
        io[0] -= self.tmp_l;
        self.tmp_r = (io[1] * self.a0) + (self.tmp_r * self.b1);
        io[1] -= self.tmp_r;
    }
}

#[derive(Clone, Copy, Default)]
pub struct TwoPoleFilter {
    tmp_l: [f32; 4],
    tmp_r: [f32; 4],
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
}

impl TwoPoleFilter {
    pub fn setup(&mut self, audio_rate: f64, mut cut_off: f64, q_factor: f64) {
        if cut_off >= audio_rate / 2.0 {
            cut_off = (audio_rate / 2.0) - SMALL_NUMBER;
        }
        let a = 1.0 / ((PI * cut_off) / audio_rate).tan();
        let r = 1.0 / q_factor;

        let a1 = 1.0 / (1.0 + r * a + a * a);
        let a2 = 2.0 * a1;
        let b1 = 2.0 * (1.0 - a * a) * a1;
        let b2 = (1.0 - r * a + a * a) * a1;

        self.a1 = a1 as f32;
        self.a2 = a2 as f32;
        self.b1 = b1 as f32;
        self.b2 = b2 as f32;
    }

    pub fn clear(&mut self) {
        self.tmp_l = [0.0; 4];
        self.tmp_r = [0.0; 4];
    }

    #[inline]
    pub fn lp_stereo(&mut self, io: &mut [f32; 2]) {
        let (a1, a2, b1, b2) = (self.a1, self.a2, self.b1, self.b2);
        let l = &mut self.tmp_l;
        let r = &mut self.tmp_r;
        let l_out = (io[0] * a1) + (l[0] * a2) + (l[1] * a1) - (l[2] * b1) - (l[3] * b2);
        let r_out = (io[1] * a1) + (r[0] * a2) + (r[1] * a1) - (r[2] * b1) - (r[3] * b2);

        l[1] = l[0];
        l[0] = io[0];
        l[3] = l[2];
        l[2] = l_out;

        r[1] = r[0];
        r[0] = io[1];
        r[3] = r[2];
        r[2] = r_out;

        io[0] = l_out;
        io[1] = r_out;
    }
}

// ---------------------------------------------------------------------------
// 2x half-band downsampler (main mixer version, with output delay)
// ---------------------------------------------------------------------------

const C: [f32; 30] = {
    let mut c = [0.0f32; 30];
    c[0] = 0.50000000000000;
    c[1] = 0.31679609962928;
    c[3] = -0.10163877066856;
    c[5] = 0.05646939759172;
    c[7] = -0.03589869172828;
    c[9] = 0.02384893442862;
    c[11] = -0.01596102646846;
    c[13] = 0.01054794795196;
    c[15] = -0.00678935474634;
    c[17] = 0.00420731862183;
    c[19] = -0.00248066436637;
    c[21] = 0.00137207386220;
    c[23] = -0.00069823637245;
    c[25] = 0.00031710491117;
    c[27] = -0.00012143320790;
    c[29] = 0.00003501888526;
    c
};

/// One channel of the polyphase half-band downsampler. `t[1..=29]` are the delay
/// line taps (t01..t29 in the C code).
#[derive(Clone, Copy)]
pub struct Downsample2x {
    t: [f32; 30],
}

impl Default for Downsample2x {
    fn default() -> Self {
        Downsample2x { t: [0.0; 30] }
    }
}

impl Downsample2x {
    pub fn clear(&mut self) {
        self.t = [0.0; 30];
    }

    #[inline]
    pub fn process(&mut self, sample1: f32, sample2: f32) -> f32 {
        let mut x = [0.0f32; 30];
        x[0] = sample2 * C[0];
        let mut j = 1;
        while j < 30 {
            x[j] = sample1 * C[j];
            j += 2;
        }

        let t = &mut self.t;
        let out = t[29] + x[29];

        // t29..t16: t[k] = t[k-1] + x[2*(k-15)-1]
        for k in (16..=29).rev() {
            t[k] = t[k - 1] + x[2 * (k - 15) - 1];
        }
        // t15 = t14 + x01 + x00
        t[15] = t[14] + x[1] + x[0];
        // t14..t02: t[k] = t[k-1] + x[2*(15-k)+1]
        for k in (2..=14).rev() {
            t[k] = t[k - 1] + x[2 * (15 - k) + 1];
        }
        t[1] = x[29];

        out
    }
}
