//! Common player interface used by the GUI, with two backends:
//! - `Pt2Player`: the bit-exact ProTracker engine (MOD files)
//! - `OpenMptPlayer`: libopenmpt (IT/XM/S3M/... and anything else it can load)

use crate::engine::{Engine, EngineConfig};
use crate::module::{Module, DEFAULT_MAX_SAMPLE_LENGTH};
use crate::paula::PAULA_VOICES;

/// Font char 0x02 is the small centered dot used by PT's "dotted" pattern mode.
pub const DOT: u8 = 0x02;

/// One pattern cell, preformatted as 8 font characters (fits PT's 8-char columns).
pub type Cell = [u8; 8];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellView {
    /// note, instrument, effect + parameter
    Effect,
    /// note, instrument, volume column
    Volume,
}

#[derive(Clone, Default, Debug)]
pub struct SampleInfo {
    pub name: String,
    pub volume: Option<u8>,
    pub finetune: Option<u8>,
    pub length: Option<u32>,
    pub loop_start: Option<u32>,
    pub loop_length: Option<u32>,
}

#[derive(Clone, Default, Debug)]
pub struct Status {
    pub playing: bool,
    pub order: i32,
    pub pattern: i32,
    pub row: i32,
    pub num_rows: i32,
    pub speed: i32,
    pub bpm: i32,
    pub seconds: f64,
}

/// Scope data for one channel: 40 samples (-128..127) and volume (0..64)
pub struct ScopeData {
    pub samples: [i8; 40],
    pub volume: u8,
}

pub trait Player: Send {
    /// true for the bit-exact ProTracker engine (pt2-clone port), false for libopenmpt
    fn is_protracker(&self) -> bool;
    /// "CIA"/"VBL" for ProTracker (as in PT's TIMING field), else the format ("IT", "XM" ...)
    fn timing_text(&self) -> String;
    fn format_name(&self) -> &str;
    fn title(&self) -> String;
    fn num_channels(&self) -> usize;
    fn num_orders(&self) -> i32;
    fn num_samples(&self) -> i32;
    fn sample_info(&self, index: i32) -> SampleInfo;
    fn file_size(&self) -> usize;
    /// true if the format has a volume column worth showing
    fn has_volume_column(&self) -> bool;

    fn render(&mut self, out: &mut [i16]);
    /// Interleaved stereo float output (-1..1). Engines that mix in float override this;
    /// the default converts from the 16-bit output.
    fn render_f32(&mut self, out: &mut [f32]) {
        let mut tmp = vec![0i16; out.len()];
        self.render(&mut tmp);
        for (o, &s) in out.iter_mut().zip(&tmp) {
            *o = s as f32 / 32768.0;
        }
    }
    fn status(&self) -> Status;
    fn play_from(&mut self, order: i32);
    fn stop(&mut self);
    fn set_muted(&mut self, ch: usize, muted: bool);
    fn is_muted(&self, ch: usize) -> bool;

    fn num_rows(&self, pattern: i32) -> i32;
    /// Pattern played at song position `order`, or None for skip / end markers
    fn order_pattern(&self, order: i32) -> Option<i32>;
    fn cell(&self, pattern: i32, row: i32, ch: usize, view: CellView) -> Cell;

    /// VU meter height 0..48. `pt2_style` backends report trigger events instead.
    fn vu(&self, ch: usize) -> VuState;
    /// Real waveform for the quadrascope, if the backend can provide it.
    fn scope(&self, ch: usize) -> Option<ScopeData>;
}

#[derive(Clone, Copy, Debug)]
pub enum VuState {
    /// PT-style: a new note was triggered (counter, height); the GUI sinks it per frame
    Trigger(u32, u8),
    /// Continuous level 0..48
    Level(u8),
}

