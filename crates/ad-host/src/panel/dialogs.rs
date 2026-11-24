//! Modal dialogs: Setup ("When…"), Randomizer "Choose…", Messages / GraphStat editors, module
//! About, the app About box, and the path field used when no file chooser is installed.

use std::path::Path;

use ad_modules::grafstat::GraphStatModule;
use ad_modules::message::{MESSAGE_SLOT_COUNT, MessageRecord, load_messages_ads, serialize_messages_ads};
use ad_sdk::prefs::{AdPrefs, FrameRateMode, MultiMonitorMode, ScaleMode, ScreenCorner, UiTheme};
use ad_sdk::sound::AdSound;
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Key, Layout, Margin, RichText, ScrollArea, Sense,
    Stroke, TextEdit, Ui, Vec2, vec2,
};

use super::pickers::PickTarget;
use super::theme::{self, Look};
use crate::common::Collection;

/// What happened to a dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Cancel,
    Accept,
}

/// Show a dialog-box frame (title bar + body) as an egui modal. Returns the body's result and
/// whether the user dismissed it (Escape / title-bar close box).
fn modal<R>(
    ctx: &egui::Context,
    look: Look,
    id: &str,
    title: &str,
    width: f32,
    body: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let pal = look.palette();
    let frame = match look {
        Look::Retro => Frame::new()
            .fill(pal.card)
            .stroke(Stroke::new(1.0, Color32::BLACK))
            .shadow(egui::Shadow { offset: [4, 4], blur: 0, spread: 0, color: Color32::from_black_alpha(120) }),
        Look::Modern => Frame::new()
            .fill(pal.card)
            .stroke(Stroke::new(1.0, Color32::from_rgb(70, 70, 76)))
            .corner_radius(CornerRadius::same(10))
            .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(140) }),
    };
    let mut closed_by_box = false;
    let resp = egui::Modal::new(Id::unique(id))
        .frame(frame)
        .backdrop_color(match look {
            Look::Retro => Color32::from_black_alpha(40),
            Look::Modern => Color32::from_black_alpha(120),
        })
        .show(ctx, |ui| {
            ui.set_width(width);
            let bar_h = if look.is_retro() { 22.0 } else { 34.0 };
            let (bar, _) = ui.allocate_exact_size(vec2(width, bar_h), Sense::hover());
            let inner = Frame::new()
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    body(ui)
                })
                .inner;
            // Title bar, painted last so it spans the final dialog width.
            let bar = egui::Rect::from_min_size(bar.min, vec2(ui.min_rect().width(), bar_h));
            let p = ui.painter();
            let font = egui::FontId::new(if look.is_retro() { 12.5 } else { 14.0 }, look.bold());
            match look {
                Look::Retro => {
                    p.rect_filled(bar, 0.0, pal.title_bg);
                    p.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, Color32::BLACK));
                    p.text(bar.center(), egui::Align2::CENTER_CENTER, title, font, pal.title_fg);
                    // System-menu box (closes the dialog, like double-clicking it did).
                    let sys = egui::Rect::from_min_size(bar.min, vec2(bar_h, bar_h));
                    let r = ui.interact(sys, ui.scope_id().with("sysmenu"), Sense::click());
                    p.rect_filled(sys, 0.0, Color32::from_rgb(192, 192, 192));
                    p.line_segment([sys.right_top(), sys.right_bottom()], Stroke::new(1.0, Color32::BLACK));
                    let dash = egui::Rect::from_center_size(sys.center(), vec2(11.0, 3.0));
                    p.rect_filled(dash, 0.0, Color32::WHITE);
                    p.rect_stroke(dash, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Outside);
                    closed_by_box = r.on_hover_text("Close").clicked();
                }
                Look::Modern => {
                    p.text(bar.left_center() + vec2(16.0, 2.0), egui::Align2::LEFT_CENTER, title, font, pal.text);
                    let xr = egui::Rect::from_center_size(bar.right_center() + vec2(-20.0, 2.0), vec2(26.0, 24.0));
                    let r = ui.interact(xr, ui.scope_id().with("close"), Sense::click());
                    if r.hovered() {
                        p.rect_filled(xr, 4.0, Color32::from_rgb(196, 43, 28));
                    }
                    p.text(xr.center(), egui::Align2::CENTER_CENTER, "✕", egui::FontId::new(13.0, look.bold()), pal.text);
                    closed_by_box = r.clicked();
                }
            }
            inner
        });
    let escape = !resp.backdrop_response.clicked() && resp.should_close();
    (resp.inner, closed_by_box || escape)
}

