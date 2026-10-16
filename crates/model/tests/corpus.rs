//! The same corpus as `crates/solid/tests/corpus.rs`, built through the **parametric** path.
//!
//! `crates/solid/tests/corpus.rs` pins the numbers the *kernel* produces. This file pins the numbers
//! a *document* produces, which is a different claim: that a sketch, a named parameter and a
//! feature tree rebuild to the same closed form the kernel does, that a parameter edit moves the
//! number to the new closed form rather than to a new arbitrary one, and that a saved file reopens
//! to the same geometry.
//!
//! The expected values are the same closed forms, written out again in full rather than imported:
//!
//! | part             | features                                | V    |
//! |------------------|-----------------------------------------|------|
//! | `plate`          | extrude                                 | 1440 |
//! | `plate_with_hole` | extrude of a two-loop sketch            | 1152 |
//! | `l_bracket`      | extrude of a concave sketch             | 204  |
//! | `chamfered_block`| extrude + blend                         | 980  |
//! | `pocketed_plate` | extrude + tool extrude + cut            | 1368 |
//! | `revolved_ring`  | revolve (the kernel cannot build it yet) | —    |
//!
//! Every part is driven by a **named parameter**, not a literal in the feature: that is what makes
//! this a *parametric* corpus, and it is why changing `height` from 6 to 8 can be checked against
//! `20 · 12 · 8 = 1920` rather than against "whatever it was last time".
//!
//! Deterministic, no files on disk, no `rand`, no wall clock.
//!
//! **Execution status: every test in this file is `NOT EXECUTED`.** It was written without a
//! terminal and has never been compiled or run; see the report that came with it.

use cadcraft_geom::{Vec2, Vec3};
use cadcraft_model::{Dep, EdgeRef, Feature, FeatureKind, Model, Param, Rebuild, Sketch, State, rebuild, rebuild_after};

fn must<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e:?}"),
    }
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

// ---------- sketch helpers ----------
//
// A sketch's loops are stored in the plane's own frame, and the kernel's convention is outer
// counter-clockwise, holes clockwise. `Sketch::new` takes world-space loops and projects them, so
// the winding written here is the winding seen from the plane's normal side.

// A rectangle drawn from `at`, `w` along `u` and `h` along `v`. World-space loop, counter-clockwise.
fn rect_loop(at: Vec3, u: Vec3, v: Vec3, w: f64, h: f64) -> Vec<Vec3> {
    vec![at, at + u * w, at + u * w + v * h, at + v * h]
}

/// A rectangle drawn the *other* way round, for a hole.
fn hole_loop(at: Vec3, u: Vec3, v: Vec3, w: f64, h: f64) -> Vec<Vec3> {
    let l = rect_loop(at, u, v, w, h);
    l.into_iter().rev().collect()
}

/// A sketch on the XY plane at `z`, from world-space loops.
fn sketch_xy(z: f64, loops: Vec<Vec<Vec3>>) -> Sketch {
    must(Sketch::new(Vec3::new(0.0, 0.0, z), Vec3::Z, Vec3::X, loops), "sketch at z")
}

/// The volume the model currently shows, or 0.
fn volume(m: &Model) -> f64 {
    m.solid().map(cadcraft_solid::Solid::volume).unwrap_or_default()
}

/// Assert the rebuild succeeded with no problems, and return the solid's volume.
fn clean_volume(m: &mut Model, what: &str) -> f64 {
    let r: Rebuild = rebuild(m, None);
    assert!(r.problems.is_empty(), "{what}: {:?}", r.problems);
    assert!(r.solid.is_some(), "{what}: no solid came back");
    r.solid.as_ref().unwrap_or_default().validate().unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = volume(m);
    assert!(v > 0.0, "{what}: volume {v}");
    v
}

fn add(m: &mut Model, name: &str, kind: FeatureKind) {
    must(m.add(Feature::new(name, kind)), name);
}

fn param(m: &mut Model, name: &str, v: f64) {
    must(m.set(name, Param::Value(v)), name);
}

// ====================================================================================
// 1. plate
// ====================================================================================

