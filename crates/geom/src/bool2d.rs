//! Boolean operations on closed planar regions: union, intersection, difference and symmetric
//! difference of polygonal regions that may carry holes, plus the loop bookkeeping the 3D kernel
//! builds on (the interior of one face of a solid is a list of regions: an outer loop and the
//! loops that are holes in it).
//!
//! # How it works
//!
//! 1. Every loop of both subjects becomes directed edges.
//! 2. Edges are split at every crossing — including collinear overlaps — so that each resulting
//!    fragment lies entirely on one side of the other subject's boundary. Midpoint queries are
//!    then meaningful.
//! 3. Fragments that both subjects contribute (the same geometry, in either direction) are merged
//!    and counted. This is what makes a shared boundary decidable instead of ambiguous.
//! 4. For each fragment a point just to either side is classified against *both* subjects, and the
//!    fragment survives only when the operation's fill rule disagrees across it. One rule covers
//!    all four operations, shared edges, tangential touching and identical inputs.
//! 5. Survivors are chained into loops by always taking the face on the left of the edge, then
//!    sorted into regions by nesting depth.
//!
//! # Guarantees and limits
//!
//! - Fill rule is **even-odd**: input loops that self-intersect or overlap within one subject are
//!   still well defined (they toggle), and input orientation is irrelevant.
//! - Output loops are simple (no self-intersection, no repeated vertex), non-degenerate, and
//!   consistently oriented: outer loops counter-clockwise, holes clockwise, each rotated to start
//!   at its lexicographically smallest vertex so results are reproducible.
//! - Tolerance scales with the bounds of the input, so the engine behaves the same on a part a
//!   micrometre across and on a georeferenced drawing.
//! - Features thinner than the tolerance are not resolved; geometry that would be degenerate is
//!   dropped rather than returned as slivers.
//! - Every entry point returns `Option`: hostile or oversized input yields `None`, never a panic.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Bounds2, Vec2, line_line_infinite, norm_angle, point_in_polygon, shoelace};

/// Input segments per subject (bounds the quadratic split on hostile input).
pub const MAX_SEGMENTS: usize = 60_000;
/// Fragments kept after splitting.
pub const MAX_FRAGMENTS: usize = 400_000;
/// Segment pairs examined while splitting.
pub const MAX_PAIR_TESTS: usize = 20_000_000;
/// Output loops.
pub const MAX_LOOPS: usize = 20_000;
/// Vertices across all loops of one input.
pub const MAX_LOOP_VERTS: usize = 400_000;

/// A closed planar region: one outer loop and the loops that are holes in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    /// Counter-clockwise outer loop.
    pub outer: Vec<Vec2>,
    /// Clockwise hole loops.
    pub holes: Vec<Vec<Vec2>>,
}

/// A set of pairwise disjoint regions — what the interior of one face of a solid is made of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Regions(pub Vec<Region>);

impl Regions {
    pub fn new(regions: Vec<Region>) -> Self {
        Regions(regions)
    }
    pub fn regions(&self) -> &[Region] {
        &self.0
    }
    pub fn regions_mut(&mut self) -> &mut Vec<Region> {
        &mut self.0
    }
}

impl std::ops::Deref for Regions {
    type Target = Vec<Region>;
    fn deref(&self) -> &Vec<Region> {
        &self.0
    }
}

impl std::ops::DerefMut for Regions {
    fn deref_mut(&mut self) -> &mut Vec<Region> {
        &mut self.0
    }
}

impl FromIterator<Region> for Regions {
    fn from_iter<I: IntoIterator<Item = Region>>(it: I) -> Self {
        Regions(it.into_iter().collect())
    }
}

/// The four planar boolean operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Intersect,
    Difference,
    Xor,
}

impl Region {
    pub fn new(outer: Vec<Vec2>) -> Self {
        Region { outer, holes: Vec::new() }
    }

    /// Every loop of the region, outer first.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<Vec2>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// Signed area: positive for a well-formed region.
    ///
    /// Outer loops are stored counter-clockwise and holes clockwise, but the absolute value of
    /// each loop is used so the result does not depend on how the caller built the region.
    pub fn signed_area(&self) -> f64 {
        let mut a = shoelace(&self.outer).abs();
        for h in &self.holes {
            a -= shoelace(h).abs();
        }
        a
    }

    /// Area enclosed by the region.
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    pub fn bounds(&self) -> Bounds2 {
        Bounds2::from_points(self.loops().flat_map(|l| l.iter().copied()))
    }

    pub fn is_empty(&self) -> bool {
        self.outer.len() < 3
    }

    /// Region set holding just this region.
    pub fn to_regions(&self) -> Regions {
        if self.is_empty() { Regions::default() } else { Regions::new(vec![self.clone()]) }
    }
}

impl Regions {
    /// Total area of the set.
    pub fn area(&self) -> f64 {
        self.iter().map(|r| r.area()).sum()
    }

    pub fn bounds(&self) -> Bounds2 {
        self.iter().fold(Bounds2::EMPTY, |b, r| b.union(&r.bounds()))
    }

    pub fn is_empty(&self) -> bool {
        self.iter().all(|r| r.is_empty())
    }

