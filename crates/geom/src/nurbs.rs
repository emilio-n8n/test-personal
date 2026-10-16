//! NURBS operations for the editing commands (TRIM, EXTEND, JOIN, LENGTHEN, FILLET): first
//! derivatives, knot insertion (Boehm), splitting, clamping, Bézier decomposition, degree
//! elevation, end-to-end joining, rational splines for lines and circular/elliptical arcs,
//! straight end extensions and arc length; plus arc length along an ellipse.
//!
//! Textbook algorithms written from the mathematics. Every loop is capped and degenerate input
//! gives `None` (or a zero vector), never a panic or NaN.

use std::f64::consts::FRAC_PI_2;

use crate::spline::{basis, find_span};
use crate::{EPS, Ellipse, Spline, TAU, Vec2};

/// Highest degree the editing operations work with.
pub const MAX_EDIT_DEGREE: usize = 25;
/// Most control points a single knot insertion grows a spline to.
const MAX_EDIT_CONTROL: usize = 50_000;
/// Most knot insertions one split makes (the degree of any spline drawn in practice).
const MAX_SPLIT_INSERTS: usize = 64;
/// Most control points a Bézier decomposition (and so a degree elevation or join) produces.
const MAX_DECOMPOSED: usize = 10_000;

/// A homogeneous control point: (w·P, w).
type H = (Vec2, f64);

fn hlerp(a: H, b: H, t: f64) -> H {
    (a.0 * (1.0 - t) + b.0 * t, a.1 * (1.0 - t) + b.1 * t)
}

fn hpoint(h: H) -> Vec2 {
    if h.1.abs() > 1e-300 { h.0 / h.1 } else { h.0 }
}

fn poly_len(pts: &[Vec2]) -> f64 {
    pts.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum()
}

/// Five-point Gauss–Legendre nodes and weights on [-1, 1].
const GAUSS: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

fn gauss(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let (m, h) = ((a + b) / 2.0, (b - a) / 2.0);
    GAUSS.iter().map(|(x, w)| w * f(m + h * x)).sum::<f64>() * h
}

/// Most halvings `adaptive_gauss` makes of one panel: with `param_at_length`'s 60 bisections
/// this bounds the work a hostile spline can ask for.
const MAX_GAUSS_DEPTH: usize = 10;

/// `gauss` over [a, b], halving until the two halves agree with the whole: a fixed panel count
/// is far off on a tight bend, and the arc-length inverse must agree with the measure.
fn adaptive_gauss(a: f64, b: f64, whole: f64, f: &impl Fn(f64) -> f64, depth: usize) -> f64 {
    let m = (a + b) / 2.0;
    let (l, r) = (gauss(a, m, f), gauss(m, b, f));
    if depth == 0 || !(l + r).is_finite() || (l + r - whole).abs() <= 1e-10 * (l + r).abs() {
        return l + r;
    }
    adaptive_gauss(a, m, l, f, depth - 1) + adaptive_gauss(m, b, r, f, depth - 1)
}

/// Clamped knots over Bézier pieces meeting at `breaks`, for degree `p`.
fn bezier_knots(breaks: &[f64], p: usize) -> Vec<f64> {
    let mut k = Vec::with_capacity(breaks.len() * p + 2);
    let n = breaks.len();
    for (i, b) in breaks.iter().enumerate() {
        let m = if i == 0 || i + 1 == n { p + 1 } else { p };
        k.extend(std::iter::repeat_n(*b, m));
    }
    k
}

/// One degree elevation of a homogeneous Bézier piece.
fn elevate_bezier(p: &[H]) -> Vec<H> {
    let n = p.len();
    let zero = (Vec2::ZERO, 0.0);
    (0..=n)
        .map(|i| {
            let prev = i.checked_sub(1).and_then(|j| p.get(j)).copied().unwrap_or(zero);
            let cur = p.get(i).copied().unwrap_or(zero);
            hlerp(cur, prev, i as f64 / n as f64)
        })
        .collect()
}

impl Spline {
    fn weight(&self, i: usize) -> f64 {
        if self.weights.is_empty() { 1.0 } else { self.weights.get(i).copied().unwrap_or(1.0) }
    }

    /// True when the spline carries weights (a NURBS rather than a plain B-spline).
    pub fn is_rational(&self) -> bool {
        !self.weights.is_empty()
    }

