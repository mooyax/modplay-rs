//! FastTracker II (.XM) loader, following ft2-clone (ft2_load_xm.c, sanitizeSample,
//! sanitizeInstrument). Sample data is stored as 16-bit (8-bit data scaled by 256);
//! stereo samples are mixed down to mono like FT2 does.

use std::fmt;

pub const NOTE_OFF: u8 = 97;
/// FT2 itself has 32 channels; OpenMPT (and libopenmpt) allow up to 127 in XM files
pub const MAX_CHANNELS: usize = 127;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Note {
    /// 0 = none, 1..96 = C-0..B-7, 97 = key off
    pub note: u8,
    pub instr: u8,
    /// raw volume column byte
    pub vol: u8,
    /// 0..35 (0..F, then G..Z)
    pub efx: u8,
    pub param: u8,
}

#[derive(Clone, Debug, Default)]
pub struct XmSample {
    pub name: String,
    pub length: usize,
    pub loop_start: usize,
    pub loop_len: usize,
    /// 0 = off, 1 = forward, 2 = ping-pong
    pub loop_type: u8,
    pub volume: u8,
    pub finetune: i8,
    pub panning: u8,
    pub relative_note: i8,
    pub is16: bool,
    pub data: Vec<i16>,
}

#[derive(Clone, Debug)]
pub struct XmInstrument {
    pub name: String,
    pub note2sample: [u8; 96],
    pub vol_env: [(i16, i16); 12],
    pub pan_env: [(i16, i16); 12],
    pub vol_len: u8,
    pub pan_len: u8,
    pub vol_sus: u8,
    pub vol_loop_start: u8,
    pub vol_loop_end: u8,
    pub pan_sus: u8,
    pub pan_loop_start: u8,
    pub pan_loop_end: u8,
    /// bit 0 enabled, bit 1 sustain, bit 2 loop
    pub vol_flags: u8,
    pub pan_flags: u8,
    pub vib_type: u8,
    pub vib_sweep: u8,
    pub vib_depth: u8,
    pub vib_rate: u8,
    pub fadeout: u16,
    pub mute: bool,
    /// always 16 entries
    pub samples: Vec<XmSample>,
}

impl Default for XmInstrument {
    fn default() -> Self {
        XmInstrument {
            name: String::new(),
            note2sample: [0; 96],
            vol_env: [(0, 0); 12],
            pan_env: [(0, 0); 12],
            vol_len: 0,
            pan_len: 0,
            vol_sus: 0,
            vol_loop_start: 0,
            vol_loop_end: 0,
            pan_sus: 0,
            pan_loop_start: 0,
            pan_loop_end: 0,
            vol_flags: 0,
            pan_flags: 0,
            vib_type: 0,
            vib_sweep: 0,
            vib_depth: 0,
            vib_rate: 0,
            fadeout: 0,
            mute: false,
            // FT2's allocateInstr(): unused sample slots have volume 64 and centre panning
            samples: vec![XmSample { volume: 64, panning: 128, ..Default::default() }; 16],
        }
    }
}

#[derive(Clone, Debug)]
pub struct XmPattern {
    pub rows: usize,
    /// channels per row (= the song's channel count)
    pub stride: usize,
    /// rows * stride
    pub notes: Vec<Note>,
}

