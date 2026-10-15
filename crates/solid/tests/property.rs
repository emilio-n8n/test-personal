//! Property-based and hostile-input tests for the 3D solid kernel.
//!
//! The 2D kernel has `geom`'s `property_tests.rs`; the 3D one had none, and a hand-written suite
//! found seven real geometry bugs in a single session. This file is the randomized half of that
//! gate: the properties below are the ones a kernel *must* satisfy for every shape it accepts, so
//! that a regression shows up as a named property with a seed attached rather than as a drawing
//! that renders wrong later.
//!
//! Deterministic throughout — a fixed-seed xorshift64\* generator per test, fixed case counts, no
//! wall clock, no `rand` dependency, no network. Every failure message carries the seed and the case
//! index, so a failure reproduces exactly.
//!
//! Everything here goes through the crate's **public** API (`support::box_at`, `extrude`, `revolve`,
//! `boolean`, `chamfer`, `fillet`, `mass_properties`, `mesh_from_solid`, …). That is deliberate: it
//! doubles as the proof that the public API is enough to state these properties at all.
//!
//! # What the kernel does *not* promise, written down where it comes up
//!
//! - Floating-point sums over a permuted face order are not bit-identical, so `a u b` and `b u a`
//!   agree to a few ulp, not bit for bit ([`union_is_commutative`]).
//! - `Solid::contains` casts a ray, so it has no answer for a point that lies exactly on a face.
//!   Every probe in this file is a point that is unambiguously on one side ([`blends_never_corrupt_a_solid`],
//!   [`booleans_keep_the_material_they_should`]).
//! - A boolean may refuse. "An operation that cannot satisfy the invariants is an error, never a
//!   silently broken solid" is the crate's own contract, so a refusal is never treated as a failure
//!   — it is counted and reported, and the properties are asserted in full wherever a solid came
//!   back.
//!
//! # Known failures
//!
//! Two tests at the bottom of this file fail against the kernel as it stands, each with a minimal
//! reproduction. They are left failing on purpose: see `union_of_two_bodies_touching_across_a_plane`,
//! `union_does_not_depend_on_the_operand_order` and `revolve_builds_no_solid`.

use cadcraft_solid::{
    Bool3, Edge, EdgeRef, Face, MassProperties, Mesh, Plane, Region, Regions, Solid, SolidError, Triangle, Vec2, Vec3, boolean, chamfer, extrude,
    fillet, mass_properties, mesh_from_solid, revolve, stl_ascii, stl_binary, support, tolerance_for,
};

// ---------- generator ----------
//
// Copied from `geom`'s property tests verbatim: the same xorshift64*, so a failure reported from
// either crate is reproduced by the same three lines of code.

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in `[-scale, scale]`.
    fn f64(&mut self, scale: f64) -> f64 {
        ((self.next() >> 11) as f64 / (1u64 << 53) as f64) * 2.0 * scale - scale
    }
    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn vec3(&mut self, scale: f64) -> Vec3 {
        Vec3::new(self.f64(scale), self.f64(scale), self.f64(scale))
    }
    /// An f64 chosen to be nasty: non-finite, subnormal, extreme or raw bit patterns.
    fn hostile(&mut self) -> f64 {
        match self.next() % 10 {
            0 => 0.0,
            1 => -0.0,
            2 => f64::NAN,
            3 => f64::INFINITY,
            4 => f64::NEG_INFINITY,
            5 => 1.0e300,
            6 => -1.0e300,
            7 => 5.0e-324,
            8 => f64::MIN_POSITIVE,
            _ => f64::from_bits(self.next()),
        }
    }
}

// ---------- comparison helpers ----------

/// Relative closeness with a floor, so zero-scale cases stay meaningful.
fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

/// Unwrap with the operation and the case named in the failure.
fn must<T>(r: Result<T, SolidError>, what: &str, at: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{at}: {what}: {e}"),
    }
}

/// Seed and case index, formatted once so every message in a loop reads the same way.
fn tag(seed: u64, case: usize) -> String {
    format!("seed {seed:#x}, case {case}")
}

/// Property 5's gate, applied to every result a property test produces: whatever comes back must
/// validate, must not pinch, and must enclose a positive volume.
fn assert_valid(s: &Solid, what: &str, at: &str) {
    if let Err(e) = s.validate() {
        panic!("{at}: {what} produced a solid that fails validate(): {e}");
    }
    if let Err(e) = s.validate_vertices() {
        panic!("{at}: {what} produced a pinched solid: {e}");
    }
    let v = s.volume();
    assert!(v > 0.0, "{at}: {what} produced a solid with volume {v}");
}

/// Volume re-derived from the tessellation, independently of `mass_properties`.
///
/// `mass_properties` picks its own reference point; this uses the same one so the two sums differ
/// only in the arithmetic, not in what they integrate. A face that was dropped, a hole that was
/// filled, or a loop wound backwards all move this number and not the other's.
fn mesh_volume(mesh: &Mesh, reference: Vec3) -> f64 {
    mesh.triangles().map(|t| ((t.a - reference).dot((t.b - reference).cross(t.c - reference))) / 6.0).sum()
}

/// Every vertex of `a` matches a distinct vertex of `b` and the other way round, within `tol`.
///
/// The shell of a solid *is* its boundary, so two solids are the same solid exactly when their
/// vertex multisets agree. Comparing sorted coordinates would be order-sensitive in a way the
/// geometry is not, so this is a greedy match over the distances.
fn same_vertex_set(a: &Solid, b: &Solid, tol: f64) -> bool {
    let av: Vec<Vec3> = a.faces.iter().flat_map(|f| f.vertices()).collect();
    let bv: Vec<Vec3> = b.faces.iter().flat_map(|f| f.vertices()).collect();
    if av.len() != bv.len() {
        return false;
    }
    let mut taken = vec![false; bv.len()];
    for p in &av {
        match bv.iter().enumerate().find(|(i, q)| !taken[*i] && p.dist(**q) <= tol) {
            Some((i, _)) => taken[i] = true,
            None => return false,
        }
    }
    true
}

/// The first line of an error, for a one-line report.
fn brief(e: &SolidError) -> String {
    e.to_string().lines().next().unwrap_or("").chars().take(96).collect()
}

/// The two faces meeting at `edge`, and the in-plane direction of each that points into the material.
///
/// The public API has no way to resolve an `EdgeRef` to a frame, so this is the same two-line rule
/// the kernel uses in `chamfer`: an in-plane direction of face 1 is `edge x n1` or `n1 x edge`, and
/// the right one is whichever points away from face 2.
fn in_face_directions(edge: &Edge, f1: &Face, f2: &Face) -> Option<(Vec3, Vec3)> {
    let e = (edge.b - edge.a).normalized();
    if e.len() < 0.5 {
        return None;
    }
    let pick = |n: Vec3, away: Vec3| [e.cross(n), n.cross(e)].into_iter().map(|c| c.normalized()).find(|c| c.len() > 0.5 && c.dot(away) < -1e-9);
    Some((pick(f1.plane.normal, f2.plane.normal)?, pick(f2.plane.normal, f1.plane.normal)?))
}

// ---------- shape generators ----------

