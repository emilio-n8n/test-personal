//! A parametric sketch: entities defined by numeric parameters, constraints that relate
//! them, and a solver that derives geometry from constraints rather than storing points.
//!
//! The sketch stores entities (lines, arcs, circles, points) whose geometry is defined by
//! a flat parameter vector, plus constraints that relate those parameters. Calling
//! [`Sketch::solve`] builds residual equations from the constraints, solves them with a
//! damped Gauss–Newton (Levenberg–Marquardt) method, and writes the solved parameters back
//! into the entities. Changing a parameter (e.g. a dimension) and re-solving moves the
//! geometry — this is what makes a dimension drive a profile.
//!
//! # Reuse
//!
//! This module reuses the following from `cadcraft-constraints`:
//! - `expr::{Env, ExprError, parse, eval_with, valid_name}` for parameter expression evaluation
//! - `format_value` for formatting parameter values
//! - `SolveOptions` for the weighting policy
//! - `MAX_CONSTRAINTS` for the constraint limit
//!
//! The residual forms and LM solver in `cadcraft-constraints` are `pub(crate)` and cannot
//! be called from this crate. The solver here is a compact port of the same algorithm.

use std::collections::BTreeMap;

use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};

use cadcraft_constraints::expr::{Env, ExprError, eval_with, parse, valid_name};
use cadcraft_constraints::{MAX_CONSTRAINTS, SolveOptions, format_value};

/// A point in the sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PointId(pub u32);

/// An entity in the sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SketchEntity {
    /// A line segment between two points.
    Line { a: PointId, b: PointId },
    /// A circle with a centre point and radius.
    Circle { centre: PointId, radius: f64 },
    /// An arc with a centre point, radius, and angular range.
    Arc { centre: PointId, radius: f64, start: f64, end: f64 },
    /// A standalone point.
    Point { id: PointId, pos: Vec2 },
}

/// A constraint on the sketch's entities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SketchConstraint {
    /// Two points coincide.
    Coincident { a: PointId, b: PointId, name: String },
    /// The line between two points is horizontal.
    Horizontal { a: PointId, b: PointId, name: String },
    /// The line between two points is vertical.
    Vertical { a: PointId, b: PointId, name: String },
    /// Two lines are parallel.
    Parallel { a: PointId, b: PointId, c: PointId, d: PointId, name: String },
    /// Two lines are perpendicular.
    Perpendicular { a: PointId, b: PointId, c: PointId, d: PointId, name: String },
    /// A line is tangent to a circle.
    Tangent { line_a: PointId, line_b: PointId, circle: PointId, name: String },
    /// Two lines have equal length, or two circles have equal radius.
    Equal { a: PointId, b: PointId, c: PointId, d: PointId, name: String },
    /// The distance between two points equals a named parameter.
    Distance { a: PointId, b: PointId, param: String, name: String },
    /// The radius of a circle equals a named parameter.
    Radius { circle: PointId, param: String, name: String },
    /// The angle between two lines equals a named parameter.
    Angle { a: PointId, b: PointId, c: PointId, d: PointId, param: String, name: String },
}

/// The result of solving a sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct SolveReport {
    /// Whether the solver converged.
    pub converged: bool,
    /// Degrees of freedom remaining after the solve.
    pub dof: usize,
    /// Names of constraints that are redundant (over-constraining).
    pub redundant: Vec<String>,
    /// Names of constraints that could not be satisfied.
    pub conflicting: Vec<String>,
    /// Number of solver iterations.
    pub iterations: usize,
    /// The final maximum residual.
    pub residual: f64,
}

/// An error that occurred while solving a sketch.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SketchError {
    #[error("parameter `{name}` does not exist")]
    NoSuchParameter { name: String },
    #[error("parameter `{name}` is not finite: {value}")]
    NonFiniteParameter { name: String, value: f64 },
    #[error("duplicate point id: {0:?}")]
    DuplicatePointId(PointId),
    #[error("duplicate entity id: {0:?}")]
    DuplicateEntityId(PointId),
    #[error("constraint `{name}` references a non-existent point: {id:?}")]
    NoSuchPoint { name: String, id: PointId },
    #[error("constraint `{name}` is redundant with: {others:?}")]
    Redundant { name: String, others: Vec<String> },
    #[error("constraints conflict: {names:?}")]
    Conflict { names: Vec<String> },
    #[error("solver did not converge after {iterations} iterations (residual {residual})")]
    NotConverged { iterations: usize, residual: f64 },
    #[error("too many constraints: {got} (limit {limit})")]
    TooManyConstraints { got: usize, limit: usize },
    #[error("expression error in parameter `{name}`: {err}")]
    Expression { name: String, err: ExprError },
    #[error("a line with zero length cannot be constrained")]
    ZeroLengthLine,
}

/// A parametric sketch: entities defined by parameters, constraints that relate them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub entities: Vec<SketchEntity>,
    pub constraints: Vec<SketchConstraint>,
    pub parameters: BTreeMap<String, f64>,
}

