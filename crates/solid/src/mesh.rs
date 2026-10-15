//! Tessellation: planar faces to triangles, plus STL writing.
//!
//! Triangulation is ear clipping after holes are bridged into the outer loop. It is exact for the
//! geometry it is given (no smoothing, no dropping), which matters because the volume a solid
//! reports and the volume its mesh reports are compared against each other: a tessellation that
//! lost a hole would show up immediately as a volume mismatch.

use cadcraft_geom::{Region, Vec2, Vec3};

use crate::{Solid, SolidError, limits::MAX_TRIANGLES};

/// One triangle of a tessellation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}

impl Triangle {
    pub fn normal(&self) -> Vec3 {
        (self.b - self.a).cross(self.c - self.a).normalized()
    }
    pub fn area(&self) -> f64 {
        (self.b - self.a).cross(self.c - self.a).len() * 0.5
    }
    pub fn center(&self) -> Vec3 {
        (self.a + self.b + self.c) / 3.0
    }
}

/// A triangle mesh: shared vertices plus index triples.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub verts: Vec<Vec3>,
    /// Indices into `verts`, wound counter-clockwise seen from outside.
    pub tris: Vec<[usize; 3]>,
}

impl Mesh {
    pub fn triangles(&self) -> impl Iterator<Item = Triangle> + '_ {
        self.tris.iter().filter_map(|t| match (self.verts.get(t[0]), self.verts.get(t[1]), self.verts.get(t[2])) {
            (Some(a), Some(b), Some(c)) => Some(Triangle { a: *a, b: *b, c: *c }),
            _ => None,
        })
    }

    pub fn bounds(&self) -> cadcraft_geom::Bounds3 {
        cadcraft_geom::Bounds3::from_points(self.verts.iter().copied())
    }
}

/// Tessellate a solid into triangles, one triangulation per face.
pub fn mesh_from_solid(solid: &Solid) -> Result<Mesh, SolidError> {
    let mut mesh = Mesh::default();
    for face in &solid.faces {
        for region in face.region.regions() {
            for tri in triangulate_region(region)? {
                let base = mesh.verts.len();
                mesh.verts.push(face.plane.to_world(tri[0].x, tri[0].y));
                mesh.verts.push(face.plane.to_world(tri[1].x, tri[1].y));
                mesh.verts.push(face.plane.to_world(tri[2].x, tri[2].y));
                mesh.tris.push([base, base + 1, base + 2]);
                if mesh.tris.len() > MAX_TRIANGLES {
                    return Err(SolidError::TooManyTriangles(mesh.tris.len()));
                }
            }
        }
    }
    if mesh.tris.is_empty() {
        return Err(SolidError::Empty);
    }
    Ok(mesh)
}

/// Triangulate one region (outer loop plus holes) in its own 2D coordinates.
///
/// The outer loop and each hole are clipped **separately** and the hole triangles are emitted with
/// reversed winding. That is exact and needs no bridge: a simple polygon is exactly covered by its
/// own ear clipping, so the outer triangles cover the outer loop and the hole triangles subtract
/// exactly the holes. Bridging holes into one weakly simple loop — the textbook route — is also
/// correct in principle, but its output depends on finding a *visible* bridge vertex, and a wrong
/// bridge silently produces triangles that overlap or miss area instead of failing.
///
/// A test asserts the triangle areas sum to `outer - holes`, which is what caught that.
pub fn triangulate_region(region: &Region) -> Result<Vec<[Vec2; 3]>, SolidError> {
    if region.outer.len() < 3 {
        return Err(SolidError::EmptyProfile);
    }
    let tol = tolerance_of(region);
    let mut out = clip_loop_ccw(&region.outer, tol)?;
    for hole in &region.holes {
        if hole.len() < 3 {
            continue;
        }
        // Whether a hole is wound clockwise is the caller's business and both windings occur in
        // practice; its *role* is not. Forcing every hole's triangles negative makes the region
        // subtract its holes whichever way they were given, instead of adding them when a caller
        // hands over a counter-clockwise hole.
        for t in clip_loop_ccw(hole, tol)? {
            out.push([t[0], t[2], t[1]]);
        }
    }
    if out.is_empty() {
        return Err(SolidError::EmptyProfile);
    }
    Ok(out)
}

/// Triangulate a loop so its triangles come out counter-clockwise, whatever the loop's own winding.
///
/// Ear clipping needs a convex corner and a clockwise loop has none, so `clip_loop` reverses first
/// and hands the triangles back in the original winding. This adds the one step that makes the
/// result independent of that: measure the signed area and flip if needed.
fn clip_loop_ccw(loop_pts: &[Vec2], tol: f64) -> Result<Vec<[Vec2; 3]>, SolidError> {
    let mut tris = clip_loop(loop_pts, tol)?;
    let signed: f64 = tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0])).sum();
    if signed < 0.0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    Ok(tris)
}

