//! The modern tracker-style UI: transport bar, per-channel scopes / VU / mute / solo,
//! a scrolling pattern view showing all channels, the order list and instruments.

use crate::Action;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use modplay::player::{CellView, Player, VuState, DOT};

// ---- colours ----
const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1c);
const BG_BEAT: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x25);
const BG_BAR: Color32 = Color32::from_rgb(0x22, 0x26, 0x2f);
const HEADER_BG: Color32 = Color32::from_rgb(0x1f, 0x23, 0x2b);
const GRID: Color32 = Color32::from_rgb(0x2b, 0x30, 0x3a);
const CUR_ROW: Color32 = Color32::from_rgb(0x2c, 0x45, 0x70);
const ACCENT: Color32 = Color32::from_rgb(0x5a, 0x9c, 0xff);
const TXT_NOTE: Color32 = Color32::from_rgb(0xe8, 0xea, 0xee);
const TXT_INS: Color32 = Color32::from_rgb(0x7f, 0xb2, 0xff);
const TXT_VOL: Color32 = Color32::from_rgb(0x7e, 0xd9, 0x9c);
const TXT_EFF: Color32 = Color32::from_rgb(0xf0, 0xa8, 0x60);
const TXT_DIM: Color32 = Color32::from_rgb(0x44, 0x4a, 0x56);
const TXT_ROWNUM: Color32 = Color32::from_rgb(0x70, 0x78, 0x86);
const SCOPE_LINE: Color32 = Color32::from_rgb(0x6f, 0xe3, 0xc8);
const MUTED: Color32 = Color32::from_rgb(0xe0, 0x55, 0x55);
const SOLO: Color32 = Color32::from_rgb(0xf2, 0xc9, 0x4c);

const FONT_SIZE: f32 = 13.0;
/// frames of level history shown for backends without waveforms
const HIST_LEN: usize = 90;
const HEADER_H: f32 = 74.0;

pub struct Modern {
    /// smoothed VU levels 0..1 per channel
    vu: Vec<f32>,
    vu_last_trigger: Vec<u32>,
    /// recent level per channel (one entry per frame), for backends without waveforms
    hist: Vec<std::collections::VecDeque<f32>>,
    solo: Option<usize>,
    /// last order shown, to keep the order list scrolled to it
    last_order: i32,
    /// the About window is open
    about: bool,
}

/// What the main window needs to know besides the player
pub struct Info<'a> {
    pub file_name: &'a str,
    pub status: &'a str,
    pub can_open_dialog: bool,
}

impl Modern {
    pub fn new() -> Self {
        Modern { vu: Vec::new(), vu_last_trigger: Vec::new(), hist: Vec::new(), solo: None, last_order: -1, about: false }
    }