    fn homogeneous(&self) -> Vec<H> {
        self.control
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let w = self.weight(i);
                (*c * w, w)
            })
            .collect()
    }

    fn from_homogeneous(degree: usize, knots: Vec<f64>, h: &[H], rational: bool) -> Spline {
        Spline {
            degree,
            knots,
            control: h.iter().map(|x| hpoint(*x)).collect(),
            weights: if rational { h.iter().map(|x| x.1).collect() } else { Vec::new() },
            ..Spline::default()
        }
    }

    /// First derivative dC/dt (zero on an invalid spline).
    pub fn derivative(&self, t: f64) -> Vec2 {
        if !self.is_valid() {
            return Vec2::ZERO;
        }
        let p = self.degree;
        let n = self.control.len() - 1;
        let (lo, hi) = self.domain();
        let t = if t.is_finite() { t.clamp(lo, hi) } else { lo };
        let span = find_span(n, p, t, &self.knots);
        let nb = basis(span, t, p, &self.knots);
        let nd = basis(span, t, p - 1, &self.knots);
        let k = |i: usize| self.knots.get(i).copied().unwrap_or(0.0);
        let ratio = |num: Option<&f64>, den: f64| if den.abs() > 1e-300 { num.copied().unwrap_or(0.0) / den } else { 0.0 };
        let (mut a, mut w, mut da, mut dw) = (Vec2::ZERO, 0.0, Vec2::ZERO, 0.0);
        for j in 0..=p {
            let Some(i) = (span + j).checked_sub(p) else { continue };
            let Some(c) = self.control.get(i) else { continue };
            let wi = self.weight(i);
            let n0 = nb.get(j).copied().unwrap_or(0.0);
            // N'_{i,p} = p (N_{i,p-1} / (u_{i+p} - u_i) - N_{i+1,p-1} / (u_{i+p+1} - u_{i+1})).
            let left = if j >= 1 { ratio(nd.get(j - 1), k(i + p) - k(i)) } else { 0.0 };
            let right = if j < p { ratio(nd.get(j), k(i + p + 1) - k(i + 1)) } else { 0.0 };
            let dn = p as f64 * (left - right);
            a += *c * (n0 * wi);
            w += n0 * wi;
            da += *c * (dn * wi);
            dw += dn * wi;
        }
        if w.abs() < 1e-300 {
            return Vec2::ZERO;
        }
        let d = (da - a / w * dw) / w;
        if d.is_finite() { d } else { Vec2::ZERO }
    }

    /// Unit tangent at `t` in the direction of increasing parameter; a central difference stands
    /// in where the derivative vanishes. Zero when the curve does not move at all.
    pub fn unit_tangent(&self, t: f64) -> Vec2 {
        let d = self.derivative(t).normalized();
        if d != Vec2::ZERO {
            return d;
        }
        let (lo, hi) = self.domain();
        let h = (hi - lo) * 1e-6;
        (self.eval((t + h).min(hi)) - self.eval((t - h).max(lo))).normalized()
    }

    /// Insert knot `t` once (Boehm's algorithm, in homogeneous coordinates). The result has no
    /// fit data and is open.
    pub fn insert_knot(&self, t: f64) -> Option<Spline> {
        if !self.is_valid() || !t.is_finite() || self.control.len() >= MAX_EDIT_CONTROL {
            return None;
        }
        let p = self.degree;
        let n = self.control.len();
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        // Span k: the last knot index in p..n with knots[k] <= t.
        let mut k = p;
        for i in p..n {
            if *self.knots.get(i)? <= t {
                k = i;
            }
        }
        let h = self.homogeneous();
        let mut out = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let pt = if i + p <= k {
                *h.get(i)?
            } else if i > k {
                *h.get(i - 1)?
            } else {
                let ki = *self.knots.get(i)?;
                let kip = *self.knots.get(i + p)?;
                let a = if (kip - ki).abs() < 1e-300 { 0.0 } else { (t - ki) / (kip - ki) };
                hlerp(*h.get(i.checked_sub(1)?)?, *h.get(i)?, a)
            };
            out.push(pt);
        }
        let mut knots = self.knots.clone();
        knots.insert(k + 1, t);
        Some(Spline::from_homogeneous(p, knots, &out, self.is_rational()))
    }

    /// Cut at `t` (inserting it to multiplicity `degree`); either side may be invalid when `t`
    /// is at or beyond an end.
    fn split_raw(&self, t: f64) -> Option<(Spline, Spline)> {
        let same = |k: &f64| (*k - t).abs() < 1e-12;
        let mut s = self.clone();
        s.fit.clear();
        let mut guard = 0;
        while s.knots.iter().filter(|k| same(k)).count() < s.degree && guard < MAX_SPLIT_INSERTS {
            s = s.insert_knot(t)?;
            guard += 1;
        }
        let p = s.degree;
        if s.knots.iter().filter(|k| same(k)).count() != p {
            return None;
        }
        let first = s.knots.iter().position(same)?;
        let mut lk = s.knots.get(..first)?.to_vec();
        lk.extend(std::iter::repeat_n(t, p + 1));
        let lc = s.control.get(..first)?.to_vec();
        let lw = if s.weights.is_empty() { Vec::new() } else { s.weights.get(..first)?.to_vec() };
        let rs = first.checked_sub(1)?;
        let mut rk = vec![t; p + 1];
        rk.extend(s.knots.get(first + p..)?.iter().copied());
        let rc = s.control.get(rs..)?.to_vec();
        let rw = if s.weights.is_empty() { Vec::new() } else { s.weights.get(rs..)?.to_vec() };
        Some((
            Spline { degree: p, knots: lk, control: lc, weights: lw, ..Spline::default() },
            Spline { degree: p, knots: rk, control: rc, weights: rw, ..Spline::default() },
        ))
    }

    /// Split at parameter `t` into (before, after). `None` at the ends or on invalid input.
    pub fn split_at(&self, t: f64) -> Option<(Spline, Spline)> {
        let (lo, hi) = self.domain();
        if !(t > lo + 1e-12 && t < hi - 1e-12) {
            return None;
        }
        let (l, r) = self.split_raw(t)?;
        (l.is_valid() && r.is_valid()).then_some((l, r))
    }

    /// The open piece between parameters `a < b`.
    pub fn sub_between(&self, a: f64, b: f64) -> Option<Spline> {
        let (lo, hi) = self.domain();
        let mut s = self.clone();
        s.fit.clear();
        s.closed = false;
        if b < hi - 1e-12 {
            s = s.split_at(b)?.0;
        }
        if a > lo + 1e-12 {
            s = s.split_at(a)?.1;
        }
        Some(s)
    }

    /// The same curve with clamped ends (end knots repeated degree + 1 times), so it starts and
    /// ends on its first and last control points. Fit data is dropped.
    pub fn clamped(&self) -> Option<Spline> {
        if !self.is_valid() {
            return None;
        }
        let p = self.degree;
        let (lo, hi) = self.domain();
        let mut s = self.clone();
        s.fit.clear();
        s.fit_opts = Default::default();
        if !s.knots.get(..=p).is_some_and(|k| k.iter().all(|x| (*x - lo).abs() < 1e-12)) {
            s = s.split_raw(lo)?.1;
        }
        let n = s.control.len();
        if !s.knots.get(n..).is_some_and(|k| k.iter().all(|x| (*x - hi).abs() < 1e-12)) {
            s = s.split_raw(hi)?.0;
        }
        s.closed = self.closed;
        s.is_valid().then_some(s)
    }

    /// Reversed direction: control points, weights, fit points and knots (and the fit end
    /// tangents swap ends and turn round).
    pub fn reversed(&self) -> Spline {
        let (lo, hi) = (self.knots.first().copied().unwrap_or(0.0), self.knots.last().copied().unwrap_or(1.0));
        Spline {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|k| lo + hi - k).collect(),
            control: self.control.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
            fit: self.fit.iter().rev().copied().collect(),
            closed: self.closed,
            fit_opts: crate::FitOptions {
                start_tangent: self.fit_opts.end_tangent.map(|t| -t),
                end_tangent: self.fit_opts.start_tangent.map(|t| -t),
                ..self.fit_opts
            },
        }
    }

    /// Bézier decomposition: the homogeneous control points of each polynomial piece
    /// (degree + 1 each) and the parameter breaks between them (pieces + 1 values).
    fn bezier_pieces(&self) -> Option<(Vec<Vec<H>>, Vec<f64>)> {
        let mut s = self.clamped()?;
        let p = s.degree;
        if p > MAX_EDIT_DEGREE {
            return None;
        }
        let (lo, hi) = s.domain();
        let mut interior: Vec<f64> = s.knots.iter().copied().filter(|k| *k > lo + 1e-12 && *k < hi - 1e-12).collect();
        interior.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        if (interior.len() + 1).saturating_mul(p) >= MAX_DECOMPOSED {
            return None;
        }
        for &u in &interior {
            let mut guard = 0;
            while s.knots.iter().filter(|k| (**k - u).abs() < 1e-12).count() < p && guard <= MAX_EDIT_DEGREE {
                s = s.insert_knot(u)?;
                guard += 1;
            }
        }
        let h = s.homogeneous();
        let pieces = interior.len() + 1;
        // A knot of multiplicity above the degree (a break in the curve) leaves extra points.
        if h.len() != pieces * p + 1 {
            return None;
        }
        let segs = (0..pieces).map(|j| h.get(j * p..=j * p + p).map(<[H]>::to_vec)).collect::<Option<Vec<_>>>()?;
        let mut breaks = vec![lo];
        breaks.extend(interior);
        breaks.push(hi);
        Some((segs, breaks))
    }

    /// The same curve at a higher (or equal) degree, clamped.
    pub fn elevated(&self, degree: usize) -> Option<Spline> {
        if degree < self.degree || degree > MAX_EDIT_DEGREE {
            return None;
        }
        if degree == self.degree {
            return self.clamped();
        }
        let (pieces, breaks) = self.bezier_pieces()?;
        let mut out: Vec<H> = Vec::with_capacity(pieces.len() * degree + 1);
        for (j, pc) in pieces.iter().enumerate() {
            let mut q = pc.clone();
            while q.len() < degree + 1 {
                q = elevate_bezier(&q);
            }
            out.extend(q.into_iter().skip(usize::from(j > 0)));
        }
        let mut s = Spline::from_homogeneous(degree, bezier_knots(&breaks, degree), &out, self.is_rational());
        s.closed = self.closed;
        s.is_valid().then_some(s)
    }

    /// `other` joined on at this spline's end (its start should lie on this end; the shared
    /// point becomes their midpoint). Degrees are matched by elevation, weights rescaled so the
    /// junction carries one weight, and `other`'s parameters follow on at a similar speed. The
    /// junction is a knot of multiplicity `degree`, so the curve passes through it exactly.
    pub fn joined(&self, other: &Spline) -> Option<Spline> {
        let deg = self.degree.max(other.degree);
        let a = self.elevated(deg)?;
        let b = other.elevated(deg)?;
        let (ha, hb) = (a.homogeneous(), b.homogeneous());
        let (la, fb) = (*ha.last()?, *hb.first()?);
        let scale = if fb.1.abs() > 1e-300 { la.1 / fb.1 } else { 1.0 };
        if !(scale.is_finite() && scale > 0.0) || ha.len() + hb.len() > MAX_DECOMPOSED * 2 {
            return None;
        }
        let ((alo, ahi), (blo, bhi)) = (a.domain(), b.domain());
        let speed = (ahi - alo) / poly_len(&a.control).max(EPS);
        let span = poly_len(&b.control) * speed;
        let k = if span.is_finite() && span > 1e-12 && bhi - blo > 1e-300 { span / (bhi - blo) } else { 1.0 };
        let mut knots: Vec<f64> = a.knots.get(..a.knots.len().checked_sub(1)?)?.to_vec();
        knots.extend(b.knots.get(deg + 1..)?.iter().map(|u| ahi + (u - blo) * k));
        let mid = hpoint(la).mid(hpoint(fb));
        let mut h = ha;
        if let Some(l) = h.last_mut() {
            *l = (mid * l.1, l.1);
        }
        h.extend(hb.iter().skip(1).map(|x| (x.0 * scale, x.1 * scale)));
        let s = Spline::from_homogeneous(deg, knots, &h, a.is_rational() || b.is_rational());
        (s.is_valid() && s.control.iter().all(|c| c.is_finite())).then_some(s)
    }

    /// A straight line from `a` to `b` as a clamped spline of `degree` (evenly spaced control
    /// points, parameter = distance).
    pub fn from_line(a: Vec2, b: Vec2, degree: usize) -> Spline {
        let p = degree.clamp(1, MAX_EDIT_DEGREE);
        let control = (0..=p).map(|i| a.lerp(b, i as f64 / p as f64)).collect();
        let len = a.dist(b);
        let hi = if len.is_finite() && len > 1e-12 { len } else { 1.0 };
        let mut knots = vec![0.0; p + 1];
        knots.extend(std::iter::repeat_n(hi, p + 1));
        Spline { degree: p, knots, control, ..Spline::default() }
    }

    /// The elliptical arc `center + u·cos θ + v·sin θ` for θ from `start` over `sweep` (negative
    /// runs clockwise) as an exact rational quadratic spline, in pieces of at most a quarter
    /// turn. Circular arcs are the case `|u| = |v|`, `u ⟂ v`.
    pub fn from_conic_arc(center: Vec2, u: Vec2, v: Vec2, start: f64, sweep: f64) -> Option<Spline> {
        if !(start.is_finite() && sweep.is_finite() && center.is_finite() && u.is_finite() && v.is_finite())
            || sweep.abs() < 1e-12
            || sweep.abs() > TAU + 1e-9
            || u.cross(v).abs() < 1e-300
        {
            return None;
        }
        let k = ((sweep.abs() / FRAC_PI_2) - 1e-9).ceil().clamp(1.0, 4.0) as usize;
        let d = sweep / k as f64;
        let at = |t: f64| center + u * t.cos() + v * t.sin();
        let w = (d / 2.0).cos();
        let mut control = vec![at(start)];
        let mut weights = vec![1.0];
        for i in 0..k {
            let a0 = start + d * i as f64;
            let m = a0 + d / 2.0;
            control.push(center + (u * m.cos() + v * m.sin()) / w);
            weights.push(w);
            control.push(at(a0 + d));
            weights.push(1.0);
        }
        let step = (d.abs() * (u.len() + v.len()) / 2.0).max(1e-9);
        let breaks: Vec<f64> = (0..=k).map(|i| step * i as f64).collect();
        let s = Spline { degree: 2, knots: bezier_knots(&breaks, 2), control, weights, ..Spline::default() };
        s.is_valid().then_some(s)
    }

    /// An ellipse or elliptical arc as an exact rational spline (counter-clockwise).
    pub fn from_ellipse(e: &Ellipse) -> Option<Spline> {
        let s = Spline::from_conic_arc(e.center, e.major, e.minor(), e.start, e.sweep())?;
        Some(Spline { closed: e.is_full(), ..s })
    }

    /// Run the spline on in a straight line along its end tangent by `dist` > 0: past the end
    /// (`at_end`) or before the start. The result is clamped, open and has no fit data.
    pub fn extended(&self, at_end: bool, dist: f64) -> Option<Spline> {
        if !(dist.is_finite() && dist > 0.0) || self.closed {
            return None;
        }
        let s = self.clamped()?;
        let (lo, hi) = s.domain();
        let t = if at_end { hi } else { lo };
        let e = s.eval(t);
        let dir = s.unit_tangent(t) * if at_end { 1.0 } else { -1.0 };
        if dir == Vec2::ZERO {
            return None;
        }
        let far = e + dir * dist;
        if at_end { s.joined(&Spline::from_line(e, far, s.degree)) } else { Spline::from_line(far, e, s.degree).joined(&s) }
    }

    /// Integration intervals for arc length between `a < b`: each knot span cut in four.
    fn length_intervals(&self, a: f64, b: f64) -> Vec<(f64, f64)> {
        let mut cuts = vec![a];
        cuts.extend(self.knots.iter().copied().filter(|k| *k > a && *k < b));
        cuts.push(b);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-15);
        let mut out = Vec::with_capacity(cuts.len() * 4);
        for w in cuts.windows(2) {
            let (Some(&x0), Some(&x1)) = (w.first(), w.get(1)) else { continue };
            for i in 0..4 {
                out.push((x0 + (x1 - x0) * i as f64 / 4.0, x0 + (x1 - x0) * (i + 1) as f64 / 4.0));
            }
        }
        out
    }

    /// Arc length of one integration panel.
    fn panel_length(&self, a: f64, b: f64) -> f64 {
        let speed = |t: f64| self.derivative(t).len();
        adaptive_gauss(a, b, gauss(a, b, speed), &speed, MAX_GAUSS_DEPTH)
    }

    /// Arc length between parameters `a` and `b` (adaptive Gauss–Legendre over each knot span).
    pub fn length_between(&self, a: f64, b: f64) -> f64 {
        if !self.is_valid() || !(a.is_finite() && b.is_finite()) {
            return 0.0;
        }
        let (lo, hi) = self.domain();
        let (a, b) = (a.min(b).clamp(lo, hi), a.max(b).clamp(lo, hi));
        let l: f64 = self.length_intervals(a, b).into_iter().map(|(x, y)| self.panel_length(x, y)).sum();
        if l.is_finite() { l } else { 0.0 }
    }

    /// Arc length of the whole spline.
    pub fn length(&self) -> f64 {
        let (lo, hi) = self.domain();
        self.length_between(lo, hi)
    }

    /// The parameter `len` along the curve from parameter `from` (forward for positive `len`,
    /// backward for negative), clamped to the domain.
    pub fn param_at_length(&self, from: f64, len: f64) -> f64 {
        let (lo, hi) = self.domain();
        if !self.is_valid() {
            // Checked before `clamp`: an invalid spline's bounds may be reversed or NaN.
            return if from.is_finite() { from } else { lo };
        }
        let from = if from.is_finite() { from.clamp(lo, hi) } else { lo };
        if !len.is_finite() || len == 0.0 {
            return from;
        }
        let (a, b, target) = if len > 0.0 { (from, hi, len) } else { (lo, from, -len) };
        let mut ivs = self.length_intervals(a, b);
        if len < 0.0 {
            ivs.reverse();
        }
        let mut acc = 0.0;
        for (x0, x1) in ivs {
            let l = self.panel_length(x0, x1);
            if acc + l >= target && l > 0.0 {
                // Bisect inside this interval, measuring from its near end.
                let rest = target - acc;
                let (near, far) = if len > 0.0 { (x0, x1) } else { (x1, x0) };
                let (mut s0, mut s1) = (near, far);
                for _ in 0..60 {
                    let m = (s0 + s1) / 2.0;
                    let lm = self.panel_length(near.min(m), near.max(m));
                    if lm < rest {
                        s0 = m;
                    } else {
                        s1 = m;
                    }
                }
                return (s0 + s1) / 2.0;
            }
            acc += l;
        }
        if len > 0.0 { hi } else { lo }
    }
}

