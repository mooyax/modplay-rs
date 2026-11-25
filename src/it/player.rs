//! Impulse Tracker replayer + mixer.
//!
//! Behaviour follows ITTECH.TXT and the IT-compatible code paths of OpenMPT
//! (BSD license), which documents most of Impulse Tracker's quirks.
//!
//! Channels 0..63 are the pattern ("host") channels. When a new note is played
//! and the instrument's New Note Action is not "cut", the old note is moved to
//! a background voice (64..MAX_VOICES) where it keeps playing / fading out.
//!
//! Not implemented yet: resonant filters (Zxx / instrument cutoff / filter
//! envelopes), MIDI macros, old-effects-mode differences, "gliss control".

use super::format::*;

pub const MAX_VOICES: usize = 256;
const ITSIN: [i8; 256] = [
    0, 2, 3, 5, 6, 8, 9, 11, 12, 14, 16, 17, 19, 20, 22, 23, 24, 26, 27, 29, 30, 32, 33, 34, 36, 37, 38, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
    52, 53, 54, 55, 56, 56, 57, 58, 59, 59, 60, 60, 61, 61, 62, 62, 62, 63, 63, 63, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 63, 63, 63, 62, 62, 62,
    61, 61, 60, 60, 59, 59, 58, 57, 56, 56, 55, 54, 53, 52, 51, 50, 49, 48, 47, 46, 45, 44, 43, 42, 41, 39, 38, 37, 36, 34, 33, 32, 30, 29, 27, 26, 24,
    23, 22, 20, 19, 17, 16, 14, 12, 11, 9, 8, 6, 5, 3, 2, 0, -2, -3, -5, -6, -8, -9, -11, -12, -14, -16, -17, -19, -20, -22, -23, -24, -26, -27, -29,
    -30, -32, -33, -34, -36, -37, -38, -39, -41, -42, -43, -44, -45, -46, -47, -48, -49, -50, -51, -52, -53, -54, -55, -56, -56, -57, -58, -59, -59,
    -60, -60, -61, -61, -62, -62, -62, -63, -63, -63, -64, -64, -64, -64, -64, -64, -64, -64, -64, -64, -64, -63, -63, -63, -62, -62, -62, -61, -61,
    -60, -60, -59, -59, -58, -57, -56, -56, -55, -54, -53, -52, -51, -50, -49, -48, -47, -46, -45, -44, -43, -42, -41, -39, -38, -37, -36, -34, -33,
    -32, -30, -29, -27, -26, -24, -23, -22, -20, -19, -17, -16, -14, -12, -11, -9, -8, -6, -5, -3, -2,
];
const PORTA_VOLCMD: [u8; 10] = [0x00, 0x01, 0x04, 0x08, 0x10, 0x20, 0x40, 0x60, 0x80, 0xFF];

/// One playing note (host channel or background voice) plus, for host
/// channels, all the effect state.
#[derive(Clone, Default)]
pub struct Chn {
    // ---- voice (note) state ----
    pub active: bool,
    pub sample: usize, // index into samples (valid if active)
    pub ins: Option<usize>,
    pub pos: f64,
    pub backwards: bool,
    /// current frequency in Hz (IT "period" in Hz mode)
    pub freq: f64,
    /// note after keyboard mapping (1..120), 0 = none
    pub note: u8,
    /// note as written in the pattern (for DCT=note)
    pub pattern_note: u8,
    /// 0..256
    pub volume: i32,
    /// sample global volume * instrument global volume, 0..64
    pub ins_vol: i32,
    /// channel volume 0..64
    pub chn_vol: i32,
    /// 0..256
    pub pan: i32,
    pub surround: bool,
    pub vol_swing: i32,
    pub pan_swing: i32,
    pub fade_vol: i32,
    pub note_fade: bool,
    pub key_off: bool,
    pub vol_env_pos: u32,
    pub pan_env_pos: u32,
    pub pitch_env_pos: u32,
    pub vol_env_on: bool,
    pub pan_env_on: bool,
    pub pitch_env_on: bool,
    pub autovib_pos: u32,
    pub autovib_depth: i32,
    pub nna: Nna,
    /// host channel this background voice belongs to
    pub master: Option<usize>,
    /// fade out quickly and stop (used when a note is cut)
    pub kill: bool,

    // ---- computed every tick ----
    pub inc: f64,
    pub target_l: f32,
    pub target_r: f32,
    pub gain_l: f32,
    pub gain_r: f32,
    pub final_vol: i32,
    /// reached the end of a non-looping sample
    pub ended: bool,

    // ---- effect state (host channels) ----
    pub last_ins: u8,
    pub cmd: u8,
    pub param: u8,
    pub volcmd: u8,
    pub mem_vol_slide: u8,
    pub mem_volcol_slide: u8,
    pub mem_porta: u8,
    pub mem_porta_g: u8,
    pub porta_target: f64,
    pub vib_speed: u8,
    pub vib_depth: u8,
    pub vib_type: u8,
    pub vib_pos: u8,
    pub vibrato_on: bool,
    pub trem_speed: u8,
    pub trem_depth: u8,
    pub trem_type: u8,
    pub trem_pos: u8,
    pub tremolo_on: bool,
    pub panb_speed: u8,
    pub panb_depth: u8,
    pub panb_type: u8,
    pub panb_pos: u8,
    pub panbrello_on: bool,
    pub panb_offset: i32,
    pub mem_tremor: u8,
    pub tremor_count: u8,
    pub tremor_on: bool,
    pub mem_arp: u8,
    pub arp_on: bool,
    pub mem_offset: u8,
    pub high_offset: u8,
    pub mem_retrig: u8,
    pub retrig_count: u8,
    pub mem_chn_vol_slide: u8,
    pub mem_pan_slide: u8,
    pub mem_gvol_slide: u8,
    pub mem_tempo: u8,
    pub mem_s: u8,
    pub loop_row: usize,
    pub loop_count: u8,
    pub note_cut_tick: Option<u32>,
    pub delayed: Option<(Cell, u32)>,
    pub glissando: bool,
    /// U (fine vibrato) is active (ST3 scales it differently)
    pub fine_vib: bool,

    // ---- resonant filter ----
    /// 0..127 (127 = open); set by instrument, Zxx macros
    pub cutoff: u8,
    /// 0..127
    pub resonance: u8,
    pub filter_hp: bool,
    /// filter active for this voice (OpenMPT's CHN_FILTER)
    pub filter_on: bool,
    pub filter_a0: f32,
    pub filter_b0: f32,
    pub filter_b1: f32,
    /// filter history [left/mono, right][y1, y2]
    pub filter_y: [[f32; 2]; 2],
    /// a new note was triggered this tick
    pub trigger_note: bool,
    /// SFx: macro used by Z00..Z7F
    pub active_macro: u8,
    /// Zxx command waiting to be evaluated this tick
    pub pending_macro: Option<u8>,
}

pub struct ItPlayer {
    pub m: ItModule,
    pub chn: Vec<Chn>,
    pub rate: u32,
    pub playing: bool,

    pub speed: u32,
    pub tempo: u32,
    pub tick: u32,
    pub extra_ticks: u32,
    pub row_repeat: u32,
    pub row_repeat_set: bool,
    pub order: usize,
    pub row: usize,
    pub pattern: usize,
    first_row: bool,
    jump_order: Option<usize>,
    break_row: Option<usize>,
    loop_jump: Option<usize>,
    pub global_vol: i32, // 0..128

    samples_to_tick: f64,
    pub seconds: f64,
    pub muted: [bool; MAX_CHANNELS],
    rng: u32,
    /// output gain applied in the mixer
    pub master_gain: f32,
    ramp_step: f32,
}

/// 8363 * 1712: period <-> frequency constant for Amiga-style periods
const S3M_K: f64 = 8363.0 * 1712.0;

#[inline]
fn slide_linear(freq: f64, units_768: f64) -> f64 {
    freq * (units_768 / 768.0).exp2()
}

impl ItPlayer {
    pub fn new(m: ItModule, rate: u32) -> Self {
        let mut p = ItPlayer {
            m,
            chn: vec![Chn::default(); MAX_VOICES],
            rate,
            playing: false,
            speed: 6,
            tempo: 125,
            tick: 0,
            extra_ticks: 0,
            row_repeat: 0,
            row_repeat_set: false,
            order: 0,
            row: 0,
            pattern: 0,
            first_row: true,
            jump_order: None,
            break_row: None,
            loop_jump: None,
            global_vol: 128,
            samples_to_tick: 0.0,
            seconds: 0.0,
            muted: [false; MAX_CHANNELS],
            rng: 0x1234_5678,
            master_gain: 1.0,
            ramp_step: 1.0 / (rate as f32 * 0.0015).max(1.0),
        };
        p.reset(0);
        p
    }

    /// Envelope carry flags (volume, panning, pitch/filter) of instrument `ii`
    fn env_carry(&self, ii: Option<usize>) -> (bool, bool, bool) {
        match ii.and_then(|i| self.m.instruments.get(i)) {
            Some(i) => (i.vol_env.carry, i.pan_env.carry, i.pitch_env.carry),
            None => (false, false, false),
        }
    }