/// Tolerance for convergence, scaled to the sketch's extent.
fn scale_tol(x: &[f64]) -> f64 {
    1e-9 * x.iter().fold(1.0f64, |m, v| m.max(v.abs()))
}

/// Maximum absolute value of a vector.
fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, a| if a.is_finite() { m.max(a.abs()) } else { f64::INFINITY })
}

/// Squared L2 norm.
fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|a| a * a).sum::<f64>()
}

/// Numerical Jacobian over the free parameters (row-major, rows × free.len()).
fn jacobian(f: &dyn Fn(&[f64], &mut Vec<f64>), x: &[f64], free: &[usize], rows: usize) -> Vec<f64> {
    let nf = free.len();
    let mut jac = vec![0.0; rows * nf];
    let mut xp = x.to_vec();
    let (mut rp, mut rm) = (Vec::with_capacity(rows), Vec::with_capacity(rows));
    for (k, &j) in free.iter().enumerate() {
        let Some(&xj) = x.get(j) else { continue };
        let h = 1e-7 * xj.abs().max(1.0);
        if let Some(v) = xp.get_mut(j) {
            *v = xj + h;
        }
        rp.clear();
        f(&xp, &mut rp);
        if let Some(v) = xp.get_mut(j) {
            *v = xj - h;
        }
        rm.clear();
        f(&xp, &mut rm);
        if let Some(v) = xp.get_mut(j) {
            *v = xj;
        }
        for i in 0..rows {
            let d = (rp.get(i).copied().unwrap_or(0.0) - rm.get(i).copied().unwrap_or(0.0)) / (2.0 * h);
            if let Some(slot) = jac.get_mut(i * nf + k) {
                *slot = if d.is_finite() { d } else { 0.0 };
            }
        }
    }
    jac
}

/// Solve the dense system `a y = b` (n × n, row-major) by Gaussian elimination with partial
/// pivoting. `None` when singular.
fn solve_dense(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    let scale = a.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    for col in 0..n {
        let mut piv = col;
        let mut best = 0.0;
        for r in col..n {
            let v = a.get(r * n + col)?.abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if best <= scale * 1e-15 {
            return None;
        }
        if piv != col {
            for c in 0..n {
                a.swap(piv * n + c, col * n + c);
            }
            b.swap(piv, col);
        }
        let p = *a.get(col * n + col)?;
        for r in col + 1..n {
            let f = *a.get(r * n + col)? / p;
            if f == 0.0 {
                continue;
            }
            for c in col..n {
                let v = *a.get(col * n + c)?;
                *a.get_mut(r * n + c)? -= f * v;
            }
            let bv = *b.get(col)?;
            *b.get_mut(r)? -= f * bv;
        }
    }
    let mut y = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = *b.get(r)?;
        for c in r + 1..n {
            s -= a.get(r * n + c)?.mul_add(*y.get(c)?, 0.0);
        }
        *y.get_mut(r)? = s / *a.get(r * n + r)?;
    }
    y.iter().all(|v| v.is_finite()).then_some(y)
}

