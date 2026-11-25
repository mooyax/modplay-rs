//! FastTracker II replayer, ported from ft2-clone's ft2_replayer.c (tickReplayer,
//! getNewNote, effects, envelopes, auto-vibrato) including FT2's arithmetic quirks.
//! The mixer (linear interpolation, linear panning, volume ramps) is our own and is
//! tuned to match libopenmpt's output level.

use super::format::*;
use super::tables::*;

// channel status flags (ft2_replayer.h)
const CS_UPDATE_VOL: u8 = 1;
const CF_UPDATE_PERIOD: u8 = 2;
const CS_TRIGGER_VOICE: u8 = 4;
const CS_UPDATE_PAN: u8 = 8;
const CS_USE_QUICK_VOLRAMP: u8 = 16;

const ENV_ENABLED: u8 = 1;
const ENV_SUSTAIN: u8 = 2;
const ENV_LOOP: u8 = 4;

#[derive(Clone, Default)]
pub struct Channel {
    pub key_off: bool,
    pub channel_off: bool,
    pub mute: bool,
    pub semitone_porta: bool,
    pub status: u8,
    pub relative_note: i8,
    pub finetune: i8,
    pub smp_num: u8,
    pub instr_num: u8,
    pub efx_data: u8,
    pub efx: u8,
    pub sample_offset: u8,
    pub tremor_param: u8,
    pub tremor_pos: u8,
    pub glob_vol_slide_speed: u8,
    pub panning_slide_speed: u8,
    pub vib_trem_ctrl: u8,
    pub porta_direction: u8,
    pub vibrato_pos: u8,
    pub tremolo_pos: u8,
    pub vibrato_speed: u8,
    pub vibrato_depth: u8,
    pub tremolo_speed: u8,
    pub tremolo_depth: u8,
    pub loop_start_row: u8,
    pub loop_counter: u8,
    pub vol_slide_speed: u8,
    pub f_vol_slide_up: u8,
    pub f_vol_slide_down: u8,
    pub f_pitch_up: u8,
    pub f_pitch_down: u8,
    pub ef_pitch_up: u8,
    pub ef_pitch_down: u8,
    pub pitch_up: u8,
    pub pitch_down: u8,
    pub retrig_speed: u8,
    pub retrig_counter: u8,
    pub retrig_vol: u8,
    pub vol_column: u8,
    pub note_num: u8,
    pub pan_env_pos: u8,
    pub auto_vib_pos: u8,
    pub vol_env_pos: u8,
    pub real_vol: u8,
    pub old_vol: u8,
    pub out_vol: u8,
    pub old_pan: u8,
    pub out_pan: u8,
    pub final_pan: u8,
    pub vol_env_delta: i16,
    pub vol_env_value: i16,
    pub pan_env_delta: i16,
    pub pan_env_value: i16,
    pub out_period: u16,
    pub real_period: u16,
    pub final_period: u16,
    pub copy_of_instr_and_note: u16,
    pub porta_target: u16,
    pub porta_speed: u16,
    pub vol_env_tick: u16,
    pub pan_env_tick: u16,
    pub auto_vib_amp: u16,
    pub auto_vib_sweep: u16,
    pub fadeout_vol: u16,
    pub fadeout_speed: u16,
    pub smp_start_pos: i32,
    pub final_vol: f32,
    /// instrument index into XmModule::instruments (0 = empty placeholder)
    pub ins: usize,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    ins: usize,
    smp: usize,
    pos: f64,
    delta: f64,
    backwards: bool,
    loop_type: u8,
    loop_start: usize,
    loop_len: usize,
    sample_end: usize,
    volume: f32,
    panning: u8,
    cur_l: f32,
    cur_r: f32,
    tgt_l: f32,
    tgt_r: f32,
    dl: f32,
    dr: f32,
    ramp: u32,
    fade_voice: bool,
}

pub struct XmPlayer {
    pub m: XmModule,
    pub ch: Vec<Channel>,
    voices: Vec<Voice>,
    rate: u32,
    pub playing: bool,
    // song state
    pub song_pos: usize,
    pub row: usize,
    pub patt_num: usize,
    curr_num_rows: usize,
    pub speed: u16,
    pub bpm: u16,
    pub tick: u16,
    pub global_volume: u16,
    patt_del_time: u8,
    patt_del_time2: u8,
    p_break_pos: u8,
    p_break_flag: bool,
    pos_jump_flag: bool,
    bxx_overflow: bool,
    // position of the row being played (for the GUI)
    pub cur_pos: usize,
    pub cur_row: usize,
    pub cur_pattern: usize,
    samples_left: u32,
    pub seconds: f64,
    pub muted: Vec<bool>,
    gain: f32,
}

impl XmPlayer {
    pub fn new(m: XmModule, rate: u32) -> Self {
        let n = m.num_channels;
        let mut p = XmPlayer {
            m,
            ch: vec![Channel::default(); n.max(1)],
            // one voice per channel plus one for fading out the previous note
            voices: vec![Voice::default(); n.max(1) * 2],
            rate,
            playing: false,
            song_pos: 0,
            row: 0,
            patt_num: 0,
            curr_num_rows: 64,
            speed: 6,
            bpm: 125,
            tick: 1,
            global_volume: 64,
            patt_del_time: 0,
            patt_del_time2: 0,
            p_break_pos: 0,
            p_break_flag: false,
            pos_jump_flag: false,
            bxx_overflow: false,
            cur_pos: 0,
            cur_row: 0,
            cur_pattern: 0,
            samples_left: 0,
            seconds: 0.0,
            muted: vec![false; n.max(1)],
            gain: 0.0,
        };
        let _ = n;
        // libopenmpt's output level (sample pre-amp 48); the same for both mix levels
        p.gain = 48.0 / 256.0 * 2.0;
        p.play_from(0);
        p.playing = false;
        p
    }

    pub fn num_channels(&self) -> usize {
        self.m.num_channels
    }