impl XmPattern {
    #[inline]
    pub fn note(&self, row: usize, ch: usize) -> Note {
        if ch >= self.stride {
            return Note::default();
        }
        self.notes.get(row * self.stride + ch).copied().unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct XmModule {
    pub title: String,
    pub tracker: String,
    pub song_length: usize,
    pub restart: usize,
    pub num_channels: usize,
    pub orders: [u8; 256],
    pub linear: bool,
    pub speed: u16,
    pub bpm: u16,
    pub patterns: Vec<XmPattern>,
    /// index 0 = FT2's empty placeholder instrument, 1..=128 = instruments (None = empty)
    pub instruments: Vec<Option<XmInstrument>>,
    pub instr_names: Vec<String>,
    /// libopenmpt plays files from FT2 (and FT2-compatible trackers) with FT2's
    /// mix levels and square-root panning law ("CompatibleFT2"); others linearly
    pub ft2_mix: bool,
}

/// libopenmpt's choice of mix levels for an XM (Load_xm.cpp)
fn detect_ft2_mix(d: &[u8], tracker_raw: &[u8], version: u16, header_size: usize) -> bool {
    if tracker_raw.starts_with(b"Fasttracker II clone") {
        return true;
    }
    if tracker_raw == b"FastTracker v2.00   " && header_size == 276 {
        if version < 0x0104 {
            return true;
        }
        // FT2 pads the song name with spaces, ModPlug with zero bytes
        let song_name = &d[17..37];
        return !song_name.contains(&0);
    }
    if tracker_raw.starts_with(b"MilkyTracker ") {
        return &tracker_raw[12..20] != b"        ";
    }
    if let Some(v) = tracker_raw.strip_prefix(b"OpenMPT ") {
        // version >= 1.22.07.19
        let s: String = v.iter().map(|&c| c as char).collect();
        let parts: Vec<u32> = s.trim().split('.').filter_map(|x| u32::from_str_radix(x.trim(), 16).ok()).collect();
        let key = parts.iter().take(4).fold(0u64, |a, &x| a * 256 + x as u64) << (8 * (4 - parts.len().min(4))) as u64;
        return key >= 0x0122_0719;
    }
    false
}

#[derive(Debug)]
pub enum XmError {
    NotXm,
    Unsupported(String),
    Truncated,
}

impl fmt::Display for XmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmError::NotXm => write!(f, "not a FastTracker II module"),
            XmError::Unsupported(s) => write!(f, "unsupported XM: {s}"),
            XmError::Truncated => write!(f, "truncated XM file"),
        }
    }
}
impl std::error::Error for XmError {}

pub fn detect(d: &[u8]) -> bool {
    d.len() >= 60 && &d[0..17] == b"Extended Module: "
}

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.d.get(self.p).copied().unwrap_or(0);
        self.p += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        self.u8() as u16 | (self.u8() as u16) << 8
    }
    fn u32(&mut self) -> u32 {
        self.u16() as u32 | (self.u16() as u32) << 16
    }
    fn bytes(&mut self, n: usize) -> &'a [u8] {
        let s = self.p.min(self.d.len());
        let e = (self.p + n).min(self.d.len());
        self.p += n;
        &self.d[s..e]
    }
    fn eof(&self) -> bool {
        self.p >= self.d.len()
    }
}

fn name(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| if (32..127).contains(&c) { c as char } else { ' ' }).collect::<String>().trim_end().to_string()
}

