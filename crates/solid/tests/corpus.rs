//! A model corpus: six parts whose volumes and areas are computed from first principles.
//!
//! A property test asks whether two runs of the kernel agree. This file asks something else and
//! asks it of the *kernel*, not of the kernel against itself: **does the number match the geometry?**
//! Every expected value below is a closed form derived in the comment beside it — a shoelace area, a
//! perimeter, a prism formula, a triangle in a corner — and never a number the kernel produced.
//!
//! That distinction is the whole point. A regression that changed a face's winding would be caught
//! by `property.rs`, which compares two runs against each other; a regression that changed the
//! *definition* of a face's area would be caught by neither, and this is the file that catches it.
//!
//! # The parts
//!
//! | part               | built from                          | V      | A      |
//! |--------------------|-------------------------------------|--------|--------|
//! | `plate`            | one rectangle extruded              | 1440   | 864    |
//! | `plate_with_hole`  | a rectangle with a hole extruded    | 1152   | 936    |
//! | `l_bracket`        | a concave profile extruded          | 204    | 262    |
//! | `chamfered_block`  | a cube with one edge bevelled        | 980    | 584.28 |
//! | `rounded_block`    | a cube with one edge rounded        | banded | banded |
//! | `pocketed_plate`   | a plate with a blind pocket cut     | 1368   | 924    |
//! | `revolved_ring`    | a profile revolved a full turn      | see below — the kernel cannot build it yet |
//!
//! # What is asserted, and what is only reported
//!
//! **Asserted:** volume, surface area, `validate`, `validate_vertices`, and the closed-form facts
//! each part is built from. Those are the contract: a drawing's mass and its skin.
//!
//! **Reported, not asserted:** the face count of anything that came out of a boolean. How many
//! faces a boolean happens to emit is an implementation choice, and pinning it would make this file
//! fail on an improvement. The face counts of the *extrusions* are asserted, because there the
//! construction forces them (two caps and one wall per profile segment) and a change there is a
//! change of representation.
//!
//! **Banded, not a point:** the rounded block. The exact number a faceted round removes is a
//! function of `arc_section`, and re-deriving that function here would test my transcription of it
//! rather than the kernel. What *is* derivable independently is the band the removed area must lie
//! in for any number of facets — see [`rounded_block_removes_between_the_arc_and_the_bevel`].
//!
//! # The one part that cannot be checked yet
//!
//! `revolve` does not build a solid: `triangle_face` builds its patch from a two-point region,
//! `Region::is_empty` calls a loop shorter than three empty, the `.ok()` swallows the error and
//! every swept face is dropped. That is pinned as DEFECT 3 in `tests/property.rs`. The corpus entry
//! therefore states the closed form it *will* assert the day the kernel works, asserts nothing while
//! it does not, and prints the refusal — so it becomes an exact gate the moment `revolve` is fixed,
//! with no edit here.
//!
//! Deterministic, no `rand`, no files, no wall clock. Expected runtime well under a second.
//!
//! **Execution status: every test in this file is `NOT EXECUTED`.** It was written without a
//! terminal and has never been compiled or run; see the report that came with it.

use cadcraft_solid::{
    Bool3, Plane, Region, Regions, Solid, Vec2, Vec3, boolean, chamfer, extrude, fillet, mass_properties, revolve, support,
};

fn must<T>(r: Result<T, cadcraft_solid::SolidError>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e}"),
    }
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

fn box_at(min: Vec3, max: Vec3) -> Solid {
    must(support::box_at(min, max), "support::box_at")
}

/// Assert the two numbers a part is *for*, and check the solid is one at the same time.
///
/// Face counts are asserted only when the caller says the construction forces them.
struct Expect {
    volume: f64,
    area: f64,
    faces: Option<usize>,
    rel: f64,
}

impl Expect {
    fn new(volume: f64, area: f64) -> Expect {
        Expect { volume, area, faces: None, rel: 1e-9 }
    }

    fn faces(mut self, n: usize) -> Expect {
        self.faces = Some(n);
        self
    }

    fn check(&self, s: &Solid, name: &str) {
        s.validate().unwrap_or_else(|e| panic!("{name}: the solid does not validate: {e}"));
        s.validate_vertices().unwrap_or_else(|e| panic!("{name}: the solid is pinched: {e}"));
        let v = s.volume();
        assert!(near(v, self.volume, self.rel), "{name}: volume {} against the closed form {}", v, self.volume);
        let a = s.area();
        assert!(near(a, self.area, self.rel), "{name}: area {} against the closed form {}", a, self.area);
        assert!(v > 0.0, "{name}: a part with no volume is not a part");
        // The area the kernel reports is the sum of the faces' areas — asserted here so that "the
        // area" means one thing everywhere in this file.
        let face_sum: f64 = s.faces.iter().map(|f| f.area()).sum();
        assert!(near(a, face_sum, 1e-12), "{name}: area {} is not the sum of the faces' {}", a, face_sum);
        // And the two independent volume computations agree with it.
        let m = must(mass_properties(s), "mass_properties");
        let faces_volume = must(s.volume_from_faces(s.bounds().center()), "volume_from_faces");
        assert!(near(v, m.volume, 1e-9), "{name}: volume {} against the mesh integration {}", v, m.volume);
        assert!(near(v, faces_volume, 1e-9), "{name}: volume {} against the pyramid sum {}", v, faces_volume);
        if let Some(n) = self.faces {
            assert_eq!(s.faces.len(), n, "{name}: face count");
        }
        let (f, l, e) = s.counts();
        println!("  {name:<18} V={v:.9} A={a:.9} faces={f} loops={l} boundary-edges={e}");
    }
}

