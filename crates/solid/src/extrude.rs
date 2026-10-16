//! Extrusion and revolution: the two ways this kernel grows a solid out of a flat profile.

use cadcraft_geom::{Region, Regions, Vec2, Vec3};

use crate::{Face, Plane, Solid, SolidError, limits::MAX_ROUND_SEGMENTS};

/// A stand-alone region carried into another plane's frame and wound to match, used for the caps of
/// an extrusion and of a revolve. See [`crate::solid::reverse_regions`] for why a carried loop may
/// need its winding restored.
///
/// The carry-over is an isometry of the plane, and an isometry that *turns the frame over* reverses
/// the sense of a loop. So the reversal is applied exactly when the two frames disagree about which
/// side is which — `from.normal · to.normal < 0` — and not otherwise. This matters in both
/// directions: a cap whose loop disagrees with its own normal is a face nothing downstream can
/// repair, because [`Solid::orient_faces`] probes the normal and never sees the loop, and reversing
/// a loop that did not need it turns a correct face inside out just as surely.
fn remap(region: &Regions, from: &Plane, to: &Plane) -> Regions {
    let carrier = Face { plane: *from, region: region.clone() };
    let carried = carrier.retargeted(*to).region;
    if from.normal.dot(to.normal) >= 0.0 { carried } else { crate::reverse_regions(&carried) }
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
/// A full turn needs no caps. A partial one does: the cap at sweep angle `t` is the profile turned
/// by `t`, carried into the profile's own plane turned by `t` (see the cap block below), and the
/// face orientation of the whole result is settled by measuring it rather than by reasoning about
/// the sweep direction.
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
    // `segments` steps, never `segments + 1`. A partial sweep's last patch runs from
    // `theta(segments - 1)` to `theta(segments)`, and `theta(segments)` is exactly the angle the far
    // cap is built at — so the loop has to stop there. Running one step further built a ninth ring of
    // patches *beyond* the requested angle, and that ring's boundary was the edge the cap never met:
    // the seam at the cap boundary. (A full turn steps `segments` times as well; `theta(segments)` is
    // a whole turn, which is the first ring again.)
    //
    // Is a point on the axis of revolution? The check above establishes that the axis runs along the
    // plane's **v**, which makes `u` the *radial* direction — so a point is on the axis exactly when
    // it has no radial component, `(w - origin) · u == 0`, and for a point of the profile plane that
    // is precisely its own `x` coordinate. Testing `v` instead would ask "does this point reach the
    // far end of the axis", which is true of every point of a profile that straddles it.
    //
    // Only a segment with *both* ends on the axis is dropped. A segment with one end there is a cone
    // apex, which is real geometry: one of its two patches is degenerate and `triangle_face` drops
    // that, and the other is the cone.
    let on_axis = |w: Vec3| -> bool { (w - origin).dot(plane.u).abs() <= 1e-12 };
    for lp in profile.loops() {
        let n = lp.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let (w0, w1) = (plane.to_world(a.x, a.y), plane.to_world(b.x, b.y));
            // A segment lying on the axis revolves to nothing.
            if on_axis(w0) && on_axis(w1) {
                continue;
            }
            for s in 0..segments {
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
        // Start and end caps.
        //
        // A cap at sweep angle `t` *is* the profile turned by `t`, so it lies in the profile's own
        // plane turned by `t`. It emphatically does **not** lie in `plane.normal × axis`: that
        // direction is perpendicular to the profile plane as well as to the axis, so the plane built
        // from it holds the axis and nothing else. Every profile point then projects onto a single
        // line of the cap plane, the cap collapses to a degenerate sliver, and its edges match none
        // of the edges the sweep built — which is what "the caps and the swept walls do not meet"
        // looked like from the outside.
        //
        // Which way the cap looks: `plane.v` runs along the axis, so `plane.normal` is
        // `plane.u × ±axis`. The material sweeps from the start cap towards the far one, so the
        // start cap looks along the turned normal and the far cap along the other one.
        let sense = if plane.v().dot(axis) >= 0.0 { 1.0 } else { -1.0 };
        for step in [0usize, segments] {
            let t = theta(step);
            let turned = turn_dir(plane.normal, axis, t);
            let cap_normal = turned * if step == 0 { sense } else { -sense };
            // `origin` is the point every rotation turns about, so it lies on the axis and
            // therefore in this cap's plane: a valid point to hang the cap's frame from.
            let cap_plane = Plane::with_u(origin, cap_normal, axis)?;
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
    // All three vertices, kept. Dropping `a` left a two-point loop, `Region::is_empty` calls a loop
    // shorter than three empty, and the patch was silently dropped — so a revolve produced no swept
    // faces at all, only its two end caps. The test that caught it is `revolve_builds_no_solid` in
    // `tests/property.rs`; this is the fix it asked for and it must not be undone.
    let region = Region::new(vec![plane.to_local(*a), plane.to_local(*b), plane.to_local(*c)]).to_regions();
    Face::new(plane, region).ok()
}

/// Rodrigues rotation about an axis through the origin.
fn rotate_about_axis(p: Vec3, axis: Vec3, theta: f64) -> Vec3 {
    let (s, c) = theta.sin_cos();
    p * c + axis.cross(p) * s + axis * (axis.dot(p) * (1.0 - c))
}

/// The same rotation applied to a *direction*.
///
/// [`revolve`]'s points go through `rot`, which subtracts the rotation centre first; a direction has
/// no position, so subtracting the centre from it is not the same rotation at all. Only the
/// un-subtracted form turns a plane normal into the normal of the turned plane.
fn turn_dir(d: Vec3, axis: Vec3, theta: f64) -> Vec3 {
    if theta <= 0.0 { d } else { rotate_about_axis(d, axis, theta) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{must, rect};

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

    // ---------- revolve ----------
    //
    // `Plane::xy` has `u = X` (radial) and `v = Y` (axial), `Plane::yz` has `u = Y` (radial) and
    // `v = Z` (axial), so in both the profile's `x` is the distance from the axis and its `y` the
    // height. A sweep by `t` about `+Y` carries a profile point `(x, y, 0)` to
    // `(x cos t, y, -x sin t)`.

    /// Area of a regular `n`-gon of circumradius `r`: what a faceted sweep of a circle leaves behind.
    fn ngon_area(n: usize, r: f64) -> f64 {
        0.5 * n as f64 * r * r * (std::f64::consts::TAU / n as f64).sin()
    }

    /// A 1x1 rectangle one unit clear of the axis: revolved whole it is a faceted tube of outer
    /// radius 2 and inner radius 1.
    fn tube_profile() -> Regions {
        rect(1.0, 0.0, 2.0, 1.0)
    }

    /// A point at radius `r`, height `h`, half way between two facet corners — where the wall is at
    /// its thinnest, so the apothem rather than the radius is what a probe has to clear.
    fn between_facets(r: f64, h: f64) -> Vec3 {
        let t = std::f64::consts::FRAC_PI_4 / 2.0;
        Vec3::new(r * t.cos(), h, -r * t.sin())
    }

    #[test]
    fn a_full_turn_revolves_a_rectangle_into_a_closed_tube() {
        // No caps at all: the last step wraps onto the first, so the shell has to close on its own.
        // Eight facets, four profile segments, two triangles per patch.
        let s = must(revolve(&tube_profile(), &Plane::xy(0.0), Vec3::Y, std::f64::consts::TAU, 8), "full turn");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        let (faces, _, _) = s.counts();
        assert_eq!(faces, 64, "four profile segments times eight steps, two triangles each");
        let expected = ngon_area(8, 2.0) - ngon_area(8, 1.0);
        assert!((s.volume() - expected).abs() < 1e-9, "volume {} expected {expected}", s.volume());
        let probe = between_facets(1.5, 0.5);
        assert!(s.contains(probe), "inside the wall at {probe:?}");
        assert!(!s.contains(Vec3::new(50.0, 0.5, 50.0)), "and nowhere near the drawing");
    }

    #[test]
    fn a_full_turn_revolves_about_the_z_axis_too() {
        // `Plane::yz` spells the same sweep with a different plane, so the axis guard and the cap
        // planes have to read the plane rather than a particular axis letter.
        let s = must(revolve(&tube_profile(), &Plane::yz(0.0), Vec3::Z, std::f64::consts::TAU, 8), "full turn about +Z");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        let (faces, _, _) = s.counts();
        assert_eq!(faces, 64, "the same 64 patches as the sweep about +Y");
        let expected = ngon_area(8, 2.0) - ngon_area(8, 1.0);
        assert!((s.volume() - expected).abs() < 1e-9, "volume {} expected {expected}", s.volume());
    }

    #[test]
    fn a_partial_turn_closes_at_both_ends() {
        // A quarter of the tube: the same swept patches, plus the two caps that close them. The caps
        // are the whole point of the test. Built as `profile_plane x axis` they used to collapse onto
        // a line of their own plane, and built one step too far round the sweep they did not meet the
        // walls at all; either way the shell came back with an edge used once.
        let s = must(revolve(&tube_profile(), &Plane::xy(0.0), Vec3::Y, std::f64::consts::FRAC_PI_2, 8), "quarter turn");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        let (faces, _, _) = s.counts();
        assert_eq!(faces, 66, "the full turn's 64 patches, plus a cap at each end — not 72, which is what stepping one step past the angle built");
        let expected = (ngon_area(8, 2.0) - ngon_area(8, 1.0)) / 4.0;
        assert!((s.volume() - expected).abs() < 1e-9, "volume {} expected {expected}", s.volume());
        let probe = between_facets(1.5, 0.5);
        assert!(s.contains(probe), "inside the swept quarter at {probe:?}");
    }

    #[test]
    fn a_partial_turns_caps_are_the_profile_in_the_planes_the_sweep_starts_and_ends_in() {
        let angle = std::f64::consts::FRAC_PI_2;
        let plane = Plane::xy(0.0);
        let axis = Vec3::Y;
        let profile = tube_profile();
        let s = must(revolve(&profile, &plane, axis, angle, 8), "quarter turn");
        s.validate().unwrap_or_else(|e| panic!("{e}"));
        // Each cap carries the whole profile, so each has the profile's area.
        let caps: Vec<&Face> = s.faces.iter().filter(|f| (f.area() - 1.0).abs() < 1e-9).collect();
        assert_eq!(caps.len(), 2, "two caps, each the 1x1 profile; every face's area was {:?}", s.faces.iter().map(|f| f.area()).collect::<Vec<_>>());
        // The start cap is the profile's own plane and the far cap is where a quarter turn puts it,
        // so the two normals are opposite: the material sweeps from one to the other.
        assert!(caps.iter().any(|f| f.plane.normal.near(Vec3::Z, 1e-12)), "the start cap looks out of the sweep");
        assert!(caps.iter().any(|f| f.plane.normal.near(Vec3::X * -1.0, 1e-12)), "and the far cap the other way");
        // Every cap holds the profile: the start one the profile itself, the far one the profile
        // turned by `angle`. A cap plane built from `profile_plane x axis` holds neither.
        for f in &caps {
            let at_start = f.plane.normal.near(Vec3::Z, 1e-12);
            for lp in profile.loops() {
                for p in lp {
                    let w = plane.to_world(p.x, p.y);
                    let w = if at_start { w } else { rotate_about_axis(w - plane.origin, axis, angle) + plane.origin };
                    assert!(f.plane.distance(w).abs() < 1e-12, "the cap with normal {:?} does not hold {w:?}", f.plane.normal);
                }
            }
        }
    }

    #[test]
    fn a_revolve_around_a_profile_that_touches_the_axis_makes_a_cone() {
        // The edge lying on the axis revolves to nothing and is dropped. A segment with only *one*
        // end on the axis is real geometry — the cone's own lateral surface — and only its degenerate
        // half is dropped, by `triangle_face`.
        let profile = Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)]).to_regions();
        let full = must(revolve(&profile, &Plane::xy(0.0), Vec3::Y, std::f64::consts::TAU, 8), "cone");
        full.validate().unwrap_or_else(|e| panic!("{e}"));
        let (faces, _, _) = full.counts();
        assert_eq!(faces, 16, "eight base wedges and eight lateral triangles; the axis edge is not a face");
        // A pyramid over the octagon the base edge sweeps: its volume is the base over three.
        assert!((full.volume() - ngon_area(8, 1.0) / 3.0).abs() < 1e-9, "a pyramid over an octagon of radius 1: {}", full.volume());
        let quarter = must(revolve(&profile, &Plane::xy(0.0), Vec3::Y, std::f64::consts::FRAC_PI_2, 8), "a slice of a cone");
        quarter.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((quarter.volume() - ngon_area(8, 1.0) / 3.0 / 4.0).abs() < 1e-9, "a quarter of the cone: {}", quarter.volume());
    }

    #[test]
    fn revolve_refuses_bad_input() {
        let p = tube_profile();
        assert!(matches!(revolve(&Regions::default(), &Plane::xy(0.0), Vec3::Y, 1.0, 8), Err(SolidError::EmptyProfile)));
        assert!(matches!(revolve(&p, &Plane::xy(0.0), Vec3::Y, 0.0, 8), Err(SolidError::BadDistance(_))));
        assert!(matches!(revolve(&p, &Plane::xy(0.0), Vec3::Y, -1.0, 8), Err(SolidError::BadDistance(_))));
        assert!(matches!(revolve(&p, &Plane::xy(0.0), Vec3::Y, f64::NAN, 8), Err(SolidError::BadDistance(_))));
        assert!(matches!(revolve(&p, &Plane::xy(0.0), Vec3::Y, f64::INFINITY, 8), Err(SolidError::BadDistance(_))));
        assert!(revolve(&p, &Plane::xy(0.0), Vec3::Y, 1.0, 0).is_err(), "no facets at all");
        assert!(revolve(&p, &Plane::xy(0.0), Vec3::Y, 1.0, MAX_ROUND_SEGMENTS + 1).is_err(), "more than the cap allows");
        assert!(revolve(&p, &Plane::xy(0.0), Vec3::ZERO, 1.0, 8).is_err(), "no axis to revolve about");
        assert!(revolve(&p, &Plane::xy(0.0), Vec3::Z, 1.0, 8).is_err(), "an axis the profile plane does not contain");
    }
}