/// One shape, drawn in **scale-free** terms: every coordinate in `[-1, 1]` and every length in
/// `[0.25, 1]`, so the same drawing can be built at any size.
///
/// Drawing and building are separate steps on purpose. A boolean's output depends on the coordinates
/// it is handed, not only on their ratios, so comparing a case against itself at two sizes only
/// means something if both sizes came from the same drawing. It also matters for the generator's own
/// bookkeeping: a box and a prism consume a different number of random values, so a generator that
/// re-drew per size would silently be comparing two different shapes.
///
/// Every edge is at least a quarter of the span long. That keeps each feature far above the kernel's
/// `diagonal x 1e-9` tolerance, even after the scale test shrinks a solid by a million and the
/// documented `1e-12` floor takes over.
#[derive(Clone, Debug)]
enum Shape {
    /// An axis-aligned box from `min` to `min + size`.
    Box { min: Vec3, size: Vec3 },
    /// A convex polygon on a circle of radius `radius` centred on `centre.xy`, extruded up from
    /// `base_z` by `height`.
    Prism { angles: Vec<f64>, radius: f64, centre: Vec3, base_z: f64, height: f64 },
}

impl std::fmt::Display for Shape {
    /// Compact and human-readable. Not bit-exact — the seed and case index are the reproduction key —
    /// but enough to recognise the shape by eye in a failure report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shape::Box { min, size } => {
                write!(f, "box min=({:.9}, {:.9}, {:.9}) size=({:.9}, {:.9}, {:.9})", min.x, min.y, min.z, size.x, size.y, size.z)
            }
            Shape::Prism { angles, radius, centre, base_z, height } => write!(
                f,
                "prism centre=({:.9}, {:.9}) base_z={:.9} radius={:.9} height={:.9} angles=[{}]",
                centre.x,
                centre.y,
                base_z,
                radius,
                height,
                angles.iter().map(|a| format!("{a:.9}")).collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

impl Shape {
    fn draw(rng: &mut Rng, anchor: Vec3) -> Shape {
        if rng.next().is_multiple_of(2) {
            Shape::Box { min: anchor, size: Vec3::new(rng.range(0.25, 1.0), rng.range(0.25, 1.0), rng.range(0.25, 1.0)) }
        } else {
            // A regular n-gon with a bounded jitter and a random phase, rather than n independent
            // random angles. Independent angles can come out as a sliver, and a sliver is useless
            // here: it overlaps almost nothing, which turns a regime meant to guarantee overlap into
            // a lottery. With this construction every angular gap stays bounded away from zero, so
            // the polygon always contains a disc of about `radius * cos(pi / n)` about its centre.
            let n = 3 + (rng.next() % 4) as usize;
            let phase = rng.range(-std::f64::consts::PI, std::f64::consts::PI);
            let step = std::f64::consts::TAU / n as f64;
            let angles: Vec<f64> = (0..n).map(|k| phase + step * (k as f64 + rng.range(-0.35, 0.35))).collect();
            Shape::Prism { angles, radius: rng.range(0.2, 0.6), centre: anchor, base_z: rng.f64(1.0), height: rng.range(0.3, 1.0) }
        }
    }

    fn build(&self, scale: f64, at: &str) -> Solid {
        match self {
            Shape::Box { min, size } => must(support::box_at(*min * scale, (*min + *size) * scale), "support::box_at", at),
            Shape::Prism { angles, radius, centre, base_z, height } => {
                let pts: Vec<Vec2> = angles.iter().map(|a| Vec2::from_angle(*a) * (radius * scale)).collect();
                let m = |p: &Vec2| Vec2::new(p.x + centre.x * scale, p.y + centre.y * scale);
                let profile = Regions::new(vec![Region { outer: pts.iter().map(m).collect(), holes: Vec::new() }]);
                must(extrude(&profile, &Plane::xy(base_z * scale), height * scale), "extrude (prism)", at)
            }
        }
    }
}

/// How the two bodies of a case are placed relative to each other.
#[derive(Clone, Copy, Debug)]
enum Regime {
    /// Close enough that both bodies certainly share material: every shape above contains a disc of
    /// at least a fifth of the span about its anchor.
    Overlap,
    /// The second body is based exactly on the first's lowest plane, exactly on its highest, or
    /// halfway up. Those are the alignments where two face planes become coplanar, where a naive
    /// section invents geometry that is not there, and where a boolean has to merge faces instead of
    /// producing two that then need welding.
    Grazing,
    /// No shared material at all, which is legal and still has to satisfy the volume identities.
    Disjoint,
}

/// A pair of shapes, drawn once, plus the size they were drawn for.
///
/// [`Case::build`] is the only way to get solids out of it, so a case drawn here is the *same* case
/// at every size — which is what makes [`volume_and_topology_survive_a_six_order_scale_change`]
/// mean anything.
#[derive(Clone, Debug)]
struct Case {
    a: Shape,
    b: Shape,
    span: f64,
}

impl Case {
    fn draw(rng: &mut Rng, span: f64, regime: Regime, at: &str) -> Case {
        let start = rng.vec3(1.0);
        let a = Shape::draw(rng, start);
        let bb = a.build(span, at).bounds();
        let anchor = match regime {
            Regime::Overlap => bb.center() + rng.vec3(0.05),
            Regime::Grazing => {
                let depth = bb.max.z - bb.min.z;
                let z = match rng.next() % 3 {
                    0 => bb.min.z,
                    1 => bb.min.z + depth,
                    _ => bb.min.z + depth * 0.5,
                };
                let (dx, dy) = (rng.f64(0.1), rng.f64(0.1));
                Vec3::new(bb.min.x + dx, bb.min.y + dy, z)
            }
            Regime::Disjoint => Vec3::new(bb.max.x + 1.0, bb.min.y, bb.min.z),
        };
        Case { b: Shape::draw(rng, anchor / span), a, span }
    }

    /// The same two shapes at `scale` times the size they were drawn for.
    fn build(&self, scale: f64, at: &str) -> (Solid, Solid) {
        (self.a.build(self.span * scale, at), self.b.build(self.span * scale, at))
    }

    /// Seed, case index and the drawing itself, for a failure message.
    fn report(&self, seed: u64, case: usize) -> String {
        format!("{}\n    a = {}\n    b = {}\n    span = {}", tag(seed, case), self.a, self.b, self.span)
    }
}

/// Draw one case and build it at unit scale.
fn pair(rng: &mut Rng, span: f64, regime: Regime, at: &str) -> (Case, (Solid, Solid)) {
    let drawn = Case::draw(rng, span, regime, at);
    let solids = drawn.build(1.0, at);
    (drawn, solids)
}

/// A random axis-aligned box or prism anywhere near the origin.
fn random_solid(rng: &mut Rng, span: f64, at: &str) -> Solid {
    let anchor = rng.vec3(1.0);
    Shape::draw(rng, anchor).build(span, at)
}

// ---------- 1. volume identities ----------

/// `vol(a \ b) + vol(a n b) == vol(a)` and `vol(a u b) + vol(a n b) == vol(a) + vol(b)`.
///
/// Operands that do not overlap are legal: `intersect` then refuses with [`SolidError::Empty`], and
/// that refusal *is* the statement that the intersection encloses nothing, so its volume enters the
/// identities as an exact zero. An operation that refuses for any other reason produced no solid at
/// all, so there is nothing to measure; those cases are counted and reported, and the identities
/// are asserted in full for every case where the kernel did return a solid.
#[test]
fn volume_identities_hold_for_boxes_prisms_and_cuts() {
    const CASES: usize = 180;
    let seed = 0x5011_D3E7_u64;
    let mut rng = Rng::new(seed);
    let (mut overlaps, mut empty, mut refused) = (0usize, 0usize, 0usize);
    for case in 0..CASES {
        // Small spans force tangency and shared-face cases; large ones force clear overlap.
        let span = if case % 4 == 0 { 0.6 } else { 12.0 };
        let regime = match case % 3 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            _ => Regime::Disjoint,
        };
        let (drawn, (a, b)) = pair(&mut rng, span, regime, "generator");
        let at = drawn.report(seed, case);
        let (va, vb) = (a.volume(), b.volume());
        assert!(va > 0.0 && vb > 0.0, "{at}: a generator produced an empty operand ({va}, {vb})");

        let mut volumes: Vec<f64> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        for (name, op) in [("union", Bool3::Union), ("difference", Bool3::Difference), ("intersect", Bool3::Intersect)] {
            match boolean(&a, &b, op) {
                Ok(s) => {
                    assert_valid(&s, name, &at);
                    if name == "intersect" {
                        overlaps += 1;
                    }
                    volumes.push(s.volume());
                }
                Err(SolidError::Empty) => {
                    empty += 1;
                    volumes.push(0.0);
                }
                Err(e) => {
                    refused += 1;
                    notes.push(format!("{name} refused: {}", brief(&e)));
                }
            }
        }
        if volumes.len() < 3 {
            if case < 4 {
                println!("{at}\n    {}", notes.join("\n    "));
            }
            continue;
        }
        let (vu, vd, vi) = (volumes[0], volumes[1], volumes[2]);

        // 1a: vol(a \ b) + vol(a n b) == vol(a).
        assert!(
            near(vd + vi, va, 1e-9),
            "{at}: vol(a\\b) {vd} + vol(a n b) {vi} = {} != vol(a) {va} (relative error {:e})",
            vd + vi,
            (vd + vi - va) / va.abs().max(1e-300)
        );
        // 1b: vol(a u b) + vol(a n b) == vol(a) + vol(b).
        assert!(
            near(vu + vi, va + vb, 1e-9),
            "{at}: vol(a u b) {vu} + vol(a n b) {vi} = {} != vol(a) {va} + vol(b) {vb} (relative error {:e})",
            vu + vi,
            (vu + vi - va - vb) / (va + vb).abs().max(1e-300)
        );
        // Nothing a boolean returns may be larger than the material that went in.
        assert!(vd <= va + 1e-9, "{at}: a cut added material: {vd} > {va}");
        assert!(vu <= va + vb + 1e-9, "{at}: a union added material: {vu} > {}", va + vb);
        assert!(vi <= va.min(vb) + 1e-9, "{at}: the intersection is bigger than an operand: {vi} > {} / {vb}", va.min(vb));
    }
    println!("volume identities: {overlaps} overlapping intersections, {empty} empty, {refused} operations refused for another reason");
    assert!(overlaps > 30, "the overlapping cases collapsed into refusals ({overlaps})");
    assert!(empty > 10, "the disjoint and grazing regimes never produced an empty result ({empty})");
}