impl Ellipse {
    /// dP/dθ at parameter `t`.
    pub fn derivative(&self, t: f64) -> Vec2 {
        self.minor() * t.cos() - self.major * t.sin()
    }

    /// Arc length from parameter `t0` counter-clockwise over `sweep` (radians, >= 0).
    pub fn arc_length_from(&self, t0: f64, sweep: f64) -> f64 {
        if !(t0.is_finite() && sweep.is_finite()) || sweep <= 0.0 {
            return 0.0;
        }
        let n = ((sweep / TAU) * 128.0).ceil().clamp(1.0, 512.0) as usize;
        let h = sweep / n as f64;
        let l: f64 = (0..n).map(|i| gauss(t0 + h * i as f64, t0 + h * (i + 1) as f64, |t| self.derivative(t).len())).sum();
        if l.is_finite() { l } else { 0.0 }
    }

    /// Arc length of the ellipse or elliptical arc.
    pub fn length(&self) -> f64 {
        self.arc_length_from(self.start, self.sweep())
    }

    /// The parameter reached `len` along the ellipse from `t0`: counter-clockwise for positive
    /// `len`, clockwise for negative. `None` for a whole turn or more, or degenerate input.
    pub fn param_at_length(&self, t0: f64, len: f64) -> Option<f64> {
        let full = self.arc_length_from(0.0, TAU);
        if !(t0.is_finite() && len.is_finite() && full > EPS) || len.abs() >= full {
            return None;
        }
        let target = len.abs();
        let along = |s: f64| if len >= 0.0 { self.arc_length_from(t0, s) } else { self.arc_length_from(t0 - s, s) };
        let (mut a, mut b) = (0.0, TAU);
        for _ in 0..80 {
            let m = (a + b) / 2.0;
            if along(m) < target {
                a = m;
            } else {
                b = m;
            }
        }
        let s = (a + b) / 2.0;
        Some(if len >= 0.0 { t0 + s } else { t0 - s })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cubic() -> Spline {
        Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 3.0), Vec2::new(6.0, -3.0), Vec2::new(9.0, 0.0), Vec2::new(12.0, 2.0)], 3)
    }

    #[test]
    fn derivative_matches_finite_differences() {
        let mut s = cubic();
        s.weights = vec![1.0, 2.0, 0.5, 1.0, 1.5];
        let (lo, hi) = s.domain();
        for i in 1..20 {
            let t = lo + (hi - lo) * i as f64 / 20.0;
            let h = 1e-6;
            let fd = (s.eval(t + h) - s.eval(t - h)) / (h * 2.0);
            assert!(s.derivative(t).near(fd, 1e-4), "{t}: {:?} vs {fd:?}", s.derivative(t));
        }
        // Clamped end: the derivative points along the last control leg.
        assert!(s.unit_tangent(hi).near((Vec2::new(12.0, 2.0) - Vec2::new(9.0, 0.0)).normalized(), 1e-9));
    }

    #[test]
    fn split_and_sub_keep_the_curve() {
        let s = cubic();
        let (l, r) = s.split_at(0.4).unwrap();
        assert!(l.eval(l.domain().1).near(s.eval(0.4), 1e-12));
        assert!(r.eval(r.domain().0).near(s.eval(0.4), 1e-12));
        assert!(r.eval(0.7).near(s.eval(0.7), 1e-12));
        let m = s.sub_between(0.2, 0.6).unwrap();
        assert!(m.eval(m.domain().0).near(s.eval(0.2), 1e-12) && m.eval(m.domain().1).near(s.eval(0.6), 1e-12));
        assert!(s.split_at(0.0).is_none() && s.split_at(f64::NAN).is_none());
    }

    #[test]
    fn periodic_spline_clamps_to_the_same_curve() {
        let fit = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 1.0), Vec2::new(5.0, 4.0), Vec2::new(1.0, 5.0)];
        let p = Spline::from_fit(&fit, true);
        assert!(p.is_periodic());
        let c = p.clamped().unwrap();
        assert!(!c.is_periodic());
        let ((plo, phi), (clo, chi)) = (p.domain(), c.domain());
        for i in 0..=20 {
            let f = i as f64 / 20.0;
            assert!(p.eval(plo + (phi - plo) * f).near(c.eval(clo + (chi - clo) * f), 1e-9));
        }
        assert!(c.control.first().unwrap().near(p.eval(plo), 1e-9));
    }

    #[test]
    fn elevation_keeps_the_curve() {
        let s = cubic();
        let e = s.elevated(5).unwrap();
        assert_eq!(e.degree, 5);
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert!(e.eval(t).near(s.eval(t), 1e-9), "{t}");
        }
        // A rational quarter circle elevated to a cubic stays on the circle.
        let q = Spline::from_conic_arc(Vec2::ZERO, Vec2::X * 2.0, Vec2::Y * 2.0, 0.0, 3.0).unwrap();
        let q3 = q.elevated(3).unwrap();
        let (lo, hi) = q3.domain();
        for i in 0..=20 {
            assert!((q3.eval(lo + (hi - lo) * i as f64 / 20.0).len() - 2.0).abs() < 1e-9);
        }
        assert!(s.elevated(2).is_none() && s.elevated(1000).is_none());
    }

    #[test]
    fn conic_arcs_are_exact() {
        let e = Ellipse { center: Vec2::new(1.0, 2.0), major: Vec2::new(4.0, 3.0), ratio: 0.5, start: 0.3, end: 5.5 };
        let s = Spline::from_ellipse(&e).unwrap();
        let (lo, hi) = s.domain();
        assert!(s.eval(lo).near(e.at_param(e.start), 1e-9) && s.eval(hi).near(e.at_param(e.end), 1e-9));
        // Every point satisfies the ellipse equation.
        let (a, b) = (e.major.len(), e.major.len() * e.ratio);
        let ux = e.major.normalized();
        for i in 0..=40 {
            let p = s.eval(lo + (hi - lo) * i as f64 / 40.0) - e.center;
            let (x, y) = (p.dot(ux), p.dot(ux.perp()));
            assert!(((x / a).powi(2) + (y / b).powi(2) - 1.0).abs() < 1e-9);
        }
        // Clockwise arcs run the other way.
        let cw = Spline::from_conic_arc(Vec2::ZERO, Vec2::X, Vec2::Y, 0.0, -1.0).unwrap();
        assert!(cw.eval(cw.domain().1).near(Vec2::new(1f64.cos(), -(1f64.sin())), 1e-12));
        assert!(Spline::from_conic_arc(Vec2::ZERO, Vec2::X, Vec2::X, 0.0, 1.0).is_none());
        assert!(Spline::from_conic_arc(Vec2::ZERO, Vec2::X, Vec2::Y, 0.0, f64::NAN).is_none());
    }

    #[test]
    fn join_line_arc_and_spline() {
        let line = Spline::from_line(Vec2::new(-5.0, 0.0), Vec2::ZERO, 1);
        let s = cubic();
        let j = line.joined(&s).unwrap();
        assert_eq!(j.degree, 3);
        let (lo, hi) = j.domain();
        assert!(j.eval(lo).near(Vec2::new(-5.0, 0.0), 1e-12) && j.eval(hi).near(Vec2::new(12.0, 2.0), 1e-12));
        // The line part is straight (parameter = distance), the spline part unchanged.
        for i in 0..=10 {
            assert!(j.eval(lo + i as f64 * 0.5).near(Vec2::new(-5.0 + i as f64 * 0.5, 0.0), 1e-12));
            let f = i as f64 / 10.0;
            assert!(j.eval(5.0 + (hi - 5.0) * f).near(s.eval(f), 1e-9));
        }
        assert!((j.length() - (5.0 + s.length())).abs() < 1e-6);
        // A rational arc joined to a non-rational line.
        let arc = Spline::from_conic_arc(Vec2::new(12.0, 5.0), Vec2::new(0.0, -3.0), Vec2::new(3.0, 0.0), 0.0, 1.0).unwrap();
        let k = j.joined(&arc).unwrap();
        assert!(k.is_rational());
        assert!(k.eval(k.domain().1).near(arc.eval(arc.domain().1), 1e-9));
    }

    #[test]
    fn extension_runs_on_along_the_tangent() {
        let s = cubic();
        let (lo, hi) = s.domain();
        let t_end = s.unit_tangent(hi);
        let e = s.extended(true, 3.0).unwrap();
        assert!(e.eval(e.domain().1).near(s.eval(hi) + t_end * 3.0, 1e-9));
        assert!((e.length() - s.length() - 3.0).abs() < 1e-6);
        let b = s.extended(false, 2.0).unwrap();
        assert!(b.eval(b.domain().0).near(s.eval(lo) - s.unit_tangent(lo) * 2.0, 1e-9));
        assert!(s.extended(true, -1.0).is_none() && s.extended(true, f64::NAN).is_none());
    }

    #[test]
    fn spline_arc_length_and_inverse() {
        let l = Spline::from_line(Vec2::ZERO, Vec2::new(3.0, 4.0), 3);
        assert!((l.length() - 5.0).abs() < 1e-12);
        let q = Spline::from_conic_arc(Vec2::ZERO, Vec2::X * 2.0, Vec2::Y * 2.0, 0.0, std::f64::consts::PI).unwrap();
        assert!((q.length() - 2.0 * std::f64::consts::PI).abs() < 1e-6);
        let (lo, hi) = q.domain();
        let t = q.param_at_length(lo, std::f64::consts::PI);
        assert!(q.eval(t).near(Vec2::new(0.0, 2.0), 1e-6));
        let t = q.param_at_length(hi, -(std::f64::consts::PI / 2.0));
        assert!(q.eval(t).near(Vec2::new(-2f64.sqrt(), 2f64.sqrt()), 1e-6));
        assert_eq!(q.param_at_length(lo, 1e9), hi);
    }

    #[test]
    fn spline_arc_length_on_a_tight_bend() {
        // One Bezier span that turns sharply: four fixed Gauss panels come out 0.3% short, and
        // the inverse disagrees with the measure (#705). Reference by composite Simpson.
        let pts = [(2.164, -9.484), (8.964, -13.012), (-16.428, -1.276), (18.58, -18.08)];
        let s = Spline::from_control(pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect(), 3);
        let (lo, hi) = s.domain();
        assert!((s.length() - 26.708_124_545_291_4).abs() < 1e-6, "{}", s.length());
        for frac in [0.25, 0.5, 0.9] {
            let want = s.length() * frac;
            let t = s.param_at_length(lo, want);
            assert!((s.length_between(lo, t) - want).abs() < 1e-6, "{frac}: {}", s.length_between(lo, t));
            let t = s.param_at_length(hi, -want);
            assert!((s.length_between(t, hi) - want).abs() < 1e-6, "{frac}: {}", s.length_between(t, hi));
        }
    }

    #[test]
    fn param_at_length_on_invalid_domain_does_not_panic() {
        // Reversed bounds (knots out of order) and a NaN bound both make the spline invalid; a
        // finite start must come back unchanged rather than reach `clamp` with bad bounds.
        let mut reversed = cubic();
        reversed.knots.reverse();
        assert!(!reversed.is_valid() && reversed.domain().0 > reversed.domain().1);
        let mut nan = cubic();
        nan.knots[nan.degree] = f64::NAN;
        assert!(!nan.is_valid() && nan.domain().0.is_nan());
        for s in [reversed, nan] {
            assert_eq!(s.param_at_length(0.5, 1.0), 0.5);
            assert_eq!(s.param_at_length(0.5, -1.0), 0.5);
        }
    }

    #[test]
    fn ellipse_arc_length() {
        let c = Ellipse::full(Vec2::ZERO, Vec2::new(3.0, 0.0), 1.0);
        assert!((c.length() - TAU * 3.0).abs() < 1e-9);
        let e = Ellipse::full(Vec2::ZERO, Vec2::new(5.0, 0.0), 0.6);
        // Ramanujan's approximation is good to ~1e-5 relative here.
        let (a, b) = (5.0f64, 3.0f64);
        let h = ((a - b) / (a + b)).powi(2);
        let ram = std::f64::consts::PI * (a + b) * (1.0 + h * 3.0 / (10.0 + (4.0 - h * 3.0).sqrt()));
        assert!((e.length() - ram).abs() < 1e-4);
        let t = e.param_at_length(0.0, e.length() / 4.0).unwrap();
        assert!((t - FRAC_PI_2).abs() < 1e-9);
        let t = e.param_at_length(0.0, -e.length() / 4.0).unwrap();
        assert!((t + FRAC_PI_2).abs() < 1e-9);
        assert!(e.param_at_length(0.0, e.length() * 2.0).is_none());
        assert!(e.param_at_length(f64::NAN, 1.0).is_none());
    }
}