/// A 20 x 12 plate, 6 deep, from one sketch and one named depth.
///
/// ```text
/// volume = w · d · h = 20 · 12 · 6 = 1440
/// ```
#[test]
fn a_plate_driven_by_a_named_depth() {
    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    let v = clean_volume(&mut m, "plate");
    assert!(near(v, 20.0 * 12.0 * 6.0, 1e-12), "20 x 12 x 6 is 1440, got {v}");
    assert!(near(v, 1440.0, 1e-12), "1440, got {v}");

    // The parametric claim: the number follows the parameter, and the new number is also closed-form.
    must(m.set("height", Param::Value(8.0)), "height");
    let r = rebuild_after(&mut m, &[Dep::Parameter("height".into())]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.rebuilt, 1, "only the feature that reads `height` is rebuilt");
    let v = volume(&m);
    assert!(near(v, 20.0 * 12.0 * 8.0, 1e-12), "20 x 12 x 8 is 1920, got {v}");

    // And a dimension driven by another dimension follows through it.
    must(m.set("twice", Param::Formula("height * 2".into())), "twice");
    must(
        m.replace(
            "body",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
                depth: "twice".into(),
                symmetric: false,
            },
        ),
        "body",
    );
    assert!(near(clean_volume(&mut m, "plate x2"), 20.0 * 12.0 * 16.0, 1e-12), "twice 8 is 16 deep: 3840, got {}", volume(&m));

    // A symmetric extrusion is the same volume from the same sketch, centred on the plane.
    must(
        m.replace(
            "body",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
                depth: "height".into(),
                symmetric: true,
            },
        ),
        "body",
    );
    assert!(
        near(clean_volume(&mut m, "symmetric plate"), 20.0 * 12.0 * 8.0, 1e-12),
        "a symmetric extrude of depth 8 is still 8 deep: 1920, got {}",
        volume(&m)
    );
}

// ====================================================================================
// 2. plate with a hole
// ====================================================================================

/// A 20 x 12 plate, 6 deep, with an 8 x 6 hole, from a two-loop sketch.
///
/// ```text
/// section = w·d − hw·hh = 240 − 48 = 192
/// volume  = 192 · 6 = 1152
/// ```
///
/// The hole loop is written clockwise, which is the kernel's convention: loops are wound so the
/// material is on their left, and that is what makes the walls of a hole face outwards. A sketch
/// with the hole the other way round is a different solid, and this test would notice.
#[test]
fn a_plate_with_a_hole_from_a_two_loop_sketch() {
    let outer = rect_loop(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0);
    let hole = hole_loop(Vec3::new(6.0, 3.0, 0.0), Vec3::X, Vec3::Y, 8.0, 6.0);
    let sketch = sketch_xy(0.0, vec![outer, hole]);

    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    add(&mut m, "body", FeatureKind::Extrude { sketch, depth: "height".into(), symmetric: false });
    let v = clean_volume(&mut m, "plate with a hole");
    assert!(near(v, (20.0 * 12.0 - 8.0 * 6.0) * 6.0, 1e-12), "192 x 6 is 1152, got {v}");

    let s = m.solid().cloned().unwrap_or_default();
    assert!(!s.contains(Vec3::new(10.0, 6.0, 3.0)), "the hole is open through the whole plate");
    assert!(s.contains(Vec3::new(2.0, 6.0, 3.0)), "and the wall beside it is material");
}

// ====================================================================================
// 3. L-bracket
// ====================================================================================

/// A concave sketch extruded 4 deep.
///
/// ```text
/// profile area by the shoelace = ½|0 + 30 + 21 + 21 + 30 + 0| = 51
/// volume = 51 · 4 = 204
/// ```
///
/// This is the corpus's concave shape, and the parametric layer's reason for existing: a sketch is
/// stored as drawn, so the first thing a rebuild has to get right is that it reproduces an *odd*
/// profile rather than only rectangles.
#[test]
fn an_l_bracket_from_a_concave_sketch() {
    let l = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(10.0, 3.0, 0.0),
        Vec3::new(3.0, 3.0, 0.0),
        Vec3::new(3.0, 10.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
    ];
    let mut m = Model::new();
    param(&mut m, "thickness", 4.0);
    add(&mut m, "bracket", FeatureKind::Extrude { sketch: sketch_xy(0.0, vec![l]), depth: "thickness".into(), symmetric: false });
    let v = clean_volume(&mut m, "L-bracket");
    assert!(near(v, 51.0 * 4.0, 1e-12), "51 x 4 is 204, got {v}");

    let s = m.solid().cloned().unwrap_or_default();
    assert!(!s.contains(Vec3::new(8.0, 8.0, 2.0)), "the notch is not material — the concave profile survived the round trip");

    must(m.set("thickness", Param::Value(6.0)), "thickness");
    rebuild_after(&mut m, &[Dep::Parameter("thickness".into())]);
    assert!(near(volume(&m), 51.0 * 6.0, 1e-12), "6 deep is 306, got {}", volume(&m));
}

