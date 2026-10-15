//! Fuzzing at every public boundary of the solid kernel.
//!
//! `tests/property.rs` has one `hostile_input_never_panics` test. This file is the systematic
//! sweep behind it: **every** public entry point gets hostile and near-degenerate input, every
//! degenerate topology family gets built on purpose, and the result is counted rather than merely
//! survived.
//!
//! # What the property is
//
//! **Survival.** The assertion is that the function is *reached the end of*. There is no
//! `#[should_panic]` anywhere in this file, and there must never be one: a test that passes because
//! the code panicked is not a test. A panic, an abort, a stack overflow or a hang is the failure;
//! everything a function returns is data.
//!
//! # What is counted, and why the counts matter
//
//! Each call is classified as `refused` (an `Err`) or `built` (an `Ok`). The ratio is printed. A
//! high refusal rate is a **finding**, not something to tune away — a kernel that refuses 90 % of
//! hostile input is technically alive and practically useless, and the number is what shows that.
//! Conversely a low refusal rate on input that *should* be refused is equally a finding: a
//! `boolean` that silently returns a solid for `NaN` coordinates is worse than one that says no.
//!
//! # What is deliberately not fuzzed here, and why
//!
//! - **`MAX_TRIANGLES` (2 000 000).** Reaching it means tessellating two million triangles; at
//!   roughly 100 bytes each that is a two-minute test for a bound that is already checked on the
//!   face-count path. `limits::MAX_TRIANGLES` is asserted to exist and the cap logic is exercised
//!   indirectly through [`every_generated_solid_is_a_value_that_survives_a_round_trip`].
//! - **`Model::load_json`.** The model is a different crate and a different layer (L3 against L0);
//!   a dependency from here would be a layering violation. File bytes are fuzzed in
//!   `crates/model/tests/fuzz.rs`.
//! - **`section`.** Not exported (see `tests/property_surface.rs`); reachable only through
//!   `boolean`, which is fuzzed here.
//! - **Stack depth.** Nothing in this crate recurses over input, and `MAX_BLOCK_DEPTH`-style guards
//!   live above it. There is no recursion to overflow here.
//!
//! Deterministic: a fixed-seed xorshift64\* generator per test, no `rand`, no wall clock. Every
//! failure message carries the seed and the case index.
//!
//! **Execution status: every test in this file is `NOT EXECUTED`.** It was written without a
//! terminal and has never been compiled or run; see the report that came with it.

use cadcraft_solid::{
    Bool3, Edge, EdgeRef, Face, MassProperties, Mesh, Plane, Region, Regions, Solid, SolidError, Triangle, Vec2, Vec3, boolean, chamfer, extrude,
    fillet, limits, mass_properties, mesh_from_solid, reverse_regions, revolve, stl_ascii, stl_binary, step_faceted_brep, step_schema, support,
    tolerance_for,
};

// ---------- generator ----------
//
// The same xorshift64* as `geom/src/property_tests.rs` and `tests/property.rs`.

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
    fn pick(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    /// An f64 chosen to be nasty: non-finite, subnormal, extreme, or a raw bit pattern.
    fn hostile(&mut self) -> f64 {
        match self.next() % 14 {
            0 => 0.0,
            1 => -0.0,
            2 => f64::NAN,
            3 => f64::INFINITY,
            4 => f64::NEG_INFINITY,
            5 => 1.0e300,
            6 => -1.0e300,
            7 => 5.0e-324,
            8 => f64::MIN_POSITIVE,
            9 => f64::MAX,
            10 => -f64::MAX,
            11 => 1.0e-300,
            _ => f64::from_bits(self.next()),
        }
    }
    /// A number that is finite and ordinary but hostile in *context*: a distance, an angle, a
    /// radius, a segment count. Zero, negative, huge, tiny.
    fn degenerate(&mut self) -> f64 {
        match self.next() % 8 {
            0 => 0.0,
            1 => -1.0,
            2 => -1.0e-12,
            3 => 1.0e300,
            4 => 5.0e-324,
            5 => 1.0e-12,
            6 => -0.0,
            _ => self.f64(1.0e3),
        }
    }
}

// ---------- bookkeeping ----------

/// How many calls were refused and how many produced geometry.
///
/// The whole point of counting rather than merely surviving: the two numbers are a property of the
/// kernel's behaviour on hostile input, and they are what a reader of the test output learns.
#[derive(Default, Clone, Copy)]
struct Tally {
    refused: usize,
    built: usize,
}

impl Tally {
    fn record<T>(&mut self, r: Result<T, SolidError>) {
        match r {
            Ok(_) => self.built += 1,
            Err(_) => self.refused += 1,
        }
    }

    /// Record, and require that a refusal explains itself. An `Err` with an empty message is a
    /// failure everywhere else in this crate's contract, so it is a failure here too.
    fn record_named<T>(&mut self, r: Result<T, SolidError>, what: &str, at: &str) {
        match r {
            Ok(_) => self.built += 1,
            Err(e) => {
                let message = e.to_string();
                assert!(!message.trim().is_empty(), "{at}: {what} refused with an empty message");
                self.refused += 1;
            }
        }
    }

    fn report(&self, what: &str) {
        println!("{what}: {} refused, {} built", self.refused, self.built);
        assert!(self.refused + self.built > 0, "{what}: nothing was called");
    }
}

fn tag(seed: u64, case: usize) -> String {
    format!("seed {seed:#x}, case {case}")
}

// ---------- degenerate-topology generators ----------
//
// Each returns a loop or a region that is legal Rust and illegal geometry. They are written once
// here rather than inline at each call site so that a family cannot be forgotten at one entry
// point and covered at another.

/// A loop with no vertices, one, or two.
fn stub_loop(n: usize) -> Vec<Vec2> {
    (0..n).map(|i| Vec2::new(i as f64, 0.0)).collect()
}

/// The same point over and over.
fn all_the_same_point(n: usize) -> Vec<Vec2> {
    vec![Vec2::new(1.0, 1.0); n]
}

/// Every point equal to the first, so the last edge closes on itself too.
fn collapsed(n: usize) -> Vec<Vec2> {
    let mut v = vec![Vec2::new(2.0, 2.0); n.max(1)];
    if let Some(first) = v.first() {
        v[0] = *first;
    }
    v
}

/// A figure-eight: the loop crosses itself in the middle, so "inside" is not even well defined.
fn figure_eight() -> Vec<Vec2> {
    vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)]
}

