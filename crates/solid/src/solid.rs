//! Faces, solids, their topology and the invariant checks that guard every operation.

use cadcraft_geom::{Bounds3, Region, Regions, Vec2, Vec3};

use crate::{
    SolidError,
    limits::{MAX_FACES, MAX_LOOP_VERTS},
    mass::{MassProperties, mass_properties},
    plane::Plane,
    tolerance_for,
};

/// One planar face: the plane it lies in and the area it bounds, in that plane's 2D frame.
///
/// The loop must run counter-clockwise in the plane's own frame, which by the right-handedness of
/// the frame means the normal points out of the material. That convention is what makes the
/// signed volume meaningful, so every constructor goes through [`Face::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub plane: Plane,
    pub region: Regions,
}

/// An edge of a solid: the segment where two faces meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: Vec3,
    pub b: Vec3,
    /// Indices of the two faces sharing it, ascending.
    pub faces: [usize; 2],
}

impl Face {
    /// A face, rejecting empty or non-finite geometry.
    pub fn new(plane: Plane, region: Regions) -> Result<Face, SolidError> {
        if region.is_empty() {
            return Err(SolidError::Empty);
        }
        if !plane.origin.is_finite() || !plane.normal.is_finite() {
            return Err(SolidError::NonFinite);
        }
        for l in region.loops() {
            if l.len() > MAX_LOOP_VERTS {
                return Err(SolidError::TooManyFaces(l.len(), MAX_LOOP_VERTS));
            }
            if l.iter().any(|p| !p.is_finite()) {
                return Err(SolidError::NonFinite);
            }
        }
        Ok(Face { plane, region })
    }

    /// The same boundary, re-expressed in another plane's frame.
    ///
    /// The frames differ by a reflection (the two planes are the same plane seen from the other
    /// side), so the loop coordinates have to be carried across. Keeping the same numbers would
    /// mirror the face instead of turning it, which puts its edges somewhere else entirely.
    pub fn retargeted(&self, plane: Plane) -> Face {
        let from = self.plane;
        let map = |l: &[Vec2]| -> Vec<Vec2> { l.iter().map(|p| plane.to_local(from.to_world(p.x, p.y))).collect() };
        let region =
            Regions::new(self.region.iter().map(|r| Region { outer: map(&r.outer), holes: r.holes.iter().map(|h| map(h)).collect() }).collect());
        Face { plane, region }
    }

    /// The same face seen from the other side: same boundary, reversed normal.
    ///
    /// Carrying the boundary across the mirrored frame already reverses how the loop reads, so it
    /// is wound back to keep the convention (outer loops counter-clockwise in the plane's own
    /// frame, which is what makes the normal point out of the material).
    ///
    /// Applied to *every* face of a shell this turns the shell inside out while keeping its faces
    /// mutually consistent. Applied to one face it would leave that face disagreeing with its
    /// neighbours, which is why [`Solid::orient_faces`] probes instead of guessing.
    pub fn flipped(&self) -> Result<Face, SolidError> {
        let f = self.retargeted(self.plane.flipped());
        let f = Face { plane: f.plane, region: reverse_regions(&f.region) };
        if f.region.is_empty() {
            return Err(SolidError::Empty);
        }
        Ok(f)
    }

    /// Area of the face. Exact: the loops are planar polygons, so the shoelace sum is exact up to
    /// rounding.
    pub fn area(&self) -> f64 {
        self.region.area()
    }

    /// Is the in-plane point inside the face (holes included)?
    pub fn contains_local(&self, p: Vec2) -> bool {
        self.region.contains(p)
    }

    /// Every loop vertex in world coordinates.
    pub fn vertices(&self) -> Vec<Vec3> {
        let plane = self.plane;
        self.region.loops().flat_map(|l| l.iter().map(move |p| plane.to_world(p.x, p.y))).collect()
    }

    /// A point strictly inside the face, or `None` when no candidate lands inside.
    ///
    /// Used to probe which way a face points, so it has to be genuinely interior: the
    /// area-weighted centroid of the outer loop first, then the mean of its vertices, then an edge
    /// midpoint.
    pub fn interior_point(&self) -> Option<Vec3> {
        let outer = self.region.iter().find(|r| r.outer.len() >= 3)?.outer.clone();
        let n = outer.len();
        // Area-weighted centroid of the loop, measured about the first vertex.
        let o = outer.first().copied()?;
        let mut a = 0.0;
        let mut cx = 0.0;
        let mut cy = 0.0;
        for i in 0..n {
            let (Some(p), Some(q)) = (outer.get(i), outer.get((i + 1) % n)) else { continue };
            let (p, q) = (*p - o, *q - o);
            let f = p.cross(q);
            a += f;
            cx += (p.x + q.x) * f;
            cy += (p.y + q.y) * f;
        }
        if a.abs() > 1e-300 {
            let c = Vec2::new(cx / (3.0 * a), cy / (3.0 * a)) + o;
            if self.contains_local(c) {
                return Some(self.plane.to_world(c.x, c.y));
            }
        }
        let mut sum = Vec2::ZERO;
        for p in &outer {
            sum += *p;
        }
        let mean = sum / n as f64;
        if self.contains_local(mean) {
            return Some(self.plane.to_world(mean.x, mean.y));
        }
        for w in outer.windows(2) {
            if let [p, q] = w {
                let m = p.mid(*q);
                if self.contains_local(m) {
                    return Some(self.plane.to_world(m.x, m.y));
                }
            }
        }
        None
    }

    /// Every boundary segment in world coordinates, in loop order.
    pub fn edges(&self) -> Vec<(Vec3, Vec3)> {
        let mut out = Vec::new();
        for l in self.region.loops() {
            let n = l.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) {
                    out.push((self.plane.to_world(a.x, a.y), self.plane.to_world(b.x, b.y)));
                }
            }
        }
        out
    }
}

/// A lattice of canonical corners, and the search that keeps two nearby corners in the same one.
///
/// # Why the cells are twice the tolerance
///
/// Bucketing corners on a `tol`-sized grid and calling each cell a corner is not a weld, it is a
/// lottery: the rounding boundary between two cells sits half a cell inside each of them, so two
/// corners of the same solid that arrived by different routes and came to rest a ten-millionth of a
/// unit apart fall either side of it as often as not. Each then gets a cell to itself, each is
/// written twice, and the pass hands back the very mismatch it was called to remove — a shell that
/// read as having an edge used once.
///
/// Cells `2 * tol` wide close that hole without changing anything else. Two corners within `tol`
/// differ by at most half a cell in each axis, so their cell indices differ by at most one and the
/// twenty-six neighbours around a corner are always enough to find the corner it belongs to. What
/// the cells *are* stops mattering; only the search does.
///
/// The search returns the **lowest-numbered** corner within `tol`, never the first one a walk
/// happens to reach. That matters because the pass runs twice: once to register the corners, once to
/// rewrite the loops against them. Both walks must reach the same verdict, or a corner is snapped to
/// one number on the way in and left on the other on the way out.
///
/// Registration is in the shell's own vertex order, so the answer does not depend on hash iteration,
/// and a shell with nothing to weld comes back untouched rather than moved by up to half a cell.
///
/// # Why the corner's *number* is the identity, and not just its position
///
/// A corner number says two points are the same vertex without ever asking which cell either of them
/// fell in, and that is the whole point: a single rounded index per axis (`vkey`) cannot make that
/// promise, because its cell boundary sits half a cell inside each cell and two points that name the
/// same corner — the last bit apart, as two faces' copies of one corner always are — land either
/// side of it as often as not. Clusters this grid builds by distance have no such line to fall on:
/// the search is over the corners, not over an index, so where the boundary would have been is not a
/// question it ever asks. That is why `weld_edges` uses the grid to *move* geometry and `Solid`
/// uses the corner *numbers* to decide identity — see the note on `vkey`.
#[derive(Debug)]
struct WeldGrid {
    tol: f64,
    cell: f64,
    origin: Vec3,
    buckets: std::collections::BTreeMap<(i64, i64, i64), Vec<usize>>,
    corners: Vec<Vec3>,
}