    /// Every loop of every region.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<Vec2>> {
        self.iter().flat_map(|r| r.loops())
    }

    /// Even-odd point-in-set test.
    pub fn contains(&self, p: Vec2) -> bool {
        let mut inside = false;
        for l in self.loops() {
            if point_in_polygon(l, p) {
                inside = !inside;
            }
        }
        inside
    }

    /// Build regions from raw loops, sorted by nesting depth: even depth starts a region, odd
    /// depth is a hole in the region enclosing it. Orientation is normalised, so loops may be
    /// given in any direction and self-intersecting ones are still handled by the even-odd rule.
    pub fn from_loops(loops: Vec<Vec<Vec2>>, tol: f64) -> Option<Regions> {
        let tol = sanitize_tol(tol);
        let clean = clean_loops(loops, tol)?;
        let mut kept: Vec<Vec<Vec2>> = Vec::new();
        for l in clean {
            if !keepable(&l, tol) {
                continue;
            }
            let area = shoelace(&l);
            if area > 0.0 {
                kept.push(l);
            } else {
                let mut r = l;
                r.reverse();
                kept.push(r);
            }
        }
        dedup_loops(&mut kept, tol);
        if kept.is_empty() {
            return Some(Regions::default());
        }
        let depths = depths_of(&kept, tol);
        Some(build_regions(kept, &depths, tol))
    }
}

fn sanitize_tol(t: f64) -> f64 {
    if t.is_finite() && t > 0.0 { t } else { 1e-12 }
}

/// Working tolerance: scaled to the bounds diagonal so the same shape behaves identically at
/// every scale, with an absolute floor so tiny geometry still merges.
fn tolerance(bb: Bounds2) -> f64 {
    let diag = bb.width().hypot(bb.height());
    if !diag.is_finite() || diag <= 0.0 {
        return 1e-12;
    }
    (diag * 1e-11).clamp(1e-13, 1e-3)
}

// ---------------------------------------------------------------------------------------------
// parity index: banded even-odd point-in-loops
// ---------------------------------------------------------------------------------------------

/// Even-odd point-in-loops with a horizontal band (active edge) index.
///
/// A ray-cast query at height `y` can only cross edges whose `y` range contains `y`, so edges are
/// bucketed by the bands their `y` range covers and a query scans one band. Bands are kept at
/// least eight tolerances tall, so a point within the tolerance of an edge always finds it.
struct Parity {
    bb: Bounds2,
    bands: usize,
    by: f64,
    y0: f64,
    grid: Vec<Vec<u32>>,
    edges: Vec<(Vec2, Vec2)>,
    tol: f64,
}

impl Parity {
    fn new(loops: &[Vec<Vec2>], tol: f64, bb: Bounds2) -> Option<Parity> {
        if bb.is_empty() {
            return None;
        }
        let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
        for l in loops {
            for w in l.windows(2) {
                if let [a, b] = w {
                    edges.push((*a, *b));
                }
            }
            if let (Some(first), Some(last)) = (l.first(), l.last())
                && !first.near(*last, tol)
            {
                edges.push((*last, *first));
            }
        }
        if edges.is_empty() {
            return None;
        }
        let bands = (edges.len() as f64).sqrt().ceil().clamp(1.0, 256.0) as usize;
        let y0 = bb.min.y;
        let by = (bb.height() / bands as f64).max(8.0 * tol).max(1e-300);
        let mut grid: Vec<Vec<u32>> = vec![Vec::new(); bands];
        for (i, (a, b)) in edges.iter().enumerate() {
            let i = i as u32;
            let b0 = Parity::band_of(y0, by, bands, a.y.min(b.y));
            let b1 = Parity::band_of(y0, by, bands, a.y.max(b.y));
            for k in grid.iter_mut().take(b1 + 1).skip(b0) {
                k.push(i);
            }
        }
        Some(Parity { bb, bands, by, y0, grid, edges, tol })
    }

    fn band_of(y0: f64, by: f64, bands: usize, y: f64) -> usize {
        let t = (y - y0) / by;
        if t.is_finite() { t.clamp(0.0, (bands - 1) as f64) as usize } else { 0 }
    }

    fn band(&self, y: f64) -> usize {
        Parity::band_of(self.y0, self.by, self.bands, y)
    }

    /// Parity at `p`, or `None` when `p` lies within the tolerance of an edge.
    fn raw(&self, p: Vec2) -> Option<bool> {
        if !p.is_finite() || !self.bb.expand(self.tol).contains(p) {
            return Some(false);
        }
        let list = self.grid.get(self.band(p.y))?;
        let mut inside = false;
        for &i in list {
            let Some((a, b)) = self.edges.get(i as usize) else { continue };
            if seg_distance(*a, *b, p) <= self.tol {
                return None;
            }
            if (a.y > p.y) != (b.y > p.y) {
                let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
                if x > p.x {
                    inside = !inside;
                }
            }
        }
        Some(inside)
    }

    /// Parity at `p`, stepping off the boundary when the query lands on an edge.
    fn contains(&self, p: Vec2) -> bool {
        if let Some(v) = self.raw(p) {
            return v;
        }
        for k in [8.0, 64.0, 512.0, 0.5] {
            let d = self.tol * k;
            for o in [Vec2::new(d, 0.0), Vec2::new(-d, 0.0), Vec2::new(0.0, d), Vec2::new(0.0, -d)] {
                if let Some(v) = self.raw(p + o) {
                    return v;
                }
            }
        }
        // On a boundary at every step: report outside, which drops a sliver rather than inventing
        // one.
        false
    }
}

// ---------------------------------------------------------------------------------------------
// edges, fragments
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Edge {
    a: Vec2,
    b: Vec2,
    /// 0 = subject A, 1 = subject B.
    subject: u8,
}

/// A split edge with how many times each subject contributed it.
#[derive(Clone, Copy, Debug)]
struct Frag {
    a: Vec2,
    b: Vec2,
    ca: u8,
    cb: u8,
}

impl Frag {
    fn mid(&self) -> Vec2 {
        self.a.mid(self.b)
    }
    /// A fragment is a real boundary edge when at least one subject contributes it an odd number
    /// of times (the even-odd rule, applied per subject).
    fn is_boundary(&self) -> bool {
        self.ca % 2 == 1 || self.cb % 2 == 1
    }
}