/// A bow-tie drawn the other way: the two lobes have opposite orientation.
fn bow_tie_reversed() -> Vec<Vec2> {
    vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 4.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 0.0)]
}

/// A rectangle whose winding is clockwise: the outer loop of a region must be counter-clockwise.
fn clockwise_rectangle() -> Vec<Vec2> {
    vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 4.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 0.0)]
}

/// A rectangle whose last vertex repeats its first, so the closing edge has zero length.
fn rectangle_with_a_doubled_corner() -> Vec<Vec2> {
    vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0), Vec2::new(0.0, 0.0)]
}

/// Two rectangles sharing one edge point exactly — a loop that touches itself at a vertex.
fn self_touching() -> Vec<Vec2> {
    vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(4.0, 0.0),
        Vec2::new(4.0, 4.0),
        Vec2::new(2.0, 2.0),
        Vec2::new(0.0, 4.0),
        Vec2::new(0.0, 0.0),
    ]
}

/// Coordinates spread over the whole finite range, so every difference overflows or underflows.
fn enormous_spread(rng: &mut Rng) -> Vec<Vec2> {
    let pick = |rng: &mut Rng| -> Vec2 {
        match rng.next() % 6 {
            0 => Vec2::new(1.0e300, -1.0e300),
            1 => Vec2::new(5.0e-324, 5.0e-324),
            2 => Vec2::new(f64::MAX, f64::MAX),
            3 => Vec2::new(-f64::MAX, f64::MAX),
            4 => Vec2::new(1.0e-150, 1.0e150),
            _ => Vec2::new(rng.f64(1.0), rng.f64(1.0)),
        }
    };
    (0..4).map(|_| pick(rng)).collect()
}

/// Every region a profile can be, legal or not.
fn hostile_profiles(rng: &mut Rng) -> Vec<Regions> {
    let mut out: Vec<Regions> = Vec::new();
    out.push(Regions::default());
    for n in [0usize, 1, 2, 3, 5] {
        out.push(Region::new(stub_loop(n)).to_regions());
        out.push(Region::new(all_the_same_point(n.max(1))).to_regions());
        out.push(Region::new(collapsed(n.max(3))).to_regions());
    }
    out.push(Region::new(figure_eight()).to_regions());
    out.push(Region::new(bow_tie_reversed()).to_regions());
    out.push(Region::new(clockwise_rectangle()).to_regions());
    out.push(Region::new(rectangle_with_a_doubled_corner()).to_regions());
    out.push(Region::new(self_touching()).to_regions());
    // A hole that is not inside the outer loop.
    out.push(
        Region {
            outer: clockwise_rectangle(),
            holes: vec![vec![Vec2::new(100.0, 100.0), Vec2::new(104.0, 100.0), Vec2::new(104.0, 104.0), Vec2::new(100.0, 104.0)]],
        }
        .to_regions(),
    );
    // A hole that swallows the outer loop.
    out.push(
        Region {
            outer: clockwise_rectangle(),
            holes: vec![vec![Vec2::new(-10.0, -10.0), Vec2::new(-10.0, 14.0), Vec2::new(14.0, 14.0), Vec2::new(14.0, -10.0)]],
        }
        .to_regions(),
    );
    // The same hole twice.
    out.push(
        Region { outer: clockwise_rectangle(), holes: vec![clockwise_rectangle(), clockwise_rectangle()] }.to_regions(),
    );
    // A hole touching the outer loop exactly along an edge.
    out.push(
        Region { outer: clockwise_rectangle(), holes: vec![vec![Vec2::new(0.0, 1.0), Vec2::new(0.0, 3.0), Vec2::new(2.0, 3.0), Vec2::new(2.0, 1.0)]] }.to_regions(),
    );
    // Nested holes, which is a third level of nesting the kernel does not model.
    out.push(
        Region {
            outer: clockwise_rectangle(),
            holes: vec![
                vec![Vec2::new(0.5, 0.5), Vec2::new(0.5, 3.5), Vec2::new(3.5, 3.5), Vec2::new(3.5, 0.5)],
                vec![Vec2::new(1.5, 1.5), Vec2::new(1.5, 2.5), Vec2::new(2.5, 2.5), Vec2::new(2.5, 1.5)],
            ],
        }
        .to_regions(),
    );
    // Several disjoint regions in one set, which is legal and which `contains` must handle.
    out.push(Regions::new(vec![
        Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]),
        Region::new(vec![Vec2::new(5.0, 5.0), Vec2::new(6.0, 5.0), Vec2::new(6.0, 6.0), Vec2::new(5.0, 6.0)]),
    ]));
    // Raw hostile coordinates.
    for _ in 0..4 {
        out.push(Region::new(enormous_spread(rng)).to_regions());
        out.push(
            Region {
                outer: (0..4).map(|_| Vec2::new(rng.hostile(), rng.hostile())).collect(),
                holes: vec![(0..4).map(|_| Vec2::new(rng.hostile(), rng.hostile())).collect()],
            }
            .to_regions(),
        );
    }
    // Loops longer than the cap.
    out.push(Region::new((0..=limits::MAX_LOOP_VERTS).map(|i| Vec2::new(i as f64, (i % 7) as f64)).collect()).to_regions());
    out
}

// ====================================================================================
// 1. planes
// ====================================================================================

