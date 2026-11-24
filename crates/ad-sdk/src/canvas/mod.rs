//! Software GDI: offscreen surface (`AdSurface`), 1991 `.bmp` loader (`AdBitmap`) and regions
//! (`AdRegion`), implemented in pure Rust so the decompiled modules run unchanged on Linux.
//!
//! The API and its semantics mirror the Win16/Win32 GDI calls the Windows port was built on:
//! - Colors passed in are `COLORREF` (`0x00BBGGRR`, see [`crate::palette::rgb`]); the pixel buffer
//!   is 32-bit top-down `0x00RRGGBB`.
//! - Each surface carries DC state: text color (default black), background color (default white),
//!   background mode, ROP2 (pens & shape brushes) and an optional clip region.
//! - Blits support every ternary raster operation (ROP3). Monochrome (1bpp) sources are colorized
//!   like GDI: `0` bits become the destination's text color, `1` bits its background color.
//! - Rectangles/ellipses exclude their right & bottom edges, `LineTo` excludes the last pixel,
//!   `StretchBlt` is nearest-neighbor (`COLORONCOLOR`).

mod raster;
pub mod region;
mod text;

use std::sync::{Arc, RwLock};

use region::Spans;
pub use text::SizedFont;

/// Win32 `RECT`.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

// Ternary raster operations (`ROP_CODE`).
pub const SRCCOPY: u32 = 0x00CC_0020;
pub const SRCPAINT: u32 = 0x00EE_0086;
pub const SRCAND: u32 = 0x0088_00C6;
pub const SRCINVERT: u32 = 0x0066_0046;
pub const SRCERASE: u32 = 0x0044_0328;
pub const NOTSRCCOPY: u32 = 0x0033_0008;
pub const NOTSRCERASE: u32 = 0x0011_00A6;
pub const MERGECOPY: u32 = 0x00C0_00CA;
pub const MERGEPAINT: u32 = 0x00BB_0226;
pub const PATCOPY: u32 = 0x00F0_0021;
pub const PATPAINT: u32 = 0x00FB_0A09;
pub const PATINVERT: u32 = 0x005A_0049;
pub const DSTINVERT: u32 = 0x0055_0009;
pub const BLACKNESS: u32 = 0x0000_0042;
pub const WHITENESS: u32 = 0x00FF_0062;

pub const ROP_SRCCOPY: u32 = SRCCOPY;
pub const ROP_SRCPAINT: u32 = SRCPAINT;
pub const ROP_SRCAND: u32 = SRCAND;
pub const ROP_SRCINVERT: u32 = SRCINVERT;
pub const ROP_SRCERASE: u32 = SRCERASE;
pub const ROP_NOTSRCCOPY: u32 = NOTSRCCOPY;
pub const ROP_NOTSRCERASE: u32 = NOTSRCERASE;
pub const ROP_MERGECOPY: u32 = MERGECOPY;
pub const ROP_MERGEPAINT: u32 = MERGEPAINT;
pub const ROP_PATCOPY: u32 = PATCOPY;
pub const ROP_PATPAINT: u32 = PATPAINT;
pub const ROP_PATINVERT: u32 = PATINVERT;
pub const ROP_DSTINVERT: u32 = DSTINVERT;
pub const ROP_BLACKNESS: u32 = BLACKNESS;
pub const ROP_WHITENESS: u32 = WHITENESS;

// Binary raster operations for pens (`R2_MODE`), all 16.
pub const R2_BLACK: i32 = 1;
pub const R2_NOTMERGEPEN: i32 = 2;
pub const R2_MASKNOTPEN: i32 = 3;
pub const R2_NOTCOPYPEN: i32 = 4;
pub const R2_MASKPENNOT: i32 = 5;
pub const R2_NOT: i32 = 6;
pub const R2_XORPEN: i32 = 7;
pub const R2_NOTMASKPEN: i32 = 8;
pub const R2_MASKPEN: i32 = 9;
pub const R2_NOTXORPEN: i32 = 10;
pub const R2_NOP: i32 = 11;
pub const R2_MERGENOTPEN: i32 = 12;
pub const R2_COPYPEN: i32 = 13;
pub const R2_MERGEPENNOT: i32 = 14;
pub const R2_MERGEPEN: i32 = 15;
pub const R2_WHITE: i32 = 16;

// Background modes (`BACKGROUND_MODE`).
pub const TRANSPARENT: u32 = 1;
pub const OPAQUE: u32 = 2;

// Hatch styles (`HATCH_BRUSH_STYLE`).
pub const HS_HORIZONTAL: i32 = 0;
pub const HS_VERTICAL: i32 = 1;
pub const HS_FDIAGONAL: i32 = 2;
pub const HS_BDIAGONAL: i32 = 3;
pub const HS_CROSS: i32 = 4;
pub const HS_DIAGCROSS: i32 = 5;

/// GDI's 8x8 hatch patterns (`1` = hatch color, `0` = background).
const HATCHES: [[u8; 8]; 6] = [
    [0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00],
    [0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08],
    [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01],
    [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80],
    [0x08, 0x08, 0x08, 0xff, 0x08, 0x08, 0x08, 0x08],
    [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],
];

/// Classic Windows 3.x teal desktop, shown by desktop-effect modules when the real desktop
/// cannot be captured (e.g. under Wayland).
pub const FALLBACK_DESKTOP_PIXEL: u32 = 0x0000_8080;

const RGB_MASK: u32 = 0x00FF_FFFF;