pub fn load(d: &[u8]) -> Result<XmModule, XmError> {
    if !detect(d) {
        return Err(XmError::NotXm);
    }
    let mut r = Rd { d, p: 17 };
    let title = name(r.bytes(20));
    r.u8(); // 0x1A
    let tracker_raw: Vec<u8> = d[38..58].to_vec();
    let tracker = name(r.bytes(20));
    let version = r.u16();
    if !(0x0102..=0x0104).contains(&version) {
        return Err(XmError::Unsupported(format!("version {:x}", version)));
    }
    let header_size = r.u32() as usize;
    let num_orders = r.u16() as usize;
    let restart = r.u16() as usize;
    let num_channels = r.u16() as usize;
    let num_patterns = r.u16() as usize;
    let num_instr = r.u16() as usize;
    let flags = r.u16();
    let speed = r.u16();
    let bpm = r.u16();
    let mut orders = [0u8; 256];
    orders.copy_from_slice(&{
        let mut o = [0u8; 256];
        let b = r.bytes(256);
        o[..b.len()].copy_from_slice(b);
        o
    });
    if num_orders > 256 || num_patterns > 256 || num_channels == 0 || num_channels > MAX_CHANNELS || num_instr > 256 {
        return Err(XmError::Unsupported("bad header".into()));
    }
    r.p = 60 + header_size;

    let mut song_length = if num_orders == 0 { 1 } else { num_orders };
    if num_orders == 0 {
        orders = [0; 256];
    } else {
        for o in orders.iter_mut().skip(num_orders) {
            *o = 0;
        }
    }
    for i in (0..256).rev() {
        if orders[i] != 0xFF {
            break;
        }
        if song_length > i {
            song_length = i;
        }
    }
    song_length = song_length.clamp(1, 255);

    let channels = num_channels.min(MAX_CHANNELS);
    let mut patterns: Vec<XmPattern> = Vec::new();
    let mut instruments: Vec<Option<XmInstrument>> = vec![None; 129];
    // FT2's placeholder for empty instruments: like a new instrument, but silent
    let mut placeholder = XmInstrument::default();
    placeholder.samples[0].volume = 0;
    instruments[0] = Some(placeholder);
    let mut instr_names = vec![String::new(); 129];
    let mut pending_samples: Vec<(usize, Vec<u32>)> = Vec::new();

    let load_patterns = |r: &mut Rd, patterns: &mut Vec<XmPattern>| -> Result<(), XmError> {
        for _ in 0..num_patterns {
            let hsize = r.u32() as usize;
            let start = r.p - 4;
            let _typ = r.u8();
            let (rows, data_size);
            if version == 0x0102 {
                rows = r.u8() as usize + 1;
                data_size = r.u16() as usize;
                if hsize > 8 {
                    r.p = start + 4 + hsize - 4;
                }
            } else {
                rows = r.u16() as usize;
                data_size = r.u16() as usize;
                if hsize > 9 {
                    r.p = start + hsize;
                }
            }
            if r.eof() && data_size > 0 {
                return Err(XmError::Truncated);
            }
            let rows = rows.min(256);
            let mut notes = vec![Note::default(); rows.max(1) * channels];
            if data_size > 0 {
                let src = r.bytes(data_size);
                unpack(&mut notes, src, rows, num_channels, channels);
            }
            let empty = notes.iter().all(|n| *n == Note::default());
            let rows = if empty || rows == 0 { 64 } else { rows };
            if notes.len() < rows * channels {
                notes.resize(rows * channels, Note::default());
            }
            patterns.push(XmPattern { rows, stride: channels, notes });
        }
        Ok(())
    };

    // instrument header + sample headers; returns sample byte lengths in file
    let load_instr_header = |r: &mut Rd, idx: usize, instruments: &mut Vec<Option<XmInstrument>>, names: &mut Vec<String>| -> Result<Vec<u32>, XmError> {
        let start = r.p;
        let mut read_size = r.u32() as usize;
        r.p = start;
        if read_size == 0 || read_size > 263 {
            read_size = 263;
        }
        let mut h = [0u8; 263];
        let b = r.bytes(read_size);
        h[..b.len()].copy_from_slice(b);
        let instr_size = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
        if instr_size > 263 {
            r.p = start + instr_size;
        }
        let num_samples = i16::from_le_bytes([h[27], h[28]]);
        if !(0..=32).contains(&num_samples) {
            return Err(XmError::Unsupported("bad instrument".into()));
        }
        if idx < 128 {
            names[1 + idx] = name(&h[4..26]);
        }
        if num_samples == 0 {
            return Ok(Vec::new());
        }
        let mut ins = XmInstrument { name: name(&h[4..26]), ..Default::default() };
        ins.note2sample.copy_from_slice(&h[33..129]);
        let rd16 = |o: usize| i16::from_le_bytes([h[o], h[o + 1]]);
        for i in 0..12 {
            ins.vol_env[i] = (rd16(129 + i * 4), rd16(131 + i * 4));
            ins.pan_env[i] = (rd16(177 + i * 4), rd16(179 + i * 4));
        }
        ins.vol_len = h[225];
        ins.pan_len = h[226];
        ins.vol_sus = h[227];
        ins.vol_loop_start = h[228];
        ins.vol_loop_end = h[229];
        ins.pan_sus = h[230];
        ins.pan_loop_start = h[231];
        ins.pan_loop_end = h[232];
        ins.vol_flags = h[233];
        ins.pan_flags = h[234];
        ins.vib_type = h[235];
        ins.vib_sweep = h[236];
        ins.vib_depth = h[237];
        ins.vib_rate = h[238];
        ins.fadeout = u16::from_le_bytes([h[239], h[240]]);
        ins.mute = h[247] == 1;
        sanitize_instrument(&mut ins);

        let n = num_samples as usize;
        let mut lens = Vec::new();
        for j in 0..n {
            let sh = r.bytes(40);
            let mut s = [0u8; 40];
            s[..sh.len()].copy_from_slice(sh);
            let len = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
            lens.push(len);
            if j >= 16 {
                continue;
            }
            let smp = &mut ins.samples[j];
            smp.length = len as usize;
            smp.loop_start = u32::from_le_bytes([s[4], s[5], s[6], s[7]]) as usize;
            smp.loop_len = u32::from_le_bytes([s[8], s[9], s[10], s[11]]) as usize;
            smp.volume = s[12];
            smp.finetune = s[13] as i8;
            let f = s[14];
            smp.loop_type = f & 3;
            smp.is16 = f & 16 != 0;
            smp.panning = s[15];
            smp.relative_note = s[16] as i8;
            // flags: 16 = 16-bit, 32 = stereo, nameLength 0xAD = ModPlug ADPCM
            smp.name = name(&s[18..40]);
            // keep raw flags for the sample-data pass
            smp.data = vec![f as i16, if s[17] == 0xAD && f & (16 | 32) == 0 { 1 } else { 0 }];
        }
        if idx < 128 {
            instruments[1 + idx] = Some(ins);
        }
        Ok(lens)
    };

    let load_instr_samples = |r: &mut Rd, idx: usize, lens: &[u32], instruments: &mut Vec<Option<XmInstrument>>| {
        let Some(Some(ins)) = instruments.get_mut(1 + idx) else {
            // empty or skipped instrument: skip its sample data
            for &l in lens {
                r.p += l as usize;
            }
            return;
        };
        for (j, &len_bytes) in lens.iter().enumerate() {
            if j >= 16 {
                r.p += len_bytes as usize;
                continue;
            }
            let s = &mut ins.samples[j];
            let (flags, adpcm) = if s.data.len() == 2 { (s.data[0] as u8, s.data[1] == 1) } else { (0, false) };
            s.data.clear();
            if len_bytes == 0 {
                *s = XmSample { name: s.name.clone(), volume: s.volume, finetune: s.finetune, panning: s.panning, relative_note: s.relative_note, ..Default::default() };
                sanitize_sample(s);
                continue;
            }
            let is16 = flags & 16 != 0;
            let stereo = flags & 32 != 0;
            if is16 {
                s.length >>= 1;
                s.loop_start >>= 1;
                s.loop_len >>= 1;
            }
            if adpcm {
                let table: Vec<i8> = r.bytes(16).iter().map(|&b| b as i8).collect();
                let bytes = (s.length + 1) / 2;
                let src = r.bytes(bytes);
                let mut v: i8 = 0;
                let mut out = Vec::with_capacity(s.length + 1);
                for &b in src {
                    for nib in [b & 0x0F, b >> 4] {
                        v = v.wrapping_add(*table.get(nib as usize).unwrap_or(&0));
                        out.push((v as i16) << 8);
                    }
                }
                out.truncate(s.length);
                out.resize(s.length, 0);
                s.data = out;
            } else {
                let src = r.bytes(len_bytes as usize);
                let n = s.length;
                // delta decoding (and stereo mixdown, like delta2Samp)
                if is16 {
                    let raw: Vec<i16> = (0..n).map(|i| if i * 2 + 1 < src.len() { i16::from_le_bytes([src[i * 2], src[i * 2 + 1]]) } else { 0 }).collect();
                    s.data = delta16(&raw, stereo);
                } else {
                    let raw: Vec<i8> = (0..n).map(|i| *src.get(i).unwrap_or(&0) as i8).collect();
                    s.data = delta8(&raw, stereo);
                }
                if stereo {
                    s.length >>= 1;
                    s.loop_start >>= 1;
                    s.loop_len >>= 1;
                }
            }
            s.data.truncate(s.length);
            s.is16 = is16;
            sanitize_sample(s);
        }
    };

    if version < 0x0104 {
        let mut all = Vec::new();
        for i in 0..num_instr {
            all.push(load_instr_header(&mut r, i, &mut instruments, &mut instr_names)?);
        }
        load_patterns(&mut r, &mut patterns)?;
        for (i, lens) in all.iter().enumerate() {
            load_instr_samples(&mut r, i, lens, &mut instruments);
        }
    } else {
        load_patterns(&mut r, &mut patterns)?;
        for i in 0..num_instr {
            let lens = load_instr_header(&mut r, i, &mut instruments, &mut instr_names)?;
            load_instr_samples(&mut r, i, &lens, &mut instruments);
            pending_samples.clear();
        }
    }
    if patterns.is_empty() {
        patterns.push(XmPattern { rows: 64, stride: channels, notes: vec![Note::default(); 64 * channels] });
    }

    Ok(XmModule {
        title,
        tracker,
        song_length,
        restart,
        num_channels: channels,
        orders,
        linear: flags & 1 != 0,
        speed: if speed == 0 { 6 } else { speed },
        bpm: if bpm == 0 { 125 } else { bpm },
        patterns,
        instruments,
        instr_names,
        // more than 32 channels cannot come from FT2: libopenmpt then uses the compatible levels
        ft2_mix: channels <= 32 && detect_ft2_mix(d, &tracker_raw, version, header_size),
    })
}

