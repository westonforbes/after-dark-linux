//! Span-list regions: the shared representation for GDI regions (`HRGN`), clip regions,
//! and the coverage of every rasterized shape before it is painted.
//!
//! A region is a vertical run of rows; each row holds sorted, non-overlapping, non-adjacent
//! half-open spans `[x0, x1)`. Boolean operations work row by row, so they are exact.

/// Half-open horizontal span `[x0, x1)`.
pub type Span = (i32, i32);

#[derive(Debug, Clone, Default)]
pub struct Spans {
    /// Device y of `rows[0]`.
    y0: i32,
    rows: Vec<Vec<Span>>,
}

impl Spans {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        if right <= left || bottom <= top {
            return Self::empty();
        }
        Self {
            y0: top,
            rows: vec![vec![(left, right)]; (bottom - top) as usize],
        }
    }

    /// Build from a per-row span function over `top..bottom`. The function may return any
    /// unsorted / overlapping spans; they are normalized.
    pub fn from_rows(top: i32, bottom: i32, mut f: impl FnMut(i32, &mut Vec<Span>)) -> Self {
        if bottom <= top {
            return Self::empty();
        }
        let mut rows = Vec::with_capacity((bottom - top) as usize);
        let mut scratch = Vec::new();
        for y in top..bottom {
            scratch.clear();
            f(y, &mut scratch);
            rows.push(normalize(&mut scratch));
        }
        let mut s = Self { y0: top, rows };
        s.trim();
        s
    }

    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|r| r.is_empty())
    }

    #[inline]
    pub fn top(&self) -> i32 {
        self.y0
    }

    #[inline]
    pub fn bottom(&self) -> i32 {
        self.y0 + self.rows.len() as i32
    }

    #[inline]
    pub fn row(&self, y: i32) -> &[Span] {
        if y < self.y0 {
            return &[];
        }
        self.rows
            .get((y - self.y0) as usize)
            .map(|r| r.as_slice())
            .unwrap_or(&[])
    }

    /// Iterate `(y, spans)` over all non-empty rows.
    pub fn iter_rows(&self) -> impl Iterator<Item = (i32, &[Span])> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_empty())
            .map(move |(i, r)| (self.y0 + i as i32, r.as_slice()))
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        let row = self.row(y);
        // Binary search for the last span with x0 <= x.
        let idx = row.partition_point(|&(x0, _)| x0 <= x);
        idx > 0 && x < row[idx - 1].1
    }

    pub fn bbox(&self) -> Option<(i32, i32, i32, i32)> {
        let mut l = i32::MAX;
        let mut r = i32::MIN;
        let mut t = None;
        let mut b = 0;
        for (y, row) in self.iter_rows() {
            t.get_or_insert(y);
            b = y + 1;
            l = l.min(row[0].0);
            r = r.max(row[row.len() - 1].1);
        }
        t.map(|t| (l, t, r, b))
    }

    pub fn union(&self, other: &Spans) -> Spans {
        self.combine(other, |a, b| a || b)
    }

    pub fn intersect(&self, other: &Spans) -> Spans {
        self.combine(other, |a, b| a && b)
    }

    pub fn xor(&self, other: &Spans) -> Spans {
        self.combine(other, |a, b| a != b)
    }

    pub fn diff(&self, other: &Spans) -> Spans {
        self.combine(other, |a, b| a && !b)
    }

    /// Add a span to a row, keeping the row normalized (used when accumulating stroke coverage).
    pub fn add_span(&mut self, y: i32, x0: i32, x1: i32) {
        if x1 <= x0 {
            return;
        }
        if self.rows.is_empty() {
            self.y0 = y;
            self.rows.push(Vec::new());
        } else if y < self.y0 {
            let extra = (self.y0 - y) as usize;
            let mut new_rows = vec![Vec::new(); extra];
            new_rows.append(&mut self.rows);
            self.rows = new_rows;
            self.y0 = y;
        } else if y >= self.bottom() {
            let extra = (y - self.bottom() + 1) as usize;
            self.rows.extend(std::iter::repeat_with(Vec::new).take(extra));
        }
        let row = &mut self.rows[(y - self.y0) as usize];
        row.push((x0, x1));
        if row.len() > 1 {
            let merged = normalize(row);
            *row = merged;
        }
    }

    fn combine(&self, other: &Spans, op: impl Fn(bool, bool) -> bool) -> Spans {
        let (top, bottom) = match (self.rows.is_empty(), other.rows.is_empty()) {
            (true, true) => return Spans::empty(),
            (true, false) => (other.top(), other.bottom()),
            (false, true) => (self.top(), self.bottom()),
            (false, false) => (
                self.top().min(other.top()),
                self.bottom().max(other.bottom()),
            ),
        };
        let mut rows = Vec::with_capacity((bottom - top) as usize);
        for y in top..bottom {
            rows.push(combine_row(self.row(y), other.row(y), &op));
        }
        let mut s = Spans { y0: top, rows };
        s.trim();
        s
    }

    fn trim(&mut self) {
        while self.rows.last().is_some_and(|r| r.is_empty()) {
            self.rows.pop();
        }
        let lead = self.rows.iter().take_while(|r| r.is_empty()).count();
        if lead > 0 {
            self.rows.drain(..lead);
            self.y0 += lead as i32;
        }
        if self.rows.is_empty() {
            self.y0 = 0;
        }
    }
}