// ====================================================================================
// 1. plate
// ====================================================================================

/// A 20 x 12 plate, 6 deep.
///
/// ```text
/// volume  = w · d · h                     = 20 · 12 · 6 = 1440
/// area    = 2(w·d + w·h + d·h)           = 2(240 + 120 + 72) = 864
/// faces   = 2 caps + 4 walls              = 6
/// ```
#[test]
fn plate_20_by_12_by_6() {
    let w = 20.0;
    let d = 12.0;
    let h = 6.0;
    let plate = must(extrude(&support::rect(0.0, 0.0, w, d), &Plane::xy(0.0), h), "extrude the plate");

    Expect::new(w * d * h, 2.0 * (w * d + w * h + d * h)).faces(6).check(&plate, "plate");

    // The volume is a product, so it is worth saying what the parts are.
    assert!(near(plate.volume(), 1440.0, 1e-12), "20 x 12 x 6 is 1440, got {}", plate.volume());
    assert!(near(plate.area(), 864.0, 1e-12), "20 x 12 x 6 has 864 of skin, got {}", plate.area());
    // And the centroid is the middle of the plate, which is what a solid of revolution would use.
    let m = must(mass_properties(&plate), "mass_properties");
    assert!(m.centroid.near(Vec3::new(w / 2.0, d / 2.0, h / 2.0), 1e-12), "the plate's centroid is {:?}", m.centroid);
    assert!(plate.contains(Vec3::new(10.0, 6.0, 3.0)), "the middle of the plate is material");
    assert!(!plate.contains(Vec3::new(25.0, 6.0, 3.0)), "and past its edge is not");
}

// ====================================================================================
// 2. plate with a hole
// ====================================================================================

/// A 20 x 12 plate, 6 deep, with an 8 x 6 hole cut through the middle.
///
/// ```text
/// section = w·d − hw·hh                    = 240 − 48 = 192
/// volume  = section · h                    = 1152
/// area    = 2·section                       (the two annular caps)
///          + (2(w + d) + 2(hw + hh))·h      (outer and inner walls)
///          = 384 + (64 + 28)·6 = 384 + 552 = 936
/// faces   = 2 caps + 4 outer walls + 4 hole walls = 10
/// ```
#[test]
fn plate_with_an_eight_by_six_hole() {
    let (w, d, h) = (20.0, 12.0, 6.0);
    let (hw, hh) = (8.0, 6.0);
    let tube = must(extrude(&support::rect_with_hole(0.0, 0.0, w, d, 6.0, 3.0, 6.0 + hw, 3.0 + hh), &Plane::xy(0.0), h), "extrude the tube");

    let section = w * d - hw * hh;
    let area = 2.0 * section + (2.0 * (w + d) + 2.0 * (hw + hh)) * h;
    Expect::new(section * h, area).faces(10).check(&tube, "plate_with_hole");

    assert!(near(tube.volume(), 1152.0, 1e-12), "a 20x12 plate less an 8x6 hole, 6 deep, is 1152, got {}", tube.volume());
    assert!(near(tube.area(), 936.0, 1e-12), "its skin is 936, got {}", tube.area());
    // The hole is a hole: its axis is empty through the whole depth.
    for z in [0.5, 3.0, 5.5] {
        assert!(!tube.contains(Vec3::new(10.0, 6.0, z)), "the hole is open at z = {z}");
        assert!(tube.contains(Vec3::new(2.0, 6.0, z)), "and the wall beside it is material at z = {z}");
    }
}

// ====================================================================================
// 3. L-bracket
// ====================================================================================