/// Drop non-finite points, zero-length edges and repeated vertices.
fn clean_loops(loops: Vec<Vec<Vec2>>, tol: f64) -> Option<Vec<Vec<Vec2>>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut total = 0usize;
    for l in loops {
        let mut v: Vec<Vec2> = Vec::with_capacity(l.len());
        for p in l {
            if p.is_finite() && !v.last().is_some_and(|q| q.near(p, tol)) {
                v.push(p);
            }
        }
        if let (Some(first), Some(last)) = (v.first().copied(), v.last().copied())
            && first.near(last, tol)
        {
            v.pop();
        }
        total += v.len();
        if total > MAX_LOOP_VERTS {
            return None;
        }
        if v.len() >= 3 {
            out.push(v);
        }
    }
    Some(out)
}

/// A loop keeps its place only if it encloses more than the tolerance squared.
fn keepable(l: &[Vec2], tol: f64) -> bool {
    if l.len() < 3 {
        return false;
    }
    if shoelace(l).abs() <= tol * tol {
        return false;
    }
    !l.windows(2).all(|w| match w {
        [a, b] => a.near(*b, tol * 8.0),
        _ => false,
    })
}

fn edges_of(loops: &[Vec<Vec2>], subject: u8) -> Vec<Edge> {
    let mut e = Vec::new();
    for l in loops {
        let n = l.len();
        for i in 0..n {
            if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) {
                e.push(Edge { a: *a, b: *b, subject });
            }
        }
    }
    e
}

/// Split every edge at every crossing, then emit fragments merged across subjects.
fn split_edges(edges: &[Edge], tol: f64) -> Option<Vec<Frag>> {
    let n = edges.len();
    if n == 0 || n > MAX_SEGMENTS * 2 {
        return None;
    }
    let bb = Bounds2::from_points(edges.iter().flat_map(|e| [e.a, e.b]));
    if bb.is_empty() {
        return None;
    }
    let mut cells = (n as f64).sqrt().ceil().clamp(1.0, 512.0) as usize;
    while cells > 1 && (bb.width() / cells as f64) < tol {
        cells /= 2;
    }
    let cw = (bb.width() / cells as f64).max(tol).max(1e-300);
    let ch = (bb.height() / cells as f64).max(tol).max(1e-300);
    let cell = |p: Vec2| {
        (
            ((p.x - bb.min.x) / cw).floor().clamp(0.0, (cells - 1) as f64) as usize,
            ((p.y - bb.min.y) / ch).floor().clamp(0.0, (cells - 1) as f64) as usize,
        )
    };
    // An ordered grid: the order pairs are examined in must be a function of the geometry alone,
    // or which of two nearly equal crossing points survives de-duplication changes run to run.
    let mut grid: BTreeMap<(usize, usize), Vec<u32>> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        let (x0, y0) = cell(e.a.min(e.b));
        let (x1, y1) = cell(e.a.max(e.b));
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 4096 {
            let sx = ((x1 - x0) / 64).max(1);
            let sy = ((y1 - y0) / 64).max(1);
            for gx in (x0..=x1).step_by(sx) {
                for gy in (y0..=y1).step_by(sy) {
                    grid.entry((gx, gy)).or_default().push(i as u32);
                }
            }
            continue;
        }
        for gx in x0..=x1 {
            for gy in y0..=y1 {
                grid.entry((gx, gy)).or_default().push(i as u32);
            }
        }
    }
    // Each split carries the *point*, not just the parameter: the two edges of a crossing then
    // get bit-identical endpoints instead of two roundings that differ in the last unit in the
    // last place. Without that the result depends on which edge the grid happened to offer first,
    // and union(a, b) differs from union(b, a).
    let mut splits: Vec<Vec<(f64, Vec2)>> = edges.iter().map(|e| vec![(0.0, e.a), (1.0, e.b)]).collect();
    let mut tested: HashSet<(u32, u32)> = HashSet::new();
    let mut budget = MAX_PAIR_TESTS;
    for list in grid.values() {
        for (k, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(k + 1) {
                if i == j || budget == 0 {
                    continue;
                }
                budget -= 1;
                let key = (i.min(j), i.max(j));
                if !tested.insert(key) {
                    continue;
                }
                let (Some(ei), Some(ej)) = (edges.get(i as usize), edges.get(j as usize)) else { continue };
                if let Some((p, t, u)) = line_line_infinite(ei.a, ei.b, ej.a, ej.b) {
                    // Only an intersection of the two *segments* splits them. The infinite lines
                    // also meet when one segment stops short, and splitting there would insert a
                    // vertex that is not a crossing.
                    if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                        if let Some(v) = splits.get_mut(i as usize) {
                            v.push((t.clamp(0.0, 1.0), p));
                        }
                        if let Some(v) = splits.get_mut(j as usize) {
                            v.push((u.clamp(0.0, 1.0), p));
                        }
                    }
                    continue;
                }
                // Parallel. When collinear, both edges must be split at the overlap ends so the
                // shared piece appears identically on both and merges below.
                let r = ei.b - ei.a;
                let s = ej.b - ej.a;
                if !(r.len() > tol && s.len() > tol) {
                    continue;
                }
                if (ej.a - ei.a).cross(r).abs() / r.len() > tol {
                    continue;
                }
                let rr = r.dot(r);
                let t0 = (ej.a - ei.a).dot(r) / rr;
                let t1 = (ej.b - ei.a).dot(r) / rr;
                let (lo, hi) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
                let (lo, hi) = (lo.max(0.0), hi.min(1.0));
                if hi - lo <= tol / r.len().max(1e-300) {
                    continue;
                }
                let p_lo = ei.a + r * lo;
                let p_hi = ei.a + r * hi;
                let ss = s.dot(s);
                if ss <= 0.0 || !ss.is_finite() {
                    continue;
                }
                let u_lo = ((p_lo - ej.a).dot(s) / ss).clamp(0.0, 1.0);
                let u_hi = ((p_hi - ej.a).dot(s) / ss).clamp(0.0, 1.0);
                if let Some(v) = splits.get_mut(i as usize) {
                    v.push((lo, p_lo));
                    v.push((hi, p_hi));
                }
                if let Some(v) = splits.get_mut(j as usize) {
                    v.push((u_lo, p_lo));
                    v.push((u_hi, p_hi));
                }
            }
        }
    }
    Some(emit_fragments(edges, &splits, tol, bb.min))
}