    fn period_lut(&self) -> &'static [u16; 1936] {
        if self.m.linear {
            &LINEAR_PERIOD_LUT
        } else {
            &AMIGA_PERIOD_LUT
        }
    }

    fn instr(&self, idx: usize) -> &XmInstrument {
        self.m.instruments[idx].as_ref().unwrap()
    }

    // ------------------------------------------------------------------
    // song position
    // ------------------------------------------------------------------

    pub fn play_from(&mut self, pos: usize) {
        let pos = pos.min(self.m.song_length.saturating_sub(1));
        for (i, c) in self.ch.iter_mut().enumerate() {
            *c = Channel { status: CS_UPDATE_VOL, old_pan: 128, out_pan: 128, final_pan: 128, ..Default::default() };
            c.channel_off = false;
            let _ = i;
        }
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.patt_del_time = 0;
        self.patt_del_time2 = 0;
        self.pos_jump_flag = false;
        self.p_break_pos = 0;
        self.p_break_flag = false;
        self.bxx_overflow = false;
        self.global_volume = 64;
        self.speed = self.m.speed.clamp(1, 31);
        self.bpm = self.m.bpm.clamp(32, 255);
        self.song_pos = pos;
        self.row = 0;
        self.patt_num = self.m.orders[pos] as usize;
        self.curr_num_rows = self.pattern_rows(self.patt_num);
        self.tick = 1;
        self.samples_left = 0;
        self.seconds = 0.0;
        self.cur_pos = pos;
        self.cur_row = 0;
        self.cur_pattern = self.patt_num;
        self.playing = true;
    }

    fn pattern_rows(&self, p: usize) -> usize {
        self.m.patterns.get(p).map(|x| x.rows).unwrap_or(64)
    }

    // ------------------------------------------------------------------
    // notes and instruments
    // ------------------------------------------------------------------

    fn reset_volumes(c: &mut Channel) {
        c.real_vol = c.old_vol;
        c.out_vol = c.old_vol;
        c.out_pan = c.old_pan;
        c.status |= CS_UPDATE_VOL + CS_UPDATE_PAN + CS_USE_QUICK_VOLRAMP;
    }

    fn trigger_instrument(&mut self, i: usize) {
        let ins = self.instr(self.ch[i].ins).clone_header();
        let c = &mut self.ch[i];
        if c.vib_trem_ctrl & 0x04 == 0 {
            c.vibrato_pos = 0;
        }
        if c.vib_trem_ctrl & 0x40 == 0 {
            c.tremolo_pos = 0;
        }
        c.retrig_counter = 0;
        c.tremor_pos = 0;
        c.key_off = false;
        if ins.vol_flags & ENV_ENABLED != 0 {
            c.vol_env_tick = 65535;
            c.vol_env_pos = 0;
        }
        if ins.pan_flags & ENV_ENABLED != 0 {
            c.pan_env_tick = 65535;
            c.pan_env_pos = 0;
        }
        c.fadeout_speed = ins.fadeout;
        c.fadeout_vol = 32768;
        if ins.vib_depth > 0 {
            c.auto_vib_pos = 0;
            if ins.vib_sweep > 0 {
                c.auto_vib_amp = 0;
                c.auto_vib_sweep = ((ins.vib_depth as u16) << 8) / ins.vib_sweep as u16;
            } else {
                c.auto_vib_amp = (ins.vib_depth as u16) << 8;
                c.auto_vib_sweep = 0;
            }
        }
    }

    fn key_off(&mut self, i: usize) {
        let ins = self.instr(self.ch[i].ins).clone_header();
        let c = &mut self.ch[i];
        c.key_off = true;
        if ins.vol_flags & ENV_ENABLED != 0 {
            let t = ins.vol_env[c.vol_env_pos as usize % 12].0 as u16;
            if c.vol_env_tick >= t {
                c.vol_env_tick = t.wrapping_sub(1);
            }
        } else {
            c.real_vol = 0;
            c.out_vol = 0;
            c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
        }
        // FT2 logic bug: tests the wrong flag
        if ins.pan_flags & ENV_ENABLED == 0 {
            let t = ins.pan_env[c.pan_env_pos as usize % 12].0 as u16;
            if c.pan_env_tick >= t {
                c.pan_env_tick = t.wrapping_sub(1);
            }
        }
    }

    fn trigger_note(&mut self, mut note: u8, efx: u8, efx_data: u8, i: usize) {
        if note == NOTE_OFF {
            self.key_off(i);
            return;
        }
        if note == 0 {
            note = self.ch[i].note_num;
            if note == 0 {
                return;
            }
        }
        self.ch[i].note_num = note;
        let instr_num = self.ch[i].instr_num as usize;
        let ins_idx = if instr_num <= 128 && self.m.instruments.get(instr_num).map(|x| x.is_some()).unwrap_or(false) { instr_num } else { 0 };
        let lut = self.period_lut();
        let (smp_num, mute) = {
            let ins = self.instr(ins_idx);
            let n = note.min(96);
            (ins.note2sample[(n - 1) as usize] & 15, ins.mute)
        };
        let (rel, vol, pan, fine) = {
            let s = &self.instr(ins_idx).samples[smp_num as usize];
            (s.relative_note, s.volume, s.panning, s.finetune)
        };
        let c = &mut self.ch[i];
        c.ins = ins_idx;
        c.mute = mute;
        let note = note.min(96);
        c.smp_num = smp_num;
        c.relative_note = rel;
        let note = note as i16 + rel as i16;
        if !(0..120).contains(&note) {
            return;
        }
        c.old_vol = vol;
        c.old_pan = pan;
        c.finetune = if efx == 0xE && efx_data & 0xF0 == 0x50 { (((efx_data & 0x0F) as i16 * 16) - 128) as i8 } else { fine };
        if note != 0 {
            let idx = ((note - 1) * 16) as usize + ((c.finetune >> 3) + 16) as usize;
            c.real_period = lut[idx.min(1935)];
            c.out_period = c.real_period;
        }
        c.status |= CF_UPDATE_PERIOD + CS_UPDATE_VOL + CS_UPDATE_PAN + CS_TRIGGER_VOICE + CS_USE_QUICK_VOLRAMP;
        if efx == 9 {
            if efx_data > 0 {
                c.sample_offset = c.efx_data;
            }
            c.smp_start_pos = (c.sample_offset as i32) << 8;
        } else {
            c.smp_start_pos = 0;
        }
    }

    // ------------------------------------------------------------------
    // tick-zero effects
    // ------------------------------------------------------------------

    fn e_effects_tick_zero(&mut self, i: usize, param: u8) {
        let efx = param >> 4;
        let param = param & 0x0F;
        if self.ch[i].channel_off {
            if efx == 6 {
                self.pattern_loop(i, param);
            } else if efx == 0xE {
                self.pattern_delay(param);
            }
            return;
        }
        let c = &mut self.ch[i];
        match efx {
            1 => {
                let p = if param == 0 { c.f_pitch_up } else { param };
                c.f_pitch_up = p;
                c.real_period = c.real_period.wrapping_sub(p as u16 * 4);
                if (c.real_period as i16) < 1 {
                    c.real_period = 1;
                }
                c.out_period = c.real_period;
                c.status |= CF_UPDATE_PERIOD;
            }
            2 => {
                let p = if param == 0 { c.f_pitch_down } else { param };
                c.f_pitch_down = p;
                c.real_period = c.real_period.wrapping_add(p as u16 * 4);
                if (c.real_period as i16) >= 32000 {
                    c.real_period = 32000 - 1;
                }
                c.out_period = c.real_period;
                c.status |= CF_UPDATE_PERIOD;
            }
            3 => c.semitone_porta = param != 0,
            4 => c.vib_trem_ctrl = (c.vib_trem_ctrl & 0xF0) | param,
            6 => self.pattern_loop(i, param),
            7 => c.vib_trem_ctrl = (param << 4) | (c.vib_trem_ctrl & 0x0F),
            0xA => {
                let p = if param == 0 { c.f_vol_slide_up } else { param };
                c.f_vol_slide_up = p;
                c.real_vol = (c.real_vol + p).min(64);
                c.out_vol = c.real_vol;
                c.status |= CS_UPDATE_VOL;
            }
            0xB => {
                let p = if param == 0 { c.f_vol_slide_down } else { param };
                c.f_vol_slide_down = p;
                c.real_vol = c.real_vol.wrapping_sub(p);
                if (c.real_vol as i8) < 0 {
                    c.real_vol = 0;
                }
                c.out_vol = c.real_vol;
                c.status |= CS_UPDATE_VOL;
            }
            0xC => {
                if param == 0 {
                    c.real_vol = 0;
                    c.out_vol = 0;
                    c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
                }
            }
            0xE => self.pattern_delay(param),
            _ => {}
        }
    }

    fn pattern_loop(&mut self, i: usize, param: u8) {
        let row = self.row as u8;
        let c = &mut self.ch[i];
        if param == 0 {
            c.loop_start_row = row;
        } else if c.loop_counter == 0 {
            c.loop_counter = param;
            self.p_break_pos = c.loop_start_row;
            self.p_break_flag = true;
        } else {
            c.loop_counter -= 1;
            if c.loop_counter > 0 {
                self.p_break_pos = c.loop_start_row;
                self.p_break_flag = true;
            }
        }
    }

    fn pattern_delay(&mut self, param: u8) {
        if self.patt_del_time2 == 0 {
            self.patt_del_time = param + 1;
        }
    }

    fn set_envelope_pos(&mut self, i: usize, param: u8) {
        let ins = self.instr(self.ch[i].ins).clone_header();
        let c = &mut self.ch[i];
        if ins.vol_flags & ENV_ENABLED != 0 {
            let (pos, tick, delta, value) = env_set_pos(&ins.vol_env, ins.vol_len, param, c.vol_env_delta, c.vol_env_value);
            c.vol_env_tick = tick;
            c.vol_env_delta = delta;
            c.vol_env_value = value;
            c.vol_env_pos = pos;
        }
        // FT2 logic bug: should have tested the panning envelope flags
        if ins.vol_flags & ENV_SUSTAIN != 0 {
            let (pos, tick, delta, value) = env_set_pos(&ins.pan_env, ins.pan_len, param, c.pan_env_delta, c.pan_env_value);
            c.pan_env_tick = tick;
            c.pan_env_delta = delta;
            c.pan_env_value = value;
            c.pan_env_pos = pos;
        }
    }

    /// effects processed on tick 0 even for muted channels (B, D, E6x/EEx, F, G, L)
    fn more_effects_tick_zero(&mut self, i: usize) {
        let (efx, param) = (self.ch[i].efx, self.ch[i].efx_data);
        match efx {
            0xB => {
                let pos = param as i16 - 1;
                if pos < 0 || pos as usize >= self.m.song_length {
                    self.bxx_overflow = true;
                } else {
                    self.song_pos = pos as usize;
                }
                self.p_break_pos = 0;
                self.pos_jump_flag = true;
            }
            0xD => {
                let p = (param >> 4) * 10 + (param & 0x0F);
                self.p_break_pos = if p <= 63 { p } else { 0 };
                self.pos_jump_flag = true;
            }
            0xE => self.e_effects_tick_zero(i, param),
            0xF => {
                if param >= 32 {
                    self.bpm = param as u16;
                } else {
                    self.speed = param as u16;
                    self.tick = param as u16;
                }
            }
            16 => {
                self.global_volume = (param as u16).min(64);
                for c in self.ch.iter_mut().take(self.m.num_channels) {
                    c.status |= CS_UPDATE_VOL;
                }
            }
            21 => self.set_envelope_pos(i, param),
            _ => {}
        }
    }

    fn vol_column_tick_zero(&mut self, i: usize) -> u8 {
        let c = &mut self.ch[i];
        let mut v = c.vol_column;
        match c.vol_column >> 4 {
            1..=5 => {
                v = v.wrapping_sub(16).min(64);
                c.real_vol = v;
                c.out_vol = v;
                c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
            }
            8 => {
                v = (0u8.wrapping_sub(c.vol_column & 0x0F)).wrapping_add(c.real_vol);
                if (v as i8) < 0 {
                    v = 0;
                }
                c.real_vol = v;
                c.out_vol = v;
                c.status |= CS_UPDATE_VOL;
            }
            9 => {
                v = ((c.vol_column & 0x0F) + c.real_vol).min(64);
                c.real_vol = v;
                c.out_vol = v;
                c.status |= CS_UPDATE_VOL;
            }
            0xA => {
                v = (c.vol_column & 0x0F) * 4;
                if v != 0 {
                    c.vibrato_speed = v;
                }
            }
            0xC => {
                v <<= 4;
                c.out_pan = v;
                c.status |= CS_UPDATE_PAN;
            }
            _ => {}
        }
        v
    }

    fn handle_effects_tick_zero(&mut self, i: usize) {
        let new_vol_col = self.vol_column_tick_zero(i);
        let (efx, param) = (self.ch[i].efx, self.ch[i].efx_data);
        if efx == 0 && param == 0 {
            return;
        }
        match efx {
            8 => {
                self.ch[i].out_pan = param;
                self.ch[i].status |= CS_UPDATE_PAN;
            }
            12 => {
                let c = &mut self.ch[i];
                c.real_vol = param.min(64);
                c.out_vol = c.real_vol;
                c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
            }
            27 => {
                // Rxy: multi retrig
                let c = &mut self.ch[i];
                let s = if param & 0x0F == 0 { c.retrig_speed } else { param & 0x0F };
                c.retrig_speed = s;
                let v = if param >> 4 == 0 { c.retrig_vol } else { param >> 4 };
                c.retrig_vol = v;
                if new_vol_col == 0 {
                    self.do_multi_note_retrig(i);
                }
            }
            33 => {
                // X1x / X2x: extra fine porta
                let c = &mut self.ch[i];
                let typ = param >> 4;
                let p = param & 0x0F;
                if typ == 1 {
                    let p = if p == 0 { c.ef_pitch_up } else { p };
                    c.ef_pitch_up = p;
                    let mut np = c.real_period.wrapping_sub(p as u16);
                    if (np as i16) < 1 {
                        np = 1;
                    }
                    c.real_period = np;
                    c.out_period = np;
                    c.status |= CF_UPDATE_PERIOD;
                } else if typ == 2 {
                    let p = if p == 0 { c.ef_pitch_down } else { p };
                    c.ef_pitch_down = p;
                    let mut np = c.real_period.wrapping_add(p as u16);
                    if (np as i16) >= 32000 {
                        np = 32000 - 1;
                    }
                    c.real_period = np;
                    c.out_period = np;
                    c.status |= CF_UPDATE_PERIOD;
                }
            }
            _ => {}
        }
        self.more_effects_tick_zero(i);
    }

    fn do_multi_note_retrig(&mut self, i: usize) {
        let c = &mut self.ch[i];
        let cnt = c.retrig_counter.wrapping_add(1);
        if cnt < c.retrig_speed {
            c.retrig_counter = cnt;
            return;
        }
        c.retrig_counter = 0;
        let mut vol = c.real_vol as i16;
        match c.retrig_vol {
            1 => vol -= 1,
            2 => vol -= 2,
            3 => vol -= 4,
            4 => vol -= 8,
            5 => vol -= 16,
            6 => vol = (vol >> 1) + (vol >> 3) + (vol >> 4),
            7 => vol >>= 1,
            9 => vol += 1,
            0xA => vol += 2,
            0xB => vol += 4,
            0xC => vol += 8,
            0xD => vol += 16,
            0xE => vol += vol >> 1,
            0xF => vol += vol,
            _ => {}
        }
        c.real_vol = vol.clamp(0, 64) as u8;
        c.out_vol = c.real_vol;
        if (0x10..=0x50).contains(&c.vol_column) {
            c.out_vol = c.vol_column - 0x10;
            c.real_vol = c.out_vol;
        } else if (0xC0..=0xCF).contains(&c.vol_column) {
            c.out_pan = (c.vol_column & 0x0F) << 4;
        }
        self.trigger_note(0, 0, 0, i);
    }

    fn prepare_portamento(&mut self, i: usize, p: &Note, inst: u8) {
        if p.note > 0 {
            if p.note == NOTE_OFF {
                self.key_off(i);
            } else {
                let lut = self.period_lut();
                let c = &mut self.ch[i];
                let note = ((p.note as i32 - 1) + c.relative_note as i32) * 16 + ((c.finetune >> 3) as i32 + 16);
                if (0..(10 * 12 * 16 + 16)).contains(&note) {
                    c.porta_target = lut[note as usize];
                    c.porta_direction = if c.porta_target == c.real_period {
                        0
                    } else if c.porta_target > c.real_period {
                        1
                    } else {
                        2
                    };
                }
            }
        }
        if inst > 0 {
            Self::reset_volumes(&mut self.ch[i]);
            if p.note != NOTE_OFF {
                self.trigger_instrument(i);
            }
        }
    }

    fn get_new_note(&mut self, i: usize, p: &Note) {
        {
            let c = &mut self.ch[i];
            c.vol_column = p.vol;
            if c.efx == 0 {
                if c.efx_data > 0 {
                    // an arpeggio was running: restore the period
                    c.out_period = c.real_period;
                    c.status |= CF_UPDATE_PERIOD;
                }
            } else if (c.efx == 4 || c.efx == 6) && (p.efx != 4 && p.efx != 6) {
                c.out_period = c.real_period;
                c.status |= CF_UPDATE_PERIOD;
            }
            c.efx = p.efx;
            c.efx_data = p.param;
            c.copy_of_instr_and_note = ((p.instr as u16) << 8) | p.note as u16;
        }
        if self.ch[i].channel_off {
            self.more_effects_tick_zero(i);
            return;
        }
        let mut inst = p.instr;
        if inst > 0 {
            if inst as usize <= 128 {
                self.ch[i].instr_num = inst;
            } else {
                inst = 0;
            }
        }
        if p.efx == 0x0E && (0xD1..=0xDF).contains(&p.param) {
            return; // note delay
        }
        if !(p.efx == 0x0E && p.param == 0x90) {
            if self.ch[i].vol_column & 0xF0 == 0xF0 {
                let param = self.ch[i].vol_column & 0x0F;
                if param > 0 {
                    self.ch[i].porta_speed = ((param as u16) << 4) * 4;
                }
                self.prepare_portamento(i, p, inst);
                self.handle_effects_tick_zero(i);
                return;
            }
            if p.efx == 3 || p.efx == 5 {
                if p.efx != 5 && p.param != 0 {
                    self.ch[i].porta_speed = p.param as u16 * 4;
                }
                self.prepare_portamento(i, p, inst);
                self.handle_effects_tick_zero(i);
                return;
            }
            if p.efx == 0x14 && p.param == 0 {
                self.key_off(i);
                if inst > 0 {
                    Self::reset_volumes(&mut self.ch[i]);
                }
                self.handle_effects_tick_zero(i);
                return;
            }
            if p.note == 0 {
                if inst > 0 {
                    Self::reset_volumes(&mut self.ch[i]);
                    self.trigger_instrument(i);
                }
                self.handle_effects_tick_zero(i);
                return;
            }
        }
        if p.note == NOTE_OFF {
            self.key_off(i);
        } else {
            self.trigger_note(p.note, p.efx, p.param, i);
        }
        if inst > 0 {
            Self::reset_volumes(&mut self.ch[i]);
            if p.note != NOTE_OFF {
                self.trigger_instrument(i);
            }
        }
        self.handle_effects_tick_zero(i);
    }

    // ------------------------------------------------------------------
    // envelopes, fadeout, auto-vibrato
    // ------------------------------------------------------------------

    fn update_vol_pan_auto_vib(&mut self, i: usize) {
        let gv = self.global_volume as i32;
        let ins = self.instr(self.ch[i].ins).clone_header();
        let c = &mut self.ch[i];
        if c.key_off {
            if c.fadeout_speed > c.fadeout_vol {
                c.fadeout_vol = 0;
                c.fadeout_speed = 0;
            } else {
                c.fadeout_vol -= c.fadeout_speed;
            }
            c.status |= CS_UPDATE_VOL;
        }
        if !c.mute {
            let fvol;
            if ins.vol_flags & ENV_ENABLED != 0 {
                let env_val = env_tick(&ins.vol_env, ins.vol_len, ins.vol_flags, ins.vol_sus, ins.vol_loop_start, ins.vol_loop_end, c.key_off, &mut c.vol_env_tick, &mut c.vol_env_pos, &mut c.vol_env_value, &mut c.vol_env_delta);
                let mut fenv = (env_val as u16) as f32 * (1.0 / (64.0 * 256.0));
                if fenv > 1.0 {
                    fenv = 1.0;
                }
                let vol = gv * c.out_vol as i32 * c.fadeout_vol as i32;
                fvol = vol as f32 * (1.0 / (64.0 * 64.0 * 32768.0)) * fenv;
                c.status |= CS_UPDATE_VOL;
            } else {
                let vol = gv * c.out_vol as i32 * c.fadeout_vol as i32;
                fvol = vol as f32 * (1.0 / (64.0 * 64.0 * 32768.0));
            }
            c.final_vol = fvol.min(1.0);
        } else {
            c.final_vol = 0.0;
        }

        if ins.pan_flags & ENV_ENABLED != 0 {
            let mut env_val = env_tick(&ins.pan_env, ins.pan_len, ins.pan_flags, ins.pan_sus, ins.pan_loop_start, ins.pan_loop_end, c.key_off, &mut c.pan_env_tick, &mut c.pan_env_pos, &mut c.pan_env_value, &mut c.pan_env_delta) as i32;
            let mut pan_mul = c.out_pan as i32 - 128;
            if pan_mul >= 0 {
                pan_mul = -pan_mul;
            }
            pan_mul += 128;
            pan_mul <<= 3;
            env_val -= 32 * 256;
            let pan_add = ((env_val * pan_mul) >> 16) as i8;
            c.final_pan = c.out_pan.wrapping_add(pan_add as u8);
            c.status |= CS_UPDATE_PAN;
        } else {
            c.final_pan = c.out_pan;
        }

        if ins.vib_depth > 0 {
            let amp;
            if c.auto_vib_sweep > 0 {
                let mut a = c.auto_vib_sweep;
                if !c.key_off {
                    a = a.wrapping_add(c.auto_vib_amp);
                    if (a >> 8) > ins.vib_depth as u16 {
                        a = (ins.vib_depth as u16) << 8;
                        c.auto_vib_sweep = 0;
                    }
                    c.auto_vib_amp = a;
                }
                amp = a;
            } else {
                amp = c.auto_vib_amp;
            }
            c.auto_vib_pos = c.auto_vib_pos.wrapping_add(ins.vib_rate);
            let pos = c.auto_vib_pos;
            let val: i16 = match ins.vib_type {
                1 => {
                    if pos > 127 {
                        64
                    } else {
                        -64
                    }
                }
                2 => ((((pos >> 1) as i16) + 64) & 127) - 64,
                3 => ((-((pos >> 1) as i16) + 64) & 127) - 64,
                _ => AUTO_VIB_SINE_TAB[pos as usize] as i16,
            };
            let val = ((val as i32 * amp as i16 as i32) >> (6 + 8)) as i16;
            let mut tmp = c.out_period.wrapping_add(val as u16);
            if tmp >= 32000 {
                tmp = 0;
            }
            c.final_period = tmp;
            c.status |= CF_UPDATE_PERIOD;
        } else {
            c.final_period = c.out_period;
        }
    }

    // ------------------------------------------------------------------
    // non-zero tick effects
    // ------------------------------------------------------------------

    fn period_to_note_period(&self, period: u16, note_offset: u8, c: &Channel) -> u16 {
        let lut = self.period_lut();
        let fine = ((c.finetune >> 3) + 16) as i32;
        let mut hi = 8 * 12 * 16;
        let mut lo = 0;
        for _ in 0..8 {
            let tmp = (((lo + hi) >> 1) & !15) + fine;
            let look = (tmp - 8).max(0) as usize;
            if period >= lut[look.min(1935)] {
                hi = (tmp - fine) & !15;
            } else {
                lo = (tmp - fine) & !15;
            }
        }
        let mut tmp = lo + fine + ((note_offset as i32) << 4);
        if tmp >= (8 * 12 * 16 + 15) - 1 {
            tmp = (8 * 12 * 16 + 16) - 1;
        }
        lut[tmp as usize]
    }

    fn do_vibrato(c: &mut Channel) {
        let mut t = (c.vibrato_pos >> 2) & 0x1F;
        match c.vib_trem_ctrl & 3 {
            0 => t = VIBRATO_TAB[t as usize],
            1 => {
                t <<= 3;
                if (c.vibrato_pos as i8) < 0 {
                    t = !t;
                }
            }
            _ => t = 255,
        }
        let t = ((t as u16 * c.vibrato_depth as u16) >> 5) as u16;
        c.out_period = if (c.vibrato_pos as i8) < 0 { c.real_period.wrapping_sub(t) } else { c.real_period.wrapping_add(t) };
        c.status |= CF_UPDATE_PERIOD;
        c.vibrato_pos = c.vibrato_pos.wrapping_add(c.vibrato_speed);
    }

    fn portamento(&mut self, i: usize) {
        let c = &self.ch[i];
        if c.porta_direction == 0 {
            return;
        }
        let mut c = self.ch[i].clone();
        if c.porta_direction > 1 {
            c.real_period = c.real_period.wrapping_sub(c.porta_speed);
            if (c.real_period as i16) <= (c.porta_target as i16) {
                c.porta_direction = 1;
                c.real_period = c.porta_target;
            }
        } else {
            c.real_period = c.real_period.wrapping_add(c.porta_speed);
            if c.real_period >= c.porta_target {
                c.porta_direction = 1;
                c.real_period = c.porta_target;
            }
        }
        c.out_period = if c.semitone_porta { self.period_to_note_period(c.real_period, 0, &c) } else { c.real_period };
        c.status |= CF_UPDATE_PERIOD;
        self.ch[i] = c;
    }

    fn vol_slide(c: &mut Channel, param: u8) {
        let p = if param == 0 { c.vol_slide_speed } else { param };
        c.vol_slide_speed = p;
        let mut v = c.real_vol;
        if p & 0xF0 == 0 {
            v = v.wrapping_sub(p);
            if (v as i8) < 0 {
                v = 0;
            }
        } else {
            v = (v + (p >> 4)).min(64);
        }
        c.real_vol = v;
        c.out_vol = v;
        c.status |= CS_UPDATE_VOL;
    }

    fn vol_column_tick_nonzero(&mut self, i: usize) {
        let c = &mut self.ch[i];
        match c.vol_column >> 4 {
            6 => {
                let mut v = (0u8.wrapping_sub(c.vol_column & 0x0F)).wrapping_add(c.real_vol);
                if (v as i8) < 0 {
                    v = 0;
                }
                c.real_vol = v;
                c.out_vol = v;
                c.status |= CS_UPDATE_VOL;
            }
            7 => {
                let v = ((c.vol_column & 0x0F) + c.real_vol).min(64);
                c.real_vol = v;
                c.out_vol = v;
                c.status |= CS_UPDATE_VOL;
            }
            0xB => {
                let p = c.vol_column & 0xF;
                if p > 0 {
                    c.vibrato_depth = p;
                }
                Self::do_vibrato(c);
            }
            0xD => {
                let mut t = c.out_pan as u16 + (0u8.wrapping_sub(c.vol_column & 0x0F)) as u16;
                if t < 256 {
                    t = 0;
                }
                c.out_pan = t as u8;
                c.status |= CS_UPDATE_PAN;
            }
            0xE => {
                let t = (c.out_pan as u16 + (c.vol_column & 0x0F) as u16).min(255);
                c.out_pan = t as u8;
                c.status |= CS_UPDATE_PAN;
            }
            0xF => self.portamento(i),
            _ => {}
        }
    }

    fn handle_effects_tick_nonzero(&mut self, i: usize) {
        if self.ch[i].channel_off {
            return;
        }
        self.vol_column_tick_nonzero(i);
        let (efx, param) = (self.ch[i].efx, self.ch[i].efx_data);
        if (efx == 0 && param == 0) || efx > 35 {
            return;
        }
        let speed = self.speed;
        let tick = self.tick;
        match efx {
            0 => {
                let t = ARPEGGIO_TAB[(tick & 31) as usize];
                let c = self.ch[i].clone();
                let out = if t == 0 {
                    c.real_period
                } else {
                    let off = if t == 1 { param >> 4 } else { param & 0x0F };
                    self.period_to_note_period(c.real_period, off, &c)
                };
                let c = &mut self.ch[i];
                c.out_period = out;
                c.status |= CF_UPDATE_PERIOD;
            }
            1 => {
                let c = &mut self.ch[i];
                let p = if param == 0 { c.pitch_up } else { param };
                c.pitch_up = p;
                c.real_period = c.real_period.wrapping_sub(p as u16 * 4);
                if (c.real_period as i16) < 1 {
                    c.real_period = 1;
                }
                c.out_period = c.real_period;
                c.status |= CF_UPDATE_PERIOD;
            }
            2 => {
                let c = &mut self.ch[i];
                let p = if param == 0 { c.pitch_down } else { param };
                c.pitch_down = p;
                c.real_period = c.real_period.wrapping_add(p as u16 * 4);
                if (c.real_period as i16) >= 32000 {
                    c.real_period = 32000 - 1;
                }
                c.out_period = c.real_period;
                c.status |= CF_UPDATE_PERIOD;
            }
            3 => self.portamento(i),
            4 => {
                let c = &mut self.ch[i];
                if param > 0 {
                    if param & 0x0F > 0 {
                        c.vibrato_depth = param & 0x0F;
                    }
                    if (param & 0xF0) >> 2 > 0 {
                        c.vibrato_speed = (param & 0xF0) >> 2;
                    }
                }
                Self::do_vibrato(c);
            }
            5 => {
                self.portamento(i);
                Self::vol_slide(&mut self.ch[i], param);
            }
            6 => {
                let c = &mut self.ch[i];
                Self::do_vibrato(c);
                Self::vol_slide(c, param);
            }
            7 => {
                let c = &mut self.ch[i];
                if param > 0 {
                    if param & 0x0F > 0 {
                        c.tremolo_depth = param & 0x0F;
                    }
                    if (param & 0xF0) >> 2 > 0 {
                        c.tremolo_speed = (param & 0xF0) >> 2;
                    }
                }
                let mut t = (c.tremolo_pos >> 2) & 0x1F;
                match (c.vib_trem_ctrl >> 4) & 3 {
                    0 => t = VIBRATO_TAB[t as usize],
                    1 => {
                        t <<= 3;
                        // FT2 bug: tests the vibrato position
                        if (c.vibrato_pos as i8) < 0 {
                            t = !t;
                        }
                    }
                    _ => t = 255,
                }
                let t = ((t as u16 * c.tremolo_depth as u16) >> 6) as i16;
                let v = if (c.tremolo_pos as i8) < 0 { (c.real_vol as i16 - t).max(0) } else { (c.real_vol as i16 + t).min(64) };
                c.out_vol = v as u8;
                c.status |= CS_UPDATE_VOL;
                c.tremolo_pos = c.tremolo_pos.wrapping_add(c.tremolo_speed);
            }
            0xA => Self::vol_slide(&mut self.ch[i], param),
            0xE => match param >> 4 {
                9 => {
                    let p = param & 0x0F;
                    if p != 0 && (speed.wrapping_sub(tick)) % p as u16 == 0 {
                        self.trigger_note(0, 0, 0, i);
                        self.trigger_instrument(i);
                    }
                }
                0xC => {
                    if (speed.wrapping_sub(tick)) as u8 == param & 0x0F {
                        let c = &mut self.ch[i];
                        c.out_vol = 0;
                        c.real_vol = 0;
                        c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
                    }
                }
                0xD => {
                    if (speed.wrapping_sub(tick)) as u8 == param & 0x0F {
                        let ian = self.ch[i].copy_of_instr_and_note;
                        self.trigger_note((ian & 0xFF) as u8, 0, 0, i);
                        if ian >> 8 > 0 {
                            Self::reset_volumes(&mut self.ch[i]);
                        }
                        self.trigger_instrument(i);
                        let c = &mut self.ch[i];
                        if (0x10..=0x50).contains(&c.vol_column) {
                            c.out_vol = c.vol_column - 16;
                            c.real_vol = c.out_vol;
                        } else if (0xC0..=0xCF).contains(&c.vol_column) {
                            c.out_pan = (c.vol_column & 0x0F) << 4;
                        }
                    }
                }
                _ => {}
            },
            17 => {
                // Hxy: global volume slide
                let c = &mut self.ch[i];
                let p = if param == 0 { c.glob_vol_slide_speed } else { param };
                c.glob_vol_slide_speed = p;
                let mut v = self.global_volume as u8;
                if p & 0xF0 == 0 {
                    v = v.wrapping_sub(p);
                    if (v as i8) < 0 {
                        v = 0;
                    }
                } else {
                    v = (v + (p >> 4)).min(64);
                }
                self.global_volume = v as u16;
                for c in self.ch.iter_mut().take(self.m.num_channels) {
                    c.status |= CS_UPDATE_VOL;
                }
            }
            20 => {
                // Kxx: key off at tick
                if (speed.wrapping_sub(tick)) as u8 == param & 31 {
                    self.key_off(i);
                }
            }
            25 => {
                // Pxy: panning slide
                let c = &mut self.ch[i];
                let p = if param == 0 { c.panning_slide_speed } else { param };
                c.panning_slide_speed = p;
                let mut np = c.out_pan as i16;
                if p & 0xF0 == 0 {
                    np = (np - p as i16).max(0);
                } else {
                    np = (np + (p >> 4) as i16).min(255);
                }
                c.out_pan = np as u8;
                c.status |= CS_UPDATE_PAN;
            }
            27 => self.do_multi_note_retrig(i),
            29 => {
                // Txy: tremor
                let c = &mut self.ch[i];
                let p = if param == 0 { c.tremor_param } else { param };
                c.tremor_param = p;
                let mut sign = c.tremor_pos & 0x80;
                let mut data = (c.tremor_pos & 0x7F).wrapping_sub(1);
                if (data as i8) < 0 {
                    if sign == 0x80 {
                        sign = 0;
                        data = p & 0x0F;
                    } else {
                        sign = 0x80;
                        data = p >> 4;
                    }
                }
                c.tremor_pos = sign | data;
                c.out_vol = if sign == 0x80 { c.real_vol } else { 0 };
                c.status |= CS_UPDATE_VOL + CS_USE_QUICK_VOLRAMP;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // the tick
    // ------------------------------------------------------------------

    fn get_next_pos(&mut self) {
        if self.tick != 1 {
            return;
        }
        self.row += 1;
        if self.patt_del_time > 0 {
            self.patt_del_time2 = self.patt_del_time;
            self.patt_del_time = 0;
        }
        if self.patt_del_time2 > 0 {
            self.patt_del_time2 -= 1;
            if self.patt_del_time2 > 0 {
                self.row -= 1;
            }
        }
        if self.p_break_flag {
            self.p_break_flag = false;
            self.row = self.p_break_pos as usize;
        }
        if self.row >= self.curr_num_rows || self.pos_jump_flag {
            self.row = self.p_break_pos as usize;
            self.p_break_pos = 0;
            self.pos_jump_flag = false;
            if self.bxx_overflow {
                self.song_pos = 0;
                self.bxx_overflow = false;
            } else {
                self.song_pos += 1;
                if self.song_pos >= self.m.song_length {
                    self.song_pos = self.m.restart.min(self.m.song_length - 1);
                }
            }
            self.patt_num = self.m.orders[self.song_pos & 0xFF] as usize;
            self.curr_num_rows = self.pattern_rows(self.patt_num);
            if self.row >= self.curr_num_rows {
                self.row = 0;
            }
        }
    }

    pub fn tick_replayer(&mut self) {
        let n = self.m.num_channels;
        self.tick -= 1;
        let mut tick_zero = false;
        if self.tick == 0 {
            self.tick = self.speed;
            tick_zero = true;
        }
        let read_new = tick_zero && self.patt_del_time2 == 0;
        if read_new {
            self.cur_row = self.row;
            self.cur_pattern = self.patt_num;
            self.cur_pos = self.song_pos;
            let pat = self.patt_num;
            let row = self.row;
            for i in 0..n {
                let note = self.m.patterns.get(pat).map(|p| p.note(row, i)).unwrap_or_default();
                self.get_new_note(i, &note);
                self.update_vol_pan_auto_vib(i);
            }
        } else {
            for i in 0..n {
                self.handle_effects_tick_nonzero(i);
                self.update_vol_pan_auto_vib(i);
            }
        }
        self.get_next_pos();
        self.update_voices();
    }

    // ------------------------------------------------------------------
    // voices / mixer
    // ------------------------------------------------------------------

    fn period_to_hz(&self, period: u16) -> f64 {
        if period == 0 {
            return 0.0;
        }
        if self.m.linear {
            let inv = (12u32 * 192 * 4).wrapping_sub(period as u32) & 0xFFFF;
            let q = inv / (12 * 16 * 4);
            let rem = inv % (12 * 16 * 4);
            let shift = (14i32 - q as i32) & 31;
            8363.0 * 256.0 * (rem as f64 / 768.0).exp2() / (shift as f64).exp2()
        } else {
            8363.0 * 1712.0 / period as f64
        }
    }

    fn samples_per_tick(&self) -> u32 {
        (self.rate as u64 * 5 / (self.bpm.max(1) as u64 * 2)) as u32
    }

    fn update_voices(&mut self) {
        let quick = ((self.rate as f32) * 0.005).max(1.0) as u32;
        let tick_len = self.samples_per_tick().max(1);
        for i in 0..self.m.num_channels {
            let status = self.ch[i].status;
            if status == 0 {
                continue;
            }
            self.ch[i].status = 0;
            let c = self.ch[i].clone();
            let mut v = self.voices[i];
            if status & CS_UPDATE_VOL != 0 {
                v.volume = c.final_vol;
            }
            if status & CS_UPDATE_PAN != 0 {
                v.panning = c.final_pan;
            }
            if status & (CS_UPDATE_VOL | CS_UPDATE_PAN) != 0 {
                let (pl, pr) = if self.m.ft2_mix {
                    // FT2's square-root panning (the right side never quite reaches 100%)
                    let p = v.panning as f32;
                    (if p > 0.0 { ((256.0 - p) / 256.0).sqrt() } else { 1.0 }, (p / 256.0).sqrt())
                } else {
                    ((256 - v.panning as i32) as f32 / 256.0, v.panning as f32 / 256.0)
                };
                v.tgt_l = v.volume * pl;
                v.tgt_r = v.volume * pr;
                let trig = status & CS_TRIGGER_VOICE != 0;
                if trig {
                    if v.cur_l > 0.0 || v.cur_r > 0.0 {
                        // fade the old note out quickly on a separate voice
                        let mut f = v;
                        f.ramp = quick;
                        f.dl = -f.cur_l / quick as f32;
                        f.dr = -f.cur_r / quick as f32;
                        f.fade_voice = true;
                        let n = self.ch.len();
                        self.voices[n + i] = f;
                    }
                    v.cur_l = 0.0;
                    v.cur_r = 0.0;
                }
                let len = if status & CS_USE_QUICK_VOLRAMP != 0 { quick } else { tick_len };
                if !trig && v.tgt_l == v.cur_l && v.tgt_r == v.cur_r {
                    v.ramp = 0;
                } else {
                    v.ramp = len;
                    v.dl = (v.tgt_l - v.cur_l) / len as f32;
                    v.dr = (v.tgt_r - v.cur_r) / len as f32;
                }
            }
            if status & CF_UPDATE_PERIOD != 0 {
                v.delta = self.period_to_hz(c.final_period) / self.rate as f64;
            }
            if status & CS_TRIGGER_VOICE != 0 {
                let s = &self.m.instruments[c.ins].as_ref().unwrap().samples[c.smp_num as usize & 15];
                let mut lt = s.loop_type;
                if s.loop_len < 1 {
                    lt = 0;
                }
                if s.data.is_empty() || s.length < 1 {
                    v.active = false;
                } else {
                    v.ins = c.ins;
                    v.smp = c.smp_num as usize & 15;
                    v.loop_type = lt;
                    v.loop_start = s.loop_start;
                    v.loop_len = s.loop_len;
                    v.sample_end = if lt == 0 { s.length } else { s.loop_start + s.loop_len };
                    v.pos = c.smp_start_pos.max(0) as f64;
                    v.backwards = false;
                    v.active = (v.pos as usize) < v.sample_end;
                }
            }
            v.fade_voice = false;
            self.voices[i] = v;
        }
    }

    /// Renders interleaved stereo f32 (about -1..1).
    pub fn render_f32(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if !self.playing {
            return;
        }
        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            if self.samples_left == 0 {
                self.tick_replayer();
                self.samples_left = self.samples_per_tick().max(1);
                self.seconds += self.samples_left as f64 / self.rate as f64;
            }
            let n = (frames - done).min(self.samples_left as usize);
            self.mix(&mut out[done * 2..(done + n) * 2]);
            done += n;
            self.samples_left -= n as u32;
        }
    }

    fn mix(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let gain = self.gain / 32768.0;
        for vi in 0..self.voices.len() {
            let ch = vi % self.ch.len();
            if ch >= self.m.num_channels {
                continue;
            }
            let mut v = self.voices[vi];
            if !v.active || v.delta <= 0.0 {
                // still advance the volume ramp state of silent voices
                if v.ramp > 0 && v.active {
                    let k = (v.ramp as usize).min(frames) as f32;
                    v.cur_l += v.dl * k;
                    v.cur_r += v.dr * k;
                    v.ramp -= k as u32;
                }
                self.voices[vi] = v;
                continue;
            }
            let muted = self.muted[ch];
            let Some(Some(ins)) = self.m.instruments.get(v.ins) else {
                continue;
            };
            let data = &ins.samples[v.smp].data;
            if data.is_empty() {
                v.active = false;
                self.voices[vi] = v;
                continue;
            }
            let limit = v.sample_end.min(data.len());
            let mut f = 0;
            while f < frames {
                // fast path: a run that reaches neither a loop point nor the end
                let k = xm_safe_run(&v, limit).min(frames - f);
                if k > 0 {
                    let k = if v.ramp > 0 { k.min(v.ramp as usize) } else { k };
                    let o = &mut out[f * 2..(f + k) * 2];
                    match (v.ramp > 0, v.backwards) {
                        (false, false) => xm_run::<false, false>(&mut v, data, o, gain, muted),
                        (false, true) => xm_run::<false, true>(&mut v, data, o, gain, muted),
                        (true, false) => xm_run::<true, false>(&mut v, data, o, gain, muted),
                        (true, true) => xm_run::<true, true>(&mut v, data, o, gain, muted),
                    }
                    f += k;
                    continue;
                }
                    let ip = v.pos as usize;
                    let fr = (v.pos - ip as f64) as f32;
                    let a = *data.get(ip).unwrap_or(&0) as f32;
                    // next sample, following the loop
                    let next = if v.backwards {
                        if ip > v.loop_start {
                            ip - 1
                        } else {
                            ip
                        }
                    } else if ip + 1 >= v.sample_end {
                        match v.loop_type {
                            1 => v.loop_start,
                            2 => ip,
                            _ => ip + 1,
                        }
                    } else {
                        ip + 1
                    };
                    let b = *data.get(next).unwrap_or(&0) as f32;
                    let s = a + (b - a) * fr;
                    if v.ramp > 0 {
                        v.cur_l += v.dl;
                        v.cur_r += v.dr;
                        v.ramp -= 1;
                        if v.ramp == 0 && !v.fade_voice {
                            v.cur_l = v.tgt_l;
                            v.cur_r = v.tgt_r;
                        }
                    }
                    if !muted {
                        out[f * 2] += s * v.cur_l * gain;
                        out[f * 2 + 1] += s * v.cur_r * gain;
                    }
                    // advance
                    if v.backwards {
                        v.pos -= v.delta;
                        if v.pos < v.loop_start as f64 {
                            v.pos = 2.0 * v.loop_start as f64 - v.pos;
                            v.backwards = false;
                        }
                    } else {
                        v.pos += v.delta;
                        if v.pos >= v.sample_end as f64 {
                            match v.loop_type {
                                0 => {
                                    v.active = false;
                                    break;
                                }
                                1 => {
                                    let over = v.pos - v.sample_end as f64;
                                    v.pos = v.loop_start as f64 + over % v.loop_len.max(1) as f64;
                                }
                                _ => {
                                    let over = v.pos - v.sample_end as f64;
                                    v.pos = (v.sample_end as f64 - 1.0 - over).max(v.loop_start as f64);
                                    v.backwards = true;
                                }
                            }
                        }
                    }
                f += 1;
            }
            if v.fade_voice && v.ramp == 0 {
                v.active = false;
            }
            self.voices[vi] = v;
        }
    }

    // ------------------------------------------------------------------
    // GUI helpers
    // ------------------------------------------------------------------

    /// Current channel level 0..1 (volume after envelopes), for VU meters
    pub fn level(&self, ch: usize) -> f32 {
        let v = &self.voices[ch];
        if !v.active || self.muted[ch] {
            return 0.0;
        }
        v.volume
    }

    /// 40 points of the waveform playing on `ch` and its volume 0..64
    pub fn scope(&self, ch: usize) -> Option<([i8; 40], u8)> {
        let v = &self.voices[ch];
        if !v.active {
            return None;
        }
        let data = &self.m.instruments.get(v.ins)?.as_ref()?.samples[v.smp].data;
        let mut out = [0i8; 40];
        // show roughly 1/60 s of the sample at its playback speed
        let step = (v.delta * self.rate as f64 / 60.0 / 40.0).max(0.01);
        let mut pos = v.pos;
        let mut back = v.backwards;
        for o in out.iter_mut() {
            let ip = pos as usize;
            *o = (*data.get(ip).unwrap_or(&0) >> 8) as i8;
            if back {
                pos -= step;
                if pos < v.loop_start as f64 {
                    pos = v.loop_start as f64;
                    back = false;
                }
            } else {
                pos += step;
                if pos >= v.sample_end as f64 {
                    match v.loop_type {
                        0 => break,
                        1 => pos = v.loop_start as f64 + (pos - v.sample_end as f64) % v.loop_len.max(1) as f64,
                        _ => {
                            pos = v.sample_end as f64 - 1.0;
                            back = true;
                        }
                    }
                }
            }
        }
        let vol = (v.volume * 64.0).round().clamp(0.0, 64.0) as u8;
        Some((out, vol))
    }
}

/// The instrument fields the replayer needs, copied so the channel can be borrowed mutably.
#[derive(Clone)]
struct InsHeader {
    vol_env: [(i16, i16); 12],
    pan_env: [(i16, i16); 12],
    vol_len: u8,
    pan_len: u8,
    vol_sus: u8,
    vol_loop_start: u8,
    vol_loop_end: u8,
    pan_sus: u8,
    pan_loop_start: u8,
    pan_loop_end: u8,
    vol_flags: u8,
    pan_flags: u8,
    vib_type: u8,
    vib_sweep: u8,
    vib_depth: u8,
    vib_rate: u8,
    fadeout: u16,
}

impl XmInstrument {
    fn clone_header(&self) -> InsHeader {
        InsHeader {
            vol_env: self.vol_env,
            pan_env: self.pan_env,
            vol_len: self.vol_len,
            pan_len: self.pan_len,
            vol_sus: self.vol_sus,
            vol_loop_start: self.vol_loop_start,
            vol_loop_end: self.vol_loop_end,
            pan_sus: self.pan_sus,
            pan_loop_start: self.pan_loop_start,
            pan_loop_end: self.pan_loop_end,
            vol_flags: self.vol_flags,
            pan_flags: self.pan_flags,
            vib_type: self.vib_type,
            vib_sweep: self.vib_sweep,
            vib_depth: self.vib_depth,
            vib_rate: self.vib_rate,
            fadeout: self.fadeout,
        }
    }
}

/// One tick of an FT2 envelope (volume or panning). Returns the envelope value (0..64*256).
#[allow(clippy::too_many_arguments)]
fn env_tick(pts: &[(i16, i16); 12], len: u8, flags: u8, sus: u8, loop_start: u8, loop_end: u8, key_off: bool, tick: &mut u16, pos: &mut u8, value: &mut i16, delta: &mut i16) -> i16 {
    let mut did_interpolate = false;
    let mut env_val: i16 = 0;
    let mut ep = *pos;
    *tick = tick.wrapping_add(1);
    if *tick == pts[(ep as usize).min(11)].0 as u16 {
        *value = ((pts[(ep as usize).min(11)].1 as i8) as i16) << 8;
        ep = ep.wrapping_add(1);
        if flags & ENV_LOOP != 0 {
            ep = ep.wrapping_sub(1);
            if ep == loop_end && (flags & ENV_SUSTAIN == 0 || ep != sus || !key_off) {
                ep = loop_start;
                *tick = pts[(ep as usize).min(11)].0 as u16;
                *value = ((pts[(ep as usize).min(11)].1 as i8) as i16) << 8;
            }
            ep = ep.wrapping_add(1);
        }
        if ep < len {
            let mut interpolate = true;
            if flags & ENV_SUSTAIN != 0 && !key_off && ep.wrapping_sub(1) == sus {
                ep = ep.wrapping_sub(1);
                *delta = 0;
                interpolate = false;
            }
            if interpolate {
                *pos = ep;
                let x0 = pts[(ep as usize - 1).min(11)].0;
                let x1 = pts[(ep as usize).min(11)].0;
                let xd = x1.wrapping_sub(x0);
                if xd > 0 {
                    let y0 = pts[(ep as usize - 1).min(11)].1;
                    let y1 = pts[(ep as usize).min(11)].1;
                    let yd = (y1.wrapping_sub(y0)) as i8;
                    *delta = (((yd as i32) << 8) / xd as i32) as i16;
                    env_val = *value;
                    did_interpolate = true;
                } else {
                    *delta = 0;
                }
            }
        } else {
            *delta = 0;
        }
    }
    if !did_interpolate {
        *value = value.wrapping_add(*delta);
        env_val = *value;
        let hi = (env_val >> 8) as u8;
        if hi > 64 {
            env_val = if hi <= 160 { 64 * 256 } else { 0 };
            *delta = 0;
        }
    }
    env_val
}

/// Lxx: set envelope position (FT2's setEnvelopePos). Returns (pos, tick, delta, value).
fn env_set_pos(pts: &[(i16, i16); 12], len: u8, param: u8, delta0: i16, value0: i16) -> (u8, u16, i16, i16) {
    let tick_out = (param as u16).wrapping_sub(1);
    let mut point: i32 = 0;
    let mut update = true;
    let mut t = param as i32;
    let (mut delta, mut value) = (delta0, value0);
    if len > 1 {
        point += 1;
        for _ in 0..len as i32 - 1 {
            if t < pts[(point as usize).min(11)].0 as i32 {
                point -= 1;
                t -= pts[(point as usize).min(11)].0 as i32;
                if t == 0 {
                    update = false;
                    break;
                }
                let x0 = pts[point as usize].0;
                let x1 = pts[(point as usize + 1).min(11)].0;
                let xd = x1.wrapping_sub(x0);
                if xd <= 0 {
                    update = true;
                    break;
                }
                let y0 = pts[point as usize].1;
                let y1 = pts[(point as usize + 1).min(11)].1;
                let yd = y1.wrapping_sub(y0) as i8;
                delta = (((yd as i32) << 8) / xd as i32) as i16;
                value = ((((y0 as i8) as i16) << 8) as i32 + (delta as i32 * (t - 1)) as i16 as i32) as i16;
                point += 1;
                update = false;
                break;
            }
            point += 1;
        }
        if update {
            point -= 1;
        }
    }
    if update {
        delta = 0;
        value = ((pts[(point as usize).min(11)].1 as i8) as i16) << 8;
    }
    if point >= len as i32 {
        point = (len as i32 - 1).max(0);
    }
    (point as u8, tick_out, delta, value)
}

/// frames that can be mixed from the voice's position without reaching a loop point or the
/// sample end (one frame of margin against rounding)
#[inline]
fn xm_safe_run(v: &Voice, limit: usize) -> usize {
    if v.delta <= 0.0 {
        return 0;
    }
    let room = if v.backwards {
        // interpolation reads ip - 1, so ip must stay above the loop start
        if v.pos >= limit as f64 {
            return 0;
        }
        v.pos - (v.loop_start as f64 + 1.0)
    } else {
        (limit as f64 - 1.0) - v.pos
    };
    if room <= 0.0 {
        return 0;
    }
    let k = (room / v.delta).floor();
    if k < 2.0 {
        0
    } else if k > 1e9 {
        usize::MAX
    } else {
        k as usize - 1
    }
}

fn xm_run<const RAMP: bool, const BACK: bool>(v: &mut Voice, data: &[i16], out: &mut [f32], gain: f32, muted: bool) {
    // 32.32 fixed-point position inside the run (no float -> int conversion per frame)
    let mut p = (v.pos * FX) as u64;
    let d = (v.delta * FX) as u64;
    let (mut cl, mut cr, dl, dr) = (v.cur_l, v.cur_r, v.dl, v.dr);
    let n = out.len() / 2;
    // xm_safe_run guarantees every index read below is inside `data`
    debug_assert!(BACK || ((p + d * n as u64) >> 32) as usize + 1 < data.len() + 1);
    for o in out.chunks_exact_mut(2) {
        let ip = (p >> 32) as usize;
        let fr = (p as u32) as f32 * FX_INV;
        let (a, b) = unsafe {
            let a = *data.get_unchecked(ip) as f32;
            let b = if BACK { *data.get_unchecked(ip - 1) } else { *data.get_unchecked(ip + 1) } as f32;
            (a, b)
        };
        let s = a + (b - a) * fr;
        if RAMP {
            cl += dl;
            cr += dr;
        }
        if !muted {
            o[0] += s * cl * gain;
            o[1] += s * cr * gain;
        }
        if BACK {
            p -= d;
        } else {
            p += d;
        }
    }
    let pos = p as f64 / FX;
    v.pos = pos;
    v.cur_l = cl;
    v.cur_r = cr;
    if RAMP {
        v.ramp -= (out.len() / 2) as u32;
        if v.ramp == 0 && !v.fade_voice {
            v.cur_l = v.tgt_l;
            v.cur_r = v.tgt_r;
        }
    }
}

/// 32.32 fixed point used by the mixing runs
const FX: f64 = 4294967296.0;
const FX_INV: f32 = 1.0 / 4294967296.0;
