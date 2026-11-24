//! **Stained Glass** (`STAINED.AD`) — 1:1 decompilation of the 1991 After Dark 1.0a module.
//!
//! Original credit: *"Stained Glass by Jean Tantra and Wes Boyd. ©1990 Berkeley Systems Inc."*
//!
//! Reconstructed 1:1 from `STAINED.AD` NE Segment 1 (`1:0012..1:460C`) and Segment 2 (`DS:0x0000..0x0198`):
//! - 8-way kaleidoscopic stained-glass quilt generator centered at `(cx, cy)` via logical-origin centering
//!   (`SetWindowOrg(-cx, -cy)`).
//! - Each tile chooses an odd half-size `R` (`h/10`, `w/14`, or `h/6`), initializes 4 independent 8-way
//!   random-walker "spiders" (with non-reversing 8-direction deltas `DS:0x0022`, follower coupling,
//!   XOR/COPYPEN modes, brush stamps, and optional corner/vertex starbursts via `ord_29`), plus an
//!   8-way symmetric scanline pixel-dither pass with optional checkerboard masks, diagonal/wedge cutoffs,
//!   offset shadow pixels, and random speckle scatter.
//! - Tile setup optionally stamps initial symmetric frames, concentric diamonds/squares/X-marks (`ord_16`,
//!   `ord_18`), corner circles (`ord_13..15`), symmetric radiating lines (`ord_27`), and nested rotated
//!   square webs (`ord_26`).
//! - Mid-tile and on tile completion, stamps the center tile across the screen using `BitBlt`/`StretchBlt`
//!   in 3×3 grids (`ord_20`), checkerboard-inverted 3×3 grids (`ord_19`), 5×5 outer perimeter rings
//!   (`ord_22`), 1/3-scaled inner core tiling (`1:1CF6`), and 2-tile cardinal/diagonal stamps (`1:1DA4`),
//!   controlled by the `Complexity`, `Duplication`, and `Color` sliders.

use ad_sdk::canvas::{AdSurface, R2_COPYPEN, R2_XORPEN, ROP_SRCCOPY};
use ad_sdk::palette::rgb;
use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult, MsvcRand};
use ad_sdk::canvas::RECT;

/// Exact 16-entry palette at `STAINED.AD` `DS:0x006A..0x00C9` (6-byte 16-bit RGB entries, high bytes used).
const PALETTE_0: [u32; 16] = [
    rgb(0, 0, 0),       // 0: Black
    rgb(255, 255, 255), // 1: White
    rgb(127, 127, 127), // 2: Gray
    rgb(0, 0, 127),     // 3: Dark Blue
    rgb(255, 0, 0),     // 4: Red
    rgb(255, 255, 0),   // 5: Yellow
    rgb(0, 255, 255),   // 6: Cyan
    rgb(127, 0, 127),   // 7: Purple
    rgb(255, 0, 255),   // 8: Magenta
    rgb(0, 127, 0),     // 9: Dark Green
    rgb(0, 0, 255),     // 10: Blue
    rgb(127, 0, 0),     // 11: Dark Red
    rgb(255, 255, 0),   // 12: Yellow
    rgb(0, 127, 127),   // 13: Teal
    rgb(0, 255, 0),     // 14: Bright Green
    rgb(0, 0, 0),       // 15: Black
];

/// Exact 9-entry direction delta table at `DS:0x0022..0x0045` (index 4 = (0,0) skipped).
const DIR_DELTAS: [(i32, i32); 9] = [
    (-1, -1), // 0
    (0, -1),  // 1
    (1, -1),  // 2
    (-1, 0),  // 3
    (0, 0),   // 4 (unused)
    (1, 0),   // 5
    (-1, 1),  // 6
    (0, 1),   // 7
    (1, 1),   // 8
];