// ---------------------------------------------------------------------------
// 3D non-rational NURBS curve (KER-SRF-001).
//
// Distinct from the 2D `Spline` above: this type carries `Vec3` control points
// and no weights. Weights are deliberately deferred to a later task; see
// `NurbsCurve::is_rational`.
// ---------------------------------------------------------------------------

/// A (so far non-rational) B-spline curve in three dimensions.
///
/// The struct fields are public for construction by the kernel, but callers are
/// expected to go through [`NurbsCurve::new`] so the invariants hold. A
/// literal with broken invariants makes the evaluation routines return zeroes
/// rather than panicking.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub control: Vec<crate::Vec3>,
}

/// Why a [`NurbsCurve`] could not be built or evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NurbsError {
    /// `degree < 1`, or too few control points for the requested degree.
    BadDegree,
    /// `knots.len() != control.len() + degree + 1`.
    KnotCountMismatch,
    /// A knot is greater than one that follows it.
    NonMonotoneKnots,
    /// The domain `[knots[degree], knots[len-degree-1]]` is empty.
    EmptyDomain,
    /// A knot or control coordinate is not finite.
    NonFiniteInput,
    /// A parameter was requested outside the curve's domain.
    OutOfDomain,
}

impl std::fmt::Display for NurbsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            NurbsError::BadDegree => "bad NURBS degree or control-point count",
            NurbsError::KnotCountMismatch => "NURBS knot count does not match the control points",
            NurbsError::NonMonotoneKnots => "NURBS knots are not non-decreasing",
            NurbsError::EmptyDomain => "NURBS domain is empty",
            NurbsError::NonFiniteInput => "NURBS input is not finite",
            NurbsError::OutOfDomain => "NURBS parameter is outside the domain",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for NurbsError {}