impl WeldGrid {
    fn new(tol: f64, origin: Vec3) -> Self {
        WeldGrid { tol, cell: 2.0 * tol, origin, buckets: std::collections::BTreeMap::new(), corners: Vec::new() }
    }

    /// The cell a corner falls in. The indices are bounded by the shell's own size over `tol`, which
    /// a cell twice that wide keeps small; the cast saturates rather than wrapping, so even a
    /// degenerate tolerance gives a cell rather than a wrapped one.
    fn cell_of(&self, v: Vec3) -> (i64, i64, i64) {
        (
            ((v.x - self.origin.x) / self.cell).floor() as i64,
            ((v.y - self.origin.y) / self.cell).floor() as i64,
            ((v.z - self.origin.z) / self.cell).floor() as i64,
        )
    }

    /// The number of the canonical corner for `v`, if one is already within `tol` of it.
    fn find_index(&self, v: Vec3) -> Option<usize> {
        let (cx, cy, cz) = self.cell_of(v);
        let mut best: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    // Saturating, not wrapping: a coordinate from a file can index a cell at the
                    // very end of the range, and `cx + 1` must be a cell that is simply empty rather
                    // than an overflow. This runs on every vertex of every shell the crate checks, so
                    // it has to be a place a hostile coordinate cannot break.
                    let key = (cx.saturating_add(dx), cy.saturating_add(dy), cz.saturating_add(dz));
                    let Some(list) = self.buckets.get(&key) else { continue };
                    for &i in list {
                        let Some(p) = self.corners.get(i) else { continue };
                        if p.dist(v) <= self.tol && best.is_none_or(|b| i < b) {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best
    }

    /// The canonical corner for `v`, if one is already within `tol` of it.
    fn find(&self, v: Vec3) -> Option<Vec3> {
        self.find_index(v).and_then(|i| self.corners.get(i).copied())
    }

    /// The canonical corner for `v`, registering `v` itself if nothing is within `tol` of it.
    fn canonical(&mut self, v: Vec3) -> Vec3 {
        if let Some(hit) = self.find(v) {
            return hit;
        }
        let key = self.cell_of(v);
        self.corners.push(v);
        self.buckets.entry(key).or_default().push(self.corners.len() - 1);
        v
    }

    /// The *number* of the canonical corner for `v`, registering `v` itself if nothing within `tol`
    /// of it does.
    ///
    /// The number is the identity two points share, and it is the only form of it that survives a
    /// cell boundary: `vkey` hands back a pair of points that are within a rounding step of being
    /// the same corner and will disagree about it the moment one of them crosses the line, while two
    /// points that name the same corner here are the same corner whatever cells they sit in. Every
    /// vertex-level decision in the crate — is this edge shared, is this vertex pinched, is this
    /// shell closed — is made on these numbers.
    fn index_of(&mut self, v: Vec3) -> usize {
        if let Some(hit) = self.find_index(v) {
            return hit;
        }
        let key = self.cell_of(v);
        self.corners.push(v);
        let n = self.corners.len() - 1;
        self.buckets.entry(key).or_default().push(n);
        n
    }
}

/// Snap every vertex onto a shared grid and split every face edge at any vertex that lands on it.
///
/// Two faces share a boundary when their edges coincide. When one face has been split at a vertex
/// the other has not — because the other was merely *touched* there, rather than cut — the two
/// edges no longer match, and the shell reports an edge used once. This is that mismatch's repair.
///
/// Snapping comes first, and is not optional. The same corner arrives at two faces by different
/// routes and lands a ten-millionth of a unit apart; split one face at that corner and not the
/// other and the weld has *created* the mismatch it was meant to remove. So every vertex is first
/// moved onto the number [`WeldGrid`] holds for it, which is the same number in every face that has
/// it.
///
/// The inserted vertices are collinear, so no region changes shape and no area or volume moves; the
/// point is that the *edges* then match their neighbours, which is what the shell is made of.
///
/// # What the snap guarantees, and what it cannot
///
/// It guarantees one `Vec3` per corner, written into every face that holds that corner. It cannot
/// guarantee one *world* bit pattern per corner, and the difference matters. A face stores its loop
/// in its own plane's frame, so the canonical number is written back through `Plane::to_local` and
/// read again through `Plane::to_world` — and that round trip multiplies, adds and normalises, so
/// it is not the identity. Two faces come out of this pass holding copies of one corner a unit in
/// the last place apart, which is six orders of magnitude inside `tol` and is exactly what
/// `tol` exists to absorb.
///
/// Anything that then decides "are these two points the same vertex?" by rounding a quotient has to
/// cope with that. Rounding cannot: the cell boundary sits half a cell inside each cell, and two
/// points a unit in the last place apart fall either side of it exactly when the drawing happens to
/// put one there — which is what a blend whose distance is a power of two does, because every
/// coordinate it makes is exact and lands exactly on the line. So the identity the shell's own
/// invariants use is `WeldGrid`'s corner *number*, found by distance over the corners rather than by
/// a single rounded index; see `vkey` and `Solid::validate`. Quantising the corner instead, to a
/// coarser lattice, would move geometry by up to half a cell and is not offered here: the pass is
/// required to leave a shell that has nothing to weld exactly as it found it.
///
/// Quadratic in faces × edges × vertices, and skipped above [`MAX_WELD_VERTICES`] rather than
/// spending a minute on a body nobody can look at.
// Boundary-safe by construction: identity is resolved by proximity over the twenty-six
// neighbouring cells of a `2 * tol` lattice (option (b) from the cell-straddle analysis), never
// by a single rounded index per axis — that index has a line two copies of one corner can fall
// either side of. Registration runs in geometry-derived order so the canonical corner is the
// smallest one in each cluster, and inserted points are snapped to the canonical corner itself
// instead of to a projection.
pub fn weld_edges(faces: &mut [Face], tol: f64) -> usize {
    let total: usize = faces.iter().map(|f| f.vertices().len()).sum();
    if total > crate::limits::MAX_WELD_VERTICES {
        return 0;
    }
    // A hostile tolerance is not a reason to skip the weld; it is a reason to use the floor the rest
    // of the kernel uses.
    let tol = if tol.is_finite() && tol > 0.0 { tol } else { 1e-12 };
    // Non-finite corners cannot be welded to anything, and rounding one into a cell would make the
    // loop arithmetic below produce NaN where it had an infinity. They are left exactly as they
    // were; validation, not this pass, decides what an unusable solid is.
    // Registration order must not decide identity: two points that name the same corner within
    // `tol` of each other must resolve to the same canonical corner no matter which face order a
    // builder happened to emit them in. Sorting the vertices into a fixed, geometry-derived order
    // makes the canonical point the lexicographically smallest corner in each tolerance cluster —
    // the `2 * tol` cell is only a bucket for finding neighbours, never the identity — which is
    // what option (b) (search the neighbouring cells) needs to be boundary-safe rather than
    // first-writer-wins. This is the cell-straddle lesson: a single rounded index per axis cannot
    // be the verdict, because it has a line for two copies of one corner to fall on either side of.
    let mut all: Vec<Vec3> = faces.iter().flat_map(|f| f.vertices()).filter(|v| v.is_finite()).collect();
    all.sort_by(|u, v| u.x.total_cmp(&v.x).then(u.y.total_cmp(&v.y)).then(u.z.total_cmp(&v.z)));
    let Some(first) = all.first().copied() else { return 0 };
    // The shell's own corner, not `Vec3::ZERO` clamped: every cell index is then small and
    // non-negative whatever the drawing's coordinates are.
    let origin = all.iter().fold(first, |acc, v| acc.min(*v));
    let mut grid = WeldGrid::new(tol, origin);
    for v in &all {
        grid.canonical(*v);
    }
    for f in faces.iter_mut() {
        let plane = f.plane;
        for region in f.region.regions_mut() {
            for loop_pts in std::iter::once(&mut region.outer).chain(region.holes.iter_mut()) {
                for p in loop_pts.iter_mut() {
                    let w = plane.to_world(p.x, p.y);
                    if !w.is_finite() {
                        continue;
                    }
                    *p = plane.to_local(grid.canonical(w));
                }
            }
        }
    }
    let vertices: &[Vec3] = &grid.corners;
    // Phase 2: split edges at the vertices that lie on them.
    let mut added = 0;
    for f in faces.iter_mut() {
        let plane = f.plane;
        let mut splits = 0;
        for region in f.region.regions_mut() {
            for loop_pts in std::iter::once(&mut region.outer).chain(region.holes.iter_mut()) {
                let n = loop_pts.len();
                if n < 3 {
                    continue;
                }
                let mut out: Vec<Vec2> = Vec::with_capacity(n);
                for i in 0..n {
                    let a = loop_pts[i];
                    let b = loop_pts[(i + 1) % n];
                    out.push(a);
                    let dir = b - a;
                    let len2 = dir.len2();
                    if len2 <= tol * tol {
                        continue;
                    }
                    // Vertices strictly between `a` and `b`, and on the segment to within `tol`.
                    let mut inner: Vec<(f64, Vec2)> = Vec::new();
                    for v in vertices {
                        // Only a vertex *in this face's plane* may split one of its edges. Testing
                        // the projection alone is not enough, and it is quietly wrong for every
                        // axis-aligned part: a vertex off the plane projects exactly onto the
                        // edges, so a box would sprout midpoints along all of them.
                        if plane.distance(*v).abs() > tol {
                            continue;
                        }
                        let w = plane.to_local(*v) - a;
                        let t = w.dot(dir) / len2;
                        if t <= 0.0 || t >= 1.0 {
                            continue;
                        }
                        let off = (w - dir * t).len();
                        if off > tol {
                            continue;
                        }
                        inner.push((t, plane.to_local(*v))); // boundary-safe: canonical grid point, not a projection
                    }
                    if inner.is_empty() {
                        continue;
                    }
                    inner.sort_by(|x, y| x.0.total_cmp(&y.0));
                    let mut last = 0.0;
                    let mut last_p: Option<Vec2> = None;
                    for (t, p) in inner {
                        if t - last <= f64::EPSILON {
                            continue;
                        }
                        // Two corners a hair apart project onto two points a hair apart along the
                        // edge, and inserting both leaves a sliver edge of no length: the boundary is
                        // "used" by a segment that is really the same segment twice.
                        if last_p.is_some_and(|q| q.dist(p) <= tol) {
                            continue;
                        }
                        last = t;
                        last_p = Some(p);
                        out.push(p);
                        splits += 1;
                    }
                }
                *loop_pts = out;
            }
        }
        added += splits;
    }
    added
}

#[cfg(test)]
mod invariant_tests {
    use super::*;
    use crate::tests_support::box_at;

    fn flat_face(normal: Vec3) -> Option<Face> {
        let plane = Plane::new(Vec3::ZERO, normal).ok()?;
        Face::new(plane, Regions::new(vec![Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)])]))
            .ok()
    }

    #[test]
    fn flipping_a_face_reverses_its_loop_as_well_as_its_normal() {
        // The loop's winding *is* what makes the normal point out of the material. Reversing only
        // the normal leaves a face whose area comes out negative, which is how a flipped face once
        // quietly subtracted material instead of adding it.
        let Some(face) = flat_face(Vec3::Z) else { return };
        let flipped = match face.flipped() {
            Ok(f) => f,
            Err(_) => return,
        };
        assert!((flipped.plane.normal.dot(face.plane.normal) + 1.0).abs() < 1e-9, "the normal turned around");
        assert!(flipped.region.iter().next().is_some_and(|r| r.signed_area() > 0.0), "the loop is still counter-clockwise in its own frame");
        assert!((flipped.area() - 16.0).abs() < 1e-9, "the same shape, just the other way round");
        // Same geometry: the points are there, only in the other order.
        let mut a = face.vertices();
        let mut b = flipped.vertices();
        a.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(p.z.total_cmp(&q.z)));
        b.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(p.z.total_cmp(&q.z)));
        assert_eq!(a, b, "flipping moves nothing");
    }

    #[test]
    fn a_shell_with_a_hole_is_not_a_solid() {
        // Three faces of a box: closed on two sides, open on the third. Every edge rule still holds
        // except the last, and that is exactly what has to catch it.
        let b = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let mut faces = b.faces.clone();
        faces.pop();
        let s = Solid::from_faces(faces).unwrap_or_default();
        match s.validate() {
            Err(SolidError::OpenEdge(_, _, n)) => assert_eq!(n, 1, "one face is missing"),
            Err(other) => panic!("expected an open edge, got {other}"),
            Ok(()) => panic!("a shell with a hole passed validation"),
        }
    }

    #[test]
    fn a_shell_pointing_inward_is_not_a_solid() {
        // Closed, consistent, but inside out: the signed volume comes out negative. This is the
        // check that catches a boolean that got the ownership rule backwards.
        let b = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let inside_out = b.flipped().unwrap_or_else(|_| b.clone());
        match inside_out.validate() {
            Err(SolidError::NonPositiveVolume(v)) => assert!(v < 0.0, "a negative volume, got {v}"),
            Err(other) => panic!("expected a non-positive volume, got {other}"),
            Ok(()) => panic!("a shell pointing inward passed validation"),
        }
        assert!((inside_out.volume() + b.volume()).abs() < 1e-9, "the same material, the other way round: {} vs {}", inside_out.volume(), b.volume());
    }

    #[test]
    fn two_boxes_touching_at_a_face_are_not_one_solid() {
        // Closed and consistently oriented, but pinched: the fans around the shared vertices are
        // two cycles instead of one. Only the vertex check catches this, which is why it exists.
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let b = box_at(Vec3::new(2.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 2.0));
        let mut faces = a.faces.clone();
        faces.extend(b.faces.iter().cloned());
        let joined = Solid::from_faces(faces).unwrap_or_default();
        match joined.validate_vertices() {
            Err(e) => assert!(e.to_string().contains("pinched"), "expected a pinched vertex, got {e}"),
            Ok(()) => panic!("two boxes touching at a face are not one manifold solid"),
        }
    }
}