fn put(cell: &mut Cell, at: usize, s: &[u8]) {
    for (i, &c) in s.iter().enumerate() {
        if at + i < 8 {
            cell[at + i] = c;
        }
    }
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

// ---------------------------------------------------------------------------
// ProTracker backend
// ---------------------------------------------------------------------------

pub struct Pt2Player {
    pub engine: Engine,
    file_size: usize,
}

impl Pt2Player {
    pub fn load(data: &[u8], output_rate: u32, stereo_separation: u8) -> Result<Self, String> {
        let song = Module::load(data, DEFAULT_MAX_SAMPLE_LENGTH).map_err(|e| e.to_string())?;
        let cfg = EngineConfig { output_rate, stereo_separation, ..EngineConfig::default() };
        Ok(Pt2Player { engine: Engine::new(song, cfg), file_size: data.len() })
    }
}

/// period -> note index (0 = no note, 1 = illegal, 2..37 = note), as in pt2_pattern_viewer.c
fn period_to_note(period: u16) -> usize {
    if period == 0 {
        return 0;
    }
    let mut beg: i32 = 0;
    let mut end: i32 = 35;
    while beg <= end {
        let mid = (beg + end) >> 1;
        let v = crate::tables::PERIOD_TABLE[mid as usize] as u16;
        if period == v {
            return 2 + mid as usize;
        }
        if period < v {
            beg = mid + 1;
        } else {
            end = mid - 1;
        }
    }
    1
}

const PT_NOTE_NAMES: [[u8; 2]; 12] = [*b"C-", *b"C#", *b"D-", *b"D#", *b"E-", *b"F-", *b"F#", *b"G-", *b"G#", *b"A-", *b"A#", *b"B-"];

impl Player for Pt2Player {
    fn is_protracker(&self) -> bool {
        true
    }
    fn timing_text(&self) -> String {
        match self.engine.rep.timing_mode {
            crate::consts::TimingMode::Cia => "CIA".into(),
            crate::consts::TimingMode::Vblank => "VBL".into(),
        }
    }
    fn format_name(&self) -> &str {
        "MOD"
    }
    fn title(&self) -> String {
        self.engine.song.name_str()
    }
    fn num_channels(&self) -> usize {
        PAULA_VOICES
    }
    fn num_orders(&self) -> i32 {
        self.engine.song.song_length as i32
    }
    fn num_samples(&self) -> i32 {
        31
    }
    fn sample_info(&self, index: i32) -> SampleInfo {
        let s = &self.engine.song.samples[(index.clamp(0, 30)) as usize];
        SampleInfo {
            name: s.name_str(),
            volume: Some(s.volume as u8),
            finetune: Some(s.fine_tune & 0xF),
            length: Some(s.length as u32),
            loop_start: Some(s.loop_start as u32),
            loop_length: Some(s.loop_length as u32),
        }
    }
    fn file_size(&self) -> usize {
        self.file_size
    }
    fn has_volume_column(&self) -> bool {
        false
    }

    fn render(&mut self, out: &mut [i16]) {
        self.engine.render(out);
    }

    fn status(&self) -> Status {
        let e = &self.engine;
        Status {
            playing: e.rep.song_playing,
            order: e.song.curr_pos as i32,
            pattern: e.song.curr_pattern as i32,
            row: e.song.curr_row as i32,
            num_rows: 64,
            speed: e.song.curr_speed,
            bpm: e.song.curr_bpm,
            seconds: e.rep.playback_seconds,
        }
    }

    fn play_from(&mut self, order: i32) {
        let e = &mut self.engine;
        e.rep.play_mode = crate::replayer::PlayMode::Normal;
        let pos = order.clamp(0, e.song.song_length as i32 - 1) as i16;
        e.mod_play(-1, pos, 0);
    }

    fn stop(&mut self) {
        self.engine.mod_stop();
    }

    fn set_muted(&mut self, ch: usize, muted: bool) {
        if ch < PAULA_VOICES {
            self.engine.rep.muted[ch] = muted;
            if muted {
                // PT: muting turns off the voice DMA right away
                self.engine.paula.write_dmacon(1 << ch);
            }
        }
    }
    fn is_muted(&self, ch: usize) -> bool {
        ch < PAULA_VOICES && self.engine.rep.muted[ch]
    }

    fn order_pattern(&self, order: i32) -> Option<i32> {
        let s = &self.engine.song;
        if order < 0 || order >= s.song_length as i32 {
            return None;
        }
        Some(s.pattern_table[order as usize] as i32)
    }
    fn num_rows(&self, _pattern: i32) -> i32 {
        64
    }

    fn cell(&self, pattern: i32, row: i32, ch: usize, _view: CellView) -> Cell {
        let mut c: Cell = [b' '; 8];
        if !(0..100).contains(&pattern) || !(0..64).contains(&row) || ch >= PAULA_VOICES {
            return c;
        }
        let n = self.engine.song.patterns[pattern as usize][row as usize * PAULA_VOICES + ch];
        match period_to_note(n.period) {
            0 => put(&mut c, 0, b"---"),
            1 => put(&mut c, 0, b"???"),
            i => {
                let k = i - 2;
                let name = PT_NOTE_NAMES[k % 12];
                put(&mut c, 0, &[name[0], name[1], b'1' + (k / 12) as u8]);
            }
        }
        put(&mut c, 3, &[HEX[(n.sample >> 4) as usize], HEX[(n.sample & 15) as usize]]);
        put(&mut c, 5, &[HEX[(n.command & 15) as usize], HEX[(n.param >> 4) as usize], HEX[(n.param & 15) as usize]]);
        c
    }

    fn vu(&self, ch: usize) -> VuState {
        let t = self.engine.rep.vu_trigger[ch.min(PAULA_VOICES - 1)];
        VuState::Trigger(t.0, t.1)
    }

    fn scope(&self, ch: usize) -> Option<ScopeData> {
        if ch >= PAULA_VOICES {
            return None;
        }
        let (mut pos, mut left, loop_start, loop_len, vol) = self.engine.paula.scope_info(ch)?;
        let data = &self.engine.song.sample_data;
        let mut out = [0i8; 40];
        for o in out.iter_mut() {
            *o = data.get(pos).copied().unwrap_or(0);
            pos += 1;
            left -= 1;
            if left == 0 {
                pos = loop_start;
                left = loop_len.max(1);
            }
        }
        Some(ScopeData { samples: out, volume: vol.min(64) })
    }
}

/// IT-style note text: 1..120 notes, 253 fade, 254 cut, 255 off
pub(crate) fn note_text(n: u8) -> [u8; 3] {
    const NAMES: [&[u8; 2]; 12] = [b"C-", b"C#", b"D-", b"D#", b"E-", b"F-", b"F#", b"G-", b"G#", b"A-", b"A#", b"B-"];
    match n {
        0 => [DOT; 3],
        1..=120 => {
            let k = (n - 1) as usize;
            let nm = NAMES[k % 12];
            [nm[0], nm[1], b'0' + (k / 12) as u8]
        }
        255 => *b"===", // key off
        254 => *b"^^^", // note cut
        253 => *b"~~~", // note fade
        _ => *b"???",
    }
}

// ---------------------------------------------------------------------------
// own Rust IT engine backend
// ---------------------------------------------------------------------------

pub struct ItEnginePlayer {
    p: crate::it::ItPlayer,
    file_size: usize,
    buf: Vec<f32>,
}

impl ItEnginePlayer {
    pub fn load(data: &[u8], output_rate: u32) -> Result<Self, String> {
        let m = crate::it::ItModule::load_any(data).map_err(|e| e.to_string())?;
        Ok(ItEnginePlayer { p: crate::it::ItPlayer::new(m, output_rate), file_size: data.len(), buf: Vec::new() })
    }
    fn pattern(&self, pattern: i32) -> Option<&crate::it::format::Pattern> {
        if pattern < 0 {
            None
        } else {
            self.p.m.patterns.get(pattern as usize)
        }
    }
}

impl Player for ItEnginePlayer {
    fn is_protracker(&self) -> bool {
        false
    }
    fn timing_text(&self) -> String {
        match self.p.m.format {
            crate::it::format::Format::S3m => "S3M".into(),
            _ => "IT".into(),
        }
    }
    fn format_name(&self) -> &str {
        match self.p.m.format {
            crate::it::format::Format::S3m => "S3M (Rust engine)",
            _ => "IT (Rust engine)",
        }
    }
    fn title(&self) -> String {
        self.p.m.title.clone()
    }
    fn num_channels(&self) -> usize {
        self.p.m.num_channels.clamp(1, 64)
    }
    fn num_orders(&self) -> i32 {
        let o = &self.p.m.orders;
        o.iter().position(|&x| x == 255).unwrap_or(o.len()).max(1) as i32
    }
    fn num_samples(&self) -> i32 {
        let m = &self.p.m;
        (if m.instrument_mode { m.instruments.len() } else { m.samples.len() }).max(1) as i32
    }
    fn sample_info(&self, index: i32) -> SampleInfo {
        let m = &self.p.m;
        let i = index.max(0) as usize;
        if m.instrument_mode {
            SampleInfo { name: m.instruments.get(i).map(|x| x.name.clone()).unwrap_or_default(), ..Default::default() }
        } else if let Some(s) = m.samples.get(i) {
            let (ls, ll) = if s.looped { (Some(s.loop_start as u32), Some((s.loop_end - s.loop_start) as u32)) } else { (None, None) };
            SampleInfo { name: s.name.clone(), volume: Some(s.volume), finetune: None, length: Some(s.length as u32), loop_start: ls, loop_length: ll }
        } else {
            SampleInfo::default()
        }
    }
    fn file_size(&self) -> usize {
        self.file_size
    }
    fn has_volume_column(&self) -> bool {
        true
    }

    fn render(&mut self, out: &mut [i16]) {
        if !self.p.playing {
            out.fill(0);
            return;
        }
        if self.buf.len() != out.len() {
            self.buf.resize(out.len(), 0.0);
        }
        self.p.render_f32(&mut self.buf);
        for (o, &x) in out.iter_mut().zip(self.buf.iter()) {
            *o = (x * 32767.0).clamp(-32768.0, 32767.0) as i16;
        }
    }
    fn render_f32(&mut self, out: &mut [f32]) {
        if !self.p.playing {
            out.fill(0.0);
            return;
        }
        self.p.render_f32(out);
    }

    fn status(&self) -> Status {
        let p = &self.p;
        Status {
            playing: p.playing,
            order: p.order as i32,
            pattern: p.pattern as i32,
            row: p.row as i32,
            num_rows: self.num_rows(p.pattern as i32),
            speed: p.speed as i32,
            bpm: p.tempo as i32,
            seconds: p.seconds,
        }
    }
    fn play_from(&mut self, order: i32) {
        let n = self.num_orders();
        self.p.play_from(order.clamp(0, n - 1) as usize);
    }
    fn stop(&mut self) {
        self.p.playing = false;
    }
    fn set_muted(&mut self, ch: usize, muted: bool) {
        if ch < self.p.muted.len() {
            self.p.muted[ch] = muted;
        }
    }
    fn is_muted(&self, ch: usize) -> bool {
        self.p.muted.get(ch).copied().unwrap_or(false)
    }

    fn order_pattern(&self, order: i32) -> Option<i32> {
        match self.p.m.orders.get(usize::try_from(order).ok()?) {
            Some(&x) if x < 254 => Some(x as i32),
            _ => None,
        }
    }
    fn num_rows(&self, pattern: i32) -> i32 {
        // IT treats a missing pattern as 64 empty rows
        self.pattern(pattern).map(|p| p.rows as i32).unwrap_or(64)
    }

    fn cell(&self, pattern: i32, row: i32, ch: usize, view: CellView) -> Cell {
        let mut c: Cell = [b' '; 8];
        let Some(pat) = self.pattern(pattern) else {
            if row >= 0 && row < 64 && ch < self.num_channels() {
                put(&mut c, 0, &[DOT, DOT, DOT, DOT, DOT, DOT, DOT, DOT]);
            }
            return c;
        };
        if ch >= self.num_channels() || row < 0 || row as usize >= pat.rows {
            return c;
        }
        let x = pat.cell(row as usize, ch);
        put(&mut c, 0, &note_text(x.note));
        match x.ins {
            0 => put(&mut c, 3, &[DOT, DOT]),
            1..=99 => put(&mut c, 3, &[b'0' + x.ins / 10, b'0' + x.ins % 10]),
            n => put(&mut c, 3, &[HEX[(n >> 4) as usize], HEX[(n & 15) as usize]]),
        }
        match view {
            CellView::Effect => {
                if x.cmd == 0 && x.param == 0 {
                    put(&mut c, 5, &[DOT; 3]);
                } else {
                    let letter = if (1..=26).contains(&x.cmd) { b'A' + x.cmd - 1 } else { b'?' };
                    let letter = if x.cmd == 0 { DOT } else { letter };
                    put(&mut c, 5, &[letter, HEX[(x.param >> 4) as usize], HEX[(x.param & 15) as usize]]);
                }
            }
            CellView::Volume => {
                // IT volume column, shown in OpenMPT style (letter + 2 digits)
                let (l, v) = match x.vol {
                    0..=64 => (b'v', x.vol),
                    65..=74 => (b'a', x.vol - 65),
                    75..=84 => (b'b', x.vol - 75),
                    85..=94 => (b'c', x.vol - 85),
                    95..=104 => (b'd', x.vol - 95),
                    105..=114 => (b'e', x.vol - 105),
                    115..=124 => (b'f', x.vol - 115),
                    128..=192 => (b'p', x.vol - 128),
                    193..=202 => (b'g', x.vol - 193),
                    203..=212 => (b'h', x.vol - 203),
                    _ => (0, 0),
                };
                if l == 0 {
                    put(&mut c, 5, &[DOT; 3]);
                } else {
                    put(&mut c, 5, &[l, b'0' + v / 10, b'0' + v % 10]);
                }
            }
        }
        c
    }

    fn vu(&self, ch: usize) -> VuState {
        if self.is_muted(ch) || !self.p.playing {
            return VuState::Level(0);
        }
        VuState::Level((self.p.level(ch).sqrt() * 48.0 + 0.5).min(48.0) as u8)
    }

    fn scope(&self, ch: usize) -> Option<ScopeData> {
        // always Some: this backend has the real waveform (silence = volume 0)
        Some(match self.p.scope(ch) {
            Some((samples, volume)) => ScopeData { samples, volume },
            None => ScopeData { samples: [0; 40], volume: 0 },
        })
    }
}


// ---------------------------------------------------------------------------
// own Rust XM engine backend (FastTracker II replayer port)
// ---------------------------------------------------------------------------

pub struct XmEnginePlayer {
    p: crate::xm::XmPlayer,
    file_size: usize,
    buf: Vec<f32>,
}

impl XmEnginePlayer {
    pub fn load(data: &[u8], output_rate: u32) -> Result<Self, String> {
        let m = crate::xm::format::load(data).map_err(|e| e.to_string())?;
        Ok(XmEnginePlayer { p: crate::xm::XmPlayer::new(m, output_rate), file_size: data.len(), buf: Vec::new() })
    }
}

impl Player for XmEnginePlayer {
    fn is_protracker(&self) -> bool {
        false
    }
    fn timing_text(&self) -> String {
        if self.p.m.linear { "XM LIN".into() } else { "XM AMI".into() }
    }
    fn format_name(&self) -> &str {
        "XM (Rust engine)"
    }
    fn title(&self) -> String {
        self.p.m.title.clone()
    }
    fn num_channels(&self) -> usize {
        self.p.m.num_channels.clamp(1, crate::xm::format::MAX_CHANNELS)
    }
    fn num_orders(&self) -> i32 {
        self.p.m.song_length.max(1) as i32
    }
    fn num_samples(&self) -> i32 {
        // instruments up to the last one with a name or samples
        let m = &self.p.m;
        let last = (1..=128).rev().find(|&i| !m.instr_names[i].trim().is_empty() || m.instruments[i].is_some()).unwrap_or(1);
        last as i32
    }
    fn sample_info(&self, index: i32) -> SampleInfo {
        let i = (index.max(0) as usize + 1).min(128);
        SampleInfo { name: self.p.m.instr_names[i].clone(), length: if self.p.m.instruments[i].is_some() { None } else { Some(0) }, ..Default::default() }
    }
    fn file_size(&self) -> usize {
        self.file_size
    }
    fn has_volume_column(&self) -> bool {
        true
    }
    fn render(&mut self, out: &mut [i16]) {
        if !self.p.playing {
            out.fill(0);
            return;
        }
        if self.buf.len() != out.len() {
            self.buf.resize(out.len(), 0.0);
        }
        self.p.render_f32(&mut self.buf);
        for (o, &x) in out.iter_mut().zip(self.buf.iter()) {
            *o = (x * 32767.0).clamp(-32768.0, 32767.0) as i16;
        }
    }
    fn render_f32(&mut self, out: &mut [f32]) {
        if !self.p.playing {
            out.fill(0.0);
            return;
        }
        self.p.render_f32(out);
    }
    fn status(&self) -> Status {
        let p = &self.p;
        Status {
            playing: p.playing,
            order: p.cur_pos as i32,
            pattern: p.cur_pattern as i32,
            row: p.cur_row as i32,
            num_rows: self.num_rows(p.cur_pattern as i32),
            speed: p.speed as i32,
            bpm: p.bpm as i32,
            seconds: p.seconds,
        }
    }
    fn play_from(&mut self, order: i32) {
        let n = self.num_orders();
        self.p.play_from(order.clamp(0, n - 1) as usize);
    }
    fn stop(&mut self) {
        self.p.playing = false;
    }
    fn set_muted(&mut self, ch: usize, muted: bool) {
        if ch < self.p.muted.len() {
            self.p.muted[ch] = muted;
        }
    }
    fn is_muted(&self, ch: usize) -> bool {
        self.p.muted.get(ch).copied().unwrap_or(false)
    }
    fn num_rows(&self, pattern: i32) -> i32 {
        if pattern < 0 {
            return 64;
        }
        self.p.m.patterns.get(pattern as usize).map(|p| p.rows as i32).unwrap_or(64)
    }
    fn order_pattern(&self, order: i32) -> Option<i32> {
        if order < 0 || order as usize >= self.p.m.song_length {
            return None;
        }
        Some(self.p.m.orders[order as usize] as i32)
    }
    fn cell(&self, pattern: i32, row: i32, ch: usize, view: CellView) -> Cell {
        let mut c: Cell = [b' '; 8];
        if pattern < 0 || row < 0 || ch >= self.num_channels() {
            return c;
        }
        let Some(pat) = self.p.m.patterns.get(pattern as usize) else {
            return c;
        };
        if row as usize >= pat.rows {
            return c;
        }
        let n = pat.note(row as usize, ch);
        put(&mut c, 0, &note_text(match n.note {
            97 => 255,
            1..=96 => n.note,
            _ => 0,
        }));
        match n.instr {
            0 => put(&mut c, 3, &[DOT, DOT]),
            1..=99 => put(&mut c, 3, &[b'0' + n.instr / 10, b'0' + n.instr % 10]),
            x => put(&mut c, 3, &[HEX[(x >> 4) as usize], HEX[(x & 15) as usize]]),
        }
        match view {
            CellView::Effect => {
                if n.efx == 0 && n.param == 0 {
                    put(&mut c, 5, &[DOT; 3]);
                } else {
                    let l = match n.efx {
                        0..=9 => b'0' + n.efx,
                        10..=35 => b'A' + n.efx - 10,
                        _ => b'?',
                    };
                    put(&mut c, 5, &[l, HEX[(n.param >> 4) as usize], HEX[(n.param & 15) as usize]]);
                }
            }
            CellView::Volume => {
                // XM volume column in OpenMPT style (letter + 2 digits)
                let v = n.vol;
                let (l, x) = match v >> 4 {
                    1..=4 => (b'v', v - 0x10),
                    5 if v == 0x50 => (b'v', 64),
                    6 => (b'd', v & 15),
                    7 => (b'c', v & 15),
                    8 => (b'b', v & 15),
                    9 => (b'a', v & 15),
                    0xA => (b'u', v & 15),
                    0xB => (b'h', v & 15),
                    0xC => (b'p', (v & 15) * 4),
                    0xD => (b'l', v & 15),
                    0xE => (b'r', v & 15),
                    0xF => (b'g', v & 15),
                    _ => (0, 0),
                };
                if l == 0 {
                    put(&mut c, 5, &[DOT; 3]);
                } else {
                    put(&mut c, 5, &[l, b'0' + x / 10, b'0' + x % 10]);
                }
            }
        }
        c
    }
    fn vu(&self, ch: usize) -> VuState {
        if self.is_muted(ch) || !self.p.playing {
            return VuState::Level(0);
        }
        VuState::Level((self.p.level(ch).sqrt() * 48.0 + 0.5).min(48.0) as u8)
    }
    fn scope(&self, ch: usize) -> Option<ScopeData> {
        Some(match self.p.scope(ch) {
            Some((samples, volume)) => ScopeData { samples, volume },
            None => ScopeData { samples: [0; 40], volume: 0 },
        })
    }
}

// ---------------------------------------------------------------------------
// libopenmpt backend
// ---------------------------------------------------------------------------

#[cfg(has_openmpt)]
pub use mpt::OpenMptPlayer;

#[cfg(has_openmpt)]
mod mpt {
    use super::*;
    use crate::openmpt::*;

    pub struct OpenMptPlayer {
        m: OpenMptModule,
        rate: u32,
        playing: bool,
        format: String,
        file_size: usize,
        muted: Vec<bool>,
        num_channels: usize,
    }

    impl OpenMptPlayer {
        pub fn load(data: &[u8], output_rate: u32, stereo_separation: u8) -> Result<Self, String> {
            let mut m = OpenMptModule::load(data)?;
            m.set_repeat_count(-1); // loop forever, like PT
            m.set_stereo_separation(stereo_separation as i32);
            let format = m.metadata("type").to_ascii_uppercase();
            let n = m.num_channels().max(1) as usize;
            Ok(OpenMptPlayer { m, rate: output_rate, playing: false, format, file_size: data.len(), muted: vec![false; n], num_channels: n })
        }
    }


    impl Player for OpenMptPlayer {
        fn is_protracker(&self) -> bool {
            false
        }
        fn timing_text(&self) -> String {
            self.format.clone()
        }
        fn format_name(&self) -> &str {
            &self.format
        }
        fn title(&self) -> String {
            self.m.metadata("title")
        }
        fn num_channels(&self) -> usize {
            self.num_channels
        }
        fn num_orders(&self) -> i32 {
            self.m.num_orders()
        }
        fn num_samples(&self) -> i32 {
            let ni = self.m.num_instruments();
            if ni > 0 { ni } else { self.m.num_samples() }
        }
        fn sample_info(&self, index: i32) -> SampleInfo {
            let name = if self.m.num_instruments() > 0 { self.m.instrument_name(index) } else { self.m.sample_name(index) };
            SampleInfo { name, ..Default::default() }
        }
        fn file_size(&self) -> usize {
            self.file_size
        }
        fn has_volume_column(&self) -> bool {
            true
        }

        fn render(&mut self, out: &mut [i16]) {
            if !self.playing {
                out.fill(0);
                return;
            }
            let frames = out.len() / 2;
            let mut done = 0;
            while done < frames {
                let n = self.m.read_stereo(self.rate, &mut out[done * 2..frames * 2]);
                if n == 0 {
                    out[done * 2..].fill(0);
                    self.playing = false;
                    break;
                }
                done += n;
            }
        }

        fn status(&self) -> Status {
            let pattern = self.m.pattern();
            Status {
                playing: self.playing,
                order: self.m.order(),
                pattern,
                row: self.m.row(),
                num_rows: self.m.pattern_num_rows(pattern),
                speed: self.m.speed(),
                bpm: self.m.tempo(),
                seconds: self.m.position_seconds(),
            }
        }

        fn play_from(&mut self, order: i32) {
            let o = order.clamp(0, (self.m.num_orders() - 1).max(0));
            self.m.set_position_order_row(o, 0);
            self.playing = true;
        }

        fn stop(&mut self) {
            self.playing = false;
        }

        fn set_muted(&mut self, ch: usize, muted: bool) {
            if ch < self.muted.len() && self.m.set_channel_mute(ch as i32, muted) {
                self.muted[ch] = muted;
            }
        }
        fn is_muted(&self, ch: usize) -> bool {
            self.muted.get(ch).copied().unwrap_or(false)
        }

        fn num_rows(&self, pattern: i32) -> i32 {
            self.m.pattern_num_rows(pattern)
        }
        fn order_pattern(&self, order: i32) -> Option<i32> {
            if order < 0 || order >= self.m.num_orders() {
                return None;
            }
            let p = self.m.order_pattern(order);
            if p < 0 || p >= self.m.num_patterns() { None } else { Some(p) }
        }

        fn cell(&self, pattern: i32, row: i32, ch: usize, view: CellView) -> Cell {
            let mut c: Cell = [b' '; 8];
            if ch >= self.num_channels || row < 0 || row >= self.m.pattern_num_rows(pattern) {
                return c;
            }
            let ch = ch as i32;
            put(&mut c, 0, &note_text(self.m.command(pattern, row, ch, CMD_NOTE)));

            let ins = self.m.command(pattern, row, ch, CMD_INSTRUMENT);
            if ins == 0 {
                put(&mut c, 3, &[DOT, DOT]);
            } else if ins < 100 {
                put(&mut c, 3, &[b'0' + ins / 10, b'0' + ins % 10]);
            } else {
                put(&mut c, 3, &[HEX[(ins >> 4) as usize], HEX[(ins & 15) as usize]]);
            }

            match view {
                CellView::Effect => {
                    let eff = self.m.command(pattern, row, ch, CMD_EFFECT);
                    let par = self.m.command(pattern, row, ch, CMD_PARAMETER);
                    if eff == 0 && par == 0 {
                        put(&mut c, 5, &[DOT; 3]);
                    } else {
                        let letter = self.m.format_command(pattern, row, ch, CMD_EFFECT).bytes().next().unwrap_or(b'?');
                        let letter = if letter == b'.' || letter == b' ' { DOT } else { letter };
                        put(&mut c, 5, &[letter, HEX[(par >> 4) as usize], HEX[(par & 15) as usize]]);
                    }
                }
                CellView::Volume => {
                    let veff = self.m.command(pattern, row, ch, CMD_VOLUMEEFFECT);
                    if veff == 0 {
                        put(&mut c, 5, &[DOT; 3]);
                    } else {
                        let letter = self.m.format_command(pattern, row, ch, CMD_VOLUMEEFFECT).bytes().next().unwrap_or(b'?');
                        let v = self.m.command(pattern, row, ch, CMD_VOLUME).min(99);
                        put(&mut c, 5, &[letter, b'0' + v / 10, b'0' + v % 10]);
                    }
                }
            }
            c
        }

        fn vu(&self, ch: usize) -> VuState {
            if self.is_muted(ch) {
                return VuState::Level(0);
            }
            let v = self.m.channel_vu(ch as i32).clamp(0.0, 1.0);
            VuState::Level((v * 48.0 + 0.5) as u8)
        }

        fn scope(&self, _ch: usize) -> Option<ScopeData> {
            None
        }
    }
}

/// Loads a file with the most fitting backend: ProTracker engine for MODs it can
/// load, otherwise libopenmpt (when available).
pub fn load_any(data: &[u8], output_rate: u32, stereo_separation: u8) -> Result<Box<dyn Player>, String> {
    load_with(data, output_rate, stereo_separation, false)
}

/// true if the data looks like an Impulse Tracker module
pub fn is_it(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == b"IMPM"
}

/// true if the built-in Rust engine plays this module (IT, S3M and XM)
pub fn own_engine(data: &[u8]) -> bool {
    is_it(data) || crate::it::s3m::detect(data) || crate::xm::format::detect(data)
}

/// Like `load_any`, but IT files go to the own Rust IT engine unless
/// `prefer_openmpt` is set (and libopenmpt is available).
pub fn load_with(data: &[u8], output_rate: u32, stereo_separation: u8, prefer_openmpt: bool) -> Result<Box<dyn Player>, String> {
    if own_engine(data) && !(prefer_openmpt && cfg!(has_openmpt)) {
        let res: Result<Box<dyn Player>, String> = if crate::xm::format::detect(data) {
            XmEnginePlayer::load(data, output_rate).map(|p| Box::new(p) as Box<dyn Player>)
        } else {
            ItEnginePlayer::load(data, output_rate).map(|p| Box::new(p) as Box<dyn Player>)
        };
        match res {
            Ok(p) => return Ok(p),
            Err(e) if !cfg!(has_openmpt) => return Err(e),
            Err(_) => {} // fall back to libopenmpt
        }
    }
    let is_mod31 = crate::module::detect_mod31(data);
    if is_mod31 {
        if let Ok(p) = Pt2Player::load(data, output_rate, stereo_separation) {
            return Ok(Box::new(p));
        }
    }

    #[cfg(has_openmpt)]
    {
        match OpenMptPlayer::load(data, output_rate, stereo_separation) {
            Ok(p) => return Ok(Box::new(p)),
            Err(e) if !is_mod31 => {
                // last resort: 15-sample (Soundtracker) MODs have no magic ID
                if let Ok(p) = Pt2Player::load(data, output_rate, stereo_separation) {
                    return Ok(Box::new(p));
                }
                return Err(e);
            }
            Err(e) => return Err(e),
        }
    }

    #[cfg(not(has_openmpt))]
    {
        Pt2Player::load(data, output_rate, stereo_separation)
            .map(|p| Box::new(p) as Box<dyn Player>)
            .map_err(|e| format!("{e} (built without libopenmpt: IT/XM/S3M are not supported)"))
    }
}