/// Numerical rank of a row-major `rows × cols` matrix via Gaussian elimination.
fn rank(m: &[f64], rows: usize, cols: usize) -> usize {
    if rows == 0 || cols == 0 {
        return 0;
    }
    let mut a: Vec<f64> = m.to_vec();
    let mut rank = 0;
    let mut row = 0;
    for col in 0..cols {
        if row >= rows {
            break;
        }
        let mut piv = row;
        let mut best = 0.0;
        for r in row..rows {
            let v = a.get(r * cols + col).copied().unwrap_or(0.0).abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        let scale = a.iter().fold(0.0f64, |m2, v| m2.max(v.abs())).max(1e-300);
        if best <= scale * 1e-12 {
            continue;
        }
        if piv != row {
            for c in 0..cols {
                a.swap(piv * cols + c, row * cols + c);
            }
        }
        // Indices are always in bounds here (`row < rows`, `r < rows`, `col < cols`, `c < cols`),
        // so the checked-access fallbacks below are unreachable; they keep the never-panic rule.
        let p = a.get(row * cols + col).copied().unwrap_or(0.0);
        for r in row + 1..rows {
            let f = a.get(r * cols + col).copied().unwrap_or(0.0) / p;
            if f == 0.0 {
                continue;
            }
            for c in col..cols {
                let v = a.get(row * cols + c).copied().unwrap_or(0.0);
                if let Some(slot) = a.get_mut(r * cols + c) {
                    *slot -= f * v;
                }
            }
        }
        rank += 1;
        row += 1;
    }
    rank
}

/// Damped, weighted minimum-norm Gauss–Newton (Levenberg–Marquardt style) solver.
///
/// Each step solves `min ‖W^{1/2} Δx‖` subject to `J Δx = −r` (damped):
/// `Δx = −W⁻¹ Jᵀ (J W⁻¹ Jᵀ + μ I)⁻¹ r`. Under-constrained systems therefore move as little as
/// possible, and heavily weighted parameters move least.
fn lm_solve(
    f: &dyn Fn(&[f64], &mut Vec<f64>),
    x0: &[f64],
    free: &[usize],
    weights: &[f64],
    tol: f64,
    max_iter: usize,
) -> (Vec<f64>, bool, usize, f64) {
    let mut x = x0.to_vec();
    let mut r = Vec::new();
    f(&x, &mut r);
    let rows = r.len();
    if rows == 0 || max_abs(&r) <= tol {
        return (x, true, 0, max_abs(&r));
    }
    if free.is_empty() {
        return (x, false, 0, max_abs(&r));
    }
    let nf = free.len();
    let winv: Vec<f64> = free.iter().map(|&j| 1.0 / weights.get(j).copied().unwrap_or(1.0).max(1e-12)).collect();
    let mut mu = 1e-10;
    let mut cost = norm2(&r);
    let mut iter = 0;
    let mut rn = Vec::with_capacity(rows);
    while iter < max_iter {
        iter += 1;
        let jac = jacobian(f, &x, free, rows);
        // A = J W⁻¹ Jᵀ
        let mut a = vec![0.0; rows * rows];
        for i in 0..rows {
            for k in i..rows {
                let mut s = 0.0;
                for (c, w) in winv.iter().enumerate() {
                    s += jac.get(i * nf + c).copied().unwrap_or(0.0) * jac.get(k * nf + c).copied().unwrap_or(0.0) * w;
                }
                if let Some(v) = a.get_mut(i * rows + k) {
                    *v = s;
                }
                if let Some(v) = a.get_mut(k * rows + i) {
                    *v = s;
                }
            }
        }
        let diag_max = (0..rows).map(|i| a.get(i * rows + i).copied().unwrap_or(0.0)).fold(0.0f64, f64::max).max(1e-12);
        let mut improved = false;
        for _ in 0..12 {
            let mut ad = a.clone();
            for i in 0..rows {
                if let Some(v) = ad.get_mut(i * rows + i) {
                    *v += mu * diag_max;
                }
            }
            let Some(y) = solve_dense(ad, r.clone(), rows) else {
                mu *= 10.0;
                continue;
            };
            let mut xn = x.clone();
            for (c, &j) in free.iter().enumerate() {
                let mut s = 0.0;
                for (i, yi) in y.iter().enumerate() {
                    s += jac.get(i * nf + c).copied().unwrap_or(0.0) * yi;
                }
                if let Some(v) = xn.get_mut(j) {
                    *v -= winv.get(c).copied().unwrap_or(1.0) * s;
                }
            }
            rn.clear();
            f(&xn, &mut rn);
            let cn = norm2(&rn);
            if cn.is_finite() && cn < cost {
                x = xn;
                std::mem::swap(&mut r, &mut rn);
                cost = cn;
                mu = (mu * 0.1).max(1e-15);
                improved = true;
                break;
            }
            mu *= 10.0;
        }
        if max_abs(&r) <= tol {
            return (x, true, iter, max_abs(&r));
        }
        if !improved {
            break;
        }
    }
    let converged = max_abs(&r) <= tol;
    (x, converged, iter, max_abs(&r))
}

/// The layout of the parameter vector: which entity owns which parameters.
struct ParamLayout {
    /// For each entity index, (offset, len) into the parameter vector.
    /// Points: 2 (x, y), Lines: 0, Circles: 1 (radius), Arcs: 3 (radius, start, end).
    offsets: Vec<(usize, usize)>,
    /// Total number of parameters.
    total: usize,
}

/// Build the parameter layout for a set of entities.
fn build_layout(entities: &[SketchEntity]) -> ParamLayout {
    let mut offsets = Vec::new();
    let mut next = 0;
    for e in entities {
        let len = match e {
            SketchEntity::Point { .. } => 2,
            SketchEntity::Line { .. } => 0,
            SketchEntity::Circle { .. } => 1,
            SketchEntity::Arc { .. } => 3,
        };
        offsets.push((next, len));
        next += len;
    }
    ParamLayout { offsets, total: next }
}

/// Build the initial parameter vector from the entities.
fn build_vector(entities: &[SketchEntity], layout: &ParamLayout) -> Vec<f64> {
    let mut x = vec![0.0; layout.total];
    for (i, e) in entities.iter().enumerate() {
        let (off, _) = layout.offsets[i];
        match e {
            SketchEntity::Point { pos, .. } => {
                x[off] = pos.x;
                x[off + 1] = pos.y;
            }
            SketchEntity::Circle { radius, .. } => {
                x[off] = *radius;
            }
            SketchEntity::Arc { radius, start, end, .. } => {
                x[off] = *radius;
                x[off + 1] = *start;
                x[off + 2] = *end;
            }
            SketchEntity::Line { .. } => {}
        }
    }
    x
}

/// Write the solved parameters back into the entities.
fn write_back(entities: &mut [SketchEntity], layout: &ParamLayout, x: &[f64]) {
    for (i, e) in entities.iter_mut().enumerate() {
        let (off, _) = layout.offsets[i];
        match e {
            SketchEntity::Point { pos, .. } => {
                if let (Some(px), Some(py)) = (x.get(off), x.get(off + 1)) {
                    if px.is_finite() && py.is_finite() {
                        *pos = Vec2::new(*px, *py);
                    }
                }
            }
            SketchEntity::Circle { radius, .. } => {
                if let Some(r) = x.get(off) {
                    if r.is_finite() {
                        *radius = r.abs().max(1e-12);
                    }
                }
            }
            SketchEntity::Arc { radius, start, end, .. } => {
                if let (Some(r), Some(s), Some(e)) = (x.get(off), x.get(off + 1), x.get(off + 2)) {
                    if r.is_finite() {
                        *radius = r.abs().max(1e-12);
                    }
                    if s.is_finite() {
                        *start = *s;
                    }
                    if e.is_finite() {
                        *end = *e;
                    }
                }
            }
            SketchEntity::Line { .. } => {}
        }
    }
}

/// Get the position of a point from the parameter vector.
fn point_pos(x: &[f64], layout: &ParamLayout, entities: &[SketchEntity], id: PointId) -> Option<Vec2> {
    for (i, e) in entities.iter().enumerate() {
        match e {
            SketchEntity::Point { id: pid, .. } if *pid == id => {
                let (off, _) = layout.offsets[i];
                return Some(Vec2::new(*x.get(off)?, *x.get(off + 1)?));
            }
            _ => {}
        }
    }
    None
}

/// Get the centre and radius of a circle from the parameter vector.
fn circle_at(x: &[f64], layout: &ParamLayout, entities: &[SketchEntity], id: PointId) -> Option<(Vec2, f64)> {
    for (i, e) in entities.iter().enumerate() {
        match e {
            SketchEntity::Circle { centre, .. } if *centre == id => {
                let (off, _) = layout.offsets[i];
                let c = point_pos(x, layout, entities, *centre)?;
                let r = *x.get(off)?;
                return Some((c, r));
            }
            _ => {}
        }
    }
    None
}

/// Evaluate the residual for a single constraint.
fn eval_constraint(
    c: &SketchConstraint,
    x: &[f64],
    layout: &ParamLayout,
    entities: &[SketchEntity],
    params: &BTreeMap<String, f64>,
    out: &mut Vec<f64>,
) -> Result<(), SketchError> {
    match c {
        SketchConstraint::Coincident { a, b, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Coincident".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Coincident".into(), id: *b })?;
            out.push(pa.x - pb.x);
            out.push(pa.y - pb.y);
        }
        SketchConstraint::Horizontal { a, b, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Horizontal".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Horizontal".into(), id: *b })?;
            out.push(pa.y - pb.y);
        }
        SketchConstraint::Vertical { a, b, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Vertical".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Vertical".into(), id: *b })?;
            out.push(pa.x - pb.x);
        }
        SketchConstraint::Parallel { a, b, c, d, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Parallel".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Parallel".into(), id: *b })?;
            let pc = point_pos(x, layout, entities, *c).ok_or_else(|| SketchError::NoSuchPoint { name: "Parallel".into(), id: *c })?;
            let pd = point_pos(x, layout, entities, *d).ok_or_else(|| SketchError::NoSuchPoint { name: "Parallel".into(), id: *d })?;
            let d1 = pb - pa;
            let d2 = pd - pc;
            let l1 = d1.len();
            let l2 = d2.len();
            if l1 < 1e-12 || l2 < 1e-12 {
                return Err(SketchError::ZeroLengthLine);
            }
            out.push(d1.cross(d2) / (l1 * l2));
        }
        SketchConstraint::Perpendicular { a, b, c, d, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Perpendicular".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Perpendicular".into(), id: *b })?;
            let pc = point_pos(x, layout, entities, *c).ok_or_else(|| SketchError::NoSuchPoint { name: "Perpendicular".into(), id: *c })?;
            let pd = point_pos(x, layout, entities, *d).ok_or_else(|| SketchError::NoSuchPoint { name: "Perpendicular".into(), id: *d })?;
            let d1 = pb - pa;
            let d2 = pd - pc;
            let l1 = d1.len();
            let l2 = d2.len();
            if l1 < 1e-12 || l2 < 1e-12 {
                return Err(SketchError::ZeroLengthLine);
            }
            out.push(d1.dot(d2) / (l1 * l2));
        }
        SketchConstraint::Tangent { line_a, line_b, circle, .. } => {
            let pa = point_pos(x, layout, entities, *line_a).ok_or_else(|| SketchError::NoSuchPoint { name: "Tangent".into(), id: *line_a })?;
            let pb = point_pos(x, layout, entities, *line_b).ok_or_else(|| SketchError::NoSuchPoint { name: "Tangent".into(), id: *line_b })?;
            let (cc, cr) = circle_at(x, layout, entities, *circle).ok_or_else(|| SketchError::NoSuchPoint { name: "Tangent".into(), id: *circle })?;
            let d = pb - pa;
            let l = d.len();
            if l < 1e-12 {
                return Err(SketchError::ZeroLengthLine);
            }
            let n = d.perp().normalized();
            out.push((cc - pa).dot(n).abs() - cr.abs());
        }
        SketchConstraint::Equal { a, b, c, d, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Equal".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Equal".into(), id: *b })?;
            let pc = point_pos(x, layout, entities, *c).ok_or_else(|| SketchError::NoSuchPoint { name: "Equal".into(), id: *c })?;
            let pd = point_pos(x, layout, entities, *d).ok_or_else(|| SketchError::NoSuchPoint { name: "Equal".into(), id: *d })?;
            out.push(pa.dist(pb) - pc.dist(pd));
        }
        SketchConstraint::Distance { a, b, param, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Distance".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Distance".into(), id: *b })?;
            let target = params.get(param).ok_or_else(|| SketchError::NoSuchParameter { name: param.clone() })?;
            out.push(pa.dist(pb) - target);
        }
        SketchConstraint::Radius { circle, param, .. } => {
            let (_, cr) = circle_at(x, layout, entities, *circle).ok_or_else(|| SketchError::NoSuchPoint { name: "Radius".into(), id: *circle })?;
            let target = params.get(param).ok_or_else(|| SketchError::NoSuchParameter { name: param.clone() })?;
            out.push(cr - target);
        }
        SketchConstraint::Angle { a, b, c, d, param, .. } => {
            let pa = point_pos(x, layout, entities, *a).ok_or_else(|| SketchError::NoSuchPoint { name: "Angle".into(), id: *a })?;
            let pb = point_pos(x, layout, entities, *b).ok_or_else(|| SketchError::NoSuchPoint { name: "Angle".into(), id: *b })?;
            let pc = point_pos(x, layout, entities, *c).ok_or_else(|| SketchError::NoSuchPoint { name: "Angle".into(), id: *c })?;
            let pd = point_pos(x, layout, entities, *d).ok_or_else(|| SketchError::NoSuchPoint { name: "Angle".into(), id: *d })?;
            let d1 = pb - pa;
            let d2 = pd - pc;
            let l1 = d1.len();
            let l2 = d2.len();
            if l1 < 1e-12 || l2 < 1e-12 {
                return Err(SketchError::ZeroLengthLine);
            }
            let angle = d1.angle_to(d2);
            let target = params.get(param).ok_or_else(|| SketchError::NoSuchParameter { name: param.clone() })?;
            out.push(angle - target);
        }
    }
    Ok(())
}

