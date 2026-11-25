//! Engine = module + Paula + replayer + output mixer.
//! The mixer part is a port of the output path of pt2_audio.c.

use crate::consts::*;
use crate::dsp::Downsample2x;
use crate::module::Module;
use crate::paula::{Paula, PAULA_VOICES};
use crate::replayer::ReplayerState;

/// cumulative mid/side normalization factor (1/sqrt(2))*(1/sqrt(2))
const STEREO_NORM_FACTOR: f32 = 0.5;
const INITIAL_DITHER_SEED: u32 = 0x12345000;
const AUDIO_GAIN: f64 = 2.0;
const NORMALIZE_VALUE: f32 = (AUDIO_GAIN * ((i16::MAX as f64 + 1.0) / PAULA_VOICES as f64)) as f32;

#[derive(Clone, Copy, Debug)]
pub struct EngineConfig {
    pub output_rate: u32,
    pub amiga_model: AmigaModel,
    /// 0..100 (%), pt2-clone default is 20
    pub stereo_separation: u8,
    pub max_sample_length: usize,
    pub enable_e8x: bool,
    pub timing_mode: TimingMode,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            output_rate: 44100,
            amiga_model: AmigaModel::A1200,
            stereo_separation: 20,
            max_sample_length: crate::module::DEFAULT_MAX_SAMPLE_LENGTH,
            enable_e8x: false,
            timing_mode: TimingMode::Cia,
        }
    }
}

pub struct AudioState {
    pub output_rate: u32,
    pub oversampling: bool,
    pub amiga_model: AmigaModel,
    pub tick_sample_counter: i32,
    pub tick_sample_counter_frac: u64,
    pub samples_per_tick_int: u32,
    pub samples_per_tick_frac: u64,
    pub samples_per_tick_int_tab: [u32; NUM_BPMS],
    pub samples_per_tick_frac_tab: [u64; NUM_BPMS],
}

pub struct Engine {
    pub song: Module,
    pub paula: Paula,
    pub rep: ReplayerState,
    pub audio: AudioState,

    stereo_separation: i32,
    side_factor: f32,
    rand_seed: u32,
    prng_state_l: f32,
    prng_state_r: f32,
    ds_l: Downsample2x,
    ds_r: Downsample2x,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
}

impl Engine {
    pub fn new(song: Module, cfg: EngineConfig) -> Self {
        let null_ptr = song.null_ptr();
        let mut e = Engine {
            song,
            paula: Paula::new(null_ptr),
            rep: ReplayerState::default(),
            audio: AudioState {
                output_rate: cfg.output_rate,
                oversampling: cfg.output_rate < 96000,
                amiga_model: cfg.amiga_model,
                tick_sample_counter: 0,
                tick_sample_counter_frac: 0,
                samples_per_tick_int: 0,
                samples_per_tick_frac: 0,
                samples_per_tick_int_tab: [0; NUM_BPMS],
                samples_per_tick_frac_tab: [0; NUM_BPMS],
            },
            stereo_separation: 100,
            side_factor: 0.0,
            rand_seed: INITIAL_DITHER_SEED,
            prng_state_l: 0.0,
            prng_state_r: 0.0,
            ds_l: Downsample2x::default(),
            ds_r: Downsample2x::default(),
            mix_l: Vec::new(),
            mix_r: Vec::new(),
        };
        e.rep.enable_e8x = cfg.enable_e8x;
        e.rep.timing_mode = cfg.timing_mode;
        e.set_stereo_separation(cfg.stereo_separation);

        let paula_freq = if e.audio.oversampling { cfg.output_rate * 2 } else { cfg.output_rate };
        e.paula.setup(paula_freq as f64, cfg.amiga_model);
        e.generate_bpm_table(cfg.output_rate as f64, cfg.timing_mode == TimingMode::Vblank);
        e.set_led_filter(false);
        e.clear_downsample_states();

        // setupLoadedMod()
        e.mod_set_speed(6);
        let tempo = e.song.initial_tempo as i32;
        e.mod_set_tempo(tempo);
        e
    }

    pub fn set_stereo_separation(&mut self, percentage: u8) {
        let p = percentage.min(100);
        self.stereo_separation = p as i32;
        self.side_factor = (p as i32 as f32 / 100.0f32) * STEREO_NORM_FACTOR;
    }

    pub fn clear_downsample_states(&mut self) {
        self.ds_l.clear();
        self.ds_r.clear();
    }

    pub fn reset_audio_dither(&mut self) {
        self.rand_seed = INITIAL_DITHER_SEED;
        self.prng_state_l = 0.0;
        self.prng_state_r = 0.0;
    }