// ====================================================================================
// 4. chamfered block
// ====================================================================================

/// A 10-cube with one vertical edge bevelled by 2, driven by a named blend size.
///
/// ```text
/// volume = 1000 − (d²/2)·L = 1000 − 2·10 = 980
/// ```
///
/// The blend's edge reference is stored as a *place*, not as an index — `EdgeRef::new((10, 0, 5))`
/// — and the blend feature reads its size from a parameter like everything else. That is the whole
/// parametric story for a blend: the geometry is re-derived from a point and a number, and the
/// number is the only thing that changed.
#[test]
fn a_chamfered_block_built_from_an_edge_reference() {
    let mut m = Model::new();
    param(&mut m, "height", 10.0);
    param(&mut m, "bevel", 2.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    add(
        &mut m,
        "bevel",
        FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], size: "bevel".into(), segments: 1, round: false },
    );
    let v = clean_volume(&mut m, "chamfered block");
    assert!(near(v, 1000.0 - 2.0, 1e-9), "1000 less the 2 x 2 triangular prism of length 10 is 980, got {v}");

    // Move the bevel: the closed form moves with it.
    must(m.set("bevel", Param::Value(1.0)), "bevel");
    rebuild_after(&mut m, &[Dep::Parameter("bevel".into())]);
    let v = volume(&m);
    assert!(near(v, 1000.0 - 0.5 * 1.0 * 1.0 * 10.0, 1e-9), "a bevel of 1 removes 5, so 995, got {v}");

    // And a round of the same radius removes *less* than the bevel, and is inside the band.
    must(
        m.replace(
            "bevel",
            FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], size: "bevel".into(), segments: 8, round: true },
        ),
        "bevel",
    );
    rebuild(&mut m, None);
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let rounded = volume(&m);
    let removed_per_edge = (1000.0 - rounded) / 10.0;
    let exact = 1.0 * (1.0 - std::f64::consts::FRAC_PI_4);
    assert!(
        removed_per_edge >= exact * (1.0 - 1e-9),
        "a round of radius 1 removed {removed_per_edge} per unit, under the {exact} a true round removes"
    );
    assert!(removed_per_edge <= 0.5 * (1.0 + 1e-9), "a round of radius 1 removed {removed_per_edge} per unit, over the 0.5 a bevel removes");
}

// ====================================================================================
// 5. pocketed plate
// ====================================================================================

/// A 20 x 12 x 6 plate with a 6 x 4 blind pocket 3 deep, cut by a second extrude.
///
/// ```text
/// tool   = a 6 x 4 sketch at z = 3, extruded 4   (so it spans z in [3, 7])
/// removed = 6 · 4 · 3 = 72                    (only the part inside the plate)
/// volume  = 1440 − 72 = 1368
/// ```
///
/// The tool crosses the plate's top face, so the pocket's floor is a face of the *tool* rather than
/// of the body. That is a different boolean path from a through hole and a different dirty-walk: the
/// cut reads the tool's solid, so changing the tool's depth reaches the cut and changing the plate's
/// height does not move the tool.
#[test]
fn a_plate_with_a_blind_pocket_cut_by_a_second_extrude() {
    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    param(&mut m, "pocket_depth", 4.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    // The tool's sketch sits at z = 3 and its extrusion runs 4, so it reaches z = 7 — one unit above
    // the plate — and its floor is 3 below the plate's top face.
    add(
        &mut m,
        "pocket_tool",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::new(7.0, 4.0, 3.0), Vec3::X, Vec3::Y, 6.0, 4.0), "tool sketch"),
            depth: "pocket_depth".into(),
            symmetric: false,
        },
    );
    add(&mut m, "pocket", FeatureKind::Cut { tool: "pocket_tool".into() });

    let v = clean_volume(&mut m, "pocketed plate");
    assert!(near(v, 20.0 * 12.0 * 6.0 - 6.0 * 4.0 * 3.0, 1e-9), "1440 less a 6 x 4 x 3 pocket is 1368, got {v}");

    let s = m.solid().cloned().unwrap_or_default();
    assert!(!s.contains(Vec3::new(10.0, 6.0, 5.0)), "the pocket is open");
    assert!(s.contains(Vec3::new(10.0, 6.0, 3.5)), "and has a floor");
    assert!(s.contains(Vec3::new(10.0, 6.0, 0.5)), "and the plate is solid under it");

    // The dirty walk: the cut reads the tool, so a tool change reaches the cut and only the cut.
    let dirty = m.dirty_from(&[Dep::Feature("pocket_tool".into())]);
    assert_eq!(dirty, vec![1, 2], "the tool and the cut, not the body: {dirty:?}");
    let dirty = m.dirty_from(&[Dep::Parameter("pocket_depth".into())]);
    assert_eq!(dirty, vec![1, 2], "the tool's depth reaches the tool and the cut: {dirty:?}");

    // A deeper tool removes more: its floor is still 3 below the top, but the part inside the plate
    // is 3 deep whatever the tool's total length, as long as it reaches the top face.
    must(m.set("pocket_depth", Param::Value(6.0)), "pocket_depth");
    rebuild_after(&mut m, &[Dep::Parameter("pocket_depth".into())]);
    assert!(near(volume(&m), 1368.0, 1e-9), "a longer tool through the same floor removes the same material: {}", volume(&m));

    // A shallower floor does remove less.
    must(m.set("pocket_depth", Param::Value(1.5)), "pocket_depth");
    rebuild_after(&mut m, &[Dep::Parameter("pocket_depth".into())]);
    let v = volume(&m);
    assert!(v > 1368.0, "a tool that stops inside the plate removes less, so the volume grows past 1368: {v}");
    assert!(near(v, 1440.0 - 6.0 * 4.0 * 1.5, 1e-9), "a 1.5-deep pocket removes 36: {}", v);
}

