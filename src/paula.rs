//! Paula (Amiga audio chip) emulator with BLEP synthesis.
//! Port of pt2_paula.c.
//!
//! Sample memory is addressed with offsets (`usize`) into one shared `i8` buffer
//! owned by the module (see `Module::sample_data`), instead of raw pointers.

use crate::consts::{AMIGA_PAL_CCK_HZ, AmigaModel};
use crate::dsp::{Blep, OnePoleFilter, TwoPoleFilter};
use std::f64::consts::PI;

pub const PAULA_VOICES: usize = 4;
pub const PAULA_PAL_CLK: f64 = AMIGA_PAL_CCK_HZ;

/// Optional sample address. `None` mirrors a NULL pointer in the C code.
pub type SmpPtr = Option<usize>;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,

    // internal registers
    sample_just_started: bool,
    next_sample_stage: bool,
    aud_dat: [i8; 2],
    location: SmpPtr,
    length_counter: u16,
    sample_counter: i32,
    f_sample: f32,
    d_delta: f64,
    d_inv_delta: f64,
    d_phase: f64,
    d_blep_delta: f64,
    d_blep_inv_delta: f64,
    d_blep_phase: f64,

    // registers modified by Paula functions
    stored_location: SmpPtr,
    stored_length: u16,
    f_stored_vol: f32,
    d_stored_delta: f64,
    d_stored_inv_delta: f64,
}

pub struct Paula {
    use_led_filter: bool,
    use_lowpass_filter: bool,
    use_highpass_filter: bool,
    d_period_to_delta_div: f64,
    d_output_freq: f64,
    blep: [Blep; PAULA_VOICES],
    filter_lo: OnePoleFilter,
    filter_hi: OnePoleFilter,
    filter_led: TwoPoleFilter,
    voice: [Voice; PAULA_VOICES],
    /// Offset of the zero-filled "null sample" area inside the sample buffer.
    null_ptr: usize,
}

#[inline]
fn read_smp(data: &[i8], pos: usize) -> i8 {
    // Paula may read past the end of a sample (into the next slot), exactly like
    // the C version. Past the very end of the buffer we just return silence.
    data.get(pos).copied().unwrap_or(0)
}

impl Paula {
    pub fn new(null_ptr: usize) -> Self {
        Paula {
            use_led_filter: false,
            use_lowpass_filter: true,
            use_highpass_filter: true,
            d_period_to_delta_div: 0.0,
            d_output_freq: 0.0,
            blep: [Blep::default(); PAULA_VOICES],
            filter_lo: OnePoleFilter::default(),
            filter_hi: OnePoleFilter::default(),
            filter_led: TwoPoleFilter::default(),
            voice: [Voice::default(); PAULA_VOICES],
            null_ptr,
        }
    }

    pub fn set_null_ptr(&mut self, null_ptr: usize) {
        self.null_ptr = null_ptr;
    }

    pub fn null_ptr(&self) -> usize {
        self.null_ptr
    }

    pub fn output_freq(&self) -> f64 {
        self.d_output_freq
    }

    pub fn setup(&mut self, output_freq: f64, model: AmigaModel) {
        assert!(output_freq != 0.0);
        self.d_output_freq = output_freq;
        self.d_period_to_delta_div = PAULA_PAL_CLK / output_freq;

        self.clear_blep_state();

        self.use_lowpass_filter = true;
        self.use_highpass_filter = true;
        self.filter_lo.clear();
        self.filter_hi.clear();
        self.filter_led.clear();

        match model {
            AmigaModel::A1200 => {
                // A1200 low-pass is ~34.4kHz, not emulated
                self.use_lowpass_filter = false;

                let r = 1360.0;
                let c = 2.2e-5;
                let cutoff = 1.0 / ((2.0 * PI) * r * c); // ~5.319Hz
                self.filter_hi.setup(output_freq, cutoff);
            }
            AmigaModel::A500 => {
                let r = 360.0;
                let c = 1e-7;
                let cutoff = 1.0 / ((2.0 * PI) * r * c); // ~4420.971Hz
                self.filter_lo.setup(output_freq, cutoff);

                let r = 1390.0;
                let c = 2.233e-5;
                let cutoff = 1.0 / ((2.0 * PI) * r * c); // ~5.128Hz
                self.filter_hi.setup(output_freq, cutoff);
            }
        }

        // "LED" filter, 2-pole Sallen-Key low-pass (same on A500/A1200)
        let r1: f64 = 10000.0;
        let r2: f64 = 10000.0;
        let c1: f64 = 6.8e-9;
        let c2: f64 = 3.9e-9;
        let cutoff = 1.0 / ((2.0 * PI) * (r1 * r2 * c1 * c2).sqrt()); // ~3090.533Hz
        let qfactor = (r1 * r2 * c1 * c2).sqrt() / (c2 * (r1 + r2)); // ~0.660225
        self.filter_led.setup(output_freq, cutoff, qfactor);
    }