/// Emit fragments from split parameters, welding and merging identical ones.
fn emit_fragments(edges: &[Edge], splits: &[Vec<(f64, Vec2)>], tol: f64, origin: Vec2) -> Vec<Frag> {
    let mut raw: Vec<Frag> = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        let Some(mut ts) = splits.get(i).cloned() else { continue };
        ts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let l = e.a.dist(e.b);
        let span = tol / l.max(1e-300);
        ts.dedup_by(|a, b| (a.0 - b.0).abs() <= span || a.1.near(b.1, tol));
        for w in ts.windows(2) {
            let [(.., p), (.., q)] = *w else { continue };
            if !p.is_finite() || !q.is_finite() || p.near(q, tol) {
                continue;
            }
            let f = if e.subject == 0 { Frag { a: p, b: q, ca: 1, cb: 0 } } else { Frag { a: p, b: q, ca: 0, cb: 1 } };
            raw.push(f);
            if raw.len() > MAX_FRAGMENTS {
                raw.truncate(MAX_FRAGMENTS);
                break;
            }
        }
    }
    // Merge fragments that describe the same geometric piece (welded endpoints, either
    // direction), counting how often each subject contributed it.
    let mut index: HashMap<((i64, i64), (i64, i64)), usize> = HashMap::new();
    let mut out: Vec<Frag> = Vec::with_capacity(raw.len());
    for f in raw {
        let ka = vkey(f.a, origin, tol);
        let kb = vkey(f.b, origin, tol);
        let (lo, hi) = if ka <= kb { (ka, kb) } else { (kb, ka) };
        match index.entry((lo, hi)) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if let Some(t) = out.get_mut(*e.get()) {
                    t.ca = t.ca.saturating_add(f.ca);
                    t.cb = t.cb.saturating_add(f.cb);
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(out.len());
                out.push(f);
            }
        }
    }
    out.retain(|f| f.is_boundary());
    out
}

/// Direction-independent sort key for an edge: both endpoints welded and ordered.
fn edge_key(e: &Edge, origin: Vec2, tol: f64) -> ((i64, i64), (i64, i64)) {
    let (ka, kb) = (vkey(e.a, origin, tol), vkey(e.b, origin, tol));
    if ka <= kb { (ka, kb) } else { (kb, ka) }
}

fn vkey(p: Vec2, origin: Vec2, q: f64) -> (i64, i64) {
    // The drawing may be far from (0, 0); shift to the local origin before quantizing or large
    // coordinates saturate i64 and alias distinct vertices onto one key.
    let d = q.max(1e-300);
    (((p.x - origin.x) / d).round() as i64, ((p.y - origin.y) / d).round() as i64)
}

// ---------------------------------------------------------------------------------------------
// classification and assembly
// ---------------------------------------------------------------------------------------------

/// Is the fragment on the boundary of the result? The fill rule is evaluated at a point just to
/// each side of the fragment against both subjects, and the fragment survives only when the two
/// sides disagree.
fn survives(f: &Frag, pa: &Parity, pb: &Parity, op: BoolOp, probe: f64) -> bool {
    let r = f.b - f.a;
    let l = r.len();
    if l <= 0.0 || !l.is_finite() {
        return false;
    }
    let mid = f.mid();
    let n = r.perp().normalized() * probe;
    let (p1, p2) = (mid + n, mid - n);
    let material = |p: Vec2| -> bool {
        let ia = pa.contains(p);
        let ib = pb.contains(p);
        match op {
            BoolOp::Union => ia || ib,
            BoolOp::Intersect => ia && ib,
            BoolOp::Difference => ia && !ib,
            BoolOp::Xor => ia != ib,
        }
    };
    material(p1) != material(p2)
}