    fn rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    /// Resets the song state and positions playback at `order`.
    pub fn reset(&mut self, order: usize) {
        self.speed = self.m.speed as u32;
        self.tempo = self.m.tempo as u32;
        self.global_vol = self.m.global_vol as i32;
        self.tick = 0;
        self.extra_ticks = 0;
        self.row_repeat = 0;
        self.row_repeat_set = false;
        self.jump_order = None;
        self.break_row = None;
        self.loop_jump = None;
        self.samples_to_tick = 0.0;
        self.seconds = 0.0;
        for (i, c) in self.chn.iter_mut().enumerate() {
            *c = Chn::default();
            c.cutoff = 0x7F;
            if i < MAX_CHANNELS {
                let p = self.m.chn_pan[i];
                c.chn_vol = self.m.chn_vol[i] as i32;
                if p == 100 {
                    c.pan = 128;
                    c.surround = true;
                } else {
                    c.pan = ((p & 0x7F).min(64) as i32) * 4;
                }
                if let Some(p256) = &self.m.chn_pan256 {
                    c.pan = p256[i];
                }
            }
        }
        self.order = order.min(self.m.orders.len().saturating_sub(1));
        self.row = 0;
        self.first_row = true;
        // the mixer gain: IT's mix volume, scaled to a reasonable level
        // matches libopenmpt's default ("compatible") mix levels
        self.master_gain = self.m.mix_vol as f32 / 128.0 * self.m.mix_mode_gain;
        if self.m.legacy_mix {
            // libopenmpt "Original" mix levels: pre-attenuation by channel count
            // (Sndmix.cpp PreAmpTable), relative to the compatible mix levels
            const PRE_AMP: [u8; 16] = [0x60, 0x60, 0x60, 0x70, 0x80, 0x88, 0x90, 0x98, 0xA0, 0xA4, 0xA8, 0xAC, 0xB0, 0xB4, 0xB8, 0xBC];
            let nchn = self.m.num_channels.clamp(1, 31);
            self.master_gain *= 64.0 / PRE_AMP[nchn / 2] as f32;
        }
    }

    // ------------------------------------------------------------------
    // position handling
    // ------------------------------------------------------------------

    /// Returns the first playable order >= `o` (skipping "+++" markers), wrapping to 0 at the end.
    fn valid_order(&self, mut o: usize) -> Option<usize> {
        let n = self.m.orders.len();
        for _ in 0..2 {
            while o < n {
                match self.m.orders[o] {
                    254 => o += 1,
                    255 => break,
                    p if (p as usize) < self.m.patterns.len() => return Some(o),
                    _ => o += 1,
                }
            }
            o = 0; // end of song: loop
        }
        None
    }

    fn advance_row(&mut self) {
        if self.first_row {
            self.first_row = false;
            match self.valid_order(self.order) {
                Some(o) => self.order = o,
                None => {
                    self.playing = false;
                    return;
                }
            }
            self.pattern = self.m.orders[self.order] as usize;
            return;
        }

        let loop_row = self.loop_jump.take();
        let jump = self.jump_order.take();
        let brk = self.break_row.take();

        let mut next_order = self.order;
        let mut next_row = self.row + 1;
        let s3m = self.m.format == Format::S3m;
        // IT: a position jump wins over a pattern loop, a pattern break does not.
        // ST3: an active pattern loop wins over both.
        let take_jump = (jump.is_some() || brk.is_some()) && (loop_row.is_none() || (!s3m && jump.is_some()));
        if take_jump {
            next_order = jump.unwrap_or(self.order + 1);
            next_row = brk.unwrap_or(0);
            if next_order >= self.m.orders.len() {
                next_order = 0;
            }
        } else if let Some(r) = loop_row {
            next_row = r;
            if r >= self.m.patterns[self.pattern].rows {
                next_order = self.order + 1;
                next_row = 0;
            }
        } else if next_row >= self.m.patterns[self.pattern].rows {
            next_order = self.order + 1;
            next_row = 0;
        }

        if s3m {
            // jumping to another position forgets running loops
            if take_jump && next_order != self.order {
                for c in self.chn.iter_mut() {
                    c.loop_count = 0;
                }
            }
            // a new pattern (or a break) resets the loop start (LoopReset.s3m)
            if next_row == 0 || (take_jump && brk.is_some()) {
                for c in self.chn.iter_mut() {
                    c.loop_row = 0;
                }
            }
        }

        if next_order != self.order || jump.is_some() || brk.is_some() {
            match self.valid_order(next_order) {
                Some(o) => {
                    if o < next_order {
                        next_row = 0; // wrapped around to the start
                    }
                    self.order = o;
                    self.pattern = self.m.orders[o] as usize;
                }
                None => {
                    self.playing = false;
                    return;
                }
            }
        }
        if next_row >= self.m.patterns[self.pattern].rows {
            next_row = 0;
        }
        self.row = next_row;
    }

    // ------------------------------------------------------------------
    // tick processing
    // ------------------------------------------------------------------

    pub fn process_tick(&mut self) {
        if self.tick == 0 {
            if self.row_repeat > 0 {
                self.row_repeat -= 1;
                self.process_row(true);
            } else {
                self.row_repeat_set = false;
                self.extra_ticks = 0;
                self.advance_row();
                if !self.playing {
                    return;
                }
                self.process_row(false);
            }
        } else {
            self.process_effects_tick();
        }

        self.update_voices();

        self.tick += 1;
        if self.tick >= self.speed + self.extra_ticks {
            self.tick = 0;
        }
    }

    fn process_row(&mut self, repeat: bool) {
        let nch = self.m.num_channels;
        let pat = self.pattern;
        let row = self.row;
        for ch in 0..nch {
            if self.m.chn_disabled.get(ch).copied().unwrap_or(false) {
                continue;
            }
            let cell = *self.m.patterns[pat].cell(row, ch);
            let c = &mut self.chn[ch];
            c.vibrato_on = false;
            c.fine_vib = false;
            c.tremolo_on = false;
            c.panbrello_on = false;
            c.arp_on = false;
            c.tremor_on = false;
            c.note_cut_tick = None;
            c.delayed = None;

            // note delay (SDx)
            if !repeat && cell.cmd == 19 {
                let p = if cell.param == 0 { c.mem_s } else { cell.param };
                if p >> 4 == 0xD {
                    let t = (p & 0x0F) as u32;
                    if t > 0 {
                        c.mem_s = p;
                        c.delayed = Some((cell, t));
                        c.cmd = 0;
                        c.volcmd = VOL_NONE;
                        continue;
                    }
                }
            }
            self.handle_cell(ch, cell, !repeat);
        }
    }

    fn process_effects_tick(&mut self) {
        let nch = self.m.num_channels;
        for ch in 0..nch {
            // delayed note
            if let Some((cell, t)) = self.chn[ch].delayed {
                if t == self.tick {
                    self.chn[ch].delayed = None;
                    self.handle_cell(ch, cell, true);
                    continue;
                }
            }
            if let Some(t) = self.chn[ch].note_cut_tick {
                if t == self.tick {
                    self.note_cut(ch);
                }
            }
            self.volcol_tick(ch);
            self.effect_tick(ch);
        }
    }

    // ------------------------------------------------------------------
    // notes and instruments
    // ------------------------------------------------------------------

    fn freq_of(&self, note: u8, c5: u32) -> f64 {
        if self.m.format == Format::S3m {
            // ST3: Amiga-style periods (OpenMPT GetPeriodFromNote / GetFreqFromPeriod)
            const TAB: [u64; 12] = [1712, 1616, 1524, 1440, 1356, 1280, 1208, 1140, 1076, 1016, 960, 907];
            let n = note.saturating_sub(1) as u64;
            let c5 = if c5 == 0 { 8363 } else { c5 as u64 };
            let period = (8363 * (TAB[(n % 12) as usize] << 5)) / (c5 << (n / 12)).max(1);
            return self.clamp_s3m(S3M_K / period.max(1) as f64);
        }
        c5 as f64 * ((note as f64 - 61.0) / 12.0).exp2()
    }

    /// ST3 period limits (64..32767, or the Amiga range with the "Amiga limits" flag)
    fn clamp_s3m(&self, freq: f64) -> f64 {
        if freq <= 0.0 {
            return freq;
        }
        let (lo, hi) = if self.m.amiga_limits { (113.0 * 4.0, 856.0 * 4.0) } else { (64.0, 32767.0) };
        (S3M_K / (S3M_K / freq).clamp(lo, hi)).max(0.0)
    }

    fn handle_cell(&mut self, ch: usize, cell: Cell, allow_note: bool) {
        let porta = cell.cmd == 7 || cell.cmd == 12 || (193..=202).contains(&cell.vol);
        {
            let c = &mut self.chn[ch];
            c.cmd = cell.cmd;
            c.param = cell.param;
            c.volcmd = cell.vol;
        }

        if allow_note {
            // ---- instrument number ----
            if cell.ins != 0 {
                self.chn[ch].last_ins = cell.ins;
            }
            match cell.note {
                NOTE_OFF => self.key_off_voice(ch),
                NOTE_CUT => self.note_cut(ch),
                NOTE_FADE => self.chn[ch].note_fade = true,
                n @ 1..=120 => self.note_on(ch, n, cell.ins, porta),
                _ => {
                    if cell.ins != 0 {
                        self.instrument_only(ch, cell.ins);
                    }
                }
            }
        }

        // ---- volume column (first tick) ----
        self.volcol_first(ch, cell.vol);
        // ---- effect (first tick) ----
        self.effect_first(ch, cell);
    }

