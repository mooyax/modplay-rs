//! Scream Tracker 3 (.S3M) loader. The module is converted to the IT data model
//! (`ItModule` with `format = Format::S3m`); S3M effect letters are the same as IT's.
//! Follows OpenMPT's Load_s3m.cpp / S3MTools.cpp.

use super::format::{byte, text, u16le, u32le, Cell, Format, ItError, ItModule, Pattern, Sample, MAX_CHANNELS, NOTE_CUT, VOL_NONE};

pub fn detect(d: &[u8]) -> bool {
    d.len() >= 0x60 && &d[0x2C..0x30] == b"SCRM" && byte(d, 0x1D) == 0x10
}

pub fn load(d: &[u8]) -> Result<ItModule, ItError> {
    if !detect(d) {
        return Err(ItError::NotIt);
    }
    let ordnum = u16le(d, 0x20) as usize;
    let smpnum = (u16le(d, 0x22) as usize).min(255);
    let patnum = (u16le(d, 0x24) as usize).min(255);
    let flags = u16le(d, 0x26);
    let cwtv = u16le(d, 0x28);
    let fmt_version = u16le(d, 0x2A); // 1 = signed samples, 2 = unsigned
    let gvol = byte(d, 0x30);
    let speed = byte(d, 0x31);
    let tempo = byte(d, 0x32);
    let master = byte(d, 0x33);
    let use_pan_table = byte(d, 0x35) == 0xFC;
    let channels: Vec<u8> = (0..32).map(|i| byte(d, 0x40 + i)).collect();

    let tracker = cwtv & 0xF000;
    let is_st3 = tracker == 0x1000;
    let num_channels = channels.iter().rposition(|&c| c != 0xFF).map(|i| i + 1).unwrap_or(0).max(4);

    let mut p = 0x60;
    if d.len() < p + ordnum + (smpnum + patnum) * 2 {
        return Err(ItError::Truncated("header"));
    }
    let orders: Vec<u8> = d[p..p + ordnum].to_vec();
    p += ordnum;
    let smp_ptr: Vec<usize> = (0..smpnum).map(|i| u16le(d, p + i * 2) as usize * 16).collect();
    p += smpnum * 2;
    let pat_ptr: Vec<usize> = (0..patnum).map(|i| u16le(d, p + i * 2) as usize * 16).collect();
    p += patnum * 2;

    // ---- which tracker saved this? (OpenMPT's heuristics) ----
    let special = u16le(d, 0x3E);
    let ultra_clicks = byte(d, 0x34);
    let offsets_canonical = !pat_ptr.is_empty() && !smp_ptr.is_empty() && pat_ptr[0] > smp_ptr[0];
    // ModPlug Tracker / OpenMPT saving as "ST3.20"
    let modplug = cwtv == 0x1320 && special == 0 && ordnum & 1 == 0 && ultra_clicks == 0 && flags & !0x50 == 0 && use_pan_table && offsets_canonical;
    // other trackers do not have Scream Tracker's playback quirks
    let non_compat = modplug || matches!(tracker, 0x2000 | 0x3000 | 0x4000);
    let st3_quirks = !non_compat;
    // really made with Scream Tracker 3? (samples with GUS addresses tell)
    let mut gus: u16 = 0;
    let mut any_samples = false;
    for &o in &smp_ptr {
        if o != 0 && o + 0x50 <= d.len() && byte(d, o) < 2 {
            if u32le(d, o + 0x10) != 0 {
                any_samples = true;
            }
            gus |= u16le(d, o + 0x28);
        }
    }
    let use_gus = gus > 1;
    let real_st3 = is_st3 && !modplug && !(any_samples && gus == 0 && cwtv != 0x1300);
    let saved_by_modplug = modplug && ordnum & 0x0F == 0;

    // ---- mixing level ("sample pre-amp") and panning ----
    let stereo = master & 0x80 != 0 || saved_by_modplug;
    let mut preamp: u32 = if fmt_version == 1 && master < 8 {
        ((master as u32 + 1) * 0x10).min(0x7F)
    } else if master == 2 || master == 0x12 {
        0x20
    } else if master & 0x7F == 0 {
        48
    } else {
        ((master & 0x7F) as u32).max(0x10)
    };
    if !stereo {
        preamp = (preamp * 8 + 5) / 11;
    }
    if real_st3 && use_gus {
        preamp = 48;
    }

    let mut pan256 = vec![128i32; MAX_CHANNELS];
    let mut disabled = vec![false; MAX_CHANNELS];
    let mut adlib = [false; 32];
    for i in 0..32 {
        let c = channels[i];
        let ctype = c & 0x7F;
        if c != 0xFF && stereo {
            pan256[i] = if ctype & 8 != 0 { 0xCC } else { 0x33 };
        }
        if c != 0xFF && c & 0x80 != 0 && st3_quirks {
            disabled[i] = true;
        }
        if (16..=29).contains(&ctype) {
            pan256[i] = 128;
            adlib[i] = true;
        }
        if c == 0xFF || ctype >= 16 {
            // unused or AdLib channel: nothing we can play
            disabled[i] = disabled[i] || c == 0xFF;
        }
    }
    if use_pan_table && p + 32 <= d.len() {
        for i in 0..32 {
            let v = d[p + i];
            if v & 0x20 != 0 && (!is_st3 || !adlib[i]) {
                pan256[i] = ((v & 0x0F) as i32 * 256 + 8) / 15;
            }
        }
    }
    let mut chn_pan = [32u8; MAX_CHANNELS];
    for i in 0..MAX_CHANNELS {
        chn_pan[i] = ((pan256[i] + 2) / 4).clamp(0, 64) as u8;
    }

    // ---- samples ----
    let mut samples = Vec::with_capacity(smpnum);
    for &o in &smp_ptr {
        samples.push(load_sample(d, o, fmt_version != 2, is_st3));
    }

    // ---- patterns (always 64 rows) ----
    let mut patterns = Vec::with_capacity(patnum);
    for &o in &pat_ptr {
        let mut pat = Pattern::empty(64);
        if o != 0 && o + 2 <= d.len() {
            let mut q = o + 2;
            let mut row = 0;
            while row < 64 && q < d.len() {
                let info = d[q];
                q += 1;
                if info == 0 {
                    row += 1;
                    continue;
                }
                let ch = (info & 31) as usize;
                let mut cell = Cell { vol: VOL_NONE, ..Default::default() };
                if info & 0x20 != 0 {
                    let note = byte(d, q);
                    cell.ins = byte(d, q + 1);
                    q += 2;
                    cell.note = if note < 0xF0 {
                        ((note & 0x0F) as u32 + 12 * (note >> 4) as u32 + 13).clamp(1, 120) as u8
                    } else if note == 0xFE {
                        NOTE_CUT
                    } else {
                        0
                    };
                }
                if info & 0x40 != 0 {
                    let v = byte(d, q);
                    q += 1;
                    // 128..192 = panning (same encoding as IT's volume column)
                    cell.vol = if (128..=192).contains(&v) { v } else { v.min(64) };
                }
                if info & 0x80 != 0 {
                    let (cmd, param) = (byte(d, q), byte(d, q + 1));
                    q += 2;
                    let (cmd, param) = convert_effect(cmd, param, cwtv, is_st3);
                    cell.cmd = cmd;
                    cell.param = param;
                }
                if ch < num_channels {
                    pat.cells[row * MAX_CHANNELS + ch] = cell;
                }
            }
        }
        patterns.push(pat);
    }

    let mut m = ItModule {
        format: Format::S3m,
        title: text(d, 0, 28),
        stereo,
        instrument_mode: false,
        linear_slides: false,
        old_effects: false,
        compat_gxx: false,
        // S3M 0..64 -> IT units 0..128
        global_vol: if gvol == 0 && cwtv < 0x1320 { 128 } else { gvol.min(64) * 2 },
        mix_vol: preamp.min(128) as u8,
        speed: if speed == 0 || (speed == 255 && is_st3) { 6 } else { speed },
        tempo: if tempo < 33 { 125 } else { tempo },
        separation: 128,
        chn_pan,
        chn_vol: [64; MAX_CHANNELS],
        orders,
        instruments: Vec::new(),
        samples,
        patterns,
        num_channels,
        cwtv,
        cmwt: 0,
        ext_filter_range: false,
        midi_sfx: vec![String::new(); 16],
        midi_zxx: vec![String::new(); 128],
        fast_vol_slides: cwtv == 0x1300 || flags & 0x40 != 0,
        st2_vibrato: flags & 0x01 != 0,
        amiga_limits: flags & 0x10 != 0,
        chn_pan256: Some(pan256),
        chn_disabled: disabled,
        st3_quirks,
        st3_no_combined_first_tick: real_st3,
        legacy_mix: false,
        mix_mode_gain: 1.0,
    };
    // MIDI macros are only kept for trackers other than Scream Tracker
    if tracker > 0x1000 && (tracker != 0x3000 || cwtv >= 0x3214) {
        m.midi_sfx[0] = "F0F000z".into();
        for i in 0..16 {
            m.midi_zxx[i] = format!("F0F001{:02X}", i * 8);
        }
    }
    Ok(m)
}

