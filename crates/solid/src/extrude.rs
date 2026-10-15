//! Extrusion and revolution: the two ways this kernel grows a solid out of a flat profile.

use cadcraft_geom::{Region, Regions, Vec2, Vec3};

use crate::{Face, Plane, Solid, SolidError, limits::MAX_ROUND_SEGMENTS};

/// A stand-alone region carried into another plane's frame and wound to match, used for the caps
/// of an extrusion. See [`crate::solid::reverse_regions`] for why the winding has to be restored.
fn remap(region: &Regions, from: &Plane, to: &Plane) -> Regions {
    let carrier = Face { plane: *from, region: region.clone() };
    crate::reverse_regions(&carrier.retargeted(*to).region)
}

/// Extrude a planar profile along a plane's normal by `distance`.
///
/// The profile is the cross-section: every loop of it becomes a wall, and the region itself
/// becomes the two caps. Faces come out already oriented outward — the caps' loops are carried
/// through the frame change, which flips them with it — so the result satisfies the kernel's
/// invariants as built.
///
/// `distance` must be positive; extruding "backwards" is a negative distance with the normal
/// flipped, which keeps every face's convention in one place.
pub fn extrude(profile: &Regions, plane: &Plane, distance: f64) -> Result<Solid, SolidError> {
    if !distance.is_finite() || distance <= 0.0 {
        return Err(SolidError::BadDistance(distance));
    }
    if profile.is_empty() {
        return Err(SolidError::EmptyProfile);
    }
    let area = profile.area();
    if area <= 0.0 || !area.is_finite() {
        return Err(SolidError::EmptyProfile);
    }
    let mut faces: Vec<Face> = Vec::new();
    // Bottom cap: the plane turned over. The new frame is a mirror of the old one, so the profile
    // is carried across and then reversed, which leaves the loop reading counter-clockwise in the
    // cap's own frame — that is, facing out of the solid, like every other face here.
    let bottom = Plane { origin: plane.origin, normal: plane.normal * -1.0, u: plane.u * -1.0 };
    faces.push(Face::new(bottom, remap(profile, plane, &bottom))?);
    // Top cap: the same frame translated along the normal, so the local coordinates are unchanged.
    faces.push(Face::new(plane.offset(distance), profile.clone())?);
    // Walls, one per boundary segment, including the ones around holes.
    for lp in profile.loops() {
        let n = lp.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let w0 = plane.to_world(a.x, a.y);
            let w1 = plane.to_world(b.x, b.y);
            let d = w1 - w0;
            let len = d.len();
            if len <= 0.0 || !len.is_finite() {
                continue;
            }
            let dir = d / len;
            // Loops are wound so the material is on the left, so `dir x n` points out of it.
            let normal = dir.cross(plane.normal).normalized();
            if normal.len() < 0.5 {
                continue;
            }
            // With u along the edge and v along the extrusion, u x v is that outward normal, so
            // the wall loop (0,0)-(len,0)-(len,d)-(0,d) reads counter-clockwise in its frame.
            let wall = Plane::with_u(w0, normal, dir)?;
            let region = Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(len, 0.0), Vec2::new(len, distance), Vec2::new(0.0, distance)]);
            faces.push(Face::new(wall, region.to_regions())?);
        }
    }
    let solid = Solid::from_faces(faces)?;
    solid.validate()?;
    Ok(solid)
}

/// Revolve a profile around an axis, as a sweep of `segments` facets.
///
/// The profile lives in `plane`, whose `v` must run along `axis`: `u` is the radial direction and
/// `v` the axial one. The surface between two rotated copies of a profile segment is not planar,
/// so each patch is a triangle pair — this is the "faceted" contract of the kernel, stated in the
/// signature instead of hidden in the output. A profile segment that touches the axis produces a
/// cone apex, and those degenerate patches are dropped rather than emitted as zero-area faces.
///
/// A full turn needs no caps. A partial one does, and the face orientation is settled by measuring
/// the result rather than by reasoning about the sweep direction.
pub fn revolve(profile: &Regions, plane: &Plane, axis: Vec3, angle: f64, segments: usize) -> Result<Solid, SolidError> {
    let profile_area = profile.area();
    if profile.is_empty() || profile_area <= 0.0 {
        return Err(SolidError::EmptyProfile);
    }
    if !angle.is_finite() || angle <= 0.0 {
        return Err(SolidError::BadDistance(angle));
    }
    if segments == 0 || segments > MAX_ROUND_SEGMENTS {
        return Err(SolidError::Invalid(format!("{segments} segments, the limit is {MAX_ROUND_SEGMENTS}")));
    }
    let axis = axis.normalized();
    if axis.len() < 0.5 || !axis.is_finite() {
        return Err(SolidError::BadDirection);
    }
    if plane.v().cross(axis).len() > 1e-9 {
        return Err(SolidError::Invalid("the revolve plane must contain the axis".into()));
    }
    let full = angle >= std::f64::consts::TAU - 1e-9;
    let theta =
        |step: usize| -> f64 { if full { std::f64::consts::TAU * step as f64 / segments as f64 } else { angle * step as f64 / segments as f64 } };
    let origin = plane.origin;
    let rot = |p: Vec3, t: f64| -> Vec3 { if t <= 0.0 { p } else { rotate_about_axis(p - origin, axis, t) + origin } };
    let mut faces: Vec<Face> = Vec::new();
    let last = segments;
    for lp in profile.loops() {
        let n = lp.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let (w0, w1) = (plane.to_world(a.x, a.y), plane.to_world(b.x, b.y));
            // A segment on the axis revolves to nothing; it is the apex of a cone.
            if (w0 - origin).dot(plane.u).abs() <= 1e-12 && (w1 - origin).dot(plane.u).abs() <= 1e-12 {
                continue;
            }
            for s in 0..=last {
                if full && s == last {
                    break;
                }
                let (p0, p1) = (rot(w0, theta(s)), rot(w1, theta(s)));
                let (q0, q1) = (rot(w0, theta(s + 1)), rot(w1, theta(s + 1)));
                if let Some(f) = triangle_face(&p0, &p1, &q1) {
                    faces.push(f);
                }
                if let Some(f) = triangle_face(&p0, &q1, &q0) {
                    faces.push(f);
                }
            }
        }
    }
    if !full {
        // Start and end caps: the profile itself, turned to the cap's own plane.
        for step in [0usize, segments] {
            let t = theta(step);
            let cap_normal = plane.normal.cross(axis).normalized() * if step == 0 { 1.0 } else { -1.0 };
            let cap_plane = Plane::with_u(rot(plane.origin, t), cap_normal, axis)?;
            faces.push(Face::new(cap_plane, remap(profile, plane, &cap_plane))?);
        }
    }
    let mut solid = Solid::from_faces(faces)?;
    solid.orient_faces()?;
    solid.validate()?;
    Ok(solid)
}