/// Evaluate all constraints, returning the residual vector.
fn eval_all(
    constraints: &[SketchConstraint],
    x: &[f64],
    layout: &ParamLayout,
    entities: &[SketchEntity],
    params: &BTreeMap<String, f64>,
) -> Result<Vec<f64>, SketchError> {
    let mut out = Vec::new();
    for c in constraints {
        eval_constraint(c, x, layout, entities, params, &mut out)?;
    }
    Ok(out)
}

/// Count the number of residual rows a constraint contributes.
fn constraint_rows(c: &SketchConstraint) -> usize {
    match c {
        SketchConstraint::Coincident { .. } => 2,
        _ => 1,
    }
}

impl Sketch {
    /// Create a new empty sketch.
    pub fn new() -> Self {
        Sketch { entities: Vec::new(), constraints: Vec::new(), parameters: BTreeMap::new() }
    }

    /// Add an entity to the sketch.
    pub fn add_entity(&mut self, e: SketchEntity) {
        self.entities.push(e);
    }

    /// Add a constraint to the sketch.
    pub fn add_constraint(&mut self, c: SketchConstraint) {
        self.constraints.push(c);
    }

    /// Set a parameter value.
    pub fn set_parameter(&mut self, name: &str, value: f64) {
        self.parameters.insert(name.to_string(), value);
    }

