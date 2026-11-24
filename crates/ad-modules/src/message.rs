//! 1:1 Decompilation of `MESSAGE.AD` ("Messages") from After Dark 1.0a (1991).
//!
//! Disassembly-backed from `docs/disasm/message_seg1.asm` and `docs/disasm/message_data.hex`.
//! Reads and writes exact 1991 `MESSAGES.ADS` binary files (8 × 246-byte records = 1968 bytes).

use std::path::{Path, PathBuf};

use ad_sdk::canvas::RECT;

use ad_sdk::canvas::{AdSurface, ROP_SRCCOPY};
use ad_sdk::palette::rgb;
use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult};

const DEFAULT_MESSAGES_ADS: &[u8] = include_bytes!("../../../assets/MESSAGES.ADS");

pub const MESSAGE_RECORD_SIZE: usize = 0xF6; // 246 bytes
pub const MESSAGE_SLOT_COUNT: usize = 8;

/// Exact 1991 `MESSAGES.ADS` slot record (`0xF6` = 246 bytes each, 8 slots = 1968 bytes).
#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub italic: bool,
    pub underline: bool,
    pub bold: bool,
    pub point_size: i32,
    pub font_face: String,
    /// `0` = Left, `1` = Center, `2` = Right.
    pub align: u16,
    /// `0` = White, `1` = Red, `2` = Green, `3` = Blue, `4` = Cyan, `5` = Magenta, `6` = Yellow.
    pub color_idx: u16,
    pub selected: bool,
    pub text: String,
}

impl Default for MessageRecord {
    fn default() -> Self {
        Self {
            italic: false,
            underline: false,
            bold: false,
            point_size: 24,
            font_face: "Helv".to_string(),
            align: 1,
            color_idx: 0,
            selected: false,
            text: "After Dark".to_string(),
        }
    }
}

impl MessageRecord {
    /// Exact 7-color table from `MESSAGE.AD` (`1:040B..1:043A`).
    pub fn colorref(&self) -> u32 {
        match self.color_idx {
            1 => rgb(255, 0, 0),     // Red
            2 => rgb(0, 255, 0),     // Green
            3 => rgb(0, 0, 255),     // Blue
            4 => rgb(0, 255, 255),   // Cyan
            5 => rgb(255, 0, 255),   // Magenta
            6 => rgb(255, 255, 0),   // Yellow
            _ => rgb(255, 255, 255), // White (0 & default)
        }
    }

    /// Map legacy Windows 3.0 bitmap font names to crisp Windows 11 equivalents.
    pub fn resolved_font_face(&self) -> &str {
        match self.font_face.trim() {
            "" | "Helv" => "Arial",
            "Tms Rmn" | "Roman" => "Times New Roman",
            "DIGITAL" => "Courier New",
            "Script" => "Segoe Script",
            "System" => "Segoe UI",
            other => other,
        }
    }
}

/// Parse an exact 1991 `MESSAGES.ADS` buffer (1968 bytes = 8 × 246-byte records).
pub fn parse_messages_ads(bytes: &[u8]) -> Vec<MessageRecord> {
    let mut records = Vec::with_capacity(MESSAGE_SLOT_COUNT);
    for i in 0..MESSAGE_SLOT_COUNT {
        let off = i * MESSAGE_RECORD_SIZE;
        if off + MESSAGE_RECORD_SIZE > bytes.len() {
            records.push(MessageRecord::default());
            continue;
        }
        let rec = &bytes[off..off + MESSAGE_RECORD_SIZE];
        let italic = u16::from_le_bytes([rec[0], rec[1]]) != 0;
        let underline = u16::from_le_bytes([rec[2], rec[3]]) != 0;
        let bold = u16::from_le_bytes([rec[4], rec[5]]) != 0;
        let point_size = u16::from_le_bytes([rec[6], rec[7]]) as i32;
        let face_raw = &rec[8..40];
        let face_end = face_raw.iter().position(|&b| b == 0).unwrap_or(face_raw.len());
        let font_face = String::from_utf8_lossy(&face_raw[..face_end]).to_string();
        let align = u16::from_le_bytes([rec[40], rec[41]]);
        let color_idx = u16::from_le_bytes([rec[42], rec[43]]);
        let selected = u16::from_le_bytes([rec[44], rec[45]]) != 0;
        let text_raw = &rec[46..MESSAGE_RECORD_SIZE];
        let text_end = text_raw.iter().position(|&b| b == 0).unwrap_or(text_raw.len());
        let text = String::from_utf8_lossy(&text_raw[..text_end]).to_string();
        records.push(MessageRecord {
            italic,
            underline,
            bold,
            point_size: point_size.clamp(8, 120),
            font_face,
            align: align.min(2),
            color_idx: color_idx.min(6),
            selected,
            text,
        });
    }
    if !records.iter().any(|r| r.selected) {
        if let Some(first) = records.first_mut() {
            first.selected = true;
        }
    }
    records
}