// ---------- 2. the two volume computations agree ----------

/// `mass_properties` (tetrahedra from a mesh) against `volume_from_faces` (the pyramid sum over
/// planar faces) against the mesh integrated again here.
///
/// The first two share no code path; the third re-derives the first's arithmetic from the public mesh
/// so that a `mass_properties` that quietly fell back to a plausible number cannot hide behind
/// itself. Every solid the rest of this file generates passes through here.
#[test]
fn the_independent_volume_computations_agree() {
    const CASES: usize = 220;
    let seed = 0x7A11_C0DE_u64;
    let mut rng = Rng::new(seed);
    let mut checked = 0usize;
    let mut revolved = 0usize;
    let mut revolved_refused = 0usize;
    let mut revolve_note = String::new();
    for case in 0..CASES {
        let span = if case % 3 == 0 { 0.6 } else { 20.0 };
        let regime = match case % 3 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            _ => Regime::Disjoint,
        };
        let (drawn, (a, b)) = pair(&mut rng, span, regime, "generator");
        let at = drawn.report(seed, case);
        let mut solids: Vec<(String, Solid)> = vec![("a".to_string(), a.clone()), ("b".to_string(), b)];

        if case % 2 == 0 {
            if let Ok(d) = boolean(&a, &a, Bool3::Difference) {
                panic!("{at}: difference(a, a) returned a solid of volume {}", d.volume());
            }
            solids.push(("a n a".to_string(), must(boolean(&a, &a, Bool3::Intersect), "intersect(a, a)", &at)));
            solids.push(("a u a".to_string(), must(boolean(&a, &a, Bool3::Union), "union(a, a)", &at)));
        }
        let tool = random_solid(&mut rng, span, "generator");
        if let Ok(cut) = boolean(&a, &tool, Bool3::Difference) {
            solids.push(("a - tool".to_string(), cut));
        }

        // A convex prism and a revolve, so faces built by the sweep builders are covered too. The
        // revolve plane's `v` has to run along the axis: `Plane::xy` has `u = X` (radial) and
        // `v = Y` (axial), so the axis is +Y and the profile's `x` is the distance from it.
        //
        // `revolve` currently refuses every input — see `revolve_builds_no_solid` — so its result is
        // recorded here rather than required, and this test stays green.
        let n = 4 + (rng.next() % 3) as usize;
        let radius = rng.range(0.3 * span, 0.8 * span);
        let phase = rng.range(-std::f64::consts::PI, std::f64::consts::PI);
        let step = std::f64::consts::TAU / n as f64;
        let convex: Vec<Vec2> = (0..n).map(|k| Vec2::from_angle(phase + step * (k as f64 + rng.range(-0.35, 0.35))) * radius).collect();
        solids.push((
            "extruded convex".to_string(),
            must(extrude(&Region::new(convex).to_regions(), &Plane::xy(rng.f64(span)), rng.range(0.3, 1.2 * span)), "extrude (convex)", &at),
        ));
        let height = rng.range(0.5, 1.5);
        let profile = support::rect(radius.max(0.2), 0.0, radius.max(0.2) + height, rng.range(0.6, 2.0) * height);
        let angle = if case % 5 == 0 { std::f64::consts::TAU } else { rng.range(0.3, 3.0) };
        let segments = 6 + (case % 8);
        match revolve(&profile, &Plane::xy(0.0), Vec3::Y, angle, segments) {
            Ok(s) => {
                revolved += 1;
                solids.push(("revolved".to_string(), s));
            }
            Err(e) => {
                revolved_refused += 1;
                if revolve_note.is_empty() {
                    revolve_note = format!("{at}\n    revolve(rect, Plane::xy, +Y, angle {angle:.6}, {segments} segments) refused: {}", brief(&e));
                }
            }
        }

        for (name, s) in solids {
            let where_ = format!("{at}\n    solid: {name}");
            assert_valid(&s, &name, &where_);
            let m: MassProperties = mass_properties(&s).unwrap_or_else(|e| panic!("{where_}: mass_properties: {e}"));
            let apex = s.interior_point().unwrap_or_else(|| s.bounds().center());
            let faces = s.volume_from_faces(apex).unwrap_or_else(|e| panic!("{where_}: volume_from_faces: {e}"));
            let mesh = mesh_from_solid(&s).unwrap_or_else(|e| panic!("{where_}: mesh_from_solid: {e}"));
            let from_mesh = mesh_volume(&mesh, s.bounds().center());
            assert!(
                near(m.volume, faces, 1e-9),
                "{where_}: tetrahedra {} vs pyramid sum {faces} (relative error {:e})",
                m.volume,
                (m.volume - faces) / faces.abs().max(1e-300)
            );
            assert!(
                near(m.volume, from_mesh, 1e-9),
                "{where_}: mass_properties {} vs the mesh integrated here {from_mesh} (relative error {:e})",
                m.volume,
                (m.volume - from_mesh) / from_mesh.abs().max(1e-300)
            );
            // The reported area is the sum of the faces', not the mesh's — a cheap third identity
            // that a dropped face would break.
            let face_area: f64 = s.faces.iter().map(|f| f.area()).sum();
            assert!(near(face_area, m.area, 1e-12), "{where_}: area {} vs sum of faces {face_area}", m.area);
            checked += 1;
        }
    }
    println!("volume cross-check: {checked} solids measured three ways; revolve built {revolved}, refused {revolved_refused}");
    if !revolve_note.is_empty() {
        println!("first revolve refusal:\n    {revolve_note}");
    }
    assert!(checked >= CASES * 4, "too few solids actually measured ({checked})");
}