/// Every `Plane` constructor and every query, with hostile origins, normals and in-plane axes.
///
/// `Plane::new` is the gate the whole face representation rests on, and `Plane::with_u` is the one
/// a caller supplies a frame to. Both are asked for planes that cannot exist — a zero normal, a
/// NaN origin, a `u` parallel to the normal, a subnormal direction — and then, where construction
/// succeeded, every query is asked of the result. A plane with a hostile origin is *finite*, so it
/// is accepted, and the arithmetic downstream is what has to survive.
#[test]
fn planes_survive_hostile_origins_normals_and_axes() {
    const CASES: usize = 300;
    let seed = 0xF1A0_0001_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    for case in 0..CASES {
        let at = tag(seed, case);
        let origin = rng.vec3(1.0e6);
        let normal = rng.vec3(1.0);
        match Plane::new(origin, normal) {
            Ok(p) => {
                tally.built += 1;
                // Every query, on a plane that exists.
                let _ = p.v();
                let _ = p.to_world(rng.hostile(), rng.hostile());
                let _ = p.to_local(rng.vec3(1.0e300));
                let _ = p.distance(rng.vec3(1.0e300));
                let _ = p.parallel(&p.flipped(), rng.degenerate());
                let _ = p.coplanar(&p.flipped(), rng.degenerate(), rng.degenerate());
                let _ = p.offset(rng.hostile());
                let flipped = p.flipped();
                let _ = flipped.to_world(0.0, 0.0);
                let _ = flipped.to_local(origin);
                // A frame must stay right-handed and unit-length whatever the input was.
                if p.normal.is_finite() && p.u.is_finite() {
                    let v = p.v();
                    assert!(v.is_finite(), "{at}: v() is not finite for {p:?}");
                    assert!(near(p.normal.len(), 1.0, 1e-12), "{at}: the normal is not unit: {}", p.normal.len());
                    assert!(near(p.u.len(), 1.0, 1e-12), "{at}: u is not unit: {}", p.u.len());
                    assert!(near(v.len(), 1.0, 1e-12), "{at}: v is not unit: {}", v.len());
                    assert!(p.u.cross(v).near(p.normal, 1e-9), "{at}: the frame is not right handed for {p:?}");
                    assert!(p.u.dot(p.normal).abs() < 1e-9, "{at}: u is not in the plane for {p:?}");
                }
                // `with_u` on the same hostile data.
                tally.record_named(Plane::with_u(origin, normal, rng.vec3(1.0)), "Plane::with_u", &at);
            }
            Err(e) => {
                tally.refused += 1;
                assert!(!e.to_string().trim().is_empty(), "{at}: refused with an empty message");
            }
        }
        // The three named planes take any height at all, including a hostile one.
        let (x, y, z) = (rng.hostile(), rng.hostile(), rng.hostile());
        for p in [Plane::xy(z), Plane::xz(y), Plane::yz(x)] {
            let _ = p.to_world(x, y);
            let _ = p.to_local(rng.vec3(1.0e300));
            let _ = p.distance(rng.vec3(1.0e300));
            let _ = p.offset(rng.hostile());
            let _ = p.v();
            let _ = p.flipped().to_local(rng.vec3(1.0));
        }
        tally.record_named(Plane::new(rng.vec3(1.0), rng.vec3(0.0)), "Plane::new with a zero normal", &at);
        tally.record_named(Plane::with_u(rng.vec3(1.0), Vec3::Z, Vec3::Z), "Plane::with_u parallel to the normal", &at);
        tally.record_named(Plane::with_u(rng.vec3(1.0), Vec3::Z, rng.vec3(0.0)), "Plane::with_u with a zero axis", &at);
    }
    tally.report("planes");
}

/// `tolerance_for` survives, and never returns something a clamp would not.
#[test]
fn the_tolerance_never_panics_and_is_always_a_clamped_positive_number() {
    const CASES: usize = 4000;
    let seed = 0xF1A0_0002_u64;
    let mut rng = Rng::new(seed);
    for case in 0..CASES {
        let at = tag(seed, case);
        let t = tolerance_for(rng.hostile());
        assert!(t.is_finite(), "{at}: tolerance_for gave {t}");
        assert!((1e-12..=1e-3).contains(&t), "{at}: tolerance_for gave {t}, outside the documented clamp");
    }
    println!("tolerance_for: 4000 hostile diagonals, every one clamped into [1e-12, 1e-3]");
}

// ====================================================================================
// 2. regions
// ====================================================================================

/// Every degenerate region shape through every region query and through `Face::new`.
///
/// The families are built once in [`hostile_profiles`] and pushed through `area`, `signed_area`,
/// `bounds`, `is_empty`, `contains`, `loops`, `regions`, `regions_mut`, `Regions::from_loops` and
/// `Face::new`. A region query is allowed to be wrong about a self-intersecting loop — there is no
/// right answer — but it may not hang, overflow to infinity silently, or panic.
#[test]
fn regions_survive_every_degenerate_topology() {
    let seed = 0xF1A0_0003_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    let plane = Plane::xy(0.0);
    let profiles = hostile_profiles(&mut rng);
    for (i, p) in profiles.iter().enumerate() {
        let at = format!("profile {i} of {}", profiles.len());
        // Queries. None of these can fail, so nothing is asserted about the *values* beyond
        // finiteness where finiteness is meaningful.
        let area = p.area();
        assert!(!area.is_nan(), "{at}: area is NaN");
        for r in p.iter() {
            assert!(!r.signed_area().is_nan(), "{at}: signed_area is NaN");
            let bb = r.bounds();
            assert!(!bb.width().is_nan() && !bb.height().is_nan(), "{at}: bounds are NaN");
        }
        let _ = p.bounds();
        let _ = p.is_empty();
        let _ = p.contains(Vec2::new(rng.hostile(), rng.hostile()));
        let _: usize = p.loops().map(|l| l.len()).sum();
        let _: usize = p.regions().len();

        // Mutation through `regions_mut`, then the same queries again.
        let mut copy = p.clone();
        if let Some(r) = copy.regions_mut().first_mut() {
            r.outer.push(Vec2::new(rng.hostile(), rng.hostile()));
            r.holes.push(vec![Vec2::ZERO, Vec2::X, Vec2::Y]);
        }
        assert!(!copy.area().is_nan(), "{at}: area is NaN after mutation");
        let _ = copy.contains(Vec2::ZERO);

        // `from_loops` sorts loops into regions by nesting depth. Degenerate loops here are the
        // interesting input, not an accident.
        let loops: Vec<Vec<Vec2>> = p.loops().cloned().collect();
        for tol in [0.0, 1e-12, 1e-9, 1.0, f64::NAN, -1.0, 1e300] {
            match Regions::from_loops(loops.clone(), tol) {
                Some(r) => {
                    tally.built += 1;
                    assert!(!r.area().is_nan(), "{at}: from_loops produced a NaN area at tol {tol:e}");
                    let _ = r.contains(Vec2::ZERO);
                    let _ = r.is_empty();
                }
                None => tally.refused += 1,
            }
        }

        // And into a face, which is the gate that decides whether any of this can become a solid.
        tally.record_named(Face::new(plane, p.clone()), "Face::new", &at);
        // And through a whole extrusion, which is where a region actually does damage.
        for distance in [1.0, 1e-9, 1e-300, 1e300] {
            tally.record_named(extrude(p, &plane, distance), "extrude", &at);
        }
        // And reversed, which `extrude` uses for its caps.
        let _ = reverse_regions(p);
    }
    tally.report("regions");
}

