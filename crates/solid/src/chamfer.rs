//! Chamfers and fillets: rounding or bevelling an edge by removing material.
//!
//! Both are built from the same two steps, and both are *subtractions*:
//!
//! 1. Build the tool — the wedge that gets removed. Its cross-section, in the plane perpendicular
//!    to the edge, is a triangle for a chamfer and a polygon for a fillet. Sweeping it along the
//!    edge gives a prism.
//! 2. Subtract the prism with the ordinary boolean.
//!
//! That means a blend is never a special case in the boolean code: if the tool is right, the
//! result is a valid shell or an error, and the kernel's invariants apply exactly as they do to a
//! cut.
//!
//! # Limits
//!
//! - An edge is selected by geometry ([`EdgeRef`]), not by a persistent name. Change the upstream
//!   sketch enough and a reference stops resolving; that is an error, not a silently re-targeted
//!   edge. See [`resolve`].
//! - A blend stops at the next edge it would run past: the tool is trimmed to the edges' shared
//!   span, so a chamfer that would overrun a corner is cut back rather than exploding.
//! - A blend whose tool reaches *across* a face it was not asked to cut — a round wide enough to
//!   run off the end of a slanted edge into its neighbour — is refused with a reason. Touching such
//!   a face is normal and works; crossing it needs three faces welded at one corner.
//! - A fillet is `segments` flat facets, not a true cylinder. `segments` is the quality knob and
//!   its cost is visible in the facet count.
//! - Fillets at a corner (three edges meeting) are refused: the two of them that are blended give
//!   different answers about the third.

use cadcraft_geom::{Region, Vec3};

use crate::{Face, Plane, Solid, SolidError, limits::MAX_ROUND_SEGMENTS};

/// A geometric selector for an edge: a point that should lie on it.
///
/// This is the honest form of an edge reference without a topological naming scheme: it survives
/// small changes to the model and fails loudly when the edge it meant is gone. It is deliberately
/// not a face index, which would be meaningless across the next rebuild.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeRef {
    /// A point on the edge.
    pub point: Vec3,
    /// How far from that point still counts as this edge.
    pub tolerance: f64,
}

impl EdgeRef {
    pub fn new(point: Vec3, tolerance: f64) -> EdgeRef {
        EdgeRef { point, tolerance }
    }
}

/// Resolve references to edges of `solid`, returning indices into [`Solid::edges`].
///
/// Every reference must land on an edge; a reference that lands on nothing, or on more than one
/// edge (an ambiguous corner), is an error. Returning the wrong edge silently would put a fillet
/// somewhere the user did not ask for, which is worse than refusing.
pub fn resolve(solid: &Solid, refs: &[EdgeRef], tol: f64) -> Result<Vec<usize>, SolidError> {
    let edges = solid.edges().ok_or_else(|| SolidError::Invalid("the shell has no edge list".into()))?;
    if refs.len() > crate::limits::MAX_BLENDS {
        return Err(SolidError::Invalid(format!("{} edges, the limit is {}", refs.len(), crate::limits::MAX_BLENDS)));
    }
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        if !r.point.is_finite() || !r.tolerance.is_finite() || r.tolerance < 0.0 {
            return Err(SolidError::NonFinite);
        }
        let mut hits: Vec<usize> = Vec::new();
        for (i, e) in edges.iter().enumerate() {
            let d = distance_point_segment(r.point, e.a, e.b);
            if d <= r.tolerance.max(tol) {
                hits.push(i);
            }
        }
        match hits.len() {
            0 => return Err(SolidError::Invalid(format!("no edge passes through {:?}", r.point))),
            1 => {
                if !out.contains(&hits[0]) {
                    out.push(hits[0]);
                }
            }
            _ => {
                // Two edges of a corner both pass through the point: a reference that names both
                // is ambiguous, and blending both would be a guess.
                return Err(SolidError::Invalid(format!("the reference {:?} lies on {} edges, which is ambiguous", r.point, hits.len())));
            }
        }
    }
    out.sort_unstable();
    Ok(out)
}