#[cfg(test)]
mod weld_tests {
    use super::*;
    use crate::tests_support::box_at;

    fn square_on(plane: Plane, points: [Vec3; 4]) -> Option<Face> {
        Face::new(plane, Regions::new(vec![Region::new(points.iter().map(|p| plane.to_local(*p)).collect())])).ok()
    }

    /// The face of a shell whose plane carries `p`, so a test can name a face by its geometry
    /// rather than by the order a builder happened to emit it in.
    ///
    /// `p` has to be away from the face's own edges, or it lies on two planes at once: `(0.5, 0, 0)`
    /// is on `y = 0` *and* on `z = 0`, and picking the wrong one silently tests nothing.
    fn face_carrying(faces: &[Face], p: Vec3) -> Option<usize> {
        faces.iter().position(|f| f.plane.distance(p).abs() <= 1e-9)
    }

    /// Where `world` sits in a face's first loop, found through the face's own frame so the answer
    /// does not depend on how the face was built.
    fn loop_vertex(f: &Face, world: Vec3) -> Option<usize> {
        let plane = f.plane;
        f.region.loops().next()?.iter().position(|p| plane.to_world(p.x, p.y).near(world, 1e-9))
    }

    /// Move one loop vertex to `world`, through the face's own frame — the same round trip
    /// `weld_edges` writes a canonical corner back through, and therefore the same round trip that
    /// leaves two faces' copies of one corner a unit in the last place apart.
    fn move_loop_vertex(f: &mut Face, index: usize, world: Vec3) -> bool {
        let plane = f.plane;
        let Some(region) = f.region.regions_mut().first_mut() else { return false };
        let Some(slot) = region.outer.get_mut(index) else { return false };
        *slot = plane.to_local(world);
        true
    }