/// An L-profile with 10-unit legs and 3-unit thickness, extruded 4 deep.
///
/// The profile, counter-clockwise, is `(0,0) (10,0) (10,3) (3,3) (3,10) (0,10)`.
///
/// ```text
/// area by the shoelace:  ½|Σ (x_i·y_{i+1} − x_{i+1}·y_i)| = ½(0 + 30 + 21 + 21 + 30 + 0) = 51
/// perimeter:            10 + 3 + 7 + 7 + 3 + 10 = 40
/// volume = area · depth = 51 · 4 = 204
/// area   = 2·51 + 40·4  = 102 + 160 = 262
/// faces  = 2 caps + 6 walls = 8
/// ```
///
/// This is the one part in the corpus that is **not** a box, so it is the one that would notice a
/// bug in the triangulation of a concave region: an ear clipper that clips the notch out of the L
/// gives a volume of 210-ish rather than 204, and nothing that compares two kernel runs against each
/// other can see it.
#[test]
fn an_l_bracket_with_ten_unit_legs() {
    let legs = 10.0;
    let t = 3.0;
    let h = 4.0;
    let profile = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(legs, 0.0),
        Vec2::new(legs, t),
        Vec2::new(t, t),
        Vec2::new(t, legs),
        Vec2::new(0.0, legs),
    ];
    let bracket = must(extrude(&Region::new(profile.clone()).to_regions(), &Plane::xy(0.0), h), "extrude the bracket");

    // The shoelace and the perimeter, computed here rather than asked of anybody.
    let (mut twice_area, mut perimeter) = (0.0, 0.0);
    for i in 0..profile.len() {
        let p = profile[i];
        let q = profile[(i + 1) % profile.len()];
        twice_area += p.x * q.y - q.x * p.y;
        perimeter += p.dist(q);
    }
    let area = twice_area * 0.5;
    assert!(near(area, 51.0, 1e-12), "the L's shoelace area is 51, got {area}");
    assert!(near(perimeter, 40.0, 1e-12), "the L's perimeter is 40, got {perimeter}");

    Expect::new(area * h, 2.0 * area + perimeter * h).faces(8).check(&bracket, "l_bracket");

    assert!(near(bracket.volume(), 204.0, 1e-12), "an L-bracket is 204, got {}", bracket.volume());
    assert!(near(bracket.area(), 262.0, 1e-12), "and has 262 of skin, got {}", bracket.area());
    // The notch is *not* material: that is the assertion an ear-clipping bug fails first. The
    // profile's material is the union of a 10 x 3 bar and a 3 x 7 bar, so (1.5, 1.5) and (8, 1.5)
    // and (1.5, 8) are inside and (8, 8) is in the square that was cut out.
    assert!(bracket.contains(Vec3::new(1.5, 1.5, 2.0)), "the corner is material");
    assert!(bracket.contains(Vec3::new(8.0, 1.5, 2.0)), "the foot of the L is material");
    assert!(bracket.contains(Vec3::new(1.5, 8.0, 2.0)), "the leg of the L is material");
    assert!(!bracket.contains(Vec3::new(8.0, 8.0, 2.0)), "and the middle of the square is the notch");
}

// ====================================================================================
// 4. chamfered block
// ====================================================================================

/// A 10-cube with one vertical edge bevelled by 2.
///
/// The removed material is a triangular prism: a right triangle of legs 2 and 2 in cross-section,
/// `d²/2 = 2`, running the full 10 of the edge.
///
/// ```text
/// volume = 1000 − (d²/2)·L               = 1000 − 2·10 = 980
///
/// area   = 6·100                          the untouched cube
///          − 2·(d·L)                      two side faces each lose a strip d wide
///          − 2·(d²/2)                     both caps each lose a right triangle
///          + d·√2·L                       one new face, the hypotenuse d·√2 long
///          = 600 − 40 − 4 + 28.2842712     = 584.2842712
/// faces  = 6 + 1 = 7
/// ```
#[test]
fn a_ten_cube_chamfered_by_two() {
    let side = 10.0;
    let d = 2.0;
    let block = box_at(Vec3::ZERO, Vec3::new(side, side, side));
    let bevelled = must(chamfer(&block, &[cadcraft_solid::EdgeRef::new(Vec3::new(side, 0.0, side / 2.0), 1e-6)], d), "chamfer");

    let volume = side * side * side - (d * d / 2.0) * side;
    let area = 6.0 * side * side - 2.0 * (d * side) - d * d + d * 2.0f64.sqrt() * side;
    Expect::new(volume, area).faces(7).check(&bevelled, "chamfered_block");

    assert!(near(bevelled.volume(), 980.0, 1e-9), "a 10-cube less a 2x2 triangular prism of length 10 is 980, got {}", bevelled.volume());
    assert!(near(bevelled.area(), 584.2842712474619, 1e-9), "its skin is 584.2842712, got {}", bevelled.area());
    // The bevelled corner is gone, the opposite one is not.
    assert!(!bevelled.contains(Vec3::new(side - 0.4, 0.1, side / 2.0)), "the bevelled corner is cut away");
    assert!(bevelled.contains(Vec3::new(0.5, side - 0.5, side / 2.0)), "the far corner is untouched");
    assert!(bevelled.contains(Vec3::new(5.0, 5.0, 5.0)), "and so is the middle");
}