    /// Forget per-song state (after loading a new file)
    pub fn reset(&mut self) {
        let about = self.about;
        *self = Modern::new();
        self.about = about;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, p: &dyn Player, info: &Info) -> Vec<Action> {
        let mut actions = Vec::new();
        self.update_vu(p);
        let st = p.status();

        // ---------------- transport bar ----------------
        egui::Panel::top("transport").frame(egui::Frame::new().fill(HEADER_BG).inner_margin(egui::Margin::symmetric(10, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (play_icon, play_tip) = if st.playing { (Icon::Pause, "Pause (Space)") } else { (Icon::Play, "Play (Space)") };
                if icon_button(ui, play_icon, true, st.playing).on_hover_text(play_tip).clicked() {
                    actions.push(Action::PlayStop);
                }
                if icon_button(ui, Icon::Stop, true, false).on_hover_text("Stop").clicked() {
                    actions.push(Action::Stop);
                }
                if icon_button(ui, Icon::Prev, true, false).on_hover_text("Previous position (PgUp)").clicked() {
                    actions.push(Action::Order(-1));
                }
                if icon_button(ui, Icon::Next, true, false).on_hover_text("Next position (PgDn)").clicked() {
                    actions.push(Action::Order(1));
                }
                ui.add_space(4.0);
                let tip = if info.can_open_dialog { "Open file (O) — or drop a file on the window" } else { "Drop a file on the window to open it" };
                if icon_button(ui, Icon::Open, info.can_open_dialog, false).on_hover_text(tip).clicked() {
                    actions.push(Action::Open);
                }
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let title = p.title();
                    let title = if title.trim().is_empty() { info.file_name.to_string() } else { title.trim().to_string() };
                    ui.label(egui::RichText::new(title).size(15.0).strong().color(TXT_NOTE));
                    ui.label(egui::RichText::new(format!("{}  ·  {}  ·  {} ch", info.file_name, p.format_name(), p.num_channels())).size(11.5).color(TXT_ROWNUM));
                });
                ui.add_space(14.0);
                let secs = st.seconds.max(0.0) as u64;
                stat(ui, "POSITION", &format!("{:02}/{:02}", st.order.max(0), (p.num_orders() - 1).max(0)));
                stat(ui, "PATTERN", &format!("{:02}", st.pattern.max(0)));
                stat(ui, "ROW", &format!("{:02}/{:02}", st.row.max(0), st.num_rows.max(1) - 1));
                stat(ui, "SPEED", &st.speed.to_string());
                stat(ui, "TEMPO", &st.bpm.to_string());
                stat(ui, "TIME", &format!("{:02}:{:02}", secs / 60, secs % 60));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").on_hover_text("Credits and licenses").clicked() {
                        self.about = !self.about;
                    }
                });
            });
        });

        // ---------------- status bar ----------------
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(HEADER_BG).inner_margin(egui::Margin::symmetric(10, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(info.status).size(12.0).color(TXT_NOTE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Space play/pause · PgUp/PgDn position · click M/S to mute/solo · drop a file to open")
                            .size(11.5)
                            .color(TXT_ROWNUM),
                    );
                });
            });
        });

        // ---------------- side panel: orders + instruments ----------------
        egui::Panel::right("side").resizable(true).default_size(230.0).frame(egui::Frame::new().fill(HEADER_BG).inner_margin(egui::Margin::same(8))).show(ui, |ui| {
            self.side_panel(ui, p, &st, &mut actions);
        });

        // ---------------- pattern area ----------------
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ui, |ui| {
            self.pattern_area(ui, p, &st, &mut actions);
        });

        if self.about {
            let mut open = true;
            egui::Window::new("About modplay").open(&mut open).collapsible(false).resizable(false).default_width(460.0).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ui.ctx(), about_contents);
            self.about = open;
        }
        actions
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, p: &dyn Player, st: &modplay::player::Status, actions: &mut Vec<Action>) {
        ui.label(egui::RichText::new("ORDER LIST").size(11.0).strong().color(TXT_ROWNUM));
        ui.add_space(2.0);
        let n = p.num_orders();
        let follow = self.last_order != st.order;
        self.last_order = st.order;
        egui::ScrollArea::vertical().id_salt("orders").max_height(ui.available_height() * 0.38).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                for o in 0..n {
                    let pat = order_pattern(p, o);
                    let cur = o == st.order;
                    let text = egui::RichText::new(match pat {
                        Some(x) => format!("{x:02}"),
                        None => "--".into(),
                    })
                    .monospace()
                    .size(12.0)
                    .color(if cur { Color32::WHITE } else { TXT_NOTE });
                    let b = egui::Button::new(text).min_size(Vec2::new(30.0, 20.0)).fill(if cur { ACCENT.gamma_multiply(0.8) } else { BG_BAR });
                    let r = ui.add(b).on_hover_text(format!("Position {o:02}"));
                    if r.clicked() {
                        actions.push(Action::JumpOrder(o));
                    }
                    if cur && follow {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        let ns = p.num_samples();
        let kind = if p.is_protracker() || p.format_name().starts_with("MOD") { "SAMPLES" } else { "INSTRUMENTS / SAMPLES" };
        ui.label(egui::RichText::new(kind).size(11.0).strong().color(TXT_ROWNUM));
        ui.add_space(2.0);
        egui::ScrollArea::vertical().id_salt("instruments").auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..ns {
                let si = p.sample_info(i);
                let name = si.name.trim().to_string();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(egui::RichText::new(format!("{:02X}", i + 1)).monospace().size(12.0).color(TXT_INS));
                    ui.label(egui::RichText::new(if name.is_empty() { "—".into() } else { name }).size(12.0).color(if si.length == Some(0) { TXT_DIM } else { TXT_NOTE }));
                });
            }
        });
    }

    fn pattern_area(&mut self, ui: &mut egui::Ui, p: &dyn Player, st: &modplay::player::Status, actions: &mut Vec<Action>) {
        let font = FontId::monospace(FONT_SIZE);
        let (cw, row_h) = ui.fonts_mut(|f| (f.glyph_width(&font, '0'), f.row_height(&font) + 2.0));
        let nch = p.num_channels();
        let has_vol = p.has_volume_column();
        // "C-5 01 v64 A06" or "C-5 01 A06"
        let cell_chars = if has_vol { 14.0 } else { 10.0 };
        let rownum_w = 3.0 * cw + 16.0;
        let full = ui.available_rect_before_wrap();
        // channels share the width when there is room (up to a comfortable maximum)
        let min_w = (cell_chars * cw + 18.0).max(110.0);
        let fit = (full.width() - rownum_w - 2.0) / nch.max(1) as f32;
        let col_w = fit.clamp(min_w, min_w * 2.0);
        let rows_top = full.top() + HEADER_H;
        let visible = (((full.bottom() - rows_top) / row_h).floor() as i32).max(1);
        let center = visible / 2;
        let pattern = st.pattern;
        let num_rows = p.num_rows(pattern).max(1);
        let cur = st.row.clamp(0, num_rows - 1);
        let first = cur - center;

        let painter = ui.painter_at(full);
        // background row bands (full width, behind everything)
        for i in 0..visible {
            let r = first + i;
            if r < 0 || r >= num_rows {
                continue;
            }
            let y = rows_top + i as f32 * row_h;
            let band = Rect::from_min_size(Pos2::new(full.left(), y), Vec2::new(full.width(), row_h));
            let c = if r == cur {
                CUR_ROW
            } else if r % 16 == 0 {
                BG_BAR
            } else if r % 4 == 0 {
                BG_BEAT
            } else {
                continue;
            };
            painter.rect_filled(band, 0.0, c);
        }

        // ---- row numbers (fixed on the left) ----
        let rn_rect = Rect::from_min_max(full.min, Pos2::new(full.left() + rownum_w, full.bottom()));
        painter.rect_filled(Rect::from_min_max(rn_rect.min, Pos2::new(rn_rect.right(), rows_top)), 0.0, HEADER_BG);
        let wide_rows = num_rows > 100;
        for i in 0..visible {
            let r = first + i;
            if r < 0 || r >= num_rows {
                continue;
            }
            let y = rows_top + i as f32 * row_h + row_h / 2.0;
            let s = if wide_rows { format!("{r:03}") } else { format!("{r:02}") };
            let col = if r == cur { Color32::WHITE } else if r % 4 == 0 { TXT_NOTE } else { TXT_ROWNUM };
            painter.text(Pos2::new(rn_rect.right() - 8.0, y), Align2::RIGHT_CENTER, s, font.clone(), col);
        }
        painter.vline(rn_rect.right(), full.y_range(), Stroke::new(1.0, GRID));

        // ---- channels (horizontally scrollable) ----
        let ch_area = Rect::from_min_max(Pos2::new(rn_rect.right() + 1.0, full.top()), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(ch_area));
        egui::ScrollArea::horizontal().id_salt("channels").auto_shrink([false, false]).show(&mut child, |ui| {
            let total_w = col_w * nch as f32;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, ch_area.height()), Sense::hover());
            let painter = ui.painter_at(rect.intersect(ch_area));
            let clip = ch_area;
            for ch in 0..nch {
                let x0 = rect.left() + ch as f32 * col_w;
                if x0 > clip.right() || x0 + col_w < clip.left() {
                    continue;
                }
                let col = Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(col_w, rect.height()));
                let muted = p.is_muted(ch);
                self.channel_header(ui, &painter, p, ch, Rect::from_min_size(col.min, Vec2::new(col_w, HEADER_H)), muted, st.playing, actions);

                // cells
                let alpha = if muted { 0.35 } else { 1.0 };
                for i in 0..visible {
                    let r = first + i;
                    if r < 0 || r >= num_rows {
                        continue;
                    }
                    let y = rows_top + i as f32 * row_h + row_h / 2.0;
                    let eff = p.cell(pattern, r, ch, CellView::Effect);
                    let vol = if has_vol { Some(p.cell(pattern, r, ch, CellView::Volume)) } else { None };
                    let mut x = x0 + 9.0;
                    let mut put = |s: &[u8], c: Color32| {
                        // empty fields: dots (IT/XM style) or "---" / "00" / "000" (ProTracker style)
                        let empty = s.iter().all(|&b| b == DOT || b == b' ' || b == b'.') || s.iter().all(|&b| b == b'-') || s.iter().all(|&b| b == b'0');
                        let text: String = if empty { "·".repeat(s.len()) } else { s.iter().map(|&b| if b == DOT { '·' } else { b as char }).collect() };
                        let color = if empty { TXT_DIM } else { c };
                        painter.text(Pos2::new(x, y), Align2::LEFT_CENTER, text, font.clone(), color.gamma_multiply(alpha));
                        x += (s.len() as f32 + 1.0) * cw;
                    };
                    put(&eff[0..3], TXT_NOTE);
                    put(&eff[3..5], TXT_INS);
                    if let Some(v) = vol {
                        put(&v[5..8], TXT_VOL);
                    }
                    put(&eff[5..8], TXT_EFF);
                }
                painter.vline(col.right(), rect.y_range(), Stroke::new(1.0, GRID));
            }
        });
        // current row outline across everything
        let y = rows_top + center as f32 * row_h;
        painter.rect_stroke(Rect::from_min_size(Pos2::new(full.left(), y), Vec2::new(full.width(), row_h)), 0.0, Stroke::new(1.0, ACCENT.gamma_multiply(0.7)), StrokeKind::Inside);
        let _ = ui.allocate_rect(full, Sense::hover());
    }

    #[allow(clippy::too_many_arguments)]
    fn channel_header(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, p: &dyn Player, ch: usize, r: Rect, muted: bool, playing: bool, actions: &mut Vec<Action>) {
        painter.rect_filled(r, 0.0, HEADER_BG);
        painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, GRID));
        // name + buttons
        let top = r.top() + 12.0;
        painter.text(Pos2::new(r.left() + 9.0, top), Align2::LEFT_CENTER, format!("CH {}", ch + 1), FontId::proportional(12.0), if muted { TXT_ROWNUM } else { TXT_NOTE });
        let bsize = Vec2::new(20.0, 16.0);
        let solo_rect = Rect::from_min_size(Pos2::new(r.right() - 8.0 - bsize.x, top - bsize.y / 2.0), bsize);
        let mute_rect = solo_rect.translate(Vec2::new(-bsize.x - 4.0, 0.0));
        let is_solo = self.solo == Some(ch);
        if small_toggle(ui, painter, mute_rect, "M", muted, MUTED, ("mute", ch)).clicked() {
            self.solo = None;
            actions.push(Action::MuteAbs(ch));
        }
        if small_toggle(ui, painter, solo_rect, "S", is_solo, SOLO, ("solo", ch)).clicked() {
            if is_solo {
                self.solo = None;
                actions.push(Action::Unsolo);
            } else {
                self.solo = Some(ch);
                actions.push(Action::Solo(ch));
            }
        }

        // scope
        let scope = Rect::from_min_max(Pos2::new(r.left() + 7.0, r.top() + 24.0), Pos2::new(r.right() - 7.0, r.bottom() - 12.0));
        painter.rect_filled(scope, CornerRadius::same(3), BG);
        let mid = scope.center().y;
        let half = scope.height() / 2.0 - 2.0;
        let mut drawn = false;
        if playing && !muted {
            if let Some(sc) = p.scope(ch) {
                if sc.volume > 0 {
                    let amp = sc.volume as f32 / 64.0;
                    let n = sc.samples.len();
                    let pts: Vec<Pos2> = sc
                        .samples
                        .iter()
                        .enumerate()
                        .map(|(i, &s)| Pos2::new(scope.left() + 2.0 + (scope.width() - 4.0) * i as f32 / (n - 1) as f32, mid - (s as f32 / 128.0) * amp * half))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(1.3, SCOPE_LINE)));
                    drawn = true;
                }
            } else if let Some(h) = self.hist.get(ch) {
                // backend without per-channel waveforms (libopenmpt): show how the
                // channel's level moved over the last ~1.5 s, newest on the right
                if h.iter().any(|&v| v > 0.01) {
                    let n = h.len().max(2);
                    let step = (scope.width() - 4.0) / (HIST_LEN - 1) as f32;
                    let x0 = scope.right() - 2.0 - (n - 1) as f32 * step;
                    let top: Vec<Pos2> = h.iter().enumerate().map(|(i, &v)| Pos2::new(x0 + i as f32 * step, mid - v * half)).collect();
                    let bot: Vec<Pos2> = top.iter().map(|p| Pos2::new(p.x, 2.0 * mid - p.y)).collect();
                    let fill = SCOPE_LINE.gamma_multiply(0.22);
                    for i in 0..top.len().saturating_sub(1) {
                        // symmetric trapezoids are convex, so they can be filled directly
                        let quad = vec![top[i], top[i + 1], bot[i + 1], bot[i]];
                        painter.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
                    }
                    painter.add(egui::Shape::line(top, Stroke::new(1.2, SCOPE_LINE)));
                    painter.add(egui::Shape::line(bot, Stroke::new(1.2, SCOPE_LINE)));
                    drawn = true;
                }
            }
        }
        if !drawn {
            painter.hline(scope.left() + 2.0..=scope.right() - 2.0, mid, Stroke::new(1.0, if muted { MUTED.gamma_multiply(0.5) } else { SCOPE_LINE.gamma_multiply(0.4) }));
        }

        // VU bar
        let v = if muted { 0.0 } else { self.vu.get(ch).copied().unwrap_or(0.0) };
        let bar = Rect::from_min_max(Pos2::new(scope.left(), r.bottom() - 8.0), Pos2::new(scope.right(), r.bottom() - 4.0));
        painter.rect_filled(bar, CornerRadius::same(2), BG);
        if v > 0.0 {
            // segment colours by position: green, then yellow, red only at the very top
            let segs = [(0.0, 0.72, Color32::from_rgb(0x5f, 0xd0, 0x7a)), (0.72, 0.9, Color32::from_rgb(0xf2, 0xc9, 0x4c)), (0.9, 1.0, Color32::from_rgb(0xf0, 0x5a, 0x4a))];
            for (lo, hi, c) in segs {
                let end = v.min(1.0).min(hi);
                if end > lo {
                    let r = Rect::from_min_max(Pos2::new(bar.left() + bar.width() * lo, bar.top()), Pos2::new(bar.left() + bar.width() * end, bar.bottom()));
                    painter.rect_filled(r, CornerRadius::ZERO, c);
                }
            }
        }
    }

    fn update_vu(&mut self, p: &dyn Player) {
        let n = p.num_channels();
        if self.vu.len() != n {
            self.vu = vec![0.0; n];
            self.vu_last_trigger = (0..n).map(|c| if let VuState::Trigger(cnt, _) = p.vu(c) { cnt } else { 0 }).collect();
        }
        if self.hist.len() != n {
            self.hist = vec![std::collections::VecDeque::with_capacity(HIST_LEN); n];
        }
        let playing = p.status().playing;
        for ch in 0..n {
            if playing {
                let lv = match p.vu(ch) {
                    VuState::Level(h) => h as f32 / 48.0,
                    VuState::Trigger(..) => 0.0,
                };
                let h = &mut self.hist[ch];
                if h.len() == HIST_LEN {
                    h.pop_front();
                }
                h.push_back(lv);
            }
            match p.vu(ch) {
                VuState::Trigger(cnt, h) => {
                    self.vu[ch] = (self.vu[ch] - 1.0 / 48.0).max(0.0);
                    if cnt != self.vu_last_trigger[ch] {
                        self.vu_last_trigger[ch] = cnt;
                        self.vu[ch] = h as f32 / 48.0;
                    }
                }
                VuState::Level(h) => {
                    let t = h as f32 / 48.0;
                    let cur = self.vu[ch];
                    self.vu[ch] = if t >= cur { t } else { (cur - 0.03).max(t) };
                }
            }
        }
    }
}