/// `COLORREF` (`0x00BBGGRR`) → pixel (`0x00RRGGBB`). The conversion is its own inverse.
#[inline]
pub const fn colorref_to_pixel(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// Pixel (`0x00RRGGBB`) → `COLORREF` (`0x00BBGGRR`).
#[inline]
pub const fn pixel_to_colorref(p: u32) -> u32 {
    colorref_to_pixel(p)
}

/// Evaluate a ternary raster operation on packed pixels.
#[inline]
fn rop3(code: u8, p: u32, s: u32, d: u32) -> u32 {
    let r = match code {
        0xCC => s,
        0x88 => s & d,
        0xEE => s | d,
        0x66 => s ^ d,
        0xF0 => p,
        0x00 => 0,
        0xFF => !0,
        0x55 => !d,
        0x33 => !s,
        0x5A => p ^ d,
        0xC0 => p & s,
        0xBB => !s | d,
        0x44 => s & !d,
        0x11 => !(s | d),
        0xAA => d,
        _ => {
            let mut r = 0u32;
            for i in 0..8u8 {
                if code >> i & 1 != 0 {
                    let pp = if i & 4 != 0 { p } else { !p };
                    let ss = if i & 2 != 0 { s } else { !s };
                    let dd = if i & 1 != 0 { d } else { !d };
                    r |= pp & ss & dd;
                }
            }
            r
        }
    };
    r & RGB_MASK
}

/// Evaluate a binary raster operation (`R2_*`, 1..=16) on packed pixels.
#[inline]
fn rop2(code: i32, p: u32, d: u32) -> u32 {
    let r = match code {
        R2_COPYPEN => p,
        R2_XORPEN => p ^ d,
        R2_NOT => !d,
        R2_NOP => d,
        _ => {
            let idx = (code.clamp(1, 16) - 1) as u8;
            let mut r = 0u32;
            for i in 0..4u8 {
                if idx >> i & 1 != 0 {
                    let pp = if i & 2 != 0 { p } else { !p };
                    let dd = if i & 1 != 0 { d } else { !d };
                    r |= pp & dd;
                }
            }
            r
        }
    };
    r & RGB_MASK
}

/// Brush used to paint spans.
#[derive(Clone, Copy)]
enum Brush {
    Solid(u32),
    /// 8x8 monochrome pattern aligned to device coordinates. `None` colors are transparent.
    Pattern {
        rows: [u8; 8],
        one: Option<u32>,
        zero: Option<u32>,
    },
}

impl Brush {
    #[inline]
    fn at(&self, x: i32, y: i32) -> Option<u32> {
        match *self {
            Brush::Solid(c) => Some(c),
            Brush::Pattern { rows, one, zero } => {
                if rows[(y & 7) as usize] >> (7 - (x & 7)) & 1 != 0 {
                    one
                } else {
                    zero
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum PaintOp {
    Rop2(i32),
    Rop3(u8),
}

/// Pixel source for blits.
#[derive(Clone, Copy)]
enum Src<'a> {
    Color { px: &'a [u32], w: i32, h: i32 },
    Mono { bits: &'a [u8], w: i32, h: i32 },
}

impl Src<'_> {
    #[inline]
    fn dims(&self) -> (i32, i32) {
        match *self {
            Src::Color { w, h, .. } | Src::Mono { w, h, .. } => (w, h),
        }
    }

    /// Caller guarantees `(x, y)` is in bounds. `fg`/`bg` colorize monochrome bits.
    #[inline]
    fn get(&self, x: i32, y: i32, fg: u32, bg: u32) -> u32 {
        match *self {
            Src::Color { px, w, .. } => px[(y * w + x) as usize],
            Src::Mono { bits, w, .. } => {
                if bits[(y * w + x) as usize] != 0 {
                    bg
                } else {
                    fg
                }
            }
        }
    }
}

enum BitmapPixels {
    /// One byte per pixel, `0` = black (text color), `1` = white (background color).
    Mono(Vec<u8>),
    Color(Vec<u32>),
}

/// A loaded Windows `.bmp` resource. 1bpp bitmaps stay monochrome so text/background color
/// colorization works exactly like a Win16 monochrome DDB.
pub struct AdBitmap {
    data: BitmapPixels,
    pub width: i32,
    pub height: i32,
    pub bpp: u16,
}

impl AdBitmap {
    /// Load a standard `.bmp` file (BITMAPFILEHEADER + BITMAPINFOHEADER + palette + pixels).
    pub fn from_bmp_bytes(bmp: &[u8]) -> Option<Self> {
        if bmp.len() < 54 || &bmp[0..2] != b"BM" {
            return None;
        }
        let rd_u32 = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(bmp.get(o..o + 4)?.try_into().ok()?)) };
        let off_bits = rd_u32(10)? as usize;
        let bi_size = rd_u32(14)? as usize;
        if bi_size < 40 || off_bits > bmp.len() {
            return None;
        }
        let width = rd_u32(18)? as i32;
        let raw_height = rd_u32(22)? as i32;
        let top_down = raw_height < 0;
        let height = raw_height.checked_abs()?;
        let bpp = u16::from_le_bytes(bmp[28..30].try_into().ok()?);
        let clr_used = rd_u32(46)? as usize;
        if width <= 0 || height <= 0 {
            return None;
        }
        let (wu, hu) = (width as usize, height as usize);
        let palette_off = 14 + bi_size;
        let src_row = |y: usize| -> usize {
            let sy = if top_down { y } else { hu - 1 - y };
            off_bits + sy * (wu * bpp as usize).div_ceil(32) * 4
        };
        let byte = |o: usize| bmp.get(o).copied().unwrap_or(0);

        let data = if bpp == 1 {
            // Normalize so 0 = darker palette entry (text color) and 1 = brighter (background).
            let lum = |i: usize| -> u32 {
                let p = palette_off + i * 4;
                byte(p) as u32 + byte(p + 1) as u32 + byte(p + 2) as u32
            };
            let invert = palette_off + 8 <= bmp.len() && lum(0) > lum(1);
            let mut bits = vec![0u8; wu * hu];
            for y in 0..hu {
                let row = src_row(y);
                for x in 0..wu {
                    let bit = byte(row + x / 8) >> (7 - (x % 8)) & 1;
                    bits[y * wu + x] = bit ^ u8::from(invert);
                }
            }
            BitmapPixels::Mono(bits)
        } else {
            let ncolors = if bpp <= 8 {
                if clr_used != 0 { clr_used } else { 1usize << bpp }
            } else {
                0
            };
            let pal: Vec<u32> = (0..ncolors)
                .map(|i| {
                    let p = palette_off + i * 4;
                    (byte(p + 2) as u32) << 16 | (byte(p + 1) as u32) << 8 | byte(p) as u32
                })
                .collect();
            let pal_at = |i: usize| pal.get(i).copied().unwrap_or(0);
            let mut px = vec![0u32; wu * hu];
            for y in 0..hu {
                let row = src_row(y);
                for x in 0..wu {
                    px[y * wu + x] = match bpp {
                        4 => {
                            let b = byte(row + (x >> 1));
                            pal_at(if x & 1 == 0 { b >> 4 } else { b & 0x0F } as usize)
                        }
                        8 => pal_at(byte(row + x) as usize),
                        24 | 32 => {
                            let p = row + x * (bpp as usize / 8);
                            (byte(p + 2) as u32) << 16 | (byte(p + 1) as u32) << 8 | byte(p) as u32
                        }
                        _ => 0,
                    };
                }
            }
            BitmapPixels::Color(px)
        };
        Some(Self {
            data,
            width,
            height,
            bpp,
        })
    }

    fn src(&self) -> Src<'_> {
        match &self.data {
            BitmapPixels::Mono(bits) => Src::Mono {
                bits,
                w: self.width,
                h: self.height,
            },
            BitmapPixels::Color(px) => Src::Color {
                px,
                w: self.width,
                h: self.height,
            },
        }
    }

    /// Is this a 1bpp monochrome bitmap?
    pub fn is_mono(&self) -> bool {
        matches!(self.data, BitmapPixels::Mono(_))
    }
}

/// A GDI region (`HRGN`): `CreateRectRgn`, `CreateEllipticRgn`, `CreatePolygonRgn`, `CombineRgn`,
/// `PtInRegion`.
#[derive(Clone, Debug, Default)]
pub struct AdRegion {
    spans: Spans,
}

impl AdRegion {
    pub fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            spans: Spans::rect(left, top, right, bottom),
        }
    }

    pub fn elliptic(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            spans: raster::ellipse(left, top, right, bottom),
        }
    }

    /// `CreatePolygonRgn(..., WINDING)`.
    pub fn polygon(pts: &[(i32, i32)]) -> Self {
        Self {
            spans: raster::polygon(pts, true),
        }
    }

    pub fn union_with(&mut self, other: &AdRegion) {
        self.spans = self.spans.union(&other.spans);
    }

    pub fn intersect_with(&mut self, other: &AdRegion) {
        self.spans = self.spans.intersect(&other.spans);
    }

    pub fn xor_with(&mut self, other: &AdRegion) {
        self.spans = self.spans.xor(&other.spans);
    }

    pub fn diff_with(&mut self, other: &AdRegion) {
        self.spans = self.spans.diff(&other.spans);
    }

    #[inline]
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.spans.contains(x, y)
    }

    pub fn spans(&self) -> &Spans {
        &self.spans
    }
}