// ====================================================================================
// 3. faces and solids
// ====================================================================================

/// A face built from hostile geometry, and every solid assembled from it.
///
/// `Face::new` only checks finiteness and caps, so a face carrying `1e300` coordinates is
/// *accepted*. Everything downstream of that — `area`, `vertices`, `edges`, `interior_point`, and
/// then `Solid::from_faces`, `volume`, `bounds`, `contains`, `edges`, `validate`,
/// `validate_vertices`, `orient_faces`, `fix_orientation`, `mass_properties`, `volume_from_faces`,
/// `mesh_from_solid` — is where the arithmetic has to hold together.
#[test]
fn faces_and_solids_survive_hostile_geometry() {
    const CASES: usize = 300;
    let seed = 0xF1A0_0004_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    for case in 0..CASES {
        let at = tag(seed, case);
        let loop_len = 3 + rng.pick(4);
        let pts: Vec<Vec2> = (0..loop_len).map(|_| match rng.next() % 5 {
            0 => Vec2::new(rng.hostile(), rng.hostile()),
            1 => Vec2::new(1.0e300, rng.f64(10.0)),
            2 => Vec2::new(rng.f64(1e-300), rng.f64(1e-300)),
            3 => Vec2::new(f64::MAX, -f64::MAX),
            _ => Vec2::new(rng.f64(10.0), rng.f64(10.0)),
        })
        .collect();
        let region = Region::new(pts).to_regions();
        let sane_plane = Plane::xy(rng.f64(10.0));

        let Ok(face) = Face::new(sane_plane, region) else {
            tally.refused += 1;
            continue;
        };
        tally.built += 1;
        let _ = face.area();
        let _ = face.contains_local(Vec2::new(rng.hostile(), rng.hostile()));
        let _ = face.vertices();
        let _ = face.interior_point();
        let _ = face.edges();
        let _ = face.retargeted(sane_plane.flipped());
        if let Ok(flipped) = face.flipped() {
            let _ = flipped.area();
            let _ = flipped.vertices();
            let _ = flipped.edges();
            let _ = flipped.interior_point();
            tally.built += 1;
        } else {
            tally.refused += 1;
        }

        // A plane carrying a hostile origin is finite, so `Face::new` accepts it and the arithmetic
        // downstream is what has to survive. `Plane::new` would refuse, so it is built by hand.
        for origin in [Vec3::new(1.0e300, 0.0, 0.0), Vec3::new(-1.0e300, 1.0e300, 0.0), Vec3::new(1.0e-300, 0.0, 0.0)] {
            let hostile_plane = Plane { origin, normal: Vec3::Z, u: Vec3::X };
            match Face::new(hostile_plane, Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)]).to_regions()) {
                Ok(f) => {
                    tally.built += 1;
                    let _ = f.area();
                    let _ = f.vertices();
                    let _ = f.interior_point();
                    match Solid::from_faces(vec![f]) {
                        Ok(s) => exercise_solid(&s, &mut rng, &at),
                        Err(_) => tally.refused += 1,
                    }
                }
                Err(_) => tally.refused += 1,
            }
        }

        // A shell of several hostile faces at once, which is where the edge bookkeeping is at its
        // worst: every edge key is a quantised coordinate and quantising 1e300 overflows.
        let mut faces = Vec::new();
        for _ in 0..2 + rng.pick(4) {
            let n = 3 + rng.pick(3);
            let p: Vec<Vec2> = (0..n).map(|_| Vec2::new(rng.f64(1.0e6), rng.f64(1.0e6))).collect();
            let pl = match Plane::new(rng.vec3(1.0e3), rng.vec3(1.0)) {
                Ok(pl) => pl,
                Err(_) => {
                    tally.refused += 1;
                    continue;
                }
            };
            match Face::new(pl, Region::new(p).to_regions()) {
                Ok(f) => faces.push(f),
                Err(_) => tally.refused += 1,
            }
        }
        if let Ok(s) = Solid::from_faces(faces) {
            tally.built += 1;
            exercise_solid(&s, &mut rng, &at);
        }
    }
    tally.report("faces and solids");
}

/// Every `Solid` method, on whatever shell was handed over.
///
/// The functions that can fail are counted; the ones that cannot are called and their results
/// dropped. Nothing here asserts a *value* — the property is that the calls complete and the ones
/// that promise a `Result` honour it.
fn exercise_solid(s: &Solid, rng: &mut Rng, at: &str) {
    let _ = s.bounds();
    let _ = s.tolerance();
    let _ = s.volume();
    let _ = s.area();
    let _ = s.contains(rng.vec3(1.0e6));
    let _ = s.edges();
    let _ = s.counts();
    let _ = s.interior_point();
    let _ = s.validate();
    let _ = s.validate_vertices();
    let _ = s.mass_properties();
    let _ = s.volume_from_faces(rng.vec3(1.0e6));
    let _ = s.volume_from_faces(s.bounds().center());
    let _ = s.flipped();
    let _ = stl_ascii(&Mesh::default()).len();
    let _ = stl_binary(&Mesh::default()).len();
    let _ = step_faceted_brep(s, "a name with 'quotes' and \\ backslashes");
    let _ = step_faceted_brep(s, "");
    let _ = step_schema();

    let mut reoriented = s.clone();
    let _ = reoriented.orient_faces();
    let _ = reoriented.fix_orientation();
    let _ = reoriented.validate();

    match mesh_from_solid(s) {
        Ok(m) => {
            let _: usize = m.tris.len();
            let _ = m.bounds();
            for t in m.triangles() {
                let _: Triangle = t;
                let _ = t.normal();
                let _ = t.area();
                let _ = t.center();
            }
            let _ = stl_ascii(&m).len();
            let bytes = stl_binary(&m);
            assert_eq!(bytes.len(), 84 + m.tris.len() * 50, "{at}: the binary STL is not 84 + 50n bytes");
        }
        Err(_) => {}
    }
    let m: Option<MassProperties> = s.mass_properties().ok();
    let _ = m.map(|p| (p.volume, p.area, p.centroid));
}