impl NurbsCurve {
    /// Build a curve, validating all invariants.
    pub fn new(degree: usize, knots: Vec<f64>, control: Vec<crate::Vec3>) -> Result<Self, NurbsError> {
        if degree < 1 {
            return Err(NurbsError::BadDegree);
        }
        if !knots.iter().all(|x| x.is_finite()) || !control.iter().all(|c| c.is_finite()) {
            return Err(NurbsError::NonFiniteInput);
        }
        let n = control.len();
        if n <= degree {
            return Err(NurbsError::BadDegree);
        }
        if knots.len() != n + degree + 1 {
            return Err(NurbsError::KnotCountMismatch);
        }
        if knots.windows(2).any(|w| w[0] > w[1]) {
            return Err(NurbsError::NonMonotoneKnots);
        }
        let lo = knots.get(degree).copied().unwrap_or(f64::NAN);
        let hi = knots.get(n).copied().unwrap_or(f64::NAN);
        if !(hi > lo) {
            return Err(NurbsError::EmptyDomain);
        }
        Ok(Self { degree, knots, control })
    }

    fn is_valid(&self) -> bool {
        if self.degree < 1 {
            return false;
        }
        let n = self.control.len();
        if n <= self.degree {
            return false;
        }
        if self.knots.len() != n + self.degree + 1 {
            return false;
        }
        if !self.knots.iter().all(|x| x.is_finite()) {
            return false;
        }
        if !self.control.iter().all(|c| c.is_finite()) {
            return false;
        }
        if self.knots.windows(2).any(|w| w[0] > w[1]) {
            return false;
        }
        let (lo, hi) = self.domain();
        hi > lo
    }