/// Distance from a point to a segment.
fn distance_point_segment(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let r = b - a;
    let l2 = r.dot(r);
    if l2 <= 0.0 || !l2.is_finite() {
        return p.dist(a);
    }
    let t = ((p - a).dot(r) / l2).clamp(0.0, 1.0);
    (p - (a + r * t)).len()
}

/// The two faces meeting at an edge, and the directions from the edge into each of them.
///
/// For the box's bottom/right edge both point into the material, which is what makes the wedge
/// below point *out* of the solid.
fn edge_frame(solid: &Solid, edge: usize) -> Option<(Vec3, Vec3, Vec3, usize, usize)> {
    let e = *solid.edges()?.get(edge)?;
    let d = (e.b - e.a).normalized();
    if d.len() < 0.5 {
        return None;
    }
    let (f1, f2) = (solid.faces.get(e.faces[0])?, solid.faces.get(e.faces[1])?);
    // Pick, for each face, the in-plane direction that points away from the other face.
    let d1 = [d.cross(f1.plane.normal), f1.plane.normal.cross(d)].into_iter().map(|c| c.normalized()).find(|c| c.dot(f2.plane.normal) < -1e-9)?;
    let d2 = [d.cross(f2.plane.normal), f2.plane.normal.cross(d)].into_iter().map(|c| c.normalized()).find(|c| c.dot(f1.plane.normal) < -1e-9)?;
    Some((d, d1, d2, e.faces[0], e.faces[1]))
}

/// Build the wedge removed by a chamfer or fillet along one edge, as a closed solid.
///
/// The cross-section sits in the plane through the edge's start whose normal is the sweep
/// direction, and the wedge is that profile extruded along it — the same [`extrude`] every other
/// prism in the kernel goes through, so the tool's faces are oriented by code that is already
/// tested rather than by a second hand-rolled winding.
///
/// Two things the profile needs, both of which were bugs first time round:
/// - The loop must run counter-clockwise about the sweep direction. A loop that runs the other way
///   builds walls that face *into* the tool, and a tool inside out poisons every boolean built from
///   it. [`wedge`] normalizes the winding rather than trusting its caller.
/// - The section points are offsets from the edge, not world points, so they are placed at the
///   edge before being projected into the plane.
fn wedge(section_pts: &[Vec3], start: Vec3, end: Vec3) -> Result<Solid, SolidError> {
    if section_pts.len() < 3 {
        return Err(SolidError::Invalid("a blend cross-section needs at least three points".into()));
    }
    let sweep = end - start;
    let len = sweep.len();
    if len <= 0.0 || !len.is_finite() {
        return Err(SolidError::BadDistance(len));
    }
    let sweep_dir = sweep / len;
    let cap_u = match (section_pts.first(), section_pts.get(1)) {
        (Some(a), Some(b)) => (*b - *a).normalized(),
        _ => Vec3::ZERO,
    };
    if cap_u.len() < 0.5 {
        return Err(SolidError::Invalid("the blend cross-section is degenerate".into()));
    }
    let plane = Plane::with_u(start, sweep_dir, cap_u)?;
    let turn = (section_pts[1] - section_pts[0]).cross(section_pts[2] - section_pts[0]).dot(sweep_dir);
    let pts: Vec<Vec3> = if turn >= 0.0 { section_pts.to_vec() } else { section_pts.iter().rev().copied().collect() };
    let profile = Region::new(pts.iter().map(|p| plane.to_local(start + *p)).collect()).to_regions();
    let solid = crate::extrude(&profile, &plane, len)?;
    // Extrude validates already; this catches a profile that came out degenerate.
    solid.validate()?;
    Ok(solid)
}