    /// Disables low-pass/high-pass filter ("LED" filter is kept).
    pub fn disable_filters(&mut self) {
        self.use_highpass_filter = false;
        self.use_lowpass_filter = false;
    }

    pub fn clear_blep_state(&mut self) {
        self.blep = [Blep::default(); PAULA_VOICES];
    }

    pub fn led_filter(&self) -> bool {
        self.use_led_filter
    }

    /// Scope info for a voice: (current read position, bytes left in this DMA
    /// cycle, loop start, loop length in bytes, volume 0..64). `None` if inactive.
    pub fn scope_info(&self, ch: usize) -> Option<(usize, usize, usize, usize, u8)> {
        let v = &self.voice[ch];
        if !v.active {
            return None;
        }
        let loc = v.location?;
        let stored = v.stored_location?;
        let len_words = if v.length_counter == 0 { 0x10000 } else { v.length_counter as usize };
        let sc = v.sample_counter.clamp(0, 2) as usize;
        let left = sc + len_words.saturating_sub(1) * 2;
        let loop_len = if v.stored_length == 0 { 0x20000 } else { v.stored_length as usize * 2 };
        let vol = (v.f_stored_vol * 8192.0 + 0.5) as u8;
        Some((loc.saturating_sub(sc), left.max(1), stored, loop_len, vol))
    }

    // ---- register writes -------------------------------------------------

    /// CIA-A PRA ($BFE001) write; only bit 1 ("LED" filter) is handled.
    pub fn write_cia_pra(&mut self, data8: u8) {
        let old = self.use_led_filter;
        self.use_led_filter = (data8 & 2) != 0;
        if self.use_led_filter != old {
            self.filter_led.clear();
        }
    }

    /// DMACON ($DFF096)
    pub fn write_dmacon(&mut self, data16: u16) {
        let set = data16 & 0x8000 != 0;
        for ch in 0..PAULA_VOICES {
            if data16 & (1 << ch) != 0 {
                if set {
                    self.start_dma(ch);
                } else {
                    self.voice[ch].active = false;
                }
            }
        }
    }

    /// AUDxLEN
    pub fn write_len(&mut self, ch: usize, len: u16) {
        self.voice[ch].stored_length = len;
    }

    /// AUDxPER
    pub fn write_per(&mut self, ch: usize, period: u16) {
        let v = &mut self.voice[ch];

        let mut real_period = period as i32;
        if real_period == 0 {
            real_period = 65536; // On Amiga: period 0 = period 65536
        } else if real_period < 113 {
            real_period = 113;
        }

        v.d_stored_delta = self.d_period_to_delta_div / real_period as f64;
        v.d_stored_inv_delta = 1.0 / v.d_stored_delta;

        // BLEP synthesis edge-case
        if v.d_blep_delta == 0.0 {
            v.d_blep_delta = v.d_delta;
            v.d_blep_inv_delta = v.d_inv_delta;
        }
    }

    /// AUDxVOL
    pub fn write_vol(&mut self, ch: usize, vol: u16) {
        let mut use_vol = (vol & 127) as i32;
        if use_vol > 64 {
            use_vol = 64;
        }
        self.voice[ch].f_stored_vol = use_vol as f32 * (1.0 / (128.0 * 64.0)) as f32;
    }

    /// AUDxLC (data pointer)
    pub fn write_ptr(&mut self, ch: usize, src: SmpPtr) {
        self.voice[ch].stored_location = Some(src.unwrap_or(self.null_ptr));
    }