/// Right-aligned OK/Cancel style button row. Returns the outcome.
fn ok_cancel(ui: &mut Ui, look: Look, ok_label: &str) -> Outcome {
    let mut out = Outcome::Open;
    ui.add_space(6.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if theme::button(ui, look, "Cancel", vec2(84.0, 26.0)).clicked() {
            out = Outcome::Cancel;
        }
        if theme::button(ui, look, ok_label, vec2(84.0, 26.0)).clicked() {
            out = Outcome::Accept;
        }
    });
    out
}

/// A left-aligned label occupying exactly `width` (for form columns).
fn fixed_label(ui: &mut Ui, width: f32, text: &str) {
    ui.allocate_ui_with_layout(vec2(width, 22.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        ui.label(text);
    });
}

fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, width: f32, value: &mut T, choices: &[(T, &str)]) -> bool {
    let current = choices.iter().find(|(v, _)| v == value).map(|(_, l)| *l).unwrap_or("");
    let before = *value;
    egui::ComboBox::from_id_salt(id).width(width).truncate().selected_text(current).show_ui(ui, |ui| {
        for (v, label) in choices {
            ui.selectable_value(value, *v, *label);
        }
    });
    *value != before
}

// ============================================================================================
// Setup ("When…")

const CORNERS: [(ScreenCorner, &str); 4] = [
    (ScreenCorner::TopLeft, "Top Left"),
    (ScreenCorner::TopRight, "Top Right"),
    (ScreenCorner::BottomLeft, "Bottom Left"),
    (ScreenCorner::BottomRight, "Bottom Right"),
];

/// Hotkey keys offered by the picker, as Windows virtual-key codes (what `AD_PREFS.INI` stores).
fn hotkey_keys() -> Vec<(u32, String)> {
    let mut keys: Vec<(u32, String)> = (b'A'..=b'Z').map(|c| (c as u32, (c as char).to_string())).collect();
    keys.extend((b'0'..=b'9').map(|c| (c as u32, (c as char).to_string())));
    keys.extend((1..=12).map(|n| (0x6F + n, format!("F{n}"))));
    keys.extend([
        (0x13, "Pause".to_string()),
        (0x91, "Scroll Lock".to_string()),
        (0x2D, "Insert".to_string()),
        (0x2E, "Delete".to_string()),
        (0x24, "Home".to_string()),
        (0x23, "End".to_string()),
    ]);
    keys
}

pub fn hotkey_label(prefs: &AdPrefs) -> String {
    let key = hotkey_keys()
        .into_iter()
        .find(|(vk, _)| *vk == prefs.hotkey_vk)
        .map(|(_, l)| l)
        .unwrap_or_else(|| format!("VK {:#04X}", prefs.hotkey_vk));
    let mut s = String::new();
    if prefs.hotkey_ctrl {
        s.push_str("Ctrl+");
    }
    if prefs.hotkey_shift {
        s.push_str("Shift+");
    }
    s + &key
}

pub struct SetupDlg {
    pub prefs: AdPrefs,
    sound: AdSound,
}