    pub fn generate_bpm_table(&mut self, audio_freq: f64, vblank: bool) {
        for bpm in MIN_BPM..=MAX_BPM {
            let hz = if vblank { AMIGA_PAL_VBLANK_HZ } else { cia_bpm_to_hz(bpm) };
            let spt = audio_freq / hz;
            let int_part = spt.trunc();
            let frac_part = spt - int_part;
            let i = (bpm - MIN_BPM) as usize;
            self.audio.samples_per_tick_int_tab[i] = int_part as u32;
            self.audio.samples_per_tick_frac_tab[i] = (frac_part * BPM_FRAC_SCALE as f64) as u64;
        }
        self.audio.tick_sample_counter = 0;
        self.audio.tick_sample_counter_frac = 0;
    }

    #[inline]
    fn random32(&mut self) -> i32 {
        self.rand_seed = self.rand_seed.wrapping_mul(134775813).wrapping_add(1);
        self.rand_seed as i32
    }

    #[inline]
    fn dither_out(&mut self, f_l: f32, f_r: f32, out: &mut [i16]) {
        const SCALE: f32 = 1.0 / (u32::MAX as f32 + 1.0);

        // left channel - 1-bit triangular dithering
        let prng = self.random32() as f32 * SCALE;
        let f_out = (f_l + prng) - self.prng_state_l;
        self.prng_state_l = prng;
        out[0] = (f_out as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;

        // right channel
        let prng = self.random32() as f32 * SCALE;
        let f_out = (f_r + prng) - self.prng_state_r;
        self.prng_state_r = prng;
        out[1] = (f_out as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }

    /// outputAudio(): renders `num_samples` stereo frames into `target` (interleaved).
    pub fn output_audio(&mut self, target: &mut [i16], num_samples: usize) {
        let paula_samples = if self.audio.oversampling { num_samples * 2 } else { num_samples };
        if self.mix_l.len() < paula_samples {
            self.mix_l.resize(paula_samples, 0.0);
            self.mix_r.resize(paula_samples, 0.0);
        }

        let mut mix_l = std::mem::take(&mut self.mix_l);
        let mut mix_r = std::mem::take(&mut self.mix_r);
        self.paula.generate(&self.song.sample_data, &mut mix_l[..paula_samples], &mut mix_r[..paula_samples]);

        let amiga_panning = self.stereo_separation == 100;
        for i in 0..num_samples {
            let (mut f_l, mut f_r) = if self.audio.oversampling {
                let l = self.ds_l.process(mix_l[i * 2], mix_l[i * 2 + 1]);
                let r = self.ds_r.process(mix_r[i * 2], mix_r[i * 2 + 1]);
                (l, r)
            } else {
                (mix_l[i], mix_r[i])
            };

            if !amiga_panning {
                let mid = (f_l + f_r) * STEREO_NORM_FACTOR;
                let side = (f_l - f_r) * self.side_factor;
                f_l = mid + side;
                f_r = mid - side;
            }

            f_l *= NORMALIZE_VALUE;
            f_r *= NORMALIZE_VALUE;

            self.dither_out(f_l, f_r, &mut target[i * 2..i * 2 + 2]);
        }

        self.mix_l = mix_l;
        self.mix_r = mix_r;
    }

    /// Real-time style rendering (what the SDL audio callback does):
    /// fills `stream` (interleaved stereo i16), ticking the replayer as needed.
    pub fn render(&mut self, stream: &mut [i16]) {
        let mut samples_left = stream.len() / 2;
        let mut pos = 0usize;
        while samples_left > 0 {
            if self.audio.tick_sample_counter <= 0 {
                if self.rep.song_playing {
                    self.tick_replayer();
                }
                self.audio.tick_sample_counter = self.audio.samples_per_tick_int as i32;
                self.audio.tick_sample_counter_frac += self.audio.samples_per_tick_frac;
                if self.audio.tick_sample_counter_frac >= BPM_FRAC_SCALE {
                    self.audio.tick_sample_counter_frac &= BPM_FRAC_MASK;
                    self.audio.tick_sample_counter += 1;
                }
            }

            let mut to_mix = samples_left;
            if self.audio.tick_sample_counter > 0 && to_mix > self.audio.tick_sample_counter as usize {
                to_mix = self.audio.tick_sample_counter as usize;
            }

            self.output_audio(&mut stream[pos * 2..(pos + to_mix) * 2], to_mix);
            pos += to_mix;
            self.audio.tick_sample_counter -= to_mix as i32;
            samples_left -= to_mix;
        }
    }

    /// Starts normal song playback from order 0 (for real-time playback).
    pub fn play_song(&mut self) {
        self.rep.play_mode = crate::replayer::PlayMode::Normal;
        self.mod_play(-1, 0, 0);
    }
}
