//! Shape rasterization into span coverage, following Win16/Win32 GDI conventions:
//! bounding boxes exclude their right/bottom edge, `LineTo` excludes its final pixel,
//! arcs run counter-clockwise (on screen) from the start radial to the end radial,
//! and wide pens are round-capped and centered on the outline.

use std::f64::consts::TAU;

use super::region::{Span, Spans};

/// Coverage of `(l, t, r, b)` shrunk/grown by `d` pixels on every side.
#[inline]
pub fn inflate(l: i32, t: i32, r: i32, b: i32, d: i32) -> (i32, i32, i32, i32) {
    (l - d, t - d, r + d, b + d)
}

/// Pixel-center span of an axis-aligned ellipse inscribed in `[l, r) x [t, b)` on row `y`.
pub fn ellipse_row(l: i32, t: i32, r: i32, b: i32, y: i32) -> Option<Span> {
    let (w, h) = (r - l, b - t);
    if w <= 0 || h <= 0 || y < t || y >= b {
        return None;
    }
    let a = w as f64 / 2.0;
    let bb = h as f64 / 2.0;
    let cx = l as f64 + a;
    let cy = t as f64 + bb;
    let dy = (y as f64 + 0.5 - cy) / bb;
    let k = 1.0 - dy * dy;
    if k < 0.0 {
        return None;
    }
    let hw = a * k.sqrt();
    let x0 = (cx - hw - 0.5).ceil() as i32;
    let x1 = (cx + hw - 0.5).floor() as i32 + 1;
    (x1 > x0).then_some((x0.max(l), x1.min(r)))
}

pub fn ellipse(l: i32, t: i32, r: i32, b: i32) -> Spans {
    Spans::from_rows(t, b, |y, out| {
        if let Some(s) = ellipse_row(l, t, r, b, y) {
            out.push(s);
        }
    })
}

pub fn round_rect(l: i32, t: i32, r: i32, b: i32, rw: i32, rh: i32) -> Spans {
    let (w, h) = (r - l, b - t);
    if w <= 0 || h <= 0 {
        return Spans::empty();
    }
    let rw = rw.clamp(0, w);
    let rh = rh.clamp(0, h);
    if rw < 2 || rh < 2 {
        return Spans::rect(l, t, r, b);
    }
    let half = rh / 2;
    Spans::from_rows(t, b, |y, out| {
        let (et, eb) = if y < t + half {
            (t, t + rh)
        } else if y >= b - half {
            (b - rh, b)
        } else {
            out.push((l, r));
            return;
        };
        let left = ellipse_row(l, et, l + rw, eb, y).map(|s| s.0);
        let right = ellipse_row(r - rw, et, r, eb, y).map(|s| s.1);
        if let (Some(x0), Some(x1)) = (left, right) {
            out.push((x0, x1));
        }
    })
}

/// Scanline polygon fill sampled at pixel centers. `winding` selects WINDING vs ALTERNATE.
pub fn polygon(pts: &[(i32, i32)], winding: bool) -> Spans {
    if pts.len() < 3 {
        return Spans::empty();
    }
    let top = pts.iter().map(|p| p.1).min().unwrap_or(0);
    let bottom = pts.iter().map(|p| p.1).max().unwrap_or(0);
    let mut xs: Vec<(f64, i32)> = Vec::new();
    Spans::from_rows(top, bottom + 1, |y, out| {
        let yc = y as f64 + 0.5;
        xs.clear();
        for i in 0..pts.len() {
            let (x0, y0) = pts[i];
            let (x1, y1) = pts[(i + 1) % pts.len()];
            if y0 == y1 {
                continue;
            }
            let (ya, yb) = (y0.min(y1) as f64, y0.max(y1) as f64);
            if yc < ya || yc >= yb {
                continue;
            }
            let x = x0 as f64 + (yc - y0 as f64) * (x1 - x0) as f64 / (y1 - y0) as f64;
            xs.push((x, if y1 > y0 { 1 } else { -1 }));
        }
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut wind = 0;
        let mut parity = false;
        for i in 0..xs.len() {
            wind += xs[i].1;
            parity = !parity;
            let inside = if winding { wind != 0 } else { parity };
            if inside && i + 1 < xs.len() {
                let x0 = (xs[i].0 - 0.5).ceil() as i32;
                let x1 = (xs[i + 1].0 - 0.5).ceil() as i32;
                if x1 > x0 {
                    out.push((x0, x1));
                }
            }
        }
    })
}

