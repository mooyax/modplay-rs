//! ahmod: a libopenmpt-compatible C API (the subset a game needs) on top of the
//! modplay-rs Rust engines: MOD (ProTracker 2 replayer), IT / S3M (Rust IT engine) and
//! XM (FastTracker II replayer port).
//!
//! The functions have the same names and signatures as in libopenmpt, so native
//! code written against libopenmpt links against this library unchanged.
//! Output levels match libopenmpt's default render settings.
//!
//! Real-time use: the read functions never block. Buffers grow only when a larger
//! block is requested than before (normally just on the first call).

#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]

use modplay::player::Player;
use std::ffi::{c_char, c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const OPENMPT_ERROR_OK: c_int = 0;
pub const OPENMPT_ERROR_BASE: c_int = 256;
pub const OPENMPT_ERROR_INVALID_ARGUMENT: c_int = OPENMPT_ERROR_BASE + 44;
pub const OPENMPT_ERROR_GENERAL: c_int = OPENMPT_ERROR_BASE + 101;

pub type openmpt_log_func = Option<unsafe extern "C" fn(message: *const c_char, user: *mut c_void)>;
pub type openmpt_error_func = Option<unsafe extern "C" fn(error: c_int, user: *mut c_void) -> c_int>;

#[repr(C)]
pub struct openmpt_module_initial_ctl {
    pub ctl: *const c_char,
    pub value: *const c_char,
}

/// Opaque module handle (`openmpt_module *` in C)
pub struct openmpt_module {
    data: Vec<u8>,
    player: Option<Box<dyn Player>>,
    rate: i32,
    repeat_count: i32,
    buf_f: Vec<f32>,
    buf_i: Vec<i16>,
    /// set after a panic inside an engine: the module only outputs silence from then on
    broken: bool,
}

const DEFAULT_RATE: i32 = 48000;
/// libopenmpt's default stereo separation (100 %) for MOD files
#[cfg_attr(not(feature = "protracker"), allow(dead_code))]
const STEREO_SEPARATION: u8 = 100;

fn load_player(data: &[u8], rate: i32) -> Option<Box<dyn Player>> {
    let mut p = make_player(data, rate as u32).ok()?;
    p.play_from(0);
    Some(p)
}

impl openmpt_module {
    /// Make sure the engine runs at `rate` (rebuilt at the current position if it changed).
    fn ensure_rate(&mut self, rate: i32) -> bool {
        if rate <= 0 || self.broken {
            return false;
        }
        if self.rate == rate && self.player.is_some() {
            return true;
        }
        let order = self.player.as_ref().map(|p| p.status().order).unwrap_or(0);
        match load_player(&self.data, rate) {
            Some(mut p) => {
                if order > 0 {
                    p.play_from(order);
                }
                self.player = Some(p);
                self.rate = rate;
                true
            }
            None => false,
        }
    }

    /// Renders `frames` stereo frames into `self.buf_f` (interleaved).
    fn render(&mut self, rate: i32, frames: usize) -> bool {
        if !self.ensure_rate(rate) {
            return false;
        }
        let n = frames * 2;
        if self.buf_f.len() < n {
            self.buf_f.resize(n, 0.0);
        }
        let player = self.player.as_mut().unwrap();
        let out = &mut self.buf_f[..n];
        let ok = if player.is_protracker() {
            // the ProTracker engine mixes in 16 bits (Paula emulation)
            if self.buf_i.len() < n {
                self.buf_i.resize(n, 0);
            }
            let tmp = &mut self.buf_i[..n];
            catch_unwind(AssertUnwindSafe(|| player.render(tmp))).is_ok() && {
                for (o, &s) in out.iter_mut().zip(tmp.iter()) {
                    *o = s as f32 / 32768.0;
                }
                true
            }
        } else {
            catch_unwind(AssertUnwindSafe(|| player.render_f32(out))).is_ok()
        };
        if !ok {
            self.broken = true;
            self.buf_f[..n].fill(0.0);
        }
        true
    }
}

unsafe fn set_error(error: *mut c_int, error_message: *mut *const c_char, code: c_int) {
    if !error.is_null() {
        *error = code;
    }
    if !error_message.is_null() {
        *error_message = std::ptr::null();
    }
}

/// Same as libopenmpt: loads a module from memory (the data is copied).
/// MOD, IT, S3M and XM are supported. `logfunc`, `errfunc` and `ctls` are accepted
/// and ignored. Returns NULL if the data is not a supported module.
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_create_from_memory2(
    filedata: *const c_void,
    filesize: usize,
    _logfunc: openmpt_log_func,
    _loguser: *mut c_void,
    _errfunc: openmpt_error_func,
    _erruser: *mut c_void,
    error: *mut c_int,
    error_message: *mut *const c_char,
    _ctls: *const openmpt_module_initial_ctl,
) -> *mut openmpt_module {
    if filedata.is_null() || filesize == 0 {
        set_error(error, error_message, OPENMPT_ERROR_INVALID_ARGUMENT);
        return std::ptr::null_mut();
    }
    let data = std::slice::from_raw_parts(filedata as *const u8, filesize).to_vec();
    let res = catch_unwind(AssertUnwindSafe(|| load_player(&data, DEFAULT_RATE)));
    match res {
        Ok(Some(p)) => {
            set_error(error, error_message, OPENMPT_ERROR_OK);
            Box::into_raw(Box::new(openmpt_module { data, player: Some(p), rate: DEFAULT_RATE, repeat_count: 0, buf_f: Vec::new(), buf_i: Vec::new(), broken: false }))
        }
        _ => {
            set_error(error, error_message, OPENMPT_ERROR_GENERAL);
            std::ptr::null_mut()
        }
    }
}

/// Same as libopenmpt's older entry point.
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_create_from_memory(
    filedata: *const c_void,
    filesize: usize,
    logfunc: openmpt_log_func,
    loguser: *mut c_void,
    ctls: *const openmpt_module_initial_ctl,
) -> *mut openmpt_module {
    openmpt_module_create_from_memory2(filedata, filesize, logfunc, loguser, None, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), ctls)
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_destroy(module: *mut openmpt_module) {
    if !module.is_null() {
        drop(Box::from_raw(module));
    }
}

/// Stored for `openmpt_module_get_repeat_count`. The Rust engines always continue
/// at the song's restart position when the song ends (libopenmpt's -1 behaviour).
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_set_repeat_count(module: *mut openmpt_module, repeat_count: i32) -> c_int {
    match module.as_mut() {
        Some(m) => {
            m.repeat_count = repeat_count;
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_get_repeat_count(module: *mut openmpt_module) -> i32 {
    module.as_ref().map(|m| m.repeat_count).unwrap_or(0)
}

/// Renders `count` frames into separate left / right buffers. Returns the number of
/// frames written (always `count` for a valid module).
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_read_float_stereo(module: *mut openmpt_module, samplerate: i32, count: usize, left: *mut f32, right: *mut f32) -> usize {
    let Some(m) = module.as_mut() else { return 0 };
    if left.is_null() || right.is_null() || count == 0 {
        return 0;
    }
    if !m.render(samplerate, count) {
        return 0;
    }
    let l = std::slice::from_raw_parts_mut(left, count);
    let r = std::slice::from_raw_parts_mut(right, count);
    for i in 0..count {
        l[i] = m.buf_f[i * 2];
        r[i] = m.buf_f[i * 2 + 1];
    }
    count
}

/// Renders `count` frames as interleaved stereo (L R L R ...).
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_read_interleaved_float_stereo(module: *mut openmpt_module, samplerate: i32, count: usize, interleaved_stereo: *mut f32) -> usize {
    let Some(m) = module.as_mut() else { return 0 };
    if interleaved_stereo.is_null() || count == 0 {
        return 0;
    }
    if !m.render(samplerate, count) {
        return 0;
    }
    std::slice::from_raw_parts_mut(interleaved_stereo, count * 2).copy_from_slice(&m.buf_f[..count * 2]);
    count
}

/// 16-bit variants, as in libopenmpt
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_read_stereo(module: *mut openmpt_module, samplerate: i32, count: usize, left: *mut i16, right: *mut i16) -> usize {
    let Some(m) = module.as_mut() else { return 0 };
    if left.is_null() || right.is_null() || count == 0 || !m.render(samplerate, count) {
        return 0;
    }
    let l = std::slice::from_raw_parts_mut(left, count);
    let r = std::slice::from_raw_parts_mut(right, count);
    for i in 0..count {
        l[i] = to_i16(m.buf_f[i * 2]);
        r[i] = to_i16(m.buf_f[i * 2 + 1]);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_read_interleaved_stereo(module: *mut openmpt_module, samplerate: i32, count: usize, interleaved_stereo: *mut i16) -> usize {
    let Some(m) = module.as_mut() else { return 0 };
    if interleaved_stereo.is_null() || count == 0 || !m.render(samplerate, count) {
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(interleaved_stereo, count * 2);
    for (o, &x) in out.iter_mut().zip(&m.buf_f[..count * 2]) {
        *o = to_i16(x);
    }
    count
}

#[inline]
fn to_i16(x: f32) -> i16 {
    (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

/// Seconds played since loading (or since the last restart)
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_get_position_seconds(module: *mut openmpt_module) -> f64 {
    module.as_ref().and_then(|m| m.player.as_ref()).map(|p| p.status().seconds).unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_get_current_order(module: *mut openmpt_module) -> i32 {
    module.as_ref().and_then(|m| m.player.as_ref()).map(|p| p.status().order).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_get_current_row(module: *mut openmpt_module) -> i32 {
    module.as_ref().and_then(|m| m.player.as_ref()).map(|p| p.status().row).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn openmpt_module_get_num_channels(module: *mut openmpt_module) -> i32 {
    module.as_ref().and_then(|m| m.player.as_ref()).map(|p| p.num_channels() as i32).unwrap_or(0)
}

/// Jumps to song position `order`, row 0 (the row argument is accepted for
/// compatibility; playback restarts at the beginning of the pattern).
#[no_mangle]
pub unsafe extern "C" fn openmpt_module_set_position_order_row(module: *mut openmpt_module, order: i32, _row: i32) -> f64 {
    if let Some(p) = module.as_mut().and_then(|m| m.player.as_mut()) {
        p.play_from(order.max(0));
    }
    0.0
}

/// Mutes or unmutes one channel (an extension: libopenmpt does this through its
/// "interactive" interface).
#[no_mangle]
pub unsafe extern "C" fn ahmod_set_channel_mute(module: *mut openmpt_module, channel: i32, mute: c_int) -> c_int {
    match module.as_mut().and_then(|m| m.player.as_mut()) {
        Some(p) if channel >= 0 => {
            p.set_muted(channel as usize, mute != 0);
            1
        }
        _ => 0,
    }
}

/// Version in libopenmpt's format (0xMMmmpppp): reports 0.8.0
#[no_mangle]
pub extern "C" fn openmpt_get_library_version() -> u32 {
    0x0008_0000
}

/// IT / S3M / XM always; MOD only with the `protracker` feature (on by default), so a build
/// for IT / XM music alone can leave the ProTracker engine out
fn make_player(data: &[u8], rate: u32) -> Result<Box<dyn Player>, String> {
    #[cfg(feature = "protracker")]
    {
        modplay::player::load_with(data, rate, STEREO_SEPARATION, false)
    }
    #[cfg(not(feature = "protracker"))]
    {
        use modplay::player::{own_engine, ItEnginePlayer, XmEnginePlayer};
        if modplay::xm::format::detect(data) {
            XmEnginePlayer::load(data, rate).map(|p| Box::new(p) as Box<dyn Player>)
        } else if own_engine(data) {
            ItEnginePlayer::load(data, rate).map(|p| Box::new(p) as Box<dyn Player>)
        } else {
            Err("unsupported format".into())
        }
    }
}
