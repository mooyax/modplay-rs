//! ProTracker 2.3D replayer. Port of pt2_replayer.c (the playback parts).
//!
//! GUI-related calls of the original (scopes, VU meters, spectrum analyzer,
//! pattern-editor cursor, step-play, record mode) are left out; they will be
//! re-added together with the GUI port.

use crate::consts::*;
use crate::engine::Engine;
use crate::module::Channel;
use crate::paula::PAULA_VOICES;
use crate::tables::{FUNK_TABLE, PERIOD_TABLE, VIBRATO_TABLE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    Normal,
    Pattern,
}

/// Replayer-internal state (the `static` variables of pt2_replayer.c)
pub struct ReplayerState {
    pub pos_jump_assert: bool,
    pub p_break_flag: bool,
    pub mod_render_done: bool,
    pub do_stop_song: bool,
    pub p_break_position: i8,
    pub mod_pattern: i8,
    pub patt_del_time: u8,
    pub low_mask: u8,
    pub patt_del_time2: u8,
    pub mod_pos: i16,
    pub dmacon_temp: u16,
    pub mod_bpm: i32,
    pub cia_set_bpm: i32,

    // "editor" state that affects playback
    pub song_playing: bool,
    pub play_mode: PlayMode,
    pub timing_mode: TimingMode,
    pub muted: [bool; PAULA_VOICES],
    pub mod2wav_ongoing: bool,
    pub row_visit_table: Vec<bool>,
    pub playback_seconds: f64,
    pub led_filter_enabled: bool,
    pub enable_e8x: bool,
    pub compo_mode: bool,
    /// VU meter triggers for the GUI: (trigger counter, height 0..47)
    pub vu_trigger: [(u32, u8); PAULA_VOICES],
}

impl Default for ReplayerState {
    fn default() -> Self {
        ReplayerState {
            pos_jump_assert: false,
            p_break_flag: false,
            mod_render_done: false,
            do_stop_song: false,
            p_break_position: 0,
            mod_pattern: 0,
            patt_del_time: 0,
            low_mask: 0xFF,
            patt_del_time2: 0,
            mod_pos: 0,
            dmacon_temp: 0,
            mod_bpm: 0,
            cia_set_bpm: 0,
            song_playing: false,
            play_mode: PlayMode::Normal,
            timing_mode: TimingMode::Cia,
            muted: [false; PAULA_VOICES],
            mod2wav_ongoing: false,
            row_visit_table: vec![false; 128 * MOD_ROWS],
            playback_seconds: 0.0,
            led_filter_enabled: false,
            enable_e8x: false,
            compo_mode: false,
            vu_trigger: [(0, 0); PAULA_VOICES],
        }
    }
}

#[inline]
fn period_at(finetune: u8, idx: usize) -> i16 {
    PERIOD_TABLE.get(finetune as usize * 37 + idx).copied().unwrap_or(0)
}

impl Engine {
    // ---- helpers for sample memory --------------------------------------

    #[inline]
    fn smp_get(&self, pos: usize) -> i8 {
        self.song.sample_data.get(pos).copied().unwrap_or(0)
    }

    #[inline]
    fn smp_set(&mut self, pos: usize, v: i8) {
        if let Some(p) = self.song.sample_data.get_mut(pos) {
            *p = v;
        }
    }

    // ---- public control ----------------------------------------------------

    pub fn turn_off_voices(&mut self) {
        self.paula.write_dmacon(0x000F);
        for i in 0..PAULA_VOICES {
            self.paula.write_vol(i, 0);
        }
        self.reset_audio_dither();
    }

    pub fn mod_set_speed(&mut self, speed: i32) {
        self.song.curr_speed = speed;
        self.song.speed = speed;
        self.song.tick = 0;
    }

    pub fn do_stop_it(&mut self) {
        let r = &mut self.rep;
        r.song_playing = false;
        r.patt_del_time = 0;
        r.patt_del_time2 = 0;
        for ch in self.song.channels.iter_mut() {
            ch.n_wavecontrol = 0;
            ch.n_glissfunk = 0;
            ch.n_finetune = 0;
            ch.n_loopcount = 0;
        }
        self.rep.do_stop_song = false;
    }