    /// Resolves (sample index, mapped note) for a pattern note + instrument number
    fn map_note(&self, note: u8, ins: u8) -> Option<(usize, u8, Option<usize>)> {
        if ins == 0 {
            return None;
        }
        if self.m.instrument_mode {
            let ii = ins as usize - 1;
            let inst = self.m.instruments.get(ii)?;
            let (n, s) = inst.keyboard[(note - 1) as usize];
            if s == 0 {
                return None;
            }
            let si = s as usize - 1;
            if si >= self.m.samples.len() || self.m.samples[si].length == 0 {
                return None;
            }
            Some((si, n + 1, Some(ii)))
        } else {
            let si = ins as usize - 1;
            if si >= self.m.samples.len() || self.m.samples[si].length == 0 {
                return None;
            }
            Some((si, note, None))
        }
    }

    fn note_on(&mut self, ch: usize, note: u8, cell_ins: u8, porta: bool) {
        let ins_num = if cell_ins != 0 { cell_ins } else { self.chn[ch].last_ins };
        let Some((si, mapped, ii)) = self.map_note(note, ins_num) else {
            // no sample for this note: IT cuts the note
            if cell_ins != 0 || self.m.instrument_mode {
                self.note_cut(ch);
            }
            return;
        };
        let smp_c5 = self.m.samples[si].c5speed;
        let target = self.freq_of(mapped, smp_c5);

        // ---- tone portamento: no retrigger ----
        if porta && self.chn[ch].active && !self.chn[ch].kill {
            let cy = self.env_carry(ii);
            let c = &mut self.chn[ch];
            c.porta_target = target;
            c.note = mapped;
            c.pattern_note = note;
            if cell_ins != 0 {
                if let Some(i) = ii.map(|i| &self.m.instruments[i]) {
                    if let Some(x) = i.cutoff {
                        c.cutoff = x;
                    }
                    if let Some(x) = i.resonance {
                        c.resonance = x;
                    }
                }
                c.volume = self.m.samples[c.sample].volume as i32 * 4;
                c.fade_vol = 65536;
                c.note_fade = false;
                c.key_off = false;
                reset_envs(c, cy);
            }
            return;
        }

        // ---- New Note Action for the currently playing note ----
        self.apply_nna(ch, si, ii, note);

        let s = &self.m.samples[si];
        let (svol, span, sgv) = (s.volume, s.pan, s.global_vol);
        let inst = ii.map(|i| &self.m.instruments[i]);
        let (igv, ipan, nna, pps, ppc, vsw, psw) = match inst {
            Some(i) => (i.global_vol as i32, i.pan, i.nna, i.pps, i.ppc, i.vol_swing, i.pan_swing),
            None => (128, None, Nna::Cut, 0, 60, 0, 0),
        };
        let (icut, ires) = match inst {
            Some(i) => (i.cutoff, i.resonance),
            None => (None, None),
        };
        let (venv, penv, pienv) = match inst {
            Some(i) => (i.vol_env.enabled, i.pan_env.enabled, i.pitch_env.enabled),
            None => (false, false, false),
        };

        let r1 = self.rand();
        let r2 = self.rand();
        let cy = self.env_carry(ii);
        let c = &mut self.chn[ch];
        let new_sample = !c.active || c.sample != si;
        c.active = true;
        c.kill = false;
        c.ended = false;
        c.sample = si;
        c.ins = ii;
        c.note = mapped;
        c.pattern_note = note;
        c.pos = 0.0;
        c.backwards = false;
        c.freq = target;
        c.porta_target = target;
        if cell_ins != 0 || new_sample {
            c.volume = svol as i32 * 4;
        }
        c.ins_vol = (sgv as i32 * igv) / 128;
        if let Some(p) = ipan {
            c.pan = p as i32 * 4;
            c.surround = false;
        }
        if let Some(p) = span {
            c.pan = p as i32 * 4;
            c.surround = false;
        }
        if pps != 0 && ii.is_some() {
            let delta = (mapped as i32 - 1 - ppc as i32) * pps as i32 / 2;
            c.pan = (c.pan + delta).clamp(0, 256);
        }
        c.vol_swing = if vsw > 0 {
            let range = (c.volume * vsw as i32) / 100;
            (r1 % (2 * range as u32 + 1)) as i32 - range
        } else {
            0
        };
        c.pan_swing = if psw > 0 {
            let range = psw as i32 * 4;
            (r2 % (2 * range as u32 + 1)) as i32 - range
        } else {
            0
        };
        c.fade_vol = 65536;
        c.note_fade = false;
        c.key_off = false;
        reset_envs(c, cy);
        c.vol_env_on = venv;
        c.pan_env_on = penv;
        c.pitch_env_on = pienv;
        c.autovib_pos = 0;
        c.autovib_depth = 0;
        c.nna = nna;
        c.master = None;
        c.vib_pos = 0;
        c.trem_pos = 0;
        c.retrig_count = 0;
        c.tremor_count = 0;
        // start volume ramps from silence
        c.gain_l = 0.0;
        c.gain_r = 0.0;
        // filter: instrument settings apply on every new note; the filter is set up
        // again (with cleared history) in update_voices
        if let Some(x) = icut {
            c.cutoff = x;
        }
        if let Some(x) = ires {
            c.resonance = x;
        }
        c.filter_on = false;
        c.trigger_note = true;
    }

    fn instrument_only(&mut self, ch: usize, ins: u8) {
        let c = &self.chn[ch];
        if !c.active {
            return;
        }
        // reset volume to the sample's default
        let si = if self.m.instrument_mode {
            if c.note == 0 {
                return;
            }
            match self.map_note(c.pattern_note.max(1), ins) {
                Some((si, _, _)) => si,
                None => return,
            }
        } else {
            let si = ins as usize - 1;
            if si >= self.m.samples.len() {
                return;
            }
            si
        };
        let vol = self.m.samples[si].volume as i32 * 4;
        let cy = self.env_carry(if self.m.instrument_mode { Some(ins as usize - 1) } else { None });
        let c = &mut self.chn[ch];
        c.volume = vol;
        if c.note_fade || c.key_off {
            // IT: an instrument number restarts envelopes / fade of a released note
            c.note_fade = false;
            c.key_off = false;
            c.fade_vol = 65536;
            reset_envs(c, cy);
        }
    }

    /// Moves the note playing on host channel `ch` to a background voice according
    /// to its NNA, and applies duplicate checks.
    fn apply_nna(&mut self, ch: usize, new_si: usize, new_ii: Option<usize>, new_note: u8) {
        // duplicate check against background voices of this channel
        if let Some(ii) = new_ii {
            let (dct, dca) = (self.m.instruments[ii].dct, self.m.instruments[ii].dca);
            if dct != 0 {
                for v in MAX_CHANNELS..MAX_VOICES {
                    let b = &self.chn[v];
                    if !b.active || b.master != Some(ch) || b.ins != Some(ii) {
                        continue;
                    }
                    let dup = match dct {
                        1 => b.pattern_note == new_note,
                        2 => b.sample == new_si,
                        3 => true,
                        _ => false,
                    };
                    if dup {
                        match dca {
                            0 => self.chn[v].kill = true,
                            1 => self.key_off_at(v),
                            _ => self.chn[v].note_fade = true,
                        }
                    }
                }
            }
        }

        let c = &self.chn[ch];
        if !c.active {
            return;
        }
        // move the old note to a background voice (always, so cut notes can ramp out)
        let Some(slot) = self.free_voice() else { return };
        let mut bg = self.chn[ch].clone();
        bg.master = Some(ch);
        bg.vibrato_on = false;
        bg.tremolo_on = false;
        bg.panbrello_on = false;
        bg.arp_on = false;
        bg.tremor_on = false;
        bg.delayed = None;
        bg.note_cut_tick = None;
        match bg.nna {
            Nna::Cut => bg.kill = true,
            Nna::Continue => {}
            Nna::Off => {
                self.chn[slot] = bg;
                self.key_off_at(slot);
                self.chn[ch].active = false;
                return;
            }
            Nna::Fade => bg.note_fade = true,
        }
        if !self.m.instrument_mode {
            bg.kill = true;
        }
        self.chn[slot] = bg;
        self.chn[ch].active = false;
    }

    fn free_voice(&self) -> Option<usize> {
        if let Some(v) = (MAX_CHANNELS..MAX_VOICES).find(|&v| !self.chn[v].active) {
            return Some(v);
        }
        // steal the quietest background voice
        (MAX_CHANNELS..MAX_VOICES).min_by_key(|&v| self.chn[v].final_vol)
    }

    fn key_off_voice(&mut self, ch: usize) {
        self.key_off_at(ch);
    }

    fn key_off_at(&mut self, v: usize) {
        let c = &self.chn[v];
        let ins = c.ins.map(|i| &self.m.instruments[i]);
        let (venv_on, fade_on_off) = match ins {
            Some(i) => (c.vol_env_on && i.vol_env.enabled, i.vol_env.looped && i.fadeout != 0),
            None => (false, false),
        };
        let c = &mut self.chn[v];
        c.key_off = true;
        if c.ins.is_some() && !venv_on {
            c.note_fade = true;
        }
        if fade_on_off {
            c.note_fade = true;
        }
        if c.ins.is_none() && !self.m.instrument_mode {
            // sample mode: note off releases the sustain loop only
        }
    }

