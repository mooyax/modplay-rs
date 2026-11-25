//! Minimal safe wrapper around the libopenmpt C API (only what the player needs).
//! Compiled only when build.rs found libopenmpt (cfg `has_openmpt`).

use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[repr(C)]
struct OpenmptModule {
    _private: [u8; 0],
}

#[repr(C)]
struct OpenmptModuleExt {
    _private: [u8; 0],
}

type ExtP = *mut OpenmptModuleExt;

/// openmpt_module_ext_interface_interactive (libopenmpt_ext.h), in declaration order
#[repr(C)]
#[derive(Default)]
struct InteractiveInterface {
    set_current_speed: Option<unsafe extern "C" fn(ExtP, i32) -> c_int>,
    set_current_tempo: Option<unsafe extern "C" fn(ExtP, i32) -> c_int>,
    set_tempo_factor: Option<unsafe extern "C" fn(ExtP, f64) -> c_int>,
    get_tempo_factor: Option<unsafe extern "C" fn(ExtP) -> f64>,
    set_pitch_factor: Option<unsafe extern "C" fn(ExtP, f64) -> c_int>,
    get_pitch_factor: Option<unsafe extern "C" fn(ExtP) -> f64>,
    set_global_volume: Option<unsafe extern "C" fn(ExtP, f64) -> c_int>,
    get_global_volume: Option<unsafe extern "C" fn(ExtP) -> f64>,
    set_channel_volume: Option<unsafe extern "C" fn(ExtP, i32, f64) -> c_int>,
    get_channel_volume: Option<unsafe extern "C" fn(ExtP, i32) -> f64>,
    set_channel_mute_status: Option<unsafe extern "C" fn(ExtP, i32, c_int) -> c_int>,
    get_channel_mute_status: Option<unsafe extern "C" fn(ExtP, i32) -> c_int>,
    set_instrument_mute_status: Option<unsafe extern "C" fn(ExtP, i32, c_int) -> c_int>,
    get_instrument_mute_status: Option<unsafe extern "C" fn(ExtP, i32) -> c_int>,
    play_note: Option<unsafe extern "C" fn(ExtP, i32, i32, f64, f64) -> i32>,
    stop_note: Option<unsafe extern "C" fn(ExtP, i32) -> c_int>,
}

pub const CMD_NOTE: c_int = 0;
pub const CMD_INSTRUMENT: c_int = 1;
pub const CMD_VOLUMEEFFECT: c_int = 2;
pub const CMD_EFFECT: c_int = 3;
pub const CMD_VOLUME: c_int = 4;
pub const CMD_PARAMETER: c_int = 5;

pub const RENDER_STEREOSEPARATION_PERCENT: c_int = 2;
pub const RENDER_INTERPOLATIONFILTER_LENGTH: c_int = 3;

type LogFunc = Option<unsafe extern "C" fn(*const c_char, *mut c_void)>;
type ErrFunc = Option<unsafe extern "C" fn(c_int, *mut c_void) -> c_int>;