/// Snapshot of the real desktop taken by the host before the saver windows appear, in virtual
/// screen coordinates. Desktop-effect modules read it through [`AdSurface::capture_desktop`].
pub struct DesktopSnapshot {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// `0x00RRGGBB`, row-major, `width * height`.
    pub pixels: Vec<u32>,
}

static DESKTOP: RwLock<Option<Arc<DesktopSnapshot>>> = RwLock::new(None);

/// Install (or clear) the desktop snapshot used by [`AdSurface::capture_desktop`].
pub fn set_desktop_snapshot(snapshot: Option<DesktopSnapshot>) {
    if let Ok(mut g) = DESKTOP.write() {
        *g = snapshot.map(Arc::new);
    }
}

/// Persistent offscreen drawing surface at logical resolution (a memory DC with a 32-bit DIB).
pub struct AdSurface {
    px: Vec<u32>,
    pub width: i32,
    pub height: i32,
    text_color: u32,
    bk_color: u32,
    bk_mode: u32,
    rop2: i32,
    clip: Option<Spans>,
}

impl AdSurface {
    pub fn new(width: i32, height: i32) -> Option<Self> {
        let width = width.max(1);
        let height = height.max(1);
        Some(Self {
            px: vec![0; (width as usize).checked_mul(height as usize)?],
            width,
            height,
            text_color: 0x0000_0000,
            bk_color: 0x00FF_FFFF,
            bk_mode: OPAQUE,
            rop2: R2_COPYPEN,
            clip: None,
        })
    }