    fn start_dma(&mut self, ch: usize) {
        let null_ptr = self.null_ptr;
        let v = &mut self.voice[ch];

        if v.stored_location.is_none() {
            v.stored_location = Some(null_ptr);
        }

        v.location = v.stored_location;
        v.length_counter = v.stored_length;

        v.sample_counter = 0;
        v.sample_just_started = true;
        refetch_period(v);

        // kludge: must be cleared *after* refetch_period()
        v.d_phase = 0.0;

        v.active = true;
    }

    // ---- mixing ------------------------------------------------------------

    /// Generates `out_l.len()` samples. Output range is about -4.00 .. 3.97.
    pub fn generate(&mut self, data: &[i8], out_l: &mut [f32], out_r: &mut [f32]) {
        let num_samples = out_l.len().min(out_r.len());
        if num_samples == 0 {
            return;
        }

        out_l[..num_samples].fill(0.0);
        out_r[..num_samples].fill(0.0);

        for i in 0..PAULA_VOICES {
            let v = &mut self.voice[i];
            let b = &mut self.blep[i];
            if !v.active || v.location.is_none() || v.stored_location.is_none() {
                continue;
            }

            // output channel mapping: L, R, R, L
            let mix: &mut [f32] = if i == 0 || i == 3 { &mut out_l[..num_samples] } else { &mut out_r[..num_samples] };

            for out in mix.iter_mut() {
                if v.next_sample_stage {
                    v.next_sample_stage = false;
                    next_sample(v, b, data);
                }

                let mut f_sample = v.f_sample;
                if b.samples_left > 0 {
                    f_sample = b.run(f_sample);
                }

                *out += f_sample;

                v.d_phase += v.d_delta;
                if v.d_phase >= 1.0 {
                    v.d_phase -= 1.0;
                    refetch_period(v);
                }
            }
        }

        // Amiga filters
        for i in 0..num_samples {
            let mut f = [out_l[i], out_r[i]];
            if self.use_lowpass_filter {
                self.filter_lo.lp_stereo(&mut f);
            }
            if self.use_led_filter {
                self.filter_led.lp_stereo(&mut f);
            }
            if self.use_highpass_filter {
                self.filter_hi.hp_stereo(&mut f);
            }
            out_l[i] = f[0];
            out_r[i] = f[1];
        }
    }
}

#[inline]
fn refetch_period(v: &mut Voice) {
    v.d_blep_phase = v.d_phase;
    v.d_blep_delta = v.d_delta;
    v.d_blep_inv_delta = v.d_inv_delta;

    // Paula only updates period (delta) during period refetching
    v.d_delta = v.d_stored_delta;
    v.d_inv_delta = v.d_stored_inv_delta;

    v.next_sample_stage = true;
}

#[inline]
fn next_sample(v: &mut Voice, b: &mut Blep, data: &[i8]) {
    if v.sample_counter == 0 {
        // time to read new samples from DMA

        // don't update AUD_LEN/AUD_LC yet on DMA trigger
        if !v.sample_just_started {
            v.length_counter = v.length_counter.wrapping_sub(1);
            if v.length_counter == 0 {
                v.length_counter = v.stored_length;
                v.location = v.stored_location;
            }
        }

        v.sample_just_started = false;

        // fill DMA data buffer
        let loc = v.location.unwrap_or(0);
        v.aud_dat[0] = read_smp(data, loc);
        v.aud_dat[1] = read_smp(data, loc + 1);
        v.location = Some(loc + 2);
        v.sample_counter = 2;
    }

    let sample = v.aud_dat[0] as i32;
    v.f_sample = sample as f32 * v.f_stored_vol;

    // fill BLEP buffer if the new sample differs from the old one
    if v.f_sample != b.last_value {
        if v.d_blep_delta > v.d_blep_phase {
            let blep_offset = v.d_blep_phase * v.d_blep_inv_delta;
            b.add(blep_offset, b.last_value - v.f_sample);
        }
        b.last_value = v.f_sample;
    }

    // progress AUD_DAT buffer
    v.aud_dat[0] = v.aud_dat[1];
    v.sample_counter -= 1;
}
