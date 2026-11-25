//! Module data structures and .MOD loading.
//! Port of pt2_structs.h, createEmptyMod() (pt2_replayer.c), pt2_module_loader.c,
//! modloaders/pt2_load_mod31.c and modloaders/pt2_load_mod15.c.

use crate::consts::*;
use crate::paula::{SmpPtr, PAULA_VOICES};
use std::fmt;

/// Default max sample length (65534 bytes). pt2-clone can also use 131070.
pub const DEFAULT_MAX_SAMPLE_LENGTH: usize = 65534;

/// Size of the zero-filled "null sample" area (same as `nullSample[0xFFFF*2]` in C).
pub const NULL_SAMPLE_LEN: usize = 0xFFFF * 2;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Note {
    pub param: u8,
    pub sample: u8,
    pub command: u8,
    pub period: u16,
}

#[derive(Clone, Default, Debug)]
pub struct Sample {
    pub name: [u8; 22],
    pub volume: i8,
    pub fine_tune: u8,
    /// Byte offset of this sample inside `Module::sample_data`
    pub offset: i32,
    pub length: i32,
    pub loop_start: i32,
    pub loop_length: i32,
}

impl Sample {
    pub fn name_str(&self) -> String {
        bytes_to_string(&self.name)
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Channel {
    pub n_start: SmpPtr,
    pub n_wavestart: SmpPtr,
    pub n_loopstart: SmpPtr,
    pub n_volume: i8,
    pub n_dmabit: u16,
    pub n_toneportdirec: i8,
    pub n_pattpos: i8,
    pub n_loopcount: i8,
    pub n_wavecontrol: u8,
    pub n_glissfunk: u8,
    pub n_sampleoffset: u8,
    pub n_toneportspeed: u8,
    pub n_vibratocmd: u8,
    pub n_tremolocmd: u8,
    pub n_finetune: u8,
    pub n_funkoffset: u8,
    pub n_samplenum: u8,
    pub n_vibratopos: u8,
    pub n_tremolopos: u8,
    pub n_period: i16,
    pub n_note: i16,
    pub n_wantedperiod: i16,
    pub n_cmd: u16,
    pub n_length: u16,
    pub n_replen: u16,
    pub n_chanindex: usize,
}

pub type Pattern = [Note; MOD_ROWS * PAULA_VOICES];

#[derive(Clone)]
pub struct Module {
    pub name: [u8; 20],
    pub pattern_table: [u16; 128],
    pub song_length: u16,
    /// used for STK/UST modules
    pub initial_tempo: u16,
    pub samples: [Sample; MOD_SAMPLES],
    pub channels: [Channel; PAULA_VOICES],
    pub patterns: Vec<Pattern>,

    /// One big buffer: 33 sample slots of `max_sample_length` bytes, followed by
    /// the null-sample area.
    pub sample_data: Vec<i8>,
    pub max_sample_length: usize,

    pub tick: i32,
    pub speed: i32,
    pub row: i8,

    pub curr_row: i8,
    pub curr_speed: i32,
    pub curr_bpm: i32,
    pub curr_pos: u16,
    pub curr_pattern: u16,

    pub rows_counter: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Io(String),
    NotAMod,
    Unsupported,
    Encrypted,
    PackedPowerPacker,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "file I/O error: {e}"),
            LoadError::NotAMod => write!(f, "not a MOD file"),
            LoadError::Unsupported => write!(f, "unsupported MOD"),
            LoadError::Encrypted => write!(f, "encrypted MOD (PX20)"),
            LoadError::PackedPowerPacker => write!(f, "PowerPacker (PP20) packed MODs are not supported yet"),
        }
    }
}

impl std::error::Error for LoadError {}