// ---------- 3. commutativity ----------

/// `a u b` and `b u a` are the same solid, and one order does not work where the other does.
///
/// **Bit-for-bit equality of the volume does not hold, and this test says so rather than asserting
/// it.** The two runs hand the same two operands to the planar boolean in opposite orders, so the
/// loop coordinates come out one or two ulp apart and the tetrahedron sum — added over faces in
/// whatever order they were built — comes out one to five ulp apart. Measured over the cases below
/// that is a worst case of 5 ulp, a relative difference of 8.6e-16. That is floating-point addition,
/// not geometry: the vertex sets are identical to 1e-12.
///
/// What *is* asserted exactly is the topology — the same number of faces, the same multiset of
/// boundary vertices — and what is asserted to a hair is every number derived from them. The
/// tolerance used below is 1e-12: three orders of magnitude tighter than the rest of this file, and
/// four orders of magnitude looser than the 8.6e-16 the kernel actually reaches.
///
/// **This test currently fails**, at the case named in the message: `union(a, b)` returns a valid
/// 13-face solid and `union(b, a)` refuses the very same pair. See
/// `union_does_not_depend_on_the_operand_order` for the pinned report.
#[test]
fn union_is_commutative() {
    const CASES: usize = 120;
    let seed = 0xC0FF_EE42_u64;
    let mut rng = Rng::new(seed);
    let (mut checked, mut refused, mut worst_ulp, mut worst_rel) = (0usize, 0usize, 0i64, 0.0f64);
    for case in 0..CASES {
        let span = if case % 3 == 0 { 0.6 } else { 14.0 };
        let regime = match case % 3 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            _ => Regime::Disjoint,
        };
        let (drawn, (a, b)) = pair(&mut rng, span, regime, "generator");
        let at = drawn.report(seed, case);
        let (ab, ba) = (boolean(&a, &b, Bool3::Union), boolean(&b, &a, Bool3::Union));
        // A refusal is itself a result, and it has to be symmetric: if one order refuses, so must
        // the other, with the same error.
        match (ab, ba) {
            (Ok(x), Ok(y)) => {
                let (ab, ba) = (x, y);
                assert_valid(&ab, "union(a, b)", &at);
                assert_valid(&ba, "union(b, a)", &at);

                assert_eq!(ab.faces.len(), ba.faces.len(), "{at}: {} faces one way, {} the other", ab.faces.len(), ba.faces.len());
                let tol = 1e-12 * ab.bounds().diagonal().max(1.0);
                assert!(
                    same_vertex_set(&ab, &ba, tol),
                    "{at}: the two unions are not the same boundary: {} vs {} vertices",
                    ab.faces.iter().map(|f| f.vertices().len()).sum::<usize>(),
                    ba.faces.iter().map(|f| f.vertices().len()).sum::<usize>()
                );

                let (va, vb) = (ab.volume(), ba.volume());
                assert!(
                    near(va, vb, 1e-12),
                    "{at}: vol(a u b) {va:.17e} vs vol(b u a) {vb:.17e} (relative error {:e})",
                    (va - vb) / va.abs().max(vb.abs())
                );
                let (aa, ab_area) = (ab.area(), ba.area());
                assert!(near(aa, ab_area, 1e-12), "{at}: area {aa:.17e} vs {ab_area:.17e}");
                worst_ulp = worst_ulp.max((va.to_bits() as i64 - vb.to_bits() as i64).abs());
                worst_rel = worst_rel.max((va - vb).abs() / va.abs().max(vb.abs()));

                // `contains` casts a ray, so it is order-independent by construction — it must agree.
                for probe in [a.interior_point(), b.interior_point(), Some(ab.bounds().center())].into_iter().flatten() {
                    assert_eq!(ab.contains(probe), ba.contains(probe), "{at}: the two unions disagree about {probe:?}");
                }
                checked += 1;
            }
            (Err(e), Err(f)) => {
                assert_eq!(e, f, "{at}: one order refused and the other did not");
                refused += 1;
            }
            (Ok(s), Err(e)) => panic!(
                "{at}\n    union(a, b) returned a valid solid of {} faces and volume {:e},\n    union(b, a) refused: {e}",
                s.faces.len(),
                s.volume()
            ),
            (Err(e), Ok(s)) => panic!(
                "{at}\n    union(b, a) returned a valid solid of {} faces and volume {:e},\n    union(a, b) refused: {e}",
                s.faces.len(),
                s.volume()
            ),
        }
    }
    println!("commutativity: {checked} pairs compared, {refused} refused both ways; worst volume gap {worst_ulp} ulp, relative {worst_rel:e}");
    assert!(checked > 80, "too few pairs were actually compared ({checked})");
}

// ---------- 4. idempotence and reflexivity ----------

/// `a u a == a`, `a n a == a`, `a \ a` is empty or an error — never a solid.
#[test]
fn booleans_are_idempotent_and_reflexive() {
    const CASES: usize = 120;
    let seed = 0x1DEA_1DEA_u64;
    let mut rng = Rng::new(seed);
    for case in 0..CASES {
        let span = if case % 3 == 0 { 0.6 } else { 14.0 };
        let (drawn, (a, _)) = pair(&mut rng, span, Regime::Overlap, "generator");
        let at = drawn.report(seed, case);
        let va = a.volume();
        assert!(va > 0.0, "{at}: the generator produced an empty operand");

        for (name, op) in [("union", Bool3::Union), ("intersect", Bool3::Intersect)] {
            let s = must(boolean(&a, &a, op), &format!("{name}(a, a)"), &at);
            assert_valid(&s, &format!("{name}(a, a)"), &at);
            assert!(near(s.volume(), va, 1e-9), "{at}: vol(a {name} a) {} != vol(a) {va}", s.volume());
            assert_eq!(s.faces.len(), a.faces.len(), "{at}: a {name} a has {} faces, a has {}", s.faces.len(), a.faces.len());
            let tol = 1e-12 * a.bounds().diagonal().max(1.0);
            assert!(same_vertex_set(&s, &a, tol), "{at}: a {name} a is not the same boundary as a");
        }

        // Subtracting a solid from itself leaves nothing. The kernel reports nothing as an error;
        // what must never happen is a body coming back.
        match boolean(&a, &a, Bool3::Difference) {
            Ok(s) => panic!("{at}: difference(a, a) returned a solid of volume {} — it must return nothing", s.volume()),
            Err(e) => assert!(!e.to_string().is_empty(), "{at}: the error has to explain itself"),
        }
    }
}

