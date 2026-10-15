//! The cross-section of a solid by a plane.
//!
//! This is the step that makes 3D booleans tractable for planar-faced solids: once a plane cuts a
//! solid, everything else happens in 2D. It is also the step with the most ways to go subtly
//! wrong, so the algorithm is spelled out.
//!
//! 1. A face of the solid either lies **in** the plane, is **parallel** to it (and contributes
//!    nothing), or **crosses** it.
//! 2. A coplanar face contributes its own loops to the section boundary. Its normal direction is
//!    irrelevant: a face of the solid bounds material whatever way it points.
//! 3. A crossing face contributes segments: the line where the two planes meet, clipped to the
//!    face's own region. Clipping is done by even-odd pairing of the line's crossings with the
//!    face's loops, which is exact and needs no half-plane clipping routine.
//! 4. Segments that land on top of each other are merged. This matters when the plane *is* a face
//!    plane, or passes exactly through an edge: then the adjacent faces produce the same segment
//!    the coplanar loop already has, and leaving both copies would cancel the section under
//!    even-odd fill.
//! 5. What is left is chained into loops and turned into regions.
//!
//! Degenerate cuts — the plane through an exact vertex, or tangent to a face — are handled by the
//! tolerance, and a section that cannot be closed is an error rather than a silently wrong region.

use cadcraft_geom::{Regions, Vec2, Vec3, bool2d};

use crate::{Face, Plane, Solid, SolidError};

/// The material of a solid where a plane crosses it.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub plane: Plane,
    /// The cross-section in `plane`'s own 2D frame.
    pub region: Regions,
}

/// Cross-section of `solid` by `plane`.
///
/// 1. A face of the solid either lies **in** the plane, is **parallel** to it (and contributes
///    nothing), or **crosses** it.
/// 2. Coplanar faces are merged first, as regions. Merging rather than concatenating matters: two
///    coplanar faces of one solid share an edge, and handing both loops to the loop-chainer makes
///    that shared edge a three-way vertex, which silently loses a loop.
/// 3. A crossing face contributes segments: the line where the two planes meet, clipped to the
///    face's own region. Clipping is by even-odd pairing of the line's crossings with the face's
///    loops, which is exact and needs no half-plane clipping routine.
/// 4. Segments that land on top of each other are merged. This matters when the plane *is* a face
///    plane, or passes exactly through an edge: then the adjacent faces produce the same segment
///    the coplanar loop already has, and leaving both copies would cancel the section.
/// 5. Coplanar loops and traced loops go through [`Regions::from_loops`] together, which sorts them
///    into regions by nesting depth.
pub fn section(solid: &Solid, plane: &Plane, tol: f64) -> Result<Section, SolidError> {
    let coplanar = coplanar_faces(solid, plane, tol);
    // Coplanar loops go back out as segments so they take part in the same merge as the crossing
    // ones: a segment the plane contains and the same segment produced by the faces around it must
    // collapse to one, or the section carries the boundary twice.
    let mut segments: Vec<(Vec2, Vec2)> = Vec::new();
    for l in coplanar.loops() {
        let n = l.len();
        for i in 0..n {
            if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) {
                segments.push((*a, *b));
            }
        }
    }
    for face in &solid.faces {
        if face.plane.parallel(plane, 1e-9) {
            continue;
        }
        segments.extend(crossing_segments(face, plane, tol));
    }
    let merged = prune_dangling(merge_segments(segments, tol), tol);
    let traced = bool2d::chain_segments(&merged, tol).ok_or_else(|| SolidError::Section("segments could not be chained".into()))?;
    let region = Regions::from_loops(traced, tol).ok_or_else(|| SolidError::Section("section loops could not be sorted".into()))?;
    // A plane that *contains* one of the solid's faces must section to exactly those faces: the
    // plane cuts nothing else. If the section differs, the plane is also grazing the solid along
    // some other wall — the degenerate case where a pocket lines up exactly with an existing face.
    // The two readings give different solids, so it is refused rather than guessed at.
    //
    // Comparing against the faces themselves, not just their area, keeps this exact: a chamfer's
    // end cap and a prism's side are both fine, because what the plane contains and what it
    // sections to are the same region.
    if !coplanar.is_empty() {
        let difference = bool2d::xor(&region, &coplanar).map(|r| r.area()).unwrap_or(f64::INFINITY);
        let slack = coplanar.area() * 1e-6 + tol * tol;
        if difference > slack {
            return Err(SolidError::Section(format!(
                "the plane contains a face of area {:.6} but sections to {:.6}: it grazes the solid elsewhere",
                coplanar.area(),
                region.area()
            )));
        }
    }
    Ok(Section { plane: *plane, region })
}

/// The faces of `solid` that lie in `plane`, expressed in `plane`'s frame.
fn coplanar_faces(solid: &Solid, plane: &Plane, tol: f64) -> Regions {
    let mut out: Regions = Regions::default();
    for f in &solid.faces {
        if !f.plane.coplanar(plane, tol, 1e-9) {
            continue;
        }
        let mapped = f.retargeted(*plane);
        out = bool2d::union(&out, &mapped.region).unwrap_or(out);
    }
    out
}