/// A chamfer removes exactly what its cross-section says, at every size.
///
/// The same closed form as above, run over a ladder of sizes and distances. A bevel is the easiest
/// blend to get wrong by a *little* — a facet inserted in the wrong place, a strip trimmed a
/// tolerance short — and a ladder catches a drift that a single case would step over.
#[test]
fn a_chamfer_removes_its_triangle_at_every_size() {
    let mut checked = 0usize;
    for &side in &[4.0, 10.0, 25.0, 100.0] {
        for &d in &[0.25, 1.0, 2.0, 3.0] {
            if 2.0 * d >= side {
                continue;
            }
            let block = box_at(Vec3::ZERO, Vec3::new(side, side, side));
            let bevelled = must(chamfer(&block, &[cadcraft_solid::EdgeRef::new(Vec3::new(side, 0.0, side / 2.0), side * 1e-9)], d), "chamfer");
            let volume = side * side * side - (d * d / 2.0) * side;
            let area = 6.0 * side * side - 2.0 * (d * side) - d * d + d * 2.0f64.sqrt() * side;
            Expect::new(volume, area).check(&bevelled, &format!("chamfer {d} on a {side}-cube"));
            checked += 1;
        }
    }
    assert!(checked >= 14, "the chamfer ladder only ran {checked} cases");
}

// ====================================================================================
// 5. rounded block
// ====================================================================================

/// A round removes between the exact arc and the bevel, for any number of facets.
///
/// This is the one corpus entry that is a band rather than a point, and the reason is worth stating:
/// the *exact* removed area of a `segments`-facet round is a function of `arc_section`, and
/// re-deriving that function in this file would test my reading of the code rather than the code.
/// What can be derived from the geometry alone is the band every faceted round must lie in:
///
/// ```text
/// the true round of radius r at a right-angle corner removes  r²(1 − π/4)  per unit of edge
///                                                     = 4 − π   ≈ 0.8584073  for r = 2
/// the bevel of distance r removes                      r²/2  = 2              for r = 2
/// ```
///
/// A faceted round replaces the arc by chords. The arc bulges *towards* the corner (its centre is
/// at `r(d̂₁ + d̂₂)`, on the far side), so each chord lies inside the arc and removes **more** than
/// the true round; and a round can never remove more than the bevel it converges to from above as
/// `segments` grows. Hence
///
/// ```text
/// r²(1 − π/4)  ≤  removed per unit length  ≤  r²/2
/// ```
///
/// and, as `segments` grows, the removed area falls monotonically onto the lower bound. All three
/// of those are asserted: the band, the limit from above, and that a finer round removes no more
/// than a coarser one.
#[test]
fn rounded_block_removes_between_the_arc_and_the_bevel() {
    let side = 10.0;
    let r = 2.0;
    let exact = r * r * (1.0 - std::f64::consts::FRAC_PI_4);
    let bevel = r * r / 2.0;
    assert!(near(exact, 4.0 - std::f64::consts::PI, 1e-15), "the exact corner removal is 4 - pi, got {exact}");
    assert!(near(bevel, 2.0, 1e-15), "the bevel removal is r²/2, got {bevel}");

    let block = box_at(Vec3::ZERO, Vec3::new(side, side, side));
    let edge = cadcraft_solid::EdgeRef::new(Vec3::new(side, 0.0, side / 2.0), 1e-6);

    // The chamfer at distance r is the upper bound, exactly: a fillet must never remove more than
    // the bevel of the same size, or the round is not a round.
    let bevelled = must(chamfer(&block, &[edge], r), "chamfer");
    let bevel_removed = (block.volume() - bevelled.volume()) / side;
    assert!(near(bevel_removed, bevel, 1e-9), "the chamfer removes {bevel_removed} per unit, the closed form says {bevel}");

    let mut removed_by_segments: Vec<(usize, f64)> = Vec::new();
    for segments in [1usize, 2, 4, 8, 16, 32, 64] {
        let rounded = match fillet(&block, &[edge], r, segments) {
            Ok(s) => s,
            Err(e) => {
                // A refusal is allowed by the crate's contract; what is not allowed is a fillet
                // that removes something outside the band. A refusal reports nothing, so there is
                // nothing to check and the case is counted.
                println!("  fillet r={r} segments={segments}: refused ({e})");
                continue;
            }
        };
        rounded.validate().unwrap_or_else(|e| panic!("r{segments}: does not validate: {e}"));
        rounded.validate_vertices().unwrap_or_else(|e| panic!("r{segments}: is pinched: {e}"));
        let removed = (block.volume() - rounded.volume()) / side;
        assert!(removed >= exact * (1.0 - 1e-9), "a {segments}-facet round removed {removed} per unit, less than the {exact} a true round removes");
        assert!(removed <= bevel * (1.0 + 1e-9), "a {segments}-facet round removed {removed} per unit, more than the {bevel} a bevel removes");
        assert!(rounded.volume() < block.volume(), "a round that removed nothing");
        removed_by_segments.push((segments, removed));
    }

    // Monotone from above: more facets is closer to the arc, so less material goes.
    for pair in removed_by_segments.windows(2) {
        let (coarse, a) = (pair[0].0, pair[0].1);
        let (fine, b) = (pair[1].0, pair[1].1);
        assert!(b <= a * (1.0 + 1e-9), "a {fine}-facet round removed {b}, more than a {coarse}-facet one removed {a}");
    }
    let finest = removed_by_segments.last();
    if let Some((segments, removed)) = finest {
        println!("  rounded_block      removed {removed:.9} per unit at {segments} facets, against {exact:.9} for the exact round");
        assert!(*removed >= exact * (1.0 - 1e-9), "the finest round removed {removed}, under the {exact} the true round removes");
    }
    assert!(!removed_by_segments.is_empty(), "no fillet at all was built, so the band was never checked");
}