/// A triangle as a face, or `None` when the three points are collinear.
fn triangle_face(a: &Vec3, b: &Vec3, c: &Vec3) -> Option<Face> {
    let n = (*b - *a).cross(*c - *a);
    if n.len() < 1e-15 || !n.is_finite() {
        return None;
    }
    let plane = Plane::new(*a, n).ok()?;
    // All three vertices. Dropping `a` left a two-point loop, `Region::is_empty` calls a loop
    // shorter than three empty, and the patch was silently dropped — so a revolve produced no
    // swept faces at all, only its two end caps. Caught by `revolve_builds_a_solid` in
    // `tests/property.rs`.
    let region = Region::new(vec![plane.to_local(*a), plane.to_local(*b), plane.to_local(*c)]).to_regions();
    Face::new(plane, region).ok()
}

/// Rodrigues rotation about an axis through the origin.
fn rotate_about_axis(p: Vec3, axis: Vec3, theta: f64) -> Vec3 {
    let (s, c) = theta.sin_cos();
    p * c + axis.cross(p) * s + axis * (axis.dot(p) * (1.0 - c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::must;

    fn square_region(x0: f64, y0: f64, x1: f64, y1: f64) -> Regions {
        Region::new(vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]).to_regions()
    }

    #[test]
    fn a_square_extrudes_to_a_box() {
        let s = must(extrude(&square_region(0.0, 0.0, 2.0, 3.0), &Plane::xy(0.0), 4.0), "extrude");
        let (faces, _, _) = s.counts();
        assert_eq!(faces, 6, "two caps and four walls");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((s.volume() - 24.0).abs() < 1e-9);
        assert!(s.contains(Vec3::new(1.0, 1.5, 2.0)));
        assert!(!s.contains(Vec3::new(5.0, 1.5, 2.0)));
    }

    #[test]
    fn a_profile_with_a_hole_extrudes_to_a_tube() {
        let profile = Region {
            outer: vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)],
            // Clockwise: loops are wound so the material is on their left, which is what makes the
            // walls of the hole face outwards.
            holes: vec![vec![Vec2::new(1.0, 1.0), Vec2::new(1.0, 3.0), Vec2::new(3.0, 3.0), Vec2::new(3.0, 1.0)]],
        };
        let s = must(extrude(&profile.to_regions(), &Plane::xy(0.0), 2.0), "extrude");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((s.volume() - (16.0 - 4.0) * 2.0).abs() < 1e-9, "volume {}", s.volume());
        assert!(!s.contains(Vec3::new(2.0, 2.0, 1.0)), "the hole must stay open");
        assert!(s.contains(Vec3::new(0.5, 2.0, 1.0)));
    }

    #[test]
    fn extrude_refuses_bad_input() {
        assert!(matches!(extrude(&Regions::default(), &Plane::xy(0.0), 1.0), Err(SolidError::EmptyProfile)));
        assert!(matches!(extrude(&square_region(0.0, 0.0, 1.0, 1.0), &Plane::xy(0.0), 0.0), Err(SolidError::BadDistance(_))));
        assert!(matches!(extrude(&square_region(0.0, 0.0, 1.0, 1.0), &Plane::xy(0.0), f64::NAN), Err(SolidError::BadDistance(_))));
        assert!(matches!(extrude(&square_region(0.0, 0.0, 1.0, 1.0), &Plane::xy(0.0), f64::INFINITY), Err(SolidError::BadDistance(_))));
    }
}