    /// The curve is non-rational (no weights) for now; weights come in a later task.
    pub fn is_rational(&self) -> bool {
        false
    }

    pub fn domain(&self) -> (f64, f64) {
        let lo = self.knots.get(self.degree).copied().unwrap_or(0.0);
        let hi = self.knots.get(self.control.len()).copied().unwrap_or(0.0);
        (lo, hi)
    }

    fn eval_raw(&self, u: f64) -> crate::Vec3 {
        if !self.is_valid() {
            return crate::Vec3::ZERO;
        }
        let n = self.control.len() - 1;
        let (lo, hi) = self.domain();
        let u = if u.is_finite() { u.clamp(lo, hi) } else { lo };
        let span = crate::spline::find_span(n, self.degree, u, &self.knots);
        let b = crate::spline::basis(span, u, self.degree, &self.knots);
        let mut p = crate::Vec3::ZERO;
        for (k, nb) in b.iter().enumerate() {
            let idx = span + k;
            if idx >= self.degree {
                let i = idx - self.degree;
                if let Some(c) = self.control.get(i) {
                    p = p + *c * *nb;
                }
            }
        }
        if !p.is_finite() { crate::Vec3::ZERO } else { p }
    }

    /// Evaluate at `u`; out-of-domain parameters are clamped to the domain.
    pub fn eval(&self, u: f64) -> crate::Vec3 {
        self.eval_raw(u)
    }

    /// Evaluate at `u`, refusing an out-of-domain or non-finite parameter.
    pub fn try_eval(&self, u: f64) -> Result<crate::Vec3, NurbsError> {
        if !self.is_valid() {
            return Err(NurbsError::NonFiniteInput);
        }
        let (lo, hi) = self.domain();
        if !u.is_finite() || u < lo || u > hi {
            return Err(NurbsError::OutOfDomain);
        }
        Ok(self.eval_raw(u))
    }

    pub fn eval_checked(&self, u: f64) -> Result<crate::Vec3, NurbsError> {
        self.try_eval(u)
    }

    pub fn eval_at(&self, u: f64) -> Result<crate::Vec3, NurbsError> {
        self.try_eval(u)
    }

    pub fn evaluate(&self, u: f64) -> Result<crate::Vec3, NurbsError> {
        self.try_eval(u)
    }

    pub fn point_at(&self, u: f64) -> Result<crate::Vec3, NurbsError> {
        self.try_eval(u)
    }

    /// Derivative of the curve at `u`, of the given `order`: `0` is the point
    /// itself, `1` the first derivative (tangent), `2` the second, and so on;
    /// orders above the degree are zero. Computed exactly by differentiating the
    /// control points `order` times — each step scales the differences by
    /// `degree / knot_span` and trims one knot off each end of the knot vector —
    /// then evaluating the resulting lower-degree B-spline. The curve is
    /// non-rational (weights are deferred), so no homogeneous correction applies.
    /// Out-of-domain parameters are clamped; a non-finite `u` evaluates at `lo`.
    pub fn derivative(&self, u: f64, order: usize) -> crate::Vec3 {
        if !self.is_valid() {
            return crate::Vec3::ZERO;
        }
        let (lo, hi) = self.domain();
        let u = if u.is_finite() { u.clamp(lo, hi) } else { lo };
        if order == 0 {
            return self.eval_raw(u);
        }
        if order > self.degree {
            return crate::Vec3::ZERO;
        }
        // Each differentiation step lowers the degree by one, drops one control
        // point and trims one knot off each end of the working knot vector, so
        // after `s` steps the working vector is `knots[s .. len - s]`. Both the
        // difference denominators and the final evaluation must read that
        // trimmed vector: evaluating against the original knots shifts the span
        // search by one and silently returns the wrong derivative.
        let mut q: Vec<crate::Vec3> = self.control.clone();
        let mut knots = self.knots.clone();
        let mut deg = self.degree;
        for _ in 0..order {
            if q.len() < 2 {
                return crate::Vec3::ZERO;
            }
            let n = q.len();
            let mut next = Vec::with_capacity(n - 1);
            for i in 0..n - 1 {
                let a = knots.get(i + deg + 1).copied().unwrap_or(0.0);
                let b = knots.get(i + 1).copied().unwrap_or(0.0);
                let denom = a - b;
                let scale = if denom.abs() > 1e-14 { deg as f64 / denom } else { 0.0 };
                let diff = q.get(i + 1).copied().unwrap_or(crate::Vec3::ZERO) - q.get(i).copied().unwrap_or(crate::Vec3::ZERO);
                next.push(diff * scale);
            }
            q = next;
            deg = deg.saturating_sub(1);
            // Unreachable-empty for a validated curve with `order <= degree`
            // (at least two knots always remain); the guard only keeps a future
            // refactor from panicking.
            if knots.len() >= 2 {
                knots.remove(0);
                knots.pop();
            }
        }
        let n = q.len().saturating_sub(1);
        let mut span = crate::spline::find_span(n, deg, u, &knots);
        if span > n {
            span = n;
        }
        let b = crate::spline::basis(span, u, deg, &knots);
        let mut p = crate::Vec3::ZERO;
        for (k, nb) in b.iter().enumerate() {
            let idx = span + k;
            if idx >= deg {
                let i = idx - deg;
                if let Some(c) = q.get(i) {
                    p = p + *c * *nb;
                }
            }
        }
        if p.is_finite() { p } else { crate::Vec3::ZERO }
    }

    /// Closest point: `(parameter, point)`.
    pub fn closest_point(&self, p: crate::Vec3) -> (f64, crate::Vec3) {
        if !self.is_valid() {
            let (lo, _) = self.domain();
            return (lo, crate::Vec3::ZERO);
        }
        if !p.is_finite() {
            let (lo, _) = self.domain();
            return (lo, self.eval_raw(lo));
        }
        let (lo, hi) = self.domain();
        const N: usize = 96;
        let mut best_u = lo;
        let mut best_d2 = (self.eval_raw(lo) - p).len() * (self.eval_raw(lo) - p).len();
        for i in 0..=N {
            let t = i as f64 / N as f64;
            let u = lo + (hi - lo) * t;
            let q = self.eval_raw(u);
            let d = q - p;
            let d2 = d.len() * d.len();
            if d2 < best_d2 {
                best_d2 = d2;
                best_u = u;
            }
        }
        let mut u = best_u;
        for _ in 0..80 {
            let q = self.eval_raw(u);
            let qp = self.derivative(u, 1);
            let qpp = self.derivative(u, 2);
            let r = q - p;
            let f = r.dot(qp);
            let fp = qp.dot(qp) + r.dot(qpp);
            if !fp.is_finite() || fp.abs() < 1e-18 {
                break;
            }
            let delta = f / fp;
            let new_u = (u - delta).clamp(lo, hi);
            if (new_u - u).abs() <= 1e-14 {
                u = new_u;
                break;
            }
            u = new_u;
        }
        (u, self.eval_raw(u))
    }

