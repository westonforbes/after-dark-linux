//! GDI-style text: `CreateFont` face/height/weight semantics on top of fontconfig + `fontdue`.
//!
//! - Face names are resolved through `fc-match`, so Windows faces map onto their metric-compatible
//!   Linux equivalents (e.g. `Arial` → Liberation Sans) exactly as fontconfig's aliases dictate.
//! - `height > 0` is the GDI *cell* height (ascent + descent); `height < 0` is the em height;
//!   `0` is the GDI default (16 px cell).
//! - Glyph advances are rounded to whole pixels and no kerning is applied, matching GDI `TextOut`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use fontdue::{Font, FontSettings};

#[derive(Clone, PartialEq, Eq, Hash)]
struct FaceKey {
    face: String,
    bold: bool,
    italic: bool,
}

fn face_cache() -> &'static Mutex<HashMap<FaceKey, Option<Arc<Font>>>> {
    static CACHE: OnceLock<Mutex<HashMap<FaceKey, Option<Arc<Font>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve `face` to a font file via `fc-match`, returning `(path, collection index)`.
fn fc_match(pattern: &str) -> Option<(PathBuf, u32)> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let file = lines.next()?.trim();
    if file.is_empty() {
        return None;
    }
    let index = lines.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    Some((PathBuf::from(file), index))
}

/// Last-resort search of common font directories when fontconfig is unavailable.
fn fallback_font_file(bold: bool) -> Option<PathBuf> {
    let names: &[&str] = if bold {
        &["DejaVuSans-Bold.ttf", "LiberationSans-Bold.ttf", "DejaVuSans.ttf"]
    } else {
        &["DejaVuSans.ttf", "LiberationSans-Regular.ttf"]
    };
    let roots = ["/usr/share/fonts", "/usr/local/share/fonts", "/run/current-system/sw/share/X11/fonts"];
    for root in roots {
        let mut stack = vec![PathBuf::from(root)];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| names.contains(&n))
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn load_font_file(path: &PathBuf, index: u32) -> Option<Font> {
    let bytes = std::fs::read(path).ok()?;
    Font::from_bytes(
        bytes,
        FontSettings {
            collection_index: index,
            scale: 40.0,
            ..FontSettings::default()
        },
    )
    .ok()
}

fn resolve_face(face: &str, bold: bool, italic: bool) -> Option<Arc<Font>> {
    let key = FaceKey {
        face: face.to_ascii_lowercase(),
        bold,
        italic,
    };
    if let Some(hit) = face_cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let mut style = String::new();
    if bold {
        style.push_str(":bold");
    }
    if italic {
        style.push_str(":italic");
    }
    let face_pat = if face.trim().is_empty() || face.eq_ignore_ascii_case("system") {
        "sans-serif".to_string()
    } else {
        face.replace([':', '-'], " ")
    };
    let font = [format!("{face_pat}{style}"), format!("sans-serif{style}")]
        .iter()
        .filter_map(|pat| fc_match(pat))
        .find_map(|(path, idx)| load_font_file(&path, idx))
        .or_else(|| fallback_font_file(bold).and_then(|p| load_font_file(&p, 0)))
        .map(Arc::new);
    if let Ok(mut cache) = face_cache().lock() {
        cache.insert(key, font.clone());
    }
    font
}

/// A font instance at a concrete pixel size (the equivalent of a selected `HFONT`).
pub struct SizedFont {
    font: Arc<Font>,
    px: f32,
    /// Pixels above the baseline.
    pub ascent: i32,
    /// Pixels below the baseline (positive).
    pub descent: i32,
}

impl SizedFont {
    pub fn new(face: &str, height: i32, bold: bool, italic: bool) -> Option<Self> {
        let font = resolve_face(face, bold, italic)?;
        let unit = font.horizontal_line_metrics(100.0)?;
        let cell_per_px = ((unit.ascent - unit.descent) / 100.0).max(0.1);
        let px = match height {
            0 => 16.0 / cell_per_px,
            h if h > 0 => h as f32 / cell_per_px,
            h => (-h) as f32,
        };
        let m = font.horizontal_line_metrics(px)?;
        Some(Self {
            font,
            px,
            ascent: m.ascent.round() as i32,
            descent: (-m.descent).round() as i32,
        })
    }

    /// GDI `tmHeight` (cell height).
    #[inline]
    pub fn line_height(&self) -> i32 {
        self.ascent + self.descent
    }

    #[inline]
    fn advance(&self, ch: char) -> i32 {
        self.font.metrics(ch, self.px).advance_width.round() as i32
    }

    /// GDI `GetTextExtentPoint32` width.
    pub fn measure(&self, text: &str) -> i32 {
        text.chars().filter(|c| !c.is_control()).map(|c| self.advance(c)).sum()
    }

    /// Rasterize one line with its top-left cell corner at `(x, y)`. `put(px, py, coverage)`.
    pub fn render_line(&self, x: i32, y: i32, text: &str, mut put: impl FnMut(i32, i32, u8)) {
        let baseline = y + self.ascent;
        let mut pen_x = x;
        for ch in text.chars().filter(|c| !c.is_control()) {
            let (m, bitmap) = self.font.rasterize(ch, self.px);
            let gx = pen_x + m.xmin;
            let gy = baseline - m.height as i32 - m.ymin;
            for row in 0..m.height {
                for col in 0..m.width {
                    let c = bitmap[row * m.width + col];
                    if c != 0 {
                        put(gx + col as i32, gy + row as i32, c);
                    }
                }
            }
            pen_x += m.advance_width.round() as i32;
        }
    }

    /// Underline position (offset from cell top) and thickness.
    pub fn underline(&self) -> (i32, i32) {
        let thick = ((self.px / 14.0).round() as i32).max(1);
        (self.ascent + (self.descent / 2).max(1), thick)
    }

    /// `DrawText(DT_WORDBREAK)` line breaking: explicit `\n` / `\r\n` breaks, then greedy word wrap.
    pub fn wrap(&self, text: &str, max_w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
            let mut cur = String::new();
            for word in para.split(' ') {
                let candidate = if cur.is_empty() {
                    word.to_string()
                } else {
                    format!("{cur} {word}")
                };
                if !cur.is_empty() && self.measure(&candidate) > max_w {
                    lines.push(std::mem::take(&mut cur));
                    cur = word.to_string();
                } else {
                    cur = candidate;
                }
            }
            lines.push(cur);
        }
        lines
    }
}