impl SetupDlg {
    pub fn new(prefs: &AdPrefs, sound: AdSound) -> Self {
        Self { prefs: prefs.clone(), sound }
    }

    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let p = &mut self.prefs;
        let sound = &self.sound;
        let (out, dismissed) = modal(ctx, look, "setup-dlg", "After Dark Setup — When, Display & Audio", 500.0, |ui| {
            let label_w = 184.0;
            let field_w = 268.0;
            let row = |ui: &mut Ui, label: &str, indent: f32, add: &mut dyn FnMut(&mut Ui)| {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    fixed_label(ui, label_w - indent, label);
                    add(ui);
                });
            };
            row(ui, "Idle Timeout:", 0.0, &mut |ui| {
                ui.add(egui::DragValue::new(&mut p.timeout_minutes).range(1..=999).speed(0.2));
                ui.label("minutes");
            });
            ui.add_space(2.0);
            let mut corners_on = !p.no_corners;
            if ui.checkbox(&mut corners_on, "Enable Hot Corners (Sleep Now / Never Sleep)").changed() {
                p.no_corners = !corners_on;
            }
            ui.add_enabled_ui(corners_on, |ui| {
                row(ui, "Sleep Now Corner:", 22.0, &mut |ui| {
                    combo(ui, "sleep-corner", field_w, &mut p.sleep_corner, &CORNERS);
                });
                row(ui, "Never Sleep Corner:", 22.0, &mut |ui| {
                    combo(ui, "wake-corner", field_w, &mut p.wake_corner, &CORNERS);
                });
            });
            ui.add_space(2.0);
            ui.checkbox(&mut p.hotkey_enabled, "Enable Global Sleep Now Hotkey");
            ui.add_enabled_ui(p.hotkey_enabled, |ui| {
                row(ui, "Hotkey:", 22.0, &mut |ui| {
                    ui.checkbox(&mut p.hotkey_ctrl, "Ctrl");
                    ui.checkbox(&mut p.hotkey_shift, "Shift");
                    ui.label("+");
                    let keys = hotkey_keys();
                    let mut choices: Vec<(u32, &str)> = keys.iter().map(|(v, l)| (*v, l.as_str())).collect();
                    let unknown = format!("VK {:#04X}", p.hotkey_vk);
                    if !keys.iter().any(|(v, _)| *v == p.hotkey_vk) {
                        choices.push((p.hotkey_vk, unknown.as_str()));
                    }
                    combo(ui, "hotkey-key", 90.0, &mut p.hotkey_vk, &choices);
                });
            });
            ui.add_space(4.0);
            ui.separator();
            row(ui, "Frame Rate & Smoothness:", 0.0, &mut |ui| {
                combo(ui, "fps", field_w, &mut p.frame_rate_mode, &[
                    (FrameRateMode::AuthenticRetro, FrameRateMode::AuthenticRetro.label()),
                    (FrameRateMode::Smooth60Fps, FrameRateMode::Smooth60Fps.label()),
                    (FrameRateMode::HighRefresh120Fps, FrameRateMode::HighRefresh120Fps.label()),
                ]);
            });
            row(ui, "Display Scale & Filter:", 0.0, &mut |ui| {
                let modes = [
                    ScaleMode::Auto,
                    ScaleMode::Scale1x,
                    ScaleMode::Scale2x,
                    ScaleMode::Scale3x,
                    ScaleMode::Classic640x480,
                    ScaleMode::CrtScanlines,
                ];
                let choices: Vec<_> = modes.iter().map(|m| (*m, m.label())).collect();
                combo(ui, "scale", field_w, &mut p.scale_mode, &choices);
            });
            row(ui, "Multi-Monitor Mode:", 0.0, &mut |ui| {
                let modes = [
                    MultiMonitorMode::PerMonitor,
                    MultiMonitorMode::DifferentPerMonitor,
                    MultiMonitorMode::Spanned,
                    MultiMonitorMode::PrimaryOnly,
                ];
                let choices: Vec<_> = modes.iter().map(|m| (*m, m.label())).collect();
                combo(ui, "multimon", field_w, &mut p.multi_monitor, &choices);
            });
            row(ui, "Control Panel UI Style:", 0.0, &mut |ui| {
                combo(ui, "ui-theme", field_w, &mut p.ui_theme, &[
                    (UiTheme::RetroWin31, "Retro Windows 3.1 (Default)"),
                    (UiTheme::ModernWin11, "Modern (Dark)"),
                ]);
            });
            ui.add_space(4.0);
            ui.separator();
            ui.checkbox(&mut p.sound_enabled, "Enable Audio (Digitized Voices, SFX & MIDI Music)");
            ui.add_enabled_ui(p.sound_enabled, |ui| {
                row(ui, "Voice / SFX Volume:", 0.0, &mut |ui| {
                    theme::slider(ui, look, &mut p.sfx_volume, 140.0);
                    ui.add_sized(vec2(34.0, 22.0), egui::Label::new(format!("{}%", p.sfx_volume)));
                    if theme::button(ui, look, "Test Audio", vec2(0.0, 22.0)).clicked() {
                        sound.set_volumes(p.sfx_volume, p.midi_volume);
                        // Bugs Bunny "What's up, Doc?" (exactly what the Windows Setup played).
                        ad_modules::looney::looney_pack().play_voice(sound, true, ad_modules::looney::BUGS_VOICES[5]);
                    }
                });
                row(ui, "MIDI Music Volume:", 0.0, &mut |ui| {
                    theme::slider(ui, look, &mut p.midi_volume, 140.0);
                    ui.add_sized(vec2(34.0, 22.0), egui::Label::new(format!("{}%", p.midi_volume)));
                });
            });
            ok_cancel(ui, look, "OK")
        });
        if out == Outcome::Accept {
            self.prefs.authentic_timing = self.prefs.frame_rate_mode == FrameRateMode::AuthenticRetro;
            self.prefs.sfx_volume = self.prefs.sfx_volume.clamp(0, 100);
            self.prefs.midi_volume = self.prefs.midi_volume.clamp(0, 100);
        }
        if dismissed { Outcome::Cancel } else { out }
    }
}