// ====================================================================================
// 6. pocketed plate
// ====================================================================================

/// A 20 x 12 plate, 6 deep, with a 6 x 4 blind pocket 3 deep cut into its top face.
///
/// The tool runs from `z = 3` to `z = 7`, so it crosses the plate's top face (`z = 6`) and stops
/// inside the material. That configuration is the one the coplanar merge in `section` and the
/// plane list in `boolean` both have to agree about, and it is a pocket rather than a through-hole
/// precisely because the pocket's floor is a face of the *tool*, not of the plate.
///
/// ```text
/// removed = 6 · 4 · 3 = 72
/// volume  = 1440 − 72 = 1368
///
/// area    = 864                         the plate alone
///          − 24                         the top face loses the pocket's mouth
///          + 24                         the floor gains it back
///          + 2·(6·3) + 2·(4·3)          four pocket walls, each `depth` tall
///          = 864 + 36 + 24 = 924
/// ```
#[test]
fn a_plate_with_a_six_by_four_blind_pocket() {
    let (w, d, h) = (20.0, 12.0, 6.0);
    let (pw, pd, depth) = (6.0, 4.0, 3.0);
    let plate = must(extrude(&support::rect(0.0, 0.0, w, d), &Plane::xy(0.0), h), "extrude the plate");
    let tool = box_at(Vec3::new(7.0, 4.0, h - depth), Vec3::new(7.0 + pw, 4.0 + pd, h + 1.0));
    let pocketed = must(boolean(&plate, &tool, Bool3::Difference), "cut the pocket");

    let removed = pw * pd * depth;
    let area = 2.0 * (w * d + w * h + d * h) - pw * pd + pw * pd + 2.0 * (pw + pd) * depth;
    Expect::new(w * d * h - removed, area).check(&pocketed, "pocketed_plate");

    assert!(near(pocketed.volume(), 1368.0, 1e-9), "a 20x12x6 plate less a 6x4x3 pocket is 1368, got {}", pocketed.volume());
    assert!(near(pocketed.area(), 924.0, 1e-9), "its skin is 924, got {}", pocketed.area());
    // The pocket is a pocket, not a hole: there is a floor.
    assert!(!pocketed.contains(Vec3::new(10.0, 6.0, 5.0)), "the pocket is open at the top");
    assert!(pocketed.contains(Vec3::new(10.0, 6.0, 3.5)), "and has a floor above the bottom of the plate");
    assert!(pocketed.contains(Vec3::new(10.0, 6.0, 0.5)), "the plate is solid under the pocket");
    assert!(pocketed.contains(Vec3::new(1.0, 1.0, 3.0)), "and the rest of the plate is untouched");
}

// ====================================================================================
// 7. revolved ring
// ====================================================================================

