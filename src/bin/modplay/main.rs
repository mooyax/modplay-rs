//! modplay - a module player with a tracker-style UI (egui), playing everything
//! with the Rust engines: MOD with the bit-exact ProTracker engine (pt2-clone port),
//! IT and S3M with the Rust IT engine, XM with the Rust XM engine (ft2-clone port).

mod modern;
mod png;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use modplay::player::{load_with, Player};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const USAGE: &str = "\
usage: modplay [file] [options]

formats: MOD, IT, S3M, XM

options:
  --stereo <0-100>        stereo separation in percent (default 20)
  --screenshot <out.png>  render the window to a PNG and quit
  --at <seconds>          playback position for --screenshot (default 10)
  --size <W>x<H>          window size (default 1280x760)

keys:
  Space          play / pause
  PgUp / PgDn    previous / next song position (also + / -)
  O              open file (or drop a file on the window)
  Esc            quit
";

type Shared = Arc<Mutex<Option<Box<dyn Player>>>>;

struct Args {
    file: Option<PathBuf>,
    stereo: u8,
    screenshot: Option<PathBuf>,
    at: f64,
    size: (f32, f32),
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { file: None, stereo: 20, screenshot: None, at: 10.0, size: (1280.0, 760.0) };
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match s.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--stereo" => a.stereo = val("--stereo")?.parse::<u8>().map_err(|_| "bad --stereo")?.min(100),
            "--screenshot" => a.screenshot = Some(PathBuf::from(val("--screenshot")?)),
            "--at" => a.at = val("--at")?.parse().map_err(|_| "bad --at")?,
            "--size" => {
                let v = val("--size")?;
                let (w, h) = v.split_once('x').ok_or("bad --size (use WxH)")?;
                a.size = (w.parse().map_err(|_| "bad --size")?, h.parse().map_err(|_| "bad --size")?);
            }
            x if x.starts_with("--") => return Err(format!("unknown option {x}\n\n{USAGE}")),
            _ => a.file = Some(PathBuf::from(s)),
        }
    }
    Ok(a)
}

fn load_file(path: &Path, rate: u32, stereo: u8) -> Result<Box<dyn Player>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_with(&data, rate, stereo, false)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = run(args) {
        eprintln!("error: {e}");
        #[cfg(windows)]
        {
            let _ = rfd::MessageDialog::new().set_title("modplay").set_description(&e).show();
        }
        std::process::exit(1);
    }
}

fn start_audio(shared: Shared) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let err_fn = |e| eprintln!("audio error: {e}");
    let mut tmp: Vec<i16> = Vec::new();

    // render stereo i16 and spread it over the device's channel layout
    let mut fill = move |frames: usize, mut write: Box<dyn FnMut(usize, i16, i16) + '_>| {
        tmp.resize(frames * 2, 0);
        match shared.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(p) => p.render(&mut tmp),
                None => tmp.fill(0),
            },
            Err(_) => tmp.fill(0),
        }
        for i in 0..frames {
            write(i, tmp[i * 2], tmp[i * 2 + 1]);
        }
    };

    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                fill(
                    frames,
                    Box::new(|i, l, r| {
                        let f = &mut data[i * channels..(i + 1) * channels];
                        f[0] = l as f32 / 32768.0;
                        if channels > 1 {
                            f[1] = r as f32 / 32768.0;
                            for x in f[2..].iter_mut() {
                                *x = 0.0;
                            }
                        }
                    }),
                );
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                let frames = data.len() / channels;
                fill(
                    frames,
                    Box::new(|i, l, r| {
                        let f = &mut data[i * channels..(i + 1) * channels];
                        f[0] = l;
                        if channels > 1 {
                            f[1] = r;
                            for x in f[2..].iter_mut() {
                                *x = 0;
                            }
                        }
                    }),
                );
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |data: &mut [u16], _| {
                let frames = data.len() / channels;
                fill(
                    frames,
                    Box::new(|i, l, r| {
                        let f = &mut data[i * channels..(i + 1) * channels];
                        f[0] = (l as i32 + 32768) as u16;
                        if channels > 1 {
                            f[1] = (r as i32 + 32768) as u16;
                            for x in f[2..].iter_mut() {
                                *x = 32768;
                            }
                        }
                    }),
                );
            },
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported audio sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

fn pick_file() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_title("Load module")
            .add_filter("Modules", &["mod", "it", "xm", "s3m"])
            .add_filter("All files", &["*"])
            .pick_file()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// A minimal empty M.K. module so the window can be drawn before a file is loaded.