/// Add a 1-pixel Bresenham line to `cov`. `include_last` false matches GDI `LineTo`.
pub fn thin_line(cov: &mut Spans, x1: i32, y1: i32, x2: i32, y2: i32, include_last: bool) {
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let (mut x, mut y) = (x1, y1);
    let mut err = dx + dy;
    loop {
        let at_end = x == x2 && y == y2;
        if at_end && !include_last {
            break;
        }
        cov.add_span(y, x, x + 1);
        if at_end {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Add a round-capped segment of width `w` (geometric pen) to `cov`.
pub fn thick_segment(cov: &mut Spans, x1: i32, y1: i32, x2: i32, y2: i32, w: i32) {
    let rad = w as f64 / 2.0;
    let r2 = rad * rad;
    let (ax, ay) = (x1 as f64 + 0.5, y1 as f64 + 0.5);
    let (bx, by) = (x2 as f64 + 0.5, y2 as f64 + 0.5);
    let (vx, vy) = (bx - ax, by - ay);
    let len2 = vx * vx + vy * vy;
    let pad = rad.ceil() as i32 + 1;
    let top = y1.min(y2) - pad;
    let bottom = y1.max(y2) + pad;
    let left = x1.min(x2) - pad;
    let right = x1.max(x2) + pad;
    for y in top..=bottom {
        let pyc = y as f64 + 0.5;
        let mut run: Option<i32> = None;
        for x in left..=right + 1 {
            let inside = x <= right && {
                let pxc = x as f64 + 0.5;
                let t = if len2 > 0.0 {
                    (((pxc - ax) * vx + (pyc - ay) * vy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (qx, qy) = (ax + t * vx - pxc, ay + t * vy - pyc);
                qx * qx + qy * qy <= r2
            };
            match (inside, run) {
                (true, None) => run = Some(x),
                (false, Some(x0)) => {
                    cov.add_span(y, x0, x);
                    run = None;
                }
                _ => {}
            }
        }
    }
}

/// Stroke an open polyline with a pen of width `w` (GDI `Polyline` / chained `LineTo`).
pub fn stroke_polyline(pts: &[(i32, i32)], w: i32) -> Spans {
    let mut cov = Spans::empty();
    if pts.len() < 2 {
        return cov;
    }
    for seg in pts.windows(2) {
        let ((x1, y1), (x2, y2)) = (seg[0], seg[1]);
        if w <= 1 {
            thin_line(&mut cov, x1, y1, x2, y2, false);
        } else {
            thick_segment(&mut cov, x1, y1, x2, y2, w);
        }
    }
    cov
}

/// Outline band for a closed convex shape drawn with pen width `w`, centered on the edge.
/// Returns `(outline, interior)`: the pen's coverage and the area the brush fills inside it.
pub fn outlined(
    l: i32,
    t: i32,
    r: i32,
    b: i32,
    w: i32,
    shape: impl Fn(i32, i32, i32, i32) -> Spans,
) -> (Spans, Spans) {
    let w = w.max(1);
    let (ol, ot, or, ob) = inflate(l, t, r, b, (w - 1) / 2);
    let outer = shape(ol, ot, or, ob);
    let (il, it, ir, ib) = inflate(ol, ot, or, ob, -w);
    let inner = shape(il, it, ir, ib);
    (outer.diff(&inner), inner)
}

/// Angle (radians, counter-clockwise on screen, 0 = +x) from `(cx, cy)` to `(x, y)`.
#[inline]
fn screen_angle(cx: f64, cy: f64, x: f64, y: f64) -> f64 {
    (-(y - cy)).atan2(x - cx).rem_euclid(TAU)
}

/// Angular sector selector for `Arc` / `Pie` / `Chord` radials.
#[derive(Clone, Copy)]
pub struct Sector {
    pub cx: f64,
    pub cy: f64,
    start: f64,
    sweep: f64,
}

impl Sector {
    pub fn new(l: i32, t: i32, r: i32, b: i32, xr1: i32, yr1: i32, xr2: i32, yr2: i32) -> Self {
        let cx = (l + r) as f64 / 2.0;
        let cy = (t + b) as f64 / 2.0;
        let start = screen_angle(cx, cy, xr1 as f64, yr1 as f64);
        let end = screen_angle(cx, cy, xr2 as f64, yr2 as f64);
        let mut sweep = (end - start).rem_euclid(TAU);
        if sweep < 1e-9 {
            sweep = TAU;
        }
        Self {
            cx,
            cy,
            start,
            sweep,
        }
    }

    #[inline]
    pub fn contains_pixel(&self, x: i32, y: i32) -> bool {
        let a = screen_angle(self.cx, self.cy, x as f64 + 0.5, y as f64 + 0.5);
        (a - self.start).rem_euclid(TAU) <= self.sweep
    }

    /// Point where the start (`end == false`) or end radial meets the ellipse `(l,t,r,b)`.
    pub fn radial_point(&self, l: i32, t: i32, r: i32, b: i32, end: bool) -> (i32, i32) {
        let ang = if end {
            self.start + self.sweep
        } else {
            self.start
        };
        let a = ((r - l) as f64 / 2.0).max(0.5);
        let bb = ((b - t) as f64 / 2.0).max(0.5);
        let (dx, dy) = (ang.cos(), -ang.sin());
        let k = 1.0 / ((dx / a).powi(2) + (dy / bb).powi(2)).sqrt();
        (
            (self.cx + dx * k - 0.5).round() as i32,
            (self.cy + dy * k - 0.5).round() as i32,
        )
    }

    pub fn mid_angle_point(&self) -> (f64, f64) {
        let ang = self.start + self.sweep / 2.0;
        (self.cx + ang.cos() * 1000.0, self.cy - ang.sin() * 1000.0)
    }
}

/// Keep only the pixels of `s` that satisfy `keep`.
pub fn filter(s: &Spans, mut keep: impl FnMut(i32, i32) -> bool) -> Spans {
    let Some((_, t, _, b)) = s.bbox() else {
        return Spans::empty();
    };
    Spans::from_rows(t, b, |y, out| {
        for &(x0, x1) in s.row(y) {
            let mut run: Option<i32> = None;
            for x in x0..=x1 {
                let inside = x < x1 && keep(x, y);
                match (inside, run) {
                    (true, None) => run = Some(x),
                    (false, Some(start)) => {
                        out.push((start, x));
                        run = None;
                    }
                    _ => {}
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(s: &Spans) -> i32 {
        s.iter_rows()
            .map(|(_, r)| r.iter().map(|(a, b)| b - a).sum::<i32>())
            .sum()
    }

    #[test]
    fn ellipse_is_symmetric_and_bounded() {
        let e = ellipse(10, 10, 30, 20);
        assert_eq!(e.bbox(), Some((10, 10, 30, 20)));
        for y in 10..20 {
            let row = e.row(y);
            let mirror = e.row(10 + 19 - y);
            assert_eq!(row, mirror);
            assert_eq!(row[0].0 - 10, 30 - row[0].1);
        }
    }

    #[test]
    fn line_excludes_last_pixel() {
        let c = stroke_polyline(&[(0, 0), (5, 0)], 1);
        assert_eq!(c.row(0), &[(0, 5)]);
        let d = stroke_polyline(&[(0, 0), (3, 3)], 1);
        assert_eq!(count(&d), 3);
    }

    #[test]
    fn polygon_square_matches_rect() {
        let p = polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)], false);
        assert_eq!(p.bbox(), Some((0, 0, 10, 10)));
        assert_eq!(count(&p), 100);
    }

    #[test]
    fn outline_ring_has_no_gaps_on_axes() {
        let (ring, inner) = outlined(0, 0, 40, 40, 1, ellipse);
        assert!(ring.contains(20, 0) && ring.contains(0, 20) && ring.contains(39, 20));
        assert!(inner.contains(20, 20) && !ring.contains(20, 20));
    }

    #[test]
    fn half_pie_sector() {
        // Start at right (3 o'clock), end at left: CCW sweep covers the top half on screen.
        let s = Sector::new(0, 0, 100, 100, 100, 50, 0, 50);
        assert!(s.contains_pixel(50, 10));
        assert!(!s.contains_pixel(50, 90));
    }
}