/// Opposite direction lookup table at `DS:0x0046..0x0057`.
const OPPOSITE_DIR: [i32; 9] = [8, 7, 6, 5, 4, 3, 2, 1, 0];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct RectI {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectI {
    fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    fn width(&self) -> i32 {
        self.right - self.left
    }

    fn height(&self) -> i32 {
        self.bottom - self.top
    }

    fn offset(&mut self, dx: i32, dy: i32) {
        self.left += dx;
        self.top += dy;
        self.right += dx;
        self.bottom += dy;
    }

    fn inflate(&mut self, dx: i32, dy: i32) {
        self.left -= dx;
        self.top -= dy;
        self.right += dx;
        self.bottom += dy;
    }

    fn zero_origin(&mut self) {
        let dx = -self.left;
        let dy = -self.top;
        self.offset(dx, dy);
    }

    fn intersect(&self, other: &Self) -> Self {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right.min(other.right);
        let bottom = self.bottom.min(other.bottom);
        if left < right && top < bottom {
            Self::new(left, top, right, bottom)
        } else {
            Self::default()
        }
    }
}

#[derive(Clone, Copy)]
struct PenState {
    color: u32,
    rop2: i32,
    pen_width: i32,
}

impl Default for PenState {
    fn default() -> Self {
        Self {
            color: rgb(255, 255, 255),
            rop2: R2_COPYPEN,
            pen_width: 1,
        }
    }
}

/// 32-byte walker struct at `DS:0x0534 + i*0x20` (`i in 0..4`).
#[derive(Clone, Copy, Default)]
struct Walker {
    x: i32,             // +0x00
    y: i32,             // +0x02
    dir: i32,           // +0x04
    step_len: i32,      // +0x08
    step_cur: i32,      // +0x0A
    draw_mode: i32,     // +0x10 (0 -> XORPEN, 1..3 -> COPYPEN + shape stamp if brush_size > 1)
    is_rect_stamp: i32, // +0x12 (always 1 at 1:11DB -> ord_33 uses shape 3 = FillRect)
    color_idx: i32,     // +0x14
    follow_prev: i32,   // +0x16
    stamp_burst: i32,   // +0x1A
    brush_size: i32,    // +0x1C (0, 1, 3, or 5)
    pixel_color: i32,   // +0x1E
}

/// Saved motif entry used by `ord_18` (`1:27F6`).
#[derive(Clone, Copy, Default)]
struct Motif {
    x: i32,
    y: i32,
    size: i32,
    pen_w: i32,
    shape: i32,
}

pub struct StainedGlassModule {
    screen_rect: RectI, // DS:0x089C
    is_color: bool,     // DS:0x0010

    // Window origin & GDI pen/brush/ROP2 tracking (`ord_17`, `ord_31`, `ord_34`, `ord_36`, `ord_41`)
    win_org_x: i32, // DS:0x08B2
    win_org_y: i32, // DS:0x08B0
    pen: PenState,  // DS:0x0012..0x0018

    // Slider change-detection & palette state (`ord_4`, `ord_8..12`)
    num_walkers: i32,      // DS:0x08A4
    prev_num_walkers: i32, // DS:0x0058
    dup_interval: i32,     // DS:0x08AE
    prev_dup_interval: i32,// DS:0x005A
    dup_countdown: i32,    // DS:0x005C
    num_colors: i32,       // DS:0x005E
    prev_num_colors: i32,  // DS:0x0060
    prev_color_param: i32, // DS:0x0062
    sub_pal_mode: i32,     // DS:0x0068 (0 = PALETTE_0, 1 = 3-color monochrome tint)
    sub_pal: [u32; 3],     // DS:0x0102..0x0113

    // Current quilt/tile state (`DS:0x04B0..0x0615`)
    complexity: i32,       // DS:0x04B0
    duplication: i32,      // DS:0x04B2
    color_param: i32,      // DS:0x04B4
    scan_row: i32,         // DS:0x04B8
    tile_count: i32,       // DS:0x04BE
    need_new_tile: bool,   // DS:0x04C0
    tile_done: bool,       // DS:0x04C2
    radius: i32,           // DS:0x04CE
    center_rect: RectI,    // DS:0x04D0 (and 0x05EA)
    center_y: i32,         // DS:0x04DA
    center_x: i32,         // DS:0x04DC
    fg_color_idx: i32,     // DS:0x04E6
    dot_color_on: i32,     // DS:0x04E8
    dot_color_off: i32,    // DS:0x04EA
    draw_scan_pass: bool,  // DS:0x04EC (0 = draw scanline pixels, 1 = skip)
    run_steps: bool,       // DS:0x04EE
    dot_range: i32,        // DS:0x04F0 (100)
    dot_thresh_base: i32,  // DS:0x04F2
    dot_thresh: i32,       // DS:0x04F4
    dot_thresh_step: i32,  // DS:0x04F6
    dot_thresh_reset: i32, // DS:0x04F8
    wedge_start_row: i32,  // DS:0x04FA
    wedge_slope_step: i32, // DS:0x04FC
    wedge_offset: i32,     // DS:0x04FE
    margin_inset: i32,     // DS:0x0500
    mask_row: i32,         // DS:0x0502
    mask_col: i32,         // DS:0x0504
    draw_axes_always: bool,// DS:0x0506
    diag_slope: i32,       // DS:0x0508
    scatter_flag: bool,    // DS:0x050A
    row_start: i32,        // DS:0x050C
    row_end: i32,          // DS:0x050E
    row_step: i32,         // DS:0x0512
    col_start: i32,        // DS:0x0514
    col_end: i32,          // DS:0x0516
    col_min: i32,          // DS:0x0518
    rev_diag_slope: i32,   // DS:0x051E
    extra_shapes: bool,    // DS:0x0520
    motif_active: bool,    // DS:0x0522
    do_shrink_tile: bool,  // DS:0x0526
    do_outer_3x3_a: bool,  // DS:0x0528
    do_outer_3x3_b: bool,  // DS:0x052A
    do_outer_ring: bool,   // DS:0x052C
    walkers_enabled: bool, // DS:0x052E
    walkers: [Walker; 4],  // DS:0x0534
    prev_wedge_flag: i32,  // DS:0x05D6
    mid_tile_enabled: bool,// DS:0x05DA
    shadow_enabled: bool,  // DS:0x05DC
    shadow_dx: i32,        // DS:0x05DE
    shadow_dy: i32,        // DS:0x05E0
}

impl Default for StainedGlassModule {
    fn default() -> Self {
        Self {
            screen_rect: RectI::default(),
            is_color: true,
            win_org_x: 0,
            win_org_y: 0,
            pen: PenState::default(),
            num_walkers: 0,
            prev_num_walkers: 0,
            dup_interval: 0,
            prev_dup_interval: 0,
            dup_countdown: 0,
            num_colors: 2, // DS:0x005E initialized to 2 in DS image
            prev_num_colors: 0,
            prev_color_param: 0,
            sub_pal_mode: 0,
            sub_pal: [rgb(0, 0, 0), rgb(255, 255, 255), rgb(0, 255, 255)],
            complexity: 10,
            duplication: 100,
            color_param: 100,
            scan_row: 0,
            tile_count: 0,
            need_new_tile: true,
            tile_done: false,
            radius: 15,
            center_rect: RectI::default(),
            center_y: 0,
            center_x: 0,
            fg_color_idx: 1,
            dot_color_on: 1,
            dot_color_off: 0,
            draw_scan_pass: false,
            run_steps: true,
            dot_range: 100,
            dot_thresh_base: 1,
            dot_thresh: 50,
            dot_thresh_step: 1,
            dot_thresh_reset: 1,
            wedge_start_row: 0,
            wedge_slope_step: 0,
            wedge_offset: 0,
            margin_inset: 0,
            mask_row: -1,
            mask_col: -1,
            draw_axes_always: false,
            diag_slope: 1,
            scatter_flag: false,
            row_start: 0,
            row_end: 0,
            row_step: 1,
            col_start: 0,
            col_end: 0,
            col_min: 0,
            rev_diag_slope: 0,
            extra_shapes: false,
            motif_active: false,
            do_shrink_tile: false,
            do_outer_3x3_a: false,
            do_outer_3x3_b: false,
            do_outer_ring: false,
            walkers_enabled: true,
            walkers: [Walker::default(); 4],
            prev_wedge_flag: 0,
            mid_tile_enabled: false,
            shadow_enabled: false,
            shadow_dx: 0,
            shadow_dy: 0,
        }
    }
}

impl StainedGlassModule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "stained".to_string(),
            filename: "STAINED.AD".to_string(),
            name: "Stained Glass".to_string(),
            credit: "Stained Glass by Jean Tantra and Wes Boyd.\n  ©1990 Berkeley Systems Inc."
                .to_string(),
            about: "STAINED GLASS(tm) produces an infinite variety of beautiful quiltlike patterns.\n\nThe \"Complexity\" slider increases the appearance of detail in the image.\n\nThe \"Duplication\" slider controls how often the central image is duplicated in the periphery.\n\nThe \"Color\" slider controls how colorful the image gets.  On the low end only a few colors are used.\n\nDedicated to Anna Benson.\nWith contributions from: Ellen Lewis, \nKristieanne Jones, Eleanor Allen, Julia Kay, and David Oster.".to_string(),
            controls: [
                AdControl::NumericSlider {
                    title: "Complexity:".to_string(),
                    default_raw: 10,
                    unit: "%".to_string(),
                    min: 0,
                    max: 100,
                },
                AdControl::NumericSlider {
                    title: "Duplication:".to_string(),
                    default_raw: 100,
                    unit: "%".to_string(),
                    min: 0,
                    max: 100,
                },
                AdControl::NumericSlider {
                    title: "Color:".to_string(),
                    default_raw: 100,
                    unit: "%".to_string(),
                    min: 0,
                    max: 100,
                },
                AdControl::None,
            ],
            target_frame_ms: 16,
        }
    }

    /// `ord_7(max_val)` (`1:42FE`): `(abs(rand()) & 0x0FFF) % (max_val + 1)`.
    #[inline]
    fn rand_upto(rng: &mut MsvcRand, max_val: i32) -> i32 {
        let denom = max_val + 1;
        if denom <= 0 {
            return 0;
        }
        let mut r = rng.rand();
        if r < 0 {
            r = -r;
        }
        (r & 0x0FFF) % denom
    }

    /// `ord_11(numer, denom)` (`1:42D6`): true with probability `numer / denom`.
    #[inline]
    fn rand_chance(rng: &mut MsvcRand, numer: i32, denom: i32) -> bool {
        if denom <= 0 {
            return false;
        }
        Self::rand_upto(rng, denom - 1) < numer
    }

    /// Resolve a logical palette index into an RGB COLORREF (`ord_9` / `ord_32`).
    fn resolve_color(&self, idx: i32) -> u32 {
        if !self.is_color {
            return if (idx & 1) != 0 {
                rgb(255, 255, 255)
            } else {
                rgb(0, 0, 0)
            };
        }
        if self.sub_pal_mode == 0 {
            let clamped = idx.clamp(0, 15) as usize;
            PALETTE_0[clamped]
        } else {
            let clamped = idx.clamp(0, 2) as usize;
            self.sub_pal[clamped]
        }
    }

    /// `ord_9(idx)` (`1:4426`): set current drawing color.
    fn set_color_idx(&mut self, idx: i32) {
        self.pen.color = self.resolve_color(idx);
    }

    /// `ord_12(max_idx)` (`1:4486`): set color index (or random in `0..=num_colors` in sub-pal mode).
    fn set_color_maybe_rand(&mut self, rng: &mut MsvcRand, mut idx: i32) {
        if self.is_color {
            if self.sub_pal_mode != 0 && Self::rand_upto(rng, 1) != 0 {
                idx = Self::rand_upto(rng, self.num_colors);
            }
        } else {
            idx &= 1;
        }
        self.set_color_idx(idx);
    }

    /// `ord_10()` (`1:44CD`): choose a random active color index.
    fn pick_color_idx(&self, rng: &mut MsvcRand) -> i32 {
        if !self.is_color {
            return Self::rand_upto(rng, 1);
        }
        if self.sub_pal_mode == 1 && Self::rand_upto(rng, 1) != 0 {
            return self.num_colors;
        }
        Self::rand_upto(rng, self.num_colors)
    }

    /// `ord_8(chosen_idx)` (`1:4511`): configure 2-color/3-color monochrome tint sub-palette.
    fn configure_sub_palette(&mut self, rng: &mut MsvcRand, chosen_idx: i32) {
        self.sub_pal_mode = 1;
        self.sub_pal[0] = rgb(0, 0, 0);
        self.sub_pal[1] = rgb(255, 255, 255);
        self.sub_pal[2] = PALETTE_0[chosen_idx.clamp(0, 15) as usize];
        self.num_colors = 2;
        if Self::rand_upto(rng, 2) == 1 && chosen_idx != 5 {
            self.sub_pal[1] = PALETTE_0[5]; // Yellow
        }
    }

    /// `ord_17(wx, wy)` (`1:43C8`): set logical WindowOrg (`dev = log - win_org`).
    #[inline]
    fn set_window_org(&mut self, wx: i32, wy: i32) {
        self.win_org_x = wx;
        self.win_org_y = wy;
    }

    /// Transform logical rect to device `RECT`.
    #[inline]
    fn to_dev_rect(&self, r: RectI) -> RECT {
        RECT {
            left: r.left - self.win_org_x,
            top: r.top - self.win_org_y,
            right: r.right - self.win_org_x,
            bottom: r.bottom - self.win_org_y,
        }
    }

    /// Plot a single pixel in logical coordinates.
    #[inline]
    fn put_pixel(&self, surface: &mut AdSurface, x: i32, y: i32, color: u32) {
        let dx = x - self.win_org_x;
        let dy = y - self.win_org_y;
        if dx >= 0 && dx < surface.width && dy >= 0 && dy < surface.height {
            surface.set_pixel(dx, dy, color);
        }
    }

    /// `ord_32(x, y, color_idx)` (`1:4565`) -> `ord_47` (`1:0B4C`): 8-way symmetric `SetPixel`.
    fn plot_8way(&self, surface: &mut AdSurface, x: i32, y: i32, color_idx: i32) {
        let c = self.resolve_color(color_idx);
        self.put_pixel(surface, x, y, c);
        self.put_pixel(surface, -x, y, c);
        self.put_pixel(surface, -y, x, c);
        self.put_pixel(surface, x, -y, c);
        self.put_pixel(surface, y, -x, c);
        self.put_pixel(surface, -x, -y, c);
        self.put_pixel(surface, y, x, c);
        self.put_pixel(surface, -y, -x, c);
    }

    /// Draw a line in logical coords with current `pen` (color, width, ROP2).
    fn draw_line(&self, surface: &mut AdSurface, x1: i32, y1: i32, x2: i32, y2: i32) {
        let old_rop = surface.set_rop2(self.pen.rop2);
        surface.line(
            x1 - self.win_org_x,
            y1 - self.win_org_y,
            x2 - self.win_org_x,
            y2 - self.win_org_y,
            self.pen.pen_width,
            self.pen.color,
        );
        surface.set_rop2(old_rop);
    }

    /// `USER.FillRect` in logical coords (always `PATCOPY`).
    fn fill_rect_color(&self, surface: &mut AdSurface, r: RectI, color: u32) {
        let rc = self.to_dev_rect(r);
        if rc.left < rc.right && rc.top < rc.bottom {
            surface.fill_rect_rgb(rc.left, rc.top, rc.right, rc.bottom, color);
        }
    }

    /// `USER.FrameRect` in logical coords (always 1px `PATCOPY`).
    fn frame_rect_color(&self, surface: &mut AdSurface, r: RectI, color: u32) {
        let rc = self.to_dev_rect(r);
        if rc.left < rc.right && rc.top < rc.bottom {
            surface.frame_rect_rgb(rc.left, rc.top, rc.right, rc.bottom, color);
        }
    }

    /// `USER.InvertRect` in logical coords.
    fn invert_rect(&self, surface: &mut AdSurface, r: RectI) {
        let rc = self.to_dev_rect(r);
        if rc.left < rc.right && rc.top < rc.bottom {
            surface.invert_rect(rc.left, rc.top, rc.right, rc.bottom);
        }
    }

    /// `ord_44(r)` (`1:26A3`): hollow ellipse with current pen & ROP2 (`NULL_BRUSH`).
    fn draw_hollow_ellipse(&self, surface: &mut AdSurface, r: RectI) {
        let rc = self.to_dev_rect(r);
        let old_rop = surface.set_rop2(self.pen.rop2);
        surface.ellipse_pen_width(
            rc.left,
            rc.top,
            rc.right,
            rc.bottom,
            self.pen.pen_width,
            Some(self.pen.color),
            None,
        );
        surface.set_rop2(old_rop);
    }

    /// `ord_43(r)` (`1:265C`): XOR filled+stroked ellipse with current pen/brush color.
    fn draw_xor_ellipse(&mut self, surface: &mut AdSurface, r: RectI) {
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        let rc = self.to_dev_rect(r);
        let old_rop = surface.set_rop2(self.pen.rop2);
        surface.ellipse_pen_width(
            rc.left,
            rc.top,
            rc.right,
            rc.bottom,
            self.pen.pen_width,
            Some(self.pen.color),
            Some(self.pen.color),
        );
        surface.set_rop2(old_rop);
        self.pen = saved;
    }

    /// `ord_39(r)` (`1:37C6`): XOR diamond connecting midpoints of rect edges.
    fn draw_diamond(&mut self, surface: &mut AdSurface, r: RectI) {
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        let mx = (r.left + r.right) / 2;
        let my = (r.top + r.bottom) / 2;
        self.draw_line(surface, mx, r.top, r.right, my);
        self.draw_line(surface, r.right, my, mx, r.bottom);
        self.draw_line(surface, mx, r.bottom, r.left, my);
        self.draw_line(surface, r.left, my, mx, r.top);
        self.pen = saved;
    }

    /// `ord_42(r)` (`1:35F3`): deflate by 1 and draw XOR X diagonals.
    fn draw_x_in_rect(&mut self, surface: &mut AdSurface, mut r: RectI) {
        r.inflate(-1, -1);
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        self.draw_line(surface, r.right, r.top, r.left, r.bottom);
        self.draw_line(surface, r.left, r.top, r.right, r.bottom);
        self.pen = saved;
    }

    /// `1:2D7C`: draw one of 6 shape primitives into rect `r` with brush color `brush_color`.
    fn draw_primitive(
        &mut self,
        surface: &mut AdSurface,
        shape: i32,
        r: RectI,
        brush_color: u32,
    ) {
        match shape {
            0 => self.frame_rect_color(surface, r, brush_color),
            1 => self.draw_diamond(surface, r),
            2 => {
                self.frame_rect_color(surface, r, brush_color);
                self.draw_x_in_rect(surface, r);
            }
            3 => self.fill_rect_color(surface, r, brush_color),
            4 => self.draw_hollow_ellipse(surface, r),
            5 => {
                // NULL_PEN + brush_color Ellipse using current ROP2
                let rc = self.to_dev_rect(r);
                let old_rop = surface.set_rop2(self.pen.rop2);
                surface.ellipse(rc.left, rc.top, rc.right, rc.bottom, None, Some(brush_color));
                surface.set_rop2(old_rop);
            }
            _ => {}
        }
    }

    /// `1:2E5A`: stamp a primitive in 8-way symmetry around logical `(0, 0)`.
    fn draw_8way_shape(
        &mut self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        shape: i32,
        x: i32,
        y: i32,
        mut size: i32,
        pen_width: i32,
        brush_override: Option<u32>,
    ) {
        let brush_color = brush_override.unwrap_or_else(|| {
            if Self::rand_upto(rng, 2) != 0 {
                rgb(0, 0, 0) // BLACK_BRUSH
            } else {
                rgb(255, 255, 255) // WHITE_BRUSH
            }
        });
        let saved = self.pen;
        self.pen.pen_width = pen_width.max(1);

        size |= 1;
        if shape == 4 {
            size += 1;
        }
        let half = size / 2;
        let mut base = if shape == 0 || shape == 3 || shape == 5 {
            RectI::new(-half, -half, half + 1, half + 1)
        } else {
            let mut r = RectI::new(1, 1, size, size);
            r.zero_origin();
            r.offset(-half, -half);
            if pen_width != 1 && shape != 4 {
                let d = -(pen_width / 2);
                r.offset(d, d);
            }
            if shape == 4 {
                r.offset(1, 1);
            }
            r
        };

        // 8-way offsets matching 1:2F62..30D3 exactly
        let offsets = [
            (y, x),
            (-y, x),
            (-x, y),
            (y, -x),
            (x, -y),
            (-y, -x),
            (x, y),
            (-x, -y),
        ];
        for (ox, oy) in offsets {
            let mut r = base;
            r.offset(ox, oy);
            self.draw_primitive(surface, shape, r, brush_color);
        }
        // Suppress unused-mut lint cleanly
        let _ = &mut base;
        self.pen = saved;
    }

    /// `ord_33` (`1:2B61`): walker brush stamp (black square or black circle in 8-way symmetry).
    fn stamp_walker_brush(
        &mut self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        x: i32,
        y: i32,
        size: i32,
        is_rect: bool,
    ) {
        let shape = if is_rect { 3 } else { 5 };
        self.draw_8way_shape(
            surface,
            rng,
            shape,
            x,
            y,
            size,
            1,
            Some(rgb(0, 0, 0)),
        );
    }

    /// `ord_29` (`1:2BA9`): walker vertex starburst stamp.
    fn stamp_walker_burst(
        &mut self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        x: i32,
        y: i32,
        size: i32,
    ) {
        let mut shape = 0;
        if Self::rand_upto(rng, 3) != 1 {
            shape = 4;
        }
        if Self::rand_upto(rng, 3) == 1 {
            shape = 5;
        }
        self.draw_8way_shape(surface, rng, shape, x, y, size, 1, None);
    }

    /// `ord_35` (`1:2C7C`): periodic random geometric accents inside center rect.
    fn draw_periodic_accents(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand, r: RectI) {
        let mut nr = r;
        nr.zero_origin();
        let w = nr.width();
        let mut shape = if Self::rand_upto(rng, 2) < 1 { 5 } else { 3 };
        if Self::rand_upto(rng, 3) == 1 {
            shape = if Self::rand_upto(rng, 2) < 1 { 4 } else { 0 };
        }
        let count = Self::rand_upto(rng, 2) + 1;
        for _ in 0..=count {
            let mut size = 3;
            if Self::rand_upto(rng, 8) == 1 {
                size = Self::rand_upto(rng, 16) | 3;
            }
            let half = (w / 2).max(1);
            let y = Self::rand_upto(rng, half) + 2;
            let x = Self::rand_upto(rng, half) + 2;
            self.draw_8way_shape(surface, rng, shape, x, y, size, 1, None);
        }
    }

    /// `ord_18` (`1:27F6`): initial ornate double-pass XOR motif inside tile rect.
    fn draw_initial_motif(
        &mut self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        r: RectI,
    ) -> bool {
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        let mut result = false;

        let pen_w_a = Self::rand_upto(rng, 3) | 1;
        let pen_w_b = if pen_w_a == 1 {
            Self::rand_upto(rng, 3) | 1
        } else {
            1
        };

        let mut nr = r;
        nr.zero_origin();
        let w = nr.width().max(2);

        let shape_a = match Self::rand_upto(rng, 8) {
            0..=3 => 4,
            5 => 1,
            7 => 2,
            _ => 4,
        };
        let shape_b = match Self::rand_upto(rng, 8) {
            0..=3 => 4,
            5 => 1,
            7 => 2,
            _ => 4,
        };

        let count_a = (Self::rand_upto(rng, 1) + 1) as usize;
        let count_b = (Self::rand_upto(rng, 1) + 1) as usize;
        let mut motifs_a = [Motif::default(); 4];
        let mut motifs_b = [Motif::default(); 4];

        for pass in 0..=1 {
            for m in motifs_a.iter_mut().take(count_a + 1) {
                if pass == 0 {
                    let size = Self::rand_upto(rng, w) | 3;
                    let half = (w / 2).max(1);
                    let y = Self::rand_upto(rng, half) + 2;
                    let x = Self::rand_upto(rng, half) + 2;
                    *m = Motif {
                        x,
                        y,
                        size,
                        pen_w: pen_w_a,
                        shape: shape_a,
                    };
                    self.draw_8way_shape(surface, rng, m.shape, m.x, m.y, m.size, m.pen_w, None);
                } else {
                    self.draw_8way_shape(surface, rng, m.shape, m.x, m.y, m.size, m.pen_w, None);
                }
            }

            if pass == 0 {
                for m in motifs_b.iter_mut().take(count_b + 1) {
                    let size = Self::rand_upto(rng, w) | 1;
                    let half = (w / 2).max(1);
                    let y = Self::rand_upto(rng, half) + 2;
                    let x = Self::rand_upto(rng, half) + 2;
                    *m = Motif {
                        x,
                        y,
                        size,
                        pen_w: pen_w_b,
                        shape: shape_b,
                    };
                    self.draw_8way_shape(surface, rng, m.shape, m.x, m.y, m.size, m.pen_w, None);
                }
            } else {
                // 1:2A9F..2AC7: sometimes leave pass B drawn (returns true -> motif_active)
                if Self::rand_upto(rng, 1) == 0 || (shape_b == 4 && Self::rand_upto(rng, 1) == 0) {
                    result = true;
                } else {
                    for m in motifs_b.iter().take(count_b + 1) {
                        self.draw_8way_shape(
                            surface, rng, m.shape, m.x, m.y, m.size, m.pen_w, None,
                        );
                    }
                }
            }
        }

        self.pen = saved;
        result
    }

    /// `ord_40` (`1:4349`): split rect into its 4 quadrants `[q1, q2, q3, q4]`.
    fn split_quadrants(r: RectI) -> [RectI; 4] {
        let mx = (r.left + r.right) / 2;
        let my = (r.top + r.bottom) / 2;
        [
            RectI::new(mx, r.top, r.right, my),       // TR (offset +0x08)
            RectI::new(mx, my, r.right, r.bottom),    // BR (offset +0x10)
            RectI::new(r.left, my, mx, r.bottom),     // BL (offset +0x18)
            RectI::new(r.left, r.top, mx, my),        // TL (offset +0x20)
        ]
    }

    /// `ord_15` (`1:3146`): draw 4 XOR ellipses in offset quadrants.
    fn draw_quadrant_ellipses(&mut self, surface: &mut AdSurface, r: RectI, shift: i32) {
        let mut q = Self::split_quadrants(r);
        q[0].offset(-shift, shift);
        q[1].offset(-shift, -shift);
        q[2].offset(shift, -shift);
        q[3].offset(shift, shift);
        for qr in q {
            self.draw_xor_ellipse(surface, qr);
        }
    }

    /// `ord_16` (`1:32E8`): concentric frame + diamond + inset frame + inset diamond.
    fn draw_nested_diamonds(&mut self, surface: &mut AdSurface, mut r: RectI) {
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        self.frame_rect_color(surface, r, self.pen.color);
        self.draw_diamond(surface, r);
        let step = (r.left - r.right) / 4; // negative
        r.inflate(-step, -step); // wait: -step > 0? No: r.left - r.right is negative! -step is positive -> inflates outward! Wait: check 1:333A..3359!
        // Let's check exact 1:333A..3359:
        // ax = left - right (negative), sar by 2 => -width/4.
        // neg ax => +width/4. InflateRect(r, +width/4, +width/4) -> EXPANDS by width/4!
        // Wait: actually let's verify: InflateRect(rc, dx, dy) does left-=dx, right+=dx!
        // Wait: in 1:333A: mov ax, [bx] (left); sub ax, [bx+4] (right) -> NEGATIVE!
        // Then 1:3355: neg ax -> POSITIVE! So InflateRect expands r outward by width/4 twice!
        // Oh! Wait: let's check if [bx] is left and [bx+4] is right: yes!
        self.frame_rect_color(surface, r, self.pen.color);
        r.inflate(-step, -step);
        self.draw_diamond(surface, r);
        self.pen = saved;
    }

    /// `ord_27` (`1:339F`): 8-way symmetric XOR lines around logical `(0, 0)`.
    fn draw_8way_lines(&mut self, surface: &mut AdSurface, x1: i32, y1: i32, x2: i32, y2: i32) {
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        self.draw_line(surface, y1, x1, y2, x2);
        self.draw_line(surface, -y1, -x1, -y2, -x2);
        self.draw_line(surface, x1, y1, x2, y2);
        self.draw_line(surface, -x1, -y1, -x2, -y2);
        self.draw_line(surface, x1, -y1, x2, -y2);
        self.draw_line(surface, -x1, y1, -x2, y2);
        self.draw_line(surface, -y1, x1, -y2, x2);
        self.draw_line(surface, y1, -x1, y2, -x2);
        self.pen = saved;
    }

    /// `ord_26` (`1:31D4`): nested frames + optional 8-way connecting web lines.
    fn draw_nested_web(
        &mut self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        cy: i32,
        cx: i32,
        r: RectI,
        count: i32,
        step: i32,
    ) {
        let do_lines = Self::rand_upto(rng, 1) != 0;
        let saved = self.pen;
        self.pen.rop2 = R2_XORPEN;
        let mut cur = r;
        let mut next = r;
        for _ in 1..=count {
            self.frame_rect_color(surface, cur, self.pen.color);
            let y1 = cur.bottom - cy;
            let x1 = (cur.right + cur.left) / 2 - cx;
            next.inflate(-step, -step);
            let x2 = next.right - cx;
            let y2 = next.bottom - cy;
            if do_lines {
                self.set_window_org(-cx, -cy);
                self.draw_8way_lines(surface, y1, x1, y2, x2);
                self.set_window_org(0, 0);
            }
            cur = next;
        }
        self.pen = saved;
    }

    /// `ord_25` (`1:34DE`) + `1:3522`: draw X diagonals across center rect in white or black.
    fn draw_tile_diagonals(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand, r: RectI) {
        let outer_white = Self::rand_upto(rng, 1) != 0;
        if outer_white {
            self.set_color_idx(1);
        }
        let inner_white = Self::rand_upto(rng, 1) != 0;
        if inner_white {
            self.set_color_idx(1);
        }
        self.draw_line(surface, r.right, r.top, r.left, r.bottom);
        self.draw_line(surface, r.left, r.top, r.right, r.bottom);
        if inner_white || outer_white {
            self.set_color_idx(0);
        }
    }

    /// `ord_24` (`1:38C0`): optional inscribed circle + XOR circles on tile completion.
    fn draw_tile_circles(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand, r: RectI) {
        let mut inner = r;
        let white = Self::rand_upto(rng, 1) != 0;
        if white {
            self.set_color_idx(1);
        }
        self.draw_hollow_ellipse(surface, r);
        if white {
            self.set_color_idx(0);
        }
        if Self::rand_upto(rng, 1) != 0 {
            self.draw_xor_ellipse(surface, r);
            inner.inflate(-16, -16);
            if Self::rand_upto(rng, 1) != 0 {
                self.draw_xor_ellipse(surface, inner);
            }
        }
    }

    /// `ord_23` (`1:26F1`): concentric random-radius inverted squares and XOR circles around origin.
    fn draw_concentric_inverts(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand, r: RectI) {
        let w = r.width();
        if Self::rand_upto(rng, 4) != 1 {
            return;
        }
        let mut rad = w / 2;
        if Self::rand_upto(rng, 1) != 0 {
            while rad > 0 {
                rad = Self::rand_upto(rng, rad);
                if rad < 3 {
                    break;
                }
                let sq = RectI::new(-(rad - 1), -(rad - 1), rad, rad);
                self.invert_rect(surface, sq);
                rad -= 6;
            }
        }
        if Self::rand_upto(rng, 1) != 0 {
            rad = w;
            while rad > 0 {
                rad = Self::rand_upto(rng, rad);
                if rad < 3 {
                    break;
                }
                let sq = RectI::new(-(rad - 1), -(rad - 1), rad, rad);
                self.draw_xor_ellipse(surface, sq);
                rad -= 6;
            }
        }
    }

    /// `ord_28` (`1:3C95`): `BitBlt` (if same size) or `StretchBlt` from `src` to `dst`.
    fn copy_rect(&self, surface: &mut AdSurface, src: RectI, dst: RectI) {
        let s = self.to_dev_rect(src);
        let d = self.to_dev_rect(dst);
        let sw = s.right - s.left;
        let sh = s.bottom - s.top;
        let dw = d.right - d.left;
        let dh = d.bottom - d.top;
        if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
            return;
        }
        if sw == dw && sh == dh {
            surface.bit_blt_self(d.left, d.top, sw, sh, s.left, s.top, ROP_SRCCOPY);
        } else {
            surface.stretch_blt_self(d.left, d.top, dw, dh, s.left, s.top, sw, sh, ROP_SRCCOPY);
        }
    }

    /// `ord_20` (`1:3B0A`): copy `src` into all 8 surrounding tiles of a 3×3 grid starting at `tl_dst`.
    fn tile_3x3(&self, surface: &mut AdSurface, src: RectI, tl_dst: RectI) {
        let w = src.width();
        let h = src.height();
        let mut cur = tl_dst;
        let mut row_start = tl_dst;

        // Row 0: (0,0), (1,0), (2,0)
        self.copy_rect(surface, src, cur);
        cur.offset(w, 0);
        self.copy_rect(surface, src, cur);
        cur.offset(w, 0);
        self.copy_rect(surface, src, cur);

        // Row 1: (0,1), (1,1) if != src, (2,1)
        cur = row_start;
        cur.offset(0, h);
        self.copy_rect(surface, src, cur);
        row_start = cur;
        cur.offset(w, 0);
        if cur != src {
            self.copy_rect(surface, src, cur);
        }
        cur.offset(w, 0);
        self.copy_rect(surface, src, cur);

        // Row 2: (0,2), (1,2), (2,2)
        cur = row_start;
        cur.offset(0, h);
        self.copy_rect(surface, src, cur);
        cur.offset(w, 0);
        self.copy_rect(surface, src, cur);
        cur.offset(w, 0);
        self.copy_rect(surface, src, cur);
    }

    /// `ord_19` (`1:3DAF`): 3×3 tiling with optional corner/cardinal skipping, blacking, and checkerboard inversion!
    fn tile_3x3_fancy(
        &self,
        surface: &mut AdSurface,
        rng: &mut MsvcRand,
        src: RectI,
        tl_dst: RectI,
    ) {
        let w = src.width();
        let h = src.height();
        let erase_skipped = Self::rand_upto(rng, 3) == 1;

        let mut do_card = true;
        let mut do_corn = true;
        let mut inv_card = false;
        let mut inv_corn = false;

        if Self::rand_upto(rng, 1) != 0 {
            do_card = Self::rand_upto(rng, 1) != 0;
            do_corn = Self::rand_upto(rng, 1) != 0;
            if !do_card && !do_corn {
                do_card = true;
                do_corn = true;
            }
        } else if Self::rand_upto(rng, 1) != 0 {
            inv_card = Self::rand_upto(rng, 1) != 0;
            inv_corn = !inv_card;
        }

        let mut card_src = src; // 0x816
        let mut corn_src = src; // 0x82E
        let mut last_card = src; // 0x836
        let mut last_corn = src; // 0x83E
        let mut cur = tl_dst; // 0x806
        let mut row_start = tl_dst; // 0x826

        let black = rgb(0, 0, 0);

        // (0,0) corner
        if do_corn {
            self.copy_rect(surface, corn_src, cur);
            last_corn = cur;
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        // (1,0) cardinal
        cur.offset(w, 0);
        if do_card {
            self.copy_rect(surface, src, cur);
            last_card = cur;
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        // Checkerboard inversion check (1:3F2E..3F95)
        let clipped = cur.intersect(&self.screen_rect);
        if clipped == cur && do_corn && do_card && (inv_card || inv_corn) {
            if inv_card {
                card_src = last_card;
                self.invert_rect(surface, card_src);
            } else {
                corn_src = last_corn;
                self.invert_rect(surface, corn_src);
            }
        }

        // (2,0) corner
        cur.offset(w, 0);
        if do_corn {
            self.copy_rect(surface, corn_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        // Row 1: (0,1) cardinal
        cur = row_start;
        cur.offset(0, h);
        if do_card {
            self.copy_rect(surface, card_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }
        row_start = cur;

        // Skip center (1,1), go to (2,1) cardinal
        cur.offset(w, 0);
        cur.offset(w, 0);
        if do_card {
            self.copy_rect(surface, card_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        // Row 2: (0,2) corner, (1,2) cardinal, (2,2) corner
        cur = row_start;
        cur.offset(0, h);
        if do_corn {
            self.copy_rect(surface, corn_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        cur.offset(w, 0);
        if do_card {
            self.copy_rect(surface, card_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }

        cur.offset(w, 0);
        if do_corn {
            self.copy_rect(surface, corn_src, cur);
        } else if erase_skipped {
            self.fill_rect_color(surface, cur, black);
        }
    }

    /// `ord_22` (`1:3958`): stamp `src` around the 5×5 outer perimeter ring (or erase it black).
    fn stamp_outer_ring(&self, surface: &mut AdSurface, rng: &mut MsvcRand, src: RectI) -> bool {
        let w = src.width();
        let erase_mode = Self::rand_upto(rng, 8) == 1;
        let black = rgb(0, 0, 0);

        let mut tl = src;
        tl.offset(-2 * w, -2 * w);
        let mut r_top = tl;
        let mut r_left = tl;

        let mut br = src;
        br.offset(2 * w, 2 * w);
        let mut r_bot = br;
        let mut r_right = br;

        for _ in 1..=5 {
            if erase_mode {
                self.fill_rect_color(surface, r_top, black);
            } else {
                self.copy_rect(surface, src, r_top);
            }
            r_top.offset(w, 0);

            if erase_mode {
                self.fill_rect_color(surface, r_left, black);
            } else {
                self.copy_rect(surface, src, r_left);
            }
            r_left.offset(0, w);

            if erase_mode {
                self.fill_rect_color(surface, r_bot, black);
            } else {
                self.copy_rect(surface, src, r_bot);
            }
            r_bot.offset(-w, 0);

            if erase_mode {
                self.fill_rect_color(surface, r_right, black);
            } else {
                self.copy_rect(surface, src, r_right);
            }
            r_right.offset(0, -w);
        }

        erase_mode
    }

    /// `1:1CF6`: take inner 1/3 core of `center_rect` and tile it 3×3 around itself.
    fn shrink_and_tile(&self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        let mut inner = self.center_rect;
        let w = inner.width();
        let inset = w / -3; // negative -> shrinks rect to 1/3 width
        inner.inflate(inset, inset);
        let src = inner;
        let mut tl = inner;
        tl.offset(-src.width(), -src.height());
        if Self::rand_upto(rng, 4) != 0 {
            self.tile_3x3(surface, src, tl);
        } else {
            self.tile_3x3_fancy(surface, rng, src, tl);
        }
    }

    /// `1:1DA4`: stamp `center_rect` at 2-tile cardinal and/or 2-tile diagonal positions.
    fn stamp_2step(&self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        let src = self.center_rect;
        let w = src.width();
        let h = src.height();
        if Self::rand_upto(rng, 1) != 0 {
            for (dx, dy) in [(0, -2 * h), (0, 2 * h), (-2 * w, 0), (2 * w, 0)] {
                let mut d = src;
                d.offset(dx, dy);
                self.copy_rect(surface, src, d);
            }
        }
        if Self::rand_upto(rng, 1) != 0 {
            for (dx, dy) in [
                (2 * w, -2 * h),
                (-2 * w, 2 * h),
                (-2 * w, -2 * h),
                (2 * w, 2 * h),
            ] {
                let mut d = src;
                d.offset(dx, dy);
                self.copy_rect(surface, src, d);
            }
        }
    }

    /// `1:0C80`: clear entire screen black, reset tile counter, request new tile.
    fn clear_black_and_reset(&mut self, surface: &mut AdSurface) {
        self.set_color_idx(0);
        surface.fill_black();
        self.need_new_tile = true;
        self.prev_wedge_flag = 0;
        self.tile_count = 0;
        self.set_color_idx(1);
    }

    /// `1:0CC7`: clear entire screen to a chosen color index and request new tile.
    fn clear_color_and_reset(&mut self, surface: &mut AdSurface, color_idx: i32) {
        self.set_color_idx(color_idx);
        self.fill_rect_color(surface, self.screen_rect, self.pen.color);
        self.need_new_tile = true;
        self.prev_wedge_flag = 0;
        self.set_color_idx(1);
    }

    /// `1:0F4F..1:19C5`: initialize a brand-new central stained-glass tile.
    fn init_new_tile(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        if Self::rand_chance(rng, 1, 30) {
            let c = self.pick_color_idx(rng);
            self.clear_color_and_reset(surface, c);
        }
        if self.is_color {
            self.set_color_maybe_rand(rng, self.num_colors);
        }
        if self.color_param == 1 {
            let chosen = Self::rand_upto(rng, 15);
            self.configure_sub_palette(rng, chosen);
        }

        self.tile_count += 1;
        self.need_new_tile = false;
        self.draw_scan_pass = false;
        self.run_steps = true;
        self.mid_tile_enabled = Self::rand_upto(rng, 1) != 0;

        let sw = self.screen_rect.width();
        let sh = self.screen_rect.height();
        let r_wide = (sw / 7) / 2;
        let r_tall = (sh / 3) / 2;
        let mut r = (sh / 5) / 2;
        if sh > 401 {
            r = r_wide;
        }
        if Self::rand_upto(rng, 8) == 3 {
            r = r_tall;
        } else if Self::rand_upto(rng, 5) == 3 {
            r = r_wide;
        }
        r = (r | 1).max(7);
        self.radius = r;

        let cx = sw / 2;
        let cy = sh / 2;
        self.center_x = cx;
        self.center_y = cy;
        self.center_rect = RectI::new(cx - r, cy - r, cx + r, cy + r);

        if Self::rand_upto(rng, 5) == 1 {
            if Self::rand_upto(rng, 1) == 0 || Self::rand_upto(rng, 1) != 0 {
                self.invert_rect(surface, self.center_rect);
            }
        }

        self.do_shrink_tile = Self::rand_chance(rng, 1, 12);
        self.do_outer_3x3_a = Self::rand_chance(rng, 1, 12);
        self.do_outer_3x3_b = Self::rand_chance(rng, 1, 20);
        self.do_outer_ring = Self::rand_chance(rng, 1, 20);

        self.dot_color_on = 1;
        self.dot_color_off = 0;
        if self.is_color {
            self.dot_color_on = self.pick_color_idx(rng);
            self.dot_color_off = Self::rand_upto(rng, self.num_colors);
            if self.dot_color_on == self.dot_color_off {
                self.dot_color_off = Self::rand_upto(rng, self.num_colors);
            }
            if Self::rand_upto(rng, 3) == 0 {
                self.dot_color_off = 0;
            }
        }

        self.walkers_enabled = true;
        if self.num_walkers == 0 {
            let _ = Self::rand_upto(rng, 3);
        }

        // Initialize 4 walkers (1:11B3..1:137C)
        for i in 0..4 {
            let color_idx = if self.is_color {
                self.pick_color_idx(rng)
            } else {
                Self::rand_upto(rng, 1)
            };
            let _ = Self::rand_upto(rng, 1); // 1:11CF overwritten immediately by 1 at 1:11DB
            let draw_mode = Self::rand_upto(rng, 3);
            let step_len = (Self::rand_upto(rng, 4) + 1) << 2;
            let dir = Self::rand_upto(rng, 7);
            let follow_prev = Self::rand_upto(rng, 1);
            let stamp_burst = if Self::rand_upto(rng, 2) < 1 { 1 } else { 0 };
            let mut brush_size = (Self::rand_upto(rng, 2) << 1) + 1;
            if Self::rand_upto(rng, 3) != 0 {
                brush_size = 0;
            }
            let pixel_color = Self::rand_upto(rng, self.num_colors);

            let (mut wx, mut wy) = match i {
                0 => (0, 0),
                1 => (r, Self::rand_upto(rng, 20) + r),
                _ => (r / 2, Self::rand_upto(rng, 20) + r / 2),
            };
            if (i & 1) != 0 {
                wx = wy;
            }
            if i == 2 {
                wy = 0;
                wx = r / 2;
            }
            self.walkers[i] = Walker {
                x: wx,
                y: wy,
                dir,
                step_len,
                step_cur: 1,
                draw_mode,
                is_rect_stamp: 1,
                color_idx,
                follow_prev,
                stamp_burst,
                brush_size,
                pixel_color,
            };
        }

        // Scanline pixel-dither parameters (1:137D..1:16A6)
        self.mask_row = -1;
        self.mask_col = -1;
        if Self::rand_upto(rng, 15) == 3 {
            self.mask_row = 1;
        } else if Self::rand_upto(rng, 15) == 3 {
            self.mask_col = 1;
        }
        let _ = Self::rand_upto(rng, 1);
        self.fg_color_idx = 1;
        self.dot_thresh_base = 1;
        self.dot_thresh = Self::rand_upto(rng, 50) + 25;
        self.dot_range = 100;
        self.dot_thresh_step = 1;
        self.dot_thresh_reset = 1;
        self.extra_shapes = Self::rand_upto(rng, 6) == 2;
        self.motif_active = false;

        let _ = Self::rand_upto(rng, 4);
        self.margin_inset = if Self::rand_upto(rng, 4) != 0 {
            0
        } else {
            Self::rand_upto(rng, 4) << 2
        };
        self.draw_axes_always = Self::rand_upto(rng, 1) != 0;
        self.scatter_flag = Self::rand_upto(rng, 6) < 1;
        self.rev_diag_slope = 0;
        self.wedge_start_row = 0;
        self.wedge_slope_step = 0;
        self.wedge_offset = 0;

        let _ = Self::rand_upto(rng, 1);
        self.diag_slope = Self::rand_upto(rng, 2);

        self.row_end = r;
        self.col_end = r;
        self.row_start = 0;
        self.col_start = 0;
        self.col_min = 0;
        self.row_step = 1;

        if Self::rand_upto(rng, 2) == 1 {
            self.row_start = r;
            if Self::rand_upto(rng, 4) == 4 {
                self.row_start = r / 2;
            }
            self.row_step = -1;
        } else {
            self.row_start = if Self::rand_upto(rng, 1) != 0 {
                Self::rand_upto(rng, (r / 2).max(1))
            } else {
                0
            };
            self.row_step = 1;
        }

        if Self::rand_upto(rng, 1) != 0 && self.row_step > 0 {
            self.diag_slope = 1;
            self.wedge_offset = 1;
            self.wedge_slope_step = match Self::rand_upto(rng, 2) {
                0 => 3,
                1 => 1,
                _ => 2,
            };
        }

        match Self::rand_upto(rng, 6) {
            0 => {
                self.dot_thresh_step = 0;
                self.dot_thresh = Self::rand_upto(rng, 50) + 30;
            }
            1 => {
                if Self::rand_upto(rng, 1) != 0 {
                    self.dot_thresh_step = 1;
                    self.dot_thresh = 20;
                } else {
                    self.dot_thresh_step = -1;
                    self.dot_thresh = 70;
                }
            }
            2 => {
                if Self::rand_upto(rng, 1) != 0 {
                    self.dot_thresh_step = -2;
                    self.dot_thresh = 70;
                } else {
                    self.dot_thresh_step = 2;
                    self.dot_thresh = 20;
                }
            }
            4 | 6 => {}
            _ => {
                self.dot_thresh_step = 2;
                self.dot_thresh = 50;
            }
        }

        if Self::rand_upto(rng, 3) == 2 && self.row_step != 0 {
            self.row_end = r / 2;
            self.rev_diag_slope = Self::rand_upto(rng, 3) + 1;
            self.row_start = Self::rand_upto(rng, 10);
        }
        self.row_end -= self.margin_inset;

        if self.wedge_slope_step != 0 {
            if Self::rand_upto(rng, 1) != 0 && self.prev_wedge_flag != 0 {
                let d = -2 - Self::rand_upto(rng, 4);
                self.wedge_start_row = r + r / d;
            } else {
                self.wedge_start_row = r - r / 2;
            }
            if self.prev_wedge_flag == 0 {
                self.dot_thresh_step = if Self::rand_upto(rng, 1) != 0 { -4 } else { 4 };
                self.dot_thresh = 50;
            }
        }
        self.dot_thresh_base = self.dot_thresh;

        // Initial decorative XOR motifs (1:16AD..1:179E)
        if Self::rand_upto(rng, 7) == 2 && self.tile_count > 5 {
            let mut big = self.center_rect;
            let mut motif_rect = self.center_rect;
            big.inflate(r, r);
            if Self::rand_upto(rng, 3) == 2 {
                motif_rect = big;
            }
            if Self::rand_upto(rng, 4) != 2 {
                self.set_color_maybe_rand(rng, self.num_colors);
                self.fill_rect_color(surface, big, rgb(0, 0, 0));
            }
            self.set_window_org(-cx, -cy);
            if self.is_color {
                self.set_color_maybe_rand(rng, self.num_colors);
            }
            self.motif_active = self.draw_initial_motif(surface, rng, motif_rect);
            self.run_steps = if self.motif_active {
                Self::rand_upto(rng, 2) < 1
            } else {
                true
            };
            self.set_window_org(0, 0);
        }

        if self.motif_active && Self::rand_upto(rng, 8) != 2 {
            self.margin_inset = Self::rand_upto(rng, 2) << 2;
            self.row_end -= self.margin_inset;
            let div = Self::rand_upto(rng, 2) + 1;
            self.row_end /= div;
            if Self::rand_upto(rng, 3) != 1 {
                self.col_end /= 2;
            }
        }

        self.scan_row = self.row_start;
        self.extra_shapes = false;
        self.shadow_enabled = Self::rand_upto(rng, 4) == 1;
        self.shadow_dx = Self::rand_upto(rng, 20);
        self.shadow_dy = Self::rand_upto(rng, 20);

        // Optional nested diamonds, corner circles, symmetric rays, and nested web (1:1835..1:19B2)
        if Self::rand_upto(rng, 5) == 4 {
            self.draw_nested_diamonds(surface, self.center_rect);
        }
        if Self::rand_upto(rng, 5) == 4 {
            if Self::rand_upto(rng, 1) != 0 {
                let shift = if Self::rand_upto(rng, 1) != 0 {
                    r / 8
                } else {
                    Self::rand_upto(rng, (r / 2).max(1))
                };
                self.draw_quadrant_ellipses(surface, self.center_rect, shift);
            } else if Self::rand_upto(rng, 1) != 0 {
                // ord_14: shift 0 then -5
                self.draw_quadrant_ellipses(surface, self.center_rect, 0);
                self.draw_quadrant_ellipses(surface, self.center_rect, -5);
            } else {
                // ord_13: shift 0 then +5
                self.draw_quadrant_ellipses(surface, self.center_rect, 0);
                self.draw_quadrant_ellipses(surface, self.center_rect, 5);
            }
        }
        if Self::rand_upto(rng, 4) == 0 {
            let (p1y, p1x) = if Self::rand_upto(rng, 1) != 0 {
                (-Self::rand_upto(rng, (r - 1).max(1)), -r)
            } else {
                (-r, -Self::rand_upto(rng, (r - 1).max(1)))
            };
            let (p2y, p2x) = if Self::rand_upto(rng, 1) != 0 {
                (Self::rand_upto(rng, (r - 1).max(1)), r)
            } else {
                (r, Self::rand_upto(rng, (r - 1).max(1)))
            };
            self.set_window_org(-cx, -cy);
            self.draw_8way_lines(surface, p1y, p1x, p2y, p2x);
            self.set_window_org(0, 0);
        }
        if Self::rand_upto(rng, 8) == 4 {
            let count = Self::rand_upto(rng, 4) + 1;
            self.draw_nested_web(surface, rng, cy, cx, self.center_rect, count, 30);
        }
        let _ = Self::rand_upto(rng, 1);
    }

    /// `1:2012`: advance the current tile by one scanline + walker burst.
    fn step_current_tile(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        let saved = self.pen;
        self.set_color_idx(self.fg_color_idx);
        let cx = self.center_x;
        let cy = self.center_y;
        self.set_window_org(-cx, -cy);

        let cur_row = self.scan_row;

        // Mid-tile periodic duplication (1:20DB..1:2204)
        let do_mid_dup = self.dup_countdown == 1
            || (self.mid_tile_enabled && cur_row != 0 && (cur_row & 0x0F) == 0);
        if do_mid_dup {
            self.set_window_org(0, 0);
            if Self::rand_upto(rng, 4) != 0 {
                self.set_color_idx(0);
            }
            let src = self.center_rect;
            let mut tl = src;
            tl.offset(-src.width(), -src.height());

            if Self::rand_upto(rng, 9) == 1 {
                self.shrink_and_tile(surface, rng);
            }
            if Self::rand_upto(rng, 99) < self.duplication {
                if Self::rand_upto(rng, 3) == 1 {
                    if Self::rand_upto(rng, 2) == 1 {
                        self.tile_3x3(surface, src, tl);
                    } else {
                        self.tile_3x3_fancy(surface, rng, src, tl);
                    }
                } else if Self::rand_upto(rng, 2) != 0 {
                    self.stamp_2step(surface, rng);
                } else if self.stamp_outer_ring(surface, rng, src) {
                    self.mid_tile_enabled = false;
                }
            }
            self.set_color_idx(self.fg_color_idx);
            self.set_window_org(-cx, -cy);
        }

        if self.dup_countdown != 0 {
            self.dup_countdown -= 1;
            if self.dup_countdown <= 0 {
                self.dup_countdown = self.dup_interval;
            }
        }

        if self.extra_shapes && (cur_row & 1) != 0 {
            self.draw_periodic_accents(surface, rng, self.center_rect);
        }

        let mut thresh = if self.dot_thresh_reset != 0 {
            self.dot_thresh_base
        } else {
            self.dot_thresh
        };

        // Step walkers 6 iterations (0..=5) across `num_walkers` (1:2241..1:2468)
        if self.walkers_enabled && self.num_walkers > 0 {
            let w_saved = self.pen;
            let count = (self.num_walkers as usize).min(4);
            for _iter in 0..=5 {
                for wi in 0..count {
                    let w = self.walkers[wi];
                    self.pen.rop2 = if w.draw_mode < 1 {
                        R2_XORPEN
                    } else {
                        R2_COPYPEN
                    };
                    self.set_color_idx(w.color_idx);

                    let mut need_turn = false;
                    if w.step_len != 0 {
                        let prev = self.walkers[wi].step_cur;
                        self.walkers[wi].step_cur = prev + 1;
                        if w.step_len <= prev {
                            need_turn = true;
                        }
                    } else {
                        need_turn = true;
                    }

                    if need_turn {
                        let w_now = self.walkers[wi];
                        if w_now.step_len > 3
                            && w_now.step_len <= w_now.step_cur
                            && w_now.stamp_burst != 0
                            && Self::rand_upto(rng, 9) < 1
                        {
                            if self.is_color {
                                self.set_color_maybe_rand(rng, self.num_colors);
                            }
                            let sz = Self::rand_upto(rng, 24) | 15;
                            self.stamp_walker_burst(surface, rng, w_now.x, w_now.y, sz);
                            if Self::rand_upto(rng, 1) != 0 {
                                self.stamp_walker_burst(
                                    surface,
                                    rng,
                                    w_now.x,
                                    w_now.y,
                                    (sz / 2) + 1,
                                );
                            }
                            self.set_color_idx(w_now.color_idx);
                        }
                        self.walkers[wi].step_cur = 1;
                        let old_dir = self.walkers[wi].dir.clamp(0, 8) as usize;
                        loop {
                            let nd = Self::rand_upto(rng, 8);
                            if nd != 4 && nd != OPPOSITE_DIR[old_dir] {
                                self.walkers[wi].dir = nd;
                                break;
                            }
                        }
                    }

                    // Move walker (or follow previous walker's direction)
                    let dir_idx = if wi > 0 && self.walkers[wi].follow_prev != 0 {
                        self.walkers[wi - 1].dir.clamp(0, 8) as usize
                    } else {
                        self.walkers[wi].dir.clamp(0, 8) as usize
                    };
                    let (dx, dy) = DIR_DELTAS[dir_idx];
                    self.walkers[wi].x += dx;
                    self.walkers[wi].y += dy;

                    let wn = self.walkers[wi];
                    if wn.brush_size > 1 && wn.draw_mode != 0 {
                        self.stamp_walker_brush(
                            surface,
                            rng,
                            wn.x,
                            wn.y,
                            wn.brush_size,
                            wn.is_rect_stamp != 0,
                        );
                        if wn.pixel_color != 0 {
                            self.plot_8way(surface, wn.y, wn.x, wn.pixel_color);
                        }
                    } else {
                        self.plot_8way(surface, wn.y, wn.x, wn.color_idx);
                    }
                    self.set_color_idx(self.fg_color_idx);
                }
            }
            self.pen = w_saved;
        }

        // Scanline 8-way dithered pixel pass (1:2469..1:25BC)
        let mut col = self.col_start;
        while col <= self.col_end && col >= self.col_min {
            if self.rev_diag_slope == 0 {
                if self.diag_slope * col >= cur_row + 1 {
                    break;
                }
                if self.wedge_slope_step != 0
                    && self.wedge_start_row < cur_row
                    && self.wedge_offset + col >= cur_row + 1
                {
                    self.wedge_offset += self.wedge_slope_step;
                    break;
                }
            } else {
                let lim = (-1 - cur_row) * self.rev_diag_slope + self.radius;
                if lim <= col {
                    break;
                }
            }

            let mask_ok = ((self.mask_row & cur_row) != 0 && (self.mask_col & col) != 0)
                || (self.draw_axes_always && (cur_row == 0 || col == 0));

            if mask_ok && !(self.draw_scan_pass && self.walkers_enabled) {
                let on = Self::rand_upto(rng, self.dot_range) < thresh;
                let c_idx = if on {
                    self.dot_color_on
                } else {
                    self.dot_color_off
                };
                self.plot_8way(surface, cur_row, col, c_idx);

                if self.shadow_enabled {
                    self.set_color_idx(self.fg_color_idx);
                    let sc = if on {
                        self.fg_color_idx
                    } else if self.fg_color_idx == 0 {
                        1
                    } else {
                        0
                    };
                    self.plot_8way(
                        surface,
                        cur_row + self.shadow_dx,
                        col + self.shadow_dy,
                        sc,
                    );
                    self.set_color_idx(self.fg_color_idx);
                }
            }

            col += 1;
            thresh += self.dot_thresh_step;
        }

        // Optional random scatter dots across tile (1:25BD..1:263B)
        if !self.motif_active && self.scatter_flag {
            while col <= self.col_end && col >= 0 {
                if (col & 1) != 0 {
                    let c_idx = if Self::rand_upto(rng, 1) == 0 {
                        0
                    } else if self.is_color {
                        self.pick_color_idx(rng)
                    } else {
                        1
                    };
                    let rx = Self::rand_upto(rng, (self.radius - 1).max(1));
                    let ry = Self::rand_upto(rng, (self.radius - 1).max(1));
                    self.plot_8way(surface, rx, ry, c_idx);
                    self.set_color_idx(self.fg_color_idx);
                }
                col += 1;
            }
        }

        self.set_window_org(0, 0);
        self.pen = saved;
    }

    /// `1:1A22..1:1CE1`: finish a tile — optional overlays + full-screen duplication cascade!
    fn finish_tile(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        self.tile_done = false;
        self.need_new_tile = true;
        self.prev_wedge_flag = self.wedge_offset;

        if Self::rand_upto(rng, 2) == 0 || (self.diag_slope == 2 && Self::rand_upto(rng, 1) != 0) {
            self.draw_tile_diagonals(surface, rng, self.center_rect);
        }
        if Self::rand_upto(rng, 4) == 0 {
            self.draw_tile_circles(surface, rng, self.center_rect);
        }
        if Self::rand_upto(rng, 8) == 0 {
            self.set_window_org(-self.center_x, -self.center_y);
            self.draw_concentric_inverts(surface, rng, self.center_rect);
            self.set_window_org(0, 0);
        }
        if self.do_shrink_tile {
            self.shrink_and_tile(surface, rng);
        }

        // Large inflated 3×3 copy A (3x tile size!) (1:1AC3..1:1B52)
        if Self::rand_upto(rng, 99) < self.duplication && self.do_outer_3x3_a {
            let mut big = self.center_rect;
            let w = big.width();
            big.inflate(w, w);
            let src = big;
            let mut tl = big;
            tl.offset(-src.width(), -src.height());
            self.tile_3x3(surface, src, tl);
        }

        // Medium inflated 3×3 copy B (2x tile size!) (1:1B52..1:1BE6)
        if Self::rand_upto(rng, 99) < self.duplication && self.do_outer_3x3_b {
            let mut med = self.center_rect;
            let w = med.width();
            med.inflate(w / 2, w / 2);
            let src = med;
            let mut tl = med;
            tl.offset(-src.width(), -src.height());
            self.tile_3x3(surface, src, tl);
        }

        // 5×5 outer perimeter ring (1:1BE6..1:1C1A)
        if (self.do_outer_ring || Self::rand_chance(rng, 1, 4))
            && Self::rand_upto(rng, 99) < self.duplication
        {
            self.stamp_outer_ring(surface, rng, self.center_rect);
        }

        // Inverted 2-step outer stamp (1:1C1A..1:1C62)
        if Self::rand_chance(rng, 1, 8) && Self::rand_upto(rng, 99) < self.duplication {
            self.invert_rect(surface, self.center_rect);
            self.stamp_2step(surface, rng);
            self.invert_rect(surface, self.center_rect);
        }

        // Standard 3×3 copy around center (1:1C62..1:1CE1)
        let src = self.center_rect;
        let mut tl = src;
        tl.offset(-src.width(), -src.height());
        if Self::rand_upto(rng, 99) < self.duplication && Self::rand_upto(rng, 1) == 1 {
            if Self::rand_upto(rng, 2) == 1 {
                self.tile_3x3(surface, src, tl);
            } else {
                self.tile_3x3_fancy(surface, rng, src, tl);
            }
        }
    }

    /// `1:0EA5`: main quilt tick.
    fn quilt_step(&mut self, surface: &mut AdSurface, rng: &mut MsvcRand) {
        if self.need_new_tile && !self.tile_done {
            self.init_new_tile(surface, rng);
        }

        if self.run_steps && !self.tile_done {
            self.step_current_tile(surface, rng);
            self.scan_row += self.row_step;
        } else {
            self.tile_done = true;
        }

        self.set_color_idx(0);
        if self.scan_row > self.row_end || self.scan_row < 0 {
            self.tile_done = true;
        }
        if self.tile_done {
            self.finish_tile(surface, rng);
        }
        self.set_window_org(0, 0);
    }
}

impl AdModule for StainedGlassModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        *self = Self::default();
        self.screen_rect = RectI::new(0, 0, ctx.surface.width, ctx.surface.height);
        self.is_color = ctx.system.bits_per_pixel >= 4;
        self.need_new_tile = true;
        self.prev_wedge_flag = 0;

        // Prime change-detection fields from initial slider values so the very first frame
        // starts drawing immediately instead of burning a frame on clear_black_and_reset.
        let complexity = ctx.module.control_value[0].clamp(0, 100);
        let color_val = ctx.module.control_value[2].clamp(0, 100);
        self.num_walkers = complexity / 30;
        self.prev_num_walkers = self.num_walkers;
        self.dup_interval = (100 - complexity) * 2;
        self.prev_dup_interval = self.dup_interval;
        self.dup_countdown = self.dup_interval;

        if self.is_color {
            if color_val == 0 {
                self.num_colors = 1;
                self.sub_pal_mode = 0;
            } else if (1..=15).contains(&color_val) {
                let chosen = if color_val == 1 {
                    Self::rand_upto(ctx.rng, 15)
                } else {
                    color_val
                };
                self.configure_sub_palette(ctx.rng, chosen);
            } else {
                self.num_colors = ((color_val / 7) + 1).min(15);
                self.sub_pal_mode = 0;
            }
        } else {
            self.num_colors = 1;
            self.sub_pal_mode = 0;
        }
        self.prev_num_colors = self.num_colors;
        self.prev_color_param = color_val;
        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        let complexity = ctx.module.control_value[0].clamp(0, 100);
        let duplication = ctx.module.control_value[1].clamp(0, 100);
        let color_val = ctx.module.control_value[2].clamp(0, 100);

        // `ord_4` (`1:0D02`) slider update handling
        self.num_walkers = complexity / 30;
        if self.num_walkers != self.prev_num_walkers {
            self.prev_num_walkers = self.num_walkers;
            self.clear_black_and_reset(ctx.surface);
        }

        self.dup_interval = (100 - complexity) * 2;
        if self.dup_interval != self.prev_dup_interval {
            self.prev_dup_interval = self.dup_interval;
            self.dup_countdown = self.dup_interval;
            self.clear_black_and_reset(ctx.surface);
            return AdResult::NoError;
        }

        if self.is_color {
            self.num_colors = if color_val == 0 {
                1
            } else if (1..=15).contains(&color_val) {
                2
            } else {
                ((color_val / 7) + 1).min(15)
            };

            if self.prev_num_colors != self.num_colors || self.prev_color_param != color_val {
                self.prev_num_colors = self.num_colors;
                self.prev_color_param = color_val;
                self.clear_black_and_reset(ctx.surface);
                if (1..=15).contains(&color_val) {
                    let chosen = if color_val == 1 {
                        Self::rand_upto(ctx.rng, 15)
                    } else {
                        color_val
                    };
                    self.configure_sub_palette(ctx.rng, chosen);
                } else {
                    self.sub_pal_mode = 0;
                }
            }
            if self.sub_pal_mode == 1 {
                self.num_colors = 2;
                self.prev_num_colors = 2;
            }
        } else {
            self.sub_pal_mode = 0;
            self.num_colors = 1;
        }

        self.complexity = complexity;
        self.duplication = duplication;
        self.color_param = color_val;

        // `1:0E25..0E7E`: random frame throttle & rare 1-in-100 full clear
        let roll = Self::rand_upto(ctx.rng, 6);
        if roll == 1 {
            if Self::rand_chance(ctx.rng, 1, 100) {
                self.clear_black_and_reset(ctx.surface);
            }
            return AdResult::NoError;
        }
        if roll == 6 {
            return AdResult::NoError;
        }

        self.set_color_idx(1);
        if self.is_color {
            self.set_color_maybe_rand(ctx.rng, self.num_colors);
        }
        self.quilt_step(ctx.surface, ctx.rng);
        self.set_color_idx(1);

        AdResult::NoError
    }

    fn do_close(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        AdResult::NoError
    }
}
