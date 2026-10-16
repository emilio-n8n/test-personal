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
//! - `lm::{solve_least_squares, Options, Report}` — the constraint crate's damped, weighted
//!   minimum-norm Gauss–Newton solver, which is public: this module supplies the residual
//!   equations and the initial guess, and carries no solver code of its own
//! - `SolveOptions` for the calling convention (its `keep`/`prefer_move` handles refer to
//!   drawing handles, which a sketch's points are not yet, so its fields are not read here)
//! - `MAX_CONSTRAINTS` for the constraint limit
//! - `expr::ExprError` for expression errors in parameter references
//!
//! The numerical Jacobian and rank helpers of `cadcraft-constraints::lm` are `pub(crate)`.
//! The copies here (`jacobian`, `rank`) exist only for the redundancy and DOF probes, which
//! need them over this module's own parameter layout; the solve itself is the shared solver.

use std::collections::{BTreeMap, HashSet};

use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};

use cadcraft_constraints::expr::ExprError;
use cadcraft_constraints::lm::{Options, Report, solve_least_squares};
use cadcraft_constraints::{MAX_CONSTRAINTS, SolveOptions};

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
    ///
    /// The constraint locks the circle's radius: tangency relates the line to the circle's
    /// current size, so the correction for tangency moves the line and the centre, never the
    /// radius. The one exception is a circle that a [`SketchConstraint::Radius`] constraint
    /// drives — then the radius is free again, and the tangency follows the driven radius
    /// instead of fighting it.
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

/// Weight of the sketch's anchor in the solve: the first [`SketchEntity::Point`] is the
/// corner everything else was drawn against, so a constraint change must not drag it away —
/// with unit weights the minimum-norm step would split a dimension's correction evenly
/// between both of its ends and slide the anchor along with them. At this weight the anchor
/// moves by at most about the correction divided by [`ANCHOR_WEIGHT`] (≈ nanometres for
/// drawing-scale corrections), which every geometric tolerance here is far above. A weight is
/// not a lock: the anchor still counts in the degrees of freedom, it just does not drift.
const ANCHOR_WEIGHT: f64 = 1e9;

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

