//! Main Control Panel window, laid out like the 1991 `AD.EXE` panel (SEG 11 proportions):
//! logo banner + power switch on top, module list on the left, the module's four control slots
//! on the right with Demo / Sleep Now / theme buttons beneath, credits and a status strip below.

use ad_sdk::AdControl;
use ad_sdk::prefs::{FrameRateMode, ScaleMode};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
    UiBuilder, pos2, vec2,
};

use super::dialogs::{self, badge};
use super::pickers::PickTarget;
use super::theme::{self, Look, bold};
use super::{Action, Filter, PanelApp, visible};
use crate::common::Collection;

/// Scale from the original 512x526 dialog-unit layout to egui points.
const K: f32 = 1.16;
pub const CLIENT_W: f32 = 512.0 * K;
pub const CLIENT_H: f32 = 546.0 * K;

fn rc(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x * K, y * K), vec2(w * K, h * K))
}

struct Layout_ {
    logo: Rect,
    power: Rect,
    switch: Rect,
    list: Rect,
    ctrl: Rect,
    demo: Rect,
    credits: Rect,
    status: Rect,
}

fn layout() -> Layout_ {
    Layout_ {
        logo: rc(8.0, 8.0, 344.0, 108.0),
        power: rc(358.0, 8.0, 146.0, 108.0),
        switch: rc(372.0, 16.0, 40.0, 58.0),
        list: rc(8.0, 122.0, 204.0, 294.0),
        ctrl: rc(218.0, 122.0, 286.0, 240.0),
        demo: rc(218.0, 368.0, 286.0, 48.0),
        credits: rc(8.0, 422.0, 496.0, 58.0),
        status: rc(8.0, 486.0, 496.0, 52.0),
    }
}

/// Run `add` in a child ui confined to `rect`.
fn at<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), add).inner
}

fn filter_choices(app: &PanelApp) -> Vec<(Filter, String)> {
    let count = |c: Collection| app.data.catalog.iter().filter(|e| Collection::of(&e.meta.id) == c).count();
    vec![
        (Filter::All, format!("All Collections ({})", app.data.catalog.len())),
        (Filter::Only(Collection::Classic), format!("Classic 1991 ({})", count(Collection::Classic))),
        (Filter::Only(Collection::Simpsons), format!("The Simpsons ({})", count(Collection::Simpsons))),
        (Filter::Only(Collection::StarTrek), format!("Star Trek: TNG ({})", count(Collection::StarTrek))),
        (Filter::Only(Collection::LooneyTunes), format!("Looney Tunes ({})", count(Collection::LooneyTunes))),
        (Filter::Favorites, "★ Favorites".to_string()),
    ]
}

