//! Impulse Tracker (.IT) module loading.
//!
//! Written from the IT file format description (ITTECH.TXT) with OpenMPT's
//! loader (BSD license) as a reference for edge cases.

use std::fmt;

pub const MAX_CHANNELS: usize = 64;

/// Special note values (internal representation)
pub const NOTE_NONE: u8 = 0;
/// 1..=120 = C-0..B-9
pub const NOTE_FADE: u8 = 253;
pub const NOTE_CUT: u8 = 254;
pub const NOTE_OFF: u8 = 255;
/// "no volume column" marker
pub const VOL_NONE: u8 = 255;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 0 = none, 1..=120 = note (C-0 = 1), 253 fade, 254 cut, 255 off
    pub note: u8,
    /// 0 = none
    pub ins: u8,
    /// raw IT volume column byte, VOL_NONE = empty
    pub vol: u8,
    /// 0 = none, 1 = A ... 26 = Z
    pub cmd: u8,
    pub param: u8,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub rows: usize,
    /// rows * MAX_CHANNELS
    pub cells: Vec<Cell>,
}

impl Pattern {
    pub fn empty(rows: usize) -> Self {
        Pattern { rows, cells: vec![Cell { vol: VOL_NONE, ..Default::default() }; rows * MAX_CHANNELS] }
    }
    #[inline]
    pub fn cell(&self, row: usize, ch: usize) -> &Cell {
        &self.cells[row * MAX_CHANNELS + ch]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvNode {
    pub tick: u16,
    pub value: i8,
}

#[derive(Clone, Debug, Default)]
pub struct Envelope {
    pub enabled: bool,
    pub looped: bool,
    pub sustain: bool,
    pub carry: bool,
    /// pitch envelope used as filter envelope
    pub filter: bool,
    pub nodes: Vec<EnvNode>,
    pub loop_start: usize,
    pub loop_end: usize,
    pub sus_start: usize,
    pub sus_end: usize,
}

impl Envelope {
    /// Interpolated value at `pos` (ticks). Values are in the envelope's own range
    /// (0..64 for volume, -32..32 for pan/pitch).
    pub fn value_at(&self, pos: u32) -> f32 {
        let n = &self.nodes;
        if n.is_empty() {
            return 0.0;
        }
        if pos <= n[0].tick as u32 {
            return n[0].value as f32;
        }
        for i in 1..n.len() {
            let (a, b) = (n[i - 1], n[i]);
            if pos <= b.tick as u32 {
                let span = (b.tick as i32 - a.tick as i32).max(1) as f32;
                let t = (pos as f32 - a.tick as f32) / span;
                return a.value as f32 + (b.value as f32 - a.value as f32) * t;
            }
        }
        n[n.len() - 1].value as f32
    }

    pub fn last_tick(&self) -> u32 {
        self.nodes.last().map(|n| n.tick as u32).unwrap_or(0)
    }

    fn node_tick(&self, i: usize) -> u32 {
        self.nodes.get(i).or(self.nodes.last()).map(|n| n.tick as u32).unwrap_or(0)
    }
    pub fn loop_ticks(&self) -> (u32, u32) {
        (self.node_tick(self.loop_start), self.node_tick(self.loop_end))
    }
    pub fn sus_ticks(&self) -> (u32, u32) {
        (self.node_tick(self.sus_start), self.node_tick(self.sus_end))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Nna {
    #[default]
    Cut,
    Continue,
    Off,
    Fade,
}

impl Nna {
    fn from(v: u8) -> Self {
        match v {
            1 => Nna::Continue,
            2 => Nna::Off,
            3 => Nna::Fade,
            _ => Nna::Cut,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Instrument {
    pub name: String,
    pub nna: Nna,
    /// duplicate check type: 0 off, 1 note, 2 sample, 3 instrument
    pub dct: u8,
    /// duplicate check action: 0 cut, 1 off, 2 fade
    pub dca: u8,
    /// fade-out speed in 1/65536 units per half tick (OpenMPT scale: IT value << 5)
    pub fadeout: u32,
    pub pps: i8,
    pub ppc: u8,
    /// 0..128
    pub global_vol: u8,
    /// 0..64 if set
    pub pan: Option<u8>,
    /// random volume / pan variation (percent / pan units)
    pub vol_swing: u8,
    pub pan_swing: u8,
    /// (note 0..119, sample 1..) for each note 0..119
    pub keyboard: [(u8, u8); 120],
    pub vol_env: Envelope,
    pub pan_env: Envelope,
    pub pitch_env: Envelope,
    /// initial filter cutoff / resonance (0..127) if enabled
    pub cutoff: Option<u8>,
    pub resonance: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub name: String,
    /// 0..64
    pub global_vol: u8,
    /// 0..64
    pub volume: u8,
    /// 0..64 if set
    pub pan: Option<u8>,
    pub length: usize,
    pub looped: bool,
    pub pingpong: bool,
    pub loop_start: usize,
    pub loop_end: usize,
    pub sus_loop: bool,
    pub sus_pingpong: bool,
    pub sus_start: usize,
    pub sus_end: usize,
    pub c5speed: u32,
    /// auto vibrato
    pub vib_speed: u8,
    pub vib_depth: u8,
    pub vib_rate: u8,
    pub vib_type: u8,
    /// sample data, 16-bit range (8-bit data is scaled by 256). Right channel for stereo.
    pub data_l: Vec<i16>,
    pub data_r: Option<Vec<i16>>,
}

/// Source format of a module played by the IT engine. S3M (and XM) files are
/// converted to the IT data model on load; the player handles their differences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    It,
    S3m,
}

#[derive(Clone, Debug)]
pub struct ItModule {
    pub format: Format,
    pub title: String,
    pub stereo: bool,
    pub instrument_mode: bool,
    pub linear_slides: bool,
    pub old_effects: bool,
    pub compat_gxx: bool,
    /// 0..128
    pub global_vol: u8,
    /// 0..128 ("mix volume" / sample preamp)
    pub mix_vol: u8,
    pub speed: u8,
    pub tempo: u8,
    /// pan separation 0..128
    pub separation: u8,
    /// 0..64, 100 = surround; bit 7 = channel disabled
    pub chn_pan: [u8; MAX_CHANNELS],
    /// 0..64
    pub chn_vol: [u8; MAX_CHANNELS],
    pub orders: Vec<u8>,
    pub instruments: Vec<Instrument>,
    pub samples: Vec<Sample>,
    pub patterns: Vec<Pattern>,
    /// highest channel used in any pattern + 1
    pub num_channels: usize,
    pub cwtv: u16,
    pub cmwt: u16,
    /// extended filter range (OpenMPT extension, header flag 0x1000)
    pub ext_filter_range: bool,
    /// MIDI macros: SF0..SFF (parametered, used by Z00..Z7F) and Z80..ZFF (fixed)
    pub midi_sfx: Vec<String>,
    pub midi_zxx: Vec<String>,

    // ---- S3M ----
    /// volume slides also act on the first tick (ST3.00 / header flag)
    pub fast_vol_slides: bool,
    /// ST2-style vibrato (twice as deep)
    pub st2_vibrato: bool,
    /// clamp periods to the Amiga range
    pub amiga_limits: bool,
    /// channel panning 0..256 when finer than `chn_pan` (S3M)
    pub chn_pan256: Option<Vec<i32>>,
    /// channels switched off in the file: not even their effects run (S3M)
    pub chn_disabled: Vec<bool>,
    /// Scream Tracker's own quirks (shared effect memory ...); off for S3M files
    /// saved by other trackers
    pub st3_quirks: bool,
    /// ST3 does not run the volume slide of K/L on the first tick
    pub st3_no_combined_first_tick: bool,
    /// saved by ModPlug Tracker / old OpenMPT: libopenmpt plays it with the legacy
    /// ("Original") mix levels, which attenuate by channel count
    pub legacy_mix: bool,
    /// output gain relative to libopenmpt's compatible mix levels for OpenMPT's
    /// "1.17RC3" mix mode (2.0); 1.0 otherwise
    pub mix_mode_gain: f32,
}

/// OpenMPT's extended song properties after "STPM": returns (mix levels, sample pre-amp)
fn mpt_song_properties(d: &[u8]) -> (Option<u32>, Option<u32>) {
    let Some(start) = d.windows(4).position(|w| w == b"STPM") else { return (None, None) };
    let mut p = start + 4;
    let (mut mix, mut preamp) = (None, None);
    while p + 6 <= d.len() {
        let code = &d[p..p + 4];
        let size = u16le(d, p + 4) as usize;
        if !code.iter().all(|c| c.is_ascii_graphic()) || p + 6 + size > d.len() {
            break;
        }
        let v = (0..size.min(4)).fold(0u32, |a, i| a | (d[p + 6 + i] as u32) << (8 * i));
        match code {
            b".MMP" => mix = Some(v),
            b".APS" => preamp = Some(v),
            _ => {}
        }
        p += 6 + size;
    }
    (mix, preamp)
}

/// libopenmpt's `interpretModPlugMade` heuristics (Load_it.cpp) for IT files
fn detect_modplug_made(d: &[u8], ins_ptr: &[usize], smp_ptr: &[usize], pat_ptr: &[usize]) -> bool {
    let cwtv = u16le(d, 0x28);
    let cmwt = u16le(d, 0x2A);
    let reserved = &d[0x3C..0x40];
    let res0 = reserved == [0, 0, 0, 0];
    let hl0 = byte(d, 0x1E) == 0 && byte(d, 0x1F) == 0;
    let ordnum = u16le(d, 0x20);
    let (sep, pwd) = (byte(d, 0x34), byte(d, 0x35));
    let _ = ins_ptr;
    if cwtv & 0xF000 == 0x5000 {
        return reserved == b"OMPT";
    }
    if cmwt == 0x888 || cwtv == 0x888 {
        return true;
    }
    if (cwtv == 0x214 && cmwt == 0x202 && res0)
        || (cwtv == 0x300 && cmwt == 0x300 && res0 && ordnum == 256 && sep == 128 && pwd == 0)
        || (cwtv == 0x217 && cmwt == 0x200 && res0)
        || (cwtv == 0x214 && cmwt == 0x200 && hl0 && res0)
    {
        return true;
    }
    if cwtv == 0x202 && cmwt == 0x200 && hl0 && res0 {
        let p0 = pat_ptr.iter().copied().find(|&x| x != 0);
        let s0 = smp_ptr.first().copied();
        if let (Some(p), Some(s)) = (p0, s0) {
            return p < s;
        }
    }
    false
}

impl ItModule {
    /// Loads an IT or S3M module.
    pub fn load_any(d: &[u8]) -> Result<ItModule, ItError> {
        if crate::it::s3m::detect(d) {
            crate::it::s3m::load(d)
        } else {
            ItModule::load(d)
        }
    }
}

#[derive(Debug)]
pub enum ItError {
    NotIt,
    Truncated(&'static str),
}

impl fmt::Display for ItError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItError::NotIt => write!(f, "not an Impulse Tracker module"),
            ItError::Truncated(what) => write!(f, "truncated IT file ({what})"),
        }
    }
}
impl std::error::Error for ItError {}

// ---------------------------------------------------------------------------
// little helpers

pub(crate) fn u16le(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).unwrap_or(0)
}
pub(crate) fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).unwrap_or(0)
}
pub(crate) fn byte(b: &[u8], o: usize) -> u8 {
    b.get(o).copied().unwrap_or(0)
}
pub(crate) fn text(b: &[u8], o: usize, len: usize) -> String {
    let s = b.get(o..(o + len).min(b.len())).unwrap_or(&[]);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    s[..end].iter().map(|&c| if (32..127).contains(&c) { c as char } else { ' ' }).collect::<String>()
}

pub fn detect(data: &[u8]) -> bool {
    data.len() >= 192 && &data[0..4] == b"IMPM"
}

impl ItModule {
    pub fn load(d: &[u8]) -> Result<ItModule, ItError> {
        if !detect(d) {
            return Err(ItError::NotIt);
        }
        let ordnum = u16le(d, 0x20) as usize;
        let insnum = u16le(d, 0x22) as usize;
        let smpnum = u16le(d, 0x24) as usize;
        let patnum = u16le(d, 0x26) as usize;
        let cwtv = u16le(d, 0x28);
        let cmwt = u16le(d, 0x2A);
        let flags = u16le(d, 0x2C);

        let mut chn_pan = [32u8; MAX_CHANNELS];
        let mut chn_vol = [64u8; MAX_CHANNELS];
        for i in 0..MAX_CHANNELS {
            chn_pan[i] = byte(d, 0x40 + i);
            chn_vol[i] = byte(d, 0x80 + i).min(64);
        }

        let mut m = ItModule {
            title: text(d, 4, 26),
            stereo: flags & 0x01 != 0,
            instrument_mode: flags & 0x04 != 0,
            linear_slides: flags & 0x08 != 0,
            old_effects: flags & 0x10 != 0,
            compat_gxx: flags & 0x20 != 0,
            global_vol: byte(d, 0x30).min(128),
            mix_vol: byte(d, 0x31).min(128),
            speed: byte(d, 0x32),
            tempo: byte(d, 0x33),
            separation: byte(d, 0x34).min(128),
            chn_pan,
            chn_vol,
            orders: Vec::new(),
            instruments: Vec::new(),
            samples: Vec::new(),
            patterns: Vec::new(),
            num_channels: 0,
            cwtv,
            cmwt,
            ext_filter_range: flags & 0x1000 != 0,
            midi_sfx: vec![String::new(); 16],
            midi_zxx: vec![String::new(); 128],
            format: Format::It,
            fast_vol_slides: false,
            st2_vibrato: false,
            amiga_limits: false,
            chn_pan256: None,
            chn_disabled: vec![false; MAX_CHANNELS],
            st3_quirks: false,
            st3_no_combined_first_tick: false,
            legacy_mix: false,
            mix_mode_gain: 1.0,
        };
        // default IT MIDI configuration: SF0 = cutoff, Z80..Z8F = resonance 0..120
        m.midi_sfx[0] = "F0F000z".into();
        for i in 0..16 {
            m.midi_zxx[i] = format!("F0F001{:02X}", i * 8);
        }
        if m.speed == 0 {
            m.speed = 6;
        }
        if m.tempo < 31 {
            m.tempo = 125;
        }

        let mut p = 0xC0;
        m.orders = d.get(p..p + ordnum).ok_or(ItError::Truncated("orders"))?.to_vec();
        p += ordnum;
        let ins_ptr: Vec<usize> = (0..insnum).map(|i| u32le(d, p + i * 4) as usize).collect();
        p += insnum * 4;
        let smp_ptr: Vec<usize> = (0..smpnum).map(|i| u32le(d, p + i * 4) as usize).collect();
        p += smpnum * 4;
        let pat_ptr: Vec<usize> = (0..patnum).map(|i| u32le(d, p + i * 4) as usize).collect();
        let modplug_made = detect_modplug_made(d, &ins_ptr, &smp_ptr, &pat_ptr);
        // an explicit mix mode / pre-amp in OpenMPT's song extensions wins
        let (mix_mode, preamp) = mpt_song_properties(d);
        let mode = mix_mode.unwrap_or(if modplug_made { 0 } else { 4 });
        m.legacy_mix = matches!(mode, 0..=2);
        m.mix_mode_gain = if mode == 3 { 2.0 } else { 1.0 };
        if let Some(pa) = preamp {
            m.mix_vol = pa.clamp(1, 128) as u8;
        }
        p += patnum * 4;

        // ---- edit history / embedded MIDI configuration ----
        let special = u16le(d, 0x2E);
        let min_ptr = ins_ptr.iter().chain(&smp_ptr).chain(&pat_ptr).copied().filter(|&x| x != 0).min().unwrap_or(d.len());
        if special & 0x02 != 0 {
            let n = u16le(d, p) as usize;
            if p + 2 + n * 8 <= min_ptr.min(d.len()) {
                p += 2 + n * 8;
            }
        }
        if (flags & 0x80 != 0 || special & 0x08 != 0) && p + 4896 <= d.len() {
            let macro_at = |o: usize| -> String {
                let raw = &d[o..o + 32];
                let end = raw.iter().position(|&b| b == 0).unwrap_or(32);
                raw[..end].iter().filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect()
            };
            let base = p + 9 * 32; // skip the 9 global macros
            for i in 0..16 {
                m.midi_sfx[i] = macro_at(base + i * 32);
            }
            for i in 0..128 {
                m.midi_zxx[i] = macro_at(base + 16 * 32 + i * 32);
            }
        }
        // files from IT versions before filters existed: Zxx does nothing
        if cwtv < 0x214 && cmwt < 0x214 {
            m.midi_sfx.iter_mut().chain(m.midi_zxx.iter_mut()).for_each(|s| s.clear());
        }

        // ---- instruments ----
        for &ip in &ins_ptr {
            let ins = if cmwt < 0x200 { load_old_instrument(d, ip) } else { load_instrument(d, ip) };
            m.instruments.push(ins);
        }

        // ---- samples ----
        for &sp in &smp_ptr {
            m.samples.push(load_sample(d, sp, cwtv));
        }

        // ---- patterns ----
        let mut max_ch = 0usize;
        for &pp in &pat_ptr {
            let (pat, used) = if pp == 0 { (Pattern::empty(64), 0) } else { load_pattern(d, pp) };
            max_ch = max_ch.max(used);
            m.patterns.push(pat);
        }
        m.num_channels = max_ch.clamp(1, MAX_CHANNELS);
        Ok(m)
    }
}

fn load_envelope(d: &[u8], o: usize, is_vol: bool) -> Envelope {
    let flags = byte(d, o);
    let num = (byte(d, o + 1) as usize).min(25);
    let mut env = Envelope {
        enabled: flags & 1 != 0,
        looped: flags & 2 != 0,
        sustain: flags & 4 != 0,
        carry: flags & 8 != 0,
        filter: flags & 0x80 != 0,
        loop_start: byte(d, o + 2) as usize,
        loop_end: byte(d, o + 3) as usize,
        sus_start: byte(d, o + 4) as usize,
        sus_end: byte(d, o + 5) as usize,
        nodes: Vec::with_capacity(num),
    };
    let mut last_tick = 0u16;
    for i in 0..num {
        let no = o + 6 + i * 3;
        let mut value = byte(d, no) as i8;
        let tick = u16le(d, no + 1);
        if is_vol {
            value = value.clamp(0, 64);
        } else {
            value = value.clamp(-32, 32);
        }
        // ticks must not go backwards
        let tick = if i > 0 && tick < last_tick { last_tick } else { tick };
        last_tick = tick;
        env.nodes.push(EnvNode { tick, value });
    }
    if env.nodes.is_empty() {
        env.enabled = false;
    }
    let max = env.nodes.len().saturating_sub(1);
    env.loop_end = env.loop_end.min(max);
    env.loop_start = env.loop_start.min(env.loop_end);
    env.sus_end = env.sus_end.min(max);
    env.sus_start = env.sus_start.min(env.sus_end);
    env
}

fn read_keyboard(d: &[u8], o: usize) -> [(u8, u8); 120] {
    let mut kb = [(0u8, 0u8); 120];
    for (i, k) in kb.iter_mut().enumerate() {
        let note = byte(d, o + i * 2);
        let smp = byte(d, o + i * 2 + 1);
        *k = (if note < 120 { note } else { i as u8 }, smp);
    }
    kb
}

fn load_instrument(d: &[u8], o: usize) -> Instrument {
    let ifc = byte(d, o + 0x3A);
    let ifr = byte(d, o + 0x3B);
    let dfp = byte(d, o + 0x19);
    // stored in OpenMPT's scale: subtracted (x2) from a 65536 fade volume every tick
    let fadeout = (u16le(d, o + 0x14) as u32).min(1024) << 5;
    Instrument {
        name: text(d, o + 0x20, 26),
        nna: Nna::from(byte(d, o + 0x11)),
        dct: byte(d, o + 0x12).min(3),
        dca: byte(d, o + 0x13).min(2),
        fadeout,
        pps: byte(d, o + 0x16) as i8,
        ppc: byte(d, o + 0x17).min(119),
        global_vol: byte(d, o + 0x18).min(128),
        pan: if dfp & 0x80 == 0 { Some((dfp & 0x7F).min(64)) } else { None },
        vol_swing: byte(d, o + 0x1A).min(100),
        pan_swing: byte(d, o + 0x1B).min(64),
        keyboard: read_keyboard(d, o + 0x40),
        vol_env: load_envelope(d, o + 0x130, true),
        pan_env: load_envelope(d, o + 0x182, false),
        pitch_env: load_envelope(d, o + 0x1D4, false),
        cutoff: if ifc & 0x80 != 0 { Some(ifc & 0x7F) } else { None },
        resonance: if ifr & 0x80 != 0 { Some(ifr & 0x7F) } else { None },
    }
}

/// Instruments of IT files saved by versions < 2.00
fn load_old_instrument(d: &[u8], o: usize) -> Instrument {
    let flags = byte(d, o + 0x11);
    let mut vol_env = Envelope {
        enabled: flags & 1 != 0,
        looped: flags & 2 != 0,
        sustain: flags & 4 != 0,
        loop_start: byte(d, o + 0x12) as usize,
        loop_end: byte(d, o + 0x13) as usize,
        sus_start: byte(d, o + 0x14) as usize,
        sus_end: byte(d, o + 0x15) as usize,
        ..Default::default()
    };
    // nodes: 25 x (tick, value) after the 200-byte precomputed envelope, 0xFF tick terminates
    for i in 0..25 {
        let tick = byte(d, o + 0x1F8 + i * 2);
        let value = byte(d, o + 0x1F9 + i * 2);
        if tick == 0xFF {
            break;
        }
        vol_env.nodes.push(EnvNode { tick: tick as u16, value: (value as i8).clamp(0, 64) });
    }
    if vol_env.nodes.is_empty() {
        vol_env.enabled = false;
    }
    let max = vol_env.nodes.len().saturating_sub(1);
    vol_env.loop_end = vol_env.loop_end.min(max);
    vol_env.loop_start = vol_env.loop_start.min(vol_env.loop_end);
    vol_env.sus_end = vol_env.sus_end.min(max);
    vol_env.sus_start = vol_env.sus_start.min(vol_env.sus_end);

    let nna = byte(d, o + 0x1A);
    Instrument {
        name: text(d, o + 0x20, 26),
        nna: Nna::from(nna),
        dct: byte(d, o + 0x1B).min(3),
        dca: 0,
        fadeout: (u16le(d, o + 0x18) as u32).min(256) << 6,
        pps: 0,
        ppc: 60,
        global_vol: 128,
        pan: None,
        vol_swing: 0,
        pan_swing: 0,
        keyboard: read_keyboard(d, o + 0x40),
        vol_env,
        pan_env: Envelope::default(),
        pitch_env: Envelope::default(),
        cutoff: None,
        resonance: None,
    }
}

fn load_sample(d: &[u8], o: usize, cwtv: u16) -> Sample {
    let flags = byte(d, o + 0x12);
    let cvt = byte(d, o + 0x2E);
    let dfp = byte(d, o + 0x2F);
    let length = u32le(d, o + 0x30) as usize;
    let mut s = Sample {
        name: text(d, o + 0x14, 26),
        global_vol: byte(d, o + 0x11).min(64),
        volume: byte(d, o + 0x13).min(64),
        pan: if dfp & 0x80 != 0 { Some((dfp & 0x7F).min(64)) } else { None },
        length,
        looped: flags & 0x10 != 0,
        pingpong: flags & 0x40 != 0,
        loop_start: u32le(d, o + 0x34) as usize,
        loop_end: u32le(d, o + 0x38) as usize,
        sus_loop: flags & 0x20 != 0,
        sus_pingpong: flags & 0x80 != 0,
        sus_start: u32le(d, o + 0x40) as usize,
        sus_end: u32le(d, o + 0x44) as usize,
        c5speed: u32le(d, o + 0x3C),
        vib_speed: byte(d, o + 0x4C).min(64),
        vib_depth: byte(d, o + 0x4D) & 0x7F,
        vib_rate: byte(d, o + 0x4E),
        vib_type: byte(d, o + 0x4F) & 3,
        data_l: Vec::new(),
        data_r: None,
    };
    if s.c5speed == 0 {
        s.c5speed = 8363;
    }
    if &d.get(o..o + 4).unwrap_or(&[]) != b"IMPS" || flags & 1 == 0 || length == 0 {
        s.length = 0;
        s.looped = false;
        s.sus_loop = false;
        return s;
    }
    // sanitize loops
    if s.loop_end > s.length {
        s.loop_end = s.length;
    }
    if s.loop_start >= s.loop_end {
        s.looped = false;
    }
    if s.sus_end > s.length {
        s.sus_end = s.length;
    }
    if s.sus_start >= s.sus_end {
        s.sus_loop = false;
    }

    let is16 = flags & 0x02 != 0;
    let stereo = flags & 0x04 != 0 && cwtv >= 0x214;
    let compressed = flags & 0x08 != 0;
    let signed = cvt & 0x01 != 0;
    let delta = cvt & 0x04 != 0;
    let ptr = u32le(d, o + 0x48) as usize;
    let src = d.get(ptr..).unwrap_or(&[]);

    let nch = if stereo { 2 } else { 1 };
    // never trust the header length further than the file can back it up
    let max_len = if compressed { src.len() * 8 / nch } else { src.len() / (nch * if is16 { 2 } else { 1 }) };
    let length = length.min(max_len).min(64 << 20);
    s.length = length;
    if length == 0 {
        s.looped = false;
        s.sus_loop = false;
        return s;
    }
    s.loop_end = s.loop_end.min(length);
    s.sus_end = s.sus_end.min(length);
    if s.loop_start >= s.loop_end {
        s.looped = false;
    }
    if s.sus_start >= s.sus_end {
        s.sus_loop = false;
    }
    let mut chans: Vec<Vec<i16>> = Vec::with_capacity(nch);
    if compressed {
        let it215 = delta;
        let mut pos = 0usize;
        for _ in 0..nch {
            let (v, used) = decompress(src.get(pos..).unwrap_or(&[]), length, is16, it215);
            pos += used;
            chans.push(v);
        }
    } else {
        let bps = if is16 { 2 } else { 1 };
        for c in 0..nch {
            let base = c * length * bps;
            let mut v = Vec::with_capacity(length);
            let mut acc: i32 = 0;
            for i in 0..length {
                let raw: i32 = if is16 {
                    let lo = *src.get(base + i * 2).unwrap_or(&0) as u16;
                    let hi = *src.get(base + i * 2 + 1).unwrap_or(&0) as u16;
                    let w = if cvt & 0x02 != 0 { (lo << 8) | hi } else { (hi << 8) | lo };
                    if signed { w as i16 as i32 } else { w as i32 - 32768 }
                } else {
                    let b = *src.get(base + i).unwrap_or(&0);
                    (if signed { b as i8 as i32 } else { b as i32 - 128 }) << 8
                };
                let val = if delta {
                    acc = acc.wrapping_add(raw);
                    if is16 { acc as i16 as i32 } else { ((acc >> 8) as i8 as i32) << 8 }
                } else {
                    raw
                };
                v.push(val as i16);
            }
            chans.push(v);
        }
    }
    s.data_l = chans.remove(0);
    s.data_r = chans.pop();
    s
}

/// IT 2.14 / 2.15 sample decompression. Returns (samples, bytes consumed).
fn decompress(src: &[u8], length: usize, is16: bool, it215: bool) -> (Vec<i16>, usize) {
    let mut out: Vec<i16> = Vec::with_capacity(length);
    let mut pos = 0usize;
    let (def_width, fetch_a, lower_b, upper_b, block) = if is16 { (17i32, 4i32, -8i32, 7i32, 0x4000usize) } else { (9, 3, -4, 3, 0x8000) };

    while out.len() < length && pos + 2 <= src.len() {
        let size = u16::from_le_bytes([src[pos], src[pos + 1]]) as usize;
        pos += 2;
        if size == 0 {
            continue;
        }
        let chunk = &src[pos..(pos + size).min(src.len())];
        pos += size;

        let mut br = BitReader { data: chunk, pos: 0, bit: 0 };
        let mut width = def_width;
        let mut mem1: i32 = 0;
        let mut mem2: i32 = 0;
        let mut left = (length - out.len()).min(block);

        'block: while left > 0 {
            if width > def_width {
                break;
            }
            let Some(v) = br.read(width as u32) else { break 'block };
            let mut v = v as i32;
            let top = 1i32 << (width - 1);
            let mut write = |v: i32, top: i32, out: &mut Vec<i16>| {
                let mut v = v;
                if top != 0 && v & top != 0 {
                    v -= top << 1;
                }
                mem1 = mem1.wrapping_add(v);
                mem2 = mem2.wrapping_add(mem1);
                let s = if it215 { mem2 } else { mem1 };
                out.push(if is16 { s as i16 } else { ((s as i8) as i16) << 8 });
            };
            if width <= 6 {
                if v == top {
                    let Some(nw) = br.read(fetch_a as u32) else { break 'block };
                    let mut nw = nw as i32 + 1;
                    if nw >= width {
                        nw += 1;
                    }
                    width = nw;
                } else {
                    write(v, top, &mut out);
                    left -= 1;
                }
            } else if width < def_width {
                if v >= top + lower_b && v <= top + upper_b {
                    let mut nw = v - (top + lower_b) + 1;
                    if nw >= width {
                        nw += 1;
                    }
                    width = nw;
                } else {
                    write(v, top, &mut out);
                    left -= 1;
                }
            } else {
                if v & top != 0 {
                    width = (v & !top) + 1;
                } else {
                    v &= !top;
                    write(v, 0, &mut out);
                    left -= 1;
                }
            }
        }
    }
    out.resize(length, 0);
    (out, pos)
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl BitReader<'_> {
    /// reads `n` bits, LSB first
    fn read(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..n {
            let b = *self.data.get(self.pos)?;
            if (b >> self.bit) & 1 != 0 {
                v |= 1 << i;
            }
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Some(v)
    }
}

fn load_pattern(d: &[u8], o: usize) -> (Pattern, usize) {
    let len = u16le(d, o) as usize;
    let rows = (u16le(d, o + 2) as usize).clamp(1, 1024);
    let mut pat = Pattern::empty(rows);
    let data = d.get(o + 8..(o + 8 + len).min(d.len())).unwrap_or(&[]);

    let mut last_mask = [0u8; MAX_CHANNELS];
    let mut last = [Cell { vol: VOL_NONE, ..Default::default() }; MAX_CHANNELS];
    let mut p = 0usize;
    let mut row = 0usize;
    let mut used = 0usize;
    let next = |p: &mut usize| -> u8 {
        let v = data.get(*p).copied().unwrap_or(0);
        *p += 1;
        v
    };

    while row < rows && p < data.len() {
        let cv = next(&mut p);
        if cv == 0 {
            row += 1;
            continue;
        }
        let ch = ((cv & 0x7F).wrapping_sub(1) & 63) as usize;
        let mask = if cv & 0x80 != 0 {
            let m = next(&mut p);
            last_mask[ch] = m;
            m
        } else {
            last_mask[ch]
        };
        let mut c = Cell { vol: VOL_NONE, ..Default::default() };
        if mask & 1 != 0 {
            let n = next(&mut p);
            last[ch].note = n;
        }
        if mask & 2 != 0 {
            last[ch].ins = next(&mut p);
        }
        if mask & 4 != 0 {
            last[ch].vol = next(&mut p);
        }
        if mask & 8 != 0 {
            last[ch].cmd = next(&mut p);
            last[ch].param = next(&mut p);
        }
        if mask & (1 | 16) != 0 {
            c.note = convert_note(last[ch].note);
        }
        if mask & (2 | 32) != 0 {
            c.ins = last[ch].ins;
        }
        if mask & (4 | 64) != 0 {
            c.vol = last[ch].vol;
        }
        if mask & (8 | 128) != 0 {
            c.cmd = if last[ch].cmd <= 26 { last[ch].cmd } else { 0 };
            c.param = last[ch].param;
        }
        let has_data = c.note != 0 || c.ins != 0 || c.vol != VOL_NONE || c.cmd != 0;
        if has_data {
            used = used.max(ch + 1);
        }
        pat.cells[row * MAX_CHANNELS + ch] = c;
    }
    (pat, used)
}

/// IT note byte -> internal note
fn convert_note(n: u8) -> u8 {
    match n {
        0..=119 => n + 1,
        255 => NOTE_OFF,
        254 => NOTE_CUT,
        _ => NOTE_FADE,
    }
}