// ============================================================================================
// Randomizer "Choose…"

pub struct RandomizerDlg {
    items: Vec<(String, String, Collection)>,
    checked: Vec<bool>,
    favorites: Vec<String>,
}

#[derive(Clone, Copy)]
enum Preset {
    All,
    Only(Collection),
    Cartoons,
    Favorites,
    Clear,
}

pub fn badge_color(c: Collection, look: Look) -> (Color32, Color32) {
    let retro = look.is_retro();
    match c {
        Collection::Classic => (if retro { Color32::from_rgb(0, 0, 128) } else { Color32::from_rgb(52, 76, 128) }, Color32::WHITE),
        Collection::Simpsons => (Color32::from_rgb(255, 217, 15), Color32::BLACK),
        Collection::StarTrek => (Color32::from_rgb(170, 30, 30), Color32::WHITE),
        Collection::LooneyTunes => (Color32::from_rgb(230, 120, 20), Color32::BLACK),
    }
}

/// A small collection badge chip.
pub fn badge(ui: &mut Ui, c: Collection, look: Look, width: f32) {
    let (bg, fg) = badge_color(c, look);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    let radius = if look.is_retro() { 0.0 } else { 8.0 };
    ui.painter().rect_filled(rect, radius, bg);
    if look.is_retro() {
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
    }
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, c.badge(), egui::FontId::proportional(10.5), fg);
}

impl RandomizerDlg {
    pub fn new(catalog: &[ad_modules::ModuleCatalogEntry], pool: &[String], favorites: &[String]) -> Self {
        let items: Vec<_> = catalog
            .iter()
            .filter(|e| e.meta.id != "randomizer")
            .map(|e| (e.meta.id.clone(), e.meta.name.clone(), Collection::of(&e.meta.id)))
            .collect();
        let checked = items.iter().map(|(id, _, _)| pool.contains(id)).collect();
        Self { items, checked, favorites: favorites.to_vec() }
    }

    pub fn pool(&self) -> Vec<String> {
        self.items.iter().zip(&self.checked).filter(|(_, c)| **c).map(|((id, _, _), _)| id.clone()).collect()
    }