fn empty_mod() -> Vec<u8> {
    let mut d = vec![0u8; 1084 + 1024];
    for i in 0..31 {
        d[20 + i * 30 + 29] = 1; // replen = 1
    }
    d[950] = 1;
    d[951] = 127;
    d[1080..1084].copy_from_slice(b"M.K.");
    d
}

#[derive(Clone, Debug)]
pub enum Action {
    PlayStop,
    Play,
    Stop,
    Order(i32),
    JumpOrder(i32),
    MuteAbs(usize),
    Solo(usize),
    Unsolo,
    Open,
    OpenPath(PathBuf),
}

/// Screenshot mode: play silently to the requested time, grab the window, quit.
struct Shot {
    out: PathBuf,
    frames_left: u32,
    requested: bool,
    done: Arc<AtomicBool>,
}

struct App {
    shared: Shared,
    rate: u32,
    stereo: u8,
    file: Option<PathBuf>,
    file_name: String,
    modern: modern::Modern,
    status: String,
    status_until: f64,
    empty: Box<dyn Player>,
    shot: Option<Shot>,
    title: String,
    shown_title: String,
}

fn run(args: Args) -> Result<(), String> {
    let shared: Shared = Arc::new(Mutex::new(None));
    // screenshot mode has no audio device: the UI advances the player itself
    let (stream, rate) = if args.screenshot.is_some() {
        (None, 48000)
    } else {
        let (s, r) = start_audio(shared.clone())?;
        (Some(s), r)
    };

    let mut app = App {
        shared,
        rate,
        stereo: args.stereo,
        file: None,
        file_name: String::new(),
        modern: modern::Modern::new(),
        status: "Ready — open a module (O) or drop one on the window".into(),
        status_until: f64::INFINITY,
        empty: Box::new(modplay::player::Pt2Player::load(&empty_mod(), rate, 20)?),
        shot: None,
        title: "modplay".into(),
        shown_title: String::new(),
    };

    let mut file = args.file.clone();
    if file.is_none() && args.screenshot.is_none() {
        file = pick_file();
    }
    if let Some(f) = &file {
        app.open(f);
    }
    if let Some(out) = &args.screenshot {
        // fast-forward silently, the last second is played frame by frame
        if let Ok(mut g) = app.shared.lock() {
            if let Some(p) = g.as_mut() {
                let mut buf = vec![0i16; (rate as usize / 60) * 2];
                let pre = ((args.at - 1.0).max(0.0) * 60.0) as usize;
                for _ in 0..pre {
                    p.render(&mut buf);
                }
            }
        }
        app.shot = Some(Shot { out: out.clone(), frames_left: 60, requested: false, done: Arc::new(AtomicBool::new(false)) });
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([args.size.0, args.size.1]).with_min_inner_size([640.0, 400.0]).with_title(app.title.clone()).with_drag_and_drop(true),
        ..Default::default()
    };
    let res = eframe::run_native(
        "modplay",
        options,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    );
    drop(stream);
    res.map_err(|e| e.to_string())
}

fn setup_style(ctx: &egui::Context) {
    // always dark, whatever the system theme
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals(egui::Visuals::dark());
    let mut style = (*ctx.global_style()).clone();
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_global_style(style);
}

impl App {
    fn set_status(&mut self, s: impl Into<String>, now: f64) {
        self.status = s.into();
        self.status_until = now + 3.0;
    }

    fn open(&mut self, path: &Path) {
        match load_file(path, self.rate, self.stereo) {
            Ok(mut p) => {
                p.play_from(0);
                self.file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                self.title = format!("modplay - {} [{}, {} ch]", self.file_name, p.format_name(), p.num_channels());
                self.modern.reset();
                self.file = Some(path.to_path_buf());
                self.status = format!("Loaded {}", self.file_name);
                self.status_until = f64::INFINITY;
                if let Ok(mut g) = self.shared.lock() {
                    *g = Some(p);
                }
            }
            Err(e) => {
                eprintln!("{e}");
                self.status = format!("Could not load {}: {e}", path.display());
                self.status_until = f64::INFINITY;
            }
        }
    }