// ====================================================================================
// 6. revolved ring
// ====================================================================================

/// A revolved ring: the one corpus part the kernel cannot build yet.
///
/// The rule the parametric layer holds itself to is stated in its own docs: "The feature is wired,
/// refused with the reason, and tested in the form the rule demands: a solid that validates, or a
/// named failure — never a broken shell." So this test asserts exactly that, and nothing else:
///
/// - a solid comes back that validates, **or**
/// - the feature is `Failed` and the message says why.
///
/// It deliberately does **not** assert a volume. The expected value is written down so it is there
/// when the kernel works — `½ · n · sin(2π/n) · (R_o² − R_i²) · h` for the faceted ring — but
/// pinning a number against a code path that always errors would be pinning a number nothing
/// produces.
///
/// `tests/property.rs::revolve_builds_no_solid` pins the defect in the kernel; this pins that the
/// model layer above it stays honest while the defect is open.
#[test]
fn a_revolved_ring_is_a_solid_or_a_named_failure() {
    let ri = 1.0;
    let ro = 2.0;
    let h = 1.0;
    let segments = 16usize;
    let mut m = Model::new();
    // The sketch lies in the XZ plane at y = 0: x is radial, z is axial, and the axis is +Y.
    add(
        &mut m,
        "ring",
        FeatureKind::Revolve {
            sketch: must(Sketch::rect(Vec3::new(ri, 0.0, 0.0), Vec3::X, Vec3::Z, ro - ri, h), "ring sketch"),
            axis: Vec3::Y,
            angle: "360".into(),
            segments,
        },
    );
    let r = rebuild(&mut m, None);
    match r.solid {
        Some(s) => {
            s.validate().unwrap_or_else(|e| panic!("a revolve must not hand back a broken shell: {e}"));
            s.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
            let faceted = 0.5 * segments as f64 * (std::f64::consts::TAU / segments as f64).sin() * (ro * ro - ri * ri) * h;
            assert!(
                near(s.volume(), faceted, 1e-9),
                "a {segments}-facet ring of inner radius {ri}, outer {ro} and height {h} is {faceted}, got {}",
                s.volume()
            );
        }
        None => {
            let problem = r.problems.iter().find(|p| p.feature == "ring").map(|p| p.message.clone()).unwrap_or_default();
            assert!(!problem.is_empty(), "a revolve that does not build has to say why");
            assert!(matches!(r.problems[0].kind, State::Failed(_)), "and it is a failure, not a shrug: {:?}", r.problems[0].kind);
            println!(
                "  revolved_ring NOT BUILT: {problem}\n    expected when it is: V={:.9}",
                0.5 * segments as f64 * (std::f64::consts::TAU / segments as f64).sin() * 3.0
            );
        }
    }
}