    fn note_cut(&mut self, ch: usize) {
        let c = &mut self.chn[ch];
        if c.active {
            c.volume = 0;
            c.kill = true;
        }
    }

    // ------------------------------------------------------------------
    // volume column
    // ------------------------------------------------------------------

    fn volcol_first(&mut self, ch: usize, v: u8) {
        if v == VOL_NONE {
            return;
        }
        let c = &mut self.chn[ch];
        match v {
            0..=64 => c.volume = v as i32 * 4,
            65..=74 => {
                // fine volume up
                let x = v - 65;
                let x = if x == 0 { c.mem_volcol_slide } else { x };
                c.mem_volcol_slide = x;
                c.volume = (c.volume + x as i32 * 4).min(256);
            }
            75..=84 => {
                let x = v - 75;
                let x = if x == 0 { c.mem_volcol_slide } else { x };
                c.mem_volcol_slide = x;
                c.volume = (c.volume - x as i32 * 4).max(0);
            }
            85..=94 => {
                if v - 85 != 0 {
                    c.mem_volcol_slide = v - 85;
                }
            }
            95..=104 => {
                if v - 95 != 0 {
                    c.mem_volcol_slide = v - 95;
                }
            }
            105..=114 | 115..=124 => {
                let x = if v >= 115 { v - 115 } else { v - 105 };
                if x != 0 {
                    c.mem_porta = x * 4;
                    if !self.m.compat_gxx {
                        c.mem_porta_g = x * 4;
                    }
                }
            }
            128..=192 => {
                c.pan = (v - 128) as i32 * 4;
                c.surround = false;
            }
            193..=202 => {
                let sp = PORTA_VOLCMD[(v - 193) as usize];
                if sp != 0 {
                    c.mem_porta_g = sp;
                    if !self.m.compat_gxx {
                        c.mem_porta = sp;
                    }
                }
            }
            203..=212 => {
                let x = v - 203;
                if x != 0 {
                    c.vib_depth = x * 4;
                }
                c.vibrato_on = true;
            }
            _ => {}
        }
    }