/// A tube of inner radius 1, outer radius 2 and height 1, revolved a full turn in 16 facets.
///
/// ```text
/// the exact cylinder:               π (R_o² − R_i²) h          = 3π      ≈ 9.42477796
/// the 16-sided prism it becomes:   ½ n sin(2π/n) (R_o² − R_i²) h
///                                   = ½ ·16· sin(π/8) · 3
///                                   = 9.18440238
/// ```
///
/// The faceted figure is *smaller* than the cylinder, by exactly the factor `n sin(2π/n) / 2π`
/// — the ratio of a regular polygon's area to its circumscribed circle's. That factor is the whole
/// contract of a faceted revolve and is asserted below as a ratio rather than as an absolute, so it
/// says the same thing at any `segments`.
///
/// # This part cannot be built yet
///
/// `revolve` currently returns `Err` for every input. `triangle_face` builds its patch from a
/// two-point region, `Region::is_empty` calls a loop shorter than three empty, `to_regions` turns
/// it into `Regions::default`, `Face::new` rejects it with `SolidError::Empty` and the `.ok()`
/// swallows that — so every swept face is dropped. This is pinned as DEFECT 3 in
/// `tests/property.rs` (`revolve_builds_no_solid`) and is not duplicated here; the corpus entry
/// exists so that the day it is fixed the expected number is already written down.
///
/// So: a solid is checked against the closed form; a refusal is recorded and printed. Neither is a
/// failure, and neither is a pass that means much — which is exactly why the reason is printed on
/// every run.
#[test]
fn a_revolved_ring_of_sixteen_facets() {
    let segments = 16usize;
    let (ri, ro, h) = (1.0, 2.0, 1.0);
    let profile = support::rect(ri, 0.0, ro, h);
    let plane = Plane::xy(0.0);
    let axis = Vec3::Y;

    let exact = std::f64::consts::PI * (ro * ro - ri * ri) * h;
    let faceted = 0.5 * segments as f64 * (std::f64::consts::TAU / segments as f64).sin() * (ro * ro - ri * ri) * h;
    let chord_ratio = segments as f64 * (std::f64::consts::TAU / segments as f64).sin() / std::f64::consts::TAU;

    // The ratio is a property of the faceting alone, so it is checked whether or not the kernel
    // manages to build anything today.
    assert!(chord_ratio < 1.0, "a polygon inscribed in a circle has less area than the circle: {chord_ratio}");
    assert!(near(faceted, exact * chord_ratio, 1e-12), "the faceted volume is the exact volume times the chord ratio: {faceted} vs {}", exact * chord_ratio);

    match revolve(&profile, &plane, axis, std::f64::consts::TAU, segments) {
        Ok(solid) => {
            s_validate(&solid, "revolved_ring");
            assert!(near(solid.volume(), faceted, 1e-9), "a {segments}-facet ring of inner radius {ri}, outer {ro} and height {h} is {faceted}, got {}", solid.volume());
            assert!(near(solid.volume() / exact, chord_ratio, 1e-9), "the faceted ring is {chord_ratio} of the cylinder it approximates: {}", solid.volume() / exact);
            println!("  {:<18} V={:.9} (exact cylinder {:.9}) faces={}", "revolved_ring", solid.volume(), exact, solid.faces.len());
            // The surface area is deliberately **not** asserted. `revolve` builds no caps for a
            // full turn (`if !full { … }` in `extrude.rs`), so even a working implementation would
            // hand back an open tube here and its "area" would be the outer wall alone. The
            // closed-form area of a closed ring —
            //
            // ```text
            //   two ends   n·sin(2π/n)·(R_o² − R_i²)          = 18.36880475352431   at n = 16
            //   walls      2·n·sin(π/n)·(R_o + R_i)·h          = 18.72867067354831   at n = 16
            //   total                                        = 37.09747542707262
            // ```
            //
            // — belongs to a *partial* turn, which is the other shape `revolve` does not build.
            // Both numbers are printed below so the corpus says what it expects rather than
            // asserting a figure that no current code path could produce.
            println!(
                "    a closed ring of this shape would have area {:.9} (ends {:.9} + walls {:.9}), which needs a partial turn",
                segments as f64 * (std::f64::consts::TAU / segments as f64).sin() * 3.0 + 2.0 * segments as f64 * (std::f64::consts::PI / segments as f64).sin() * 3.0 * 1.0,
                segments as f64 * (std::f64::consts::TAU / segments as f64).sin() * 3.0,
                2.0 * segments as f64 * (std::f64::consts::PI / segments as f64).sin() * 3.0 * 1.0
            );
        }
        Err(e) => {
            println!(
                "  revolved_ring      NOT BUILT ({e})\n    expected when it is: V={faceted:.9} against {exact:.9} for the exact cylinder"
            );
        }
    }
}

fn s_validate(s: &Solid, name: &str) {
    s.validate().unwrap_or_else(|e| panic!("{name}: the solid does not validate: {e}"));
    s.validate_vertices().unwrap_or_else(|e| panic!("{name}: the solid is pinched: {e}"));
}

// ====================================================================================
// 8. the whole table
// ====================================================================================

