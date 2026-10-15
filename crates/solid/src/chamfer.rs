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
//! # Status
//!
//! **Not yet correct.** The tool is built and subtracted, and the result goes through the kernel's
//! full validation, so a blend that cannot be done is an error rather than a broken body — but the
//! boolean does not yet merge a blend tool's side face with the face of the target it lies *in* the
//! plane of, so in practice every chamfer and fillet is currently refused. The tests that state the
//! correct volumes are `#[ignore]`d and run with `cargo test -p cadcraft-solid -- --ignored`.
//!
//! # Limits
//!
//! - An edge is selected by geometry ([`EdgeRef`]), not by a persistent name. Change the upstream
//!   sketch enough and a reference stops resolving; that is an error, not a silently re-targeted
//!   edge. See [`resolve`].
//! - A blend stops at the next edge it would run past: the tool is trimmed to the edges' shared
//!   span, so a chamfer that would overrun a corner is cut back rather than exploding. Fillets at
//!   a corner (three edges meeting) are **not** built yet and are refused.
//! - A fillet is `segments` flat facets, not a true cylinder. `segments` is the quality knob and
//!   its cost is visible in the facet count.

use cadcraft_geom::{Region, Regions, Vec3};

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
/// The cross-section sits in the plane through the edge's first point whose normal is the edge
/// direction; the wedge is then swept along the edge. `section` is a list of points around the
/// cross-section's outline, `start`/`end` are points on the edge.
fn wedge(section_pts: &[Vec3], start: Vec3, end: Vec3) -> Result<Solid, SolidError> {
    let m = section_pts.len();
    if m < 3 {
        return Err(SolidError::Invalid("a blend cross-section needs at least three points".into()));
    }
    // Start cap: the cross-section, ordered so its normal opposes the sweep.
    let cap_normal = (section_pts[1] - section_pts[0]).cross(section_pts[2] - section_pts[0]).normalized();
    // The cap's in-plane axis is the cross-section's first edge, *not* the sweep direction: the
    // sweep is parallel to the cap's normal, so asking for it as `u` is asking for a plane with no
    // in-plane axis at all.
    let cap_u = match (section_pts.first(), section_pts.get(1)) {
        (Some(a), Some(b)) => (*b - *a).normalized(),
        _ => Vec3::X,
    };
    if cap_u.len() < 0.5 {
        return Err(SolidError::Invalid("the blend cross-section is degenerate".into()));
    }
    let start_plane = Plane::with_u(start, cap_normal, cap_u)?;
    // The cross-section points are offsets from the edge, so they only become plane coordinates
    // once placed at the cap. Reading them as world points puts the whole tool off its own edge.
    let start_region = Regions::new(vec![Region::new(section_pts.iter().map(|p| start_plane.to_local(start + *p)).collect())]);
    let end_plane = Plane::with_u(end, cap_normal * -1.0, cap_u)?;
    let end_region = Regions::new(vec![Region::new(section_pts.iter().map(|p| end_plane.to_local(end + *p)).collect())]);
    let sweep = end - start;
    let len = sweep.len();
    if len <= 0.0 || !len.is_finite() {
        return Err(SolidError::BadDistance(len));
    }
    let mut faces = vec![Face::new(start_plane, start_region)?, Face::new(end_plane, end_region)?];
    for i in 0..m {
        let (Some(p0), Some(p1)) = (section_pts.get(i), section_pts.get((i + 1) % m)) else { continue };
        let (a, b) = (start + *p0, start + *p1);
        let (c, d) = (end + *p1, end + *p0);
        // Four points, two triangles, each its own plane: this is the faceted contract again.
        let normal = (b - a).cross(c - a);
        if normal.len() < 1e-15 {
            continue;
        }
        let plane = Plane::with_u(a, normal, b - a)?;
        for tri in [[a, b, c], [a, c, d]] {
            let poly = [plane.to_local(tri[0]), plane.to_local(tri[1]), plane.to_local(tri[2])];
            if poly.iter().all(|p| p.is_finite()) {
                faces.push(Face::new(plane, Region::new(poly.to_vec()).to_regions())?);
            }
        }
    }
    Solid::from_faces(faces)
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
        let Some((dir, d1, d2, _, _)) = edge_frame(&current, r) else {
            return Err(SolidError::Invalid("the two faces of this edge are parallel or degenerate".into()));
        };
        let (start, end) = trimmed_span(&current, &edge);
        // The wedge cross-section: the edge, then one step into each face.
        let section_pts = [Vec3::ZERO, d1 * distance, d2 * distance];
        let tool = wedge(&section_pts, start, end)?;
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
        let Some((_, d1, d2, _, _)) = edge_frame(&current, r) else {
            return Err(SolidError::Invalid("the two faces of this edge are parallel or degenerate".into()));
        };
        let (start, end) = trimmed_span(&current, &edge);
        let section_pts = arc_section(d1, d2, radius, segments);
        let tool = wedge(&section_pts, start, end)?;
        current = crate::boolean(&current, &tool, crate::Bool3::Difference)?;
        current.validate()?;
    }
    Ok(current)
}