/// Pattern number at order `o`, or None for skip/end markers (shown as "--")
fn order_pattern(p: &dyn Player, o: i32) -> Option<i32> {
    p.order_pattern(o)
}

/// A small "LABEL / value" read-out box
fn stat(ui: &mut egui::Ui, label: &str, value: &str) {
    let w = (value.len() as f32 * 9.5).max(label.len() as f32 * 6.5) + 16.0;
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(r, CornerRadius::same(4), BG);
    painter.text(Pos2::new(r.center().x, r.top() + 9.0), Align2::CENTER_CENTER, label, FontId::proportional(9.5), TXT_ROWNUM);
    painter.text(Pos2::new(r.center().x, r.top() + 24.0), Align2::CENTER_CENTER, value, FontId::monospace(15.0), TXT_NOTE);
}

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Pause,
    Stop,
    Prev,
    Next,
    Open,
}

/// A button with a vector-drawn icon (the default fonts lack media symbols)
fn icon_button(ui: &mut egui::Ui, icon: Icon, enabled: bool, active: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(36.0, 30.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    let fill = if active {
        ACCENT.gamma_multiply(0.85)
    } else if enabled && resp.is_pointer_button_down_on() {
        BG_BAR.gamma_multiply(2.2)
    } else if enabled && resp.hovered() {
        BG_BAR.gamma_multiply(1.7)
    } else {
        BG_BAR
    };
    painter.rect_filled(r, CornerRadius::same(5), fill);
    let c = if !enabled {
        TXT_DIM
    } else if active {
        Color32::WHITE
    } else {
        TXT_NOTE
    };
    let m = r.center();
    let tri = |pts: [Pos2; 3]| egui::Shape::convex_polygon(pts.to_vec(), c, Stroke::NONE);
    match icon {
        Icon::Play => {
            painter.add(tri([m + Vec2::new(-5.0, -7.0), m + Vec2::new(-5.0, 7.0), m + Vec2::new(7.0, 0.0)]));
        }
        Icon::Pause => {
            painter.rect_filled(Rect::from_center_size(m + Vec2::new(-3.5, 0.0), Vec2::new(4.0, 13.0)), CornerRadius::same(1), c);
            painter.rect_filled(Rect::from_center_size(m + Vec2::new(3.5, 0.0), Vec2::new(4.0, 13.0)), CornerRadius::same(1), c);
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(m, Vec2::new(12.0, 12.0)), CornerRadius::same(2), c);
        }
        Icon::Prev => {
            painter.rect_filled(Rect::from_center_size(m + Vec2::new(-6.0, 0.0), Vec2::new(2.5, 12.0)), CornerRadius::ZERO, c);
            painter.add(tri([m + Vec2::new(6.0, -6.0), m + Vec2::new(6.0, 6.0), m + Vec2::new(-4.5, 0.0)]));
        }
        Icon::Next => {
            painter.rect_filled(Rect::from_center_size(m + Vec2::new(6.0, 0.0), Vec2::new(2.5, 12.0)), CornerRadius::ZERO, c);
            painter.add(tri([m + Vec2::new(-6.0, -6.0), m + Vec2::new(-6.0, 6.0), m + Vec2::new(4.5, 0.0)]));
        }
        Icon::Open => {
            // folder
            let body = Rect::from_center_size(m + Vec2::new(0.0, 1.5), Vec2::new(16.0, 11.0));
            let tab = Rect::from_min_size(body.min - Vec2::new(0.0, 3.0), Vec2::new(7.0, 4.0));
            painter.rect_filled(tab, CornerRadius::same(1), c);
            painter.rect_filled(body, CornerRadius::same(2), c);
        }
    }
    resp
}