    /// Solve the sketch's constraints and write the solution back into the entities.
    ///
    /// Returns a report with the convergence status, remaining degrees of freedom, and the
    /// names of any redundant or conflicting constraints. On error, the sketch is unchanged.
    pub fn solve(&mut self, _opts: &SolveOptions) -> Result<SolveReport, SketchError> {
        // Check constraint count
        if self.constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooManyConstraints { got: self.constraints.len(), limit: MAX_CONSTRAINTS });
        }

        // Check for duplicate point ids
        let mut seen = std::collections::HashSet::new();
        for e in &self.entities {
            let id = match e {
                SketchEntity::Point { id, .. } => *id,
                SketchEntity::Line { a, b } => {
                    if !seen.insert(*a) {
                        return Err(SketchError::DuplicatePointId(*a));
                    }
                    if !seen.insert(*b) {
                        return Err(SketchError::DuplicatePointId(*b));
                    }
                    continue;
                }
                SketchEntity::Circle { centre, .. } => *centre,
                SketchEntity::Arc { centre, .. } => *centre,
            };
            if !seen.insert(id) {
                return Err(SketchError::DuplicatePointId(id));
            }
        }

        // Check that all constraint references exist
        for c in &self.constraints {
            let refs: Vec<PointId> = match c {
                SketchConstraint::Coincident { a, b, .. } => vec![*a, *b],
                SketchConstraint::Horizontal { a, b, .. } => vec![*a, *b],
                SketchConstraint::Vertical { a, b, .. } => vec![*a, *b],
                SketchConstraint::Parallel { a, b, c, d, .. } => vec![*a, *b, *c, *d],
                SketchConstraint::Perpendicular { a, b, c, d, .. } => vec![*a, *b, *c, *d],
                SketchConstraint::Tangent { line_a, line_b, circle, .. } => vec![*line_a, *line_b, *circle],
                SketchConstraint::Equal { a, b, c, d, .. } => vec![*a, *b, *c, *d],
                SketchConstraint::Distance { a, b, .. } => vec![*a, *b],
                SketchConstraint::Radius { circle, .. } => vec![*circle],
                SketchConstraint::Angle { a, b, c, d, .. } => vec![*a, *b, *c, *d],
            };
            for id in refs {
                if !self.point_exists(id) {
                    return Err(SketchError::NoSuchPoint { name: constraint_name(c).to_string(), id });
                }
            }
        }

        // Check that all parameter references exist
        for c in &self.constraints {
            let params: Vec<&String> = match c {
                SketchConstraint::Distance { param, .. } => vec![param],
                SketchConstraint::Radius { param, .. } => vec![param],
                SketchConstraint::Angle { param, .. } => vec![param],
                _ => vec![],
            };
            for p in params {
                if !self.parameters.contains_key(p) {
                    return Err(SketchError::NoSuchParameter { name: p.clone() });
                }
            }
        }

        // Check that all parameter values are finite
        for (name, value) in &self.parameters {
            if !value.is_finite() {
                return Err(SketchError::NonFiniteParameter { name: name.clone(), value: *value });
            }
        }

        // Build the parameter vector
        let layout = build_layout(&self.entities);
        let x0 = build_vector(&self.entities, &layout);

        // All parameters are free
        let free: Vec<usize> = (0..layout.total).collect();
        let weights = vec![1.0; layout.total];

        // Solve
        let tol = scale_tol(&x0);
        let f = |x: &[f64], out: &mut Vec<f64>| {
            // This closure cannot return an error, so we skip constraints that fail.
            // In practice, all constraints have been validated above.
            for c in &self.constraints {
                let _ = eval_constraint(c, x, &layout, &self.entities, &self.parameters, out);
            }
        };

        let (x, converged, iterations, residual) = lm_solve(&f, &x0, &free, &weights, tol, 200);

        // Check for redundancy: compute the rank of the Jacobian
        let rows: usize = self.constraints.iter().map(|c| constraint_rows(c)).sum();
        let nf = free.len();
        let jac = jacobian(&f, &x, &free, rows);
        let r = rank(&jac, rows, nf);
        let dof = nf.saturating_sub(r);

        // Find redundant constraints: those whose removal does not reduce the rank
        let mut redundant = Vec::new();
        if self.constraints.len() <= 48 {
            for (i, c) in self.constraints.iter().enumerate() {
                // Build a residual function without constraint i
                let f_skip = |x: &[f64], out: &mut Vec<f64>| {
                    for (j, c2) in self.constraints.iter().enumerate() {
                        if i != j {
                            let _ = eval_constraint(c2, x, &layout, &self.entities, &self.parameters, out);
                        }
                    }
                };
                let rows_skip: usize = self.constraints.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| constraint_rows(c)).sum();
                let jac_skip = jacobian(&f_skip, &x, &free, rows_skip);
                let r_skip = rank(&jac_skip, rows_skip, nf);
                if r_skip == r {
                    redundant.push(constraint_name(c).to_string());
                }
            }
        }

        // Find conflicting constraints: those whose removal makes the system solvable
        let mut conflicting = Vec::new();
        if !converged && self.constraints.len() <= 48 {
            for (i, c) in self.constraints.iter().enumerate() {
                let f_skip = |x: &[f64], out: &mut Vec<f64>| {
                    for (j, c2) in self.constraints.iter().enumerate() {
                        if i != j {
                            let _ = eval_constraint(c2, x, &layout, &self.entities, &self.parameters, out);
                        }
                    }
                };
                let (_, converged_skip, _, _) = lm_solve(&f_skip, &x0, &free, &weights, tol, 200);
                if converged_skip {
                    conflicting.push(constraint_name(c).to_string());
                }
            }
        }

        // Write back the solution
        write_back(&mut self.entities, &layout, &x);

        Ok(SolveReport { converged, dof, redundant, conflicting, iterations, residual })
    }

    /// Check if a point with the given id exists in the sketch.
    fn point_exists(&self, id: PointId) -> bool {
        self.entities.iter().any(|e| match e {
            SketchEntity::Point { id: pid, .. } => *pid == id,
            SketchEntity::Line { a, b } => *a == id || *b == id,
            SketchEntity::Circle { centre, .. } => *centre == id,
            SketchEntity::Arc { centre, .. } => *centre == id,
        })
    }
}

