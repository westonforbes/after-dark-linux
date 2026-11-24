//! The two Control Panel looks: "Retro Windows 3.1" (grey, bevelled, the default) and "Modern"
//! (dark cards), plus the bevel/platform painting the retro look needs.

use std::sync::Arc;

use ad_sdk::prefs::UiTheme;
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response,
    Sense, Stroke, StrokeKind, TextStyle, TextWrapMode, Ui, Vec2, WidgetText, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Retro,
    Modern,
}

impl Look {
    pub fn of(theme: UiTheme) -> Self {
        match theme {
            UiTheme::RetroWin31 => Self::Retro,
            UiTheme::ModernWin11 => Self::Modern,
        }
    }

    pub fn is_retro(self) -> bool {
        self == Self::Retro
    }

    pub fn palette(self) -> Palette {
        match self {
            Look::Retro => Palette {
                bg: Color32::from_rgb(192, 192, 192),
                card: Color32::from_rgb(192, 192, 192),
                text: Color32::BLACK,
                muted: Color32::from_rgb(48, 48, 48),
                accent: Color32::from_rgb(0, 0, 128),
                off: Color32::from_rgb(220, 60, 60),
                good: Color32::from_rgb(0, 128, 0),
                logo_bg: Color32::from_rgb(0, 0, 128),
                logo_border: Color32::BLACK,
                field_bg: Color32::WHITE,
                title_bg: Color32::from_rgb(0, 0, 128),
                title_fg: Color32::WHITE,
            },
            Look::Modern => Palette {
                bg: Color32::from_rgb(28, 28, 30),
                card: Color32::from_rgb(42, 42, 46),
                text: Color32::from_rgb(242, 242, 245),
                muted: Color32::from_rgb(172, 178, 190),
                accent: Color32::from_rgb(96, 205, 255),
                off: Color32::from_rgb(220, 60, 60),
                good: Color32::from_rgb(108, 203, 95),
                logo_bg: Color32::from_rgb(0, 0, 128),
                logo_border: Color32::from_rgb(64, 88, 130),
                field_bg: Color32::from_rgb(32, 32, 36),
                title_bg: Color32::from_rgb(42, 42, 46),
                title_fg: Color32::from_rgb(242, 242, 245),
            },
        }
    }

    /// Font family for bold labels.
    pub fn bold(self) -> FontFamily {
        FontFamily::Name(match self {
            Look::Retro => "retro-bold".into(),
            Look::Modern => "modern-bold".into(),
        })
    }

    fn regular(self) -> FontFamily {
        FontFamily::Name(match self {
            Look::Retro => "retro".into(),
            Look::Modern => "modern".into(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub off: Color32,
    pub good: Color32,
    pub logo_bg: Color32,
    pub logo_border: Color32,
    pub field_bg: Color32,
    pub title_bg: Color32,
    pub title_fg: Color32,
}

const GREY_HI: Color32 = Color32::WHITE;
const GREY_SH: Color32 = Color32::from_rgb(128, 128, 128);

// ---------------------------------------------------------------------------------------------
// Fonts

/// System fonts that approximate the originals (MS Sans Serif / Segoe UI). All optional: any that
/// are missing fall back to egui's built-in font.
const RETRO_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];
const RETRO_BOLD_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
];
const MODERN_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
];
const MODERN_BOLD_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
];

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, candidates) in [
        ("retro", RETRO_FONTS),
        ("retro-bold", RETRO_BOLD_FONTS),
        ("modern", MODERN_FONTS),
        ("modern-bold", MODERN_BOLD_FONTS),
    ] {
        let mut list = Vec::new();
        if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            defs.font_data.insert(family.to_string(), Arc::new(FontData::from_owned(bytes)));
            list.push(family.to_string());
        }
        // DejaVu covers symbols (★ ● …) that Noto Sans and egui's default font lack.
        if family.starts_with("modern") && defs.font_data.contains_key("retro") {
            list.push("retro".to_string());
        }
        list.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(defs);
}

// ---------------------------------------------------------------------------------------------
// Styles