    /// The same, for a new vertex *between* two of the loop's own.
    ///
    /// Only useful on an edge: a vertex off the line changes the region, and this pass is required
    /// never to.
    fn insert_loop_vertex(f: &mut Face, index: usize, world: Vec3) -> bool {
        let plane = f.plane;
        let Some(region) = f.region.regions_mut().first_mut() else { return false };
        if index > region.outer.len() {
            return false;
        }
        region.outer.insert(index, plane.to_local(world));
        true
    }

    #[test]
    fn welding_inserts_a_vertex_that_lands_on_another_faces_edge() {
        // The flat face spans an edge whole; a face standing on that edge already has a vertex in
        // the middle of it. Left alone, the two do not share an edge any more and the shell reads
        // as having one used once — which is exactly what a blend tool touching a face used to do.
        let flat = match Plane::new(Vec3::ZERO, Vec3::Z) {
            Ok(p) => p,
            Err(_) => return,
        };
        let upright = match Plane::new(Vec3::ZERO, Vec3::Y) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(a) = square_on(flat, [Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 0.0), Vec3::new(0.0, 4.0, 0.0)]) else {
            return;
        };
        let Some(b) = square_on(upright, [Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 2.0), Vec3::ZERO]) else {
            return;
        };
        let mut faces = vec![a, b];
        assert_eq!(weld_edges(&mut faces, 1e-9), 1, "the point at (2, 0, 0) splits the flat face's edge");
        let flat_edges = faces[0].edges();
        assert!(
            flat_edges.iter().any(|(p, q)| p.near(Vec3::new(2.0, 0.0, 0.0), 1e-9) || q.near(Vec3::new(2.0, 0.0, 0.0), 1e-9)),
            "the flat face now has a vertex there: {flat_edges:?}"
        );
    }

    #[test]
    fn welding_ignores_a_vertex_that_is_not_in_the_face() {
        // A vertex one unit *above* the plane projects exactly onto the middle of that face's
        // edge, because boxes are axis-aligned. Projecting without checking the plane first is how
        // a solid grew midpoints along every edge it had.
        let flat = match Plane::new(Vec3::ZERO, Vec3::Z) {
            Ok(p) => p,
            Err(_) => return,
        };
        let upper = match Plane::new(Vec3::new(0.0, 0.0, 5.0), Vec3::Z) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(a) = square_on(flat, [Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 0.0), Vec3::new(0.0, 4.0, 0.0)]) else {
            return;
        };
        let Some(b) = square_on(upper, [Vec3::new(2.0, 0.0, 5.0), Vec3::new(6.0, 0.0, 5.0), Vec3::new(6.0, 4.0, 5.0), Vec3::new(2.0, 4.0, 5.0)])
        else {
            return;
        };
        let mut faces = vec![a, b];
        assert_eq!(weld_edges(&mut faces, 1e-9), 0, "a vertex in another plane cannot split this face");
    }

    #[test]
    fn welding_changes_no_area_or_volume() {
        // The inserted points are collinear, so a welded solid is the same solid. Anything else
        // would mean the pass moves geometry, which it must never do.
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let mut faces = b.faces.clone();
        let before_area: f64 = faces.iter().map(|f| f.area()).sum();
        let before_volume = b.volume();
        let before: Vec<Vec<Vec2>> = b.faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        weld_edges(&mut faces, b.tolerance());
        let after_area: f64 = faces.iter().map(|f| f.area()).sum();
        assert!((after_area - before_area).abs() < 1e-9, "area moved: {before_area} -> {after_area}");
        let after = Solid::from_faces(faces).unwrap_or_default();
        assert!((after.volume() - before_volume).abs() < 1e-9, "volume moved: {before_volume} -> {}", after.volume());
        let after_loops: Vec<Vec<Vec2>> = after.faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(before, after_loops, "a box has nothing to weld, and comes back the same loops");
    }

    /// Two faces that share one corner, and give it two numbers a millionth of a tolerance apart.
    ///
    /// On a tolerance-sized grid the rounding boundary sits half a cell inside each cell, so those
    /// two numbers land either side of it: each face keeps its own, the corner is written twice, and
    /// the weld hands back the mismatch it was called to remove. This is the boundary case, and it is
    /// the one a cell-keyed lookup cannot see.
    #[test]
    fn welding_joins_two_corners_that_straddle_a_cell_boundary() {
        let tol = 1e-9;
        // `0.5e-9` rounds onto the next cell of a tolerance-sized grid and `0.5e-9 - 1e-15` does not.
        // They are a millionth of a tolerance apart, which is nothing at all.
        let hi = 0.5e-9;
        let lo = 0.5e-9 - 1e-15;
        let upright = match Plane::new(Vec3::ZERO, Vec3::Y) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(a) =
            square_on(Plane::xy(0.0), [Vec3::new(lo, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 0.0), Vec3::new(lo, 2., 0.0)])
        else {
            return;
        };
        let Some(b) = square_on(upright, [Vec3::new(hi, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 2.0), Vec3::new(hi, 0.0, 2.0)])
        else {
            return;
        };
        let mut faces = vec![a, b];
        let added = weld_edges(&mut faces, tol);
        // One number for one corner, in both faces: the first one registered, which is the lower of
        // the two.
        let shared = Vec3::new(lo, 0.0, 0.0);
        assert!(faces[0].vertices().iter().any(|v| v.near(shared, 1e-18)), "the flat face kept its own corner: {:?}", faces[0].vertices());
        assert!(faces[1].vertices().iter().any(|v| v.near(shared, 1e-18)), "and the upright face kept the other: {:?}", faces[1].vertices());
        assert!(!faces[1].vertices().iter().any(|v| v.near(Vec3::new(hi, 0.0, 0.0), 1e-18)), "the second number is still in the shell");
        // And the weld still does its other half: the upright face's base edge lies along the flat
        // face's, so the flat face has to be cut at (2, 0, 0) or the two edges no longer match.
        assert!(added >= 1, "nothing was split: {added}");
        assert!(
            faces[0].vertices().iter().any(|v| v.near(Vec3::new(2.0, 0.0, 0.0), 1e-18)),
            "the flat face was not cut at (2, 0, 0): {:?}",
            faces[0].vertices()
        );
    }

    #[test]
    fn welding_twice_changes_nothing_more() {
        // The pass runs its search twice over the same corners — once to register them, once to
        // rewrite the loops against them — and both passes have to reach the same verdict, or a
        // corner is snapped on the way in and left alone on the way out.
        let upright = match Plane::new(Vec3::ZERO, Vec3::Y) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(a) = square_on(Plane::xy(0.0), [Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)]) else {
            return;
        };
        let Some(b) =
            square_on(upright, [Vec3::new(0.5e-9, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 2.0), Vec3::new(0.5e-9, 0.0, 2.0)])
        else {
            return;
        };
        let mut faces = vec![a, b];
        weld_edges(&mut faces, 1e-9);
        let after_first: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(weld_edges(&mut faces, 1e-9), 0, "a welded shell has nothing left to split");
        let after_second: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(after_first, after_second, "the second pass moved a corner");
    }

    #[test]
    fn welding_leaves_corners_further_apart_than_the_tolerance_alone() {
        // The search is bounded by the tolerance, not by the cell. Two corners four tolerances apart
        // are two corners, however close their cells, and a weld that merged them would silently
        // move one face's boundary onto another's.
        let tol = 1e-9;
        let Some(a) = square_on(Plane::xy(0.0), [Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)]) else {
            return;
        };
        let gap = 2.0 + 4.0 * tol;
        let Some(b) =
            square_on(Plane::xy(0.0), [Vec3::new(gap, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 0.0), Vec3::new(gap, 2.0, 0.0)])
        else {
            return;
        };
        let mut faces = vec![a, b];
        let before: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(weld_edges(&mut faces, tol), 0, "nothing lies on anything else's edge");
        let after: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(before, after, "corners four tolerances apart were merged");
    }

    /// One cube, one of its corners held by the face on `y = 0` *half a tolerance* from where the
    /// other two faces hold it — a displacement that lands exactly on a rounding boundary.
    ///
    /// This is the shell-level statement the boundary case needs: everything within `tol` is one
    /// point by the kernel's own definition, so the shell is the same shell either way, and the only
    /// thing that can tell the difference is how the shell decides that two numbers name one corner.
    /// A rounded index per axis cannot: `half / tol` is `0.5` to the bit, so the two copies are in
    /// cells `0` and `1` and the edge they share stops being shared — the shell reads as open.
    ///
    /// The displacement is `tol / 2` rather than a unit in the last place because it can be placed
    /// exactly. A last-bit displacement is the realistic one and the same code path; it cannot be
    /// aimed at the boundary without knowing where the boundary is, which is the whole difficulty.
    #[test]
    fn a_corner_held_half_a_tolerance_away_is_still_one_vertex() {
        let b = box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let tol = b.tolerance();
        let half = tol * 0.5;
        let mut faces = b.faces.clone();
        // The face on `y = 0`, named by a point only that plane carries.
        let Some(y0) = face_carrying(&faces, Vec3::new(0.5, 0.0, 0.5)) else { return };
        let Some(at) = loop_vertex(&faces[y0], Vec3::ZERO) else { return };
        if !move_loop_vertex(&mut faces[y0], at, Vec3::new(half, 0.0, 0.0)) {
            return;
        }
        // Preconditions, asserted rather than assumed: this is the case only if the keying bound is
        // the origin and the two copies really do straddle a cell.
        let s = Solid::from_faces(faces).unwrap_or_default();
        assert!(s.bounds().min.near(Vec3::ZERO, 1e-12), "nothing moved below the origin, so the bound still is: {:?}", s.bounds().min);
        assert_ne!(
            vkey(Vec3::new(half, 0.0, 0.0), Vec3::ZERO, tol),
            vkey(Vec3::ZERO, Vec3::ZERO, tol),
            "the two copies must land in different cells or this test says nothing"
        );
        match s.validate() {
            Ok(()) => {}
            Err(e) => panic!("a corner {half} from its twin is still one corner: {e}"),
        }
        assert!((s.volume() - 1.0).abs() < 1e-8, "the cube lost or gained material: {}", s.volume());
        assert!((s.area() - 6.0).abs() < 1e-8, "its skin moved: {}", s.area());
    }

    /// The same straddling corner, through the weld: the pass has to give both faces one number, and
    /// a body it only re-reads must come back the body it was given.
    #[test]
    fn welding_a_corner_that_lands_on_a_cell_boundary_moves_no_area_and_no_volume() {
        let b = box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let tol = b.tolerance();
        let half = tol * 0.5;
        let mut faces = b.faces.clone();
        let Some(y0) = face_carrying(&faces, Vec3::new(0.5, 0.0, 0.5)) else { return };
        let Some(at) = loop_vertex(&faces[y0], Vec3::ZERO) else { return };
        if !move_loop_vertex(&mut faces[y0], at, Vec3::new(half, 0.0, 0.0)) {
            return;
        }
        assert_ne!(
            vkey(Vec3::new(half, 0.0, 0.0), Vec3::ZERO, tol),
            vkey(Vec3::ZERO, Vec3::ZERO, tol),
            "the corner has to straddle a cell or this test says nothing"
        );
        // Moved face last, so the corner it lost is registered first and is therefore the number
        // the other two faces keep. The pass answers with the *first* corner within tolerance, and
        // which one that is must not be down to the order a builder emitted its faces in — so the
        // test pins the order down rather than inheriting it.
        let moved = faces.remove(y0);
        faces.push(moved);
        weld_edges(&mut faces, tol);
        // One number for the corner, in the face that was moved and in the two that were not. They
        // are not bit-identical — each face reads its own local copy back through its own frame —
        // but they must be within a thousandth of a tolerance of each other, and not half a
        // tolerance: that gap is the whole defect.
        let after = Solid::from_faces(faces).unwrap_or_default();
        let mut corner: Option<Vec3> = None;
        for f in &after.faces {
            let verts = f.vertices();
            let found = verts.iter().find(|v| v.len() <= tol);
            let Some(first) = corner else {
                if let Some(v) = found {
                    corner = Some(*v);
                }
                continue;
            };
            let Some(v) = found else { continue };
            assert!(
                (*v - first).len() <= tol * 1e-3,
                "a face kept its own copy of the corner, {} from its neighbour's: {first:?} vs {v:?}",
                (*v - first).len()
            );
        }
        assert!(corner.is_some(), "no face holds the corner at all");
        // The welded cube is the cube: the weld moves a *number*, never a point, so the only way it
        // could come back different is by not resolving the straddle at all.
        match after.validate() {
            Ok(()) => {}
            Err(e) => panic!("the welded cube is closed: {e}"),
        }
        assert!((after.volume() - b.volume()).abs() < 1e-12, "welding moved volume: {} -> {}", b.volume(), after.volume());
        assert!((after.area() - b.area()).abs() < 1e-12, "welding moved area: {} -> {}", b.area(), after.area());
        assert_eq!(after.faces.len(), b.faces.len(), "welding invented or dropped a face");
    }

    /// A closed body whose faces disagree by a vertex, welded — twice — with nothing moving.
    ///
    /// The face on `y = 0` carries a corner halfway along its bottom edge that the face on `z = 0`
    /// has not got, so that neighbour's edge has to be cut there before the two faces share it. The
    /// cut point is on the line, so no area and no volume may move; and a second pass has to find
    /// nothing left to do and change nothing at all, which is the only way to see that the pass's
    /// two searches — one to register the corners, one to rewrite the loops against them — reached
    /// the same verdict both times.
    #[test]
    fn welding_a_missing_corner_onto_a_closed_body_is_idempotent_and_moves_no_area_or_volume() {
        let b = box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let tol = b.tolerance();
        let mut faces = b.faces.clone();
        let Some(y0) = face_carrying(&faces, Vec3::new(0.5, 0.0, 0.5)) else { return };
        let (Some(at), Some(far)) = (loop_vertex(&faces[y0], Vec3::ZERO), loop_vertex(&faces[y0], Vec3::new(1.0, 0.0, 0.0))) else { return };
        // Between (0,0,0) and (1,0,0) — the edge the face on `z = 0` shares and has not been cut. The
        // loop is cyclic, so the slot between two neighbours is "one past the first of them", taken
        // modulo the loop's length; without the wrap a face that happens to start on that edge
        // would leave this test silently doing nothing.
        let n = faces[y0].region.loops().next().map_or(0, |l| l.len());
        if n < 4 {
            return;
        }
        let next_of = |i: usize| (i + 1) % n;
        let between = if next_of(at) == far {
            next_of(at)
        } else if next_of(far) == at {
            next_of(far)
        } else {
            return;
        };
        if !insert_loop_vertex(&mut faces[y0], between, Vec3::new(0.5, 0.0, 0.0)) {
            return;
        }
        assert_eq!(weld_edges(&mut faces, tol), 1, "the missing corner has to cut the neighbour's edge, and only that one");

        let once = Solid::from_faces(faces.clone()).unwrap_or_default();
        match once.validate() {
            Ok(()) => {}
            Err(e) => panic!("the welded cube is closed: {e}"),
        }
        assert_eq!(once.edges().map(|e| e.len()), Some(13), "a cube has twelve edges, and the one that was cut became two");
        assert!((once.volume() - b.volume()).abs() < 1e-12, "cutting an edge in two moved volume: {} -> {}", b.volume(), once.volume());
        assert!((once.area() - b.area()).abs() < 1e-12, "cutting an edge in two moved area: {} -> {}", b.area(), once.area());

        // And again: nothing to split, and not one loop touched.
        let loops: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(weld_edges(&mut faces, tol), 0, "a welded shell has nothing left to split");
        assert_eq!(loops, faces.iter().flat_map(|f| f.region.loops().cloned()).collect::<Vec<Vec<Vec2>>>(), "the second pass moved a corner");
        let twice = Solid::from_faces(faces).unwrap_or_default();
        assert!((twice.volume() - once.volume()).abs() < 1e-12, "the second pass moved volume: {} -> {}", once.volume(), twice.volume());
        assert!((twice.area() - once.area()).abs() < 1e-12, "the second pass moved area: {} -> {}", once.area(), twice.area());
    }

    /// A vertex that lands exactly on a cell boundary of the welding grid is welded the same
    /// way as a vertex a hair to either side of it: the boundary is never the verdict.
    ///
    /// This is the cell-straddle from `KER-BOOL-004`, stated for `weld_edges` itself rather than
    /// for validation. The grid buckets on a `2 * tol` lattice and resolves identity by proximity
    /// over the twenty-six neighbouring cells (option (b)), so a point sitting exactly on that
    /// lattice still finds the corner it belongs to instead of being rounded into a cell of its own.
    #[test]
    fn welding_a_vertex_on_a_cell_boundary_moves_no_area_or_volume() {
        let b = box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let tol = b.tolerance();
        // A corner half a tolerance away is, by `vkey`'s toleranced grid, on a cell boundary.
        let half = tol * 0.5;
        let mut faces = b.faces.clone();
        let Some(y0) = face_carrying(&faces, Vec3::new(0.5, 0.0, 0.5)) else { return };
        let Some(at) = loop_vertex(&faces[y0], Vec3::ZERO) else { return };
        if !move_loop_vertex(&mut faces[y0], at, Vec3::new(half, 0.0, 0.0)) {
            return;
        }
        assert_ne!(
            vkey(Vec3::new(half, 0.0, 0.0), Vec3::ZERO, tol),
            vkey(Vec3::ZERO, Vec3::ZERO, tol),
            "the corner must straddle a cell or this test says nothing"
        );

        let baseline_volume = Solid::from_faces(faces.clone()).unwrap_or_default().volume();
        let baseline_area: f64 = faces.iter().map(|f| f.area()).sum();

        weld_edges(&mut faces, tol);

        let after_volume = Solid::from_faces(faces.clone()).unwrap_or_default().volume();
        let after_area: f64 = faces.iter().map(|f| f.area()).sum();
        assert!((after_volume - baseline_volume).abs() < 1e-8, "welding moved volume across the boundary: {baseline_volume} -> {after_volume}");
        assert!((after_area - baseline_area).abs() < 1e-8, "welding moved area across the boundary: {baseline_area} -> {after_area}");
    }

    /// Welding a second time after the first pass must find nothing left to do and leave every
    /// loop byte-for-byte: a pass that leaves a move for its successor never finishes.
    #[test]
    fn welding_twice_is_idempotent_and_preserves_volume_and_area() {
        let b = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let tol = b.tolerance();
        let mut faces = b.faces.clone();
        let before_volume = b.volume();
        let before_area = b.area();

        weld_edges(&mut faces, tol);
        let after_first_loops: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        let after_first_volume = Solid::from_faces(faces.clone()).unwrap_or_default().volume();
        let after_first_area: f64 = faces.iter().map(|f| f.area()).sum();

        assert_eq!(weld_edges(&mut faces, tol), 0, "a welded shell has nothing left to split");
        let after_second_loops: Vec<Vec<Vec2>> = faces.iter().flat_map(|f| f.region.loops().cloned()).collect();
        assert_eq!(after_first_loops, after_second_loops, "the second pass moved a corner");
        let after_second_volume = Solid::from_faces(faces.clone()).unwrap_or_default().volume();
        let after_second_area: f64 = faces.iter().map(|f| f.area()).sum();
        assert!((after_second_volume - after_first_volume).abs() < 1e-12, "the second pass moved volume");
        assert!((after_second_area - after_first_area).abs() < 1e-12, "the second pass moved area");
        assert!((after_first_volume - before_volume).abs() < 1e-9, "welding moved volume");
        assert!((after_first_area - before_area).abs() < 1e-9, "welding moved area");
    }
}