/// Chain fragments into closed loops, always taking the face on the left of the edge.
fn assemble(frags: &[Frag], tol: f64) -> Option<Vec<Vec<Vec2>>> {
    if frags.is_empty() {
        return Some(Vec::new());
    }
    let origin = Bounds2::from_points(frags.iter().flat_map(|f| [f.a, f.b])).min;
    let mut pts: Vec<Vec2> = Vec::with_capacity(frags.len());
    let mut vindex: HashMap<(i64, i64), usize> = HashMap::new();
    let vid = |p: Vec2, pts: &mut Vec<Vec2>, vindex: &mut HashMap<(i64, i64), usize>| -> usize {
        let k = vkey(p, origin, tol);
        *vindex.entry(k).or_insert_with(|| {
            pts.push(p);
            pts.len() - 1
        })
    };
    // Endpoints of every fragment as vertex ids.
    let mut ends: Vec<(usize, usize)> = Vec::with_capacity(frags.len());
    for f in frags {
        let u = vid(f.a, &mut pts, &mut vindex);
        let v = vid(f.b, &mut pts, &mut vindex);
        ends.push((u, v));
    }
    // Adjacency by vertex: from this vertex, this fragment leads to `to`.
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); pts.len()];
    for (i, (u, v)) in ends.iter().enumerate() {
        if u == v {
            continue;
        }
        if let Some(a) = adj.get_mut(*u) {
            a.push((i, *v));
        }
        if let Some(a) = adj.get_mut(*v) {
            a.push((i, *u));
        }
    }
    let mut used = vec![false; frags.len()];
    let mut loops: Vec<Vec<Vec2>> = Vec::new();
    let mut budget = (MAX_LOOP_VERTS + frags.len()) as isize;
    for start in 0..frags.len() {
        if used.get(start) == Some(&true) {
            continue;
        }
        budget -= 1;
        if budget < 0 {
            return None;
        }
        let Some((from0, to0)) = ends.get(start).copied() else { continue };
        let mut out: Vec<Vec2> = Vec::new();
        let (mut from, mut to) = (from0, to0);
        let mut cur = start;
        let mut closed = false;
        for _ in 0..=frags.len() {
            used[cur] = true;
            let (Some(pfrom), Some(pto)) = (pts.get(from).copied(), pts.get(to).copied()) else { break };
            out.push(pfrom);
            // Choose the next fragment at `to`: the one making the smallest counter-clockwise
            // turn from the reversed incoming direction. That keeps the traced face on the left,
            // which is what makes tangencies and crossings trace correctly.
            let back = pfrom - pto;
            let mut best: Option<(f64, usize, usize)> = None;
            for &(e, nb) in adj.get(to).into_iter().flatten() {
                if e == cur {
                    continue;
                }
                // The fragment the walk started with is the only way back to its first vertex,
                // so it stays a candidate there even though it is marked used.
                let closes = e == start && to == from0;
                if used.get(e) == Some(&true) && !closes {
                    continue;
                }
                let Some(pnb) = pts.get(nb).copied() else { continue };
                let mut cw = norm_angle(back.angle() - (pnb - pto).angle());
                if cw < 1e-12 {
                    cw = std::f64::consts::TAU;
                }
                if best.is_none_or(|(bc, _, _)| cw < bc) {
                    best = Some((cw, e, nb));
                }
            }
            let Some((_, e, nb)) = best else { break };
            if e == start && nb == to0 && to == from0 {
                closed = true;
                break;
            }
            from = to;
            to = nb;
            cur = e;
        }
        if closed && out.len() >= 3 && keepable(&out, tol) {
            loops.push(out);
            if loops.len() > MAX_LOOPS {
                return None;
            }
        }
    }
    Some(loops)
}

/// Drop vertices that lie on the straight line between their neighbours.
///
/// Splitting leaves the points where subjects touched, and a run of collinear points is not an
/// edge of the result — dropping it keeps output loops minimal and makes vertex counts
/// predictable. Spikes (a reversal) are kept: they carry a real turn.
fn drop_collinear(l: Vec<Vec2>) -> Vec<Vec2> {
    let n = l.len();
    if n < 3 {
        return l;
    }
    let mut keep: Vec<Vec2> = Vec::with_capacity(n);
    for i in 0..n {
        let prev = if i == 0 { l.get(n - 1) } else { l.get(i - 1) };
        let next = if i + 1 == n { l.first() } else { l.get(i + 1) };
        let collinear = match (prev, l.get(i), next) {
            (Some(p), Some(v), Some(q)) => {
                let d1 = *v - *p;
                let d2 = *q - *v;
                let scale = d1.len() * d2.len();
                d1.dot(d2) > 0.0 && d1.cross(d2).abs() <= 1e-9 * scale.max(1e-300)
            }
            _ => false,
        };
        if let Some(v) = l.get(i)
            && !collinear
        {
            keep.push(*v);
        }
    }
    if keep.len() < 3 { l } else { keep }
}

/// Rotate a loop to start at its lexicographically smallest vertex and orient outer loops
/// counter-clockwise, holes clockwise.
fn normalize_loop(l: Vec<Vec2>, ccw: bool) -> Vec<Vec2> {
    let mut l = drop_collinear(l);
    if shoelace(&l) < 0.0 {
        l.reverse();
    }
    if !ccw {
        l.reverse();
    }
    let Some((i, _)) = l.iter().enumerate().min_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.y.total_cmp(&b.1.y))) else {
        return l;
    };
    l.rotate_left(i);
    l
}

/// Drop loops that are the same geometry, reversed duplicates included.
fn dedup_loops(loops: &mut Vec<Vec<Vec2>>, tol: f64) {
    let mut seen: HashMap<Vec<(i64, i64)>, ()> = HashMap::new();
    loops.retain(|l| {
        let mut k: Vec<(i64, i64)> = l.iter().map(|p| vkey(*p, Vec2::ZERO, tol)).collect();
        k.sort_unstable();
        seen.insert(k, ()).is_none()
    });
}

/// Sort loops into regions by nesting depth: even depth starts a region, odd depth is a hole in
/// the region enclosing it.
fn build_regions(loops: Vec<Vec<Vec2>>, depths: &[usize], tol: f64) -> Regions {
    let mut regions: Vec<Region> = Vec::new();
    let mut owner: Vec<usize> = vec![usize::MAX; loops.len()];
    for (i, d) in depths.iter().enumerate() {
        if d % 2 == 0 {
            let Some(l) = loops.get(i) else { continue };
            regions.push(Region::new(normalize_loop(l.clone(), true)));
            if let Some(o) = owner.get_mut(i) {
                *o = regions.len() - 1;
            }
        }
    }
    for (i, d) in depths.iter().enumerate() {
        if d % 2 == 0 {
            continue;
        }
        // A vertex of the hole lies strictly inside the region that owns it; the smallest such
        // region is the innermost one.
        let Some(v) = loops.get(i).and_then(|l| l.first().copied()) else { continue };
        let mut best: Option<(f64, usize)> = None;
        for (j, o) in owner.iter().enumerate() {
            if *o == usize::MAX || depths.get(j).map(|x| x % 2 != 0).unwrap_or(true) {
                continue;
            }
            let Some(area) = regions.get(*o).map(|r| r.area()) else { continue };
            if let Some((ba, _)) = best
                && area >= ba
            {
                continue;
            }
            if region_contains(regions.get(*o), v, tol) {
                best = Some((area, *o));
            }
        }
        if let Some((_, o)) = best
            && let Some(l) = loops.get(i)
            && let Some(r) = regions.get_mut(o)
        {
            r.holes.push(normalize_loop(l.clone(), false));
        }
    }
    regions.retain(|r| r.outer.len() >= 3);
    Regions::new(regions)
}