fn unpack(notes: &mut [Note], src: &[u8], rows: usize, num_channels: usize, stride: usize) {
    let mut q = 0usize;
    let mut next = || -> u8 {
        let v = src.get(q).copied().unwrap_or(0);
        q += 1;
        v
    };
    for row in 0..rows {
        for ch in 0..num_channels {
            let b = next();
            let n = if b & 0x80 != 0 {
                Note {
                    note: if b & 1 != 0 { next() } else { 0 },
                    instr: if b & 2 != 0 { next() } else { 0 },
                    vol: if b & 4 != 0 { next() } else { 0 },
                    efx: if b & 8 != 0 { next() } else { 0 },
                    param: if b & 16 != 0 { next() } else { 0 },
                }
            } else {
                Note { note: b, instr: next(), vol: next(), efx: next(), param: next() }
            };
            if ch < stride {
                notes[row * stride + ch] = n;
            }
        }
    }
}

fn delta8(raw: &[i8], stereo: bool) -> Vec<i16> {
    if stereo {
        let n = raw.len() / 2;
        let (mut l, mut r) = (0i8, 0i8);
        (0..n)
            .map(|i| {
                l = l.wrapping_add(raw[i]);
                r = r.wrapping_add(raw[n + i]);
                ((((l as i16) + (r as i16)) >> 1) as i8 as i16) << 8
            })
            .collect()
    } else {
        let mut v = 0i8;
        raw.iter()
            .map(|&x| {
                v = v.wrapping_add(x);
                (v as i16) << 8
            })
            .collect()
    }
}