// ====================================================================================
// 7. the whole corpus, saved and reopened
// ====================================================================================

/// Every corpus part, saved and reopened, comes back with the same volume.
///
/// A `.ccm` file stores the *history*, not the geometry, so the claim this makes is stronger than
/// a byte-comparison of a shape: it says the file round-trips through the same closed forms. The
/// parameters round-trip as written (`Param::Formula` keeps the text the user typed), the features
/// round-trip in order, and the rebuilt solid has the same volume and the same face count.
#[test]
fn every_corpus_part_survives_a_save_and_reopen() {
    let cases: Vec<(&str, Vec<f64>, Model)> = vec![
        ("plate", vec![1440.0], plate_model()),
        ("plate_with_hole", vec![1152.0], holed_plate_model()),
        ("l_bracket", vec![204.0, 306.0], bracket_model()),
        ("chamfered_block", vec![980.0, 995.0], chamfered_model()),
        ("pocketed_plate", vec![1368.0], pocketed_model()),
    ];
    let mut checked = 0usize;
    for (name, expected, mut m) in cases {
        assert!(near(clean_volume(&mut m, name), expected[0], 1e-9), "{name}: built {} against {}", volume(&m), expected[0]);
        let text = m.save_json();
        assert!(text.contains("CADCraft model"), "{name}: the file says what it is");
        let (reopened, r) = must(Model::load_json(&text), name);
        assert!(r.problems.is_empty(), "{name}: reopening reported {:?}", r.problems);
        assert_eq!(reopened.parameters, m.parameters, "{name}: the parameters round-trip as written");
        assert_eq!(reopened.features, m.features, "{name}: the history round-trips");
        assert!(near(volume(&reopened), volume(&m), 1e-12), "{name}: reopened at {} against {}", volume(&reopened), volume(&m));
        assert_eq!(reopened.solid().map(|s| s.faces.len()), m.solid().map(|s| s.faces.len()), "{name}: the same shape, not just the same volume");
        checked += 1;
    }
    println!("corpus save/reopen: {checked} parts round-tripped through a document");
    assert_eq!(checked, 5);
}

// ---------- the models the corpus is built from ----------

fn plate_model() -> Model {
    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    m
}

fn holed_plate_model() -> Model {
    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    let sketch =
        sketch_xy(0.0, vec![rect_loop(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), hole_loop(Vec3::new(6.0, 3.0, 0.0), Vec3::X, Vec3::Y, 8.0, 6.0)]);
    add(&mut m, "body", FeatureKind::Extrude { sketch, depth: "height".into(), symmetric: false });
    m
}

fn bracket_model() -> Model {
    let mut m = Model::new();
    param(&mut m, "thickness", 4.0);
    let l = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(10.0, 3.0, 0.0),
        Vec3::new(3.0, 3.0, 0.0),
        Vec3::new(3.0, 10.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
    ];
    add(&mut m, "bracket", FeatureKind::Extrude { sketch: sketch_xy(0.0, vec![l]), depth: "thickness".into(), symmetric: false });
    m
}

fn chamfered_model() -> Model {
    let mut m = Model::new();
    param(&mut m, "height", 10.0);
    param(&mut m, "bevel", 2.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    add(
        &mut m,
        "bevel",
        FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], size: "bevel".into(), segments: 1, round: false },
    );
    m
}

fn pocketed_model() -> Model {
    let mut m = Model::new();
    param(&mut m, "height", 6.0);
    param(&mut m, "pocket_depth", 4.0);
    add(
        &mut m,
        "body",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 20.0, 12.0), "sketch"),
            depth: "height".into(),
            symmetric: false,
        },
    );
    add(
        &mut m,
        "pocket_tool",
        FeatureKind::Extrude {
            sketch: must(Sketch::rect(Vec3::new(7.0, 4.0, 3.0), Vec3::X, Vec3::Y, 6.0, 4.0), "tool sketch"),
            depth: "pocket_depth".into(),
            symmetric: false,
        },
    );
    add(&mut m, "pocket", FeatureKind::Cut { tool: "pocket_tool".into() });
    m
}

/// `Vec2` is imported for the same reason `parametric.rs` imports `Vec3`: the sketch loops are
/// written in world coordinates, and this file asserts on the 2D result nowhere — but the import
/// keeps the module's dependency on `cadcraft_geom` explicit rather than incidental.
#[allow(dead_code)]
fn _keeps_the_geom_import_honest(p: Vec2) -> Vec2 {
    p
}