fn regions_from_loops(loops: Vec<Vec<Vec2>>, tol: f64) -> Regions {
    if loops.is_empty() {
        return Regions::default();
    }
    let depths = depths_of(&loops, tol);
    build_regions(loops, &depths, tol)
}

// ---------------------------------------------------------------------------------------------
// point-in-loop helpers
// ---------------------------------------------------------------------------------------------

/// Is `p` inside the simple loop `l`, treating the boundary as outside?
fn loop_contains(l: &[Vec2], p: Vec2, tol: f64) -> bool {
    if l.len() < 3 {
        return false;
    }
    let n = l.len();
    for i in 0..n {
        if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n))
            && seg_distance(*a, *b, p) <= tol
        {
            return false;
        }
    }
    point_in_polygon(l, p)
}

/// Does `outer` enclose loop `inner`? Loops that come out of a boolean never cross, so a single
/// vertex of `inner` strictly inside `outer` settles it. (Interior points do not: nested loops can
/// share one.)
fn loop_encloses(outer: &[Vec2], inner: &[Vec2], tol: f64) -> bool {
    inner.iter().any(|v| loop_contains(outer, *v, tol))
}

/// Nesting depth of every loop: how many other loops enclose it.
fn depths_of(loops: &[Vec<Vec2>], tol: f64) -> Vec<usize> {
    loops.iter().enumerate().map(|(i, l)| loops.iter().enumerate().filter(|(j, o)| *j != i && loop_encloses(o, l, tol)).count()).collect()
}

fn region_contains(r: Option<&Region>, p: Vec2, tol: f64) -> bool {
    let Some(r) = r else { return false };
    if !loop_contains(&r.outer, p, tol) {
        return false;
    }
    !r.holes.iter().any(|h| loop_contains(h, p, tol))
}

/// Distance from `p` to the segment.
fn seg_distance(a: Vec2, b: Vec2, p: Vec2) -> f64 {
    let r = b - a;
    let l2 = r.dot(r);
    if l2 <= 0.0 || !l2.is_finite() {
        return p.dist(a);
    }
    let t = ((p - a).dot(r) / l2).clamp(0.0, 1.0);
    (p - (a + r * t)).len()
}

// ---------------------------------------------------------------------------------------------
// public entry points
// ---------------------------------------------------------------------------------------------

/// Boolean operation between two sets of regions.
///
/// `None` only for hostile or oversized input; a well-formed empty result is an empty `Regions`.
pub fn boolean(a: &Regions, b: &Regions, op: BoolOp) -> Option<Regions> {
    if a.is_empty() || b.is_empty() {
        return Some(match op {
            BoolOp::Union => a.clone(),
            BoolOp::Intersect | BoolOp::Difference | BoolOp::Xor => Regions::default(),
        });
    }
    let bb = a.bounds().union(&b.bounds());
    let tol = tolerance(bb);
    let la = clean_loops(a.iter().flat_map(|r| r.loops().cloned()).collect(), tol)?;
    let lb = clean_loops(b.iter().flat_map(|r| r.loops().cloned()).collect(), tol)?;
    if la.len() > MAX_LOOPS || lb.len() > MAX_LOOPS {
        return None;
    }
    let mut edges = edges_of(&la, 0);
    edges.extend(edges_of(&lb, 1));
    if edges.len() > MAX_SEGMENTS * 2 {
        return None;
    }
    // Order the edges by geometry, not by which operand contributed them: the split then runs in
    // an order that does not depend on the argument order, so union(a, b) and union(b, a) agree
    // to the last bit. The sort is stable, so an edge both operands share keeps subject A's copy.
    edges.sort_by_key(|e| edge_key(e, bb.min, tol));
    let frags = split_edges(&edges, tol)?;
    if frags.is_empty() {
        return Some(Regions::default());
    }
    let pa = Parity::new(&la, tol, bb)?;
    let pb = Parity::new(&lb, tol, bb)?;
    let probe = tol * 8.0;
    let kept: Vec<Frag> = frags.into_iter().filter(|f| survives(f, &pa, &pb, op, probe)).collect();
    let loops = assemble(&kept, tol)?;
    Some(regions_from_loops(loops, tol))
}

/// Union of two region sets.
pub fn union(a: &Regions, b: &Regions) -> Option<Regions> {
    boolean(a, b, BoolOp::Union)
}

/// Intersection of two region sets.
pub fn intersect(a: &Regions, b: &Regions) -> Option<Regions> {
    boolean(a, b, BoolOp::Intersect)
}

/// `a` minus `b`.
pub fn difference(a: &Regions, b: &Regions) -> Option<Regions> {
    boolean(a, b, BoolOp::Difference)
}

