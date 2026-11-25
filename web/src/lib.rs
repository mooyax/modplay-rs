//! modplay in the browser: the same Rust engines and tracker-style UI as the desktop
//! player, compiled to WebAssembly. Audio goes through Web Audio (cpal); modules come from
//! drag & drop, the open button (a file input in index.html) or `?url=` (fetched by index.html).

#[path = "../../src/bin/modplay/modern.rs"]
mod modern;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use modplay::player::{load_with, Player};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;

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
}

type Shared = Arc<Mutex<Option<Box<dyn Player>>>>;

thread_local! {
    /// modules handed over from JavaScript (file input, ?url=), picked up by the app each frame
    static INCOMING: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// Called from index.html with the bytes of a module file.
#[wasm_bindgen]
pub fn load_module(name: String, data: Vec<u8>) {
    INCOMING.with(|q| q.borrow_mut().push((name, data)));
}

/// Starts the player on the canvas with the given id.
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), JsValue> {
    let document = web_sys::window().ok_or("no window")?.document().ok_or("no document")?;
    let canvas = document
        .get_element_by_id(&canvas_id)
        .ok_or("canvas not found")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| {
                cc.egui_ctx.set_visuals(egui::Visuals::dark());
                let mut style = (*cc.egui_ctx.global_style()).clone();
                style.spacing.button_padding = egui::vec2(8.0, 4.0);
                cc.egui_ctx.set_global_style(style);
                Ok(Box::new(App::new()))
            }),
        )
        .await
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

struct App {
    shared: Shared,
    /// created on the first click / key press: browsers only allow audio after a user gesture
    stream: Option<cpal::Stream>,
    rate: u32,
    file_name: String,
    modern: modern::Modern,
    status: String,
    empty: Box<dyn Player>,
}

impl App {
    fn new() -> Self {
        App {
            shared: Arc::new(Mutex::new(None)),
            stream: None,
            rate: 48000,
            file_name: String::new(),
            modern: modern::Modern::new(),
            status: "Drop a module (MOD / IT / S3M / XM) on the page, or click the folder button".into(),
            empty: Box::new(modplay::player::Pt2Player::load(&empty_mod(), 48000, 20).expect("empty module")),
        }
    }

    fn start_audio(&mut self) {
        if self.stream.is_some() {
            return;
        }
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            self.status = "No audio output available".into();
            return;
        };
        let Ok(cfg) = device.default_output_config() else { return };
        let rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let config = cpal::StreamConfig { channels: cfg.channels(), sample_rate: cfg.sample_rate(), buffer_size: cpal::BufferSize::Fixed(2048) };
        let shared = self.shared.clone();
        let mut tmp: Vec<f32> = Vec::new();
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                tmp.resize(frames * 2, 0.0);
                match shared.lock() {
                    Ok(mut g) => match g.as_mut() {
                        Some(p) => p.render_f32(&mut tmp),
                        None => tmp.fill(0.0),
                    },
                    Err(_) => tmp.fill(0.0),
                }
                for (i, f) in data.chunks_exact_mut(channels).enumerate() {
                    f[0] = tmp[i * 2];
                    if channels > 1 {
                        f[1] = tmp[i * 2 + 1];
                    }
                }
            },
            |e| web_sys::console::error_1(&format!("audio error: {e}").into()),
            None,
        );
        match stream {
            Ok(s) => {
                let _ = s.play();
                // the module may have been loaded at a guessed rate: reload it at the real one
                if rate != self.rate {
                    self.rate = rate;
                }
                self.stream = Some(s);
            }
            Err(e) => self.status = format!("Audio error: {e}"),
        }
    }

    fn open(&mut self, name: String, data: &[u8]) {
        match load_with(data, self.rate, 20, false) {
            Ok(mut p) => {
                p.play_from(0);
                self.status = format!("Loaded {name} [{}, {} ch]", p.format_name(), p.num_channels());
                self.file_name = name;
                self.modern.reset();
                if let Ok(mut g) = self.shared.lock() {
                    *g = Some(p);
                }
            }
            Err(e) => self.status = format!("Could not load {name}: {e}"),
        }
    }

    fn apply(&mut self, a: Action) {
        if let Action::Open = a {
            // the hidden <input type=file> in index.html
            if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("file")) {
                if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
                    el.click();
                }
            }
            return;
        }
        let Ok(mut guard) = self.shared.lock() else { return };
        let Some(p) = guard.as_mut() else { return };
        let st = p.status();
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
                p.play_from(st.order);
                p.stop();
            }
            Action::Order(d) => {
                let n = p.num_orders().max(1);
                p.play_from((st.order + d).rem_euclid(n));
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
                }
            }
            Action::Solo(ch) => {
                for c in 0..p.num_channels() {
                    p.set_muted(c, c != ch);
                }
            }
            Action::Unsolo => {
                for c in 0..p.num_channels() {
                    p.set_muted(c, false);
                }
            }
            Action::Open => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // audio may only start after a user gesture
        if self.stream.is_none() && ctx.input(|i| i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })) || !i.raw.dropped_files.is_empty()) {
            self.start_audio();
        }

        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let mut k = |key| i.consume_key(egui::Modifiers::NONE, key);
            if k(egui::Key::Space) {
                actions.push(Action::PlayStop);
            }
            if k(egui::Key::PageDown) || k(egui::Key::Plus) || k(egui::Key::Equals) {
                actions.push(Action::Order(1));
            }
            if k(egui::Key::PageUp) || k(egui::Key::Minus) {
                actions.push(Action::Order(-1));
            }
            if k(egui::Key::O) {
                actions.push(Action::Open);
            }
        });
        // dropped files are read asynchronously and arrive through INCOMING a moment later
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            wasm_bindgen_futures::spawn_local(async move {
                let name = f.path().file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                match f.bytes_async().await {
                    Ok(data) => load_module(name, data),
                    Err(e) => web_sys::console::error_1(&format!("could not read {name}: {e}").into()),
                }
            });
        }
        let incoming: Vec<(String, Vec<u8>)> = INCOMING.with(|q| q.borrow_mut().drain(..).collect());
        for (name, data) in incoming {
            self.open(name, &data);
        }

        let waiting = self.stream.is_none() && !self.file_name.is_empty();
        let status = if waiting { "Click anywhere (or press a key) to start the sound".to_string() } else { self.status.clone() };
        {
            let guard = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let p: &dyn Player = match guard.as_ref() {
                Some(p) => p.as_ref(),
                None => self.empty.as_ref(),
            };
            let info = modern::Info { file_name: if self.file_name.is_empty() { "no file" } else { &self.file_name }, status: &status, can_open_dialog: true };
            actions.extend(self.modern.show(ui, p, &info));
        }
        for a in actions {
            self.apply(a);
        }
        ctx.request_repaint();
    }
}