    fn volcol_tick(&mut self, ch: usize) {
        let v = self.chn[ch].volcmd;
        if v == VOL_NONE {
            return;
        }
        match v {
            85..=94 => {
                let c = &mut self.chn[ch];
                c.volume = (c.volume + c.mem_volcol_slide as i32 * 4).min(256);
            }
            95..=104 => {
                let c = &mut self.chn[ch];
                c.volume = (c.volume - c.mem_volcol_slide as i32 * 4).max(0);
            }
            105..=114 => {
                let amt = self.chn[ch].mem_porta as f64 * 4.0;
                self.freq_slide(ch, -amt);
            }
            115..=124 => {
                let amt = self.chn[ch].mem_porta as f64 * 4.0;
                self.freq_slide(ch, amt);
            }
            193..=202 => self.tone_porta(ch),
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // effects
    // ------------------------------------------------------------------

    fn freq_slide(&mut self, ch: usize, amount: f64) {
        let linear = self.m.linear_slides;
        let c = &mut self.chn[ch];
        if c.freq <= 0.0 || amount == 0.0 {
            return;
        }
        if linear {
            c.freq = slide_linear(c.freq, amount);
        } else {
            // IT Amiga slides (frequency domain)
            const K: f64 = 1712.0 * 8363.0;
            if amount < 0.0 {
                c.freq = K * c.freq / (c.freq * -amount + K);
            } else {
                let div = K - c.freq * amount;
                if div <= 0.0 {
                    c.freq = 0.0;
                    c.note_fade = true;
                    c.fade_vol = 0;
                } else {
                    c.freq = K * c.freq / div;
                }
            }
        }
        c.freq = c.freq.clamp(0.0, 1.0e7);
        if self.m.format == Format::S3m {
            let f = self.chn[ch].freq;
            self.chn[ch].freq = self.clamp_s3m(f);
        }
    }

    fn tone_porta(&mut self, ch: usize) {
        let linear = self.m.linear_slides;
        let c = &self.chn[ch];
        if !c.active || c.porta_target <= 0.0 || c.freq <= 0.0 {
            return;
        }
        let speed = c.mem_porta_g as f64 * 4.0;
        if speed == 0.0 {
            return;
        }
        let (freq, target) = (c.freq, c.porta_target);
        if freq < target {
            self.freq_slide(ch, speed);
            if self.chn[ch].freq >= target {
                self.chn[ch].freq = target;
            }
        } else if freq > target {
            self.freq_slide(ch, -speed);
            if self.chn[ch].freq <= target {
                self.chn[ch].freq = target;
            }
        }
        let _ = linear;
    }

    fn vol_slide_value(cur: i32, param: u8, first: bool, max: i32, scale: i32) -> i32 {
        let (hi, lo) = ((param >> 4) as i32, (param & 0x0F) as i32);
        let mut v = cur;
        if lo == 0x0F && hi != 0 {
            if first {
                v += hi * scale;
            }
            return v.clamp(0, max);
        }
        if hi == 0x0F && lo != 0 {
            if first {
                v -= lo * scale;
            }
            return v.clamp(0, max);
        }
        // D0F / DF0 also slide on the first tick
        if first && lo == 0x0F && hi == 0 {
            v -= 15 * scale;
        } else if first && hi == 0x0F && lo == 0 {
            v += 15 * scale;
        }
        if !first {
            if lo != 0 {
                if hi == 0 {
                    v -= lo * scale;
                }
            } else {
                v += hi * scale;
            }
        }
        v.clamp(0, max)
    }

    fn do_vol_slide(&mut self, ch: usize, first: bool) {
        let fast = self.m.fast_vol_slides;
        let c = &mut self.chn[ch];
        let p = c.mem_vol_slide;
        let (hi, lo) = (p >> 4, p & 0x0F);
        let fine = (lo == 0x0F && hi != 0) || (hi == 0x0F && lo != 0);
        // ST3 fast volume slides: normal slides also act on the first tick
        let first = first && !(fast && !fine);
        c.volume = Self::vol_slide_value(c.volume, p, first, 256, 4);
    }

    fn effect_first(&mut self, ch: usize, cell: Cell) {
        self.effect_first_inner(ch, cell);
        // ST3: one shared parameter memory for D/E/F/I/J/K/L/Q/R/S, updated by any
        // effect with a non-zero parameter
        if self.m.st3_quirks && cell.cmd != 0 && cell.param != 0 {
            let p = cell.param;
            let c = &mut self.chn[ch];
            c.mem_vol_slide = p;
            c.mem_porta = p;
            c.mem_tremor = p;
            c.mem_arp = p;
            c.mem_retrig = p;
            c.trem_depth = (p & 0x0F) << 2;
            c.trem_speed = p >> 4;
            c.mem_s = p;
        }
    }

    fn effect_first_inner(&mut self, ch: usize, cell: Cell) {
        let cmd = cell.cmd;
        let mut param = cell.param;
        match cmd {
            // Z: MIDI macro (evaluated in update_voices, after the note's filter setup)
            26 => self.chn[ch].pending_macro = Some(param),
            // A: set speed
            1 => {
                if param > 0 {
                    self.speed = param as u32;
                }
            }
            // B: jump to order
            2 => self.jump_order = Some(param as usize),
            // C: pattern break (ST3 ignores rows past the pattern end)
            3 => {
                if !(self.m.format == Format::S3m && param >= 64) {
                    self.break_row = Some(param as usize);
                }
            }
            // D: volume slide
            4 => {
                if param != 0 {
                    self.chn[ch].mem_vol_slide = param;
                }
                self.do_vol_slide(ch, true);
            }
            // E/F: portamento down/up
            5 | 6 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_porta = param;
                    if !self.m.compat_gxx {
                        c.mem_porta_g = param;
                    }
                } else {
                    param = c.mem_porta;
                }
                let sign = if cmd == 6 { 1.0 } else { -1.0 };
                if param >= 0xF0 {
                    self.freq_slide(ch, sign * ((param & 0x0F) as f64) * 4.0);
                } else if param >= 0xE0 {
                    self.freq_slide(ch, sign * (param & 0x0F) as f64);
                }
            }
            // G: tone portamento
            7 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_porta_g = param;
                    if !self.m.compat_gxx {
                        c.mem_porta = param;
                    }
                }
            }
            // H: vibrato
            8 => {
                let c = &mut self.chn[ch];
                if param & 0x0F != 0 {
                    c.vib_depth = (param & 0x0F) * 4;
                }
                if param & 0xF0 != 0 {
                    c.vib_speed = param >> 4;
                }
                c.vibrato_on = true;
            }
            // I: tremor
            9 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_tremor = param;
                }
                c.tremor_on = true;
            }
            // J: arpeggio
            10 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_arp = param;
                }
                c.arp_on = true;
            }
            // K: vibrato + volume slide
            11 => {
                if param != 0 {
                    self.chn[ch].mem_vol_slide = param;
                }
                self.chn[ch].vibrato_on = true;
                // ST3 does not run combined slides on the first tick
                if !self.m.st3_no_combined_first_tick {
                    self.do_vol_slide(ch, true);
                }
            }
            // L: tone portamento + volume slide
            12 => {
                if param != 0 {
                    self.chn[ch].mem_vol_slide = param;
                }
                if !self.m.st3_no_combined_first_tick {
                    self.do_vol_slide(ch, true);
                }
            }
            // M: channel volume
            13 => {
                if param <= 64 {
                    self.chn[ch].chn_vol = param as i32;
                }
            }
            // N: channel volume slide
            14 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_chn_vol_slide = param;
                }
                c.chn_vol = Self::vol_slide_value(c.chn_vol, c.mem_chn_vol_slide, true, 64, 1);
            }
            // O: sample offset (applied together with the note, see below)
            15 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_offset = param;
                }
                if (1..=120).contains(&cell.note) && c.active && c.pos == 0.0 {
                    let off = ((c.high_offset as usize) << 16) | ((c.mem_offset as usize) << 8);
                    let len = self.m.samples[c.sample].length;
                    if off < len {
                        c.pos = off as f64;
                    } else {
                        // IT: offsets beyond the sample end play from the end (silence)
                        c.pos = len.saturating_sub(1) as f64;
                    }
                }
            }
            // P: panning slide
            16 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_pan_slide = param;
                }
                self.pan_slide(ch, true);
            }
            // Q: retrigger
            17 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_retrig = param;
                }
                if !(1..=120).contains(&cell.note) {
                    self.retrig_tick(ch);
                } else {
                    let c = &mut self.chn[ch];
                    c.retrig_count = c.mem_retrig & 0x0F;
                }
            }
            // R: tremolo
            18 => {
                let c = &mut self.chn[ch];
                if param & 0x0F != 0 {
                    c.trem_depth = (param & 0x0F) << 2;
                }
                if param & 0xF0 != 0 {
                    c.trem_speed = param >> 4;
                }
                c.tremolo_on = true;
            }
            // S: extended
            19 => {
                if param == 0 {
                    param = self.chn[ch].mem_s;
                } else {
                    self.chn[ch].mem_s = param;
                }
                self.s_command(ch, param);
            }
            // T: tempo
            20 => {
                let c = &mut self.chn[ch];
                if param != 0 {
                    c.mem_tempo = param;
                } else {
                    param = c.mem_tempo;
                }
                if param >= 0x20 {
                    self.tempo = param as u32;
                }
            }
            // U: fine vibrato
            21 => {
                let s3m = self.m.format == Format::S3m;
                let c = &mut self.chn[ch];
                if param & 0x0F != 0 {
                    // ST3 shares the depth memory with H (stored x4, scaled back when applied)
                    c.vib_depth = (param & 0x0F) * if s3m { 4 } else { 1 };
                }
                c.fine_vib = s3m;
                if param & 0xF0 != 0 {
                    c.vib_speed = param >> 4;
                }
                c.vibrato_on = true;
            }
            // V: global volume (S3M: 0..64)
            22 => {
                if self.m.format == Format::S3m {
                    if param <= 64 {
                        self.global_vol = param as i32 * 2;
                    }
                } else if param <= 0x80 {
                    self.global_vol = param as i32;
                }
            }
            // W: global volume slide
            23 => {
                if param != 0 {
                    self.chn[ch].mem_gvol_slide = param;
                }
                let p = self.chn[ch].mem_gvol_slide;
                self.global_vol = Self::vol_slide_value(self.global_vol, p, true, 128, 1);
            }
            // X: set panning (S3M: 00..80, A4 = surround)
            24 => {
                let s3m = self.m.format == Format::S3m;
                let c = &mut self.chn[ch];
                if !s3m {
                    c.pan = param as i32;
                    c.surround = false;
                } else if param <= 0x80 {
                    c.pan = param as i32 * 2;
                    c.surround = false;
                } else if param == 0xA4 {
                    c.surround = true;
                    c.pan = 128;
                }
            }
            // Y: panbrello
            25 => {
                let c = &mut self.chn[ch];
                if param & 0x0F != 0 {
                    c.panb_depth = param & 0x0F;
                }
                if param & 0xF0 != 0 {
                    c.panb_speed = param >> 4;
                }
                c.panbrello_on = true;
            }
            _ => {}
        }
    }

    fn effect_tick(&mut self, ch: usize) {
        let (cmd, param) = (self.chn[ch].cmd, self.chn[ch].param);
        match cmd {
            4 => self.do_vol_slide(ch, false),
            5 | 6 => {
                let p = self.chn[ch].mem_porta;
                if p < 0xE0 {
                    let sign = if cmd == 6 { 1.0 } else { -1.0 };
                    self.freq_slide(ch, sign * p as f64 * 4.0);
                }
            }
            7 => self.tone_porta(ch),
            11 => self.do_vol_slide(ch, false),
            12 => {
                self.tone_porta(ch);
                self.do_vol_slide(ch, false);
            }
            14 => {
                let c = &mut self.chn[ch];
                c.chn_vol = Self::vol_slide_value(c.chn_vol, c.mem_chn_vol_slide, false, 64, 1);
            }
            16 => self.pan_slide(ch, false),
            17 => self.retrig_tick(ch),
            19 => {
                // SCx note cut handled via note_cut_tick
            }
            20 => {
                let p = self.chn[ch].mem_tempo;
                if p < 0x10 {
                    self.tempo = self.tempo.saturating_sub((p & 0x0F) as u32).max(32);
                } else if p < 0x20 {
                    self.tempo = (self.tempo + (p & 0x0F) as u32).min(255);
                }
            }
            23 => {
                let p = self.chn[ch].mem_gvol_slide;
                self.global_vol = Self::vol_slide_value(self.global_vol, p, false, 128, 1);
            }
            _ => {}
        }
        let _ = param;
    }

    fn pan_slide(&mut self, ch: usize, first: bool) {
        let c = &mut self.chn[ch];
        let p = c.mem_pan_slide;
        let (hi, lo) = ((p >> 4) as i32, (p & 0x0F) as i32);
        let mut d = 0;
        if lo == 0x0F && hi != 0 {
            if first {
                d = -hi * 4;
            }
        } else if hi == 0x0F && lo != 0 {
            if first {
                d = lo * 4;
            }
        } else if !first {
            if lo != 0 {
                if hi == 0 {
                    d = lo * 4;
                }
            } else {
                d = -hi * 4;
            }
        }
        if d != 0 {
            c.pan = (c.pan + d).clamp(0, 256);
            c.surround = false;
        }
    }

    fn retrig_tick(&mut self, ch: usize) {
        let c = &mut self.chn[ch];
        if !c.active {
            return;
        }
        let p = c.mem_retrig;
        let interval = (p & 0x0F).max(1);
        if c.retrig_count == 0 {
            c.retrig_count = interval;
        }
        c.retrig_count -= 1;
        if c.retrig_count == 0 {
            c.retrig_count = interval;
            c.pos = 0.0;
            c.backwards = false;
            c.ended = false;
            let v = c.volume;
            c.volume = match p >> 4 {
                1 => v - 4,
                2 => v - 8,
                3 => v - 16,
                4 => v - 32,
                5 => v - 64,
                6 => v * 2 / 3,
                7 => v / 2,
                9 => v + 4,
                10 => v + 8,
                11 => v + 16,
                12 => v + 32,
                13 => v + 64,
                14 => v * 3 / 2,
                15 => v * 2,
                _ => v,
            }
            .clamp(0, 256);
        }
    }

    fn s_command(&mut self, ch: usize, param: u8) {
        let x = param & 0x0F;
        match param >> 4 {
            0xF => self.chn[ch].active_macro = x,
            0x1 => self.chn[ch].glissando = x != 0,
            0x3 => self.chn[ch].vib_type = x & 3,
            0x4 => self.chn[ch].trem_type = x & 3,
            0x5 => {
                self.chn[ch].panb_type = x & 3;
                self.chn[ch].panb_pos = 0;
            }
            0x6 => {
                if self.tick == 0 {
                    self.extra_ticks += x as u32;
                }
            }
            0x7 => self.s7_command(ch, x),
            0x8 => {
                let c = &mut self.chn[ch];
                c.pan = (x as i32 * 0x11).min(256);
                c.surround = false;
            }
            0x9 => {
                if x == 1 {
                    let c = &mut self.chn[ch];
                    c.surround = true;
                    c.pan = 128;
                }
            }
            0xA => self.chn[ch].high_offset = x,
            0xB => {
                if self.tick != 0 {
                    return;
                }
                let row = self.row;
                // ST3 keeps one pattern-loop state for all channels
                let ch = if self.m.format == Format::S3m { 0 } else { ch };
                if x == 0 {
                    self.chn[ch].loop_row = row;
                    return;
                }
                if self.chn[ch].loop_count > 0 {
                    self.chn[ch].loop_count -= 1;
                    if self.chn[ch].loop_count == 0 {
                        // IT: the next loop starts on the row after this loop
                        self.chn[ch].loop_row = row + 1;
                        return;
                    }
                } else {
                    self.chn[ch].loop_count = x;
                }
                self.loop_jump = Some(self.chn[ch].loop_row);
                // IT: SBx wins over a position jump to its left
                if self.m.format != Format::S3m {
                    self.jump_order = None;
                }
            }
            0xC => {
                let t = if x == 0 { 1 } else { x as u32 };
                self.chn[ch].note_cut_tick = Some(t);
            }
            0xE => {
                if self.tick == 0 && !self.row_repeat_set {
                    self.row_repeat = x as u32;
                    self.row_repeat_set = true;
                }
            }
            _ => {}
        }
    }

    fn s7_command(&mut self, ch: usize, x: u8) {
        match x {
            0..=2 => {
                // past note actions on background voices of this channel
                for v in MAX_CHANNELS..MAX_VOICES {
                    if self.chn[v].active && self.chn[v].master == Some(ch) {
                        match x {
                            0 => self.chn[v].kill = true,
                            1 => self.key_off_at(v),
                            _ => self.chn[v].note_fade = true,
                        }
                    }
                }
            }
            3 => self.chn[ch].nna = Nna::Cut,
            4 => self.chn[ch].nna = Nna::Continue,
            5 => self.chn[ch].nna = Nna::Off,
            6 => self.chn[ch].nna = Nna::Fade,
            7 => self.chn[ch].vol_env_on = false,
            8 => self.chn[ch].vol_env_on = true,
            9 => self.chn[ch].pan_env_on = false,
            10 => self.chn[ch].pan_env_on = true,
            11 => self.chn[ch].pitch_env_on = false,
            12 => self.chn[ch].pitch_env_on = true,
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // per-tick voice update (volume, pitch, envelopes)
    // ------------------------------------------------------------------

    fn update_voices(&mut self) {
        let sep = self.m.separation as i32;
        let stereo = self.m.stereo;
        for v in 0..MAX_VOICES {
            if !self.chn[v].active {
                continue;
            }
            let is_host = v < MAX_CHANNELS;
            let host = if is_host { v } else { self.chn[v].master.unwrap_or(0) };
            let muted = host < MAX_CHANNELS && self.muted[host];

            let mut c = std::mem::take(&mut self.chn[v]);

            // ---- volume ----
            let mut vol = c.volume;
            if c.vol_swing != 0 {
                vol += c.vol_swing;
            }
            if is_host && c.tremolo_on {
                let delta = wave(c.trem_type, c.trem_pos, &mut self.rng);
                vol += (delta * c.trem_depth as i32) / 32;
                c.trem_pos = c.trem_pos.wrapping_add(4u8.wrapping_mul(c.trem_speed));
            }
            if is_host && c.tremor_on {
                let p = c.mem_tremor;
                let on = (p >> 4).max(1);
                let off = (p & 0x0F).max(1);
                if c.tremor_count == 0 {
                    c.tremor_count = on + off;
                }
                let pos = (on + off) - c.tremor_count;
                if pos >= on {
                    vol = 0;
                }
                c.tremor_count -= 1;
            }
            let mut vol = vol.clamp(0, 256) << 6; // 0..16384

            let ins = c.ins.map(|i| &self.m.instruments[i]);
            let mut pan = c.pan + c.pan_swing;
            if let Some(ins) = ins {
                // envelopes (IT: position advances before it is used)
                if c.vol_env_on && ins.vol_env.enabled {
                    let e = &ins.vol_env;
                    let (pos, end_reached) = env_step(e, c.vol_env_pos, c.key_off);
                    c.vol_env_pos = pos;
                    let val = e.value_at(pos.saturating_sub(1)) as f32;
                    vol = (vol as f32 * val / 64.0) as i32;
                    if end_reached {
                        c.note_fade = true;
                        if e.nodes.last().map(|n| n.value == 0).unwrap_or(false) {
                            c.fade_vol = 0;
                        }
                    }
                }
                if c.note_fade {
                    if ins.fadeout != 0 {
                        c.fade_vol = (c.fade_vol - ins.fadeout as i32 * 2).max(0);
                    }
                    vol = ((vol as i64 * c.fade_vol as i64) >> 16) as i32;
                }
                if c.pan_env_on && ins.pan_env.enabled {
                    let e = &ins.pan_env;
                    let (pos, _) = env_step(e, c.pan_env_pos, c.key_off);
                    c.pan_env_pos = pos;
                    let val = e.value_at(pos.saturating_sub(1)) as i32; // -32..32
                    pan = pan.clamp(0, 256);
                    pan += if pan >= 128 { (val * (256 - pan)) / 32 } else { (val * pan) / 32 };
                }
            } else if c.note_fade {
                c.fade_vol = 0;
                vol = 0;
            }

            if is_host && c.panbrello_on {
                let d = wave(c.panb_type, c.panb_pos, &mut self.rng);
                c.panb_offset = d;
                c.panb_pos = c.panb_pos.wrapping_add(c.panb_speed);
                pan += (d * c.panb_depth as i32 + 2) / 8;
            }

            let real_vol = ((vol as i64 * self.global_vol as i64 * 2 * c.chn_vol as i64 * c.ins_vol as i64) >> 20) as i32;
            c.final_vol = real_vol;

            // ---- pitch ----
            let mut freq = c.freq;
            if is_host && c.arp_on {
                let p = c.mem_arp;
                let step = match self.tick % 3 {
                    1 => p >> 4,
                    2 => p & 0x0F,
                    _ => 0,
                };
                if step != 0 {
                    freq *= (step as f64 / 12.0).exp2();
                }
            }
            if is_host && c.vibrato_on {
                if self.m.format == Format::S3m {
                    // ST3: period vibrato, position advances after use (not on the first tick)
                    let delta = wave(c.vib_type, c.vib_pos, &mut self.rng);
                    let shift = if self.m.st2_vibrato { 5 } else { 6 } + if c.fine_vib { 2 } else { 0 };
                    let amount = (delta * c.vib_depth as i32) >> shift;
                    if freq > 0.0 {
                        let period = (S3M_K / freq - amount as f64).max(1.0);
                        freq = S3M_K / period;
                    }
                    if self.tick != 0 {
                        c.vib_pos = c.vib_pos.wrapping_add(4u8.wrapping_mul(c.vib_speed));
                    }
                } else {
                    c.vib_pos = c.vib_pos.wrapping_add(4u8.wrapping_mul(c.vib_speed));
                    let delta = wave(c.vib_type, c.vib_pos, &mut self.rng);
                    let amount = (delta * c.vib_depth as i32) / 64;
                    freq = slide_linear(freq, amount as f64);
                }
            }
            // sample auto-vibrato
            let s = &self.m.samples[c.sample];
            if s.vib_speed != 0 && s.vib_depth != 0 {
                let pos = (c.autovib_pos & 0xFF) as u8;
                c.autovib_depth = (c.autovib_depth + s.vib_rate as i32).min(s.vib_depth as i32 * 256);
                c.autovib_pos = c.autovib_pos.wrapping_add(s.vib_speed as u32);
                let d = match s.vib_type {
                    1 => 64 - (pos as i32 + 1) / 2,
                    2 => {
                        if pos < 128 {
                            64
                        } else {
                            0
                        }
                    }
                    3 => (self.rng % 128) as i32 - 64,
                    _ => ITSIN[pos as usize] as i32,
                };
                let amount = (d * (c.autovib_depth / 256)) / 64;
                freq = slide_linear(freq, amount as f64);
            }
            if let Some(ins) = ins {
                if c.pitch_env_on && ins.pitch_env.enabled && !ins.pitch_env.filter {
                    let e = &ins.pitch_env;
                    let (pos, _) = env_step(e, c.pitch_env_pos, c.key_off);
                    c.pitch_env_pos = pos;
                    let val = e.value_at(pos.saturating_sub(1)); // -32..32 => +-16 semitones
                    freq *= (val as f64 / 24.0).exp2();
                }
            }
            c.inc = if freq > 0.0 { freq / self.rate as f64 } else { 0.0 };

            // ---- resonant filter ----
            // order as in OpenMPT: new-note setup, then Zxx macros, then filter envelope
            let fc = FilterCfg { rate: self.rate, ext_range: self.m.ext_filter_range };
            if c.trigger_note {
                setup_filter(&mut c, true, 512, &fc);
            }
            if is_host {
                if let Some(param) = c.pending_macro.take() {
                    let mac = if param < 0x80 { &self.m.midi_sfx[c.active_macro as usize & 15] } else { &self.m.midi_zxx[(param & 0x7F) as usize] };
                    run_macro(&mut c, mac, param, &fc);
                }
            }
            if let Some(ins) = ins {
                let e = &ins.pitch_env;
                if e.enabled && e.filter && !e.nodes.is_empty() {
                    let pos = if c.pitch_env_on {
                        let (pos, _) = env_step(e, c.pitch_env_pos, c.key_off);
                        c.pitch_env_pos = pos;
                        pos
                    } else {
                        c.pitch_env_pos
                    };
                    // envelope value -32..32 -> modifier -256..256 (0 = cutoff as set)
                    let env_mod = if pos == 0 { 0 } else { (e.value_at(pos - 1) * 8.0).round() as i32 };
                    let reset = !c.filter_on;
                    setup_filter(&mut c, reset, env_mod + 256, &fc);
                }
            }
            c.trigger_note = false;

            // ---- panning / gains ----
            let mut p = if c.surround { 128 } else { pan.clamp(0, 256) };
            if !stereo {
                p = 128;
            } else {
                p = 128 + ((p - 128) * sep) / 128;
            }
            let vol_f = if muted || c.kill { 0.0 } else { real_vol as f32 / 16384.0 };
            // linear panning (same law as OpenMPT's default mixer); surround = inverted right side
            c.target_l = vol_f * (256 - p) as f32 / 256.0;
            c.target_r = vol_f * p as f32 / 256.0 * if c.surround && stereo { -1.0 } else { 1.0 };

            // stop finished notes
            if c.note_fade && c.fade_vol == 0 && c.gain_l == 0.0 && c.gain_r == 0.0 {
                c.active = false;
            }
            if c.kill && c.gain_l == 0.0 && c.gain_r == 0.0 {
                c.active = false;
            }
            self.chn[v] = c;
        }
    }

    // ------------------------------------------------------------------
    // mixing
    // ------------------------------------------------------------------

    /// Renders interleaved stereo f32 samples (unclipped), advancing the song.
    pub fn render_f32(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            if self.samples_to_tick <= 0.0 {
                if self.playing {
                    self.process_tick();
                }
                let spt = ((self.rate as u64 * 5) / (self.tempo.max(1) as u64 * 2)).max(1) as f64;
                self.samples_to_tick += spt;
                if self.playing {
                    self.seconds += spt / self.rate as f64;
                }
            }
            let n = (self.samples_to_tick.ceil() as usize).clamp(1, frames - done);
            self.mix(&mut out[done * 2..(done + n) * 2]);
            self.samples_to_tick -= n as f64;
            done += n;
        }
    }

    fn mix(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let step = self.ramp_step;
        let gain = self.master_gain;
        for v in 0..MAX_VOICES {
            if !self.chn[v].active {
                continue;
            }
            let c = &mut self.chn[v];
            let s = &self.m.samples[c.sample];
            let data_l = &s.data_l;
            let data_r = s.data_r.as_deref();
            let len = s.length.min(data_l.len()).min(data_r.map_or(usize::MAX, |r| r.len()));
            if len == 0 {
                c.active = false;
                continue;
            }
            // which loop applies
            let (has_loop, ls, le, pp) = if s.sus_loop && !c.key_off {
                (true, s.sus_start, s.sus_end, s.sus_pingpong)
            } else if s.looped {
                (true, s.loop_start, s.loop_end, s.pingpong)
            } else {
                (false, 0, len, false)
            };
            let end = if has_loop { le.min(len) } else { len };
            let lp = Loop { has_loop, ls, end, pp, ls_f: ls as f64, end_f: end as f64 };
            let filter = c.filter_on;

            let mut f = 0;
            while f < frames {
                // fast path: no ramp, not ended, and a run of frames that cannot reach a loop
                // point or the sample end -- no per-frame checks inside it
                if !c.ended && c.gain_l == c.target_l && c.gain_r == c.target_r && c.pos >= 0.0 {
                    let k = safe_run(c, &lp).min(frames - f);
                    if k > 0 {
                        let g = (c.gain_l * gain / 32768.0, c.gain_r * gain / 32768.0);
                        let o = &mut out[f * 2..(f + k) * 2];
                        match (data_r, filter) {
                            (None, false) => run_mono::<false>(c, data_l, o, g),
                            (None, true) => run_mono::<true>(c, data_l, o, g),
                            (Some(r), false) => run_stereo::<false>(c, data_l, r, o, g),
                            (Some(r), true) => run_stereo::<true>(c, data_l, r, o, g),
                        }
                        f += k;
                        continue;
                    }
                }
                // slow path: one frame with ramps and loop / end handling
                if !slow_frame(c, data_l, data_r, &lp, &mut out[f * 2..f * 2 + 2], step, gain) {
                    break;
                }
                f += 1;
            }
            if c.ended && c.gain_l == 0.0 && c.gain_r == 0.0 {
                c.active = false;
            }
            if c.kill && c.gain_l == 0.0 && c.gain_r == 0.0 {
                c.active = false;
            }
        }
    }

    pub fn play_from(&mut self, order: usize) {
        self.reset(order);
        self.playing = true;
    }

    /// The sample waveform around the current play position of host channel `ch`
    /// (or its newest background voice), for the scopes. Returns (40 points, volume 0..64).
    pub fn scope(&self, ch: usize) -> Option<([i8; 40], u8)> {
        let c = if self.chn[ch].active && !self.chn[ch].kill {
            &self.chn[ch]
        } else {
            return None;
        };
        let s = &self.m.samples[c.sample];
        if s.length == 0 || c.ended {
            return None;
        }
        let mut out = [0i8; 40];
        // one video frame worth of output across 40 pixels
        let stride = (c.inc * self.rate as f64 / 60.0 / 40.0).max(1.0);
        let (has_loop, ls, le) = if s.sus_loop && !c.key_off {
            (true, s.sus_start, s.sus_end)
        } else if s.looped {
            (true, s.loop_start, s.loop_end)
        } else {
            (false, 0, s.length)
        };
        let mut p = c.pos;
        for o in out.iter_mut() {
            let i = p as usize;
            *o = (s.data_l.get(i).copied().unwrap_or(0) >> 8) as i8;
            p += stride;
            if p >= le as f64 {
                if has_loop && le > ls {
                    p = ls as f64 + (p - le as f64) % (le - ls) as f64;
                } else {
                    p = le as f64;
                }
            }
        }
        let vol = ((c.final_vol as f32 / 16384.0) * 64.0).clamp(0.0, 64.0) as u8;
        Some((out, vol))
    }

    /// Level (0..1) of host channel `ch` including its background voices.
    pub fn level(&self, ch: usize) -> f32 {
        let mut v = 0i32;
        if self.chn[ch].active && !self.chn[ch].kill {
            v = self.chn[ch].final_vol;
        }
        (v as f32 / 16384.0).clamp(0.0, 1.0)
    }
}