impl PanelApp {
    pub(super) fn draw(&mut self, root: &mut Ui) -> Vec<Action> {
        let look = self.look;
        let pal = look.palette();
        let mut actions = Vec::new();
        let l = layout();

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.bg))
            .show(root, |ui| {
                let origin = ui.max_rect().min.to_vec2();
                let l = Layout_ {
                    logo: l.logo.translate(origin),
                    power: l.power.translate(origin),
                    switch: l.switch.translate(origin),
                    list: l.list.translate(origin),
                    ctrl: l.ctrl.translate(origin),
                    demo: l.demo.translate(origin),
                    credits: l.credits.translate(origin),
                    status: l.status.translate(origin),
                };
                let painter = ui.painter().clone();
                for r in [l.power, l.list, l.ctrl, l.demo, l.credits, l.status] {
                    theme::platform(&painter, r, look);
                }
                self.draw_logo(ui, l.logo, &mut actions);
                self.draw_power(ui, l.power, l.switch, &mut actions);
                self.draw_list(ui, l.list, &mut actions);
                self.draw_controls(ui, l.ctrl, &mut actions);
                self.draw_demo_row(ui, l.demo, &mut actions);
                self.draw_credits(ui, l.credits, &mut actions);
                self.draw_status(ui, l.status, &mut actions);
            });
        actions
    }

    fn draw_logo(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let pal = look.palette();
        let p = ui.painter();
        let radius = if look.is_retro() { 0.0 } else { 8.0 };
        p.rect_filled(rect, radius, pal.logo_bg);
        p.rect_stroke(rect, radius, Stroke::new(1.0, pal.logo_border), egui::StrokeKind::Inside);
        if let Some(art) = &self.art {
            let src = super::art::Art::size(&art.logo);
            let avail = rect.shrink(4.0 * K).size();
            // Integer-ish nearest-neighbour stretch preserving aspect.
            let scale = (avail.x / src.x).min(avail.y / src.y);
            let size = src * scale;
            let img = Rect::from_center_size(rect.center(), size);
            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
            p.image(art.logo.id(), img, uv, Color32::WHITE);
            let m = super::art::Art::size(&art.meteor) * scale;
            p.image(art.meteor.id(), Rect::from_min_size(img.min, m), uv, Color32::WHITE);
        } else {
            p.text(rect.center(), Align2::CENTER_CENTER, "After Dark", FontId::new(40.0, look.bold()), Color32::WHITE);
        }
        let resp = ui.interact(rect, ui.scope_id().with("logo"), Sense::click());
        if resp.on_hover_text("About After Dark").clicked() {
            actions.push(Action::AppAbout);
        }
    }

    fn draw_power(&mut self, ui: &mut Ui, panel: Rect, switch: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let pal = look.palette();
        let on = self.data.prefs.screen_saver_on;
        let p = ui.painter().clone();
        if let Some(art) = &self.art {
            let tex = if on { &art.on_switch } else { &art.off_switch };
            let size = super::art::Art::size(tex) * 1.85;
            let r = Rect::from_min_size(switch.min + vec2(4.0, 2.0), size);
            p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        let font = FontId::new(15.0, look.bold());
        let tx = switch.right() + 8.0;
        p.text(pos2(tx, switch.top() + 6.0), Align2::LEFT_TOP, "On", font.clone(), if on { pal.accent } else { pal.muted });
        p.text(pos2(tx, switch.top() + 34.0), Align2::LEFT_TOP, "Off", font, if on { pal.muted } else { pal.off });
        let hit = Rect::from_min_max(panel.min, pos2(panel.right(), switch.bottom() + 4.0));
        let resp = ui.interact(hit, ui.scope_id().with("power"), Sense::click());
        if resp
            .on_hover_text(if on { "After Dark is On — click to switch it Off" } else { "After Dark is Off — click to switch it On" })
            .clicked()
        {
            actions.push(Action::TogglePower);
        }
        let btn = Rect::from_min_max(pos2(panel.left() + 10.0, panel.bottom() - 34.0), pos2(panel.right() - 10.0, panel.bottom() - 8.0));
        at(ui, btn, |ui| {
            if theme::button(ui, look, "When / Setup…", btn.size()).on_hover_text("Alt+W").clicked() {
                actions.push(Action::Setup);
            }
        });
    }

    fn draw_list(&mut self, ui: &mut Ui, panel: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let inner = panel.shrink(8.0);
        let filtered_before = self.filtered();
        let keyboard_blocked = self.dialog.is_some() || ui.ctx().egui_wants_keyboard_input();
        at(ui, inner, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let fav_w = 58.0;
                let combo_w = inner.width() - fav_w - ui.spacing().item_spacing.x;
                let choices = filter_choices(self);
                let current = choices.iter().find(|(f, _)| *f == self.filter).map(|(_, s)| s.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("collection-filter")
                    .width(combo_w - 8.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (f, label) in &choices {
                            if ui.selectable_value(&mut self.filter, *f, label).changed() {
                                self.scroll_to_selected = true;
                            }
                        }
                    });
                let fav = self.is_favorite(&self.entry().meta.id.clone());
                if theme::button(ui, look, if fav { "★ Fav" } else { "☆ Fav" }, vec2(fav_w, 22.0))
                    .on_hover_text(if fav { "Remove from Favorites" } else { "Add to Favorites" })
                    .clicked()
                {
                    actions.push(Action::ToggleFavorite);
                }
            });
            let hint = format!("Search {} modules...", self.data.catalog.len());
            if ui.add(theme::text_edit(look, TextEdit::singleline(&mut self.search).hint_text(hint).desired_width(f32::INFINITY))).changed() {
                self.scroll_to_selected = true;
            }
            let filtered = self.filtered();
            // Like the Windows list: if the selection is filtered out, the first match becomes selected.
            if (filtered != filtered_before || self.scroll_to_selected)
                && let Some(&first) = filtered.first()
                && !filtered.contains(&self.selected)
            {
                actions.push(Action::Select { idx: first, demo: false });
            }
            // Arrow keys walk the list like the Win 3.1 list box (when no text field has focus).
            if !keyboard_blocked && let Some(pos) = filtered.iter().position(|&i| i == self.selected) {
                let step = ui.input_mut(|i| {
                    let mut step = 0isize;
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        step = 1;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        step = -1;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
                        step = 10;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) {
                        step = -10;
                    }
                    step
                });
                if step != 0 {
                    let next = (pos as isize + step).clamp(0, filtered.len() as isize - 1) as usize;
                    actions.push(Action::Select { idx: filtered[next], demo: false });
                    self.scroll_next = true;
                }
            }
            let list_rect = Rect::from_min_max(ui.cursor().min, inner.max);
            at(ui, list_rect, |ui| {
                dialogs::list_frame(look).show(ui, |ui| {
                    ui.set_min_size(list_rect.size() - vec2(8.0, 8.0));
                    ScrollArea::vertical().id_salt("module-list").auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if filtered.is_empty() {
                            ui.label(RichText::new("No matching modules").italics().color(look.palette().muted));
                        }
                        for idx in filtered {
                            let e = &self.data.catalog[idx];
                            let fav = self.is_favorite(&e.meta.id);
                            let text = if fav { format!("★ {}", e.meta.name) } else { e.meta.name.clone() };
                            let sel = idx == self.selected;
                            let resp = list_row(ui, look, &text, sel);
                            if sel && self.scroll_to_selected {
                                resp.scroll_to_me(Some(Align::Center));
                            }
                            if resp.double_clicked() {
                                actions.push(Action::Select { idx, demo: true });
                            } else if resp.clicked() {
                                actions.push(Action::Select { idx, demo: false });
                            }
                        }
                    });
                });
            });
        });
        self.scroll_to_selected = std::mem::take(&mut self.scroll_next);
    }

    fn draw_controls(&mut self, ui: &mut Ui, panel: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let pal = look.palette();
        let entry = self.entry().clone();
        let raw = self.data.raw_controls(&entry);
        let inner = panel.shrink2(vec2(12.0 * K, 10.0 * K));
        at(ui, inner, |ui| {
            // Module name heading with its collection badge.
            ui.horizontal(|ui| {
                ui.label(RichText::new(&entry.meta.name).family(look.bold()).size(15.0).color(pal.accent));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    badge(ui, Collection::of(&entry.meta.id), look, 78.0);
                });
            });
            ui.add_space(2.0);
            let row_h = 54.0 * K * 0.92;
            let shown: Vec<usize> = (0..4).filter(|&s| visible(&entry.meta.controls[s])).collect();
            if shown.is_empty() {
                ui.add_space(20.0);
                ui.label(RichText::new("This module has no settings.").italics().color(pal.muted));
            }
            for slot in shown {
                let ctrl = &entry.meta.controls[slot];
                let top = ui.cursor().min.y;
                let width = inner.width();
                ui.scope_builder(UiBuilder::new().id_salt(("slot", &entry.meta.id, slot)), |ui| {
                    self.draw_slot(ui, look, ctrl, slot, raw[slot], width, &entry.meta.id, actions);
                });
                let used = ui.cursor().min.y - top;
                if used < row_h {
                    ui.add_space(row_h - used);
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_slot(&self, ui: &mut Ui, look: Look, ctrl: &AdControl, slot: usize, raw: i32, width: f32, id: &str, actions: &mut Vec<Action>) {
        let pal = look.palette();
        match ctrl {
            AdControl::LabeledSlider { title, .. } | AdControl::NumericSlider { title, .. } => {
                ui.horizontal(|ui| {
                    ui.label(bold(look, title));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(ctrl.format_display_value(raw)).color(pal.accent));
                    });
                });
                let mut v = raw.clamp(0, 100);
                let r = theme::slider(ui, look, &mut v, width - 4.0);
                if r.changed() && v != raw {
                    actions.push(Action::Control { slot, raw: v });
                }
            }
            AdControl::Menu { title, choices, .. } => {
                ui.label(bold(look, title));
                let mut v = raw.clamp(0, choices.len().saturating_sub(1) as i32);
                let current = choices.get(v as usize).cloned().unwrap_or_default();
                egui::ComboBox::from_id_salt(("menu", id, slot)).width(width - 8.0).selected_text(current).show_ui(ui, |ui| {
                    for (i, c) in choices.iter().enumerate() {
                        ui.selectable_value(&mut v, i as i32, c);
                    }
                });
                if v != raw {
                    actions.push(Action::Control { slot, raw: v });
                }
            }
            AdControl::Checkbox { title, .. } => {
                ui.add_space(8.0);
                let mut on = raw != 0;
                if ui.checkbox(&mut on, bold(look, title)).changed() {
                    actions.push(Action::Control { slot, raw: i32::from(on) });
                }
            }
            AdControl::Button { title } => {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, look, title.as_str(), vec2(120.0, 26.0)).clicked() {
                        actions.push(Action::ModuleButton { slot });
                    }
                    // Show the current picture/folder for the picker buttons.
                    let path = match id {
                        "logo" => Some((&self.data.prefs.logo_file, PickTarget::LogoFile)),
                        "globe" => Some((&self.data.prefs.globe_file, PickTarget::GlobeFile)),
                        "slidesho" => Some((&self.data.prefs.slide_show_dir, PickTarget::SlideShowDir)),
                        _ => None,
                    };
                    if let Some((path, target)) = path {
                        let name = std::path::Path::new(path.as_str())
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .filter(|_| !path.trim().is_empty());
                        let text = match (name, target.is_dir()) {
                            (Some(n), true) => format!("{n}/"),
                            (Some(n), false) => n,
                            (None, _) => "(built-in picture)".to_string(),
                        };
                        ui.add(egui::Label::new(RichText::new(text).small().color(pal.muted)).truncate())
                            .on_hover_text(path.as_str());
                    }
                });
            }
            AdControl::None => {}
        }
    }

    fn draw_demo_row(&mut self, ui: &mut Ui, panel: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let inner = panel.shrink2(vec2(10.0, 0.0));
        at(ui, inner, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.set_height(inner.height());
                let w = (inner.width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                let demo = self.demo_running();
                if theme::button(ui, look, if demo { "Stop Demo" } else { "Demo" }, vec2(w, 30.0))
                    .on_hover_text(if demo { "Stop the running demo" } else { "Preview this module full screen (Alt+D). Escape or click to stop." })
                    .clicked()
                {
                    actions.push(Action::Demo);
                }
                if theme::button(ui, look, "Sleep Now", vec2(w, 30.0)).on_hover_text("Start the screen saver now (Alt+S)").clicked() {
                    actions.push(Action::SleepNow);
                }
                let label = if look.is_retro() { "Modern UI" } else { "Retro 3.1" };
                if theme::button(ui, look, label, vec2(w, 30.0)).on_hover_text("Switch the Control Panel look").clicked() {
                    actions.push(Action::ToggleTheme);
                }
            });
        });
    }

    fn draw_credits(&mut self, ui: &mut Ui, panel: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let pal = look.palette();
        let credit = self.entry().meta.credit.replace("\r\n", "\n");
        let btn_w = 64.0;
        let text_rect = Rect::from_min_max(panel.min + vec2(10.0, 7.0), pos2(panel.right() - btn_w - 22.0, panel.bottom() - 6.0));
        let galley = ui.painter().layout(credit, FontId::new(12.0, look.bold()), pal.text, text_rect.width());
        ui.painter().with_clip_rect(text_rect).galley(text_rect.min, galley, pal.text);
        // Clicking the credits shows the module's About text (1991 MORECREDITWNDPROC).
        if ui.interact(text_rect, ui.scope_id().with("credits"), Sense::click()).on_hover_text("About this module").clicked() {
            actions.push(Action::ModuleAbout);
        }
        let btn = Rect::from_center_size(pos2(panel.right() - 12.0 - btn_w / 2.0, panel.center().y), vec2(btn_w, 28.0));
        at(ui, btn, |ui| {
            if theme::button(ui, look, "Info…", btn.size()).clicked() {
                actions.push(Action::ModuleAbout);
            }
        });
    }

    fn draw_status(&mut self, ui: &mut Ui, panel: Rect, actions: &mut Vec<Action>) {
        let look = self.look;
        let pal = look.palette();
        let p = &self.data.prefs;
        let corners = if p.no_corners {
            "Corners: Off".to_string()
        } else {
            format!("Sleep: {} | Never: {}", p.sleep_corner.label(), p.wake_corner.label())
        };
        let fps = match p.frame_rate_mode {
            FrameRateMode::AuthenticRetro => "Retro FPS",
            FrameRateMode::Smooth60Fps => "60 FPS",
            FrameRateMode::HighRefresh120Fps => "120 FPS",
        };
        let scale = match p.scale_mode {
            ScaleMode::Auto => "Auto",
            ScaleMode::Scale1x => "1x",
            ScaleMode::Scale2x => "2x",
            ScaleMode::Scale3x => "3x",
            ScaleMode::Classic640x480 => "640x480 4:3",
            ScaleMode::CrtScanlines => "2x CRT",
        };
        let hotkey = if p.hotkey_enabled { dialogs::hotkey_label(p) } else { "Hotkey: Off".into() };
        let status = format!("Idle: {}m  |  {}  |  {}  |  {} ({})", p.timeout_minutes, corners, hotkey, scale, fps);
        let inner = panel.shrink2(vec2(10.0, 4.0));
        at(ui, inner, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if let Some((msg, _)) = &self.notice {
                ui.add(egui::Label::new(RichText::new(msg).color(pal.off)).truncate());
            } else {
                ui.add(egui::Label::new(RichText::new(status).color(pal.muted)).truncate());
            }
            ui.horizontal(|ui| {
                ui.set_height(22.0);
                let (dot, text, color) = if self.daemon_up {
                    ("●", "Idle monitor running", pal.good)
                } else {
                    ("○", "Idle monitor not running", pal.off)
                };
                ui.label(RichText::new(dot).color(color));
                ui.label(RichText::new(text).color(pal.text)).on_hover_text(
                    "`after-dark daemon` watches for idle time, hot corners and the Sleep Now hotkey.",
                );
                if !self.daemon_up && theme::button(ui, look, "Start", vec2(54.0, 20.0)).clicked() {
                    actions.push(Action::StartDaemon);
                }
                if !self.data.prefs.screen_saver_on {
                    ui.label(RichText::new("(After Dark is Off)").color(pal.muted));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut auto = self.autostart;
                    if ui
                        .checkbox(&mut auto, "Run at login")
                        .on_hover_text("Start the idle monitor automatically when you log in")
                        .changed()
                    {
                        actions.push(Action::Autostart(auto));
                    }
                });
            });
        });
    }
}