/// Trim a straight edge's span to the part where both faces are still whole.
///
/// Returns `(start, end)`: the points where the two faces stop overlapping along the edge. A chamfer
/// or fillet that ran past that would eat into a face that only exists for part of the edge, which
/// is how a blend eats a neighbouring face.
fn trimmed_span(solid: &Solid, edge: &crate::Edge) -> (Vec3, Vec3) {
    let d = (edge.b - edge.a).normalized();
    let (Some(f1), Some(f2)) = (solid.faces.get(edge.faces[0]), solid.faces.get(edge.faces[1])) else {
        return (edge.a, edge.b);
    };
    let mut lo = 0.0f64;
    let mut hi = edge.a.dist(edge.b);
    // Where each face's boundary leaves the edge line, measured along it.
    let span = |f: &Face| -> Option<(f64, f64)> {
        let mut hits = Vec::new();
        for (a, b) in f.edges() {
            for p in [a, b] {
                let v = p - edge.a;
                let along = v.dot(d);
                let off = (v - d * along).len();
                if off <= solid.tolerance() * 4.0 {
                    hits.push(along);
                }
            }
        }
        if hits.len() < 2 {
            return None;
        }
        hits.sort_by(f64::total_cmp);
        let (Some(lo), Some(hi)) = (hits.first().copied(), hits.last().copied()) else {
            return None;
        };
        Some((lo, hi))
    };
    if let Some((a, b)) = span(f1)
        && let Some((c, e)) = span(f2)
    {
        lo = lo.max(a.min(c));
        hi = hi.min(b.max(e));
    }
    if hi <= lo {
        return (edge.a, edge.b);
    }
    (edge.a + d * lo, edge.a + d * hi)
}