/// Serialize 8 `MessageRecord` slots back into exact 1991 `MESSAGES.ADS` binary format (1968 bytes).
pub fn serialize_messages_ads(records: &[MessageRecord]) -> Vec<u8> {
    let mut out = vec![0u8; MESSAGE_SLOT_COUNT * MESSAGE_RECORD_SIZE];
    for i in 0..MESSAGE_SLOT_COUNT {
        let rec = records.get(i).cloned().unwrap_or_default();
        let dst = &mut out[i * MESSAGE_RECORD_SIZE..(i + 1) * MESSAGE_RECORD_SIZE];
        dst[0..2].copy_from_slice(&(u16::from(rec.italic)).to_le_bytes());
        dst[2..4].copy_from_slice(&(u16::from(rec.underline)).to_le_bytes());
        dst[4..6].copy_from_slice(&(u16::from(rec.bold)).to_le_bytes());
        dst[6..8].copy_from_slice(&(rec.point_size.clamp(8, 120) as u16).to_le_bytes());
        let face_bytes = rec.font_face.as_bytes();
        let face_len = face_bytes.len().min(31);
        dst[8..8 + face_len].copy_from_slice(&face_bytes[..face_len]);
        dst[40..42].copy_from_slice(&(rec.align.min(2)).to_le_bytes());
        dst[42..44].copy_from_slice(&(rec.color_idx.min(6)).to_le_bytes());
        dst[44..46].copy_from_slice(&(u16::from(rec.selected)).to_le_bytes());
        let text_bytes = rec.text.as_bytes();
        let text_len = text_bytes.len().min(MESSAGE_RECORD_SIZE - 47);
        dst[46..46 + text_len].copy_from_slice(&text_bytes[..text_len]);
    }
    out
}

/// Load `MESSAGES.ADS` from `data_dir` (or fall back to the authentic embedded 1991 file).
pub fn load_messages_ads(data_dir: &str) -> Vec<MessageRecord> {
    if !data_dir.is_empty() {
        let p = Path::new(data_dir).join("MESSAGES.ADS");
        if let Ok(bytes) = std::fs::read(&p) {
            if bytes.len() >= MESSAGE_RECORD_SIZE {
                return parse_messages_ads(&bytes);
            }
        }
    }
    parse_messages_ads(DEFAULT_MESSAGES_ADS)
}

pub struct MessagesModule {
    msg_surf: Option<AdSurface>,
    box_w: i32,
    box_h: i32,
    text_w: i32,
    text_h: i32,
    move_mode: i32,
    speed_step: i32,
    // Floating state (`1:0490`)
    float_x: i32,
    float_y: i32,
    dx_dir: i32,
    dy_dir: i32,
    // Popping state (`1:0516`)
    pop_x: i32,
    pop_y: i32,
    pop_timer: i32,
    // Sliding state (`1:05CA`)
    slide_x: i32,
    slide_y: i32,
    last_step_ms: u64,
    first_step: bool,
}