    fn apply(&mut self, a: Action, now: f64) {
        match a {
            Action::Open => {
                if let Some(f) = pick_file() {
                    self.open(&f);
                }
                return;
            }
            Action::OpenPath(f) => {
                self.open(&f);
                return;
            }
            _ => {}
        }
        let Ok(mut guard) = self.shared.lock() else { return };
        let Some(p) = guard.as_mut() else { return };
        let st = p.status();
        let mut msg: Option<String> = None;
        match a {
            Action::PlayStop => {
                if st.playing {
                    p.stop();
                } else {
                    p.play_from(st.order);
                }
            }
            Action::Play => p.play_from(st.order),
            Action::Stop => {
                p.stop();
                // back to the start of the current position
                p.play_from(st.order);
                p.stop();
            }
            Action::Order(d) => {
                let n = p.num_orders().max(1);
                let o = (st.order + d).rem_euclid(n);
                p.play_from(o);
                if !st.playing {
                    p.stop();
                }
            }
            Action::JumpOrder(o) => {
                p.play_from(o);
                if !st.playing {
                    p.stop();
                }
            }
            Action::MuteAbs(ch) => {
                if ch < p.num_channels() {
                    let m = p.is_muted(ch);
                    p.set_muted(ch, !m);
                    if p.is_muted(ch) == m {
                        msg = Some("Mute is not available for this player".into());
                    }
                }
            }
            Action::Solo(ch) => {
                for c in 0..p.num_channels() {
                    p.set_muted(c, c != ch);
                }
                msg = Some(format!("Solo: channel {}", ch + 1));
            }
            Action::Unsolo => {
                for c in 0..p.num_channels() {
                    p.set_muted(c, false);
                }
            }
            _ => {}
        }
        drop(guard);
        if let Some(m) = msg {
            self.set_status(m, now);
        }
    }

    fn keyboard(&self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        // take the player keys before any widget sees them (a focused button
        // would otherwise also react to Space, and Tab would move the focus)
        ctx.input_mut(|i| {
            let mut k = |key| i.consume_key(egui::Modifiers::NONE, key);
            if k(egui::Key::Space) {
                out.push(Action::PlayStop);
            }
            if k(egui::Key::PageDown) || k(egui::Key::Plus) || k(egui::Key::Equals) {
                out.push(Action::Order(1));
            }
            if k(egui::Key::PageUp) || k(egui::Key::Minus) {
                out.push(Action::Order(-1));
            }
            if k(egui::Key::O) {
                out.push(Action::Open);
            }
        });
        ctx.input(|i| {
            for f in &i.raw.dropped_files {
                let p = f.path();
                if !p.as_os_str().is_empty() {
                    out.push(Action::OpenPath(p.to_path_buf()));
                }
            }
        });
        out
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.shown_title != self.title {
            self.shown_title = self.title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title.clone()));
        }
        if now > self.status_until {
            self.status = if self.file.is_some() { format!("{}", self.file_name) } else { String::new() };
            self.status_until = f64::INFINITY;
        }

        // screenshot mode: advance the song by one video frame, then grab the window
        if let Some(shot) = &mut self.shot {
            if shot.done.load(Ordering::SeqCst) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if shot.frames_left > 0 {
                shot.frames_left -= 1;
                if let Ok(mut g) = self.shared.lock() {
                    if let Some(p) = g.as_mut() {
                        let mut buf = vec![0i16; (self.rate as usize / 60) * 2];
                        p.render(&mut buf);
                    }
                }
            } else if !shot.requested {
                shot.requested = true;
                let out = shot.out.clone();
                let done = shot.done.clone();
                ctx.request_screenshot(move |img| {
                    let px: Vec<u32> = img.pixels.iter().map(|c| ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32).collect();
                    match std::fs::write(&out, png::encode(&px, img.size[0], img.size[1], 1)) {
                        Ok(()) => eprintln!("wrote {}", out.display()),
                        Err(e) => eprintln!("error: {e}"),
                    }
                    done.store(true, Ordering::SeqCst);
                });
            }
        }

        let mut actions = self.keyboard(&ctx);
        {
            let guard = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let p: &dyn Player = match guard.as_ref() {
                Some(p) => p.as_ref(),
                None => self.empty.as_ref(),
            };
            let info = modern::Info {
                file_name: if self.file_name.is_empty() { "no file" } else { &self.file_name },
                status: &self.status,
                can_open_dialog: cfg!(windows),
            };
            actions.extend(self.modern.show(ui, p, &info));
        }
        for a in actions {
            self.apply(a, now);
        }
        ctx.request_repaint();
    }
}