/// Install the style for `look` and make it the active one.
pub fn apply(ctx: &egui::Context, look: Look) {
    let theme = match look {
        Look::Retro => egui::Theme::Light,
        Look::Modern => egui::Theme::Dark,
    };
    let mut style = (*ctx.style_of(theme)).clone();
    let pal = look.palette();
    let fam = look.regular();
    let size = match look {
        Look::Retro => 12.5,
        Look::Modern => 13.5,
    };
    style.text_styles = [
        (TextStyle::Small, FontId::new(size - 2.5, fam.clone())),
        (TextStyle::Body, FontId::new(size, fam.clone())),
        (TextStyle::Button, FontId::new(size, fam.clone())),
        (TextStyle::Heading, FontId::new(size + 4.0, look.bold())),
        (TextStyle::Monospace, FontId::new(size, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.button_padding = vec2(8.0, 3.0);
    style.spacing.item_spacing = vec2(6.0, 5.0);
    style.spacing.interact_size.y = 22.0;

    let v = &mut style.visuals;
    v.panel_fill = pal.bg;
    v.window_fill = pal.card;
    v.extreme_bg_color = pal.field_bg;
    v.text_edit_bg_color = Some(pal.field_bg);
    v.slider_trailing_fill = !look.is_retro();
    match look {
        Look::Retro => {
            let flat = CornerRadius::ZERO;
            v.window_corner_radius = flat;
            v.menu_corner_radius = flat;
            v.window_stroke = Stroke::new(1.0, Color32::BLACK);
            v.window_shadow = egui::Shadow { offset: [3, 3], blur: 0, spread: 0, color: Color32::from_black_alpha(110) };
            v.popup_shadow = v.window_shadow;
            v.faint_bg_color = Color32::from_rgb(208, 208, 208);
            v.selection.bg_fill = pal.accent;
            v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            v.hyperlink_color = pal.accent;
            let w = &mut v.widgets;
            for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                wv.corner_radius = flat;
                wv.expansion = 0.0;
                wv.fg_stroke = Stroke::new(1.0, Color32::BLACK);
                wv.bg_stroke = Stroke::new(1.0, Color32::BLACK);
                wv.bg_fill = Color32::WHITE;
                wv.weak_bg_fill = Color32::from_rgb(192, 192, 192);
            }
            w.noninteractive.bg_stroke = Stroke::new(1.0, GREY_SH);
            w.noninteractive.bg_fill = pal.bg;
            w.hovered.weak_bg_fill = Color32::from_rgb(208, 208, 208);
            w.active.weak_bg_fill = Color32::from_rgb(176, 176, 176);
            w.open.weak_bg_fill = Color32::from_rgb(208, 208, 208);
            // Checkbox interiors stay white; pressed ones go grey like Win 3.1.
            w.active.bg_fill = Color32::from_rgb(192, 192, 192);
        }
        Look::Modern => {
            v.window_corner_radius = CornerRadius::same(8);
            v.window_stroke = Stroke::new(1.0, Color32::from_rgb(64, 64, 68));
            v.faint_bg_color = Color32::from_rgb(36, 36, 40);
            v.selection.bg_fill = Color32::from_rgb(0, 95, 140);
            v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            v.hyperlink_color = pal.accent;
            let w = &mut v.widgets;
            for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                wv.corner_radius = CornerRadius::same(4);
            }
            w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 64, 68));
            w.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            w.inactive.fg_stroke = Stroke::new(1.0, pal.text);
            w.inactive.weak_bg_fill = Color32::from_rgb(58, 58, 64);
            w.inactive.bg_fill = Color32::from_rgb(58, 58, 64);
            w.hovered.weak_bg_fill = Color32::from_rgb(70, 70, 78);
            w.hovered.bg_stroke = Stroke::new(1.0, pal.accent);
            w.active.weak_bg_fill = Color32::from_rgb(0, 95, 140);
        }
    }
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}

// ---------------------------------------------------------------------------------------------
// Painting

/// Win 3.1 style 3-D edge: `raised` = white top/left, grey bottom/right.
pub fn bevel(painter: &egui::Painter, rect: Rect, raised: bool, width: f32) {
    let (tl, br) = if raised { (GREY_HI, GREY_SH) } else { (GREY_SH, GREY_HI) };
    for i in 0..width as i32 {
        let r = rect.shrink(i as f32 + 0.5);
        painter.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, tl));
        painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, tl));
        painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, br));
        painter.line_segment([r.right_bottom(), r.left_bottom()], Stroke::new(1.0, br));
    }
}

/// The 1991 `PLATFORMWNDPROC` panel (black frame + bevel) or a modern dark card.
pub fn platform(painter: &egui::Painter, rect: Rect, look: Look) {
    let pal = look.palette();
    match look {
        Look::Retro => {
            painter.rect_filled(rect, 0.0, pal.card);
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
            bevel(painter, rect.shrink(1.0), true, 1.0);
        }
        Look::Modern => {
            painter.rect_filled(rect, 8.0, pal.card);
            painter.rect_stroke(rect, 8.0, Stroke::new(1.0, Color32::from_rgb(64, 64, 68)), StrokeKind::Inside);
        }
    }
}