/// The segments where `face` crosses `plane`, expressed in `plane`'s frame.
///
/// The two planes meet in a line with direction `face.normal x plane.normal`. Walking that line,
/// the crossings with the face's loops alternate in and out of the face, so pairing them gives
/// exactly the pieces of the line inside the face.
fn crossing_segments(face: &Face, plane: &Plane, tol: f64) -> Vec<(Vec2, Vec2)> {
    let dir = face.plane.normal.cross(plane.normal).normalized();
    if dir.len() < 0.5 {
        return Vec::new();
    }
    // g(q) = 0 is the line, in the face's own frame.
    let a = face.plane.u.dot(plane.normal);
    let b = face.plane.v().dot(plane.normal);
    let c0 = (face.plane.origin - plane.origin).dot(plane.normal);
    if a.abs() < 1e-15 && b.abs() < 1e-15 {
        return Vec::new();
    }
    let g = |q: Vec2| -> f64 { c0 + a * q.x + b * q.y };
    // Crossings are kept as *points*, not as distances along the line: the line is only a
    // direction, and rebuilding a point from origin + dir * t would drop the offset across the
    // line, which is the whole reason the segment ends up somewhere else.
    let mut crossings: Vec<(f64, Vec3)> = Vec::new();
    for lp in face.region.loops() {
        let n = lp.len();
        for i in 0..n {
            let (Some(p), Some(q)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let (gp, gq) = (g(*p), g(*q));
            if !gp.is_finite() || !gq.is_finite() {
                continue;
            }
            let on_line = |g: f64| g.abs() <= tol * (a.abs() + b.abs()).max(1.0);
            if on_line(gp) || on_line(gq) {
                let w = if on_line(gp) { face.plane.to_world(p.x, p.y) } else { face.plane.to_world(q.x, q.y) };
                crossings.push(((w - plane.origin).dot(dir), w));
                continue;
            }
            if (gp > 0.0) != (gq > 0.0) {
                let s = gp / (gp - gq);
                if !(0.0..=1.0).contains(&s) {
                    continue;
                }
                let w = face.plane.to_world(p.x + s * (q.x - p.x), p.y + s * (q.y - p.y));
                crossings.push(((w - plane.origin).dot(dir), w));
            }
        }
    }
    if crossings.len() < 2 {
        return Vec::new();
    }
    crossings.sort_by(|x, y| x.0.total_cmp(&y.0));
    // The same point found from both incident edges of a vertex on the line.
    let span = tol.max(1e-300);
    crossings.dedup_by(|x, y| (x.0 - y.0).abs() <= span || x.1.near(y.1, tol));
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < crossings.len() {
        let (Some((_, p0)), Some((_, p1))) = (crossings.get(i), crossings.get(i + 1)) else {
            break;
        };
        if (*p0).dist(*p1) > span {
            out.push((plane.to_local(*p0), plane.to_local(*p1)));
        }
        i += 2;
    }
    out
}

/// Drop segments that dangle from the section boundary.
///
/// The section of a closed solid by a plane is a closed curve: every vertex has even degree. A
/// wall that ends exactly *on* the cutting plane — a blend tool whose end cap grazes the edge it is
/// blending — contributes a stub with one dangling end, and that stub is what makes the loop
/// chaining lose the real boundary next to it.
///
/// Only degree *one* is pruned. A vertex where three boundary pieces meet is a real tangency in the
/// surface, not an artifact, and removing edges there would delete geometry.
fn prune_dangling(segments: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<(Vec2, Vec2)> {
    let mut current = segments;
    for _ in 0..8 {
        if current.is_empty() {
            break;
        }
        let bb = cadcraft_geom::Bounds2::from_points(current.iter().flat_map(|(a, b)| [*a, *b]));
        let key = |p: Vec2| (((p.x - bb.min.x) / tol).round() as i64, ((p.y - bb.min.y) / tol).round() as i64);
        let mut degree: std::collections::HashMap<(i64, i64), usize> = std::collections::HashMap::new();
        for (a, b) in &current {
            *degree.entry(key(*a)).or_default() += 1;
            *degree.entry(key(*b)).or_default() += 1;
        }
        let free = |p: Vec2| degree.get(&key(p)).copied() == Some(1);
        let next: Vec<(Vec2, Vec2)> = current.iter().copied().filter(|(a, b)| !(free(*a) || free(*b))).collect();
        if next.len() == current.len() {
            break;
        }
        current = next;
    }
    current
}

/// Drop segments that describe the same piece twice, keeping one copy.
///
/// Coplanar faces and the walls around them produce the same segment when the cutting plane
/// contains a face or an edge; under even-odd fill two copies cancel and the section would come
/// out empty.
fn merge_segments(segments: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<(Vec2, Vec2)> {
    let bb = cadcraft_geom::Bounds2::from_points(segments.iter().flat_map(|(a, b)| [*a, *b]));
    if bb.is_empty() {
        return segments;
    }
    let mut out: Vec<(Vec2, Vec2)> = Vec::with_capacity(segments.len());
    for (a, b) in segments {
        if !a.is_finite() || !b.is_finite() || a.near(b, tol) {
            continue;
        }
        let dup = out.iter().any(|(c, d)| (a.near(*c, tol) && b.near(*d, tol)) || (a.near(*d, tol) && b.near(*c, tol)));
        if !dup {
            out.push((a, b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Vec3, extrude,
        tests_support::{box_at as box_solid, must},
    };

    #[test]
    fn a_plane_through_the_middle_of_a_box_sections_it_as_a_rectangle() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::xy(2.0), 1e-9), "section");
        assert!((sec.region.area() - 16.0).abs() < 1e-9, "area {}", sec.region.area());
    }

    #[test]
    fn a_plane_outside_the_solid_sections_nothing() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::xy(10.0), 1e-9), "section");
        assert!(sec.region.is_empty());
    }

    #[test]
    fn a_plane_just_touching_the_solid_sections_nothing() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::xy(4.0 + 1e-6), 1e-9), "section");
        assert!(sec.region.is_empty(), "a plane that misses must not pick up the face it is near");
    }

    #[test]
    fn a_plane_at_a_face_plane_sections_that_face() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::xy(0.0), 1e-9), "section");
        assert!((sec.region.area() - 16.0).abs() < 1e-9, "area {}", sec.region.area());
    }

    #[test]
    fn a_plane_at_a_face_plane_from_above_also_works() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::xy(4.0), 1e-9), "section");
        assert!((sec.region.area() - 16.0).abs() < 1e-9, "area {}", sec.region.area());
    }

    #[test]
    fn a_vertical_plane_sections_the_side() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let sec = must(section(&s, &Plane::yz(2.0), 1e-9), "section");
        assert!((sec.region.area() - 16.0).abs() < 1e-9, "area {}", sec.region.area());
    }

    #[test]
    fn a_diagonal_plane_sections_the_box_as_a_rectangle() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        // The plane x + y = 2 runs along the bottom face and meets two edges at exact vertices,
        // which is the degenerate case: the section is only closed if the points on those vertices
        // are used as the section's own corners rather than rebuilt from the crossing line.
        let Ok(plane) = Plane::new(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)) else { panic!("plane") };
        let sec = must(section(&s, &plane, 1e-9), "section");
        // 4 long in z, and the chord across the bottom face is from (2, 0) to (0, 2) — 2*sqrt(2).
        let expected = 4.0 * 2.0 * 2f64.sqrt();
        assert!((sec.region.area() - expected).abs() < 1e-9, "area {} expected {expected}", sec.region.area());
        // The same cut, moved off the vertices by a hair, has to give the same answer: a section
        // that changes shape when the plane moves by a millionth is not a section anybody can build
        // a face from.
        let Ok(nudged) = Plane::new(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)) else { panic!("plane") };
        let other = Plane::new(nudged.origin + nudged.u * 1e-7, nudged.normal).unwrap_or(nudged);
        let sec2 = must(section(&s, &other, 1e-9), "section");
        assert!((sec2.region.area() - expected).abs() < 1e-5, "a nudged plane gives {}", sec2.region.area());
    }

    #[test]
    fn a_plane_through_a_vertex_only_never_panics() {
        // x + y = 0 touches the box along one edge and nothing else. There is no area there, so the
        // honest answer is an empty section; what matters is that it is an answer and not a panic.
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let Ok(plane) = Plane::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)) else { panic!("plane") };
        if let Ok(sec) = section(&s, &plane, 1e-9) {
            assert!(sec.region.area() <= 1e-9, "a tangent plane has no area: {}", sec.region.area());
        }
    }

    #[test]
    fn a_tube_sections_as_an_annulus() {
        let s = must(extrude(&crate::tests_support::rect_with_hole(0.0, 0.0, 10.0, 10.0, 3.0, 3.0, 7.0, 7.0), &Plane::xy(0.0), 2.0), "extrude");
        let sec = must(section(&s, &Plane::xy(1.0), 1e-9), "section");
        assert!((sec.region.area() - 84.0).abs() < 1e-9, "area {}", sec.region.area());
    }

    #[test]
    fn a_section_is_independent_of_the_cutting_plane_origin() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let a = must(section(&s, &Plane::xy(2.0), 1e-9), "section");
        // Same plane, different origin on it.
        let Ok(shifted) = Plane::new(Vec3::new(100.0, 100.0, 2.0), Vec3::Z) else { panic!("plane") };
        let b = must(section(&s, &shifted, 1e-9), "section");
        assert!((a.region.area() - b.region.area()).abs() < 1e-9);
    }

    #[test]
    fn hostile_planes_do_not_panic() {
        let s = box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        for p in [
            Plane::new(Vec3::new(f64::NAN, 0.0, 1.0), Vec3::Z),
            Plane::new(Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 1.0)),
            Plane::new(Vec3::ZERO, Vec3::new(1e-300, 1e-300, 0.0)),
        ] {
            let Ok(plane) = p else { continue };
            let _ = section(&s, &plane, 1e-9);
        }
    }
}