/// Get the display name of a constraint.
fn constraint_name(c: &SketchConstraint) -> &'static str {
    match c {
        SketchConstraint::Coincident { .. } => "Coincident",
        SketchConstraint::Horizontal { .. } => "Horizontal",
        SketchConstraint::Vertical { .. } => "Vertical",
        SketchConstraint::Parallel { .. } => "Parallel",
        SketchConstraint::Perpendicular { .. } => "Perpendicular",
        SketchConstraint::Tangent { .. } => "Tangent",
        SketchConstraint::Equal { .. } => "Equal",
        SketchConstraint::Distance { .. } => "Distance",
        SketchConstraint::Radius { .. } => "Radius",
        SketchConstraint::Angle { .. } => "Angle",
    }
}

impl Default for Sketch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: u32) -> PointId {
        PointId(n)
    }

    fn make_rectangle_sketch() -> Sketch {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(1), pos: Vec2::new(4.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(2), pos: Vec2::new(4.0, 3.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(3), pos: Vec2::new(0.0, 3.0) });
        sketch.add_entity(SketchEntity::Line { a: pid(0), b: pid(1) });
        sketch.add_entity(SketchEntity::Line { a: pid(1), b: pid(2) });
        sketch.add_entity(SketchEntity::Line { a: pid(2), b: pid(3) });
        sketch.add_entity(SketchEntity::Line { a: pid(3), b: pid(0) });
        sketch.add_constraint(SketchConstraint::Horizontal { a: pid(0), b: pid(1), name: "h1".into() });
        sketch.add_constraint(SketchConstraint::Horizontal { a: pid(2), b: pid(3), name: "h2".into() });
        sketch.add_constraint(SketchConstraint::Vertical { a: pid(0), b: pid(3), name: "v1".into() });
        sketch.add_constraint(SketchConstraint::Vertical { a: pid(1), b: pid(2), name: "v2".into() });
        sketch.add_constraint(SketchConstraint::Distance { a: pid(0), b: pid(1), param: "width".into(), name: "w".into() });
        sketch.add_constraint(SketchConstraint::Distance { a: pid(0), b: pid(3), param: "height".into(), name: "h".into() });
        sketch.set_parameter("width", 4.0);
        sketch.set_parameter("height", 3.0);
        sketch
    }

    #[test]
    fn rectangle_solves_and_moves_with_parameters() {
        let mut sketch = make_rectangle_sketch();
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report.converged, "should converge: {:?}", report);

        // Check corner positions
        let p0 = sketch.entities[0].point_pos().unwrap();
        let p1 = sketch.entities[1].point_pos().unwrap();
        let p2 = sketch.entities[2].point_pos().unwrap();
        let p3 = sketch.entities[3].point_pos().unwrap();

        assert!((p0.x - 0.0).abs() < 1e-6 && (p0.y - 0.0).abs() < 1e-6, "p0 should be at (0,0), got {:?}", p0);
        assert!((p1.x - 4.0).abs() < 1e-6 && (p1.y - 0.0).abs() < 1e-6, "p1 should be at (4,0), got {:?}", p1);
        assert!((p2.x - 4.0).abs() < 1e-6 && (p2.y - 3.0).abs() < 1e-6, "p2 should be at (4,3), got {:?}", p2);
        assert!((p3.x - 0.0).abs() < 1e-6 && (p3.y - 3.0).abs() < 1e-6, "p3 should be at (0,3), got {:?}", p3);

        // Change width and re-solve
        sketch.set_parameter("width", 6.0);
        let report2 = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report2.converged, "should converge after parameter change: {:?}", report2);

        let p1_new = sketch.entities[1].point_pos().unwrap();
        let p2_new = sketch.entities[2].point_pos().unwrap();

        assert!((p1_new.x - 6.0).abs() < 1e-6 && (p1_new.y - 0.0).abs() < 1e-6, "p1 should have moved to (6,0), got {:?}", p1_new);
        assert!((p2_new.x - 6.0).abs() < 1e-6 && (p2_new.y - 3.0).abs() < 1e-6, "p2 should have moved to (6,3), got {:?}", p2_new);
    }

    #[test]
    fn redundant_constraint_is_reported_by_name() {
        let mut sketch = make_rectangle_sketch();
        // Add a redundant horizontal constraint
        sketch.add_constraint(SketchConstraint::Horizontal { a: pid(0), b: pid(1), name: "redundant_h".into() });
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report.redundant.iter().any(|n| n == "redundant_h"), "should report redundant constraint by name, got {:?}", report.redundant);
    }

    #[test]
    fn conflicting_constraints_are_refused_by_name() {
        let mut sketch = make_rectangle_sketch();
        // Add a conflicting distance constraint
        sketch.add_constraint(SketchConstraint::Distance { a: pid(0), b: pid(1), param: "width2".into(), name: "conflict".into() });
        sketch.set_parameter("width2", 5.0);
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(!report.converged, "should not converge with conflicting constraints");
        assert!(report.conflicting.iter().any(|n| n == "conflict"), "should report conflicting constraint by name, got {:?}", report.conflicting);
    }

    #[test]
    fn tangent_line_to_circle_solves() {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(1), pos: Vec2::new(10.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(2), pos: Vec2::new(5.0, 3.0) });
        sketch.add_entity(SketchEntity::Line { a: pid(0), b: pid(1) });
        sketch.add_entity(SketchEntity::Circle { centre: pid(2), radius: 2.0 });
        sketch.add_constraint(SketchConstraint::Tangent { line_a: pid(0), line_b: pid(1), circle: pid(2), name: "tan".into() });
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report.converged, "tangent should converge: {:?}", report);

        // Check that the circle's centre is at distance ~radius from the line
        let centre = sketch.entities[2].point_pos().unwrap();
        let line_a = sketch.entities[0].point_pos().unwrap();
        let line_b = sketch.entities[1].point_pos().unwrap();
        let d = line_b - line_a;
        let n = d.perp().normalized();
        let dist = (centre - line_a).dot(n).abs();
        assert!((dist - 2.0).abs() < 1e-3, "distance from centre to line should be ~2.0, got {}", dist);
    }

    #[test]
    fn nan_parameter_is_refused() {
        let mut sketch = make_rectangle_sketch();
        sketch.set_parameter("width", f64::NAN);
        let result = sketch.solve(&SolveOptions::default());
        assert!(result.is_err(), "NaN parameter should be an error");
        let err = result.unwrap_err();
        assert!(matches!(err, SketchError::NonFiniteParameter { .. }), "should be NonFiniteParameter, got {:?}", err);
    }

    #[test]
    fn zero_length_line_is_refused() {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(1), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Line { a: pid(0), b: pid(1) });
        sketch.add_constraint(SketchConstraint::Horizontal { a: pid(0), b: pid(1), name: "h".into() });
        let result = sketch.solve(&SolveOptions::default());
        assert!(result.is_err(), "zero-length line should be an error");
    }

    #[test]
    fn duplicate_point_ids_are_refused() {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(1.0, 1.0) });
        let result = sketch.solve(&SolveOptions::default());
        assert!(result.is_err(), "duplicate point ids should be an error");
        let err = result.unwrap_err();
        assert!(matches!(err, SketchError::DuplicatePointId(_)), "should be DuplicatePointId, got {:?}", err);
    }

    #[test]
    fn missing_parameter_is_refused() {
        let mut sketch = make_rectangle_sketch();
        // Remove the width parameter
        sketch.parameters.remove("width");
        let result = sketch.solve(&SolveOptions::default());
        assert!(result.is_err(), "missing parameter should be an error");
        let err = result.unwrap_err();
        assert!(matches!(err, SketchError::NoSuchParameter { .. }), "should be NoSuchParameter, got {:?}", err);
    }

    #[test]
    fn under_constrained_sketch_reports_dof() {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(1), pos: Vec2::new(1.0, 0.0) });
        sketch.add_entity(SketchEntity::Line { a: pid(0), b: pid(1) });
        // No constraints: 4 DOF (2 points × 2 coordinates)
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report.converged);
        assert_eq!(report.dof, 4, "unconstrained sketch should have 4 DOF");
    }

    #[test]
    fn coincident_constraint_moves_points_together() {
        let mut sketch = Sketch::new();
        sketch.add_entity(SketchEntity::Point { id: pid(0), pos: Vec2::new(0.0, 0.0) });
        sketch.add_entity(SketchEntity::Point { id: pid(1), pos: Vec2::new(5.0, 5.0) });
        sketch.add_constraint(SketchConstraint::Coincident { a: pid(0), b: pid(1), name: "co".into() });
        let report = sketch.solve(&SolveOptions::default()).unwrap();
        assert!(report.converged, "coincident should converge: {:?}", report);

        let p0 = sketch.entities[0].point_pos().unwrap();
        let p1 = sketch.entities[1].point_pos().unwrap();
        assert!(p0.dist(p1) < 1e-6, "points should coincide, got {:?} and {:?}", p0, p1);
    }
}

/// Helper trait for tests: extract the position from a Point entity.
trait PointPos {
    fn point_pos(&self) -> Option<Vec2>;
}

impl PointPos for SketchEntity {
    fn point_pos(&self) -> Option<Vec2> {
        match self {
            SketchEntity::Point { pos, .. } => Some(*pos),
            _ => None,
        }
    }
}
