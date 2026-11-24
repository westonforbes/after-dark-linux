//! 1:1 Decompilation of `GRAFSTAT.AD` ("GraphStat") from After Dark 1.0a (1991).
//!
//! Disassembly-backed from `docs/disasm/grafstat_seg1.asm` and `docs/disasm/grafstat_data.hex`.

use std::path::Path;

use ad_sdk::canvas::RECT;

use ad_sdk::palette::rgb;
use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult};

const DEFAULT_GRAFSTAT_TXT: &str = include_str!("../../../assets/GRAFSTAT.TXT");

/// Exact 64-entry fixed-point `log2(1 + i/64) * 65536` table (`DS:0x0058..0x00D7`).
const LOG2_TABLE: [u16; 64] = [
    0x0000, 0x05BA, 0x0B5D, 0x10EB, 0x1664, 0x1BC8, 0x2119, 0x2656, 0x2B80, 0x3098, 0x359F,
    0x3A94, 0x3F78, 0x444C, 0x4910, 0x4DC5, 0x526A, 0x5700, 0x5B89, 0x6003, 0x646F, 0x68CE,
    0x6D20, 0x7165, 0x759D, 0x79CA, 0x7DEA, 0x81FF, 0x8608, 0x8A06, 0x8DFA, 0x91E2, 0x95C0,
    0x9994, 0x9D5E, 0xA11E, 0xA4D4, 0xA881, 0xAC24, 0xAFBE, 0xB350, 0xB6D9, 0xBA59, 0xBDD1,
    0xC140, 0xC4A8, 0xC807, 0xCB5F, 0xCEAF, 0xD1F7, 0xD538, 0xD872, 0xDBA5, 0xDED0, 0xE1F5,
    0xE513, 0xE82A, 0xEB3B, 0xEE45, 0xF149, 0xF446, 0xF73E, 0xFA2F, 0xFD1A,
];

/// Exact 16.16 fixed-point `log2(val)` from `GRAFSTAT.AD` (`1:0000..1:007D`).
fn fix_log2(mut val: u32) -> i32 {
    if val == 0 {
        return 0;
    }
    let mut exp: i32 = 31;
    while (val as i32) >= 0 && exp > 0 {
        val <<= 1;
        exp -= 1;
    }
    val <<= 1;
    let idx = ((val >> 26) & 0x3F) as usize;
    (exp << 16) + LOG2_TABLE[idx] as i32
}

pub struct GraphStatModule {
    sections: [Vec<String>; 4],
    first_frame: bool,
    last_draw_tick: u64,
}