impl MessagesModule {
    pub fn new() -> Self {
        Self {
            msg_surf: None,
            box_w: 0,
            box_h: 0,
            text_w: 0,
            text_h: 0,
            move_mode: 1,
            speed_step: 4,
            float_x: 40,
            float_y: 40,
            dx_dir: 1,
            dy_dir: 1,
            pop_x: 0,
            pop_y: 0,
            pop_timer: 0,
            slide_x: -10_000,
            slide_y: 0,
            last_step_ms: 0,
            first_step: true,
        }
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "message".to_string(),
            filename: "MESSAGE.AD".to_string(),
            name: "Messages".to_string(),
            credit: "Messages by Tracy Elmore.\n©1990 Berkeley Systems Inc.".to_string(),
            about: "MESSAGES(tm) allows you to create and display any one of eight different screen saving messages. To edit the message press the \"Edit..\" button and type in any message you choose. You can format, change the color, font, size and style of the message.\n\n\"Popping\" causes the message to pop up randomly on the screen.\n\n\"Floating\" causes the message to float about the screen.\n\n\"Sliding\" causes the message to scroll across the screen from right to left.\n\nOriginal concept by Bruce Burkhalter.".to_string(),
            controls: [
                AdControl::Menu {
                    title: "Move:".to_string(),
                    default_index: 1,
                    choices: vec![
                        "Popping".to_string(),
                        "Floating".to_string(),
                        "Sliding".to_string(),
                    ],
                },
                AdControl::LabeledSlider {
                    title: "Speed:".to_string(),
                    default_raw: 56,
                    labels: vec![
                        "Sluggish".to_string(),
                        "Slow".to_string(),
                        "Medium".to_string(),
                        "Moving".to_string(),
                        "Quick".to_string(),
                        "Flying".to_string(),
                    ],
                    thresholds: vec![15, 30, 45, 60, 75, 90],
                },
                AdControl::Button {
                    title: "Edit...".to_string(),
                },
                AdControl::None,
            ],
            target_frame_ms: 33,
        }
    }

    fn render_message_bitmap(&mut self, ctx: &AdContext<'_>) {
        let records = load_messages_ads(&ctx.system.data_dir);
        let active = records
            .iter()
            .find(|r| r.selected && !r.text.trim().is_empty())
            .or_else(|| records.iter().find(|r| !r.text.trim().is_empty()))
            .cloned()
            .unwrap_or_default();

        let move_mode = ctx.module.control_value[0];
        let speed_val = ctx.module.control_value[1];
        self.move_mode = move_mode;
        self.speed_step = (speed_val / 15) + 1;

        let face = active.resolved_font_face();
        // Scale font height proportionally for modern HD/4K screens (1:1 at 480p, crisp & legible at 1080p/4K!)
        let sf = (ctx.module.rgn_height as f32 / 360.0).clamp(0.85, 4.5);
        let font_h = ((active.point_size.clamp(12, 96) as f32) * sf).round().clamp(12.0, 240.0) as i32;
        let color = active.colorref();

        if move_mode == 2 {
            // Sliding: flatten newlines to spaces (`1:0000..1:0052`)
            let single_line: String = active
                .text
                .chars()
                .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
                .collect();
            let (tw, th) = ctx
                .surface
                .measure_text_font(face, font_h, active.bold, active.italic, &single_line);
            let tw = tw.max(16);
            let th = th.max(font_h);
            let avg_char = (tw / single_line.len().max(1) as i32).max(8);
            let bmp_w = tw + avg_char + (speed_val / 10) + 16;
            let bmp_h = th + 8;

            if let Some(mut surf) = AdSurface::new(bmp_w, bmp_h) {
                surf.fill_black();
                let rc = RECT {
                    left: 0,
                    top: 0,
                    right: bmp_w,
                    bottom: bmp_h,
                };
                surf.draw_text_font(
                    face,
                    font_h,
                    active.bold,
                    active.italic,
                    active.underline,
                    color,
                    rc,
                    false,
                    false,
                    &single_line,
                );
                self.text_w = tw;
                self.text_h = bmp_h;
                self.box_w = bmp_w;
                self.box_h = bmp_h;
                self.msg_surf = Some(surf);
            }
        } else {
            // Popping (0) or Floating (1): multiline wrapped box with 7px black border (`1:0053..1:00CC`)
            let lines: Vec<&str> = active
                .text
                .split('\n')
                .map(|l| l.trim_end_matches('\r'))
                .collect();
            let mut max_line_w = 16;
            let mut line_h = font_h;
            for line in &lines {
                let (lw, lh) = ctx
                    .surface
                    .measure_text_font(face, font_h, active.bold, active.italic, line);
                max_line_w = max_line_w.max(lw);
                line_h = line_h.max(lh);
            }
            let max_box_w = (ctx.module.rgn_width - 16).max(64);
            let inner_w = (max_line_w + 16).min(max_box_w - 14).max(32);
            let wrapped_lines = lines
                .iter()
                .map(|l| {
                    let (lw, _) = ctx
                        .surface
                        .measure_text_font(face, font_h, active.bold, active.italic, l);
                    ((lw + inner_w - 1) / inner_w).max(1)
                })
                .sum::<i32>()
                .max(1);
            let bmp_w = (inner_w + 14).min(ctx.module.rgn_width.max(32));
            let bmp_h = (wrapped_lines * line_h + self.speed_step + 18)
                .min(ctx.module.rgn_height.max(32));

            if let Some(mut surf) = AdSurface::new(bmp_w, bmp_h) {
                surf.fill_black();
                let rc = RECT {
                    left: 7,
                    top: 7,
                    right: bmp_w - 7,
                    bottom: bmp_h - 7,
                };
                surf.draw_text_font(
                    face,
                    font_h,
                    active.bold,
                    active.italic,
                    active.underline,
                    color,
                    rc,
                    active.align == 1,
                    true,
                    &active.text,
                );
                self.text_w = bmp_w;
                self.text_h = bmp_h;
                self.box_w = bmp_w;
                self.box_h = bmp_h;
                self.msg_surf = Some(surf);
            }
        }
    }

    pub fn messages_ads_path(data_dir: &str) -> PathBuf {
        PathBuf::from(data_dir).join("MESSAGES.ADS")
    }
}