// ---------- 5. validation is the real gate ----------

/// Every generated result is `Err`, or it validates, does not pinch, and encloses volume.
///
/// This is the gate the whole file leans on: a kernel that hands back an open, inside-out or pinched
/// shell instead of an error shows up here with its seed and its exact inputs, which is the only
/// difference between a bug report and a hunch.
#[test]
fn every_generated_solid_validates() {
    const CASES: usize = 200;
    let seed = 0xDEAD_BEEF_u64;
    let mut rng = Rng::new(seed);
    let (mut ok, mut err) = (0usize, 0usize);
    for case in 0..CASES {
        let span = if case % 4 == 0 { 0.5 } else { 25.0 };
        let regime = match case % 4 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            2 => Regime::Disjoint,
            _ => Regime::Overlap,
        };
        let (drawn, (a, b)) = pair(&mut rng, span, regime, "generator");
        let at = drawn.report(seed, case);
        assert_valid(&a, "operand a", &at);
        assert_valid(&b, "operand b", &at);
        for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
            match boolean(&a, &b, op) {
                Ok(s) => {
                    assert_valid(&s, &format!("{op:?}"), &at);
                    ok += 1;
                }
                Err(e) => {
                    // A refusal has to name itself.
                    assert!(!e.to_string().is_empty(), "{at}: {op:?} returned an empty error message");
                    err += 1;
                }
            }
        }
    }
    println!("validation: {ok} solids validated, {err} operations refused");
    assert!(ok > CASES, "almost every boolean refused ({ok} successes) — the generator is broken, not the kernel");
}

/// The three set operations agree with `Solid::contains` about where the material ended up.
///
/// The probe point is strictly interior to `a`, and its membership in `b` is decided by the whole
/// small cube around it rather than by the point alone. That matters: `contains` casts a ray, and a
/// ray cast from a point that lies exactly on a face has no answer, so a test that probes such a
/// point is testing the ray caster rather than the boolean. Where the cube straddles a boundary the
/// case is skipped and counted.
#[test]
fn booleans_keep_the_material_they_should() {
    const CASES: usize = 200;
    let seed = 0xB005_7A11_u64;
    let mut rng = Rng::new(seed);
    let mut checked = 0usize;
    for case in 0..CASES {
        let span = if case % 3 == 0 { 0.5 } else { 25.0 };
        let regime = match case % 3 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            _ => Regime::Disjoint,
        };
        let (drawn, (a, b)) = pair(&mut rng, span, regime, "generator");
        let at = drawn.report(seed, case);
        let Some(p) = a.interior_point() else { continue };
        let e = 1e-7 * a.bounds().diagonal().max(1.0);
        let corners = [
            Vec3::new(e, 0.0, 0.0),
            Vec3::new(-e, 0.0, 0.0),
            Vec3::new(0.0, e, 0.0),
            Vec3::new(0.0, -e, 0.0),
            Vec3::new(0.0, 0.0, e),
            Vec3::new(0.0, 0.0, -e),
        ];
        if !corners.iter().all(|o| a.contains(p + *o)) {
            continue;
        }
        let all_in = corners.iter().all(|o| b.contains(p + *o));
        let all_out = corners.iter().all(|o| !b.contains(p + *o));
        if !all_in && !all_out {
            continue;
        }
        let in_a = true;
        let in_b = all_in;
        for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
            let Ok(s) = boolean(&a, &b, op) else { continue };
            let want = match op {
                Bool3::Union => in_a || in_b,
                Bool3::Intersect => in_a && in_b,
                Bool3::Difference => in_a && !in_b,
            };
            let got = corners.iter().all(|o| s.contains(p + *o));
            assert_eq!(got, want, "{at}: {op:?} says {got} about the material at {p:?}, the operands say {want} (a: {in_a}, b: {in_b})");
            checked += 1;
        }
    }
    println!("material placement: {checked} probes checked against the operands");
    assert!(checked > 200, "too few probes survived the boundary filter ({checked})");
}

// ---------- 6. scale invariance ----------

/// The same two shapes at 1e-6, 1 and 1e6: the same topology, and a volume that scales as the cube.
///
/// The shapes are **drawn once and rebuilt** at each scale ([`Case`]), not redrawn — a generator that
/// redraws compares two different bodies, because a box and a prism consume a different number of
/// random values and the stream diverges immediately.
///
/// The bodies are 20 units across at unit scale, so at 1e-6 they are still about 2e-5 across and
/// every feature in them is many orders of magnitude above the `1e-12` floor the documented tolerance
/// clamp puts under small solids. Below that floor the kernel's own contract says features thinner
/// than the tolerance are not resolved, and this test would be measuring that clamp rather than scale
/// invariance.
#[test]
fn volume_and_topology_survive_a_six_order_scale_change() {
    const CASES: usize = 60;
    const SCALES: [f64; 3] = [1e-6, 1.0, 1e6];
    let seed = 0x5CA1_E5C0_u64;
    let mut rng = Rng::new(seed);
    let (mut compared, mut refused) = (0usize, 0usize);
    for case in 0..CASES {
        let drawn = Case::draw(&mut rng, 20.0, Regime::Overlap, "generator");
        let at = drawn.report(seed, case);
        let run = |a: &Solid, b: &Solid| [boolean(a, b, Bool3::Union), boolean(a, b, Bool3::Intersect), boolean(a, b, Bool3::Difference)];
        let (a1, b1) = drawn.build(1.0, &at);
        let reference = run(&a1, &b1);
        for (name, r) in ["union", "intersect", "difference"].iter().zip(reference.iter()) {
            match r {
                Ok(s) => assert_valid(s, name, &at),
                // Nothing to scale if there is no reference result. Counted and reported.
                Err(_) => refused += 1,
            }
        }

        for scale in SCALES {
            let (a, b) = drawn.build(scale, &at);
            let at_s = format!("{at}\n    scale = {scale:e}");
            let got = run(&a, &b);
            let cube = scale * scale * scale;
            for (k, (name, r)) in ["union", "intersect", "difference"].iter().zip(got.iter()).enumerate() {
                let Ok(g) = r.as_ref() else {
                    refused += 1;
                    continue;
                };
                let Ok(w) = reference[k].as_ref() else { continue };
                assert_valid(g, name, &at_s);
                compared += 1;
                assert_eq!(g.faces.len(), w.faces.len(), "{at_s}: {name} has {} faces, the unit-scale one has {}", g.faces.len(), w.faces.len());
                let expect = w.volume() * cube;
                assert!(
                    near(g.volume(), expect, 1e-9),
                    "{at_s}: vol({name}) {:.12e} != unit volume {:.12e} x {cube:e} = {expect:.12e} (relative error {:e})",
                    g.volume(),
                    w.volume(),
                    (g.volume() - expect) / expect.abs().max(1e-300)
                );
            }
        }
    }
    println!("scale invariance: {compared} results compared, {refused} refused");
    assert!(compared > 400, "too little survived to compare ({compared})");
}

// ---------- 7. blends never corrupt ----------