impl GraphStatModule {
    pub fn new() -> Self {
        Self {
            sections: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            first_frame: true,
            last_draw_tick: 0,
        }
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "grafstat".to_string(),
            filename: "GRAFSTAT.AD".to_string(),
            name: "GraphStat".to_string(),
            credit: "GraphStat by Harry Chesley & Wes Boyd.\n©1990 Berkeley Systems Inc."
                .to_string(),
            about: "GRAPHSTAT (tm) draws meaningless but significant looking graphs on your screen.\n\nUse the \"Delay\" slider to set a time delay between each graph appearing.\n\nUse the \"Edit..\" button to change the text used in GraphStat labels. The text is divided into 4 sections, each separated by a blank line.".to_string(),
            controls: [
                AdControl::NumericSlider {
                    title: "Delay:".to_string(),
                    default_raw: 15,
                    unit: " sec".to_string(),
                    min: 0,
                    max: 100,
                },
                AdControl::None,
                AdControl::Button {
                    title: "Edit..".to_string(),
                },
                AdControl::None,
            ],
            target_frame_ms: 50,
        }
    }

    pub fn load_grafstat_text(data_dir: &str) -> String {
        if !data_dir.is_empty() {
            let p = Path::new(data_dir).join("GRAFSTAT.TXT");
            if let Ok(text) = std::fs::read_to_string(&p) {
                if !text.trim().is_empty() {
                    return text;
                }
            }
        }
        DEFAULT_GRAFSTAT_TXT.to_string()
    }

    fn parse_sections(text: &str) -> [Vec<String>; 4] {
        let mut secs: [Vec<String>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut cur = 0usize;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if cur < 3 && !secs[cur].is_empty() {
                    cur += 1;
                }
            } else if cur < 4 {
                secs[cur].push(trimmed.to_string());
            }
        }
        secs
    }

    /// Centered signed rand matching `GRAFSTAT.AD`'s `(rand() - 0x3E80)` (`-16000..+16767`).
    #[inline]
    fn srand_centered(ctx: &mut AdContext<'_>) -> i32 {
        ctx.rng.rand() - 0x3E80
    }

    fn draw_graph_paper_and_labels(&self, ctx: &mut AdContext<'_>) {
        let w = ctx.module.rgn_width.max(64);
        let h = ctx.module.rgn_height.max(64);
        let sf = (h as f32 / 480.0).clamp(0.85, 4.0);
        ctx.surface.fill_black();

        let grid_step = (((Self::srand_centered(ctx) % 30 + 50).max(16) as f32) * sf).round() as i32;
        let green_pen = if ctx.system.bits_per_pixel < 4 {
            rgb(255, 255, 255)
        } else {
            rgb(0, 255, 0)
        };
        let gray_pen = if ctx.system.bits_per_pixel < 4 {
            rgb(255, 255, 255)
        } else {
            rgb(140, 140, 140)
        };

        if Self::srand_centered(ctx) % 3 == 0 {
            // 1/3: Polar / Radar chart (`1:0652..1:0899`)
            let half_w = w / 2;
            let third_w = w / 3;
            let fifth_w = w / 5;
            let cx = half_w + (Self::srand_centered(ctx) & fifth_w);
            let cy = h / 2 + Self::srand_centered(ctx) % (h / 5).max(1);

            // 8 radial green spokes through (cx, cy)
            ctx.surface.line(cx, 0, cx, h, 1, green_pen);
            ctx.surface.line(0, cy, w, cy, 1, green_pen);
            ctx.surface.line(cx - w, cy - w, cx + w, cy + w, 1, green_pen);
            ctx.surface.line(cx - w, cy + w, cx + w, cy - w, 1, green_pen);
            ctx.surface
                .line(cx + half_w, cy - w, cx - half_w, cy + w, 1, green_pen);
            ctx.surface
                .line(cx + w, cy - half_w, cx - w, cy + half_w, 1, green_pen);
            ctx.surface
                .line(cx + w, cy + half_w, cx - w, cy - half_w, 1, green_pen);
            ctx.surface
                .line(cx + half_w, cy + w, cx - half_w, cy - w, 1, green_pen);

            // Concentric gray circles (`1:0820..1:0898`)
            let mut left = cx - grid_step;
            let mut right = cx + grid_step;
            let mut top = cy - grid_step;
            let mut bottom = cy + grid_step;
            while -third_w < left || (w + third_w) > right {
                ctx.surface
                    .ellipse(left, top, right, bottom, Some(gray_pen), None);
                left -= grid_step;
                right += grid_step;
                top -= grid_step;
                bottom += grid_step;
            }
        } else {
            // 2/3: Cartesian / Semi-Log / Log-Log green grid (`1:089A..1:0AE0`)
            // Horizontal lines (Y axis)
            if Self::srand_centered(ctx) % 2 == 0 {
                let offset = Self::srand_centered(ctx) % 30;
                let count = (Self::srand_centered(ctx) % 20 + 30).max(5);
                let span = h + (Self::srand_centered(ctx) % 20).abs();
                let log_max = fix_log2(count as u32).max(1) as i64;
                for i in 1..=count {
                    let y_pos = ((((fix_log2(i as u32) as i64) * (span as i64)) / log_max) as i32)
                        + offset;
                    ctx.surface.line(0, y_pos, w, y_pos, 1, green_pen);
                }
            } else {
                let mut y = Self::srand_centered(ctx) % grid_step;
                while y < h {
                    ctx.surface.line(0, y, w, y, 1, green_pen);
                    y += grid_step;
                }
            }

            // Vertical lines (X axis)
            if Self::srand_centered(ctx) % 2 == 0 {
                let offset = Self::srand_centered(ctx) % 30;
                let count = (Self::srand_centered(ctx) % 20 + 30).max(5);
                let span = w + (Self::srand_centered(ctx) % 20).abs();
                let log_max = fix_log2(count as u32).max(1) as i64;
                for i in 1..=count {
                    let x_pos = ((((fix_log2(i as u32) as i64) * (span as i64)) / log_max) as i32)
                        + offset;
                    ctx.surface.line(x_pos, 0, x_pos, h, 1, green_pen);
                }
            } else {
                let mut x = Self::srand_centered(ctx) % grid_step;
                while x < w {
                    ctx.surface.line(x, 0, x, h, 1, green_pen);
                    x += grid_step;
                }
            }
        }

        // Meaningless technical title from GRAFSTAT.TXT (`1:0AED..1:0C9C`)
        let tx = (Self::srand_centered(ctx) % 50).abs() + 20;
        let ty = (Self::srand_centered(ctx) % (h - 70).max(1)).abs() + 20;

        if !self.sections[0].is_empty()
            && !self.sections[1].is_empty()
            && !self.sections[2].is_empty()
        {
            let s0 = &self.sections[0]
                [(Self::srand_centered(ctx).abs() as usize) % self.sections[0].len()];
            let s1 = &self.sections[1]
                [(Self::srand_centered(ctx).abs() as usize) % self.sections[1].len()];
            let s2 = &self.sections[2]
                [(Self::srand_centered(ctx).abs() as usize) % self.sections[2].len()];
            let title = format!("{s0} {s1} {s2}");
            let title_h = ((if h < 400 { 18.0 } else { 26.0 }) * sf).round() as i32;
            let white = rgb(255, 255, 255);
            let rc = RECT {
                left: tx,
                top: ty,
                right: w,
                bottom: h,
            };
            ctx.surface
                .draw_text_font("Arial", title_h, true, false, false, white, rc, false, false, &title);

            if !self.sections[3].is_empty() && Self::srand_centered(ctx) % 2 == 0 {
                let s3 = &self.sections[3]
                    [(Self::srand_centered(ctx).abs() as usize) % self.sections[3].len()];
                let sub_h = ((if h < 400 { 12.0 } else { 19.0 }) * sf).round() as i32;
                let sub_rc = RECT {
                    left: tx + (20.0 * sf) as i32,
                    top: ty + title_h + (6.0 * sf) as i32,
                    right: w,
                    bottom: h,
                };
                ctx.surface
                    .draw_text_font("Arial", sub_h, true, false, false, white, sub_rc, false, false, s3);
            }
        }
    }

    fn draw_full_graph(&mut self, ctx: &mut AdContext<'_>) {
        let w = ctx.module.rgn_width.max(64);
        let h = ctx.module.rgn_height.max(64);
        self.draw_graph_paper_and_labels(ctx);

        // Pick cubic polynomial y(x) = (a3*x + a2)*x^2 + a1*x (`1:0D1D..1:0F1A`)
        let (a1, a2, a3, x0, min_y, scale, y_off) = loop {
            let a1 = (Self::srand_centered(ctx) % 100) as i64;
            let a2 = if Self::srand_centered(ctx) % 2 == 0 {
                0i64
            } else {
                (Self::srand_centered(ctx) % 100) as i64
            };
            let a3 = if Self::srand_centered(ctx) % 2 == 0 {
                0i64
            } else {
                (Self::srand_centered(ctx) % 10) as i64
            };
            let x0 = (Self::srand_centered(ctx) % w) as i64;

            let mut max_v = 0i64;
            let mut min_v = 0i64;
            for i in 0..=w {
                let x = x0 + i as i64;
                let v = (a3 * x + a2) * x * x + a1 * x;
                if v > max_v {
                    max_v = v;
                }
                if v < min_v {
                    min_v = v;
                }
            }
            let y_off = Self::srand_centered(ctx) % 50;
            let mut scale = (max_v - min_v) / (h - 1).max(1) as i64;
            let jitter = (Self::srand_centered(ctx) % 100) as i64;
            scale += (scale * jitter) / 1000;
            if scale >= 1 {
                break (a1, a2, a3, x0, min_v, scale, y_off);
            }
        };

        let white = rgb(255, 255, 255);
        let random_circles = (Self::srand_centered(ctx) % 4) < 1;
        let mut last_circle_step: i32 = -100;

        let eval_y = |x: i64| -> i32 {
            let v = (a3 * x + a2) * x * x + a1 * x;
            (((v - min_y) / scale) as i32) + y_off
        };

        let mut prev_x = 0;
        let mut prev_y = eval_y(x0 - 1);
        let max_step = w / 4;

        for step in 1..=max_step {
            let x_px = step * 4;
            let x_val = x0 - 1 + x_px as i64;
            let y_px = eval_y(x_val);

            // Detect grid intersections via GetPixel and draw 2px white callout rings (`1:108D..1:13C8`)
            if last_circle_step + 10 < step
                && step > 50
                && step < max_step - 5
                && y_px > 20
                && y_px < h - 20
            {
                let on_grid = (ctx.surface.get_pixel(x_px, y_px) != 0
                    && ctx.surface.get_pixel(x_px + 1, y_px) != 0)
                    || (random_circles && Self::srand_centered(ctx) % 15 == 0);
                if on_grid {
                    let odd_hit = (5..=27)
                        .step_by(2)
                        .any(|dx| ctx.surface.get_pixel(x_px + dx, y_px) != 0);
                    let clear = if !odd_hit {
                        true
                    } else {
                        !(6..=28)
                            .step_by(2)
                            .any(|dx| ctx.surface.get_pixel(x_px + dx, y_px) != 0)
                    };
                    if clear {
                        let sf = (h as f32 / 480.0).clamp(0.85, 4.0);
                        let pen_w = (2.0 * sf).round().max(2.0) as i32;
                        let rad = (((Self::srand_centered(ctx) % 10 + 30).max(16) as f32) * sf).round() as i32;
                        let left = x_px - rad + 2;
                        let top = y_px - rad + 2;
                        ctx.surface.ellipse_pen_width(
                            left,
                            top,
                            left + rad * 2,
                            top + rad * 2,
                            pen_w,
                            Some(white),
                            None,
                        );
                        last_circle_step = step;
                    }
                }
            }

            let line_w = ((h as f32 / 480.0).clamp(1.0, 4.0) * 2.0).round() as i32;
            ctx.surface.line(prev_x, prev_y, x_px, y_px, line_w, white);
            prev_x = x_px;
            prev_y = y_px;
        }
    }
}

impl Default for GraphStatModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AdModule for GraphStatModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        let text = Self::load_grafstat_text(&ctx.system.data_dir);
        self.sections = Self::parse_sections(&text);
        self.first_frame = true;
        self.last_draw_tick = 0;
        AdResult::NoError
    }

    fn do_blank(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        // Original DoBlank is a no-op; first DoDrawFrame clears and draws immediately
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        let now_tick = ctx.elapsed_ms >> 4;
        let delay_ticks = (60 * ctx.module.control_value[0].max(0)) as u64;
        if self.first_frame || now_tick.saturating_sub(self.last_draw_tick) > delay_ticks {
            self.first_frame = false;
            self.draw_full_graph(ctx);
            self.last_draw_tick = now_tick;
        }
        AdResult::NoError
    }
}