    fn apply(&mut self, preset: Preset) {
        for ((id, _, coll), checked) in self.items.iter().zip(self.checked.iter_mut()) {
            *checked = match preset {
                Preset::All => true,
                Preset::Only(c) => *coll == c,
                Preset::Cartoons => matches!(coll, Collection::Simpsons | Collection::LooneyTunes),
                Preset::Favorites => self.favorites.contains(id),
                Preset::Clear => false,
            };
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let total = self.items.len();
        let (out, dismissed) = modal(ctx, look, "randomizer-dlg", "Randomizer — Choose Modules & Pack Presets", 440.0, |ui| {
            ui.label("Quick Pack Presets (or tick individual modules below):");
            let all_label = format!("All ({total})");
            let presets: [[(&str, Preset); 4]; 2] = [
                [
                    (all_label.as_str(), Preset::All),
                    ("Classic 1991", Preset::Only(Collection::Classic)),
                    ("Simpsons", Preset::Only(Collection::Simpsons)),
                    ("Star Trek", Preset::Only(Collection::StarTrek)),
                ],
                [
                    ("Looney Tunes", Preset::Only(Collection::LooneyTunes)),
                    ("All Cartoons", Preset::Cartoons),
                    ("★ Favorites", Preset::Favorites),
                    ("Clear All", Preset::Clear),
                ],
            ];
            let bw = (ui.available_width() - 3.0 * 6.0) / 4.0;
            for row in presets {
                ui.horizontal(|ui| {
                    for (label, preset) in row {
                        if theme::button(ui, look, label, vec2(bw, 24.0)).clicked() {
                            self.apply(preset);
                        }
                    }
                });
            }
            ui.add_space(4.0);
            let list_frame = list_frame(look);
            list_frame.show(ui, |ui| {
                ScrollArea::vertical().id_salt("rnd-list").max_height(280.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (i, (_, name, coll)) in self.items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.checked[i], "");
                            badge(ui, *coll, look, 78.0);
                            if ui.add(egui::Label::new(name.as_str()).sense(Sense::click())).clicked() {
                                self.checked[i] = !self.checked[i];
                            }
                        });
                    }
                });
            });
            let n = self.checked.iter().filter(|c| **c).count();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{n} of {total} modules in the Randomizer pool")).color(look.palette().muted));
            });
            ok_cancel(ui, look, "OK")
        });
        if dismissed { Outcome::Cancel } else { out }
    }
}