/// A chamfer or a fillet on a random edge of a random box or prism either refuses, or returns a
/// solid that validates and is strictly smaller than what it started from.
///
/// The edge itself is not used as the probe: an edge midpoint lies exactly on two faces, and
/// `Solid::contains` casts a ray, which has no answer for a point on a face. The probes are a little
/// way into the material along the corner's bisector, at a descending ladder of distances. The nearest
/// one is what any blend has to remove. The further rungs only matter at the obtuse corners a polygon
/// prism has, where a round's arc sits very close to the edge and a probe a quarter of the radius in
/// is already inside the retained material — going a quarter of the way along the two face *normals*
/// instead is worse still, because that leaves each face's plane entirely.
#[test]
fn blends_never_corrupt_a_solid() {
    const CASES: usize = 140;
    let seed = 0xF11E_7BAD_u64;
    let mut rng = Rng::new(seed);
    let (mut ok, mut err) = (0usize, 0usize);
    for case in 0..CASES {
        // Bodies with every edge at least `span / 4` long: every edge is then a clean straight edge
        // with two whole neighbours, and `resolve` cannot confuse it with another.
        let span = 10.0;
        let start = rng.vec3(1.0);
        let shape = Shape::draw(&mut rng, start);
        let b = shape.build(span, "generator");
        let at = format!("{}\n    {shape}", tag(seed, case));
        let v0 = b.volume();
        assert!(v0 > 0.0, "{at}: empty body");
        let edges: Vec<Edge> = b.edges().unwrap_or_else(|| panic!("{at}: a fresh body has no edge list"));
        assert!(!edges.is_empty(), "{at}: a fresh body has no edges");
        let pick = (rng.next() % edges.len() as u64) as usize;
        let Some(edge) = edges.get(pick) else { continue };
        // Aim at the middle of the edge, never at a corner: `resolve` refuses an ambiguous reference,
        // and that refusal is documented behaviour, not the property under test.
        let aim = edge.a.mid(edge.b);
        let refs = [EdgeRef::new(aim, b.tolerance())];
        let distance = rng.range(0.02, 0.25);
        let segments = 1 + (rng.next() % 10) as usize;

        let (Some(f1), Some(f2)) = (b.faces.get(edge.faces[0]), b.faces.get(edge.faces[1])) else { continue };
        let Some((d1, d2)) = in_face_directions(edge, f1, f2) else { continue };
        let bisector = d1 + d2;
        let probes: Vec<Vec3> = [0.5, 0.25, 0.125, 0.0625].iter().map(|f| aim + bisector * (distance * f)).collect();
        assert!(probes.iter().all(|p| b.contains(*p)), "{at}: a probe is not inside the body to begin with");
        let centre = b.bounds().center();
        assert!(b.contains(centre), "{at}: the bounds centre is not inside a convex body");

        for (name, got) in [("chamfer", chamfer(&b, &refs, distance)), ("fillet", fillet(&b, &refs, distance, segments))] {
            match got {
                Ok(s) => {
                    assert_valid(&s, name, &at);
                    let v = s.volume();
                    assert!(v > 0.0 && v < v0, "{at}: {name} on the edge at {aim:?} at {distance:e} gave volume {v}, the body is {v0}");
                    assert!(
                        probes.iter().any(|p| !s.contains(*p)),
                        "{at}: {name} on the edge at {aim:?} at {distance:e} removed nothing next to the edge; the probes {probes:?} are all still in the material"
                    );
                    assert!(s.contains(centre), "{at}: {name} ate the middle of the part, not just the edge {aim:?}");
                    ok += 1;
                }
                Err(e) => {
                    assert!(!e.to_string().is_empty(), "{at}: {name} refused with an empty message");
                    err += 1;
                }
            }
        }
    }
    println!("blends: {ok} applied, {err} refused");
    assert!(ok > 20, "nearly every blend refused ({ok}) — the generator is broken, not the kernel");
}

// ---------- 8. hostile input never panics ----------