fn bytes_to_string(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

/// Bounds-safe byte reader (reads past EOF return 0, the C code would read garbage)
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    fn u8(&mut self) -> u8 {
        let v = self.buf.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        v
    }
    fn u16be(&mut self) -> u16 {
        let hi = self.u8() as u16;
        let lo = self.u8() as u16;
        (hi << 8) | lo
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        for b in out.iter_mut() {
            *b = self.u8();
        }
        out
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
    fn copy_to(&mut self, dst: &mut [i8]) {
        for d in dst.iter_mut() {
            *d = self.u8() as i8;
        }
    }
}

impl Module {
    /// createEmptyMod()
    pub fn new_empty(max_sample_length: usize) -> Self {
        let alloc_len = (MOD_SAMPLES + 2) * max_sample_length + NULL_SAMPLE_LEN;

        let mut samples: [Sample; MOD_SAMPLES] = Default::default();
        for (i, s) in samples.iter_mut().enumerate() {
            s.loop_length = 2;
            s.offset = (max_sample_length * i) as i32;
        }

        let mut m = Module {
            name: [0; 20],
            pattern_table: [0; 128],
            song_length: 1,
            initial_tempo: 0,
            samples,
            channels: [Channel::default(); PAULA_VOICES],
            patterns: vec![[Note::default(); MOD_ROWS * PAULA_VOICES]; MAX_PATTERNS],
            sample_data: vec![0i8; alloc_len],
            max_sample_length,
            tick: 0,
            speed: 0,
            row: 0,
            curr_row: 0,
            curr_speed: 0,
            curr_bpm: 0,
            curr_pos: 0,
            curr_pattern: 0,
            rows_counter: 0,
        };
        m.initialize_channels();
        m
    }

    /// Offset of the null-sample area in `sample_data`
    pub fn null_ptr(&self) -> usize {
        (MOD_SAMPLES + 2) * self.max_sample_length
    }

    pub fn name_str(&self) -> String {
        bytes_to_string(&self.name)
    }

    pub fn initialize_channels(&mut self) {
        self.channels = [Channel::default(); PAULA_VOICES];
        for (i, ch) in self.channels.iter_mut().enumerate() {
            ch.n_chanindex = i;
            ch.n_dmabit = 1 << i;
        }
    }

    pub fn load_file(path: &std::path::Path) -> Result<Module, LoadError> {
        let data = std::fs::read(path).map_err(|e| LoadError::Io(e.to_string()))?;
        Module::load(&data, DEFAULT_MAX_SAMPLE_LENGTH)
    }

    /// modLoad(): detect format, load, then sanitize.
    pub fn load(buf: &[u8], max_sample_length: usize) -> Result<Module, LoadError> {
        if buf.len() >= 4 {
            match &buf[0..4] {
                b"PX20" => return Err(LoadError::Encrypted),
                b"PP20" => return Err(LoadError::PackedPowerPacker),
                _ => {}
            }
        }
        // TODO: XPK (SQSH) unpacking (modloaders/pt2_xpk_unpack.c)

        let mut m = if detect_mod31(buf) {
            load_mod31(buf, max_sample_length)?
        } else {
            load_mod15(buf, max_sample_length)?
        };

        // ---- sanitation ----
        for c in m.name.iter_mut() {
            if (*c < b' ' || *c > b'~') && *c != 0 {
                *c = b' ';
            }
        }
        fix_zeroes_in_string(&mut m.name);

        let max_len = max_sample_length as i32;
        for i in 0..MOD_SAMPLES {
            let s = &mut m.samples[i];
            for c in s.name.iter_mut() {
                if (*c < b' ' || *c > b'~') && *c != 0 {
                    *c = b' ';
                }
            }
            fix_zeroes_in_string(&mut s.name);

            if s.length > max_len {
                s.length = max_len;
            }
            if (s.volume as u8) > 64 {
                s.volume = 64;
            }
            if s.loop_length < 2 {
                s.loop_length = 2;
            }

            // disable incompatible loops
            if s.loop_start > max_len || s.loop_start + s.loop_length > max_len {
                s.loop_start = 0;
                s.loop_length = 2;
            }

            // some modules are broken like this, adjust sample length if possible
            if s.length > 0 && s.loop_length > 2 && s.loop_start + s.loop_length > s.length {
                let overflow = (s.loop_start + s.loop_length) - s.length;
                if s.length + overflow <= max_len {
                    s.length += overflow;
                } else {
                    s.loop_start = 0;
                    s.loop_length = 2;
                }
            }

            // clear first two bytes of non-looping samples to prevent beep
            if s.length >= 2 && s.loop_start + s.loop_length <= 2 {
                let off = s.offset as usize;
                m.sample_data[off] = 0;
                m.sample_data[off + 1] = 0;
            }
        }

        m.initialize_channels();
        Ok(m)
    }
}

/// Converts zeroes to spaces in a string, up until the last non-zero found
fn fix_zeroes_in_string(s: &mut [u8]) {
    let mut i = s.len() as i32 - 1;
    while i >= 0 {
        if s[i as usize] != 0 {
            break;
        }
        i -= 1;
    }
    if i > 0 {
        for c in s[..i as usize].iter_mut() {
            if *c == 0 {
                *c = b' ';
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 31-sample MOD loader
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mod31Format {
    Unknown,
    Pt,
    Flt,
    Ft2,
    Nt,
    Hmnt,
}

fn get_mod31_type(buf: &[u8]) -> (Mod31Format, u8) {
    if buf.len() < 1084 + 1024 {
        return (Mod31Format::Unknown, 0);
    }
    let id = &buf[1080..1084];
    let is = |s: &[u8; 4]| id == s;
    let digit = |c: u8| c.is_ascii_digit();

    if is(b"M.K.") || is(b"M!K!") || is(b"NSMS") || is(b"LARD") || is(b"PATT") {
        (Mod31Format::Pt, 4)
    } else if is(b"FLT4") {
        (Mod31Format::Flt, 4)
    } else if is(b"N.T.") {
        (Mod31Format::Nt, 4)
    } else if is(b"M&K!") || is(b"FEST") {
        (Mod31Format::Hmnt, 4)
    } else if digit(id[0]) && id[1] == b'C' && id[2] == b'H' && id[3] == b'N' {
        (Mod31Format::Ft2, id[0] - b'0')
    } else if digit(id[0]) && digit(id[1]) && id[1] == b'C' && id[2] == b'H' {
        // (this condition can never be true, same as in the C code)
        (Mod31Format::Ft2, (id[0] - b'0') * 10 + (id[1] - b'0'))
    } else {
        (Mod31Format::Unknown, 4)
    }
}

pub fn detect_mod31(buf: &[u8]) -> bool {
    get_mod31_type(buf).0 != Mod31Format::Unknown
}

fn load_mod31(buf: &[u8], max_sample_length: usize) -> Result<Module, LoadError> {
    let mut m = Module::new_empty(max_sample_length);
    let max_len = max_sample_length as i32;

    let (fmt, num_channels) = get_mod31_type(buf);
    if fmt == Mod31Format::Unknown || num_channels as usize > PAULA_VOICES {
        return Err(LoadError::Unsupported);
    }
    let num_channels = num_channels as usize;

    let mut p = Reader::new(buf);
    m.name = p.bytes::<20>();

    let mut real_lengths = [0i32; MOD_SAMPLES];
    for i in 0..MOD_SAMPLES {
        let s = &mut m.samples[i];
        s.name = p.bytes::<22>();
        real_lengths[i] = p.u16be() as i32 * 2;
        s.length = real_lengths[i].min(max_len);
        s.fine_tune = p.u8() & 0xF;
        s.volume = p.u8() as i8;
        s.loop_start = p.u16be() as i32 * 2;
        s.loop_length = p.u16be() as i32 * 2;

        // fix for poorly converted STK (< v2.5) -> PT/NT modules
        if s.loop_length > 2 && s.loop_start + s.loop_length > s.length && (s.loop_start / 2) + s.loop_length <= s.length {
            s.loop_start /= 2;
        }
    }

    m.song_length = p.u8() as u16;
    p.skip(1); // 127 in PT MODs, "restart pos" in PC MODs

    if fmt == Mod31Format::Pt && m.song_length == 129 {
        m.song_length = 127; // fixes a specific copy of beatwave.mod
    }
    if m.song_length == 0 || m.song_length > 129 {
        return Err(LoadError::NotAMod);
    }

    let mut num_patterns = 0usize;
    for i in 0..128 {
        m.pattern_table[i] = p.u8() as u16;
        num_patterns = num_patterns.max(m.pattern_table[i] as usize);
    }
    num_patterns += 1;
    if num_patterns > MAX_PATTERNS {
        return Err(LoadError::Unsupported);
    }

    p.skip(4); // magic ID

    for pat in m.patterns.iter_mut().take(num_patterns) {
        for row in 0..MOD_ROWS {
            for ch in 0..num_channels {
                let b = p.bytes::<4>();
                pat[row * PAULA_VOICES + ch] = Note {
                    period: (((b[0] & 0x0F) as u16) << 8) | b[1] as u16,
                    sample: ((b[0] & 0xF0) | (b[2] >> 4)) & 31,
                    command: b[2] & 0x0F,
                    param: b[3],
                };
            }
        }
    }

    if fmt != Mod31Format::Pt {
        for pat in m.patterns.iter_mut().take(num_patterns) {
            for note in pat.iter_mut() {
                if fmt == Mod31Format::Nt || fmt == Mod31Format::Hmnt {
                    if note.command == 0xD {
                        note.param = 0;
                    }
                    if note.command == 0xF && note.param == 0 {
                        note.command = 0;
                    }
                } else if fmt == Mod31Format::Flt {
                    if note.command == 0xE {
                        note.command = 0;
                        note.param = 0;
                    }
                    if note.command == 0xF && note.param > 0x1F {
                        note.param = 0x1F;
                    }
                }
                // remove E8x and EFx (PT-only effects)
                if note.command == 0xE && (note.param >> 4) == 0x8 {
                    note.command = 0;
                    note.param = 0;
                }
                if note.command == 0xE && (note.param >> 4) == 0xF {
                    note.command = 0;
                    note.param = 0;
                }
            }
        }
    }

    for i in 0..MOD_SAMPLES {
        let s = &m.samples[i];
        let off = s.offset as usize;
        let len = s.length as usize;
        p.copy_to(&mut m.sample_data[off..off + len]);
        if real_lengths[i] > max_len {
            p.skip((real_lengths[i] - max_len) as usize);
        }
    }

    m.initial_tempo = 125;
    Ok(m)
}

// ---------------------------------------------------------------------------
// 15-sample MOD loader (Ultimate SoundTracker etc.)
// ---------------------------------------------------------------------------

fn load_mod15(buf: &[u8], max_sample_length: usize) -> Result<Module, LoadError> {
    let mut m = Module::new_empty(max_sample_length);
    let max_len = max_sample_length as i32;

    let mut very_late_stk = false; // "DFJ SoundTracker III" and later
    let mut late_stk = false; // "TJC SoundTracker II" and later

    let mut p = Reader::new(buf);
    m.name = p.bytes::<20>();

    let mut real_lengths = [0i32; 15];
    for i in 0..15 {
        let s = &mut m.samples[i];
        s.name = p.bytes::<22>();
        real_lengths[i] = p.u16be() as i32 * 2;
        s.length = real_lengths[i].min(max_len);
        if s.length > 9999 {
            late_stk = true;
        }
        p.skip(1); // no finetune in STK/UST
        s.volume = p.u8() as i8;
        s.loop_start = p.u16be() as i32; // in bytes in UST
        s.loop_length = p.u16be() as i32 * 2;
    }

    m.song_length = p.u8() as u16;
    if m.song_length == 0 || m.song_length > 128 {
        return Err(LoadError::NotAMod);
    }

    let mut init_tempo = p.u8();
    if init_tempo > 220 {
        return Err(LoadError::NotAMod);
    }
    if init_tempo == 0 {
        init_tempo = 120; // 120 = 50Hz (125BPM)
    }
    if m.name_str() == "jjk55" {
        init_tempo = 120; // bogus STK tempo in jjk55.mod
    }

    m.initial_tempo = 125;
    if init_tempo != 120 {
        let cia_period: u16 = (240 - init_tempo as u16) * 122;
        let hz = CIA_PAL_CLK / (cia_period as u32 + 1) as f64;
        m.initial_tempo = ((hz * (125.0 / 50.0)) + 0.5) as u16;
    }

    let mut num_patterns = 0usize;
    for i in 0..128 {
        m.pattern_table[i] = p.u8() as u16;
        num_patterns = num_patterns.max(m.pattern_table[i] as usize);
    }
    num_patterns += 1;
    if num_patterns > MAX_PATTERNS {
        return Err(LoadError::Unsupported);
    }

    for pat in m.patterns.iter_mut().take(num_patterns) {
        for note in pat.iter_mut() {
            let b = p.bytes::<4>();
            note.period = (((b[0] & 0x0F) as u16) << 8) | b[1] as u16;
            note.sample = (b[0] & 0xF0) | (b[2] >> 4);
            note.command = b[2] & 0x0F;
            note.param = b[3];

            if note.sample > 31 {
                note.sample = 0;
            }
            if note.command == 0xC || note.command == 0xD || note.command == 0xE {
                late_stk = true;
            }
            if note.command == 0xF {
                late_stk = true;
                very_late_stk = true;
            }
        }
    }

    for pat in m.patterns.iter_mut().take(num_patterns) {
        for note in pat.iter_mut() {
            if !late_stk {
                // old SoundTracker 1.x commands
                if note.command == 1 {
                    note.command = 0; // arpeggio
                } else if note.command == 2 {
                    if note.param & 0xF0 != 0 {
                        note.command = 2;
                        note.param >>= 4;
                    } else if note.param & 0x0F != 0 {
                        note.command = 1;
                    }
                }
            } else if note.command == 0xD {
                if very_late_stk {
                    note.param = 0; // pattern break w/ no param
                } else {
                    note.command = 0xA; // volume slide
                }
            }

            if note.command == 0xF && note.param == 0 {
                note.command = 0;
            }
            if note.command == 0xE && (note.param >> 4) == 0x8 {
                note.command = 0;
                note.param = 0;
            }
            if note.command == 0xE && (note.param >> 4) == 0xF {
                note.command = 0;
                note.param = 0;
            }
        }
    }

    for i in 0..15 {
        let s = &mut m.samples[i];

        // for UST modules, only the loop area of a looped sample is played
        if s.loop_start > 0 && s.loop_length < s.length && s.loop_start < s.length {
            s.length -= s.loop_start;
            p.skip(s.loop_start as usize);
            s.loop_start = 0;
        }

        let mut bytes_to_skip = 0i32;
        if real_lengths[i] > max_len {
            bytes_to_skip = real_lengths[i] - max_len;
        }

        // don't load sample data after loop end
        let loop_end = s.loop_start + s.loop_length;
        if loop_end > 2 && s.length > loop_end {
            bytes_to_skip += s.length - loop_end;
            s.length = loop_end;
        }

        let off = s.offset as usize;
        let len = s.length as usize;
        p.copy_to(&mut m.sample_data[off..off + len]);
        if bytes_to_skip > 0 {
            p.skip(bytes_to_skip as usize);
        }
    }

    Ok(m)
}