/// A closed shell of planar faces.
///
/// Faces carry no topology of their own; the shared boundary between faces is the topology, and
/// [`Solid::validate`] is what proves it is a shell. Faces are ordered, and indices into
/// `faces` are the only identity a face has inside one operation — across operations (a feature
/// tree editing a body) references are geometric, which is documented where they are made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solid {
    pub faces: Vec<Face>,
}

impl Solid {
    /// Wrap faces, checking only the cheap invariants (finite, non-empty, capped).
    ///
    /// Topology is deliberately *not* checked here: building a solid is allowed to fail later,
    /// in [`Solid::validate`], so an operation can assemble candidates and validate once at the
    /// end instead of after every face.
    pub fn from_faces(faces: Vec<Face>) -> Result<Solid, SolidError> {
        if faces.is_empty() {
            return Err(SolidError::Empty);
        }
        if faces.len() > MAX_FACES {
            return Err(SolidError::TooManyFaces(faces.len(), MAX_FACES));
        }
        Ok(Solid { faces })
    }

    pub fn bounds(&self) -> Bounds3 {
        Bounds3::from_points(self.faces.iter().flat_map(|f| f.vertices()))
    }

    /// The same shell with every face turned the other way: the same solid seen from inside.
    ///
    /// Its volume comes out negative, which is the cheapest way to see that orientation is a real
    /// property and not a convention: the same faces, the same numbers, the other way round.
    pub fn flipped(&self) -> Result<Solid, SolidError> {
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            faces.push(f.flipped()?);
        }
        Solid::from_faces(faces)
    }

    /// Working tolerance: scaled to the solid so the same shape behaves the same at every size.
    pub fn tolerance(&self) -> f64 {
        tolerance_for(self.bounds().diagonal())
    }

    /// Volume, exact for a polyhedron. See [`crate::mass`].
    pub fn volume(&self) -> f64 {
        mass_properties(self).map(|m| m.volume).unwrap_or(0.0)
    }

    pub fn area(&self) -> f64 {
        self.faces.iter().map(|f| f.area()).sum()
    }

    /// Is a world point inside the material?
    ///
    /// Ray casting along +X. Faces whose plane the ray runs parallel to are skipped, which is the
    /// measure-zero case; a point exactly on a face is reported by the caller's own tolerance
    /// rules, not here.
    pub fn contains(&self, p: Vec3) -> bool {
        let dir = Vec3::X;
        let mut inside = false;
        for f in &self.faces {
            let n = f.plane.normal.dot(dir);
            if n.abs() < 1e-12 {
                continue;
            }
            // Where the ray `p + t * dir` meets this face's plane:
            // (p + t * dir - origin) . normal = 0  =>  t = (origin - p) . normal / (dir . normal)
            let t = (f.plane.origin - p).dot(f.plane.normal) / n;
            if t <= 0.0 {
                continue;
            }
            let q = p + dir * t;
            if f.contains_local(f.plane.to_local(q)) {
                inside = !inside;
            }
        }
        inside
    }

    /// The shell's edges, each shared by exactly two faces.
    ///
    /// Returns `None` when some edge is not shared by exactly two faces — an open shell has no
    /// well-defined edge list, and saying so is better than handing back a partial one.
    ///
    /// The ends of an edge are identified by `WeldGrid`'s corner numbers, not by a rounded cell
    /// index: a face and its neighbour hold the same corner as two numbers a unit in the last place
    /// apart, and two such numbers belong to the same corner whatever cells they fall in.
    pub fn edges(&self) -> Option<Vec<Edge>> {
        let tol = self.tolerance();
        let bb = self.bounds();
        let mut grid = WeldGrid::new(tol, bb.min);
        let mut usage: std::collections::HashMap<(usize, usize), Vec<(usize, Vec3, Vec3)>> = std::collections::HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                let (ka, kb) = (grid.index_of(a), grid.index_of(b));
                let key = if ka <= kb { (ka, kb) } else { (kb, ka) };
                usage.entry(key).or_default().push((fi, a, b));
            }
        }
        let mut out = Vec::with_capacity(usage.len());
        for (key, list) in usage {
            if list.len() != 2 {
                return None;
            }
            let (f0, a0, b0) = list.first().copied().unwrap_or((0, Vec3::ZERO, Vec3::ZERO));
            let (f1, _, _) = list.get(1).copied().unwrap_or((0, Vec3::ZERO, Vec3::ZERO));
            // Keep the direction of the lower face index.
            let (a, b) = if f0 <= f1 { (a0, b0) } else { list.get(1).map(|x| (x.1, x.2)).unwrap_or((a0, b0)) };
            let _ = key;
            out.push(Edge { a, b, faces: [f0.min(f1), f0.max(f1)] });
        }
        out.sort_by(|x, y| {
            x.a.x
                .total_cmp(&y.a.x)
                .then(x.a.y.total_cmp(&y.a.y))
                .then(x.a.z.total_cmp(&y.a.z))
                .then(x.b.x.total_cmp(&y.b.x))
                .then(x.b.y.total_cmp(&y.b.y))
                .then(x.b.z.total_cmp(&y.b.z))
        });
        Some(out)
    }

    /// Check the invariants: closed, consistently oriented, outward, finite.
    ///
    /// This is the gate every operation passes through. A solid that would come out open,
    /// inside-out or non-manifold is an error here rather than a body the renderer shows wrong.
    pub fn validate(&self) -> Result<(), SolidError> {
        if self.faces.is_empty() {
            return Err(SolidError::Empty);
        }
        let tol = self.tolerance();
        let bb = self.bounds();
        if bb.is_empty() {
            return Err(SolidError::Empty);
        }
        // I1: finite, non-empty faces.
        for f in &self.faces {
            if f.region.is_empty() {
                return Err(SolidError::Empty);
            }
            if !f.plane.origin.is_finite() || !f.plane.normal.is_finite() {
                return Err(SolidError::NonFinite);
            }
            for v in f.vertices() {
                if !v.is_finite() {
                    return Err(SolidError::NonFinite);
                }
            }
        }
        // I2 + I3: every edge exactly twice, once in each direction.
        //
        // A `BTreeMap`, not a `HashMap`: the report is the *first* offending edge in iteration
        // order, and hash order changes from run to run. A kernel that names a different broken
        // edge each time it is asked is one nobody can debug. The key is a pair of `WeldGrid` corner
        // numbers, so iteration order is the shell's own face-and-edge order — equally fixed from
        // run to run, and with no cell boundary in it for two copies of one corner to disagree
        // about.
        let mut grid = WeldGrid::new(tol, bb.min);
        let mut usage: std::collections::BTreeMap<(usize, usize), Vec<(usize, bool, Vec3, Vec3)>> = std::collections::BTreeMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                let (ka, kb) = (grid.index_of(a), grid.index_of(b));
                let forward = ka <= kb;
                let key = if forward { (ka, kb) } else { (kb, ka) };
                usage.entry(key).or_default().push((fi, forward, a, b));
            }
        }
        for list in usage.values() {
            let (a, b) = match (list.first(), list.get(1)) {
                (Some((_, _, a, _)), Some((_, _, _, b))) => (*a, *b),
                _ => {
                    let (a, b) = list.first().map(|(_, _, a, b)| (*a, *b)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
                    return Err(SolidError::OpenEdge(a, b, list.len()));
                }
            };
            if list.len() != 2 {
                return Err(SolidError::OpenEdge(a, b, list.len()));
            }
            let forward_count = list.iter().filter(|(_, fw, _, _)| *fw).count();
            if forward_count != 1 {
                return Err(SolidError::InconsistentFaces(format!("{a:?} -> {b:?} used {forward_count} times forward")));
            }
        }
        // I4: outward. A closed, consistently oriented shell has a definite signed volume; if it
        // comes out negative the faces point inward.
        let m = mass_properties(self)?;
        if m.volume <= 0.0 {
            return Err(SolidError::NonPositiveVolume(m.volume));
        }
        Ok(())
    }

    /// Invariant I5: every vertex has a single fan of faces around it.
    ///
    /// Edges being shared by exactly two faces gives manifoldness edge by edge, but a shell can
    /// still pinch: two lumps meeting at a single vertex pass every edge check and are not a
    /// surface. Here the faces around each vertex must form one cycle. If they form two, the
    /// shell touches itself there, and callers get told instead of a solid that renders with a
    /// seam.
    ///
    /// Quadratic in the vertex fan, so it is not on the default [`Solid::validate`] path.
    pub fn validate_vertices(&self) -> Result<(), SolidError> {
        let tol = self.tolerance();
        let bb = self.bounds();
        // vertex -> the faces touching it, with a representative position for the fan check.
        // Ordered, for the same reason `validate` is: the report names one vertex, and it must be
        // the same one every time. `WeldGrid` corner numbers, so the faces around a vertex land on
        // the same fan however far apart two copies of that vertex ended up.
        let mut grid = WeldGrid::new(tol, bb.min);
        let mut fans: std::collections::BTreeMap<usize, (Vec3, Vec<usize>)> = std::collections::BTreeMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for v in f.vertices() {
                fans.entry(grid.index_of(v)).or_insert((v, Vec::new())).1.push(fi);
            }
        }
        for (key, (v, faces)) in fans {
            let unique: Vec<usize> = {
                let mut u = faces.clone();
                u.sort_unstable();
                u.dedup();
                u
            };
            if unique.len() < 2 {
                continue;
            }
            if !fan_is_one_cycle(&self.faces, &unique, v, tol, &mut grid) {
                return Err(SolidError::Invalid(format!("vertex {key} has a pinched fan of {} faces", unique.len())));
            }
        }
        Ok(())
    }

    /// Turn every face outward, one face at a time.
    ///
    ///    /// A boolean mixes faces that came from different operands, and an operand's face normal is
    ///    /// outward for *that* operand — not necessarily for the result. Reasoning about which is
    ///    /// which per plane is exactly where sign errors hide, so instead each face is probed: a point
    ///    /// just off the face, on the normal side, must be outside the material. A face that fails is
    ///    /// flipped. Faces where the probe is ambiguous (a shell thinner than the probe) are left
    ///    /// alone rather than guessed at, and [`Solid::validate`] still has the last word.
    pub fn orient_faces(&mut self) -> Result<(), SolidError> {
        let mut flips: Vec<usize> = Vec::new();
        for i in 0..self.faces.len() {
            if self.face_points_inward(i) {
                flips.push(i);
            }
        }
        for i in flips {
            if let Some(f) = self.faces.get(i) {
                let flipped = f.flipped()?;
                self.faces[i] = flipped;
            }
        }
        Ok(())
    }

    /// Does face `i` point into the material?
    ///
    /// A local test, with no interior point to guess at: cast a ray from a point *on* the face,
    /// along the face's own normal, and count the other faces it crosses. An odd count means the
    /// ray starts inside the material, so this side of the face is the inside.
    ///
    /// A global reference point cannot be used here: the bounds centre of a plate with a pocket in
    /// it is in the pocket, and every face would then be judged against a point that is not in
    /// the material at all.
    fn face_points_inward(&self, i: usize) -> bool {
        let Some(f) = self.faces.get(i) else { return false };
        let Some(c) = f.interior_point() else { return false };
        let dir = f.plane.normal;
        let mut crossings = 0usize;
        for (j, g) in self.faces.iter().enumerate() {
            if j == i {
                continue;
            }
            let n = g.plane.normal.dot(dir);
            if n.abs() < 1e-12 {
                continue;
            }
            let t = (g.plane.origin - c).dot(g.plane.normal) / n;
            if t <= 0.0 {
                continue;
            }
            let q = c + dir * t;
            if g.contains_local(g.plane.to_local(q)) {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    }

    /// Flip every face if the shell came out inside-out.
    ///
    /// Orientation has to be *consistent* for the signed volume to mean anything, and measuring
    /// it is more reliable than reasoning about which way a sweep or a cut turned. Only call this
    /// on a shell whose faces agree with each other.
    pub fn fix_orientation(&mut self) -> Result<(), SolidError> {
        let v = mass_properties(self)?.volume;
        if v < 0.0 {
            for f in &mut self.faces {
                *f = f.flipped()?;
            }
        }
        Ok(())
    }

    /// Volume, centroid and inertia, exact for the polyhedron.
    pub fn mass_properties(&self) -> Result<MassProperties, SolidError> {
        mass_properties(self)
    }

    /// Number of faces, loops and boundary edges, for diagnostics.
    pub fn counts(&self) -> (usize, usize, usize) {
        let loops = self.faces.iter().map(|f| f.region.loops().count()).sum();
        let edges = self.faces.iter().map(|f| f.region.loops().map(|l| l.len()).sum::<usize>()).sum();
        (self.faces.len(), loops, edges)
    }

    /// A point inside the material, or `None` when the shell has no interior to speak of.
    ///
    /// Several candidates are tried because the obvious one can be outside: the centroid of a
    /// U-shape is in the notch. The mass centroid comes first, then the bounds centre, then
    /// midpoints towards each face, and the containment test decides.
    pub fn interior_point(&self) -> Option<Vec3> {
        let bb = self.bounds();
        // The bounds centre first: it does not depend on how the faces happen to be oriented,
        // which matters because this runs while they are still mixed.
        let mut candidates: Vec<Vec3> = vec![bb.center()];
        if let Ok(m) = self.mass_properties() {
            candidates.push(m.centroid);
        }
        for c in candidates.iter().take(2).copied() {
            if self.contains(c) {
                return Some(c);
            }
        }
        for f in &self.faces {
            if let Some(p) = f.interior_point() {
                let m = bb.center().mid(p);
                if self.contains(m) {
                    return Some(m);
                }
            }
        }
        None
    }
}

/// Reverse every loop of a region, keeping the same boundary.
///
/// Needed whenever a loop is carried into a mirrored frame: the carry-over mirrors it, and the
/// convention (outer counter-clockwise, holes clockwise, in the face's own frame) has to be
/// restored or the face ends up facing into the material.
pub fn reverse_regions(region: &Regions) -> Regions {
    let flip = |l: &[Vec2]| -> Vec<Vec2> {
        let mut v = l.to_vec();
        v.reverse();
        v
    };
    Regions::new(region.iter().map(|r| Region { outer: flip(&r.outer), holes: r.holes.iter().map(|h| flip(h)).collect() }).collect())
}

/// A pure vertex key: `(p - origin)` quantised on a `tol` grid, nearest cell.
///
/// # It is a pure key, and that is all it is
///
/// Two points get the same key when they round into the same cell, which is *not* the same as
/// being the same vertex. The cell boundary sits half a cell inside each cell, so two points
/// within `tol` of each other land either side of it as often as not, and a key built this way
/// decides "is this one point?" by asking a question whose answer changes with the drawing's
/// alignment. Nothing can be done about it inside this function — it returns one triple, so the
/// boundary is in the answer by construction — and no caller should read a key as an identity.
///
/// It is kept, and kept cheap, for the callers that want a *bucket*: grouping, not deciding.
/// Anything in the kernel that decides whether two points are one vertex asks `WeldGrid` instead,
/// which clusters by distance over the corners and so has no boundary to fall on. `step.rs`'s
/// welding is such a caller: it groups exported points into a shell's vertices and the group is
/// then carried through `Solid::validate`, which resolves identity itself.
///
/// Input-derived and therefore hostile: a non-finite coordinate or a degenerate tolerance gives a
/// saturating or zero index rather than a panic.
pub(crate) fn vkey(p: Vec3, origin: Vec3, tol: f64) -> (i64, i64, i64) {
    let d = tol.max(1e-300);
    (((p.x - origin.x) / d).round() as i64, ((p.y - origin.y) / d).round() as i64, ((p.z - origin.z) / d).round() as i64)
}

/// Do the faces meeting at one vertex form a single closed fan?
///
/// Around a well-formed vertex every face contributes exactly two boundary edges, and those
/// edges chain into one closed ring. Two separate rings mean the shell touches itself at that
/// vertex: every edge check still passes, so this is the check that catches a pinch.
///
/// The ring is walked on `WeldGrid` corner numbers, the same identity `validate` uses, so an end
/// shared by two faces is recognised as shared however the two copies of it came to differ in
/// their last bit.
fn fan_is_one_cycle(faces: &[Face], fan: &[usize], v: Vec3, tol: f64, grid: &mut WeldGrid) -> bool {
    let mut links: Vec<(Vec3, Vec3)> = Vec::new();
    for &fi in fan {
        let Some(f) = faces.get(fi) else { continue };
        let mut ends: Vec<Vec3> = Vec::new();
        for (a, b) in f.edges() {
            // A face touching the vertex in more than two places is not a simple fan.
            if a.near(v, tol) && !ends.iter().any(|e| e.near(b, tol)) {
                ends.push(b);
            }
            if b.near(v, tol) && !ends.iter().any(|e| e.near(a, tol)) {
                ends.push(a);
            }
        }
        if ends.len() != 2 {
            return false;
        }
        links.push((ends[0], ends[1]));
    }
    if links.len() < 2 {
        return true;
    }
    // Every welded endpoint appears exactly twice, and all of them belong to one ring.
    let mut degree: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut parent: Vec<usize> = (0..links.len() * 2).collect();
    let mut seen: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (li, (a, b)) in links.iter().enumerate() {
        for (p, side) in [(*a, 0usize), (*b, 1usize)] {
            let key = grid.index_of(p);
            *degree.entry(key).or_default() += 1;
            let id = li * 2 + side;
            if let Some(prev) = seen.insert(key, id) {
                union(&mut parent, prev, id);
            }
        }
    }
    // A link joins its own two ends, and two links that share an end are joined. Everything in
    // one class means the fan is one ring rather than several meeting at this vertex.
    for li in 0..links.len() {
        union(&mut parent, li * 2, li * 2 + 1);
    }
    if degree.values().any(|d| *d != 2) {
        return false;
    }
    let root = find(&mut parent, 0);
    (0..links.len() * 2).all(|i| find(&mut parent, i) == root)
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while let Some(p) = parent.get(x).copied()
        && p != x
    {
        x = p;
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if let Some(slot) = parent.get_mut(ra) {
        *slot = rb;
    }
}