/// The size and count caps are refusals, not allocations.
///
/// `Solid::from_faces` caps faces, `Face::new` caps loop vertices, `revolve` and `fillet` cap
/// segments and `resolve` caps blends. Every one of those caps is an error path, and an error path
/// that allocates first is a denial of service. The inputs below are one element past each cap.
#[test]
fn every_cap_is_a_refusal_rather_than_an_allocation() {
    let plane = Plane::xy(0.0);
    let at = "caps";

    // MAX_FACES + 1 faces. Built once: the point is that the check happens before any of them are
    // examined, not that sixty thousand clones are fast.
    let one = must_face(plane);
    let too_many_faces: Vec<Face> = vec![one.clone(); limits::MAX_FACES + 1];
    assert_eq!(too_many_faces.len(), limits::MAX_FACES + 1);
    match Solid::from_faces(too_many_faces) {
        Err(SolidError::TooManyFaces(n, cap)) => assert_eq!((n, cap), (limits::MAX_FACES + 1, limits::MAX_FACES)),
        Ok(_) => panic!("{} faces was accepted", limits::MAX_FACES + 1),
        Err(other) => panic!("{at}: a face-count overflow reported {other:?}"),
    }
    // One under the cap is accepted as a value (validation is a separate question).
    let at_cap: Vec<Face> = vec![one; limits::MAX_FACES];
    assert!(Solid::from_faces(at_cap).is_ok(), "exactly MAX_FACES faces is not over the cap");

    // MAX_LOOP_VERTS + 1 vertices in one loop.
    let over: Vec<Vec2> = (0..=limits::MAX_LOOP_VERTS).map(|i| Vec2::new(i as f64, 0.0)).collect();
    match Face::new(plane, Region::new(over).to_regions()) {
        Err(SolidError::TooManyFaces(n, cap)) => assert_eq!((n, cap), (limits::MAX_LOOP_VERTS + 1, limits::MAX_LOOP_VERTS)),
        Ok(_) => panic!("{} vertices in a loop was accepted", limits::MAX_LOOP_VERTS + 1),
        Err(other) => panic!("{at}: a loop-vertex overflow reported {other:?}"),
    }

    // Segment counts, on `revolve` and `fillet`.
    let profile = support::rect(1.0, 0.0, 2.0, 1.0);
    for segments in [0usize, limits::MAX_ROUND_SEGMENTS + 1, usize::MAX, usize::MAX / 2] {
        let r = revolve(&profile, &Plane::xy(0.0), Vec3::Y, std::f64::consts::FRAC_PI_2, segments);
        assert!(r.is_err(), "{at}: revolve accepted {segments} segments");
    }
    let box_solid = must_box();
    let edge_ref = [EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)];
    for segments in [0usize, limits::MAX_ROUND_SEGMENTS + 1, usize::MAX] {
        assert!(fillet(&box_solid, &edge_ref, 1.0, segments).is_err(), "{at}: fillet accepted {segments} segments");
    }

    // More references than MAX_BLENDS.
    let many: Vec<EdgeRef> = (0..=limits::MAX_BLENDS).map(|i| EdgeRef::new(Vec3::new(10.0, 0.0, 1.0 + (i % 90) as f64 * 0.1), 1e-6)).collect();
    assert_eq!(many.len(), limits::MAX_BLENDS + 1);
    match chamfer(&box_solid, &many, 0.5) {
        Err(e) => assert!(e.to_string().contains("limit"), "{at}: the blend cap reported {e}"),
        Ok(_) => panic!("{at}: chamfer accepted {} references", limits::MAX_BLENDS + 1),
    }

    // And the limits module says what the docs say.
    assert_eq!(limits::MAX_FACES, 60_000);
    assert_eq!(limits::MAX_LOOP_VERTS, 60_000);
    assert_eq!(limits::MAX_WELD_VERTICES, 20_000);
    assert_eq!(limits::MAX_TRIANGLES, 2_000_000);
    assert_eq!(limits::MAX_ROUND_SEGMENTS, 64);
    assert_eq!(limits::MAX_BLENDS, 2_000);
    println!("caps: MAX_FACES, MAX_LOOP_VERTS, MAX_ROUND_SEGMENTS and MAX_BLENDS all refused at +1; the constants are the documented ones");
}

fn must_face(plane: Plane) -> Face {
    match Face::new(
        plane,
        Region::new(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]).to_regions(),
    ) {
        Ok(f) => f,
        Err(e) => panic!("the fixture face should build: {e}"),
    }
}

fn must_box() -> Solid {
    match support::box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)) {
        Ok(b) => b,
        Err(e) => panic!("the fixture box should build: {e}"),
    }
}

// ====================================================================================
// 4. the builders
// ====================================================================================

/// `extrude`, `revolve` and `support::*` with degenerate distances, angles, axes and profiles.
///
/// The specific shapes this adds over the region sweep: **zero and negative distances** (which the
/// docs say are "a negative distance with the normal flipped", not an error — so a negative
/// distance is interesting rather than automatically refused), **angles of zero, a full turn, and
/// a hundred turns**, **axes that are not directions**, and **revolve planes that do not contain
/// the axis**, which `revolve` documents as a refusal.
#[test]
fn the_builders_survive_degenerate_distances_angles_and_axes() {
    const CASES: usize = 300;
    let seed = 0xF1A0_0006_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    let profiles: Vec<Regions> = vec![
        support::rect(1.0, 0.0, 2.0, 1.0),
        support::rect_with_hole(1.0, 0.0, 6.0, 5.0, 2.0, 1.0, 5.0, 4.0),
        Region::new(figure_eight()).to_regions(),
        Region::new(clockwise_rectangle()).to_regions(),
        Regions::default(),
    ];
    let distances = [0.0, -1.0, -1e-300, 1e-300, 1.0, 1e300, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE];
    let angles = [0.0, -1.0, 1e-12, std::f64::consts::FRAC_PI_2, std::f64::consts::TAU, 100.0 * std::f64::consts::TAU, f64::NAN];
    let axes = [Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z, Vec3::new(1e-300, 1.0e-300, 1.0e-300), Vec3::new(f64::NAN, 0.0, 0.0)];

    for case in 0..CASES {
        let at = tag(seed, case);
        let profile = profiles.get(rng.pick(profiles.len())).cloned().unwrap_or_default();
        let distance = distances.get(rng.pick(distances.len())).copied().unwrap_or(1.0);
        tally.record_named(extrude(&profile, &Plane::xy(rng.f64(10.0)), distance), "extrude", &at);

        let angle = angles.get(rng.pick(angles.len())).copied().unwrap_or(1.0);
        let axis = axes.get(rng.pick(axes.len())).copied().unwrap_or(Vec3::Y);
        let segments = rng.pick(limits::MAX_ROUND_SEGMENTS + 4);
        // Both a plane whose `v` is the axis and one whose `v` is not: the second must be refused
        // with a reason rather than swept into nothing.
        for plane in [Plane::xy(0.0), Plane::xz(0.0), Plane::yz(0.0)] {
            tally.record_named(revolve(&profile, &plane, axis, angle, segments), "revolve", &at);
        }

        // `support` builders with hostile extents. `box_at` with min > max is the interesting one:
        // it is not an empty box, it is a box built the other way round.
        let (lo, hi) = (rng.vec3(1.0), rng.vec3(1.0));
        tally.record_named(support::box_at(lo, hi), "support::box_at", &at);
        tally.record_named(support::box_at(hi, lo), "support::box_at (reversed)", &at);
        tally.record_named(support::box_at(rng.vec3(1.0e300), rng.vec3(1.0e300)), "support::box_at (a point)", &at);
        let h = |rng: &mut Rng| rng.hostile();
        let _ = support::rect(h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng));
        let _ = support::rect_with_hole(h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng), h(&mut rng));
        let _ = support::rect(f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::MAX);
    }
    tally.report("builders");
}