/// Sort and merge overlapping/adjacent spans.
fn normalize(spans: &mut [Span]) -> Vec<Span> {
    spans.sort_unstable_by_key(|s| s.0);
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for &(x0, x1) in spans.iter() {
        if x1 <= x0 {
            continue;
        }
        match out.last_mut() {
            Some(last) if x0 <= last.1 => last.1 = last.1.max(x1),
            _ => out.push((x0, x1)),
        }
    }
    out
}

/// Boolean-combine two normalized rows by sweeping all span edges.
fn combine_row(a: &[Span], b: &[Span], op: &impl Fn(bool, bool) -> bool) -> Vec<Span> {
    let mut edges: Vec<i32> = Vec::with_capacity((a.len() + b.len()) * 2);
    for &(x0, x1) in a.iter().chain(b.iter()) {
        edges.push(x0);
        edges.push(x1);
    }
    edges.sort_unstable();
    edges.dedup();
    let mut out: Vec<Span> = Vec::new();
    let (mut ia, mut ib) = (0usize, 0usize);
    for w in edges.windows(2) {
        let (x0, x1) = (w[0], w[1]);
        while ia < a.len() && a[ia].1 <= x0 {
            ia += 1;
        }
        while ib < b.len() && b[ib].1 <= x0 {
            ib += 1;
        }
        let in_a = ia < a.len() && a[ia].0 <= x0;
        let in_b = ib < b.len() && b[ib].0 <= x0;
        if op(in_a, in_b) {
            match out.last_mut() {
                Some(last) if last.1 == x0 => last.1 = x1,
                _ => out.push((x0, x1)),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_ops() {
        let a = Spans::rect(0, 0, 10, 10);
        let b = Spans::rect(5, 5, 15, 15);
        assert!(a.union(&b).contains(12, 12));
        assert!(a.union(&b).contains(2, 2));
        assert!(!a.union(&b).contains(12, 2));
        let i = a.intersect(&b);
        assert_eq!(i.bbox(), Some((5, 5, 10, 10)));
        let d = a.diff(&b);
        assert!(d.contains(9, 4));
        assert!(!d.contains(9, 5));
        let x = a.xor(&b);
        assert!(!x.contains(7, 7));
        assert!(x.contains(12, 12));
        assert!(a.intersect(&Spans::rect(20, 20, 30, 30)).is_empty());
    }

    #[test]
    fn add_span_merges() {
        let mut s = Spans::empty();
        s.add_span(5, 0, 3);
        s.add_span(5, 3, 6);
        s.add_span(2, 1, 2);
        assert_eq!(s.row(5), &[(0, 6)]);
        assert_eq!(s.row(2), &[(1, 2)]);
        assert_eq!(s.top(), 2);
    }
}
