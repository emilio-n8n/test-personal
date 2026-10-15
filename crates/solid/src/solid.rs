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
    pub fn edges(&self) -> Option<Vec<Edge>> {
        let tol = self.tolerance();
        let bb = self.bounds();
        let mut usage: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), Vec<(usize, Vec3, Vec3)>> = std::collections::HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                let (ka, kb) = (vkey(a, bb.min, tol), vkey(b, bb.min, tol));
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
        let mut usage: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), Vec<(usize, bool, Vec3, Vec3)>> =
            std::collections::HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                let (ka, kb) = (vkey(a, bb.min, tol), vkey(b, bb.min, tol));
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
        // vertex -> the faces touching it, with a representative position for the fan check
        let mut fans: std::collections::HashMap<(i64, i64, i64), (Vec3, Vec<usize>)> = std::collections::HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for v in f.vertices() {
                fans.entry(vkey(v, bb.min, tol)).or_insert((v, Vec::new())).1.push(fi);
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
            if !fan_is_one_cycle(&self.faces, &unique, v, tol) {
                return Err(SolidError::Invalid(format!("vertex {key:?} has a pinched fan of {} faces", unique.len())));
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

/// Vertex key for welding on a tolerance grid.
pub(crate) fn vkey(p: Vec3, origin: Vec3, tol: f64) -> (i64, i64, i64) {
    let d = tol.max(1e-300);
    (((p.x - origin.x) / d).round() as i64, ((p.y - origin.y) / d).round() as i64, ((p.z - origin.z) / d).round() as i64)
}

/// Do the faces meeting at one vertex form a single closed fan?
///
/// Around a well-formed vertex every face contributes exactly two boundary edges, and those
/// edges chain into one closed ring. Two separate rings mean the shell touches itself at that
/// vertex: every edge check still passes, so this is the check that catches a pinch.
fn fan_is_one_cycle(faces: &[Face], fan: &[usize], v: Vec3, tol: f64) -> bool {
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
    let mut degree: std::collections::HashMap<(i64, i64, i64), usize> = std::collections::HashMap::new();
    let mut parent: Vec<usize> = (0..links.len() * 2).collect();
    let mut seen: std::collections::HashMap<(i64, i64, i64), usize> = std::collections::HashMap::new();
    for (li, (a, b)) in links.iter().enumerate() {
        for (p, side) in [(*a, 0usize), (*b, 1usize)] {
            let key = vkey(p, v, tol);
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