    /// Direct access to the 32-bit top-down `0x00RRGGBB` pixel buffer.
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        &self.px
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.px
    }

    // ------------------------------------------------------------------ DC state

    pub fn set_text_color(&mut self, colorref: u32) {
        self.text_color = colorref;
    }

    pub fn set_bk_color(&mut self, colorref: u32) {
        self.bk_color = colorref;
    }

    pub fn set_bk_mode(&mut self, mode: i32) {
        self.bk_mode = mode as u32;
    }

    pub fn set_rop2(&mut self, rop2: i32) -> i32 {
        std::mem::replace(&mut self.rop2, rop2.clamp(1, 16))
    }

    /// `SelectClipRgn`: the region is copied; `None` removes clipping.
    pub fn set_clip_region(&mut self, rgn: Option<&AdRegion>) {
        self.clip = rgn.map(|r| r.spans.clone());
    }

    // ------------------------------------------------------------------ core painters

    /// Visit the parts of `[x0, x1)` on row `y` that survive surface bounds and the clip region.
    #[inline]
    fn for_visible(&self, y: i32, x0: i32, x1: i32, mut f: impl FnMut(i32, i32)) {
        if y < 0 || y >= self.height {
            return;
        }
        let (x0, x1) = (x0.max(0), x1.min(self.width));
        if x0 >= x1 {
            return;
        }
        match &self.clip {
            None => f(x0, x1),
            Some(clip) => {
                for &(c0, c1) in clip.row(y) {
                    let (a, b) = (x0.max(c0), x1.min(c1));
                    if a < b {
                        f(a, b);
                    }
                }
            }
        }
    }

    fn paint_spans(&mut self, cov: &Spans, brush: Brush, op: PaintOp) {
        let w = self.width;
        let mut runs: Vec<(i32, i32)> = Vec::new();
        for (y, row) in cov.iter_rows() {
            runs.clear();
            for &(x0, x1) in row {
                self.for_visible(y, x0, x1, |a, b| runs.push((a, b)));
            }
            let base = (y * w) as usize;
            for &(a, b) in &runs {
                for x in a..b {
                    let Some(p) = brush.at(x, y) else { continue };
                    let d = &mut self.px[base + x as usize];
                    *d = match op {
                        PaintOp::Rop2(c) => rop2(c, p, *d),
                        PaintOp::Rop3(c) => rop3(c, p, 0, *d),
                    };
                }
            }
        }
    }

    /// `FillRect`/`InvertRect`/`PatBlt` rectangle: like USER/GDI (which implement these as
    /// `PatBlt(left, top, right - left, bottom - top)` with signed extents) unordered corners
    /// are normalized rather than rejected. Mondrian relies on this.
    fn paint_rect(&mut self, l: i32, t: i32, r: i32, b: i32, brush: Brush, op: PaintOp) {
        let (l, r) = (l.min(r), l.max(r));
        let (t, b) = (t.min(b), t.max(b));
        self.paint_spans(&Spans::rect(l, t, r, b), brush, op);
    }

    /// Fill then outline a shape with the current ROP2 (GDI `Rectangle`/`Ellipse`/...).
    fn paint_shape(&mut self, fill: Option<(&Spans, u32)>, outline: Option<(&Spans, u32)>) {
        let op = PaintOp::Rop2(self.rop2);
        if let Some((cov, c)) = fill {
            self.paint_spans(cov, Brush::Solid(colorref_to_pixel(c)), op);
        }
        if let Some((cov, c)) = outline {
            self.paint_spans(cov, Brush::Solid(colorref_to_pixel(c)), op);
        }
    }

    /// Generic `BitBlt` / `StretchBlt` (nearest-neighbor) with a ROP3 and pattern color `pat`.
    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        src: Src<'_>,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        pat: u32,
        rop: u32,
    ) {
        if dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0 {
            return;
        }
        let code = (rop >> 16) as u8;
        let (srcw, srch) = src.dims();
        let fg = colorref_to_pixel(self.text_color);
        let bg = colorref_to_pixel(self.bk_color);
        let stretched = sw != dw || sh != dh;
        let w = self.width;
        let map_x = |i: i32| sx + (((2 * i as i64 + 1) * sw as i64) / (2 * dw as i64)) as i32;
        let map_y = |i: i32| sy + (((2 * i as i64 + 1) * sh as i64) / (2 * dh as i64)) as i32;
        let mut runs: Vec<(i32, i32)> = Vec::new();
        for y in dy.max(0)..(dy + dh).min(self.height) {
            let syy = map_y(y - dy);
            if syy < 0 || syy >= srch {
                continue;
            }
            // Restrict to destination x whose source x is inside the source bitmap.
            let (mut xa, mut xb) = (dx, dx + dw);
            if !stretched {
                xa = xa.max(dx - sx);
                xb = xb.min(dx + (srcw - sx));
            }
            runs.clear();
            self.for_visible(y, xa, xb, |a, b| runs.push((a, b)));
            let base = (y * w) as usize;
            for &(a, b) in &runs {
                if !stretched && code == 0xCC
                    && let Src::Color { px, w: pw, .. } = src {
                        let s0 = (syy * pw + (a - dx + sx)) as usize;
                        let n = (b - a) as usize;
                        self.px[base + a as usize..base + a as usize + n]
                            .copy_from_slice(&px[s0..s0 + n]);
                        continue;
                    }
                for x in a..b {
                    let sxx = if stretched { map_x(x - dx) } else { x - dx + sx };
                    if sxx < 0 || sxx >= srcw {
                        continue;
                    }
                    let s = src.get(sxx, syy, fg, bg);
                    let d = &mut self.px[base + x as usize];
                    *d = rop3(code, pat, s, *d);
                }
            }
        }
    }

    /// Copy a rectangle of this surface so it can be used as a blit source onto itself.
    fn snapshot_rect(&self, sx: i32, sy: i32, sw: i32, sh: i32) -> (Vec<u32>, i32, i32, i32, i32) {
        let (x0, y0) = (sx.max(0), sy.max(0));
        let (x1, y1) = ((sx + sw).min(self.width), (sy + sh).min(self.height));
        let (w, h) = ((x1 - x0).max(0), (y1 - y0).max(0));
        let mut buf = Vec::with_capacity((w * h) as usize);
        for y in y0..y0 + h {
            let base = (y * self.width) as usize;
            buf.extend_from_slice(&self.px[base + x0 as usize..base + (x0 + w) as usize]);
        }
        (buf, w, h, x0 - sx, y0 - sy)
    }

    // ------------------------------------------------------------------ fills

    pub fn fill_black(&mut self) {
        self.pat_blt_black(0, 0, self.width, self.height);
    }

    /// `FillRect` with a solid brush (always `PATCOPY`, ignores ROP2).
    pub fn fill_rect_rgb(&mut self, left: i32, top: i32, right: i32, bottom: i32, colorref: u32) {
        let p = colorref_to_pixel(colorref);
        self.paint_rect(left, top, right, bottom, Brush::Solid(p), PaintOp::Rop3(0xF0));
    }

    /// `FrameRect` with a solid brush: a 1-pixel border just inside the rectangle.
    pub fn frame_rect_rgb(&mut self, left: i32, top: i32, right: i32, bottom: i32, colorref: u32) {
        if right <= left || bottom <= top {
            return;
        }
        let ring = Spans::rect(left, top, right, bottom)
            .diff(&Spans::rect(left + 1, top + 1, right - 1, bottom - 1));
        self.paint_spans(&ring, Brush::Solid(colorref_to_pixel(colorref)), PaintOp::Rop3(0xF0));
    }

    fn norm_rect(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let (x, w) = if w < 0 { (x + w, -w) } else { (x, w) };
        let (y, h) = if h < 0 { (y + h, -h) } else { (y, h) };
        (x, y, x + w, y + h)
    }

    pub fn pat_blt_black(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (l, t, r, b) = Self::norm_rect(x, y, w, h);
        self.paint_rect(l, t, r, b, Brush::Solid(0), PaintOp::Rop3(0x00));
    }

    pub fn pat_blt_rgb(&mut self, x: i32, y: i32, w: i32, h: i32, colorref: u32) {
        self.pat_blt_rop(x, y, w, h, colorref, PATCOPY);
    }

    /// `PatBlt` with a solid brush and an arbitrary pattern/destination ROP3.
    pub fn pat_blt_rop(&mut self, x: i32, y: i32, w: i32, h: i32, colorref: u32, rop: u32) {
        let (l, t, r, b) = Self::norm_rect(x, y, w, h);
        let p = colorref_to_pixel(colorref);
        self.paint_rect(l, t, r, b, Brush::Solid(p), PaintOp::Rop3((rop >> 16) as u8));
    }

    /// `PatBlt` with an 8x8 monochrome pattern brush (`CreateBitmap(8, 8, 1, 1, rows)`): `0` bits
    /// take the text color, `1` bits the background color.
    pub fn pat_blt_pattern8x8(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        pattern_rows: &[u16; 8],
        rop: u32,
    ) {
        let mut rows = [0u8; 8];
        for (dst, &src) in rows.iter_mut().zip(pattern_rows) {
            // Word-aligned 1bpp rows: the first byte in memory holds pixels 0..8.
            *dst = src.to_le_bytes()[0];
        }
        let brush = Brush::Pattern {
            rows,
            one: Some(colorref_to_pixel(self.bk_color)),
            zero: Some(colorref_to_pixel(self.text_color)),
        };
        let (l, t, r, b) = Self::norm_rect(x, y, w, h);
        self.paint_rect(l, t, r, b, brush, PaintOp::Rop3((rop >> 16) as u8));
    }

    /// `FillRect` with `CreateHatchBrush(style, color)`; the gaps use the background color in
    /// `OPAQUE` mode and are left untouched in `TRANSPARENT` mode.
    pub fn fill_rect_hatch(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        hatch_style: i32,
        colorref: u32,
    ) {
        let rows = HATCHES[hatch_style.clamp(0, 5) as usize];
        let brush = Brush::Pattern {
            rows,
            one: Some(colorref_to_pixel(colorref)),
            zero: (self.bk_mode == OPAQUE).then(|| colorref_to_pixel(self.bk_color)),
        };
        self.paint_rect(left, top, right, bottom, brush, PaintOp::Rop3(0xF0));
    }

    pub fn invert_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
        self.paint_rect(left, top, right, bottom, Brush::Solid(0), PaintOp::Rop3(0x55));
    }

    pub fn fill_rgn_rgb(&mut self, rgn: &AdRegion, colorref: u32) {
        let p = colorref_to_pixel(colorref);
        self.paint_spans(&rgn.spans, Brush::Solid(p), PaintOp::Rop3(0xF0));
    }

    pub fn invert_rgn(&mut self, rgn: &AdRegion) {
        self.paint_spans(&rgn.spans, Brush::Solid(0), PaintOp::Rop3(0x55));
    }

    // ------------------------------------------------------------------ pixels

    #[inline]
    pub fn set_pixel(&mut self, x: i32, y: i32, colorref: u32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        if let Some(clip) = &self.clip
            && !clip.contains(x, y) {
                return;
            }
        self.px[(y * self.width + x) as usize] = colorref_to_pixel(colorref);
    }

    /// `GetPixel`: returns a `COLORREF`, or `CLR_INVALID` (`0xFFFFFFFF`) when out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 0xFFFF_FFFF;
        }
        pixel_to_colorref(self.px[(y * self.width + x) as usize])
    }

    // ------------------------------------------------------------------ lines & shapes

    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, width: i32, colorref: u32) {
        self.polyline(&[(x1, y1), (x2, y2)], width, colorref);
    }

    pub fn polyline_segments(&mut self, pts: &[(i32, i32)], width: i32, colorref: u32) {
        self.polyline(pts, width, colorref);
    }

    pub fn polyline(&mut self, pts: &[(i32, i32)], width: i32, colorref: u32) {
        if pts.len() < 2 {
            return;
        }
        let cov = raster::stroke_polyline(pts, width);
        self.paint_shape(None, Some((&cov, colorref)));
    }

    pub fn rectangle(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        self.rectangle_pen_width(left, top, right, bottom, 1, pen_color, brush_color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rectangle_pen_width(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        pen_width: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        self.closed_shape(left, top, right, bottom, pen_width, pen_color, brush_color, Spans::rect);
    }

    pub fn ellipse(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        self.ellipse_pen_width(left, top, right, bottom, 1, pen_color, brush_color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse_pen_width(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        pen_width: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        self.closed_shape(left, top, right, bottom, pen_width, pen_color, brush_color, raster::ellipse);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn round_rect(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        rw: i32,
        rh: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        let shape = |l, t, r, b| {
            // Shrinking the box by the pen keeps the corner curvature concentric.
            let shrink = (right - left) - (r - l);
            raster::round_rect(l, t, r, b, (rw - shrink).max(0), (rh - shrink).max(0))
        };
        self.closed_shape(left, top, right, bottom, 1, pen_color, brush_color, shape);
    }

    /// Shared `Rectangle`/`Ellipse`/`RoundRect` logic. With a NULL pen GDI fills a figure one
    /// pixel smaller in each dimension; that quirk is preserved.
    #[allow(clippy::too_many_arguments)]
    fn closed_shape(
        &mut self,
        mut left: i32,
        mut top: i32,
        mut right: i32,
        mut bottom: i32,
        pen_width: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
        shape: impl Fn(i32, i32, i32, i32) -> Spans,
    ) {
        if right < left {
            std::mem::swap(&mut left, &mut right);
        }
        if bottom < top {
            std::mem::swap(&mut top, &mut bottom);
        }
        match pen_color {
            Some(pc) => {
                let (ring, inner) = raster::outlined(left, top, right, bottom, pen_width, &shape);
                self.paint_shape(brush_color.map(|c| (&inner, c)), Some((&ring, pc)));
            }
            None => {
                if let Some(bc) = brush_color {
                    let cov = shape(left, top, right - 1, bottom - 1);
                    self.paint_shape(Some((&cov, bc)), None);
                }
            }
        }
    }

    pub fn polygon(&mut self, pts: &[(i32, i32)], pen_color: Option<u32>, brush_color: Option<u32>) {
        if pts.len() < 3 {
            return;
        }
        let fill = raster::polygon(pts, false);
        match pen_color {
            Some(pc) => {
                let mut closed = pts.to_vec();
                closed.push(pts[0]);
                let outline = raster::stroke_polyline(&closed, 1);
                let interior = fill.diff(&outline);
                self.paint_shape(brush_color.map(|c| (&interior, c)), Some((&outline, pc)));
            }
            None => self.paint_shape(brush_color.map(|c| (&fill, c)), None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arc(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        xr1: i32,
        yr1: i32,
        xr2: i32,
        yr2: i32,
        width: i32,
        pen_color: u32,
    ) {
        let sector = raster::Sector::new(left, top, right, bottom, xr1, yr1, xr2, yr2);
        let (ring, _) = raster::outlined(left, top, right, bottom, width, raster::ellipse);
        let arc = raster::filter(&ring, |x, y| sector.contains_pixel(x, y));
        self.paint_shape(None, Some((&arc, pen_color)));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pie(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        xr1: i32,
        yr1: i32,
        xr2: i32,
        yr2: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        let sector = raster::Sector::new(left, top, right, bottom, xr1, yr1, xr2, yr2);
        let (ring, inner) = raster::outlined(left, top, right, bottom, 1, raster::ellipse);
        let fill = raster::filter(&inner, |x, y| sector.contains_pixel(x, y));
        let outline = pen_color.map(|_| {
            let mut o = raster::filter(&ring, |x, y| sector.contains_pixel(x, y));
            let c = ((sector.cx - 0.5).round() as i32, (sector.cy - 0.5).round() as i32);
            for end in [false, true] {
                let (px, py) = sector.radial_point(left, top, right, bottom, end);
                raster::thin_line(&mut o, c.0, c.1, px, py, true);
            }
            o
        });
        self.fill_and_outline(fill, outline, pen_color, brush_color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn chord(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        xr1: i32,
        yr1: i32,
        xr2: i32,
        yr2: i32,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        let sector = raster::Sector::new(left, top, right, bottom, xr1, yr1, xr2, yr2);
        let p1 = sector.radial_point(left, top, right, bottom, false);
        let p2 = sector.radial_point(left, top, right, bottom, true);
        let (mx, my) = sector.mid_angle_point();
        let side = |x: f64, y: f64| {
            ((p2.0 - p1.0) as f64) * (y - p1.1 as f64) - ((p2.1 - p1.1) as f64) * (x - p1.0 as f64)
        };
        let arc_side = side(mx, my).signum();
        let (ring, inner) = raster::outlined(left, top, right, bottom, 1, raster::ellipse);
        let fill = raster::filter(&inner, |x, y| side(x as f64, y as f64) * arc_side >= 0.0);
        let outline = pen_color.map(|_| {
            let mut o = raster::filter(&ring, |x, y| sector.contains_pixel(x, y));
            raster::thin_line(&mut o, p1.0, p1.1, p2.0, p2.1, true);
            o
        });
        self.fill_and_outline(fill, outline, pen_color, brush_color);
    }

    fn fill_and_outline(
        &mut self,
        fill: Spans,
        outline: Option<Spans>,
        pen_color: Option<u32>,
        brush_color: Option<u32>,
    ) {
        let fill = match &outline {
            Some(o) => fill.diff(o),
            None => fill,
        };
        let fill_arg = brush_color.map(|c| (&fill, c));
        let outline_arg = outline.as_ref().zip(pen_color);
        self.paint_shape(fill_arg, outline_arg);
    }

    /// `ScrollDC(hdc, dx, dy, &rc, &rc, NULL, &rUpdate)` + `FillRect(&rUpdate, BLACK_BRUSH)`.
    pub fn scroll_rect_fill_black(
        &mut self,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        dx: i32,
        dy: i32,
    ) {
        let w = right - left;
        let h = bottom - top;
        if w <= 0 || h <= 0 || (dx == 0 && dy == 0) {
            return;
        }
        let copy_w = (w - dx.abs()).max(0);
        let copy_h = (h - dy.abs()).max(0);
        if copy_w > 0 && copy_h > 0 {
            let sx = if dx > 0 { left } else { left - dx };
            let sy = if dy > 0 { top } else { top - dy };
            let dst_x = if dx > 0 { left + dx } else { left };
            let dst_y = if dy > 0 { top + dy } else { top };
            self.bit_blt_self(dst_x, dst_y, copy_w, copy_h, sx, sy, SRCCOPY);
        }
        if dx > 0 {
            self.pat_blt_black(left, top, dx.min(w), h);
        } else if dx < 0 {
            let strip = (-dx).min(w);
            self.pat_blt_black(right - strip, top, strip, h);
        }
        if dy > 0 {
            self.pat_blt_black(left, top, w, dy.min(h));
        } else if dy < 0 {
            let strip = (-dy).min(h);
            self.pat_blt_black(left, bottom - strip, w, strip);
        }
    }

    // ------------------------------------------------------------------ blits

    /// Default DC brush (`WHITE_BRUSH`) for ROPs that reference the pattern without one selected.
    const DEFAULT_PAT: u32 = 0x00FF_FFFF;

    #[allow(clippy::too_many_arguments)]
    pub fn bit_blt_bitmap(&mut self, dx: i32, dy: i32, w: i32, h: i32, bmp: &AdBitmap, sx: i32, sy: i32, rop: u32) {
        self.blit(dx, dy, w, h, bmp.src(), sx, sy, w, h, Self::DEFAULT_PAT, rop);
    }

    /// `BitBlt` with an explicit solid pattern brush selected (3-operand ROPs such as
    /// `DSPDxax = 0x00E20746` and `MERGECOPY`).
    #[allow(clippy::too_many_arguments)]
    pub fn bit_blt_bitmap_with_brush(
        &mut self,
        dx: i32,
        dy: i32,
        w: i32,
        h: i32,
        bmp: &AdBitmap,
        sx: i32,
        sy: i32,
        brush_color: u32,
        rop: u32,
    ) {
        self.blit(dx, dy, w, h, bmp.src(), sx, sy, w, h, colorref_to_pixel(brush_color), rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bit_blt_surface(&mut self, dx: i32, dy: i32, w: i32, h: i32, src: &AdSurface, sx: i32, sy: i32, rop: u32) {
        self.blit(dx, dy, w, h, src.as_src(), sx, sy, w, h, Self::DEFAULT_PAT, rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bit_blt_surface_with_brush(
        &mut self,
        dx: i32,
        dy: i32,
        w: i32,
        h: i32,
        src: &AdSurface,
        sx: i32,
        sy: i32,
        brush_color: u32,
        rop: u32,
    ) {
        self.blit(dx, dy, w, h, src.as_src(), sx, sy, w, h, colorref_to_pixel(brush_color), rop);
    }

    /// `BitBlt` from this surface onto itself (overlap-safe, like GDI).
    #[allow(clippy::too_many_arguments)]
    pub fn bit_blt_self(&mut self, dx: i32, dy: i32, w: i32, h: i32, sx: i32, sy: i32, rop: u32) {
        self.stretch_blt_self(dx, dy, w, h, sx, sy, w, h, rop);
    }

    /// `StretchBlt` from this surface onto itself (overlap-safe).
    #[allow(clippy::too_many_arguments)]
    pub fn stretch_blt_self(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        rop: u32,
    ) {
        if dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0 {
            return;
        }
        // Copy the whole requested source rect (out-of-bounds parts become black, which only
        // matters for pathological calls) so overlapping source/destination behave.
        let (buf, bw, bh, ox, oy) = self.snapshot_rect(sx, sy, sw, sh);
        let mut full = vec![0u32; (sw * sh) as usize];
        for y in 0..bh {
            let dst = ((y + oy) * sw + ox) as usize;
            full[dst..dst + bw as usize].copy_from_slice(&buf[(y * bw) as usize..((y + 1) * bw) as usize]);
        }
        let src = Src::Color {
            px: &full,
            w: sw,
            h: sh,
        };
        self.blit(dx, dy, dw, dh, src, 0, 0, sw, sh, Self::DEFAULT_PAT, rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stretch_blt_bitmap(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        bmp: &AdBitmap,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        rop: u32,
    ) {
        self.blit(dx, dy, dw, dh, bmp.src(), sx, sy, sw, sh, Self::DEFAULT_PAT, rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stretch_blt_surface(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        src: &AdSurface,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        rop: u32,
    ) {
        self.blit(dx, dy, dw, dh, src.as_src(), sx, sy, sw, sh, Self::DEFAULT_PAT, rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stretch_blt_bitmap_with_brush(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        bmp: &AdBitmap,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        brush_color: u32,
        rop: u32,
    ) {
        self.blit(dx, dy, dw, dh, bmp.src(), sx, sy, sw, sh, colorref_to_pixel(brush_color), rop);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stretch_blt_surface_with_brush(
        &mut self,
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        src: &AdSurface,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        brush_color: u32,
        rop: u32,
    ) {
        self.blit(dx, dy, dw, dh, src.as_src(), sx, sy, sw, sh, colorref_to_pixel(brush_color), rop);
    }

    fn as_src(&self) -> Src<'_> {
        Src::Color {
            px: &self.px,
            w: self.width,
            h: self.height,
        }
    }

    /// Copy the current surface pixels into a newly allocated `AdSurface`.
    pub fn clone_surface(&self) -> Option<Self> {
        let mut copy = Self::new(self.width, self.height)?;
        copy.px.copy_from_slice(&self.px);
        Some(copy)
    }

    // ------------------------------------------------------------------ text

    /// Composite a coverage value onto pixel `(x, y)` in `pixel` color, honoring clipping.
    #[inline]
    fn blend_px(&mut self, x: i32, y: i32, pixel: u32, cov: u8, clip_rect: &RECT) {
        if x < clip_rect.left || x >= clip_rect.right || y < clip_rect.top || y >= clip_rect.bottom {
            return;
        }
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        if let Some(clip) = &self.clip
            && !clip.contains(x, y) {
                return;
            }
        let d = &mut self.px[(y * self.width + x) as usize];
        if cov == 255 {
            *d = pixel;
            return;
        }
        let a = cov as u32;
        let mix = |s: u32, d: u32| (s * a + d * (255 - a) + 127) / 255;
        let r = mix(pixel >> 16 & 0xFF, *d >> 16 & 0xFF);
        let g = mix(pixel >> 8 & 0xFF, *d >> 8 & 0xFF);
        let b = mix(pixel & 0xFF, *d & 0xFF);
        *d = r << 16 | g << 8 | b;
    }

    fn draw_line_of_text(&mut self, font: &SizedFont, x: i32, y: i32, text: &str, underline: bool, clip: &RECT) {
        let pixel = colorref_to_pixel(self.text_color);
        let mut glyphs: Vec<(i32, i32, u8)> = Vec::new();
        font.render_line(x, y, text, |gx, gy, c| glyphs.push((gx, gy, c)));
        for (gx, gy, c) in glyphs {
            self.blend_px(gx, gy, pixel, c, clip);
        }
        if underline {
            let (off, thick) = font.underline();
            let w = font.measure(text);
            for yy in y + off..y + off + thick {
                for xx in x..x + w {
                    self.blend_px(xx, yy, pixel, 255, clip);
                }
            }
        }
    }

    /// `TextOut` with the DC's default (`SYSTEM_FONT`-like) font, text color, and background mode.
    pub fn text_out(&mut self, x: i32, y: i32, text: &str) {
        let Some(font) = SizedFont::new("System", 16, true, false) else {
            return;
        };
        if self.bk_mode == OPAQUE {
            let w = font.measure(text);
            self.fill_rect_rgb(x, y, x + w, y + font.line_height(), self.bk_color);
        }
        let all = RECT {
            left: i32::MIN,
            top: i32::MIN,
            right: i32::MAX,
            bottom: i32::MAX,
        };
        self.draw_line_of_text(&font, x, y, text, false, &all);
    }

    /// `CreateFont` + `DrawText(DT_NOPREFIX | DT_LEFT/DT_CENTER | DT_SINGLELINE/DT_WORDBREAK)`
    /// in `TRANSPARENT` mode. Like the GDI original this leaves `color` as the DC text color.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_font(
        &mut self,
        face: &str,
        height: i32,
        bold: bool,
        italic: bool,
        underline: bool,
        color: u32,
        rect: RECT,
        centered: bool,
        multiline: bool,
        text: &str,
    ) {
        self.text_color = color;
        self.bk_mode = TRANSPARENT;
        let Some(font) = SizedFont::new(face, height, bold, italic) else {
            return;
        };
        let lines = if multiline {
            font.wrap(text, rect.right - rect.left)
        } else {
            vec![text.chars().filter(|c| !c.is_control()).collect()]
        };
        let mut y = rect.top;
        for line in lines {
            let x = if centered {
                rect.left + (rect.right - rect.left - font.measure(&line)) / 2
            } else {
                rect.left
            };
            self.draw_line_of_text(&font, x, y, &line, underline, &rect);
            y += font.line_height();
            if y >= rect.bottom {
                break;
            }
        }
    }

    /// `GetTextExtentPoint32` for a `CreateFont`-style font: `(width, cell height)`.
    pub fn measure_text_font(&self, face: &str, height: i32, bold: bool, italic: bool, text: &str) -> (i32, i32) {
        SizedFont::new(face, height, bold, italic)
            .map(|f| (f.measure(text), f.line_height()))
            .unwrap_or((0, 0))
    }

    // ------------------------------------------------------------------ desktop & presentation

    /// Fill this surface from the host's desktop snapshot (`src_*` in virtual-screen pixels),
    /// scaled nearest-neighbor. Areas outside the snapshot, or everything when no snapshot is
    /// available, use [`FALLBACK_DESKTOP_PIXEL`].
    pub fn capture_desktop(&mut self, src_x: i32, src_y: i32, src_w: i32, src_h: i32) {
        let snap = DESKTOP.read().ok().and_then(|g| g.clone());
        let Some(snap) = snap.filter(|_| src_w > 0 && src_h > 0) else {
            self.px.fill(FALLBACK_DESKTOP_PIXEL);
            return;
        };
        let (w, h) = (self.width, self.height);
        for y in 0..h {
            let vy = src_y + ((y as i64 * src_h as i64) / h as i64) as i32 - snap.y;
            for x in 0..w {
                let vx = src_x + ((x as i64 * src_w as i64) / w as i64) as i32 - snap.x;
                self.px[(y * w + x) as usize] = if vx >= 0 && vy >= 0 && vx < snap.width && vy < snap.height {
                    snap.pixels[(vy * snap.width + vx) as usize] & RGB_MASK
                } else {
                    FALLBACK_DESKTOP_PIXEL
                };
            }
        }
    }

    /// Scale this surface into a `dst_w x dst_h` `0x00RRGGBB` framebuffer (nearest-neighbor),
    /// optionally pillarboxed to a centered 4:3 stage.
    pub fn present_into(&self, dst: &mut [u32], dst_w: i32, dst_h: i32, pillarbox_4_3: bool) {
        if dst_w <= 0 || dst_h <= 0 || dst.len() < (dst_w as usize) * (dst_h as usize) {
            return;
        }
        let (stage_x, stage_w) = if pillarbox_4_3 {
            let sw = ((dst_h * 4) / 3).min(dst_w);
            ((dst_w - sw) / 2, sw)
        } else {
            (0, dst_w)
        };
        let xmap: Vec<usize> = (0..stage_w)
            .map(|x| ((x as i64 * self.width as i64) / stage_w as i64) as usize)
            .collect();
        let (dw, sw) = (dst_w as usize, self.width as usize);
        for y in 0..dst_h as usize {
            let row = &mut dst[y * dw..(y + 1) * dw];
            let sy = (y * self.height as usize) / dst_h as usize;
            let src = &self.px[sy * sw..(sy + 1) * sw];
            row[..stage_x as usize].fill(0);
            row[(stage_x + stage_w) as usize..].fill(0);
            if stage_w as usize == sw {
                row[stage_x as usize..(stage_x + stage_w) as usize].copy_from_slice(src);
            } else {
                for (d, &sx) in row[stage_x as usize..(stage_x + stage_w) as usize]
                    .iter_mut()
                    .zip(&xmap)
                {
                    *d = src[sx];
                }
            }
        }
    }

    /// Apply a subtle retro CRT phosphor scanline effect to every odd scanline.
    pub fn apply_crt_scanlines(&mut self) {
        let w = self.width as usize;
        for row in self.px.chunks_mut(w).skip(1).step_by(2) {
            for c in row {
                let r = ((*c >> 16 & 0xFF) * 3) >> 2;
                let g = ((*c >> 8 & 0xFF) * 3) >> 2;
                let b = ((*c & 0xFF) * 3) >> 2;
                *c = r << 16 | g << 8 | b;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    #[test]
    fn colorref_round_trip() {
        let mut s = AdSurface::new(4, 4).unwrap();
        s.set_pixel(1, 1, rgb(0x12, 0x34, 0x56));
        assert_eq!(s.pixels()[5], 0x0012_3456);
        assert_eq!(s.get_pixel(1, 1), rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn rop3_generic_matches_fast_paths() {
        for &code in &[0xCCu8, 0x88, 0xEE, 0x66, 0xF0, 0x55, 0x33, 0x5A, 0xC0, 0xBB, 0x44, 0x11] {
            let (p, s, d) = (0x00F0_F00Fu32, 0x00CC_33AA, 0x00AA_55CC);
            let mut r = 0u32;
            for i in 0..8u8 {
                if code >> i & 1 != 0 {
                    let pp = if i & 4 != 0 { p } else { !p };
                    let ss = if i & 2 != 0 { s } else { !s };
                    let dd = if i & 1 != 0 { d } else { !d };
                    r |= pp & ss & dd;
                }
            }
            assert_eq!(rop3(code, p, s, d), r & RGB_MASK, "rop {code:#x}");
        }
    }

    #[test]
    fn rop2_table() {
        assert_eq!(rop2(R2_BLACK, 0x123456, 0x654321), 0);
        assert_eq!(rop2(R2_WHITE, 0x123456, 0x654321), RGB_MASK);
        assert_eq!(rop2(R2_MASKPEN, 0xF0F0F0, 0x3C3C3C), 0x303030);
        assert_eq!(rop2(R2_MERGEPEN, 0xF0F0F0, 0x0F0F0F), RGB_MASK);
        assert_eq!(rop2(R2_NOTCOPYPEN, 0x00FF00, 0), 0xFF00FF);
    }

    #[test]
    fn mono_blit_uses_text_and_bk_colors() {
        // 2x1 1bpp BMP: palette black, white; pixels 0b01.. => x0 black(0), x1 white(1).
        let mut bmp = vec![0u8; 62 + 4];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&62u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&2i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
        bmp[28..30].copy_from_slice(&1u16.to_le_bytes());
        bmp[58..62].copy_from_slice(&[255, 255, 255, 0]);
        bmp[62] = 0b0100_0000;
        let b = AdBitmap::from_bmp_bytes(&bmp).unwrap();
        let mut s = AdSurface::new(2, 1).unwrap();
        s.set_text_color(rgb(255, 0, 0));
        s.set_bk_color(rgb(0, 0, 255));
        s.bit_blt_bitmap(0, 0, 2, 1, &b, 0, 0, SRCCOPY);
        assert_eq!(s.pixels(), &[0x00FF_0000, 0x0000_00FF]);
    }

    #[test]
    fn xor_pen_rectangle_does_not_double_apply() {
        let mut s = AdSurface::new(10, 10).unwrap();
        s.set_rop2(R2_XORPEN);
        s.rectangle(0, 0, 10, 10, Some(rgb(255, 255, 255)), Some(rgb(255, 255, 255)));
        assert!(s.pixels().iter().all(|&p| p == RGB_MASK));
    }

    #[test]
    fn overlapping_self_blit() {
        let mut s = AdSurface::new(6, 1).unwrap();
        s.pixels_mut().copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        s.bit_blt_self(1, 0, 5, 1, 0, 0, SRCCOPY);
        assert_eq!(s.pixels(), &[1, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn invert_rect_normalizes_unordered_corners() {
        let mut s = AdSurface::new(10, 10).unwrap();
        s.invert_rect(8, 6, 2, 1);
        assert_eq!(s.pixels().iter().filter(|&&p| p != 0).count(), 6 * 5);
    }

    #[test]
    fn clip_region_limits_fills() {
        let mut s = AdSurface::new(10, 10).unwrap();
        s.set_clip_region(Some(&AdRegion::rect(2, 2, 4, 4)));
        s.fill_rect_rgb(0, 0, 10, 10, rgb(255, 255, 255));
        assert_eq!(s.pixels().iter().filter(|&&p| p != 0).count(), 4);
    }

    #[test]
    fn present_scales_and_pillarboxes() {
        let mut s = AdSurface::new(2, 2).unwrap();
        s.pixels_mut().copy_from_slice(&[1, 2, 3, 4]);
        let mut dst = vec![9u32; 8 * 4];
        s.present_into(&mut dst, 8, 4, false);
        assert_eq!(&dst[0..8], &[1, 1, 1, 1, 2, 2, 2, 2]);
        s.present_into(&mut dst, 8, 4, true);
        assert_eq!(&dst[0..8], &[0, 1, 1, 1, 2, 2, 0, 0]);
    }
}