/// Sunken white list box (retro) / dark inset (modern).
pub fn list_frame(look: Look) -> Frame {
    let pal = look.palette();
    match look {
        Look::Retro => Frame::new().fill(pal.field_bg).stroke(Stroke::new(1.0, Color32::BLACK)).inner_margin(Margin::same(3)),
        Look::Modern => Frame::new()
            .fill(pal.field_bg)
            .stroke(Stroke::new(1.0, Color32::from_rgb(64, 64, 68)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(4)),
    }
}

// ============================================================================================
// Messages "Edit…" (MESSAGES.ADS)

const MSG_FONT_CHOICES: [&str; 6] = ["Helv", "Arial", "Times New Roman", "Courier New", "Segoe UI", "Segoe Script"];
const MSG_COLORS: [&str; 7] = ["White", "Red", "Green", "Blue", "Cyan", "Magenta", "Yellow"];

pub struct MessagesDlg {
    records: Vec<MessageRecord>,
    cur: usize,
}

impl MessagesDlg {
    pub fn new(data_dir: &str) -> Self {
        let mut records = load_messages_ads(data_dir);
        records.resize_with(records.len().max(MESSAGE_SLOT_COUNT), MessageRecord::default);
        let cur = records.iter().position(|r| r.selected).unwrap_or(0);
        Self { records, cur }
    }

    /// Write `MESSAGES.ADS` (at least one message stays selected, like the original).
    pub fn save(&mut self, data_dir: &str) -> std::io::Result<()> {
        if !self.records.iter().any(|r| r.selected)
            && let Some(first) = self.records.first_mut()
        {
            first.selected = true;
        }
        std::fs::write(Path::new(data_dir).join("MESSAGES.ADS"), serialize_messages_ads(&self.records))
    }

    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let (out, dismissed) = modal(ctx, look, "messages-dlg", "Messages — Edit MESSAGES.ADS", 470.0, |ui| {
            let count = self.records.len();
            ui.horizontal(|ui| {
                ui.label("Message Slot:");
                let slots: Vec<(usize, String)> = (0..count).map(|i| (i, format!("Message {}", i + 1))).collect();
                let choices: Vec<(usize, &str)> = slots.iter().map(|(i, s)| (*i, s.as_str())).collect();
                combo(ui, "msg-slot", 130.0, &mut self.cur, &choices);
                ui.add_space(10.0);
                let rec = &mut self.records[self.cur];
                ui.checkbox(&mut rec.selected, "Include in Rotation");
            });
            let rec = &mut self.records[self.cur];
            ui.add_space(4.0);
            ui.label("Message Text:");
            ui.add(theme::text_edit(
                look,
                TextEdit::multiline(&mut rec.text)
                    .id_salt(("msg-text", self.cur))
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            ));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                fixed_label(ui, 44.0, "Font:");
                let mut face = MSG_FONT_CHOICES
                    .iter()
                    .position(|f| f.eq_ignore_ascii_case(rec.font_face.trim()))
                    .unwrap_or(0);
                let choices: Vec<(usize, &str)> = MSG_FONT_CHOICES.iter().copied().enumerate().collect();
                if combo(ui, "msg-font", 160.0, &mut face, &choices) {
                    rec.font_face = MSG_FONT_CHOICES[face].to_string();
                }
                ui.add_space(12.0);
                ui.label("Size (pt):");
                ui.add(egui::DragValue::new(&mut rec.point_size).range(8..=120));
            });
            ui.horizontal(|ui| {
                fixed_label(ui, 44.0, "Align:");
                let mut align = rec.align.min(2);
                if combo(ui, "msg-align", 110.0, &mut align, &[(0, "Left"), (1, "Center"), (2, "Right")]) {
                    rec.align = align;
                }
                ui.add_space(12.0);
                ui.label("Color:");
                let mut color = rec.color_idx.min(6);
                let choices: Vec<(u16, &str)> = MSG_COLORS.iter().enumerate().map(|(i, c)| (i as u16, *c)).collect();
                if combo(ui, "msg-color", 110.0, &mut color, &choices) {
                    rec.color_idx = color;
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(50.0);
                ui.checkbox(&mut rec.bold, "Bold");
                ui.checkbox(&mut rec.italic, "Italic");
                ui.checkbox(&mut rec.underline, "Underline");
            });
            ok_cancel(ui, look, "Save")
        });
        if out == Outcome::Accept {
            for rec in &mut self.records {
                rec.point_size = rec.point_size.clamp(8, 120);
            }
        }
        if dismissed { Outcome::Cancel } else { out }
    }
}

// ============================================================================================
// GraphStat "Edit…" (GRAFSTAT.TXT)

pub struct GraphStatDlg {
    pub text: String,
}

impl GraphStatDlg {
    pub fn new(data_dir: &str) -> Self {
        Self { text: GraphStatModule::load_grafstat_text(data_dir) }
    }

    pub fn save(&self, data_dir: &str) -> std::io::Result<()> {
        std::fs::write(Path::new(data_dir).join("GRAFSTAT.TXT"), &self.text)
    }

    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let (out, dismissed) = modal(ctx, look, "grafstat-dlg", "GraphStat — Edit GRAFSTAT.TXT", 470.0, |ui| {
            ui.label("4 label sections separated by blank lines (Titles, Adjectives, Subjects, Units):");
            list_frame(look).show(ui, |ui| {
                ScrollArea::both().id_salt("graf-scroll").max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut self.text)
                            .id_salt("graf-text")
                            .font(egui::TextStyle::Monospace)
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18),
                    );
                });
            });
            let mut out = Outcome::Open;
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::button(ui, look, "Reset Default", vec2(104.0, 26.0)).clicked() {
                    self.text = GraphStatModule::load_grafstat_text("");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::button(ui, look, "Cancel", vec2(84.0, 26.0)).clicked() {
                        out = Outcome::Cancel;
                    }
                    if theme::button(ui, look, "Save", vec2(84.0, 26.0)).clicked() {
                        out = Outcome::Accept;
                    }
                });
            });
            out
        });
        if dismissed { Outcome::Cancel } else { out }
    }
}