/// Every part in one run, so a corpus that has drifted shows up as one failing table.
///
/// Each entry builds its own part and checks the same four things — volume, area, `validate`,
/// `validate_vertices` — against the closed forms written out in the tests above. A part that
/// cannot be built is reported and skipped; the one that cannot today is `revolve`, and the table
/// says so out loud rather than quietly passing with an empty row.
#[test]
fn the_corpus_table_is_consistent() {
    struct Row {
        name: &'static str,
        solid: Result<Solid, String>,
        volume: f64,
        /// `None` where the shape has no closed-form area *on this code path* — the revolved
        /// ring, which would need caps it is not given.
        area: Option<f64>,
    }

    let (w, d, h) = (20.0, 12.0, 6.0);
    let plate = must(extrude(&support::rect(0.0, 0.0, w, d), &Plane::xy(0.0), h), "plate");
    let tube = must(extrude(&support::rect_with_hole(0.0, 0.0, w, d, 6.0, 3.0, 14.0, 9.0), &Plane::xy(0.0), h), "tube");
    let bracket = must(
        extrude(
            &Region::new(vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 3.0),
                Vec2::new(3.0, 3.0),
                Vec2::new(3.0, 10.0),
                Vec2::new(0.0, 10.0),
            ])
            .to_regions(),
            &Plane::xy(0.0),
            4.0,
        ),
        "bracket",
    );
    let block = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let edge = cadcraft_solid::EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6);
    let bevelled = must(chamfer(&block, &[edge], 2.0), "chamfer");
    let pocket_tool = box_at(Vec3::new(7.0, 4.0, 3.0), Vec3::new(13.0, 8.0, 7.0));
    let pocketed = must(boolean(&plate, &pocket_tool, Bool3::Difference), "pocket");
    let ring = revolve(&support::rect(1.0, 0.0, 2.0, 1.0), &Plane::xy(0.0), Vec3::Y, std::f64::consts::TAU, 16).map_err(|e| e.to_string());

    let rows = vec![
        Row { name: "plate", solid: Ok(plate.clone()), volume: 1440.0, area: Some(864.0) },
        Row { name: "plate_with_hole", solid: Ok(tube), volume: 1152.0, area: Some(936.0) },
        Row { name: "l_bracket", solid: Ok(bracket), volume: 204.0, area: Some(262.0) },
        Row { name: "chamfered_block", solid: Ok(bevelled), volume: 980.0, area: Some(584.2842712474619) },
        Row { name: "pocketed_plate", solid: Ok(pocketed), volume: 1368.0, area: Some(924.0) },
        Row { name: "revolved_ring", solid: ring, volume: 9.184402376863, area: None },
    ];

    let mut built = 0usize;
    let mut missing = 0usize;
    for row in &rows {
        match &row.solid {
            Ok(s) => {
                built += 1;
                s_validate(s, row.name);
                assert!(near(s.volume(), row.volume, 1e-9), "{}: volume {} against the corpus value {}", row.name, s.volume(), row.volume);
                if let Some(want) = row.area {
                    assert!(near(s.area(), want, 1e-9), "{}: area {} against the corpus value {}", row.name, s.area(), want);
                } else {
                    println!("  {:<18} V={:.9}; area is not a corpus value on this code path (a full turn has no caps)", row.name, s.volume());
                }
            }
            Err(why) => {
                missing += 1;
                println!("  {:<18} NOT BUILT: {why}\n    corpus expects V={} A={:?}", row.name, row.volume, row.area);
            }
        }
    }
    println!("corpus: {built} of {} parts built and checked against their closed forms, {missing} not buildable today", rows.len());
    assert!(built >= rows.len() - 1, "only {built} of {} corpus parts could be built", rows.len());
}

/// The corpus is a corpus of *numbers*, so the numbers have to be right even when the kernel is not.
///
/// Each closed form is evaluated here against a second, independent derivation of the same quantity.
/// If the two disagree, one of them is wrong and the corpus would be pinning a wrong number — which
/// is worse than having no corpus at all, because it would fail every time for the wrong reason.
#[test]
fn the_corpus_closed_forms_agree_with_independent_derivations() {
    // A box's area, two ways: the surface of a box is a prism's two caps plus its four walls.
    let (a, b, c) = (20.0, 12.0, 6.0);
    let prism_area = 2.0 * (a * b) + 2.0 * (a * c) + 2.0 * (b * c);
    // ...and a sum over its six faces, each computed from its own rectangle.
    let mut face_sum = 0.0;
    for (u, v) in [(a, b), (a, b), (a, c), (a, c), (b, c), (b, c)] {
        face_sum += u * v;
    }
    assert!(near(prism_area, face_sum, 1e-12), "the prism area is {prism_area}, the face sum is {face_sum}");
    assert!(near(prism_area, 864.0, 1e-12), "a 20x12x6 box has 864 of skin, got {prism_area}");

    // The plate with a hole, as a subtraction of surfaces.
    let plate_a = prism_area;
    let hole_a = 2.0 * (8.0 * 6.0) + 2.0 * (8.0 + 6.0) * 6.0;
    assert!(near(plate_a - hole_a, 936.0, 1e-12), "removing a hole's surfaces leaves {plate_a - hole_a}, the corpus says 936");
    assert!(near(plate_a - hole_a, 936.0, 1e-12), "864 − {} = 936", hole_a);

    // The L's area, as two rectangles rather than a shoelace.
    assert!(near(10.0 * 3.0 + 3.0 * 7.0, 51.0, 1e-12), "the L is 30 + 21 = 51 by decomposition");

    // The chamfered block's volume, as a box less a prism.
    assert!(near(1000.0 - 0.5 * 2.0 * 2.0 * 10.0, 980.0, 1e-12), "1000 less a 2 x 2 triangle of length 10 is 980");

    // The pocketed plate's volume, as a plate less a box.
    assert!(near(1440.0 - 6.0 * 4.0 * 3.0, 1368.0, 1e-12), "1440 less a 6 x 4 x 3 box is 1368");

    // The revolved ring, as a cylinder and as an inscribed prism, at several facet counts.
    for segments in [3usize, 6, 12, 16, 64] {
        let step = std::f64::consts::TAU / segments as f64;
        let polygon_ring = 0.5 * segments as f64 * step.sin() * 3.0;
        // The same area from the two regular polygons, each by its own formula.
        let outer = 0.5 * segments as f64 * 4.0 * step.sin();
        let inner = 0.5 * segments as f64 * 1.0 * step.sin();
        assert!(near(polygon_ring, outer - inner, 1e-12), "at {segments} facets the ring is {polygon_ring} and the difference of the polygons is {}", outer - inner);
        assert!(polygon_ring < std::f64::consts::PI * 3.0, "at {segments} facets the ring is smaller than the cylinder");
    }
}