/// A boolean over two hostile solids, in both operand orders, for all three operations.
///
/// This is where the kernel does the most arithmetic on the least trustworthy numbers: every face
/// plane of both operands becomes a sectioning plane, and each section chains, merges and sorts
/// loops in the hostile solid's own frame. The cases include enormous coordinate spreads *within
/// one solid* (every difference overflows), subnormal coordinates, and a difference of two solids
/// whose bounding boxes touch but whose material does not.
#[test]
fn booleans_survive_hostile_operands_in_both_orders() {
    const CASES: usize = 240;
    let seed = 0xF1A0_0007_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    let good = must_box();

    let build = |rng: &mut Rng| -> Option<Solid> {
        let n = 3 + rng.pick(4);
        let pts: Vec<Vec2> = (0..n)
            .map(|_| match rng.next() % 6 {
                0 => Vec2::new(1.0e18, rng.f64(10.0)),
                1 => Vec2::new(rng.f64(10.0), -1.0e18),
                2 => Vec2::new(rng.f64(1e-300), rng.f64(1e-300)),
                3 => Vec2::new(rng.f64(1e150), rng.f64(1e150)),
                4 => Vec2::new(rng.f64(10.0), rng.f64(10.0)),
                _ => Vec2::new(rng.f64(1.0), rng.f64(1.0)),
            })
            .collect();
        extrude(&Region::new(pts).to_regions(), &Plane::xy(rng.range(-1e150, 1e150)), rng.range(1e-300, 1e150)).ok()
    };

    for case in 0..CASES {
        let at = tag(seed, case);
        let (Some(a), Some(b)) = (build(&mut rng), build(&mut rng)) else {
            tally.refused += 1;
            continue;
        };
        for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
            tally.record_named(boolean(&a, &b, op), "boolean(a, b)", &at);
            tally.record_named(boolean(&b, &a, op), "boolean(b, a)", &at);
            tally.record_named(boolean(&a, &good, op), "boolean(a, a valid box)", &at);
            tally.record_named(boolean(&good, &a, op), "boolean(a valid box, a)", &at);
        }
        // Against the empty solid, which is a legal operand.
        for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
            tally.record_named(boolean(&a, &Solid::default(), op), "boolean(a, empty)", &at);
            tally.record_named(boolean(&Solid::default(), &a, op), "boolean(empty, a)", &at);
        }
        tally.record_named(boolean(&Solid::default(), &Solid::default(), Bool3::Union), "boolean(empty, empty)", &at);
        // Bodies whose bounds touch exactly, which is where the early disjoint path is not taken.
        let touching = match support::box_at(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)) {
            Ok(s) => s,
            Err(_) => {
                tally.refused += 1;
                continue;
            }
        };
        for op in [Bool3::Union, Bool3::Intersect, Bool3::Difference] {
            tally.record_named(boolean(&good, &touching, op), "boolean(touching boxes)", &at);
        }
    }
    tally.record_named(Solid::from_faces(Vec::new()), "Solid::from_faces(vec![])", "empty face list");
    tally.report("booleans");
}

// ====================================================================================
// 5. blends
// ====================================================================================

/// `chamfer` and `fillet` with hostile reference points, tolerances and sizes.
///
/// `resolve` is the first thing both call, so every one of these either resolves or is refused at
/// the reference stage — which means this file also covers `resolve`'s own input checks, the ones
/// property.rs and this file's other tests do not: a reference whose tolerance is `INFINITY`, a
/// reference whose point is `INFINITY`, and a shell with no edge list at all.
#[test]
fn blends_survive_hostile_references_tolerances_and_sizes() {
    const CASES: usize = 300;
    let seed = 0xF1A0_0008_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    let host = must_box();
    // A shell with a hole in it, so `edges()` is `None` and `resolve` cannot even start.
    let open = {
        let mut faces = host.faces.clone();
        faces.pop();
        match Solid::from_faces(faces) {
            Ok(s) => s,
            Err(e) => panic!("the open fixture should build: {e}"),
        }
    };
    let points: Vec<Vec3> = vec![
        Vec3::new(10.0, 0.0, 5.0), // a real edge
        Vec3::new(5.0, 0.0, 10.0), // another real edge
        Vec3::new(5.0, 5.0, 5.0),  // the middle: no edge
        Vec3::new(0.0, 0.0, 0.0),   // a corner: three edges
        Vec3::new(50.0, 50.0, 50.0),
        Vec3::new(f64::NAN, 0.0, 0.0),
        Vec3::new(f64::INFINITY, 0.0, 0.0),
        Vec3::new(-f64::MAX, f64::MAX, 0.0),
        Vec3::new(1.0e-300, 1.0e-300, 1.0e-300),
        Vec3::new(10.0 + 1e-9, 1e-9, 5.0),
    ];
    let tolerances = [1e-6, 0.0, -1.0, f64::NAN, f64::INFINITY, 1e300];
    let sizes = [1.0, 0.0, -1.0, 1e-12, 1e-9, 1e3, 1e300, f64::NAN, f64::INFINITY];

    for case in 0..CASES {
        let at = tag(seed, case);
        let p = points.get(rng.pick(points.len())).copied().unwrap_or(Vec3::ZERO);
        let t = tolerances.get(rng.pick(tolerances.len())).copied().unwrap_or(1e-6);
        let size = sizes.get(rng.pick(sizes.len())).copied().unwrap_or(1.0);
        let refs = [EdgeRef::new(p, t)];
        tally.record_named(chamfer(&host, &refs, size), "chamfer", &at);
        tally.record_named(fillet(&host, &refs, size, rng.pick(limits::MAX_ROUND_SEGMENTS + 3)), "fillet", &at);
        // An infinite tolerance against a point in the middle of nothing: `resolve` accepts the
        // reference, so what comes back is a blend that touched something, or a refusal.
        tally.record_named(chamfer(&host, &[EdgeRef::new(Vec3::new(5.0, 5.0, 5.0), f64::INFINITY)], 1.0), "chamfer with an infinite tolerance", &at);
        tally.record_named(fillet(&host, &[EdgeRef::new(Vec3::new(5.0, 5.0, 5.0), f64::INFINITY)], 1.0, 8), "fillet with an infinite tolerance", &at);
        // An open shell: no edge list, so the refusal must happen before anything is cut.
        tally.record_named(chamfer(&open, &refs, 1.0), "chamfer on an open shell", &at);
        tally.record_named(fillet(&open, &refs, 1.0, 8), "fillet on an open shell", &at);
        // No references at all: documented as a no-op, and it returns the body it was given.
        let same = chamfer(&host, &[], size);
        match same {
            Ok(s) => assert_eq!(s.faces.len(), host.faces.len(), "{at}: chamfer with no references changed the body"),
            Err(_) => tally.refused += 1,
        }
    }
    tally.report("blends");
}