/// The cross-section of a round between two face directions: `segments` points on the arc from
/// `d1` to `d2`, plus the edge itself at the origin.
///
/// A fillet's cross-section is the circular arc of radius `r` tangent to both faces. The centre
/// lies along the bisector at distance `r / sin(half-angle)`; the arc runs from the tangent point
/// on the first face to the tangent point on the second.
fn arc_section(d1: Vec3, d2: Vec3, radius: f64, segments: usize) -> Vec<Vec3> {
    let bisector = (d1 + d2).normalized();
    if bisector.len() < 0.5 {
        return vec![Vec3::ZERO, d1 * radius, d2 * radius];
    }
    // Distance from the edge to the arc centre.
    let half = d1.angle_to(d2) / 2.0;
    if half <= 1e-9 || !half.is_finite() {
        return vec![Vec3::ZERO, d1 * radius, d2 * radius];
    }
    let centre_dist = radius / half.sin();
    let centre = bisector * centre_dist;
    // Arc from the tangent point on d1 to the one on d2, about `centre`.
    let a0 = centre.angle_to(d1 * radius);
    let a1 = centre.angle_to(d2 * radius);
    let mut sweep = a1 - a0;
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let mut out = Vec::with_capacity(segments + 1);
    out.push(Vec3::ZERO);
    for i in 0..=segments {
        let t = a0 + sweep * i as f64 / segments as f64;
        out.push(Vec3::from_angle(t) * radius + centre);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Plane, extrude, tests_support::box_at, tests_support::must, tests_support::rect};

    #[test]
    #[ignore = "pending: the boolean does not yet merge a blend tool's face with the coplanar face it sits on"]
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
    #[ignore = "pending: see a_chamfered_box_loses_the_corner_material"]
    fn a_chamfered_edge_keeps_the_opposite_corner() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let r = must(chamfer(&b, &[EdgeRef::new(Vec3::new(0.0, 10.0, 5.0), 1e-6)], 2.0), "chamfer");
        assert!(r.contains(Vec3::new(0.5, 9.5, 5.0)), "the chamfered corner is cut");
        assert!(r.contains(Vec3::new(9.5, 0.5, 5.0)), "the far corner is untouched");
        assert!(r.contains(Vec3::new(0.5, 0.5, 5.0)));
    }

    #[test]
    #[ignore = "pending: see a_chamfered_box_loses_the_corner_material"]
    fn a_chamfer_on_a_horizontal_edge_cuts_the_top() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let r = must(chamfer(&b, &[EdgeRef::new(Vec3::new(5.0, 0.0, 10.0), 1e-6)], 3.0), "chamfer");
        r.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!(!r.contains(Vec3::new(5.0, 0.0, 10.0)), "the edge itself is gone");
        assert!(r.contains(Vec3::new(5.0, 3.5, 9.5)), "below the chamfer is solid");
        assert!(r.contains(Vec3::new(5.0, 5.0, 5.0)));
    }

    #[test]
    #[ignore = "pending: see a_chamfered_box_loses_the_corner_material"]
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
    #[ignore = "pending: see a_chamfered_box_loses_the_corner_material"]
    fn a_fillet_rounds_the_corner_in_facets() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let fi = must(fillet(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0, 4), "fillet");
        fi.validate().unwrap_or_else(|e| panic!("{e}"));
        fi.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        // The tangent point on each face must still be on the solid, the corner must not be.
        assert!(!fi.contains(Vec3::new(10.0, 0.0, 5.0)));
        assert!(fi.contains(Vec3::new(8.0, 0.0, 5.0)));
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
    #[ignore = "pending: see a_chamfered_box_loses_the_corner_material"]
    fn a_chamfer_then_a_cut_stays_valid() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let c = must(chamfer(&b, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let tool = must(extrude(&rect(0.0, 0.0, 4.0, 4.0), &Plane::xy(-1.0), 20.0), "extrude");
        let d = must(crate::boolean(&c, &tool, crate::Bool3::Difference), "cut");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        d.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - (c.volume() - 4.0 * 4.0 * 10.0)).abs() < 1e-9, "volume {}", d.volume());
    }
}