impl Default for MessagesModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AdModule for MessagesModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.render_message_bitmap(ctx);
        self.pop_timer = 0;
        self.pop_x = 0;
        self.pop_y = 0;
        self.float_x = 40.min((ctx.module.rgn_width - self.box_w).max(0));
        self.float_y = 40.min((ctx.module.rgn_height - self.box_h).max(0));
        self.dx_dir = 1;
        self.dy_dir = 1;
        self.slide_x = -10_000;
        self.slide_y = 0;
        self.last_step_ms = 0;
        self.first_step = true;
        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        if ctx.module.control_value[0] != self.move_mode {
            return AdResult::RestartMe;
        }
        // Exact 50ms pacing (`1:0683..1:069E`)
        if !self.first_step && ctx.elapsed_ms < self.last_step_ms + 50 {
            return AdResult::NoError;
        }
        self.first_step = false;
        self.last_step_ms = ctx.elapsed_ms;

        let Some(ref msg_surf) = self.msg_surf else {
            return AdResult::NoError;
        };
        let rgn_w = ctx.module.rgn_width;
        let rgn_h = ctx.module.rgn_height;
        let speed_val = ctx.module.control_value[1];
        self.speed_step = (speed_val / 15) + 1;

        match self.move_mode {
            0 => {
                // Popping (`1:0516..1:05C9`)
                if self.pop_timer <= 0 {
                    ctx.surface
                        .pat_blt_black(self.pop_x, self.pop_y, self.box_w, self.box_h);
                    let max_x = (rgn_w - self.box_w + 7).max(1);
                    let max_y = (rgn_h - self.box_h + 7).max(1);
                    self.pop_x = ctx.rng.rand_mod(max_x);
                    self.pop_y = ctx.rng.rand_mod(max_y);
                    ctx.surface.bit_blt_surface(
                        self.pop_x,
                        self.pop_y,
                        self.box_w,
                        self.box_h,
                        msg_surf,
                        0,
                        0,
                        ROP_SRCCOPY,
                    );
                    self.pop_timer = (150 - speed_val).max(5);
                } else {
                    self.pop_timer -= 1;
                }
            }
            1 => {
                // Floating (`1:0490..1:0515`)
                self.float_x += self.speed_step * self.dx_dir;
                if self.float_x + self.box_w - 7 >= rgn_w || self.float_x < -7 {
                    self.dx_dir = -self.dx_dir;
                    self.float_x = self.float_x.clamp(-7, (rgn_w - self.box_w + 7).max(-7));
                }
                self.float_y += self.dy_dir;
                if self.float_y + self.box_h - 7 >= rgn_h || self.float_y < -7 {
                    self.dy_dir = -self.dy_dir;
                    self.float_y = self.float_y.clamp(-7, (rgn_h - self.box_h + 7).max(-7));
                }
                ctx.surface.bit_blt_surface(
                    self.float_x,
                    self.float_y,
                    self.box_w,
                    self.box_h,
                    msg_surf,
                    0,
                    0,
                    ROP_SRCCOPY,
                );
            }
            _ => {
                // Sliding right-to-left (`1:05CA..1:065F`)
                if self.slide_x <= -self.text_w || self.slide_x == -10_000 {
                    let max_y = (rgn_h - self.text_h).max(1);
                    self.slide_y = ctx.rng.rand_mod(max_y);
                    self.slide_x = rgn_w;
                } else {
                    self.slide_x -= (speed_val / 15) + 1;
                }
                ctx.surface.bit_blt_surface(
                    self.slide_x,
                    self.slide_y,
                    self.box_w,
                    self.box_h,
                    msg_surf,
                    0,
                    0,
                    ROP_SRCCOPY,
                );
            }
        }

        AdResult::NoError
    }

    fn do_close(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        self.msg_surf = None;
        AdResult::NoError
    }
}
