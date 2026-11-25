//! MOD -> WAV renderer. Port of the rendering logic of pt2_mod2wav.c.

use crate::consts::*;
use crate::engine::Engine;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug)]
pub struct Mod2WavOptions {
    pub output_rate: u32,
    /// how many extra times the song is looped (0..50)
    pub num_loops: i8,
    /// fade out over the last `fadeout_seconds` seconds
    pub fadeout: bool,
    pub fadeout_seconds: u32,
    /// Safety cap: stop after this many frames (0 = unlimited). Some songs never
    /// end (e.g. certain E6x pattern-loop combinations); pt2-clone renders those forever.
    pub max_frames: usize,
}

impl Default for Mod2WavOptions {
    fn default() -> Self {
        Mod2WavOptions { output_rate: 44100, num_loops: 0, fadeout: false, fadeout_seconds: 6, max_frames: 0 }
    }
}

/// Renders the whole song and returns interleaved stereo 16-bit samples.
/// `progress` is called once per replayer tick with the number of frames rendered so far.
pub fn render_mod(engine: &mut Engine, opt: &Mod2WavOptions, mut progress: impl FnMut(usize)) -> Vec<i16> {
    let rate = opt.output_rate;

    // do some prep work (mod2WavRender)
    engine.rep.mod2wav_ongoing = true;
    engine.audio.oversampling = true; // MOD2WAV always uses 2x oversampling
    engine.audio.output_rate = rate;
    let vblank = engine.rep.timing_mode == TimingMode::Vblank;
    engine.generate_bpm_table(rate as f64, vblank);
    let model = engine.audio.amiga_model;
    engine.paula.setup((rate * 2) as f64, model);
    engine.restart_song();
    engine.clear_downsample_states();

    let mut out: Vec<i16> = Vec::new();
    let mut tick_buf: Vec<i16> = Vec::new();
    let mut samples_to_mix_frac: u64 = 0;
    let mut num_loops = opt.num_loops;

    let mut render_done = false;
    while !render_done {
        if opt.max_frames != 0 && out.len() / 2 >= opt.max_frames {
            break;
        }
        if !engine.tick_replayer() {
            num_loops -= 1;
            if num_loops < 0 {
                render_done = true; // this tick is the last tick
            } else {
                engine.rep.row_visit_table.fill(false);
            }
        }

        let mut samples_to_mix = engine.audio.samples_per_tick_int as usize;
        samples_to_mix_frac += engine.audio.samples_per_tick_frac;
        if samples_to_mix_frac >= BPM_FRAC_SCALE {
            samples_to_mix_frac &= BPM_FRAC_MASK;
            samples_to_mix += 1;
        }

        tick_buf.resize(samples_to_mix * 2, 0);
        engine.output_audio(&mut tick_buf, samples_to_mix);
        out.extend_from_slice(&tick_buf);
        progress(out.len() / 2);
    }

    // fadeout
    if opt.fadeout {
        let total = out.len() / 2;
        let num_fade = ((rate * opt.fadeout_seconds) as usize).min(total);
        if num_fade > 0 {
            let delta = 1.0 / num_fade as f64;
            let mut val = 1.0f64;
            let start = total - num_fade;
            for i in 0..num_fade {
                let k = (start + i) * 2;
                out[k] = (out[k] as f64 * val) as i16;
                out[k + 1] = (out[k + 1] as f64 * val) as i16;
                val -= delta;
            }
        }
    }

    // resetSong()-ish
    engine.rep.mod2wav_ongoing = false;
    engine.mod_stop();

    out
}

/// Writes a 16-bit stereo PCM WAV file (same 44-byte header layout as pt2-clone).
pub fn write_wav<W: Write>(w: &mut W, samples: &[i16], sample_rate: u32) -> io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let num_channels: u16 = 2;
    let bits: u16 = 16;
    let byte_rate = sample_rate * num_channels as u32 * bits as u32 / 8;
    let block_align = num_channels * bits / 8;

    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&num_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;

    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    w.write_all(&bytes)
}