/// IT vibrato/tremolo/panbrello waveforms (-64..64)
/// 32.32 fixed point used by the mixing runs
const FX: f64 = 4294967296.0;
const FX_INV: f32 = 1.0 / 4294967296.0;

/// the loop (or sample end) a voice is playing against
struct Loop {
    has_loop: bool,
    ls: usize,
    end: usize,
    pp: bool,
    ls_f: f64,
    end_f: f64,
}

/// how many frames can be mixed from the current position without crossing a loop point or
/// the end (with one frame of margin against rounding), so the inner loop needs no checks
#[inline]
fn safe_run(c: &Chn, lp: &Loop) -> usize {
    let last = lp.end_f - 1.0; // interpolation reads ip and ip + 1
    if c.pos >= last {
        return 0;
    }
    if c.inc <= 0.0 {
        return usize::MAX;
    }
    let room = if c.backwards { c.pos - lp.ls_f } else { last - c.pos };
    if room <= 0.0 {
        return 0;
    }
    let k = (room / c.inc).floor();
    if k < 2.0 {
        0
    } else if k > 1e9 {
        usize::MAX
    } else {
        k as usize - 1
    }
}

#[inline(always)]
fn filter_step(x: f32, y: &mut [f32; 2], a0: f32, b0: f32, b1: f32, hp: bool) -> f32 {
    // 2-pole resonant filter; history clipped to twice the 16-bit range
    let val = a0 * x + b0 * y[0].clamp(-65536.0, 65534.0) + b1 * y[1].clamp(-65536.0, 65534.0);
    y[1] = y[0];
    y[0] = if hp { val - x } else { val };
    val
}