#[cfg_attr(windows, link(name = "libopenmpt"))]
#[cfg_attr(not(windows), link(name = "openmpt"))]
extern "C" {
    fn openmpt_get_library_version() -> u32;
    fn openmpt_free_string(s: *const c_char);
    #[allow(dead_code)]
    fn openmpt_module_create_from_memory2(
        filedata: *const c_void,
        filesize: usize,
        logfunc: LogFunc,
        loguser: *mut c_void,
        errfunc: ErrFunc,
        erruser: *mut c_void,
        error: *mut c_int,
        error_message: *mut *const c_char,
        ctls: *const c_void,
    ) -> *mut OpenmptModule;
    fn openmpt_module_destroy(m: *mut OpenmptModule);
    fn openmpt_module_ext_create_from_memory(
        filedata: *const c_void,
        filesize: usize,
        logfunc: LogFunc,
        loguser: *mut c_void,
        errfunc: ErrFunc,
        erruser: *mut c_void,
        error: *mut c_int,
        error_message: *mut *const c_char,
        ctls: *const c_void,
    ) -> ExtP;
    fn openmpt_module_ext_destroy(m: ExtP);
    fn openmpt_module_ext_get_module(m: ExtP) -> *mut OpenmptModule;
    fn openmpt_module_ext_get_interface(m: ExtP, id: *const c_char, iface: *mut c_void, size: usize) -> c_int;
    fn openmpt_module_set_repeat_count(m: *mut OpenmptModule, count: i32) -> c_int;
    fn openmpt_module_set_render_param(m: *mut OpenmptModule, param: c_int, value: i32) -> c_int;
    fn openmpt_module_read_interleaved_stereo(m: *mut OpenmptModule, rate: i32, count: usize, buf: *mut i16) -> usize;
    fn openmpt_module_get_metadata(m: *mut OpenmptModule, key: *const c_char) -> *const c_char;
    fn openmpt_module_get_position_seconds(m: *mut OpenmptModule) -> f64;
    fn openmpt_module_set_position_order_row(m: *mut OpenmptModule, order: i32, row: i32) -> f64;
    fn openmpt_module_get_current_speed(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_current_tempo(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_current_order(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_current_pattern(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_current_row(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_current_channel_vu_mono(m: *mut OpenmptModule, channel: i32) -> f32;
    fn openmpt_module_get_num_channels(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_num_orders(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_num_patterns(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_num_instruments(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_num_samples(m: *mut OpenmptModule) -> i32;
    fn openmpt_module_get_instrument_name(m: *mut OpenmptModule, index: i32) -> *const c_char;
    fn openmpt_module_get_sample_name(m: *mut OpenmptModule, index: i32) -> *const c_char;
    fn openmpt_module_get_order_pattern(m: *mut OpenmptModule, order: i32) -> i32;
    fn openmpt_module_get_pattern_num_rows(m: *mut OpenmptModule, pattern: i32) -> i32;
    fn openmpt_module_get_pattern_row_channel_command(
        m: *mut OpenmptModule,
        pattern: i32,
        row: i32,
        channel: i32,
        command: c_int,
    ) -> u8;
    fn openmpt_module_format_pattern_row_channel_command(
        m: *mut OpenmptModule,
        pattern: i32,
        row: i32,
        channel: i32,
        command: c_int,
    ) -> *const c_char;
}

pub fn library_version() -> u32 {
    unsafe { openmpt_get_library_version() }
}

unsafe fn take_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    openmpt_free_string(p);
    s
}

pub struct OpenMptModule {
    m: *mut OpenmptModule,
    ext: ExtP,
    interactive: InteractiveInterface,
}

// libopenmpt modules may be used from any thread (not concurrently); we guard with a Mutex.
unsafe impl Send for OpenMptModule {}

impl Drop for OpenMptModule {
    fn drop(&mut self) {
        unsafe {
            if self.ext.is_null() {
                openmpt_module_destroy(self.m)
            } else {
                openmpt_module_ext_destroy(self.ext)
            }
        }
    }
}

impl OpenMptModule {
    /// Loads a module (with the "interactive" extension when available, for channel muting).
    pub fn load(data: &[u8]) -> Result<Self, String> {
        let mut err: c_int = 0;
        let mut msg: *const c_char = std::ptr::null();
        let ext = unsafe {
            openmpt_module_ext_create_from_memory(
                data.as_ptr() as *const c_void,
                data.len(),
                None,
                std::ptr::null_mut(),
                None,
                std::ptr::null_mut(),
                &mut err,
                &mut msg,
                std::ptr::null(),
            )
        };
        if ext.is_null() {
            let s = unsafe { take_string(msg) };
            return Err(if s.is_empty() { format!("libopenmpt error {err}") } else { s });
        }
        unsafe {
            if !msg.is_null() {
                openmpt_free_string(msg);
            }
        }
        let m = unsafe { openmpt_module_ext_get_module(ext) };
        let mut interactive = InteractiveInterface::default();
        let id = CString::new("interactive").unwrap();
        let ok = unsafe {
            openmpt_module_ext_get_interface(
                ext,
                id.as_ptr(),
                &mut interactive as *mut InteractiveInterface as *mut c_void,
                std::mem::size_of::<InteractiveInterface>(),
            )
        };
        if ok == 0 {
            interactive = InteractiveInterface::default();
        }
        Ok(OpenMptModule { m, ext, interactive })
    }

    /// Mutes / unmutes a pattern channel. Returns false if not supported.
    pub fn set_channel_mute(&mut self, ch: i32, mute: bool) -> bool {
        match self.interactive.set_channel_mute_status {
            Some(f) => unsafe { f(self.ext, ch, mute as c_int) != 0 },
            None => false,
        }
    }

    pub fn set_repeat_count(&mut self, n: i32) {
        unsafe { openmpt_module_set_repeat_count(self.m, n) };
    }
    pub fn set_stereo_separation(&mut self, percent: i32) {
        unsafe { openmpt_module_set_render_param(self.m, RENDER_STEREOSEPARATION_PERCENT, percent) };
    }
    /// 1 = nearest, 2 = linear, 4 = cubic, 8 = sinc
    pub fn set_interpolation(&mut self, taps: i32) {
        unsafe { openmpt_module_set_render_param(self.m, RENDER_INTERPOLATIONFILTER_LENGTH, taps) };
    }
    pub fn read_stereo(&mut self, rate: u32, buf: &mut [i16]) -> usize {
        unsafe { openmpt_module_read_interleaved_stereo(self.m, rate as i32, buf.len() / 2, buf.as_mut_ptr()) }
    }
    pub fn metadata(&self, key: &str) -> String {
        let k = CString::new(key).unwrap();
        unsafe { take_string(openmpt_module_get_metadata(self.m, k.as_ptr())) }
    }
    pub fn position_seconds(&self) -> f64 {
        unsafe { openmpt_module_get_position_seconds(self.m) }
    }
    pub fn set_position_order_row(&mut self, order: i32, row: i32) {
        unsafe { openmpt_module_set_position_order_row(self.m, order, row) };
    }
    pub fn speed(&self) -> i32 {
        unsafe { openmpt_module_get_current_speed(self.m) }
    }
    pub fn tempo(&self) -> i32 {
        unsafe { openmpt_module_get_current_tempo(self.m) }
    }
    pub fn order(&self) -> i32 {
        unsafe { openmpt_module_get_current_order(self.m) }
    }
    pub fn pattern(&self) -> i32 {
        unsafe { openmpt_module_get_current_pattern(self.m) }
    }
    pub fn row(&self) -> i32 {
        unsafe { openmpt_module_get_current_row(self.m) }
    }
    pub fn channel_vu(&self, ch: i32) -> f32 {
        unsafe { openmpt_module_get_current_channel_vu_mono(self.m, ch) }
    }
    pub fn num_channels(&self) -> i32 {
        unsafe { openmpt_module_get_num_channels(self.m) }
    }
    pub fn num_orders(&self) -> i32 {
        unsafe { openmpt_module_get_num_orders(self.m) }
    }
    pub fn num_patterns(&self) -> i32 {
        unsafe { openmpt_module_get_num_patterns(self.m) }
    }
    pub fn num_instruments(&self) -> i32 {
        unsafe { openmpt_module_get_num_instruments(self.m) }
    }
    pub fn num_samples(&self) -> i32 {
        unsafe { openmpt_module_get_num_samples(self.m) }
    }
    pub fn instrument_name(&self, i: i32) -> String {
        unsafe { take_string(openmpt_module_get_instrument_name(self.m, i)) }
    }
    pub fn sample_name(&self, i: i32) -> String {
        unsafe { take_string(openmpt_module_get_sample_name(self.m, i)) }
    }
    pub fn order_pattern(&self, order: i32) -> i32 {
        unsafe { openmpt_module_get_order_pattern(self.m, order) }
    }
    pub fn pattern_num_rows(&self, pattern: i32) -> i32 {
        unsafe { openmpt_module_get_pattern_num_rows(self.m, pattern) }
    }
    pub fn command(&self, pattern: i32, row: i32, ch: i32, cmd: c_int) -> u8 {
        unsafe { openmpt_module_get_pattern_row_channel_command(self.m, pattern, row, ch, cmd) }
    }
    pub fn format_command(&self, pattern: i32, row: i32, ch: i32, cmd: c_int) -> String {
        unsafe { take_string(openmpt_module_format_pattern_row_channel_command(self.m, pattern, row, ch, cmd)) }
    }
}