/// A Control Panel push button: a chunky bevelled Win 3.1 button in the retro look, a regular
/// egui button otherwise.
pub fn button(ui: &mut Ui, look: Look, text: impl Into<WidgetText>, min_size: Vec2) -> Response {
    if !look.is_retro() {
        return ui.add(egui::Button::new(text).min_size(min_size));
    }
    let galley = text.into().into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
    let size = (galley.size() + vec2(16.0, 8.0)).max(min_size);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let pressed = resp.is_pointer_button_down_on();
        p.rect_filled(rect, 0.0, Color32::from_rgb(192, 192, 192));
        // Rounded-corner black outline like the Win 3.x button frame.
        let r = rect.shrink(0.5);
        let black = Stroke::new(1.0, Color32::BLACK);
        p.line_segment([r.left_top() + vec2(1.0, 0.0), r.right_top() - vec2(1.0, 0.0)], black);
        p.line_segment([r.left_bottom() + vec2(1.0, 0.0), r.right_bottom() - vec2(1.0, 0.0)], black);
        p.line_segment([r.left_top() + vec2(0.0, 1.0), r.left_bottom() - vec2(0.0, 1.0)], black);
        p.line_segment([r.right_top() + vec2(0.0, 1.0), r.right_bottom() - vec2(0.0, 1.0)], black);
        let inner = rect.shrink(1.0);
        if pressed {
            bevel(p, inner, false, 1.0);
        } else {
            bevel(p, inner, true, 2.0);
        }
        let off = if pressed { vec2(1.0, 1.0) } else { Vec2::ZERO };
        let pos = rect.center() - galley.size() / 2.0 + off;
        let color = if ui.is_enabled() { Color32::BLACK } else { GREY_SH };
        if resp.has_focus() {
            let fr = Rect::from_min_size(pos, galley.size()).expand2(vec2(3.0, 1.0));
            dotted_rect(p, fr);
        }
        p.galley(Pos2::new(pos.x.round(), pos.y.round()), galley, color);
    }
    resp
}

fn dotted_rect(p: &egui::Painter, r: Rect) {
    let s = Stroke::new(1.0, Color32::BLACK);
    let mut x = r.left();
    while x < r.right() {
        p.line_segment([Pos2::new(x, r.top()), Pos2::new(x + 1.0, r.top())], s);
        p.line_segment([Pos2::new(x, r.bottom()), Pos2::new(x + 1.0, r.bottom())], s);
        x += 2.0;
    }
    let mut y = r.top();
    while y < r.bottom() {
        p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.left(), y + 1.0)], s);
        p.line_segment([Pos2::new(r.right(), y), Pos2::new(r.right(), y + 1.0)], s);
        y += 2.0;
    }
}

/// Bold label text in the current look.
pub fn bold(look: Look, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(look.bold())
}

/// A 0..=100 slider: a Win 3.1 style trackbar (sunken groove, raised thumb, ticks) in the retro
/// look, an egui slider otherwise. `changed()` is set when the value moves.
pub fn slider(ui: &mut Ui, look: Look, value: &mut i32, width: f32) -> Response {
    if !look.is_retro() {
        ui.spacing_mut().slider_width = width;
        return ui.add(egui::Slider::new(value, 0..=100).show_value(false).step_by(1.0));
    }
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click_and_drag());
    let thumb_w = 11.0;
    let groove = Rect::from_min_max(
        Pos2::new(rect.left() + thumb_w / 2.0, rect.top() + 7.0),
        Pos2::new(rect.right() - thumb_w / 2.0, rect.top() + 12.0),
    );
    let old = *value;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((pos.x - groove.left()) / groove.width()).clamp(0.0, 1.0);
        *value = (t * 100.0).round() as i32;
    }
    if resp.has_focus() {
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                *value -= 1;
            }
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                *value += 1;
            }
            if i.key_pressed(egui::Key::PageDown) {
                *value -= 10;
            }
            if i.key_pressed(egui::Key::PageUp) {
                *value += 10;
            }
        });
        *value = (*value).clamp(0, 100);
    }
    if *value != old {
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(groove, 0.0, Color32::WHITE);
        bevel(p, groove.expand(1.0), false, 1.0);
        // Ticks every 10%.
        for i in 0..=10 {
            let x = (groove.left() + groove.width() * i as f32 / 10.0).round() + 0.5;
            p.line_segment([Pos2::new(x, rect.bottom() - 5.0), Pos2::new(x, rect.bottom() - 1.0)], Stroke::new(1.0, Color32::BLACK));
        }
        let cx = groove.left() + groove.width() * (*value).clamp(0, 100) as f32 / 100.0;
        let thumb = Rect::from_center_size(Pos2::new(cx.round(), rect.top() + 9.5), vec2(thumb_w, 18.0));
        p.rect_filled(thumb, 0.0, Color32::from_rgb(192, 192, 192));
        p.rect_stroke(thumb, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
        bevel(p, thumb.shrink(1.0), true, 1.0);
        if resp.has_focus() {
            dotted_rect(p, rect.shrink(0.5));
        }
    }
    resp
}

/// A single/multi-line text field in the current look (retro: white box with a black border,
/// which egui would otherwise draw in the white selection colour while focused).
pub fn text_edit<'t>(look: Look, edit: egui::TextEdit<'t>) -> egui::TextEdit<'t> {
    match look {
        Look::Retro => edit.frame(
            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, Color32::BLACK))
                .inner_margin(egui::Margin::symmetric(5, 3)),
        ),
        Look::Modern => edit,
    }
}