    pub fn mod_set_tempo(&mut self, bpm: i32) {
        if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
            return;
        }
        self.rep.mod_bpm = bpm;
        if !self.rep.mod2wav_ongoing {
            self.song.curr_bpm = bpm;
        }
        let i = (bpm - MIN_BPM) as usize;
        self.audio.samples_per_tick_int = self.audio.samples_per_tick_int_tab[i];
        self.audio.samples_per_tick_frac = self.audio.samples_per_tick_frac_tab[i];
    }

    pub fn set_led_filter(&mut self, state: bool) {
        if self.rep.led_filter_enabled == state {
            return;
        }
        self.rep.led_filter_enabled = state;
        self.paula.write_cia_pra((state as u8) << 1);
    }

    /// modPlay(patt, pos, row). Pass -1 for "don't set".
    pub fn mod_play(&mut self, patt: i16, pos: i16, row: i8) {
        self.do_stop_it();
        self.turn_off_voices();

        if row != -1 {
            if (0..=63).contains(&row) {
                self.song.row = row;
                self.song.curr_row = row;
            }
        } else {
            self.song.row = 0;
            self.song.curr_row = 0;
        }

        let song_len = self.song.song_length as i16;
        if self.rep.play_mode != PlayMode::Pattern {
            if self.rep.mod_pos >= song_len {
                self.rep.mod_pos = 0;
                self.song.curr_pos = 0;
            }
            if pos >= 0 && pos < song_len {
                self.rep.mod_pos = pos;
                self.song.curr_pos = pos as u16;
            }
            if pos >= song_len {
                self.rep.mod_pos = 0;
                self.song.curr_pos = 0;
            }
        }

        if (0..=(MAX_PATTERNS as i16 - 1)).contains(&patt) {
            self.rep.mod_pattern = patt as i8;
        } else {
            self.rep.mod_pattern = self.song.pattern_table[self.rep.mod_pos as usize] as i8;
        }
        self.song.curr_pattern = self.rep.mod_pattern as u16;

        self.song.tick = self.song.speed - 1;
        self.rep.cia_set_bpm = -1;

        self.rep.mod_render_done = false;
        self.rep.song_playing = true;

        if self.rep.play_mode != PlayMode::Pattern {
            self.rep.playback_seconds = 0.0;
        }

        self.audio.tick_sample_counter = 0;
        self.audio.tick_sample_counter_frac = 0;
    }

    pub fn mod_stop(&mut self) {
        self.rep.song_playing = false;
        self.turn_off_voices();
        for ch in self.song.channels.iter_mut() {
            ch.n_wavecontrol = 0;
            ch.n_glissfunk = 0;
            ch.n_finetune = 0;
            ch.n_loopcount = 0;
        }
        let r = &mut self.rep;
        r.p_break_flag = false;
        r.patt_del_time = 0;
        r.patt_del_time2 = 0;
        r.p_break_position = 0;
        r.pos_jump_assert = false;
        r.mod_render_done = true;
        r.do_stop_song = false;

        // setReplayerPosToTrackerPos()
        r.mod_pattern = self.song.curr_pattern as i8;
        r.mod_pos = self.song.curr_pos as i16;
        self.song.row = self.song.curr_row;
        self.song.tick = 0;
    }

    /// restartSong() - used at the beginning of MOD2WAV
    pub fn restart_song(&mut self) {
        if self.rep.song_playing {
            self.mod_stop();
        }
        self.rep.play_mode = PlayMode::Normal;
        self.song.row = 0;
        self.song.curr_row = 0;
        self.song.rows_counter = 0;
        self.rep.row_visit_table.fill(false);

        self.song.curr_speed = 6;
        self.song.curr_bpm = 125;
        self.mod_set_speed(6);
        self.mod_set_tempo(125);
        self.mod_play(-1, 0, 0);
    }

    // ---- effects -------------------------------------------------------------

    fn set_vu_meter_height(&mut self, c: usize) {
        let ch = self.song.channels[c];
        if self.rep.muted[ch.n_chanindex] || self.rep.mod2wav_ongoing {
            return;
        }
        let mut vol = ch.n_volume as u8;
        if ch.n_cmd & 0xF00 == 0xC00 {
            vol = (ch.n_cmd & 0xFF) as u8; // handle Cxx effect
        }
        if vol > 64 {
            vol = 64;
        }
        let t = &mut self.rep.vu_trigger[ch.n_chanindex];
        t.0 = t.0.wrapping_add(1);
        t.1 = crate::tables::VU_METER_HEIGHTS[vol as usize];
    }

    fn update_funk(&mut self, c: usize) {
        let ch = self.song.channels[c];
        let funk_speed = (ch.n_glissfunk >> 4) as usize;
        if funk_speed == 0 {
            return;
        }

        let ch = &mut self.song.channels[c];
        ch.n_funkoffset = ch.n_funkoffset.wrapping_add(FUNK_TABLE[funk_speed]);
        if ch.n_funkoffset >= 128 {
            ch.n_funkoffset = 0;

            if let (Some(loopstart), Some(wavestart)) = (ch.n_loopstart, ch.n_wavestart) {
                let mut ws = wavestart + 1;
                if ws >= loopstart + ((ch.n_replen as usize) << 1) {
                    ws = loopstart;
                }
                ch.n_wavestart = Some(ws);
                let v = self.smp_get(ws);
                self.smp_set(ws, (-1i8).wrapping_sub(v));
            }
        }
    }

    fn set_gliss_control(ch: &mut Channel) {
        ch.n_glissfunk = (ch.n_glissfunk & 0xF0) | (ch.n_cmd & 0x0F) as u8;
    }

    fn set_vibrato_control(ch: &mut Channel) {
        ch.n_wavecontrol = (ch.n_wavecontrol & 0xF0) | (ch.n_cmd & 0x0F) as u8;
    }

    fn set_fine_tune(ch: &mut Channel) {
        ch.n_finetune = (ch.n_cmd & 0xF) as u8;
    }

    fn jump_loop(&mut self, c: usize) {
        if self.song.tick != 0 {
            return;
        }
        let row = self.song.row;
        let ch = &mut self.song.channels[c];
        if ch.n_cmd & 0xF == 0 {
            ch.n_pattpos = row;
        } else {
            if ch.n_loopcount == 0 {
                ch.n_loopcount = (ch.n_cmd & 0xF) as i8;
            } else {
                ch.n_loopcount -= 1;
                if ch.n_loopcount == 0 {
                    return;
                }
            }

            self.rep.p_break_position = ch.n_pattpos;
            self.rep.p_break_flag = true;

            // for MOD2WAV end-of-song detection
            if self.rep.mod2wav_ongoing {
                let mut p = self.rep.p_break_position as i32;
                while p <= row as i32 {
                    let idx = (self.rep.mod_pos as i32 * MOD_ROWS as i32 + p) as usize;
                    if let Some(v) = self.rep.row_visit_table.get_mut(idx) {
                        *v = false;
                    }
                    p += 1;
                }
            }
        }
    }

    fn set_tremolo_control(ch: &mut Channel) {
        ch.n_wavecontrol = (((ch.n_cmd & 0xF) as u8) << 4) | (ch.n_wavecontrol & 0xF);
    }

    /// E8x - Karplus-Strong (disabled by default, as in pt2-clone)
    fn karplus_strong(&mut self, c: usize) {
        if !self.rep.enable_e8x {
            return;
        }
        let ch = self.song.channels[c];
        let Some(ls) = ch.n_loopstart else { return };

        // yes, this means that it's buggy for >64kB loops!
        let end = ((ch.n_replen as u32 * 2).wrapping_sub(2)) as u16 as usize;
        for i in 0..=end {
            let a = self.smp_get(ls + i) as i32;
            let b = self.smp_get(ls + i + 1) as i32;
            self.smp_set(ls + i, ((a + b) >> 1) as i8);
        }
        let a = self.smp_get(ls + end + 1) as i32;
        let b = self.smp_get(ls) as i32;
        self.smp_set(ls + end + 1, ((a + b) >> 1) as i8);
    }

    fn do_retrg(&mut self, c: usize) {
        let ch = self.song.channels[c];
        let v = ch.n_chanindex;

        self.paula.write_dmacon(ch.n_dmabit); // voice DMA off
        self.paula.write_ptr(v, ch.n_start); // n_start is increased on 9xx
        self.paula.write_len(v, ch.n_length);
        self.paula.write_per(v, ch.n_period as u16);
        self.paula.write_dmacon(0x8000 | ch.n_dmabit); // voice DMA on

        // these take effect after the current DMA cycle is done
        self.paula.write_ptr(v, ch.n_loopstart);
        self.paula.write_len(v, ch.n_replen);

        self.set_vu_meter_height(c);
    }

    fn retrig_note(&mut self, c: usize) {
        let ch = self.song.channels[c];
        let p = (ch.n_cmd & 0xF) as i32;
        if p > 0 {
            if self.song.tick == 0 && (ch.n_note & 0xFFF) > 0 {
                return;
            }
            if self.song.tick % p == 0 {
                self.do_retrg(c);
            }
        }
    }

    fn volume_slide(ch: &mut Channel) {
        let param = (ch.n_cmd & 0xFF) as u8;
        if param & 0xF0 == 0 {
            ch.n_volume = ch.n_volume.wrapping_sub((param & 0x0F) as i8);
            if ch.n_volume < 0 {
                ch.n_volume = 0;
            }
        } else {
            ch.n_volume = ch.n_volume.wrapping_add((param >> 4) as i8);
            if ch.n_volume > 64 {
                ch.n_volume = 64;
            }
        }
    }

    fn volume_fine_up(&mut self, c: usize) {
        if self.song.tick == 0 {
            let ch = &mut self.song.channels[c];
            ch.n_volume = ch.n_volume.wrapping_add((ch.n_cmd & 0xF) as i8);
            if ch.n_volume > 64 {
                ch.n_volume = 64;
            }
        }
    }

    fn volume_fine_down(&mut self, c: usize) {
        if self.song.tick == 0 {
            let ch = &mut self.song.channels[c];
            ch.n_volume = ch.n_volume.wrapping_sub((ch.n_cmd & 0xF) as i8);
            if ch.n_volume < 0 {
                ch.n_volume = 0;
            }
        }
    }

    fn note_cut(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        if self.song.tick == (ch.n_cmd & 0xF) as i32 {
            ch.n_volume = 0;
        }
    }

    fn note_delay(&mut self, c: usize) {
        let ch = self.song.channels[c];
        if self.song.tick == (ch.n_cmd & 0xF) as i32 && (ch.n_note & 0xFFF) > 0 {
            self.do_retrg(c);
        }
    }

    fn pattern_delay(&mut self, c: usize) {
        if self.song.tick == 0 && self.rep.patt_del_time2 == 0 {
            self.rep.patt_del_time = (self.song.channels[c].n_cmd & 0xF) as u8 + 1;
        }
    }

    fn funk_it(&mut self, c: usize) {
        if self.song.tick == 0 {
            let ch = &mut self.song.channels[c];
            ch.n_glissfunk = (((ch.n_cmd & 0xF) as u8) << 4) | (ch.n_glissfunk & 0xF);
            if ch.n_glissfunk & 0xF0 > 0 {
                self.update_funk(c);
            }
        }
    }

    fn position_jump(&mut self, c: usize) {
        if self.rep.play_mode != PlayMode::Pattern {
            // B00 results in -1, but it safely jumps to order 0
            self.rep.mod_pos = (self.song.channels[c].n_cmd & 0xFF) as i16 - 1;
        }
        self.rep.p_break_position = 0;
        self.rep.pos_jump_assert = true;
    }

    fn volume_change(ch: &mut Channel) {
        ch.n_volume = (ch.n_cmd & 0xFF) as u8 as i8;
        if ch.n_volume as u8 > 64 {
            ch.n_volume = 64;
        }
    }

    fn pattern_break(&mut self, c: usize) {
        let cmd = self.song.channels[c].n_cmd;
        let mut pos = ((((cmd & 0xF0) >> 4) * 10) + (cmd & 0x0F)) as u8 as i8;
        if pos as u8 > 63 {
            pos = 0;
        }
        self.rep.p_break_position = pos;
        self.rep.pos_jump_assert = true;
    }

    fn set_speed(&mut self, c: usize) {
        let param = (self.song.channels[c].n_cmd & 0xFF) as i32;
        if param > 0 {
            if self.rep.timing_mode == TimingMode::Vblank || param < 32 {
                self.mod_set_speed(param);
            } else {
                // the CIA chip doesn't use its new timer value until the next interrupt
                self.rep.cia_set_bpm = param;
            }
        } else {
            self.rep.do_stop_song = true; // F00 - stop song
        }
    }

    fn arpeggio(&mut self, c: usize) {
        let ch = self.song.channels[c];
        let arp_note: usize = match self.song.tick % 3 {
            1 => (ch.n_cmd >> 4) as usize,
            2 => (ch.n_cmd & 0xF) as usize,
            _ => {
                self.paula.write_per(ch.n_chanindex, ch.n_period as u16);
                return;
            }
        };

        // NOTE: (ch.n_cmd >> 4) includes the command nibble too, but the
        // arpeggio command is 0, so it is 0..15 here (same as in C).
        for base in 0..37 {
            if ch.n_period >= period_at(ch.n_finetune, base) {
                let p = period_at(ch.n_finetune, base + arp_note);
                self.paula.write_per(ch.n_chanindex, p as u16);
                break;
            }
        }
    }

    fn porta_up(&mut self, c: usize) {
        let low_mask = self.rep.low_mask;
        let ch = &mut self.song.channels[c];
        ch.n_period = ch.n_period.wrapping_sub(((ch.n_cmd & 0xFF) as u8 & low_mask) as i16);
        self.rep.low_mask = 0xFF;

        // PT BUG: sign removed before comparison, underflow not clamped!
        if (ch.n_period & 0xFFF) < 113 {
            ch.n_period = (ch.n_period & (0xF000u16 as i16)) | 113;
        }
        let (v, p) = (ch.n_chanindex, (ch.n_period & 0xFFF) as u16);
        self.paula.write_per(v, p);
    }

    fn porta_down(&mut self, c: usize) {
        let low_mask = self.rep.low_mask;
        let ch = &mut self.song.channels[c];
        ch.n_period = ch.n_period.wrapping_add(((ch.n_cmd & 0xFF) as u8 & low_mask) as i16);
        self.rep.low_mask = 0xFF;

        if (ch.n_period & 0xFFF) > 856 {
            ch.n_period = (ch.n_period & (0xF000u16 as i16)) | 856;
        }
        let (v, p) = (ch.n_chanindex, (ch.n_period & 0xFFF) as u16);
        self.paula.write_per(v, p);
    }

    fn filter_on_off(&mut self, c: usize) {
        if self.song.tick == 0 {
            let filter_on = ((self.song.channels[c].n_cmd & 1) ^ 1) as u8;
            self.paula.write_cia_pra(filter_on << 1);
            self.rep.led_filter_enabled = filter_on != 0;
        }
    }

    fn fine_porta_up(&mut self, c: usize) {
        if self.song.tick == 0 {
            self.rep.low_mask = 0xF;
            self.porta_up(c);
        }
    }

    fn fine_porta_down(&mut self, c: usize) {
        if self.song.tick == 0 {
            self.rep.low_mask = 0xF;
            self.porta_down(c);
        }
    }

    fn set_tone_porta(ch: &mut Channel) {
        let note = (ch.n_note as u16 & 0xFFF) as i32;

        let mut i = 0usize;
        loop {
            // period[36] = 0, so i=36 is safe
            if note >= period_at(ch.n_finetune, i) as i32 {
                break;
            }
            i += 1;
            if i >= 37 {
                i = 35;
                break;
            }
        }

        if (ch.n_finetune & 8) != 0 && i > 0 {
            i -= 1;
        }

        ch.n_wantedperiod = period_at(ch.n_finetune, i);
        ch.n_toneportdirec = 0;

        if ch.n_period == ch.n_wantedperiod {
            ch.n_wantedperiod = 0;
        } else if ch.n_period > ch.n_wantedperiod {
            ch.n_toneportdirec = 1;
        }
    }

    fn tone_port_no_change(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        if ch.n_wantedperiod <= 0 {
            return;
        }

        if ch.n_toneportdirec > 0 {
            ch.n_period = ch.n_period.wrapping_sub(ch.n_toneportspeed as i16);
            if ch.n_period <= ch.n_wantedperiod {
                ch.n_period = ch.n_wantedperiod;
                ch.n_wantedperiod = 0;
            }
        } else {
            ch.n_period = ch.n_period.wrapping_add(ch.n_toneportspeed as i16);
            if ch.n_period >= ch.n_wantedperiod {
                ch.n_period = ch.n_wantedperiod;
                ch.n_wantedperiod = 0;
            }
        }

        let ch = *ch;
        if ch.n_glissfunk & 0xF == 0 {
            self.paula.write_per(ch.n_chanindex, ch.n_period as u16);
        } else {
            let mut i = 0usize;
            loop {
                if ch.n_period >= period_at(ch.n_finetune, i) {
                    break;
                }
                i += 1;
                if i >= 37 {
                    i = 35;
                    break;
                }
            }
            self.paula.write_per(ch.n_chanindex, period_at(ch.n_finetune, i) as u16);
        }
    }

    fn tone_portamento(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        if ch.n_cmd & 0xFF > 0 {
            ch.n_toneportspeed = (ch.n_cmd & 0xFF) as u8;
            ch.n_cmd &= 0xFF00;
        }
        self.tone_port_no_change(c);
    }

    fn vibrato2(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        let vibrato_pos = (ch.n_vibratopos >> 2) & 0x1F;
        let vibrato_type = ch.n_wavecontrol & 3;

        let mut data: u16 = match vibrato_type {
            0 => VIBRATO_TABLE[vibrato_pos as usize] as u16,
            1 => {
                if ch.n_vibratopos < 128 {
                    (vibrato_pos as u16) << 3
                } else {
                    255 - ((vibrato_pos as u16) << 3)
                }
            }
            _ => 255,
        };

        data = ((data as i32 * (ch.n_vibratocmd & 0xF) as i32) >> 7) as u16;

        if ch.n_vibratopos < 128 {
            data = (ch.n_period as i32 + data as i32) as u16;
        } else {
            data = (ch.n_period as i32 - data as i32) as u16;
        }

        let v = ch.n_chanindex;
        ch.n_vibratopos = ch.n_vibratopos.wrapping_add((ch.n_vibratocmd >> 2) & 0x3C);
        self.paula.write_per(v, data);
    }

    fn vibrato(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        if ch.n_cmd & 0x0F > 0 {
            ch.n_vibratocmd = (ch.n_vibratocmd & 0xF0) | (ch.n_cmd & 0x0F) as u8;
        }
        if ch.n_cmd & 0xF0 > 0 {
            ch.n_vibratocmd = (ch.n_cmd & 0xF0) as u8 | (ch.n_vibratocmd & 0x0F);
        }
        self.vibrato2(c);
    }

    fn tone_plus_vol_slide(&mut self, c: usize) {
        self.tone_port_no_change(c);
        Self::volume_slide(&mut self.song.channels[c]);
    }

    fn vibrato_plus_vol_slide(&mut self, c: usize) {
        self.vibrato2(c);
        Self::volume_slide(&mut self.song.channels[c]);
    }

    fn tremolo(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];
        if ch.n_cmd & 0x0F > 0 {
            ch.n_tremolocmd = (ch.n_tremolocmd & 0xF0) | (ch.n_cmd & 0x0F) as u8;
        }
        if ch.n_cmd & 0xF0 > 0 {
            ch.n_tremolocmd = (ch.n_cmd & 0xF0) as u8 | (ch.n_tremolocmd & 0x0F);
        }

        let tremolo_pos = (ch.n_tremolopos >> 2) & 0x1F;
        let tremolo_type = (ch.n_wavecontrol >> 4) & 3;

        let mut data: i16 = match tremolo_type {
            0 => VIBRATO_TABLE[tremolo_pos as usize] as i16,
            1 => {
                if ch.n_vibratopos < 128 {
                    // PT bug, should've been n_tremolopos
                    (tremolo_pos as i16) << 3
                } else {
                    255 - ((tremolo_pos as i16) << 3)
                }
            }
            _ => 255,
        };

        data = (((data as u16) as i32 * (ch.n_tremolocmd & 0xF) as i32) >> 6) as i16;

        if ch.n_tremolopos < 128 {
            data = (ch.n_volume as i32 + data as i32) as i16;
            if data > 64 {
                data = 64;
            }
        } else {
            data = (ch.n_volume as i32 - data as i32) as i16;
            if data < 0 {
                data = 0;
            }
        }

        let v = ch.n_chanindex;
        ch.n_tremolopos = ch.n_tremolopos.wrapping_add((ch.n_tremolocmd >> 2) & 0x3C);
        self.paula.write_vol(v, data as u16);
    }

    fn sample_offset(ch: &mut Channel) {
        if ch.n_cmd & 0xFF > 0 {
            ch.n_sampleoffset = (ch.n_cmd & 0xFF) as u8;
        }
        let new_offset: u16 = (ch.n_sampleoffset as u16) << 7;
        if new_offset < ch.n_length {
            ch.n_length -= new_offset;
            ch.n_start = ch.n_start.map(|s| s + ((new_offset as usize) << 1));
        } else {
            ch.n_length = 1;
        }
    }

    fn e_commands(&mut self, c: usize) {
        let ecmd = ((self.song.channels[c].n_cmd & 0x00F0) >> 4) as u8;
        match ecmd {
            0x0 => return self.filter_on_off(c),
            0x1 => return self.fine_porta_up(c),
            0x2 => return self.fine_porta_down(c),
            0x3 => return Self::set_gliss_control(&mut self.song.channels[c]),
            0x4 => return Self::set_vibrato_control(&mut self.song.channels[c]),
            0x5 => return Self::set_fine_tune(&mut self.song.channels[c]),
            0x6 => return self.jump_loop(c),
            0x7 => return Self::set_tremolo_control(&mut self.song.channels[c]),
            0x8 => return self.karplus_strong(c),
            0xE => return self.pattern_delay(c),
            _ => {}
        }

        if self.rep.muted[self.song.channels[c].n_chanindex] {
            return;
        }

        match ecmd {
            0x9 => self.retrig_note(c),
            0xA => self.volume_fine_up(c),
            0xB => self.volume_fine_down(c),
            0xC => self.note_cut(c),
            0xD => self.note_delay(c),
            0xF => self.funk_it(c),
            _ => {}
        }
    }

    fn check_more_effects(&mut self, c: usize) {
        let cmd = ((self.song.channels[c].n_cmd & 0x0F00) >> 8) as u8;
        match cmd {
            0x9 => return Self::sample_offset(&mut self.song.channels[c]),
            0xB => return self.position_jump(c),
            0xD => return self.pattern_break(c),
            0xE => return self.e_commands(c),
            0xF => return self.set_speed(c),
            _ => {}
        }

        let ch = self.song.channels[c];
        if self.rep.muted[ch.n_chanindex] {
            return;
        }

        if cmd == 0xC {
            Self::volume_change(&mut self.song.channels[c]);
            return;
        }

        self.paula.write_per(ch.n_chanindex, ch.n_period as u16);
    }

    fn chkefx2(&mut self, c: usize) {
        self.update_funk(c);

        let ch = self.song.channels[c];
        if ch.n_cmd & 0xFFF == 0 {
            return;
        }

        let cmd = ((ch.n_cmd & 0x0F00) >> 8) as u8;
        match cmd {
            0x0 => return self.arpeggio(c),
            0x1 => return self.porta_up(c),
            0x2 => return self.porta_down(c),
            0x3 => return self.tone_portamento(c),
            0x4 => return self.vibrato(c),
            0x5 => return self.tone_plus_vol_slide(c),
            0x6 => return self.vibrato_plus_vol_slide(c),
            0xE => return self.e_commands(c),
            _ => {}
        }

        self.paula.write_per(ch.n_chanindex, ch.n_period as u16);

        if cmd == 0x7 {
            self.tremolo(c);
        } else if cmd == 0xA {
            Self::volume_slide(&mut self.song.channels[c]);
        }
    }

    fn check_effects(&mut self, c: usize) {
        if self.rep.muted[self.song.channels[c].n_chanindex] {
            return;
        }

        self.chkefx2(c);

        // tremolo sets the volume itself
        let ch = self.song.channels[c];
        if (ch.n_cmd & 0x0F00) >> 8 != 0x7 {
            self.paula.write_vol(ch.n_chanindex, ch.n_volume as i16 as u16);
        }
    }

    fn set_period(&mut self, c: usize) {
        let ch = &mut self.song.channels[c];

        let note = (ch.n_note as u16 & 0xFFF) as i32;
        let mut i = 0usize;
        while i < 37 {
            if note >= PERIOD_TABLE[i] as i32 {
                break;
            }
            i += 1;
        }

        // yes it's safe if i=37 because of zero-padding
        ch.n_period = period_at(ch.n_finetune, i);

        if ch.n_cmd & 0xFF0 != 0xED0 {
            // no note delay
            let ch_copy = *ch;
            let v = ch_copy.n_chanindex;
            self.paula.write_dmacon(ch_copy.n_dmabit); // voice DMA off (turned on in set_dma())

            let ch = &mut self.song.channels[c];
            if ch.n_wavecontrol & 0x04 == 0 {
                ch.n_vibratopos = 0;
            }
            if ch.n_wavecontrol & 0x40 == 0 {
                ch.n_tremolopos = 0;
            }

            self.paula.write_len(v, ch.n_length);
            self.paula.write_ptr(v, ch.n_start);

            if ch.n_start.is_none() {
                ch.n_loopstart = None;
                self.paula.write_len(v, 1);
                ch.n_replen = 1;
            }

            self.paula.write_per(v, ch.n_period as u16);

            self.rep.dmacon_temp |= ch.n_dmabit;
        }
        self.check_more_effects(c);
    }

    fn play_voice(&mut self, c: usize) {
        let ch = self.song.channels[c];
        if ch.n_note == 0 && ch.n_cmd == 0 {
            self.paula.write_per(ch.n_chanindex, ch.n_period as u16);
        }

        let row = self.song.row as usize;
        let pat = self.rep.mod_pattern as usize;
        let note = self.song.patterns[pat][(row * PAULA_VOICES) + ch.n_chanindex];

        let null_ptr = self.paula.null_ptr();
        {
            let ch = &mut self.song.channels[c];
            ch.n_note = note.period as i16;
            ch.n_cmd = ((note.command as u16) << 8) | note.param as u16;

            if (1..=31).contains(&note.sample) {
                ch.n_samplenum = note.sample - 1;
                let s = &self.song.samples[ch.n_samplenum as usize];

                let start = s.offset as usize;
                ch.n_start = Some(start);
                ch.n_finetune = s.fine_tune & 0xF;
                ch.n_volume = s.volume;
                ch.n_length = (s.length >> 1) as u16;
                ch.n_replen = (s.loop_length >> 1) as u16;

                let repeat = (s.loop_start >> 1) as u16;
                if repeat > 0 {
                    ch.n_loopstart = Some(start + ((repeat as usize) << 1));
                    ch.n_wavestart = ch.n_loopstart;
                    ch.n_length = repeat.wrapping_add(ch.n_replen);
                } else {
                    ch.n_loopstart = Some(start);
                    ch.n_wavestart = Some(start);
                }

                // set safe sample space for uninitialized voices
                if ch.n_length == 0 {
                    ch.n_loopstart = Some(null_ptr);
                    ch.n_wavestart = Some(null_ptr);
                }
            }
        }

        let ch = self.song.channels[c];
        if (ch.n_note & 0xFFF) > 0 {
            if ch.n_cmd & 0xFF0 == 0xE50 {
                Self::set_fine_tune(&mut self.song.channels[c]);
                self.set_period(c);
            } else {
                let cmd = (ch.n_cmd & 0x0F00) >> 8;
                if cmd == 3 || cmd == 5 {
                    self.set_vu_meter_height(c);
                    Self::set_tone_porta(&mut self.song.channels[c]);
                    self.check_more_effects(c);
                } else if cmd == 9 {
                    self.check_more_effects(c);
                    self.set_period(c);
                } else {
                    self.set_period(c);
                }
            }
        } else {
            self.check_more_effects(c);
        }
    }

    fn update_ui_positions(&mut self) {
        if self.rep.mod2wav_ongoing {
            return;
        }
        self.song.curr_row = self.song.row;
        self.song.curr_pos = self.rep.mod_pos as u16;
        self.song.curr_pattern = self.rep.mod_pattern as u16;
    }

    fn next_position(&mut self) {
        self.song.row = self.rep.p_break_position;
        self.rep.p_break_position = 0;
        self.rep.pos_jump_assert = false;

        if self.rep.play_mode != PlayMode::Pattern {
            self.rep.mod_pos = (self.rep.mod_pos + 1) & 127;
            if self.rep.mod_pos >= self.song.song_length as i16 {
                self.rep.mod_pos = 0;

                if self.rep.compo_mode {
                    self.do_stop_it();
                    self.turn_off_voices();
                    self.rep.mod_pos = 0;
                    self.rep.mod_pattern = self.song.pattern_table[0] as i8;
                    self.song.row = 0;
                    self.update_ui_positions();
                }

                if self.rep.mod2wav_ongoing {
                    self.rep.mod_render_done = true;
                }
            }

            self.rep.mod_pattern = self.song.pattern_table[self.rep.mod_pos as usize] as i8;
            if self.rep.mod_pattern > MAX_PATTERNS as i8 - 1 {
                self.rep.mod_pattern = MAX_PATTERNS as i8 - 1;
            }
        }
    }

    fn increase_playback_timer(&mut self) {
        let bpm = self.rep.mod_bpm;
        if self.rep.play_mode != PlayMode::Pattern && (MIN_BPM..=MAX_BPM).contains(&bpm) {
            let hz = if self.rep.timing_mode == TimingMode::Cia { cia_bpm_to_hz(bpm) } else { AMIGA_PAL_VBLANK_HZ };
            self.rep.playback_seconds += 1.0 / hz;
            if self.rep.playback_seconds >= (99.0 * 60.0) + 60.0 {
                self.rep.playback_seconds = 0.0;
            }
        }
    }

    fn set_curr_row_to_visited(&mut self) {
        if self.rep.mod2wav_ongoing {
            let idx = self.rep.mod_pos as i32 * MOD_ROWS as i32 + self.song.row as i32;
            if let Some(v) = self.rep.row_visit_table.get_mut(idx as usize) {
                *v = true;
            }
        }
    }

    fn render_end_check(&mut self) -> bool {
        if !self.rep.mod2wav_ongoing {
            return true;
        }
        if self.rep.patt_del_time2 == 0 && self.song.tick == self.song.speed - 1 {
            let idx = self.rep.mod_pos as i32 * MOD_ROWS as i32 + self.song.row as i32;
            let visited = self.rep.row_visit_table.get(idx as usize).copied().unwrap_or(false);
            if visited || self.rep.mod_render_done {
                self.rep.mod_render_done = false;
                return false; // we're done rendering
            }
        }
        true
    }

    fn set_dma(&mut self) {
        for i in 0..PAULA_VOICES {
            if self.rep.muted[i] {
                self.rep.dmacon_temp &= !(1 << i);
            }
        }

        self.paula.write_dmacon(0x8000 | self.rep.dmacon_temp);

        for i in 0..PAULA_VOICES {
            let ch = self.song.channels[i];
            if self.rep.dmacon_temp & ch.n_dmabit != 0 {
                self.set_vu_meter_height(i);
            }
            // these take effect after the current DMA cycle is done
            self.paula.write_ptr(i, ch.n_loopstart);
            self.paula.write_len(i, ch.n_replen);
        }
    }

    /// Advances the replayer by one tick.
    /// Returns `false` when MOD2WAV rendering has reached the end of the song.
    pub fn tick_replayer(&mut self) -> bool {
        // quirk: CIA BPM changes are delayed by one tick in PT
        if self.rep.cia_set_bpm != -1 {
            let new_bpm = self.rep.cia_set_bpm;
            self.mod_set_tempo(new_bpm);
            self.rep.cia_set_bpm = -1;
        }

        self.increase_playback_timer();
        self.song.tick += 1;

        let mut read_new_note = false;
        if self.song.tick as u32 >= self.song.speed as u32 {
            self.song.tick = 0;
            read_new_note = true;
        }

        if read_new_note {
            if self.rep.patt_del_time2 == 0 {
                self.rep.dmacon_temp = 0;

                self.set_curr_row_to_visited();
                self.update_ui_positions();

                for i in 0..PAULA_VOICES {
                    self.play_voice(i);
                    let vol = self.song.channels[i].n_volume;
                    self.paula.write_vol(i, vol as i16 as u16);
                }

                self.set_dma();
            } else {
                for i in 0..PAULA_VOICES {
                    self.check_effects(i);
                }
            }

            self.song.row = self.song.row.wrapping_add(1);
            self.song.rows_counter = self.song.rows_counter.wrapping_add(1);

            if self.rep.patt_del_time > 0 {
                self.rep.patt_del_time2 = self.rep.patt_del_time;
                self.rep.patt_del_time = 0;
            }

            if self.rep.patt_del_time2 > 0 {
                self.rep.patt_del_time2 -= 1;
                if self.rep.patt_del_time2 > 0 {
                    self.song.row = self.song.row.wrapping_sub(1);
                    self.song.rows_counter = self.song.rows_counter.wrapping_sub(1);
                }
            }

            if self.rep.p_break_flag {
                self.song.row = self.rep.p_break_position;
                self.rep.p_break_position = 0;
                self.rep.p_break_flag = false;
            }

            if self.song.row as i32 >= MOD_ROWS as i32 || self.rep.pos_jump_assert {
                self.next_position();
            }
        } else {
            for i in 0..PAULA_VOICES {
                self.check_effects(i);
            }
            if self.rep.pos_jump_assert {
                self.next_position();
            }
        }

        // command F00 = stop song
        if self.rep.do_stop_song {
            self.rep.do_stop_song = false;
            self.rep.song_playing = false;
            self.rep.play_mode = PlayMode::Normal;
        }

        self.render_end_check()
    }
}