    /// Arc length over the whole domain, by adaptive Gauss-Legendre quadrature.
    pub fn length(&self, tolerance: f64) -> f64 {
        if !self.is_valid() {
            return 0.0;
        }
        let tol = if tolerance.is_finite() && tolerance > 0.0 { tolerance } else { 1e-9 };
        let (lo, hi) = self.domain();
        if !(hi > lo) {
            return 0.0;
        }
        let speed = |u: f64| self.derivative(u, 1).len();
        let whole = gauss(lo, hi, &speed);
        let est = adaptive_gauss_tol(lo, hi, whole, &speed, MAX_GAUSS_DEPTH, tol);
        if est.is_finite() && est >= 0.0 { est } else { 0.0 }
    }
}

#[cfg(test)]
mod nurbs3d_tests {
    use super::*;

    #[test]
    fn bezier_arc_degree2_matches_closed_form() {
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let c0 = crate::Vec3::new(0.0, 0.0, 0.0);
        let c1 = crate::Vec3::new(0.5, 1.0, 0.2);
        let c2 = crate::Vec3::new(1.0, 0.0, 0.4);
        let c = NurbsCurve::new(2, knots, vec![c0, c1, c2]).unwrap();
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            let o = 1.0 - t;
            let expected = c0 * (o * o) + c1 * (o * 2.0 * t) + c2 * (t * t);
            let got = c.eval(t);
            assert!((got - expected).len() < 1e-12);
        }
        assert!(!c.is_rational());
    }

    #[test]
    fn line_degree1_endpoints_and_midpoint_exact() {
        let a = crate::Vec3::new(0.0, 0.0, 0.0);
        let b = crate::Vec3::new(2.0, 4.0, 6.0);
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let c = NurbsCurve::new(1, knots, vec![a, b]).unwrap();
        assert_eq!(c.eval(0.0), a);
        assert_eq!(c.eval(1.0), b);
        let mid = c.eval(0.5);
        assert!((mid - crate::Vec3::new(1.0, 2.0, 3.0)).len() < 1e-15);
        assert_eq!(c.domain(), (0.0, 1.0));
    }

    #[test]
    fn quarter_circle_approximation_stays_close() {
        // A degree-2 B-spline with five control points on the quarter circle.
        let pts = [
            (1.0_f64, 0.0_f64),
            (0.8660254037844386_f64, 0.5_f64),
            (0.7071067811865476_f64, 0.7071067811865476_f64),
            (0.5_f64, 0.8660254037844386_f64),
            (0.0_f64, 1.0_f64),
        ];
        let degree = 2usize;
        let n = pts.len();
        // Clamped uniform-ish knot vector for an open curve.
        let mut knots = Vec::with_capacity(n + degree + 1);
        for _ in 0..=degree {
            knots.push(0.0);
        }
        for i in 1..=n - degree - 1 {
            knots.push(i as f64 / (n - degree) as f64);
        }
        for _ in 0..=degree {
            knots.push(1.0);
        }
        // Pad if needed to exact length.
        while knots.len() < n + degree + 1 {
            knots.push(1.0);
        }
        let ctrl: Vec<crate::Vec3> = pts.iter().map(|&(x, y)| crate::Vec3::new(x, y, 0.0)).collect();
        let c = NurbsCurve::new(degree, knots, ctrl).unwrap();
        let (lo, hi) = c.domain();
        for i in 0..=20 {
            let u = lo + (hi - lo) * i as f64 / 20.0;
            let p = c.eval(u);
            let r = (p.x * p.x + p.y * p.y).sqrt();
            assert!((r - 1.0).abs() <= 0.2, "radius {} at u {}", r, u);
        }
    }

    #[test]
    fn derivative_matches_finite_difference() {
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0];
        let ctrl: Vec<crate::Vec3> = (0..6).map(|i| crate::Vec3::new(i as f64, (i as f64).sin(), (i as f64).cos())).collect();
        let c = NurbsCurve::new(3, knots, ctrl).unwrap();
        let (lo, hi) = c.domain();
        let h = 1e-5;
        for i in 1..=5 {
            let u = lo + (hi - lo) * i as f64 / 6.0;
            let ana = c.derivative(u, 1);
            let um = (u - h).clamp(lo, hi);
            let up = (u + h).clamp(lo, hi);
            let num = (c.eval(up) - c.eval(um)) * (1.0 / (h * 2.0));
            // componentwise compare
            assert!((ana.x - num.x).abs() < 1e-6, "x {} vs {}", ana.x, num.x);
            assert!((ana.y - num.y).abs() < 1e-6, "y {} vs {}", ana.y, num.y);
            assert!((ana.z - num.z).abs() < 1e-6, "z {} vs {}", ana.z, num.z);
        }
    }

    /// de Casteljau evaluation of a Bezier segment with the given control points.
    fn bezier(pts: &[crate::Vec3], t: f64) -> crate::Vec3 {
        let mut d = pts.to_vec();
        let mut n = d.len();
        while n > 1 {
            for i in 0..n - 1 {
                d[i] = d[i] + (d[i + 1] - d[i]) * t;
            }
            n -= 1;
        }
        d[0]
    }

    #[test]
    fn derivative_matches_closed_form_bezier() {
        // A single Bezier segment (fully clamped knots, no interior knots): the
        // derivative of degree p is the degree p-1 Bezier of the scaled
        // control-point differences — exact, no finite differences needed.
        let p0 = crate::Vec3::new(1.0, -2.0, 0.5);
        let p1 = crate::Vec3::new(4.0, 0.5, -1.0);
        let p2 = crate::Vec3::new(-2.0, 3.0, 2.0);
        let p3 = crate::Vec3::new(5.0, 1.0, -3.0);

        // Degree 1: the derivative is the constant P1 - P0.
        let c = NurbsCurve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![p0, p1]).unwrap();
        for i in 0..=10 {
            let u = i as f64 / 10.0;
            assert!(c.derivative(u, 0).near(c.eval(u), 1e-12), "deg1 d0 at {}", u);
            assert!((c.derivative(u, 1) - (p1 - p0)).len() < 1e-12, "deg1 d1 at {}", u);
            assert!(c.derivative(u, 2).near(crate::Vec3::ZERO, 1e-12), "deg1 d2 at {}", u);
        }

        // Degree 2: the first derivative runs along 2(P1-P0) -> 2(P2-P1); the
        // second is the constant 2(P2 - 2P1 + P0).
        let c = NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![p0, p1, p2]).unwrap();
        for i in 0..=10 {
            let u = i as f64 / 10.0;
            let d1 = (p1 - p0) * (2.0 * (1.0 - u)) + (p2 - p1) * (u * 2.0);
            assert!((c.derivative(u, 1) - d1).len() < 1e-12, "deg2 d1 at {}", u);
            let d2 = (p2 - p1 * 2.0 + p0) * 2.0;
            assert!((c.derivative(u, 2) - d2).len() < 1e-12, "deg2 d2 at {}", u);
            assert!(c.derivative(u, 3).near(crate::Vec3::ZERO, 1e-12), "deg2 d3 at {}", u);
        }

        // Degree 3: the first derivative is the quadratic Bezier of
        // 3(P_{i+1} - P_i), the second the line of 6(P_{i+2} - 2P_{i+1} + P_i),
        // the third the constant 6(P3 - 3P2 + 3P1 - P0).
        let c = NurbsCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], vec![p0, p1, p2, p3]).unwrap();
        let q0 = p1 - p0;
        let q1 = p2 - p1;
        let q2 = p3 - p2;
        for i in 0..=10 {
            let u = i as f64 / 10.0;
            let d1 = bezier(&[q0 * 3.0, q1 * 3.0, q2 * 3.0], u);
            assert!((c.derivative(u, 1) - d1).len() < 1e-12, "deg3 d1 at {}", u);
            let d2 = (q1 - q0) * (6.0 * (1.0 - u)) + (q2 - q1) * (u * 6.0);
            assert!((c.derivative(u, 2) - d2).len() < 1e-12, "deg3 d2 at {}", u);
            let d3 = (q2 - q1 * 2.0 + q0) * 6.0;
            assert!((c.derivative(u, 3) - d3).len() < 1e-12, "deg3 d3 at {}", u);
            assert!(c.derivative(u, 4).near(crate::Vec3::ZERO, 1e-12), "deg3 d4 at {}", u);
        }
    }

    #[test]
    fn derivative_with_repeated_knots_matches_closed_form() {
        // Degree 2 with a double interior knot at u = 1: two quadratic Bezier
        // segments meeting C^0 at P2, so the derivative jumps at the break.
        let p0 = crate::Vec3::new(0.0, 0.0, 0.0);
        let p1 = crate::Vec3::new(2.0, 1.0, -1.0);
        let p2 = crate::Vec3::new(1.0, 4.0, 0.5);
        let p3 = crate::Vec3::new(5.0, 3.0, 2.0);
        let p4 = crate::Vec3::new(6.0, -2.0, 1.0);
        let c = NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0], vec![p0, p1, p2, p3, p4]).unwrap();
        for i in 0..=48 {
            let u = i as f64 / 24.0;
            if (u - 1.0).abs() < 1e-15 {
                continue; // the break: asserted separately below
            }
            let (d1, d2) = if u < 1.0 {
                ((p1 - p0) * (2.0 * (1.0 - u)) + (p2 - p1) * (u * 2.0), (p2 - p1 * 2.0 + p0) * 2.0)
            } else {
                let s = u - 1.0;
                ((p3 - p2) * (2.0 * (1.0 - s)) + (p4 - p3) * (s * 2.0), (p4 - p3 * 2.0 + p2) * 2.0)
            };
            assert!((c.derivative(u, 1) - d1).len() < 1e-12, "d1 at {}", u);
            assert!((c.derivative(u, 2) - d2).len() < 1e-12, "d2 at {}", u);
        }
        // At the break the half-open span convention returns the right derivative.
        assert!((c.derivative(1.0, 1) - (p3 - p2) * 2.0).len() < 1e-12, "break d1");
        assert!((c.derivative(1.0, 2) - (p4 - p3 * 2.0 + p2) * 2.0).len() < 1e-12, "break d2");
        assert!(c.derivative(0.5, 3).near(crate::Vec3::ZERO, 1e-12), "d3");
    }

    #[test]
    fn closest_point_is_local_minimum() {
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let c0 = crate::Vec3::new(0.0, 0.0, 0.0);
        let c1 = crate::Vec3::new(2.0, 3.0, 0.0);
        let c2 = crate::Vec3::new(4.0, 0.0, 0.0);
        let c = NurbsCurve::new(2, knots, vec![c0, c1, c2]).unwrap();
        let q = crate::Vec3::new(2.0, 1.0, 0.0);
        let (u, p) = c.closest_point(q);
        let h = 1e-6;
        let (lo, hi) = c.domain();
        let up = (u + h).min(hi);
        let um = (u - h).max(lo);
        let dp = (p - q).len();
        let dup = (c.eval(up) - q).len();
        let dum = (c.eval(um) - q).len();
        assert!(dp <= dup + 1e-9);
        assert!(dp <= dum + 1e-9);
    }

    #[test]
    fn hostile_inputs_return_named_errors() {
        use super::NurbsError;
        // bad degree
        assert!(matches!(NurbsCurve::new(0, vec![0.0, 0.0, 1.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]), Err(NurbsError::BadDegree)));
        // too few control points
        assert!(matches!(NurbsCurve::new(2, vec![0.0; 6], vec![crate::Vec3::ZERO, crate::Vec3::X]), Err(NurbsError::BadDegree)));
        // knot count mismatch
        assert!(matches!(NurbsCurve::new(1, vec![0.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]), Err(NurbsError::KnotCountMismatch)));
        // non-monotone
        assert!(matches!(NurbsCurve::new(1, vec![0.0, 2.0, 1.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]), Err(NurbsError::NonMonotoneKnots)));
        // empty domain
        assert!(matches!(NurbsCurve::new(1, vec![0.0, 0.0, 0.0, 0.0], vec![crate::Vec3::ZERO, crate::Vec3::X]), Err(NurbsError::EmptyDomain)));
        // non-finite input
        assert!(matches!(
            NurbsCurve::new(1, vec![0.0, 0.0, f64::NAN, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]),
            Err(NurbsError::NonFiniteInput)
        ));
        assert!(matches!(
            NurbsCurve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::new(f64::NAN, 0.0, 0.0)]),
            Err(NurbsError::NonFiniteInput)
        ));
        // out-of-domain evaluation
        let c = NurbsCurve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]).unwrap();
        assert!(matches!(c.try_eval(10.0), Err(NurbsError::OutOfDomain)));
        assert!(matches!(c.try_eval(f64::NAN), Err(NurbsError::OutOfDomain)));
        // eval on out-of-domain does not panic
        let _ = c.eval(10.0);
        let _ = c.derivative(10.0, 1);
        let _ = c.closest_point(crate::Vec3::new(f64::NAN, 0.0, 0.0));
        let _ = c.length(f64::NAN);
        let _ = c.length(-1.0);
    }

    #[test]
    fn nurbs_curve_length_is_sane() {
        let a = crate::Vec3::ZERO;
        let b = crate::Vec3::new(3.0, 4.0, 0.0);
        let c = NurbsCurve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![a, b]).unwrap();
        assert!((c.length(1e-9) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn out_of_domain_methods_do_not_panic() {
        let c = NurbsCurve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![crate::Vec3::ZERO, crate::Vec3::X]).unwrap();
        for (f, name) in [(c.eval_checked(2.0).is_err(), "checked")] {
            let _ = (f, name);
        }
        assert!(c.eval_checked(2.0).is_err());
        assert!(c.eval_at(2.0).is_err());
        assert!(c.evaluate(2.0).is_err());
        assert!(c.point_at(2.0).is_err());
    }
}

/// Adaptive Gauss-Legendre over `[a, b]` with an absolute tolerance.
fn adaptive_gauss_tol(a: f64, b: f64, whole: f64, f: &impl Fn(f64) -> f64, depth: usize, tol: f64) -> f64 {
    let m = (a + b) / 2.0;
    let (l, r) = (gauss(a, m, f), gauss(m, b, f));
    if depth == 0 || !(l + r).is_finite() || (l + r - whole).abs() <= tol {
        return l + r;
    }
    adaptive_gauss_tol(a, m, l, f, depth.saturating_sub(1), tol * 0.5) + adaptive_gauss_tol(m, b, r, f, depth.saturating_sub(1), tol * 0.5)
}