fn run_mono<const FILTER: bool>(c: &mut Chn, d: &[i16], out: &mut [f32], (gl, gr): (f32, f32)) {
    let (a0, b0, b1, hp) = (c.filter_a0, c.filter_b0, c.filter_b1, c.filter_hp);
    let mut y = c.filter_y[0];
    // 32.32 fixed-point position inside the run; safe_run keeps ip + 1 inside the data
    let (mut p, inc, back) = ((c.pos * FX) as u64, (c.inc * FX) as u64, c.backwards);
    for o in out.chunks_exact_mut(2) {
        let ip = (p >> 32) as usize;
        let fr = (p as u32) as f32 * FX_INV;
        let (a, b) = unsafe { (*d.get_unchecked(ip) as f32, *d.get_unchecked(ip + 1) as f32) };
        let mut x = a + (b - a) * fr;
        if FILTER {
            x = filter_step(x, &mut y, a0, b0, b1, hp);
        }
        o[0] += x * gl;
        o[1] += x * gr;
        p = if back { p - inc } else { p + inc };
    }
    c.pos = p as f64 / FX;
    c.filter_y[0] = y;
}

fn run_stereo<const FILTER: bool>(c: &mut Chn, dl: &[i16], dr: &[i16], out: &mut [f32], (gl, gr): (f32, f32)) {
    let (a0, b0, b1, hp) = (c.filter_a0, c.filter_b0, c.filter_b1, c.filter_hp);
    let [mut yl, mut yr] = c.filter_y;
    let (mut p, inc, back) = ((c.pos * FX) as u64, (c.inc * FX) as u64, c.backwards);
    for o in out.chunks_exact_mut(2) {
        let ip = (p >> 32) as usize;
        let fr = (p as u32) as f32 * FX_INV;
        // safe_run keeps ip + 1 inside both channels' data
        let (l0, l1, r0, r1) = unsafe {
            (*dl.get_unchecked(ip) as f32, *dl.get_unchecked(ip + 1) as f32, *dr.get_unchecked(ip) as f32, *dr.get_unchecked(ip + 1) as f32)
        };
        let mut l = l0 + (l1 - l0) * fr;
        let mut r = r0 + (r1 - r0) * fr;
        if FILTER {
            l = filter_step(l, &mut yl, a0, b0, b1, hp);
            r = filter_step(r, &mut yr, a0, b0, b1, hp);
        }
        o[0] += l * gl;
        o[1] += r * gr;
        p = if back { p - inc } else { p + inc };
    }
    c.pos = p as f64 / FX;
    c.filter_y = [yl, yr];
}