/// Non-finite, subnormal, astronomically large, zero and negative numbers through **every** public
/// entry point of the kernel.
///
/// The property is "the process survives and every call comes back a `Result`", so it is asserted
/// by surviving: each call is made, its `Result` is looked at, and nothing is required of the value.
/// There is no `#[should_panic]` here — a test that passes because the code panicked is not a test.
#[test]
fn hostile_input_never_panics() {
    const CASES: usize = 260;
    let seed = 0xBAD0_0BAD_u64;
    let mut rng = Rng::new(seed);
    for case in 0..CASES {
        let at = tag(seed, case);
        let h = |rng: &mut Rng| rng.hostile();
        let h3 = |rng: &mut Rng| Vec3::new(rng.hostile(), rng.hostile(), rng.hostile());

        // --- Plane: every constructor and every query, with hostile origins, normals and u axes.
        let origin = h3(&mut rng);
        let normal = h3(&mut rng);
        let plane = match Plane::new(origin, normal) {
            Ok(p) => p,
            Err(e) => {
                assert!(!e.to_string().is_empty(), "{at}: Plane::new refused with an empty message");
                // Refused at construction, which is the documented answer. Carry on with a sane plane
                // so the rest of the surface is still exercised.
                Plane::xy(h(&mut rng))
            }
        };
        let _ = plane.v();
        let _ = plane.to_world(h(&mut rng), h(&mut rng));
        let _ = plane.to_local(h3(&mut rng));
        let _ = plane.distance(h3(&mut rng));
        let _ = plane.parallel(&plane.flipped(), h(&mut rng));
        let _ = plane.coplanar(&plane.flipped(), h(&mut rng), h(&mut rng));
        let _ = plane.offset(h(&mut rng));
        let _ = Plane::with_u(origin, normal, h3(&mut rng));
        let _ = Plane::xy(h(&mut rng));
        let _ = Plane::xz(h(&mut rng));
        let _ = Plane::yz(h(&mut rng));
        let _ = tolerance_for(h(&mut rng));

        // --- Face: hostile loops on a finite plane, and a hostile origin on a finite loop.
        let loop_len = 3 + (rng.next() % 3) as usize;
        let pts: Vec<Vec2> = (0..loop_len).map(|_| Vec2::new(h(&mut rng), h(&mut rng))).collect();
        let region = Region::new(pts).to_regions();
        let sane_plane = Plane::xy(0.0);
        if let Ok(f) = Face::new(sane_plane, region) {
            let _ = f.area();
            let _ = f.contains_local(Vec2::new(h(&mut rng), h(&mut rng)));
            let _ = f.vertices();
            let _ = f.interior_point();
            let _ = f.edges();
            let _ = f.retargeted(sane_plane.flipped());
            if let Ok(g) = f.flipped() {
                let _ = g.area();
            }
            if let Ok(s) = Solid::from_faces(vec![f]) {
                let _ = s.bounds();
                let _ = s.tolerance();
                let _ = s.volume();
                let _ = s.area();
                let _ = s.contains(h3(&mut rng));
                let _ = s.edges();
                let _ = s.counts();
                let _ = s.interior_point();
                let _ = s.validate();
                let _ = s.validate_vertices();
                let _ = s.mass_properties();
                let _ = s.volume_from_faces(h3(&mut rng));
                let _ = s.volume_from_faces(s.bounds().center());
                if let Ok(m) = mesh_from_solid(&s) {
                    m.triangles().for_each(|t: Triangle| {
                        let _ = (t.normal(), t.area(), t.center());
                    });
                    let _ = m.bounds();
                    let _ = stl_ascii(&m).len();
                    let _ = stl_binary(&m).len();
                }
                let mut reoriented = s.clone();
                let _ = reoriented.orient_faces();
                let _ = reoriented.fix_orientation();
                let _ = s.flipped();
                for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
                    let _ = boolean(&s, &s, op);
                }
            }
        }
        // A plane carrying a hostile origin is itself finite, so this face is accepted and the
        // arithmetic downstream is what has to survive.
        if let Ok(big) = Plane::new(h3(&mut rng), Vec3::Z) {
            let loop_pts: Vec<Vec2> = (0..4).map(|_| Vec2::new(h(&mut rng), h(&mut rng))).collect();
            if let Ok(f) = Face::new(big, Region::new(loop_pts).to_regions())
                && let Ok(s) = Solid::from_faces(vec![f])
            {
                let _ = s.bounds();
                let _ = s.volume();
                let _ = s.validate();
                let _ = s.validate_vertices();
                let _ = s.volume_from_faces(s.bounds().center());
                let _ = mesh_from_solid(&s).map(|m| stl_binary(&m).len());
            }
        }

        // --- extrude / revolve / support: hostile profiles, planes, distances, angles, counts.
        let _ = support::box_at(h3(&mut rng), h3(&mut rng));
        let _ = support::rect(h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng));
        let _ = support::rect_with_hole(h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng));
        let profile = if case % 2 == 0 {
            Region::new((0..4).map(|_| Vec2::new(h(&mut rng), h(&mut rng))).collect()).to_regions()
        } else {
            support::rect(h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng))
        };
        let _ = extrude(&profile, &sane_plane, h(&mut rng));
        let _ = extrude(&Regions::default(), &sane_plane, h(&mut rng));
        let _ = revolve(&profile, &Plane::xz(0.0), h3(&mut rng), h(&mut rng), (rng.next() % 200) as usize);
        let _ = revolve(&support::rect(1.0, 0.0, 2.0, 1.0), &Plane::xy(0.0), Vec3::Y, 1.0, 0);
        let _ = revolve(&support::rect(1.0, 0.0, 2.0, 1.0), &Plane::xy(0.0), Vec3::Y, 1.0, usize::MAX);

        // --- blends: hostile distances, segment counts and reference points/tolerances.
        let Ok(host_box) = support::box_at(Vec3::ZERO, Vec3::new(10.0, 12.0, 14.0)) else { continue };
        let refs = [EdgeRef::new(h3(&mut rng), h(&mut rng))];
        let _ = chamfer(&host_box, &refs, h(&mut rng));
        let _ = fillet(&host_box, &refs, h(&mut rng), (rng.next() % 200) as usize);
        let _ = chamfer(&host_box, &[], h(&mut rng));
        let _ = chamfer(&host_box, &refs, 1.0);
        let _ = fillet(&host_box, &refs, 1.0, 0);

        // --- a boolean of two hostile solids, which is where the section code does its worst
        // arithmetic: huge coordinates, subnormal ones, and a difference of two huge ones.
        let mk = |rng: &mut Rng| -> Option<Solid> {
            let n = 3 + (rng.next() % 4) as usize;
            let pts: Vec<Vec2> = (0..n)
                .map(|_| match rng.next() % 4 {
                    0 => Vec2::new(1.0e18, rng.f64(10.0)),
                    1 => Vec2::new(rng.f64(10.0), -1.0e18),
                    2 => Vec2::new(rng.f64(1e-300), rng.f64(1e-300)),
                    _ => Vec2::new(rng.f64(1e150), rng.f64(1e150)),
                })
                .collect();
            extrude(&Region::new(pts).to_regions(), &Plane::xy(rng.range(-1e150, 1e150)), rng.range(1e-300, 1e150)).ok()
        };
        if let (Some(x), Some(y)) = (mk(&mut rng), mk(&mut rng)) {
            for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
                let _ = boolean(&x, &y, op);
                let _ = boolean(&y, &x, op);
            }
            if let Ok(good) = support::box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)) {
                for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
                    let _ = boolean(&good, &x, op);
                    let _ = boolean(&x, &good, op);
                }
            }
        }
        // An empty solid is a legal operand (`Solid::default()`); an empty face list is not.
        let _ = boolean(&Solid::default(), &host_box, Bool3::Union);
        let _ = boolean(&host_box, &Solid::default(), Bool3::Union);
        let _ = boolean(&Solid::default(), &Solid::default(), Bool3::Union);
        let _ = Solid::from_faces(Vec::new());
    }
}

// ---------- 9. known kernel failures ----------
//
// The three tests below fail against the kernel as it stands. They are left failing on purpose: each
// is a minimal, deterministic reproduction of a defect in `crates/solid/src`, not a property this
// file has mis-stated. Mark one `#[ignore]` when the defect is fixed.

/// **DEFECT 1 — `boolean.rs`.** Two solids that touch face to face, across a shared plane whose
/// footprints do *not* coincide, cannot be unioned at all.
///
/// The shared plane here is `z = 1`: the first block's top face and the second block's bottom face,
/// with normals `+Z` and `-Z`, i.e. opposite sides of one plane. They overlap over
/// `x in [1, 2]`, `y in [0, 2]`, so the union's boundary along that plane is an L, not a rectangle.
/// The kernel drops the whole plane — `boolean.rs` answers `(Union, true, true, false)` with an
/// empty region on the grounds that "opposite sides" means the plane runs through the interior of the
/// result — and the shell is left open along `x = 0`.
///
/// Only the exactly-aligned stack works: two blocks of the same footprint union to a clean 6-face
/// prism. Everything else refuses, whatever the shape of the step:
///
/// | `a`                          | `b`                          | `union`                     |
/// |------------------------------|------------------------------|-----------------------------|
/// | `(0,0,0)..(2,2,1)`           | `(0,0,1)..(2,2,2)`           | 6 faces, volume 8 (aligned)  |
/// | `(0,0,0)..(2,2,1)`           | `(1,0,1)..(3,2,2)`           | `OpenEdge`, used by 1 face   |
/// | `(0,0,0)..(2,2,1)`           | `(1,-1,1)..(4,3,3)`          | `OpenEdge`, used by 1 face   |
/// | `(0,0,0)..(4,4,1)`           | `(1,1,1)..(3,3,2)`           | `OpenEdge`, used by 1 face   |
/// | `(0,0,0)..(1,1,1)`           | `(1,1,1)..(2,2,2)`           | `OpenEdge`, used by 1 face   |
///
/// `difference` and `intersect` are fine on the same pairs. `boolean.rs` documents the opposite for
/// the union: "Coplanar faces fall out for free. Two boxes sharing a face plane produce one face, not
/// two that then have to be merged and re-welded."
#[test]
fn union_of_two_bodies_touching_across_a_plane() {
    let box_at = |min: Vec3, max: Vec3| must(support::box_at(min, max), "support::box_at", "step");
    // The aligned stack, as a control: this one works.
    let (a, b) = (box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)), box_at(Vec3::new(0.0, 0.0, 1.0), Vec3::new(2.0, 2.0, 2.0)));
    let aligned = must(boolean(&a, &b, Bool3::Union), "union of two exactly stacked blocks", "step");
    assert_eq!(aligned.faces.len(), 6);
    assert!((aligned.volume() - 8.0).abs() < 1e-9, "the aligned stack is a 2x2x2 cube, got {}", aligned.volume());

    // The same two blocks, stepped by one unit in x. The union is an L-shaped prism of volume 12
    // with 10 faces; today it is refused.
    let (a, b) = (box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)), box_at(Vec3::new(1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 2.0)));
    match boolean(&a, &b, Bool3::Union) {
        Ok(s) => {
            assert_valid(&s, "union of two blocks sharing half a face", "step");
            assert!((s.volume() - 12.0).abs() < 1e-9, "a 2x2x1 block and a 2x2x1 block stepped over it enclose 12, got {}", s.volume());
            assert_eq!(s.faces.len(), 10, "an L-shaped prism has 10 faces");
        }
        Err(e) => panic!(
            "DEFECT (crates/solid/src/boolean.rs): union(box (0,0,0)-(2,2,1), box (1,0,1)-(3,2,2)) failed.\n  \
             The two blocks touch across the plane z=1 on opposite sides, overlapping over x in [1,2], y in [0,2].\n  \
             Expected a 10-face L-shaped prism of volume 12; got: {e}"
        ),
    }
}