/// One module-list row: full-width, navy highlight like a Win 3.1 list box.
fn list_row(ui: &mut Ui, look: Look, text: &str, selected: bool) -> egui::Response {
    let pal = look.palette();
    let h = 19.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let (bg, fg) = if selected {
            match look {
                Look::Retro => (Some(pal.accent), Color32::WHITE),
                Look::Modern => (Some(Color32::from_rgb(0, 95, 140)), Color32::WHITE),
            }
        } else if resp.hovered() {
            (Some(if look.is_retro() { Color32::from_rgb(224, 224, 232) } else { Color32::from_rgb(48, 48, 54) }), pal.text)
        } else {
            (None, pal.text)
        };
        if let Some(bg) = bg {
            p.rect_filled(rect, if look.is_retro() { 0.0 } else { 4.0 }, bg);
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        let galley = p.layout_no_wrap(text.to_string(), font, fg);
        p.with_clip_rect(rect).galley(pos2(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    }
    resp
}

/// Dimmed overlay while an external file chooser is open.
pub fn waiting_overlay(ctx: &egui::Context, look: Look, target: PickTarget) {
    let pal = look.palette();
    egui::Modal::new(egui::Id::unique("picker-wait"))
        .frame(egui::Frame::new().fill(pal.card).stroke(Stroke::new(1.0, pal.muted)).inner_margin(16))
        .show(ctx, |ui| {
            ui.label(bold(look, target.title()));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Choose in the file chooser window…");
            });
        });
}