// ====================================================================================
// 6. writers
// ====================================================================================

/// The STL and STEP writers survive the solids they are handed.
///
/// `stl_ascii` and `stl_binary` cannot fail, so the property is purely that they terminate and
/// produce the shape the STL specification demands. `step_faceted_brep` can fail, and its failures
/// have to name themselves: an empty solid, a non-finite coordinate, a zero-area face or an edge
/// shorter than the tolerance are all documented refusals.
///
/// The binary STL's layout is asserted exactly (80-byte header, `u32` count, 50 bytes per facet),
/// because a writer that got that wrong produces a file no reader accepts and no other test would
/// notice.
#[test]
fn the_writers_survive_hostile_solids_and_names() {
    const CASES: usize = 120;
    let seed = 0xF1A0_0009_u64;
    let mut rng = Rng::new(seed);
    let mut tally = Tally::default();
    let good = must_box();

    // Names are sanitised before they reach a string literal, and part 21 has no escape for a
    // quote — so every one of these must either be refused or produce a file whose literals are
    // intact. The check is that the file still ends with `END-ISO-10303-21;`.
    let names = [
        "",
        "part",
        "a name with 'single quotes'",
        "a name with \"double quotes\"",
        "a name with a \\ backslash",
        "a name with a ; semicolon",
        "a name with a newline \n in it",
        "\u{1F600} an emoji",
        "\u{0} a null",
    ];
    for name in names {
        match step_faceted_brep(&good, name) {
            Ok(text) => {
                tally.built += 1;
                assert!(text.ends_with("END-ISO-10303-21;\n"), "a STEP file for {name:?} does not close with its terminator");
                assert!(text.contains(step_schema()), "a STEP file for {name:?} does not carry the schema");
                assert!(text.starts_with("ISO-10303-21;\n"), "a STEP file for {name:?} does not open with its marker");
                // Part 21 has no escape for a quote, so the name is sanitised rather than escaped.
                // The observable consequence is that every literal in the file is still a literal:
                // the number of `'` characters is even.
                let quotes = text.matches('\'').count();
                assert!(quotes % 2 == 0, "a STEP file for {name:?} has an odd number of quotes ({quotes}): a literal was left open");
            }
            Err(e) => {
                tally.refused += 1;
                assert!(!e.to_string().trim().is_empty(), "step_faceted_brep refused {name:?} with an empty message");
            }
        }
    }

    for case in 0..CASES {
        let at = tag(seed, case);
        tally.record_named(step_faceted_brep(&good, &format!("solid {case}")), "step_faceted_brep", &at);
        // An empty solid and a shell with no volume are documented refusals.
        tally.record_named(step_faceted_brep(&Solid::default(), "empty"), "step_faceted_brep(empty)", &at);
        let inside_out = match good.flipped() {
            Ok(s) => s,
            Err(_) => {
                tally.refused += 1;
                continue;
            }
        };
        tally.record_named(step_faceted_brep(&inside_out, "inside out"), "step_faceted_brep(inside out)", &at);

        // A solid built from one hostile face, which is the case where a coordinate of 1e300 or a
        // subnormal reaches the writer.
        let pts: Vec<Vec2> = (0..4).map(|_| match rng.next() % 3 { 0 => Vec2::new(rng.hostile(), rng.hostile()), 1 => Vec2::new(1.0e300, 0.0), _ => Vec2::new(rng.f64(1.0), rng.f64(1.0)) }).collect();
        if let Ok(f) = Face::new(Plane::xy(0.0), Region::new(pts).to_regions())
            && let Ok(s) = Solid::from_faces(vec![f])
        {
            tally.record_named(step_faceted_brep(&s, "hostile face"), "step_faceted_brep(hostile face)", &at);
        }
    }

    // The binary STL layout, asserted rather than merely measured.
    let mesh = match mesh_from_solid(&good) {
        Ok(m) => m,
        Err(e) => panic!("the fixture box should tessellate: {e}"),
    };
    let bytes = stl_binary(&mesh);
    assert_eq!(bytes.len(), 84 + mesh.tris.len() * 50, "every facet is 50 bytes and the header is 84");
    assert_eq!(u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize, mesh.tris.len(), "the count field matches");
    assert_eq!(&bytes[132..134], &[0, 0], "the attribute word is zeroed, as the format's byte count wants");
    let text = stl_ascii(&mesh);
    assert!(text.starts_with("solid cadcraft") && text.trim_end().ends_with("endsolid cadcraft"), "the ASCII STL has its header and footer");
    assert_eq!(text.matches("facet normal").count(), mesh.tris.len(), "one facet line per triangle");
    assert_eq!(text.matches("vertex").count(), 3 * mesh.tris.len(), "three vertex lines per facet");
    println!("writers: {} STEP files written, {} refused; the binary STL layout is exact", tally.built, tally.refused);
}