fn tolerance_of(region: &Region) -> f64 {
    let bb = region.bounds();
    let d = bb.width().hypot(bb.height());
    if !d.is_finite() || d <= 0.0 { 1e-12 } else { (d * 1e-11).clamp(1e-13, 1e-3) }
}

/// Ear clipping for one simple loop, with the triangles left in the loop's own winding.
fn clip_loop(loop_pts: &[Vec2], tol: f64) -> Result<Vec<[Vec2; 3]>, SolidError> {
    // Repeated points first: a zero-length edge can never be a convex corner, so leaving one in
    // stops the clipper dead rather than producing a slightly wrong triangle.
    let mut clean: Vec<Vec2> = Vec::with_capacity(loop_pts.len());
    for p in loop_pts {
        if !p.is_finite() {
            continue;
        }
        if clean.last().is_none_or(|q| !q.near(*p, tol)) {
            clean.push(*p);
        }
    }
    if clean.len() >= 2 && clean.first().is_some_and(|f| f.near(*clean.last().unwrap_or(f), tol)) {
        clean.pop();
    }
    let mut poly = drop_collinear(clean);
    if poly.len() < 3 {
        return Err(SolidError::EmptyProfile);
    }
    // Ear clipping needs a convex corner, which a clockwise loop has none of. Clip counter-
    // clockwise and hand the triangles back in the loop's own winding.
    let ccw = cadcraft_geom::shoelace(&poly) > 0.0;
    if !ccw {
        poly.reverse();
    }
    let mut tris = if poly.len() == 3 { vec![[poly[0], poly[1], poly[2]]] } else { ear_clip(&poly, tol)? };
    if !ccw {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    Ok(tris)
}

/// Drop vertices lying on the straight line between their neighbours; a spike is kept.
fn drop_collinear(l: Vec<Vec2>) -> Vec<Vec2> {
    let n = l.len();
    if n < 3 {
        return l;
    }
    let mut keep: Vec<Vec2> = Vec::with_capacity(n);
    for i in 0..n {
        let prev = if i == 0 { l.get(n - 1) } else { l.get(i - 1) };
        let next = if i + 1 == n { l.first() } else { l.get(i + 1) };
        let drop = match (prev, l.get(i), next) {
            (Some(p), Some(v), Some(q)) => {
                let d1 = *v - *p;
                let d2 = *q - *v;
                let scale = d1.len() * d2.len();
                d1.dot(d2) > 0.0 && d1.cross(d2).abs() <= 1e-12 * scale.max(1e-300)
            }
            _ => false,
        };
        if let Some(v) = l.get(i)
            && !drop
        {
            keep.push(*v);
        }
    }
    if keep.len() < 3 { l } else { keep }
}

/// Ear clipping on a simple loop. Triangles come out with the loop's winding.
fn ear_clip(poly: &[Vec2], tol: f64) -> Result<Vec<[Vec2; 3]>, SolidError> {
    let n = poly.len();
    let mut prev: Vec<usize> = (0..n).map(|i| if i == 0 { n - 1 } else { i - 1 }).collect();
    let mut next: Vec<usize> = (0..n).map(|i| if i + 1 == n { 0 } else { i + 1 }).collect();
    let mut alive = vec![true; n];
    let mut out: Vec<[Vec2; 3]> = Vec::with_capacity(n);
    let mut remaining = n;
    while remaining > 3 {
        let mut clipped = false;
        for i in 0..n {
            if !alive.get(i).copied().unwrap_or(false) {
                continue;
            }
            let (Some(pi), Some(ni)) = (prev.get(i).copied(), next.get(i).copied()) else { continue };
            let (Some(a), Some(b), Some(c)) = (poly.get(pi), poly.get(i), poly.get(ni)) else { continue };
            if !is_ear(poly, a, b, c, &alive, tol) {
                continue;
            }
            out.push([*a, *b, *c]);
            alive[i] = false;
            next[pi] = ni;
            prev[ni] = pi;
            remaining -= 1;
            clipped = true;
            break;
        }
        if !clipped {
            return Err(SolidError::Invalid(format!(
                "a loop of {} vertices (area {:.6}) could not be triangulated",
                remaining,
                cadcraft_geom::shoelace(poly)
            )));
        }
    }
    let live: Vec<usize> = (0..n).filter(|i| alive.get(*i).copied().unwrap_or(false)).collect();
    let tri = match live[..] {
        [a, b, c] => match (poly.get(a), poly.get(b), poly.get(c)) {
            (Some(p), Some(q), Some(r)) => Some([*p, *q, *r]),
            _ => None,
        },
        _ => None,
    };
    if let Some(t) = tri {
        out.push(t);
    }
    if out.is_empty() {
        return Err(SolidError::Invalid("triangulation produced nothing".into()));
    }
    Ok(out)
}

/// Is the corner `a-b-c` an ear? Convex, and no *remaining* vertex inside it.
fn is_ear(poly: &[Vec2], a: &Vec2, b: &Vec2, c: &Vec2, alive: &[bool], tol: f64) -> bool {
    let cross = (*b - *a).cross(*c - *a);
    if cross <= tol * tol {
        return false;
    }
    let tri = [*a, *b, *c];
    for (i, p) in poly.iter().enumerate() {
        if !alive.get(i).copied().unwrap_or(false) {
            continue;
        }
        if p.near(*a, tol) || p.near(*b, tol) || p.near(*c, tol) {
            continue;
        }
        if point_in_triangle(*p, tri) {
            return false;
        }
    }
    true
}

fn point_in_triangle(p: Vec2, t: [Vec2; 3]) -> bool {
    let d1 = (t[1] - t[0]).cross(p - t[0]);
    let d2 = (t[2] - t[1]).cross(p - t[1]);
    let d3 = (t[0] - t[2]).cross(p - t[2]);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// ASCII STL, one `facet normal` per triangle.
pub fn stl_ascii(mesh: &Mesh) -> String {
    let mut s = String::with_capacity(mesh.tris.len() * 160);
    s.push_str("solid cadcraft\n");
    for t in mesh.triangles() {
        let n = t.normal();
        s.push_str(&format!(
            "facet normal {} {} {}\n  outer loop\n    vertex {:.6} {:.6} {:.6}\n    vertex {:.6} {:.6} {:.6}\n    vertex {:.6} {:.6} {:.6}\n  endloop\nendfacet\n",
            n.x, n.y, n.z, t.a.x, t.a.y, t.a.z, t.b.x, t.b.y, t.b.z, t.c.x, t.c.y, t.c.z
        ));
    }
    s.push_str("endsolid cadcraft\n");
    s
}

/// Binary STL: an 80-byte header, a `u32` triangle count, then 50 bytes per triangle.
///
/// The format is fixed by the spec: 12 **single-precision** values (normal and three vertices) plus
/// a 2-byte attribute. Writing `f64` here, or a short header, produces a file every reader rejects,
/// so both are exact — a facet is 48 + 2 bytes, always.
pub fn stl_binary(mesh: &Mesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + mesh.tris.len() * 50);
    let mut header = [0u8; 80];
    let name = b"CADCraft binary STL";
    header[..name.len().min(80)].copy_from_slice(&name[..name.len().min(80)]);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(mesh.tris.len() as u32).to_le_bytes());
    for t in mesh.triangles() {
        let n = t.normal();
        for v in [n.x, n.y, n.z, t.a.x, t.a.y, t.a.z, t.b.x, t.b.y, t.b.z, t.c.x, t.c.y, t.c.z] {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{box_at, must};
    use cadcraft_geom::{Region, Vec2};

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]
    }

    /// **Signed** area of a triangulation. Hole triangles come out clockwise and subtract, which is
    /// exactly what makes the sum equal `outer - holes`; taking absolute values here would report
    /// the holes as material and hide the very bug it should catch.
    fn area_of(tris: &[[Vec2; 3]]) -> f64 {
        tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]) * 0.5).sum()
    }

    fn f32_at(bytes: &[u8], at: usize) -> f32 {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&bytes[at..at + 4]);
        f32::from_le_bytes(buf)
    }

    #[test]
    fn a_binary_stl_has_the_layout_the_format_asks_for() {
        // 80-byte header, a u32 count, then 50 bytes per facet. Writing f64 (96 bytes), or leaving
        // the header short, makes a file that no reader accepts and that no test notices — this is
        // the test that notices.
        let s = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let mesh = must(mesh_from_solid(&s), "mesh");
        let bytes = stl_binary(&mesh);
        assert_eq!(bytes.len(), 84 + mesh.tris.len() * 50, "every facet is 50 bytes");
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        assert_eq!(count, mesh.tris.len(), "the count matches the triangles written");
        assert_eq!(bytes.len(), 84 + count * 50);
        // Values are single precision: the first facet's normal and vertices have to read back as
        // f32 numbers, not as half of an f64.
        let first = &bytes[84..134];
        let n = Vec3::new(f64::from(f32_at(first, 0)), f64::from(f32_at(first, 4)), f64::from(f32_at(first, 8)));
        assert!((n.len() - 1.0).abs() < 1e-6, "a unit normal, got {n:?}");
        let a = Vec3::new(f64::from(f32_at(first, 12)), f64::from(f32_at(first, 16)), f64::from(f32_at(first, 20)));
        assert!(a.x >= -1.0 && a.x <= 3.0, "a vertex of the box, got {a:?}");
        // The 2-byte attribute word is zeroed, as the spec's "attribute byte count" wants.
        assert_eq!(&bytes[132..134], &[0, 0]);
    }

    #[test]
    fn the_mesh_volume_matches_the_exact_volume_of_the_solid() {
        // Two ways of measuring the same body: the pyramid sum from the faces, and tetrahedra from
        // the triangles. They agree to a hair on a box; if they ever do not, one of them is lying.
        let s = box_at(Vec3::new(-1.0, 2.0, 3.0), Vec3::new(2.0, 4.0, 5.0));
        let mesh = must(mesh_from_solid(&s), "mesh");
        let mesh_volume: f64 = mesh.triangles().map(|t| (t.a - s.bounds().min).cross(t.b - s.bounds().min).dot(t.c - s.bounds().min) / 6.0).sum();
        assert!((mesh_volume - s.volume()).abs() < 1e-9, "mesh {mesh_volume} vs exact {}", s.volume());
        assert!((s.volume() - 12.0).abs() < 1e-9, "a 3x2x2 box is 12");
    }

    #[test]
    fn a_square_makes_two_triangles() {
        let r = Region::new(square(0.0, 0.0, 2.0, 3.0));
        let tris = triangulate_region(&r).unwrap_or_default();
        assert_eq!(tris.len(), 2);
        assert!((area_of(&tris) - 6.0).abs() < 1e-12);
    }

    #[test]
    fn triangles_cover_the_polygon_exactly() {
        // A concave polygon: ear clipping must not cover the notch.
        let poly = vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 4.0)];
        let r = Region::new(poly);
        let tris = triangulate_region(&r).unwrap_or_default();
        assert_eq!(tris.len(), 3);
        assert!((area_of(&tris) - 12.0).abs() < 1e-12);
    }

    #[test]
    fn a_region_with_a_hole_triangulates_to_outer_minus_hole() {
        let r = Region { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![square(3.0, 3.0, 7.0, 7.0)] };
        let tris = triangulate_region(&r).unwrap_or_default();
        assert!((area_of(&tris) - 84.0).abs() < 1e-9, "area {}", area_of(&tris));
    }

    #[test]
    fn two_holes_triangulate_to_the_right_area() {
        let r = Region { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![square(1.0, 1.0, 3.0, 3.0), square(6.0, 6.0, 9.0, 9.0)] };
        let tris = triangulate_region(&r).unwrap_or_default();
        assert!((area_of(&tris) - (100.0 - 4.0 - 9.0)).abs() < 1e-9, "area {}", area_of(&tris));
    }

    #[test]
    fn an_off_centre_hole_still_bridges() {
        let r = Region { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![square(0.2, 0.2, 1.0, 9.5)] };
        let tris = triangulate_region(&r).unwrap_or_default();
        assert!((area_of(&tris) - (100.0 - 0.8 * 9.3)).abs() < 1e-9, "area {}", area_of(&tris));
    }

    #[test]
    fn degenerate_input_is_an_error_not_a_panic() {
        assert!(triangulate_region(&Region::default()).is_err());
        assert!(triangulate_region(&Region::new(vec![Vec2::ZERO, Vec2::ZERO, Vec2::ZERO])).is_err());
        let bad = Region::new(vec![Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)]);
        let _ = triangulate_region(&bad);
    }

    #[test]
    fn stl_round_trip_shape() {
        let mesh = Mesh { verts: vec![Vec3::ZERO, Vec3::X, Vec3::Y], tris: vec![[0, 1, 2]] };
        let bin = stl_binary(&mesh);
        assert_eq!(bin.len(), 84 + 50);
        assert_eq!(u32::from_le_bytes(bin[80..84].try_into().unwrap_or([0; 4])), 1);
        let txt = stl_ascii(&mesh);
        assert!(txt.starts_with("solid cadcraft"));
        assert!(txt.contains("facet normal"));
        assert_eq!(txt.matches("vertex").count(), 3);
    }
}