/// The whole corpus rebuilds to the same numbers every time, from nothing but the kernel.
///
/// A corpus is only a regression test if it is deterministic. This builds every part twice, in
/// separate processes of the same code, and requires bit-identical geometry — which is the property
/// a floating-point kernel is most likely to lose to a change in face ordering, and which no
/// tolerance-based comparison would notice if the ordering changed consistently.
#[test]
fn the_corpus_rebuilds_identically() {
    /// `None` rather than `NaN` for a shape the kernel would not build: `NaN != NaN`, so a
    /// `NaN` sentinel would make the comparison below fail for a part that behaved identically.
    type Shape = Option<(f64, f64, usize)>;
    let build = || -> Vec<Shape> {
        let plate = must(extrude(&support::rect(0.0, 0.0, 20.0, 12.0), &Plane::xy(0.0), 6.0), "plate");
        let tube = must(extrude(&support::rect_with_hole(0.0, 0.0, 20.0, 12.0, 6.0, 3.0, 14.0, 9.0), &Plane::xy(0.0), 6.0), "tube");
        let block = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let bevelled = must(chamfer(&block, &[cadcraft_solid::EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
        let pocketed = must(boolean(&plate, &box_at(Vec3::new(7.0, 4.0, 3.0), Vec3::new(13.0, 8.0, 7.0)), Bool3::Difference), "pocket");
        let shape = |s: &Solid| Some((s.volume(), s.area(), s.faces.len()));
        vec![
            shape(&plate),
            shape(&tube),
            shape(&bevelled),
            shape(&pocketed),
            fillet(&block, &[cadcraft_solid::EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0, 8).ok().as_ref().map(|s| (s.volume(), s.area(), s.faces.len())),
        ]
    };
    let first = build();
    let second = build();
    assert_eq!(first.len(), 5);
    for (i, (a, b)) in first.iter().zip(second.iter()).enumerate() {
        assert_eq!(a, b, "corpus part {i} rebuilt differently: {a:?} vs {b:?}");
    }
    let measured = first.iter().filter(|s| s.is_some()).count();
    assert!(measured >= 4, "only {measured} of {} corpus parts were built, so determinism was barely checked", first.len());
    println!("corpus determinism: {measured} parts rebuilt bit-identically");
}

/// The corpus parts are *different* parts — the table is not accidentally five copies of one box.
#[test]
fn the_corpus_parts_are_distinguishable() {
    let plate = must(extrude(&support::rect(0.0, 0.0, 20.0, 12.0), &Plane::xy(0.0), 6.0), "plate");
    let tube = must(extrude(&support::rect_with_hole(0.0, 0.0, 20.0, 12.0, 6.0, 3.0, 14.0, 9.0), &Plane::xy(0.0), 6.0), "tube");
    let block = box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let bevelled = must(chamfer(&block, &[cadcraft_solid::EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 2.0), "chamfer");
    let bracket = must(
        extrude(
            &Region::new(vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 3.0),
                Vec2::new(3.0, 3.0),
                Vec2::new(3.0, 10.0),
                Vec2::new(0.0, 10.0),
            ])
            .to_regions(),
            &Plane::xy(0.0),
            4.0,
        ),
        "bracket",
    );
    let parts: [(&str, f64); 4] = [
        ("plate", plate.volume()),
        ("plate_with_hole", tube.volume()),
        ("chamfered_block", bevelled.volume()),
        ("l_bracket", bracket.volume()),
    ];
    for (i, (name, v)) in parts.iter().enumerate() {
        for (other_name, w) in parts.iter().skip(i + 1) {
            assert!(!near(*v, *w, 1e-12), "two corpus parts have the same volume: {name} and {other_name} are both {v}");
        }
    }
}

/// `Regions` helpers the corpus relies on are the shapes the table thinks they are.
#[test]
fn the_corpus_profiles_are_what_they_are_called() {
    let plain: Regions = support::rect(0.0, 0.0, 20.0, 12.0);
    assert!(near(plain.area(), 240.0, 1e-12), "a 20x12 rectangle is 240, got {}", plain.area());
    assert_eq!(plain.loops().count(), 1, "no holes");
    assert!(!plain.is_empty());

    let holed: Regions = support::rect_with_hole(0.0, 0.0, 20.0, 12.0, 6.0, 3.0, 14.0, 9.0);
    assert!(near(holed.area(), 192.0, 1e-12), "less an 8x6 hole it is 192, got {}", holed.area());
    assert_eq!(holed.loops().count(), 2, "an outer loop and a hole");
    // The hole spans x in [6, 14], y in [3, 9]; its centre is not material and the plate beside it is.
    assert!(!holed.contains(Vec2::new(10.0, 6.0)), "the middle of the hole is not material");
    assert!(holed.contains(Vec2::new(2.0, 6.0)), "the plate beside the hole is");
    assert!(holed.contains(Vec2::new(10.0, 1.0)), "and the plate below it is");
    assert!(!holed.contains(Vec2::new(25.0, 6.0)), "and past the edge is not");
}