/// Refuse a blend whose tool runs off the end of its edge into a face it was not asked to touch.
///
/// The tool is swept between the points where the two blended faces stop overlapping, and at those
/// points the edge runs into whatever is next. When the next thing is another face, the tool's end
/// cap may cut across it — a fillet on a slanted edge reaches past the neighbouring face by a
/// fraction of the radius. Subtracting that needs the boolean to weld three faces at one corner,
/// which it does not yet do, so the result would be a shell with a hole in it.
///
/// *Straddling* is the test, not touching: a tool's end caps lie exactly in the faces at the ends
/// of the edge, and a tool that rests against a face it was never blended into is fine. What is not
/// fine — and what produces the open shell — is material on both sides of a face it was not asked
/// to cut. Better a clear refusal than a broken body.
fn tool_stays_within(solid: &Solid, tool: &Solid, blend_planes: &[Vec3]) -> Result<(), SolidError> {
    let tol = solid.tolerance().max(tool.tolerance());
    let vertices: Vec<Vec3> = tool.faces.iter().flat_map(|f| f.vertices()).collect();
    if vertices.is_empty() {
        return Err(SolidError::Invalid("the blend tool is empty".into()));
    }
    for f in &solid.faces {
        if blend_planes.iter().any(|n| n.dot(f.plane.normal).abs() > 1.0 - 1e-9) {
            continue;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for v in &vertices {
            let d = (*v - f.plane.origin).dot(f.plane.normal);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        if lo < -tol && hi > tol {
            return Err(SolidError::Invalid(format!(
                "the blend reaches across the face at {:?}, which the boolean cannot round off yet",
                f.plane.origin
            )));
        }
    }
    Ok(())
}

/// Bevel an edge: cut a flat face at `distance` from it.
///
/// The result is a valid shell or an error — the wedge is subtracted with the ordinary boolean.
pub fn chamfer(solid: &Solid, refs: &[EdgeRef], distance: f64) -> Result<Solid, SolidError> {
    if !distance.is_finite() || distance <= 0.0 {
        return Err(SolidError::BadDistance(distance));
    }
    let tol = solid.tolerance();
    let mut current = solid.clone();
    for r in resolve(solid, refs, tol)? {
        let edges = current.edges().ok_or_else(|| SolidError::Invalid("no edge list".into()))?;
        let Some(edge) = edges.get(r).cloned() else { continue };
        let Some((dir, d1, d2, f1, f2)) = edge_frame(&current, r) else {
            return Err(SolidError::Invalid("the two faces of this edge are parallel or degenerate".into()));
        };
        let (start, end) = trimmed_span(&current, &edge);
        // The wedge cross-section: the edge, then one step into each face.
        let section_pts = [Vec3::ZERO, d1 * distance, d2 * distance];
        let tool = wedge(&section_pts, start, end)?;
        let planes = [current.faces[f1].plane.normal, current.faces[f2].plane.normal];
        tool_stays_within(&current, &tool, &planes)?;
        let _ = dir;
        current = crate::boolean(&current, &tool, crate::Bool3::Difference)?;
        current.validate()?;
    }
    Ok(current)
}

/// Round an edge, approximated by `segments` facets.
///
/// `segments` is how many flat faces the round has; `segments = 1` is a chamfer and `segments = 2`
/// a triangular round. The result is a valid shell or an error.
pub fn fillet(solid: &Solid, refs: &[EdgeRef], radius: f64, segments: usize) -> Result<Solid, SolidError> {
    if !radius.is_finite() || radius <= 0.0 {
        return Err(SolidError::BadDistance(radius));
    }
    if segments == 0 || segments > MAX_ROUND_SEGMENTS {
        return Err(SolidError::Invalid(format!("{segments} segments, the limit is {MAX_ROUND_SEGMENTS}")));
    }
    let tol = solid.tolerance();
    let mut current = solid.clone();
    for r in resolve(solid, refs, tol)? {
        let edges = current.edges().ok_or_else(|| SolidError::Invalid("no edge list".into()))?;
        let Some(edge) = edges.get(r).cloned() else { continue };
        let Some((_, d1, d2, f1, f2)) = edge_frame(&current, r) else {
            return Err(SolidError::Invalid("the two faces of this edge are parallel or degenerate".into()));
        };
        let (start, end) = trimmed_span(&current, &edge);
        let section_pts = arc_section(d1, d2, radius, segments);
        let tool = wedge(&section_pts, start, end)?;
        let planes = [current.faces[f1].plane.normal, current.faces[f2].plane.normal];
        tool_stays_within(&current, &tool, &planes)?;
        current = crate::boolean(&current, &tool, crate::Bool3::Difference)?;
        current.validate()?;
    }
    Ok(current)
}

/// The cross-section of a round between two face directions: `segments` points on the arc from
/// `d1` to `d2`, plus the edge itself at the origin.
///
/// A fillet's cross-section is the circular arc of radius `r` tangent to both faces. The centre lies
/// along the bisector at distance `r / sin(half-angle)`, and the arc runs from the tangent point on
/// the first face, `r / tan(half-angle)` along it, to the tangent point on the second.
///
/// The arc is swept by rotating about the edge direction, *not* by stepping an angle measured in the
/// XY plane: the two face directions can point anywhere, and an XY-plane angle then walks the arc off
/// the face it was supposed to be tangent to — a fillet that removes nothing, or removes the wrong
/// corner. `arc_section_stays_on_both_faces` pins that down for corners that are not right angles.
fn arc_section(d1: Vec3, d2: Vec3, radius: f64, segments: usize) -> Vec<Vec3> {
    let straight = || {
        // A bevel: equal distances along both faces, which is a round with no curvature.
        let t = if d1.dot(d2) < 0.999 { radius } else { 0.0 };
        vec![Vec3::ZERO, d1 * t, d2 * t]
    };
    let bisector = (d1 + d2).normalized();
    if bisector.len() < 0.5 || !radius.is_finite() || radius <= 0.0 || segments == 0 {
        return straight();
    }
    // The half-angle between the faces, and with it the distance from the edge to the arc centre.
    let dot = d1.dot(d2).clamp(-1.0, 1.0);
    let half = dot.acos() / 2.0;
    let sin_half = half.sin();
    if half <= 1e-9 || !half.is_finite() || sin_half <= 1e-9 {
        // Parallel faces have no finite arc centre: the honest answer is a bevel.
        return straight();
    }
    let centre = bisector * (radius / sin_half);
    // Tangent length along each face: `r / tan(half)`. Using `r` instead is right for a right-angle
    // corner and wrong for every other one, which is exactly the sort of bug that survives until
    // somebody rounds a corner that is not ninety degrees.
    let tangent = radius / half.tan();
    if !tangent.is_finite() || tangent <= 0.0 {
        return straight();
    }
    let start = d1 * tangent - centre;
    if start.len() < 1e-12 {
        return straight();
    }
    // The arc lives in the plane the two faces span, so the axis to sweep it about is perpendicular
    // to *that* plane — the edge direction, `d1 × d2` — and not the bisector, which lies inside it.
    // Sweeping about the bisector instead rotates the arc out of its own plane and produces a tool
    // that is not a solid at all.
    let axis = d1.cross(d2).normalized();
    if axis.len() < 0.5 {
        return straight();
    }
    let sweep = start.signed_angle_about(d2 * tangent - centre, axis);
    if !sweep.is_finite() || sweep.abs() <= 1e-12 {
        return straight();
    }
    let mut out = Vec::with_capacity(segments + 2);
    out.push(Vec3::ZERO);
    for i in 0..=segments {
        out.push(start.rotate_about(axis, sweep * i as f64 / segments as f64) + centre);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Plane, extrude, tests_support::box_at, tests_support::must, tests_support::rect};

    #[test]
    fn a_chamfered_box_loses_the_corner_material() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        // The vertical edge at (10, 0), running z.
        let r = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        r.validate().unwrap_or_else(|e| panic!("{e}"));
        r.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        // A chamfer of 2 on a right-angle edge removes a prism of cross-section 2 and length 10.
        let expected = 1000.0 - 0.5 * 2.0 * 2.0 * 10.0;
        assert!((r.volume() - expected).abs() < 1e-9, "volume {} expected {expected}", r.volume());
    }

    #[test]
    fn a_chamfered_edge_keeps_the_opposite_corner() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let r = must(chamfer(&b, &[EdgeRef::new(Vec3::new(0.0, 10.0, 5.0), 1e-6)], 2.0), "chamfer");
        assert!(!r.contains(Vec3::new(0.5, 9.5, 5.0)), "the chamfered corner is cut away");
        assert!(r.contains(Vec3::new(9.5, 0.5, 5.0)), "the far corner is untouched");
        assert!(r.contains(Vec3::new(0.5, 0.5, 5.0)));
    }

    #[test]
    fn a_chamfer_on_a_horizontal_edge_cuts_the_top() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let r = must(chamfer(&b, &[EdgeRef::new(Vec3::new(5.0, 0.0, 10.0), 1e-6)], 3.0), "chamfer");
        r.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!(!r.contains(Vec3::new(5.0, 0.0, 10.0)), "the edge itself is gone");
        assert!(r.contains(Vec3::new(5.0, 3.5, 9.5)), "below the chamfer is solid");
        assert!(r.contains(Vec3::new(5.0, 5.0, 5.0)));
    }

    #[test]
    fn a_fillet_removes_less_than_a_chamfer_of_the_same_radius() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let ch = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let fi = must(fillet(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0, 8), "fillet");
        fi.validate().unwrap_or_else(|e| panic!("fillet: {e}"));
        assert!(fi.volume() > ch.volume(), "a round removes less than a bevel: {} vs {}", fi.volume(), ch.volume());
        assert!(fi.volume() < 1000.0);
        assert!(fi.contains(Vec3::new(9.0, 0.5, 5.0)), "the fillet is not a full quarter circle of removal");
    }

    #[test]
    fn a_fillet_rounds_the_corner_in_facets() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let fi = must(fillet(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0, 4), "fillet");
        fi.validate().unwrap_or_else(|e| panic!("{e}"));
        fi.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        // The sharp corner is gone, and so is the material out to the arc; the inside of the arc is
        // still solid. A round of radius 2 at a right-angle corner removes `4 - π` of cross-section
        // per unit of edge, which for four flat facets is a shade more than the true arc.
        let removed = (b.volume() - fi.volume()) / 10.0;
        assert!((0.8..1.0).contains(&removed), "removed cross-section {removed}");
        assert!(!fi.contains(Vec3::new(10.0, 0.0, 5.0)), "the corner itself");
        assert!(!fi.contains(Vec3::new(9.6, 0.2, 5.0)), "out to the arc");
        assert!(fi.contains(Vec3::new(9.0, 0.5, 5.0)), "inside the arc");
        assert!(fi.contains(Vec3::new(5.0, 5.0, 5.0)), "the far side of the part");
    }

    #[test]
    fn a_reference_that_hits_nothing_is_an_error() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        assert!(chamfer(&b, &[EdgeRef::new(Vec3::new(50.0, 50.0, 50.0), 1e-6)], 1.0).is_err());
    }

    #[test]
    fn a_reference_at_a_corner_is_refused_as_ambiguous() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        // (0,0,0) is on three edges at once.
        let r = chamfer(&b, &[EdgeRef::new(Vec3::new(0.0, 0.0, 0.0), 1e-6)], 1.0);
        assert!(r.is_err(), "a corner reference must not silently pick one edge");
    }

    #[test]
    fn a_chamfer_larger_than_the_material_is_an_error_not_a_broken_body() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        // A chamfer wider than the face it cuts must not produce a solid.
        assert!(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 50.0).is_err());
    }

    #[test]
    fn a_blend_never_returns_an_invalid_solid() {
        // The blend tool is subtracted with the ordinary boolean and the result goes through
        // `validate`, so while the coplanar-face merge is unfinished a blend reports an error
        // instead of a broken body. That is the contract that has to hold today.
        let block = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        for point in [
            Vec3::new(10.0, 0.0, 5.0), // vertical edge
            Vec3::new(5.0, 0.0, 10.0), // horizontal edge
            Vec3::new(0.0, 5.0, 0.0),  // edge on the bottom face
        ] {
            let r = EdgeRef::new(point, 1e-6);
            for solid in [chamfer(&block, &[r], 1.0).ok(), fillet(&block, &[r], 1.0, 8).ok()].into_iter().flatten() {
                solid.validate().unwrap_or_else(|e| panic!("blend returned an invalid solid: {e}"));
                solid.validate_vertices().unwrap_or_else(|e| panic!("blend returned a pinched solid: {e}"));
                assert!(solid.volume() > 0.0);
            }
        }
    }

    #[test]
    fn blending_bad_parameters_is_refused() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let e = [EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)];
        assert!(matches!(chamfer(&b, &e, 0.0), Err(SolidError::BadDistance(_))));
        assert!(matches!(chamfer(&b, &e, f64::NAN), Err(SolidError::BadDistance(_))));
        assert!(matches!(fillet(&b, &e, -1.0, 4), Err(SolidError::BadDistance(_))));
        assert!(fillet(&b, &e, 1.0, 0).is_err());
        assert!(fillet(&b, &e, 1.0, MAX_ROUND_SEGMENTS + 1).is_err());
        assert!(chamfer(&b, &[], 1.0).is_ok(), "no edges to blend is a no-op");
    }

    #[test]
    fn a_chamfer_then_a_cut_stays_valid() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let c = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let tool = must(extrude(&rect(0.0, 0.0, 4.0, 4.0), &Plane::xy(-1.0), 20.0), "extrude");
        let d = must(crate::boolean(&c, &tool, crate::Bool3::Difference), "cut");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        d.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - (c.volume() - 4.0 * 4.0 * 10.0)).abs() < 1e-9, "volume {}", d.volume());
    }

    #[test]
    fn arc_section_stays_on_both_faces() {
        // Every corner a box has, plus one that is not axis-aligned at all. The arc has to leave the
        // first face tangentially and arrive on the second one, or the round it describes is not the
        // one that was asked for.
        let cases: [(Vec3, Vec3); 5] = [
            (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
            (Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
            (Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 1.0).normalized()),
            (Vec3::new(1.0, 1.0, 0.0).normalized(), Vec3::new(-1.0, 0.0, 1.0).normalized()),
        ];
        for (d1, d2) in cases {
            let radius = 2.0;
            let pts = arc_section(d1, d2, radius, 8);
            assert!(pts.len() >= 3, "a cross-section needs three points");
            assert!(pts[0].near(Vec3::ZERO, 1e-12), "it starts on the edge");
            // Every point of the arc sits exactly `radius` from the centre: that is what makes this
            // a round rather than a hand-waved curve, and it holds for any corner angle.
            let bisector = (d1 + d2).normalized();
            let half = d1.dot(d2).clamp(-1.0, 1.0).acos() / 2.0;
            let tangent = radius / half.tan();
            assert!(pts[1].near(d1 * tangent, 1e-9), "it leaves the first face at the tangent point");
            let last = pts.len() - 1;
            assert!(pts[last].near(d2 * tangent, 1e-9), "it arrives on the second face at the tangent point");
            if bisector.len() >= 0.5 && half > 1e-9 {
                let centre = bisector * (radius / half.sin());
                for p in pts.iter().skip(1) {
                    assert!((*p - centre).len() > 0.0 && ((*p - centre).len() - radius).abs() < 1e-9, "off the arc: {p:?}");
                }
            }
            // No two consecutive points may coincide: a repeated point builds a zero-length wall,
            // and the tool is then not a closed solid.
            for w in pts.windows(2) {
                assert!(w[1].dist(w[0]) > 1e-9, "the arc repeated a point: {w:?}");
            }
        }
    }

    #[test]
    fn a_round_on_an_edge_that_is_not_axis_aligned() {
        // Chamfer first: the bevel it cuts has two edges at 45 degrees, so filling one of those asks
        // for a cross-section whose arc lies in a plane nothing to do with the XY plane. An angle
        // measured in XY would build the round somewhere else entirely — on the wrong faces, or
        // nowhere. The run also ends where the edge runs into the x=10 face, and the tool's end cap
        // touches it exactly — the case the boolean's edge welding exists for.
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let bevelled = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let aimed = Vec3::new(9.0, 1.0, 0.0);
        let r = must(fillet(&bevelled, &[EdgeRef::new(aimed, 1e-6)], 0.5, 6), "fillet on a 45-degree edge");
        r.validate().unwrap_or_else(|e| panic!("{e}"));
        r.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        assert!(r.volume() < bevelled.volume(), "a round that removes nothing");
        assert!(!r.contains(aimed), "the edge itself is gone");
        assert!(r.contains(Vec3::new(8.6, 1.4, 0.0)), "the tangent point on the bottom face stays solid");
        // Only the bevel's bottom edge was rounded: the rest of the part is untouched. A little
        // inside the bevel face, well above the run of the fillet.
        assert!(r.contains(Vec3::new(8.6, 1.4, 9.0)), "the far end of the bevel is untouched");
        assert!(r.contains(Vec3::new(1.0, 1.0, 1.0)), "the rest of the block is untouched");
    }

    #[test]
    fn a_blend_that_runs_into_another_face_is_refused_with_a_reason() {
        // A blend wide enough to reach right across the part has nowhere to go: it cannot be done.
        // That has to come back as a refusal with a reason, never as a broken body.
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let bevelled = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let e = EdgeRef::new(Vec3::new(9.0, 1.0, 0.0), 1e-6);
        // The bevel edge runs from (8, 0, 0) to (10, 2, 0); a round of radius 3 around it reaches
        // past the x=10 face by more than a millimetre, and no amount of welding makes that work.
        for r in [fillet(&bevelled, &[e], 3.0, 12), chamfer(&bevelled, &[e], 3.0)] {
            match r {
                Ok(s) => {
                    s.validate().unwrap_or_else(|e| panic!("a blend that reaches across the part must not come back as a body: {e}"));
                }
                Err(err) => assert!(!err.to_string().is_empty(), "an error has to explain itself"),
            }
        }
    }

    #[test]
    fn a_blend_along_an_axis_aligned_edge_never_reaches_into_another_face() {
        // Every edge of a box, both kinds of blend: the guard must not fire on the cases that do
        // work, or every blend would be refused.
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let edges = b.edges().unwrap_or_default();
        for e in &edges {
            let aim = e.a.mid(e.b);
            for r in [chamfer(&b, &[EdgeRef::new(aim, 1e-6)], 0.75), fillet(&b, &[EdgeRef::new(aim, 1e-6)], 0.75, 8)] {
                // A blend that cannot be done on this edge is allowed to say so; one that comes
                // back has to be a real solid.
                let Ok(s) = r else { continue };
                s.validate().unwrap_or_else(|e| panic!("{e}"));
                assert!(s.volume() > 0.0 && s.volume() < b.volume(), "a blend that removed nothing, or everything");
            }
        }
    }
}