/// one frame with volume ramping and loop / end handling; false when the voice is done for
/// this block (ended and ramped to silence)
fn slow_frame(c: &mut Chn, data_l: &[i16], data_r: Option<&[i16]>, lp: &Loop, o: &mut [f32], step: f32, gain: f32) -> bool {
    let Loop { has_loop, ls, end, pp, ls_f, end_f } = *lp;
    c.gain_l += (c.target_l - c.gain_l).clamp(-step, step);
    c.gain_r += (c.target_r - c.gain_r).clamp(-step, step);
    if c.ended {
        return !(c.gain_l == 0.0 && c.gain_r == 0.0);
    }
    let ip = c.pos as usize;
    let fr = (c.pos - ip as f64) as f32;
    let next_idx = if ip + 1 < end {
        ip + 1
    } else if has_loop {
        if pp { ip } else { ls }
    } else {
        ip
    };
    let a = data_l.get(ip).copied().unwrap_or(0) as f32;
    let b = data_l.get(next_idx).copied().unwrap_or(0) as f32;
    let mut sl = a + (b - a) * fr;
    let mut sr = match data_r {
        Some(r) => {
            let a = r.get(ip).copied().unwrap_or(0) as f32;
            let b = r.get(next_idx).copied().unwrap_or(0) as f32;
            a + (b - a) * fr
        }
        None => sl,
    };
    if c.filter_on {
        let (a0, b0, b1, hp) = (c.filter_a0, c.filter_b0, c.filter_b1, c.filter_hp);
        sl = filter_step(sl, &mut c.filter_y[0], a0, b0, b1, hp);
        sr = if data_r.is_some() { filter_step(sr, &mut c.filter_y[1], a0, b0, b1, hp) } else { sl };
    }
    o[0] += sl * c.gain_l * gain / 32768.0;
    o[1] += sr * c.gain_r * gain / 32768.0;

    // advance
    if c.backwards {
        c.pos -= c.inc;
        if c.pos < ls_f {
            c.pos = ls_f + (ls_f - c.pos);
            c.backwards = false;
        }
    } else {
        c.pos += c.inc;
    }
    if c.pos >= end_f {
        if has_loop && end > ls {
            if pp {
                c.pos = end_f - (c.pos - end_f) - 1.0;
                if c.pos < ls_f {
                    c.pos = ls_f;
                }
                c.backwards = true;
            } else {
                let span = end_f - ls_f;
                c.pos = ls_f + (c.pos - end_f) % span;
            }
        } else {
            c.ended = true;
            c.target_l = 0.0;
            c.target_r = 0.0;
        }
    }
    true
}

fn wave(kind: u8, pos: u8, rng: &mut u32) -> i32 {
    match kind & 3 {
        1 => 64 - (pos as i32 + 1) / 2,
        2 => {
            if pos < 128 {
                64
            } else {
                0
            }
        }
        3 => {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 17;
            *rng ^= *rng << 5;
            (*rng % 128) as i32 - 64
        }
        _ => ITSIN[pos as usize] as i32,
    }
}

/// IT envelope position handling: returns (new position (1-based), end reached)
fn env_step(e: &Envelope, stored: u32, key_off: bool) -> (u32, bool) {
    // `stored` is position+1 (0 = not started). The position used this tick is `stored`.
    let mut position = stored;
    let mut end_reached = false;
    let (start, end) = if e.sustain && !key_off {
        let (a, b) = e.sus_ticks();
        (a, b + 1)
    } else if e.looped {
        let (a, b) = e.loop_ticks();
        (a, b + 1)
    } else {
        let last = e.last_tick();
        if position > last {
            end_reached = true;
        }
        (last, last)
    };
    if position >= end {
        position = start;
    }
    (position + 1, end_reached)
}


/// Mixer settings the filter coefficients depend on.
struct FilterCfg {
    rate: u32,
    ext_range: bool,
}

/// OpenMPT's SetupChannelFilter (IT filter behaviour). `env` is 256 + OpenMPT's
/// envModifier (0..512): 512 without filter envelope, 256..512 / 0..256 from the envelope. Returns true if the filter is active.
fn setup_filter(c: &mut Chn, reset: bool, env: i32, fc: &FilterCfg) -> bool {
    let cutoff = c.cutoff.min(127) as i32;
    let resonance = (c.resonance & 0x7F) as i32;
    let env = env.clamp(0, 512);
    let computed = cutoff * env / 256;
    if resonance == 0 && computed >= 254 {
        // fully open filter without resonance: IT bypasses it (only a new note turns it off)
        if c.trigger_note {
            c.filter_on = false;
        }
        return false;
    }
    c.filter_on = true;

    let dmpfac = 10f32.powf(-(resonance as f32) * ((24.0 / 128.0) / 20.0));
    let x = (cutoff * env) as f32;
    let range = if fc.ext_range { 20.0 * 512.0 } else { 24.0 * 512.0 };
    let freq = (110.0 * 2f32.powf(0.25 + x / range)).clamp(120.0, 20000.0).min(fc.rate as f32 * 0.5);
    let w = freq * 2.0 * std::f32::consts::PI;
    let (d, e) = if !fc.ext_range {
        let r = fc.rate as f32 / w;
        (dmpfac * r + dmpfac - 1.0, r * r)
    } else {
        let r = w / fc.rate as f32;
        let d = ((1.0 - 2.0 * dmpfac) * r).min(2.0);
        ((2.0 * dmpfac - d) / r, 1.0 / (r * r))
    };
    let fg = 1.0 / (1.0 + d + e);
    let fb0 = (d + e + e) / (1.0 + d + e);
    let fb1 = -e / (1.0 + d + e);
    if c.filter_hp {
        c.filter_a0 = 1.0 - fg;
    } else {
        c.filter_a0 = fg;
    }
    c.filter_b0 = fb0;
    c.filter_b1 = fb1;
    if reset {
        c.filter_y = [[0.0; 2]; 2];
    }
    true
}

/// Evaluates a MIDI macro string for a Zxx command. Only OpenMPT's "internal device"
/// messages are meaningful here: F0 F0 00 xx (cutoff), F0 F0 01 xx (resonance),
/// F0 F0 02 xx (filter mode). External MIDI is ignored.
fn run_macro(c: &mut Chn, mac: &str, param: u8, fc: &FilterCfg) {
    let mut bytes: Vec<u8> = Vec::with_capacity(8);
    let mut nib: Option<u8> = None;
    let push_byte = |bytes: &mut Vec<u8>, nib: &mut Option<u8>, b: u8| {
        if let Some(h) = nib.take() {
            bytes.push(h << 4);
        }
        bytes.push(b);
    };
    for ch in mac.chars() {
        match ch {
            '0'..='9' | 'A'..='F' => {
                let v = ch.to_digit(16).unwrap() as u8;
                match nib.take() {
                    Some(h) => bytes.push((h << 4) | v),
                    None => nib = Some(v),
                }
            }
            // MIDI channel nibble (no MIDI output here)
            'c' => match nib.take() {
                Some(h) => bytes.push(h << 4),
                None => nib = Some(0),
            },
            'z' => push_byte(&mut bytes, &mut nib, param & 0x7F),
            'a'..='y' => push_byte(&mut bytes, &mut nib, 0),
            _ => {}
        }
    }
    if bytes.len() == 4 && bytes[0] == 0xF0 && bytes[1] == 0xF0 {
        let v = bytes[3];
        match bytes[2] {
            0x00 if v < 0x80 => {
                c.cutoff = v;
                let reset = !c.filter_on;
                setup_filter(c, reset, 512, fc);
            }
            0x01 if v < 0x80 => {
                c.resonance = v;
                let reset = !c.filter_on;
                setup_filter(c, reset, 512, fc);
            }
            0x02 if v < 0x20 => {
                c.filter_hp = v >> 4 == 1;
                let reset = !c.filter_on;
                setup_filter(c, reset, 512, fc);
            }
            _ => {}
        }
    }
}

/// Restarts the envelopes of a new note, except those with "carry" set
/// (they continue from where the previous note on this channel left off).
fn reset_envs(c: &mut Chn, carry: (bool, bool, bool)) {
    if !carry.0 {
        c.vol_env_pos = 0;
    }
    if !carry.1 {
        c.pan_env_pos = 0;
    }
    if !carry.2 {
        c.pitch_env_pos = 0;
    }
}