// ====================================================================================
// 7. the whole surface, round trip
// ====================================================================================

/// Every generated solid is a value that survives being flipped, reoriented and re-validated.
///
/// The last property in the file, and the one that ties the others together: take whatever the
/// kernel produced, copy it, and run the full orientation machinery over it twice. A solid that
/// changes shape when it is copied, flipped back, oriented and validated has a hidden dependency on
/// something it should not depend on — a shared buffer, a cached normal, an ordering assumption.
#[test]
fn every_generated_solid_is_a_value_that_survives_a_round_trip() {
    const CASES: usize = 150;
    let seed = 0xF1A0_000A_u64;
    let mut rng = Rng::new(seed);
    let (mut round_tripped, mut refused) = (0usize, 0usize);
    for case in 0..CASES {
        let at = tag(seed, case);
        let n = 3 + rng.pick(5);
        let r = rng.range(0.5, 3.0);
        let origin = rng.vec3(10.0);
        let pts: Vec<Vec2> = (0..n).map(|k| Vec2::from_angle(std::f64::consts::TAU * k as f64 / n as f64 + rng.f64(0.01)) * r).collect();
        let moved: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p.x + origin.x, p.y + origin.y)).collect();
        let profile = Region::new(moved).to_regions();
        let solid = match extrude(&profile, &Plane::xy(origin.z), rng.range(0.5, 3.0)) {
            Ok(s) => s,
            Err(e) => {
                refused += 1;
                let _ = e;
                continue;
            }
        };

        let volume = solid.volume();
        let area = solid.area();
        let face_count = solid.faces.len();
        let vertices: Vec<Vec3> = solid.faces.iter().flat_map(|f| f.vertices()).collect();
        let edges: Vec<Edge> = solid.edges().unwrap_or_default();

        // A copy is identical.
        let copy = solid.clone();
        assert!(copy == solid, "{at}: a clone is not equal to its original");
        assert_eq!(copy.faces, solid.faces, "{at}: cloning the faces changed them");

        // Flip and flip back: the same solid, to the last bit.
        match solid.flipped() {
            Ok(inside_out) => {
                assert!(near(inside_out.volume(), -volume, 1e-12), "{at}: flipping changed the sign of the volume by more than rounding");
                match inside_out.flipped() {
                    Ok(back) => {
                        assert!(near(back.volume(), volume, 1e-12), "{at}: flipping twice did not come back: {} vs {volume}", back.volume());
                        assert_eq!(back.faces.len(), face_count, "{at}: flipping twice changed the face count");
                        assert_eq!(back.faces, solid.faces, "{at}: flipping twice did not reproduce the faces exactly");
                        round_tripped += 1;
                    }
                    Err(e) => {
                        refused += 1;
                        assert!(!e.to_string().trim().is_empty(), "{at}: refused with an empty message: {e}");
                    }
                }
            }
            Err(e) => {
                refused += 1;
                assert!(!e.to_string().trim().is_empty(), "{at}: refused with an empty message: {e}");
            }
        }

        // Orient twice: the second pass has nothing left to do, so nothing may change.
        let mut oriented = solid.clone();
        match oriented.orient_faces() {
            Ok(()) => {
                oriented.validate().unwrap_or_else(|e| panic!("{at}: orient_faces left a shell that does not validate: {e}"));
                assert!(near(oriented.volume(), volume, 1e-12), "{at}: orient_faces on an already-correct solid moved it");
                assert!(near(oriented.area(), area, 1e-12), "{at}: orient_faces changed the area");
                assert_eq!(oriented.faces.len(), face_count, "{at}: orient_faces changed the face count");
                let _ = oriented.orient_faces();
                oriented.validate().unwrap_or_else(|e| panic!("{at}: orient_faces twice left a shell that does not validate: {e}"));
                assert!(near(oriented.volume(), volume, 1e-12), "{at}: orient_faces twice moved the volume");
            }
            Err(e) => {
                refused += 1;
                assert!(!e.to_string().trim().is_empty(), "{at}: refused with an empty message: {e}");
            }
        }

        // The vertex multiset and the edge list are the same as before anything was derived.
        let again: Vec<Vec3> = solid.faces.iter().flat_map(|f| f.vertices()).collect();
        assert_eq!(again, vertices, "{at}: reading the vertices twice gave different answers");
        let edges_again = solid.edges().unwrap_or_default();
        assert_eq!(edges_again.len(), edges.len(), "{at}: reading the edge list twice gave different answers");
        for (x, y) in edges.iter().zip(edges_again.iter()) {
            assert!(x.a.near(y.a, 0.0) && x.b.near(y.b, 0.0) && x.faces == y.faces, "{at}: the edge list is not deterministic: {x:?} vs {y:?}");
        }

        // `fix_orientation` on a correct shell is a no-op; on a flipped one it is the repair.
        let mut fixed = solid.clone();
        let _ = fixed.fix_orientation();
        assert!(near(fixed.volume(), volume, 1e-12), "{at}: fix_orientation moved a correctly-oriented solid");
        if let Ok(inside_out) = solid.flipped() {
            let mut flipped_solid = inside_out;
            let _ = flipped_solid.fix_orientation();
            assert!(near(flipped_solid.volume(), volume, 1e-12), "{at}: fix_orientation did not repair an inside-out shell: {} vs {volume}", flipped_solid.volume());
        }

        // And the exporters see the same thing.
        if let Ok(mesh) = mesh_from_solid(&solid) {
            let mesh_volume: f64 = mesh.triangles().map(|t| t.a.cross(t.b).dot(t.c) / 6.0).sum();
            let faces_volume = solid.volume_from_faces(solid.bounds().center()).unwrap_or(f64::NAN);
            assert!(!mesh_volume.is_nan() && !faces_volume.is_nan(), "{at}: a volume came back NaN");
        }
        let _ = mass_properties(&solid);
    }
    println!("round trip: {round_tripped} solids flipped-and-restored, {refused} refused");
    assert!(round_tripped > CASES / 2, "too few solids survived the round trip ({round_tripped} of {CASES})");
}

// `near` is shared by every file in this directory.
fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}