// ============================================================================================
// Message box (module About / errors)

pub struct MessageBox {
    pub title: String,
    pub body: String,
}

impl MessageBox {
    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let (out, dismissed) = modal(ctx, look, "message-box", &self.title, 400.0, |ui| {
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                let pal = look.palette();
                ui.painter().circle_filled(r.center(), 15.0, if look.is_retro() { Color32::from_rgb(0, 0, 255) } else { pal.accent });
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::new(20.0, look.bold()), Color32::WHITE);
                ui.add_space(8.0);
                let w = ui.available_width();
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        ui.add(egui::Label::new(self.body.replace("\r\n", "\n")).wrap());
                    });
                });
            });
            ui.add_space(6.0);
            let mut out = Outcome::Open;
            ui.vertical_centered(|ui| {
                if theme::button(ui, look, "OK", vec2(84.0, 26.0)).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    out = Outcome::Accept;
                }
            });
            out
        });
        if dismissed { Outcome::Cancel } else { out }
    }
}

// ============================================================================================
// Path field fallback (no zenity / kdialog)

pub struct PathPrompt {
    pub target: PickTarget,
    pub text: String,
    /// A chooser exists but failed (vs. none installed).
    pub chooser_failed: bool,
}

impl PathPrompt {
    pub fn show(&mut self, ctx: &egui::Context, look: Look) -> Outcome {
        let title = self.target.title();
        let (out, dismissed) = modal(ctx, look, "path-prompt", title, 470.0, |ui| {
            ui.label(if self.target.is_dir() {
                "Folder containing .BMP pictures:"
            } else {
                "Path of a Windows bitmap (.BMP) file:"
            });
            let r = ui.add(theme::text_edit(look, TextEdit::singleline(&mut self.text).desired_width(f32::INFINITY)));
            let p = Path::new(self.text.trim());
            let valid = if self.target.is_dir() { p.is_dir() } else { p.is_file() };
            if !self.text.trim().is_empty() && !valid {
                ui.label(RichText::new(if self.target.is_dir() { "No such folder." } else { "No such file." }).color(look.palette().off));
            }
            ui.label(
                RichText::new(if self.chooser_failed {
                    "The file chooser could not be opened; type the path instead."
                } else {
                    "Install zenity or kdialog for a graphical file chooser."
                })
                .small()
                .color(look.palette().muted),
            );
            let mut out = ok_cancel(ui, look, "OK");
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                out = Outcome::Accept;
            }
            if out == Outcome::Accept && !valid && !self.text.trim().is_empty() {
                out = Outcome::Open;
            }
            out
        });
        if dismissed { Outcome::Cancel } else { out }
    }
}

// ============================================================================================
// About After Dark

pub fn about_body(daemon: bool) -> String {
    let p = crate::common::paths();
    format!(
        "After Dark for Linux {}\n\nA Rust port of Berkeley Systems' After Dark 1.0a (1991) screen saver, \
         with the Classic, Simpsons, Star Trek: TNG and Looney Tunes collections.\n\n\
         Settings: {}\nPlug-in modules: {}\nIdle monitor (after-dark daemon): {}\n\n\
         Tip: hold Ctrl while starting the Control Panel to start After Dark switched Off.",
        env!("CARGO_PKG_VERSION"),
        p.config_dir.display(),
        p.plugin_dir.display(),
        if daemon { "running" } else { "not running" },
    )
}