/// The parameter index of a circle's radius, or `None` when no circle with that centre exists.
fn circle_radius_index(layout: &ParamLayout, entities: &[SketchEntity], id: PointId) -> Option<usize> {
    for (i, e) in entities.iter().enumerate() {
        if let SketchEntity::Circle { centre, .. } = e
            && *centre == id
        {
            return layout.offsets.get(i).map(|(off, _)| *off);
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
    ///
    /// The solve itself is `cadcraft-constraints`' shared solver (`lm::solve_least_squares`):
    /// a damped, weighted minimum-norm Gauss–Newton that moves the geometry as little as
    /// possible. A system with more than 800 residual rows (`lm::MAX_ROWS`) is reported
    /// non-converged rather than attempted.
    pub fn solve(&mut self, _opts: &SolveOptions) -> Result<SolveReport, SketchError> {
        // Check constraint count
        if self.constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooManyConstraints { got: self.constraints.len(), limit: MAX_CONSTRAINTS });
        }

        // Check for duplicate point ids.
        //
        // Only two `Point` entities claiming the same id are a duplicate. A line, circle or arc
        // merely *refers* to a point by id -- a line whose endpoint is a corner the sketch also
        // declares as a `Point` is the normal way to build one, not a collision. Inserting those
        // referenced ids here refused every sketch that had both, before the solver ever ran.
        let mut seen = std::collections::HashSet::new();
        for e in &self.entities {
            if let SketchEntity::Point { id, .. } = e
                && !seen.insert(*id)
            {
                return Err(SketchError::DuplicatePointId(*id));
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
                    return Err(SketchError::NoSuchPoint { name: constraint_name(c), id });
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

        // Decision: a directional constraint on a zero-length line is REFUSED, not
        // satisfied. A segment with no length has no direction, so "this line is
        // horizontal" on coincident points would be trivially true — an equation that adds
        // nothing while claiming to constrain the sketch, and one the solver can never
        // report as violated. `Distance` and `Equal` keep their meaning between coincident
        // points, so they stay allowed. The tolerance scales with the sketch's extent: a
        // segment shorter than 1e-9 of it has no usable direction.
        let scale = x0.iter().fold(1.0f64, |m, v| m.max(v.abs()));
        let eps = 1e-9 * scale.max(1e-12);
        for c in &self.constraints {
            let segs: &[(PointId, PointId)] = match c {
                SketchConstraint::Horizontal { a, b, .. } | SketchConstraint::Vertical { a, b, .. } => &[(*a, *b)],
                SketchConstraint::Parallel { a, b, c: d, d: e, .. }
                | SketchConstraint::Perpendicular { a, b, c: d, d: e, .. }
                | SketchConstraint::Angle { a, b, c: d, d: e, .. } => &[(*a, *b), (*d, *e)],
                SketchConstraint::Tangent { line_a, line_b, .. } => &[(*line_a, *line_b)],
                _ => &[],
            };
            for (a, b) in segs {
                let Some(pa) = point_pos(&x0, &layout, &self.entities, *a) else {
                    return Err(SketchError::NoSuchPoint { name: constraint_name(c), id: *a });
                };
                let Some(pb) = point_pos(&x0, &layout, &self.entities, *b) else {
                    return Err(SketchError::NoSuchPoint { name: constraint_name(c), id: *b });
                };
                if pa.dist(pb) <= eps {
                    return Err(SketchError::ZeroLengthLine);
                }
            }
        }

        // Free parameters: everything moves, except radii locked by tangency (see the
        // `Tangent` variant's doc — a tangent's correction belongs to the line and the
        // centre, not to the radius, unless a `Radius` dimension drives that circle).
        let tangent_locked: HashSet<usize> = self
            .constraints
            .iter()
            .filter_map(|c| match c {
                SketchConstraint::Tangent { circle, .. } => circle_radius_index(&layout, &self.entities, *circle),
                _ => None,
            })
            .collect();
        let radius_driven: HashSet<usize> = self
            .constraints
            .iter()
            .filter_map(|c| match c {
                SketchConstraint::Radius { circle, .. } => circle_radius_index(&layout, &self.entities, *circle),
                _ => None,
            })
            .collect();
        let mut free: Vec<usize> = (0..layout.total).filter(|j| !tangent_locked.contains(j) || radius_driven.contains(j)).collect();
        if free.is_empty() && layout.total > 0 {
            // The solver reads `free: []` as "everything may move", which would unlock the
            // radii; an index past the end of the vector is ignored, so it expresses
            // "nothing may move" — the honest answer when every parameter is locked.
            free.push(layout.total);
        }

        // Weights: the sketch's first point is its anchor (see `ANCHOR_WEIGHT`); all other
        // parameters move at unit weight.
        let anchor: Option<usize> = self.entities.iter().enumerate().find_map(|(i, e)| match e {
            SketchEntity::Point { .. } => layout.offsets.get(i).map(|(off, _)| *off),
            _ => None,
        });
        let weights: Vec<f64> = (0..layout.total)
            .map(|j| match anchor {
                Some(a) if j == a || j == a + 1 => ANCHOR_WEIGHT,
                _ => 1.0,
            })
            .collect();

        // Solve. The tolerance scales with the sketch's extent — a fixed 1e-12 sits below
        // the coordinate noise of a large drawing.
        let tol = scale_tol(&x0);
        let f = |x: &[f64], out: &mut Vec<f64>| {
            // This closure cannot return an error, so we skip constraints that fail.
            // In practice, all constraints have been validated above.
            for c in &self.constraints {
                let _ = eval_constraint(c, x, &layout, &self.entities, &self.parameters, out);
            }
        };
        let opts = Options { tol, free: free.clone(), weights: weights.clone(), ..Options::default() };
        let Report { x, converged, iterations, residual } = solve_least_squares(&f, &x0, &opts);

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
                    redundant.push(constraint_name(c));
                }
            }
        }

        // Find conflicting constraints: those whose removal makes the system solvable
        let mut conflicting = Vec::new();
        if !converged && self.constraints.len() <= 48 {
            let skip_opts = Options { free, weights, ..opts };
            for (i, c) in self.constraints.iter().enumerate() {
                let f_skip = |x: &[f64], out: &mut Vec<f64>| {
                    for (j, c2) in self.constraints.iter().enumerate() {
                        if i != j {
                            let _ = eval_constraint(c2, x, &layout, &self.entities, &self.parameters, out);
                        }
                    }
                };
                if solve_least_squares(&f_skip, &x0, &skip_opts).converged {
                    conflicting.push(constraint_name(c));
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

/// The display name of a constraint: its own `name` when it has one, its kind otherwise.
/// Reports (`SolveReport::redundant`, `SolveReport::conflicting`) and errors carry this, so a
/// caller — and a test — can match the very constraint the user named.
fn constraint_name(c: &SketchConstraint) -> String {
    let (name, kind): (&String, &str) = match c {
        SketchConstraint::Coincident { name, .. } => (name, "Coincident"),
        SketchConstraint::Horizontal { name, .. } => (name, "Horizontal"),
        SketchConstraint::Vertical { name, .. } => (name, "Vertical"),
        SketchConstraint::Parallel { name, .. } => (name, "Parallel"),
        SketchConstraint::Perpendicular { name, .. } => (name, "Perpendicular"),
        SketchConstraint::Tangent { name, .. } => (name, "Tangent"),
        SketchConstraint::Equal { name, .. } => (name, "Equal"),
        SketchConstraint::Distance { name, .. } => (name, "Distance"),
        SketchConstraint::Radius { name, .. } => (name, "Radius"),
        SketchConstraint::Angle { name, .. } => (name, "Angle"),
    };
    if name.is_empty() { kind.to_string() } else { name.clone() }
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
#[cfg(test)]
trait PointPos {
    fn point_pos(&self) -> Option<Vec2>;
}

#[cfg(test)]
impl PointPos for SketchEntity {
    fn point_pos(&self) -> Option<Vec2> {
        match self {
            SketchEntity::Point { pos, .. } => Some(*pos),
            _ => None,
        }
    }
}