/// Symmetric difference.
pub fn xor(a: &Regions, b: &Regions) -> Option<Regions> {
    boolean(a, b, BoolOp::Xor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]
    }
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
        Region::new(square(x0, y0, x1, y1))
    }
    fn one(r: Region) -> Regions {
        Regions::new(vec![r])
    }
    fn hole_region() -> Region {
        Region { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![square(3.0, 3.0, 7.0, 7.0)] }
    }

    #[test]
    fn area_of_a_square_and_its_hole() {
        assert!((hole_region().area() - 84.0).abs() < 1e-9);
        assert!((hole_region().signed_area() - 84.0).abs() < 1e-9);
    }

    #[test]
    fn two_overlapping_squares() {
        let a = one(rect(0.0, 0.0, 2.0, 2.0));
        let b = one(rect(1.0, 1.0, 3.0, 3.0));
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 7.0).abs() < 1e-9, "union area {}", u.area());
        let i = intersect(&a, &b).unwrap_or_default();
        assert!((i.area() - 1.0).abs() < 1e-9, "intersect area {}", i.area());
        let d = difference(&a, &b).unwrap_or_default();
        assert!((d.area() - 3.0).abs() < 1e-9, "difference area {}", d.area());
        let x = xor(&a, &b).unwrap_or_default();
        assert!((x.area() - 6.0).abs() < 1e-9, "xor area {}", x.area());
    }

    #[test]
    fn disjoint_union_is_two_regions() {
        let a = one(rect(0.0, 0.0, 1.0, 1.0));
        let b = one(rect(5.0, 5.0, 6.0, 6.0));
        let u = union(&a, &b).unwrap_or_default();
        assert_eq!(u.len(), 2);
        assert!((u.area() - 2.0).abs() < 1e-9);
        let i = intersect(&a, &b).unwrap_or_default();
        assert!(i.is_empty());
        let d = difference(&a, &b).unwrap_or_default();
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn identical_inputs() {
        let a = one(rect(0.0, 0.0, 4.0, 3.0));
        assert!((union(&a, &a).unwrap_or_default().area() - 12.0).abs() < 1e-9);
        assert!((intersect(&a, &a).unwrap_or_default().area() - 12.0).abs() < 1e-9);
        assert!(difference(&a, &a).unwrap_or_default().is_empty());
        assert!(xor(&a, &a).unwrap_or_default().is_empty());
    }

    #[test]
    fn nested_region_is_a_hole_after_difference() {
        let a = one(rect(0.0, 0.0, 10.0, 10.0));
        let b = one(rect(3.0, 3.0, 7.0, 7.0));
        let d = difference(&a, &b).unwrap_or_default();
        assert_eq!(d.len(), 1);
        assert!((d.area() - 84.0).abs() < 1e-9);
        assert_eq!(d.first().map(|r| r.holes.len()), Some(1));
        let i = intersect(&a, &b).unwrap_or_default();
        assert_eq!(i.len(), 1);
        assert!((i.area() - 16.0).abs() < 1e-9);
        assert_eq!(i.first().map(|r| r.holes.len()), Some(0));
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_square_lying_in_a_hole() {
        let a = one(hole_region());
        let b = one(rect(3.0, 3.0, 7.0, 7.0));
        // b lies entirely in the void of a, so subtracting it changes nothing.
        let d = difference(&a, &b).unwrap_or_default();
        assert_eq!(d.len(), 1);
        assert_eq!(d.first().map(|r| r.holes.len()), Some(1));
        assert!((d.area() - 84.0).abs() < 1e-9, "difference {}", d.area());
        // The union is the full square with no hole left.
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 100.0).abs() < 1e-9, "union {}", u.area());
        assert_eq!(u.first().map(|r| r.holes.len()), Some(0));
        // The square lies in the void of a, so the intersection is empty.
        let i = intersect(&a, &b).unwrap_or_default();
        assert!(i.is_empty(), "intersect area {}", i.area());
    }

    #[test]
    fn squares_sharing_an_edge() {
        // Touching along x = 1: the shared edge is interior to the union.
        let a = one(rect(0.0, 0.0, 1.0, 1.0));
        let b = one(rect(1.0, 0.0, 2.0, 1.0));
        let u = union(&a, &b).unwrap_or_default();
        assert_eq!(u.len(), 1, "shared edge must not survive the union");
        assert!((u.area() - 2.0).abs() < 1e-9);
        assert_eq!(u.first().map(|r| r.outer.len()), Some(4));
        let d = difference(&a, &b).unwrap_or_default();
        assert!((d.area() - 1.0).abs() < 1e-9);
        assert_eq!(d.first().map(|r| r.holes.len()), Some(0));
    }

    #[test]
    fn rectangles_touching_along_an_edge() {
        // b covers the top half of a's right edge.
        let a = one(rect(0.0, 0.0, 2.0, 2.0));
        let b = one(rect(2.0, 1.0, 4.0, 3.0));
        // They touch along x = 2 without overlapping, so the union keeps the full area of both.
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 8.0).abs() < 1e-9, "union {}", u.area());
        assert_eq!(u.len(), 1);
        let d = difference(&a, &b).unwrap_or_default();
        assert!((d.area() - 4.0).abs() < 1e-9, "difference {}", d.area());
    }

    #[test]
    fn rectangles_touching_on_a_collinear_piece() {
        // Both inputs span x = 1..3 on y = 0, so the shared piece is collinear.
        let a = one(rect(0.0, -1.0, 3.0, 0.0));
        let b = one(rect(1.0, 0.0, 4.0, 1.0));
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 6.0).abs() < 1e-9, "union {}", u.area());
        let d = difference(&a, &b).unwrap_or_default();
        assert!((d.area() - 3.0).abs() < 1e-9, "difference {}", d.area());
    }

    #[test]
    fn a_notch_on_one_edge() {
        let a = one(rect(0.0, 0.0, 4.0, 4.0));
        let b = one(rect(1.0, 3.0, 3.0, 5.0));
        let d = difference(&a, &b).unwrap_or_default();
        assert!((d.area() - 14.0).abs() < 1e-9, "difference {}", d.area());
        assert_eq!(d.len(), 1);
        assert_eq!(d.first().map(|r| r.holes.len()), Some(0));
        let u = union(&a, &b).unwrap_or_default();
        assert!((u.area() - 18.0).abs() < 1e-9, "union {}", u.area());
    }

    #[test]
    fn hole_inside_a_hole_stays_a_region() {
        // a has a hole; b sits inside that hole.
        let a = one(hole_region());
        let b = one(rect(4.0, 4.0, 6.0, 6.0));
        let u = union(&a, &b).unwrap_or_default();
        assert_eq!(u.len(), 2, "the island in the hole is its own region");
        assert!((u.area() - 88.0).abs() < 1e-9, "union {}", u.area());
    }

    #[test]
    fn output_is_deterministic() {
        let a = one(rect(0.0, 0.0, 2.0, 2.0));
        let b = one(rect(1.0, 1.0, 3.0, 3.0));
        let u1 = union(&a, &b).unwrap_or_default();
        let u2 = union(&a, &b).unwrap_or_default();
        assert_eq!(u1, u2);
        let d1 = difference(&a, &b).unwrap_or_default();
        let d2 = difference(&b, &a).unwrap_or_default();
        // Opposite subtractions are different shapes.
        assert!((d1.area() - d2.area()).abs() < 1e-9);
        assert_ne!(d1, d2);
    }

    #[test]
    fn outer_loops_are_ccw_and_holes_cw() {
        let a = one(rect(0.0, 0.0, 10.0, 10.0));
        let b = one(rect(3.0, 3.0, 7.0, 7.0));
        let d = difference(&a, &b).unwrap_or_default();
        let r = d.first().cloned().unwrap_or_default();
        assert!(shoelace(&r.outer) > 0.0);
        for h in &r.holes {
            assert!(shoelace(h) < 0.0, "hole must be clockwise");
        }
    }

    #[test]
    fn input_orientation_does_not_matter() {
        let a1 = one(Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)]));
        let mut a2 = a1.first().map(|r| r.outer.clone()).unwrap_or_default();
        a2.reverse();
        let a2 = one(Region::new(a2));
        let b = one(rect(1.0, 1.0, 3.0, 3.0));
        assert_eq!(union(&a1, &b).unwrap_or_default(), union(&a2, &b).unwrap_or_default());
    }

    #[test]
    fn self_intersecting_input_is_even_odd() {
        // A bowtie: two triangles, area 1, crossing in the middle.
        let bow = one(Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)]));
        // shoelace of a self-crossing loop cancels; the filled (even-odd) area is 2.
        assert!((bow.area() - 0.0).abs() < 1e-9);
        assert!((bow.area() - 0.0).abs() < 1e-9);
        let cut = one(rect(-1.0, 0.0, 3.0, 0.5));
        let d = difference(&bow, &cut).unwrap_or_default();
        // The bowtie encloses 2 (two triangles of area 1); below y = 0.5 only 0.25 survives.
        assert!((d.area() - 1.75).abs() < 1e-9, "even-odd difference {}", d.area());
    }

    #[test]
    fn scale_invariance() {
        for k in [1e-6, 1.0, 1e6] {
            let a = one(rect(0.0, 0.0, 2.0 * k, 2.0 * k));
            let b = one(rect(k, k, 3.0 * k, 3.0 * k));
            let u = union(&a, &b).unwrap_or_default();
            assert!(((u.area() / (k * k)) - 7.0).abs() < 1e-6, "scale {k}: area {}", u.area());
            assert_eq!(u.first().map(|r| r.outer.len()), Some(8), "scale {k}: vertices");
        }
    }

    #[test]
    fn hostile_input_never_panics() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let bad_loop = vec![Vec2::new(nan, 0.0), Vec2::new(0.0, 1.0), Vec2::new(1.0, inf)];
        let bad = one(Region::new(bad_loop));
        let ok = one(rect(0.0, 0.0, 1.0, 1.0));
        let _ = union(&bad, &ok);
        let _ = difference(&ok, &bad);
        let _ = intersect(&bad, &ok);
        let _ = xor(&ok, &bad);
        // Degenerate loops.
        let _ = union(&one(Region::new(vec![Vec2::new(1.0, 1.0); 8])), &ok);
        let _ = union(&one(Region::new(vec![Vec2::new(1.0, 1.0), Vec2::new(1.0, 1.0)])), &ok);
        // Huge values.
        let huge = one(rect(-1e300, -1e300, 1e300, 1e300));
        let _ = union(&huge, &huge);
        let _ = difference(&huge, &ok);
        // Empty inputs.
        assert!(union(&Regions::default(), &ok).unwrap_or_default().is_empty());
        assert!(intersect(&ok, &Regions::default()).unwrap_or_default().is_empty());
    }

    #[test]
    fn from_loops_sorts_nesting() {
        let loops = vec![square(0.0, 0.0, 10.0, 10.0), square(3.0, 3.0, 7.0, 7.0), square(4.0, 4.0, 6.0, 6.0)];
        let r = Regions::from_loops(loops, 1e-9).unwrap_or_default();
        assert_eq!(r.len(), 2, "outer, hole and island");
        assert!((r.area() - 88.0).abs() < 1e-9, "from_loops area {}", r.area());
    }

    #[test]
    fn contains_agrees_with_area() {
        let a = one(hole_region());
        assert!(a.contains(Vec2::new(1.0, 1.0)));
        assert!(!a.contains(Vec2::new(5.0, 5.0)), "the hole is not in the region");
        assert!(!a.contains(Vec2::new(50.0, 1.0)));
    }
}