/// S3M effect letter -> IT effect (same letters), with S3M-specific parameter fixes
fn convert_effect(cmd: u8, param: u8, cwtv: u16, is_st3: bool) -> (u8, u8) {
    match cmd {
        // C: pattern break, the row number is BCD in S3M
        3 => (3, (param >> 4) * 10 + (param & 0x0F)),
        // S: old SoundBlaster stereo control (ST3 < 3.20) is not supported
        19 if param & 0xF0 == 0xA0 && cwtv < 0x1320 => (0, 0),
        // O00 did nothing in ST3.00/3.01
        15 if param == 0 && is_st3 && cwtv <= 0x1301 => (0, 0),
        1..=26 => (cmd, param),
        _ => (0, 0),
    }
}

fn load_sample(d: &[u8], o: usize, signed: bool, is_st3: bool) -> Sample {
    let mut s = Sample {
        name: text(d, o + 0x30, 28),
        global_vol: 64,
        volume: 0,
        pan: None,
        length: 0,
        looped: false,
        pingpong: false,
        loop_start: 0,
        loop_end: 0,
        sus_loop: false,
        sus_pingpong: false,
        sus_start: 0,
        sus_end: 0,
        c5speed: 8363,
        vib_speed: 0,
        vib_depth: 0,
        vib_rate: 0,
        vib_type: 0,
        data_l: Vec::new(),
        data_r: None,
    };
    if o == 0 || o + 0x50 > d.len() {
        return s;
    }
    let typ = byte(d, o);
    s.volume = byte(d, o + 0x1C).min(64);
    let mut c5 = u32le(d, o + 0x20);
    if is_st3 {
        c5 = c5.min(0xFFFF);
    }
    s.c5speed = if c5 == 0 { 8363 } else { c5 };
    if typ != 1 {
        return s; // empty slot or AdLib instrument
    }
    let flags = byte(d, o + 0x1F);
    let pack = byte(d, o + 0x1E);
    let is16 = flags & 4 != 0;
    let is_stereo = flags & 2 != 0;
    let mut len = u32le(d, o + 0x10) as usize;
    let ls = u32le(d, o + 0x14) as usize;
    let le = u32le(d, o + 0x18) as usize;
    let off = ((byte(d, o + 0x0D) as usize) << 20) | ((byte(d, o + 0x0E) as usize) << 4) | ((byte(d, o + 0x0F) as usize) << 12);
    if pack == 4 && !is16 && !is_stereo {
        // ModPlug ADPCM: 16-byte table followed by 4-bit deltas
        let table: Vec<i8> = d.get(off..off + 16).map(|t| t.iter().map(|&b| b as i8).collect()).unwrap_or_default();
        if table.len() == 16 {
            let bytes = (len + 1) / 2;
            let src = &d[(off + 16).min(d.len())..(off + 16 + bytes).min(d.len())];
            let mut v: i8 = 0;
            let mut out = Vec::with_capacity(len);
            for &b in src {
                for nib in [b & 0x0F, b >> 4] {
                    if out.len() < len {
                        v = v.wrapping_add(table[nib as usize]);
                        out.push((v as i16) << 8);
                    }
                }
            }
            len = out.len();
            s.data_l = out;
        }
    } else {
        let bps = if is16 { 2 } else { 1 };
        // never read past the end of the file
        let avail = d.len().saturating_sub(off) / bps / if is_stereo { 2 } else { 1 };
        len = len.min(avail);
        let read = |start: usize| -> Vec<i16> {
            (0..len)
                .map(|i| {
                    if is16 {
                        let v = u16le(d, start + i * 2);
                        (if signed { v } else { v ^ 0x8000 }) as i16
                    } else {
                        let v = d[start + i];
                        ((if signed { v } else { v ^ 0x80 }) as i8 as i16) << 8
                    }
                })
                .collect()
        };
        s.data_l = read(off);
        if is_stereo {
            s.data_r = Some(read(off + len * bps));
        }
    }
    s.length = len;
    if flags & 1 != 0 && len > 0 {
        let ls = ls.min(len - 1);
        let le = le.min(len);
        if le >= 2 && ls < le {
            s.looped = true;
            s.loop_start = ls;
            s.loop_end = le;
        }
    }
    s
}