fn small_toggle(ui: &mut egui::Ui, painter: &egui::Painter, r: Rect, text: &str, on: bool, on_color: Color32, id: (&str, usize)) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
    let fill = if on {
        on_color
    } else if resp.hovered() {
        BG_BAR.gamma_multiply(1.6)
    } else {
        BG_BAR
    };
    painter.rect_filled(r, CornerRadius::same(3), fill);
    painter.text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.0), if on { Color32::BLACK } else { TXT_ROWNUM });
    resp
}

/// The About window: what the engines are built on, with thanks.
fn about_contents(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.label(egui::RichText::new("modplay-rs").size(16.0).strong().color(TXT_NOTE));
    ui.label("Tracker music (MOD / IT / S3M / XM) played by engines written in Rust.");
    ui.hyperlink_to("github.com/mooyax/modplay-rs", "https://github.com/mooyax/modplay-rs");
    ui.separator();
    ui.label(egui::RichText::new("Built on the work of").strong().color(TXT_NOTE));
    let credit = |ui: &mut egui::Ui, name: &str, url: &str, what: &str| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.hyperlink_to(name, url);
            ui.label(egui::RichText::new(what).color(TXT_ROWNUM));
        });
    };
    credit(ui, "pt2-clone", "https://github.com/8bitbubsy/pt2-clone", "by Olav Sørensen (8bitbubsy) — the MOD engine is a port of its replayer, Paula emulation and filters (BSD-3-Clause)");
    credit(ui, "ft2-clone", "https://github.com/8bitbubsy/ft2-clone", "by Olav Sørensen (8bitbubsy) — the XM engine is a port of its replayer (BSD-3-Clause)");
    credit(ui, "OpenMPT / libopenmpt", "https://openmpt.org/", "by the OpenMPT Project Developers and Olivier Lapicque — the reference for the IT / S3M engine; its resonant filter and mix-level tables follow OpenMPT (BSD-3-Clause)");
    credit(ui, "libxmp", "https://github.com/libxmp/libxmp", "— its test modules were used to check the engines");
    ui.separator();
    ui.label(egui::RichText::new("And the trackers themselves").strong().color(TXT_NOTE));
    ui.label(egui::RichText::new("Ultimate Soundtracker and ProTracker (Amiga), Scream Tracker 3 (Future Crew), FastTracker 2 (Triton), Impulse Tracker (Jeffrey Lim) — and everyone who made music with them.").color(TXT_ROWNUM));
    ui.separator();
    ui.label(egui::RichText::new("BSD 3-Clause License. The copyright notices of pt2-clone, ft2-clone and OpenMPT are in the LICENSE file.").size(11.5).color(TXT_ROWNUM));
}