/// **DEFECT 2 — `boolean.rs`.** The union does not depend on the operand order.
///
/// On one pair of valid solids, `union(a, b)` returns a 13-face solid of volume 9.34e-2 and
/// `union(b, a)` refuses the *same* pair with `InconsistentFaces`. On that pair `intersect` and
/// `difference` both succeed in either order and their volumes agree to 1e-9, so the asymmetry is in
/// the union alone.
///
/// The operands are replayed from the same fixed seed `union_is_commutative` uses, so this test is
/// self-contained: it draws cases 0 to 66 of that test's stream and keeps the 67th. The offending
/// edge runs from `(-0.12517489641765145, -0.2817919269876356, 0.0971722770581193)` to
/// `(-0.12517489641765145, -0.3155866809193366, 0.0971722770581193)` — the first step down `b`'s
/// bottom cap, at the corner `b.min.x, b.max.y, b.min.z`, which the union splits but does not weld.
///
/// `boolean.rs` promises the opposite: "It is symmetric. For union and intersection the plane list and
/// the two sections are the same whichever way round the operands go, so `a u b` and `b u a` agree."
/// The existing unit test `boolean_is_commutative_for_union_and_intersection` compares the two
/// volumes but never checks that *both* orders succeed, so it cannot see this.
#[test]
fn union_does_not_depend_on_the_operand_order() {
    // Replay `union_is_commutative`'s generator up to its case 66: same seed, same 67 draws.
    let seed = 0xC0FF_EE42_u64;
    let mut rng = Rng::new(seed);
    let mut drawn = Case::draw(&mut rng, 0.6, Regime::Overlap, "order");
    for case in 1..=66usize {
        let span = if case % 3 == 0 { 0.6 } else { 14.0 };
        let regime = match case % 3 {
            0 => Regime::Overlap,
            1 => Regime::Grazing,
            _ => Regime::Disjoint,
        };
        drawn = Case::draw(&mut rng, span, regime, "order");
    }
    let at = drawn.report(seed, 66);
    let (a, b) = drawn.build(1.0, &at);
    assert_valid(&a, "operand a", &at);
    assert_valid(&b, "operand b", &at);
    // Control: `intersect` is commutative and `difference` is not, but the two cuts and the
    // intersection must still partition the material between them in both orders.
    let (i, j) = (must(boolean(&a, &b, Bool3::Intersect), "intersect(a, b)", &at), must(boolean(&b, &a, Bool3::Intersect), "intersect(b, a)", &at));
    assert!(near(i.volume(), j.volume(), 1e-9), "{at}: the intersection is order dependent: {} vs {}", i.volume(), j.volume());
    for (x, y, mine, yours) in [(&a, &b, "a", "b"), (&b, &a, "b", "a")] {
        let cut = must(boolean(x, y, Bool3::Difference), "difference", &at);
        assert!(
            near(cut.volume() + i.volume(), x.volume(), 1e-9),
            "{at}: vol({mine} \\ {yours}) {} + vol(a n b) {} != vol({mine}) {}",
            cut.volume(),
            i.volume(),
            x.volume()
        );
    }

    match (boolean(&a, &b, Bool3::Union), boolean(&b, &a, Bool3::Union)) {
        (Ok(x), Ok(y)) => {
            assert!(near(x.volume(), y.volume(), 1e-12), "{at}: vol(a u b) {} vs vol(b u a) {}", x.volume(), y.volume());
        }
        (Ok(s), Err(e)) => panic!(
            "{at}\n    union(a, b) returned a valid solid of {} faces and volume {:e},\n    union(b, a) refused the same pair: {e}\n    \
             DEFECT (crates/solid/src/boolean.rs): the union is not symmetric in its operands.",
            s.faces.len(),
            s.volume()
        ),
        (Err(e), Ok(s)) => panic!(
            "{at}\n    union(b, a) returned a valid solid of {} faces and volume {:e},\n    union(a, b) refused the same pair: {e}\n    \
             DEFECT (crates/solid/src/boolean.rs): the union is not symmetric in its operands.",
            s.faces.len(),
            s.volume()
        ),
        (Err(e), Err(f)) => panic!("{at}\n    union refused in both orders, so there is nothing to compare: {e} / {f}"),
    }
}

/// **DEFECT 3 — `extrude.rs`.** `revolve` cannot build a solid from any input at all.
///
/// `triangle_face` builds its face from a two-point region:
///
/// ```text
/// let region = Region::new(vec![plane.to_local(*b), plane.to_local(*c)]).to_regions();
/// ```
///
/// `Region::is_empty` is `outer.len() < 3`, so a two-point region is empty, `to_regions` turns it
/// into `Regions::default`, and `Face::new` rejects it with `SolidError::Empty`. The `.ok()` swallows
/// that, so **every** swept patch face is silently dropped and the function never reaches a solid.
/// A full turn has no caps and so ends at `Solid::from_faces(vec![])` → `Empty`; a partial turn keeps
/// only its two end caps and fails `validate` with an edge "used 2 times forward".
///
/// The third vertex is free: the plane's origin is `a`, so `a` is at local `(0, 0)` and the region
/// wants `vec![Vec2::ZERO, to_local(b), to_local(c)]`.
///
/// `revolve` is re-exported from the crate root and has no unit test of its own — `extrude.rs`'s test
/// module covers `extrude` only.
#[test]
fn revolve_builds_no_solid() {
    // A rectangle well clear of the axis, revolved a full turn: a cylinder of radius 1, height 1.
    let profile = support::rect(1.0, 0.0, 2.0, 1.0);
    for (name, plane, axis, angle, segments) in [
        ("full turn about +Y", Plane::xy(0.0), Vec3::Y, std::f64::consts::TAU, 8usize),
        ("quarter turn about +Y", Plane::xy(0.0), Vec3::Y, std::f64::consts::FRAC_PI_2, 8),
        ("full turn about +Z", Plane::yz(0.0), Vec3::Z, std::f64::consts::TAU, 8),
    ] {
        match revolve(&profile, &plane, axis, angle, segments) {
            Ok(s) => {
                assert_valid(&s, name, "revolve");
                assert!(s.volume() > 0.0, "{name}: revolve of a 1x1 rectangle 1 away from the axis encloses nothing");
            }
            Err(e) => panic!(
                "DEFECT (crates/solid/src/extrude.rs): revolve({name}) refused.\n  Expected a faceted cylinder of radius 1 and height 1; got: {e}"
            ),
        }
    }
}
