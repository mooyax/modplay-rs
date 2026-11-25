//! Constants from pt2_header.h / pt2_replayer.h / pt2_audio.h

/// Main crystal oscillator for PAL Amiga systems
pub const AMIGA_PAL_XTAL_HZ: u32 = 28375160;
pub const AMIGA_PAL_CCK_HZ: f64 = AMIGA_PAL_XTAL_HZ as f64 / 8.0;
pub const CIA_PAL_CLK: f64 = AMIGA_PAL_CCK_HZ / 5.0;
/// Nominal framerate in normal PAL videomodes (~49.92Hz)
pub const AMIGA_PAL_VBLANK_HZ: f64 = AMIGA_PAL_CCK_HZ / (313 * 227) as f64;

pub const MOD_ROWS: usize = 64;
pub const MOD_SAMPLES: usize = 31;
pub const MAX_PATTERNS: usize = 100;

pub const MIN_BPM: i32 = 32;
pub const MAX_BPM: i32 = 255;
pub const NUM_BPMS: usize = (MAX_BPM - MIN_BPM + 1) as usize;

pub const MIN_AUDIO_FREQUENCY: u32 = 44100;
pub const MAX_AUDIO_FREQUENCY: u32 = 192000;

pub const BPM_FRAC_BITS: u32 = 52;
pub const BPM_FRAC_SCALE: u64 = 1u64 << BPM_FRAC_BITS;
pub const BPM_FRAC_MASK: u64 = BPM_FRAC_SCALE - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmigaModel {
    A1200,
    A500,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingMode {
    Cia,
    Vblank,
}

/// CIA timer BPM -> Hz (ProTracker style, with its truncation quirk)
pub fn cia_bpm_to_hz(bpm: i32) -> f64 {
    if bpm == 0 {
        return 0.0;
    }
    let cia_period: u32 = 1773447 / bpm as u32; // yes, PT truncates here
    CIA_PAL_CLK / (cia_period + 1) as f64 // +1, CIA triggers on underflow
}