fn delta16(raw: &[i16], stereo: bool) -> Vec<i16> {
    if stereo {
        let n = raw.len() / 2;
        let (mut l, mut r) = (0i16, 0i16);
        (0..n)
            .map(|i| {
                l = l.wrapping_add(raw[i]);
                r = r.wrapping_add(raw[n + i]);
                ((l as i32 + r as i32) >> 1) as i16
            })
            .collect()
    } else {
        let mut v = 0i16;
        raw.iter()
            .map(|&x| {
                v = v.wrapping_add(x);
                v
            })
            .collect()
    }
}

fn sanitize_sample(s: &mut XmSample) {
    if s.loop_type == 3 {
        s.loop_type = 2;
    }
    s.volume = s.volume.min(64);
    s.relative_note = s.relative_note.clamp(-48, 71);
    if s.data.len() < s.length {
        s.length = s.data.len();
    }
    if s.loop_len == 0 || s.loop_start + s.loop_len > s.length {
        s.loop_start = 0;
        s.loop_len = 0;
        s.loop_type = 0;
    }
}

fn sanitize_instrument(ins: &mut XmInstrument) {
    ins.vib_depth = ins.vib_depth.min(0x0F);
    ins.vib_rate = ins.vib_rate.min(0x3F);
    if ins.vib_type > 3 {
        ins.vib_type = 0;
    }
    for x in ins.note2sample.iter_mut() {
        if *x >= 16 {
            *x = 15;
        }
    }
    ins.vol_len = ins.vol_len.min(12);
    ins.pan_len = ins.pan_len.min(12);
    for v in [&mut ins.vol_loop_start, &mut ins.vol_loop_end, &mut ins.vol_sus, &mut ins.pan_loop_start, &mut ins.pan_loop_end, &mut ins.pan_sus] {
        *v = (*v).min(11);
    }
    for i in 0..12 {
        if ins.vol_env[i].0 as u16 > 32767 {
            ins.vol_env[i].0 = 32767;
        }
        if ins.pan_env[i].0 as u16 > 32767 {
            ins.pan_env[i].0 = 32767;
        }
        if ins.vol_env[i].1 as u16 > 64 {
            ins.vol_env[i].1 = 64;
        }
        if ins.pan_env[i].1 as u16 > 63 {
            ins.pan_env[i].1 = 63;
        }
    }
}
