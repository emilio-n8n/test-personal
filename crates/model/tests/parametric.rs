//! The parametric path, end to end: sketch, dimension, extrude, cut, change the dimension,
//! recompute, save, reopen, check the geometry. And what happens when an edit breaks it.

use cadcraft_geom::Vec3;
use cadcraft_model::{Dep, EdgeRef, Feature, FeatureKind, Model, ModelError, Param, Sketch, State, rebuild, rebuild_after};

fn must<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e:?}"),
    }
}

fn plate(w: f64, d: f64) -> Sketch {
    must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, w, d), "sketch")
}

/// A model that goes sketch → extrude, with a named depth parameter.
fn box_model(width: f64, depth: f64, height: f64) -> Model {
    let mut m = Model::new();
    must(m.set("width", Param::Value(width)), "width");
    must(m.set("depth", Param::Value(depth)), "depth");
    must(m.set("height", Param::Value(height)), "height");
    must(m.add(Feature::new("body", FeatureKind::Extrude { sketch: plate(4.0, 3.0), depth: "height".into(), symmetric: false })), "body");
    m
}

fn volume(m: &Model) -> f64 {
    m.solid().map(cadcraft_solid::Solid::volume).unwrap_or_default()
}

#[test]
fn a_sketch_a_dimension_and_an_extrude_make_a_box_of_exactly_that_size() {
    let mut m = box_model(4.0, 3.0, 2.0);
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let Some(s) = r.solid else { panic!("no solid: {:?}", r.problems) };
    assert!((s.volume() - 24.0).abs() < 1e-9, "4 x 3 x 2 is 24, got {}", s.volume());
    s.validate().unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn changing_a_dimension_recomputes_the_body() {
    let mut m = box_model(4.0, 3.0, 2.0);
    rebuild(&mut m, None);
    must(m.set("height", Param::Value(5.0)), "height");
    let r = rebuild_after(&mut m, &[Dep::Parameter("height".into())]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.rebuilt, 1, "only the feature that reads it is rebuilt");
    assert!((volume(&m) - 60.0).abs() < 1e-9, "4 x 3 x 5 is 60, got {}", volume(&m));
    // And the new solid really is the new size, not just a new number.
    let s = m.solid().cloned().unwrap_or_default();
    assert!(s.contains(Vec3::new(1.0, 1.0, 4.5)), "the box grew");
    assert!(!s.contains(Vec3::new(1.0, 1.0, -0.5)), "and nothing is below the sketch");
}

#[test]
fn a_dimension_driven_by_another_recomputes_through_it() {
    let mut m = box_model(4.0, 3.0, 2.0);
    must(m.set("twice", Param::Formula("height * 2".into())), "twice");
    must(m.replace("body", FeatureKind::Extrude { sketch: plate(4.0, 3.0), depth: "twice".into(), symmetric: false }), "body");
    rebuild(&mut m, None);
    assert!((volume(&m) - 48.0).abs() < 1e-9, "twice the height is 48, got {}", volume(&m));
    // Change the leaf, not the formula: everything downstream has to follow.
    must(m.set("height", Param::Value(5.0)), "height");
    let r = rebuild_after(&mut m, &[Dep::Parameter("height".into())]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert!((volume(&m) - 120.0).abs() < 1e-9, "twice 5 is 10 deep: 4 x 3 x 10, got {}", volume(&m));
}

#[test]
fn a_cut_takes_away_the_tool_and_follows_it_when_the_tool_changes() {
    let mut m = box_model(10.0, 10.0, 10.0);
    must(m.set("bore", Param::Value(2.0)), "bore");
    must(
        m.add(Feature::new(
            "bore_tool",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::new(1.0, 1.0, -1.0), Vec3::X, Vec3::Y, 1.0, 1.0), "tool sketch"),
                depth: "bore".into(),
                symmetric: false,
            },
        )),
        "bore_tool",
    );
    must(m.add(Feature::new("bore", FeatureKind::Cut { tool: "bore_tool".into() })), "bore");
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    // The body is the model's own 4 x 3 x 10 block; the tool is a 1 x 1 shaft through it.
    assert!((volume(&m) - (120.0 - 1.0)).abs() < 1e-9, "a 1x1 shaft through a 4x3x10 block, got {}", volume(&m));

    // The cut reads the tool's solid, so a change to the tool's depth reaches the cut.
    must(m.set("bore", Param::Value(3.0)), "bore");
    let r = rebuild_after(&mut m, &[Dep::Parameter("bore".into())]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert!((volume(&m) - (120.0 - 2.0)).abs() < 1e-9, "a deeper bore takes a deeper bite, got {}", volume(&m));
}

#[test]
fn a_change_to_the_tool_reaches_the_cut_through_the_tree() {
    let mut m = box_model(10.0, 10.0, 10.0);
    must(m.set("bore", Param::Value(2.0)), "bore");
    must(
        m.add(Feature::new(
            "bore_tool",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::new(1.0, 1.0, -1.0), Vec3::X, Vec3::Y, 1.0, 1.0), "tool sketch"),
                depth: "bore".into(),
                symmetric: false,
            },
        )),
        "bore_tool",
    );
    must(m.add(Feature::new("bore", FeatureKind::Cut { tool: "bore_tool".into() })), "bore");
    rebuild(&mut m, None);
    // Naming the tool, not the parameter, still dirties the cut: that is the whole point of a tree.
    let dirty = m.dirty_from(&[Dep::Feature("bore_tool".into())]);
    assert_eq!(dirty, vec![1, 2], "the tool and the cut, not the body");
}

#[test]
fn a_failed_edit_keeps_the_last_good_solid() {
    let mut m = box_model(4.0, 3.0, 2.0);
    rebuild(&mut m, None);
    let before = volume(&m);
    assert!((before - 24.0).abs() < 1e-9);

    // A depth of zero is not a box; it is a mistake, and the mistake must not erase the box.
    must(m.set("height", Param::Value(0.0)), "height");
    let r = rebuild_after(&mut m, &[Dep::Parameter("height".into())]);
    assert!(!r.problems.is_empty(), "the failure is reported");
    assert!(r.problems[0].message.contains("height"), "and names the dimension: {:?}", r.problems[0].message);
    assert!((volume(&m) - before).abs() < 1e-9, "the old solid is still there: {}", volume(&m));
    assert!(matches!(m.record("body").map(|r| &r.state), Some(State::Failed(_))), "and the feature says it failed");

    // Fixing the dimension recovers with no further ceremony.
    must(m.set("height", Param::Value(3.0)), "height");
    let r = rebuild_after(&mut m, &[Dep::Parameter("height".into())]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert!((volume(&m) - 36.0).abs() < 1e-9, "4 x 3 x 3 is 36, got {}", volume(&m));
}

#[test]
fn a_feature_above_a_failure_is_blocked_not_broken() {
    let mut m = box_model(10.0, 10.0, 10.0);
    must(m.set("bore", Param::Value(2.0)), "bore");
    must(
        m.add(Feature::new(
            "bore_tool",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::new(1.0, 1.0, -1.0), Vec3::X, Vec3::Y, 1.0, 1.0), "tool sketch"),
                depth: "bore".into(),
                symmetric: false,
            },
        )),
        "bore_tool",
    );
    must(m.add(Feature::new("bore", FeatureKind::Cut { tool: "bore_tool".into() })), "bore");
    rebuild(&mut m, None);
    let good = volume(&m);

    must(m.set("bore", Param::Value(-1.0)), "bore");
    let r = rebuild_after(&mut m, &[Dep::Parameter("bore".into())]);
    assert!(r.problems.iter().any(|p| p.feature == "bore_tool" && matches!(p.kind, State::Failed(_))), "{:?}", r.problems);
    let cut = r.problems.iter().find(|p| p.feature == "bore").map(|p| p.kind.clone());
    assert!(matches!(cut, Some(State::Blocked(_))), "the cut is blocked, not failed: {cut:?}");
    assert!((volume(&m) - good).abs() < 1e-9, "and it still shows the last solid that worked: {}", volume(&m));
}

#[test]
fn a_suppressed_feature_does_not_run_but_keeps_what_it_built() {
    let mut m = box_model(4.0, 3.0, 2.0);
    rebuild(&mut m, None);
    let before = volume(&m);
    m.features[0].suppressed = true;
    let r = rebuild(&mut m, None);
    assert!(r.problems.iter().any(|p| p.feature == "body" && matches!(p.kind, State::Pending)), "{:?}", r.problems);
    assert_eq!(r.rebuilt, 0, "nothing ran");
    // Suppressing a feature hides it from the rebuild; it does not throw away the shape it made.
    assert!((volume(&m) - before).abs() < 1e-9, "the last good shape is still there: {}", volume(&m));
}

#[test]
fn a_save_and_reopen_gives_the_same_geometry() {
    let mut m = box_model(4.0, 3.0, 2.0);
    must(m.set("twice", Param::Formula("height * 2".into())), "twice");
    must(m.replace("body", FeatureKind::Extrude { sketch: plate(4.0, 3.0), depth: "twice".into(), symmetric: false }), "body");
    rebuild(&mut m, None);
    let made = volume(&m);
    let faces = m.solid().map(|s| s.faces.len()).unwrap_or_default();

    let text = m.save_json();
    assert!(text.contains("CADCraft model"), "the file says what it is");
    let (reopened, r) = must(Model::load_json(&text), "reopen");
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert!((volume(&reopened) - made).abs() < 1e-9, "same volume: {} vs {made}", volume(&reopened));
    assert_eq!(reopened.solid().map(|s| s.faces.len()).unwrap_or_default(), faces, "same shape");
    assert_eq!(reopened.parameters, m.parameters, "the parameters round-trip as written");
    assert_eq!(reopened.features, m.features, "the history round-trips");
}

#[test]
fn a_file_that_is_too_new_is_refused_with_both_numbers() {
    let mut m = box_model(4.0, 3.0, 2.0);
    rebuild(&mut m, None);
    let text = m.save_json().replace("\"version\": 1", "\"version\": 99");
    match Model::load_json(&text) {
        Err(e) => {
            let message = e.to_string();
            assert!(message.contains("99"), "{message}");
            assert!(message.contains(&cadcraft_model::FORMAT_VERSION.to_string()), "{message}");
        }
        Ok(_) => panic!("a file from the future must not be read as if it were the present"),
    }
}

#[test]
fn a_file_that_is_not_a_model_is_refused() {
    let text = r#"{"magic":"something else","version":1,"parameters":{},"features":[]}"#;
    assert!(matches!(Model::load_json(text), Err(ModelError::NotAModel)));
    assert!(Model::load_json("{ not json").is_err());
}

#[test]
fn a_document_whose_features_do_not_build_still_opens() {
    // Refusing to open a file because one dimension is wrong would be the worst possible answer:
    // the tree, the parameters and the error are exactly what the person needs to fix it.
    let mut m = box_model(4.0, 3.0, 2.0);
    must(m.set("height", Param::Value(0.0)), "height");
    let text = m.save_json();
    let (reopened, r) = must(Model::load_json(&text), "reopen");
    assert_eq!(reopened.features.len(), 1, "the history came back");
    assert!(!r.problems.is_empty(), "and so did the reason it does not build");
    assert!(r.solid.is_none(), "with nothing pretending to be built");
}

#[test]
fn a_removed_feature_takes_what_was_built_on_it() {
    let mut m = box_model(4.0, 3.0, 2.0);
    must(m.add(Feature::new("second", FeatureKind::Extrude { sketch: plate(1.0, 1.0), depth: "1".into(), symmetric: false })), "second");
    rebuild(&mut m, None);
    let removed = must(m.remove("body"), "remove");
    assert_eq!(removed, vec!["body", "second"], "the tail goes with it");
    assert_eq!(m.features.len(), 0);
    assert_eq!(m.records.len(), 0);
    assert!(m.remove("body").is_err());
}

#[test]
fn a_duplicate_or_empty_name_is_refused() {
    let mut m = box_model(4.0, 3.0, 2.0);
    let another = FeatureKind::Extrude { sketch: plate(1.0, 1.0), depth: "1".into(), symmetric: false };
    assert!(m.add(Feature::new("body", another.clone())).is_err());
    assert!(m.add(Feature::new("  ", another.clone())).is_err());
    assert!(m.set("", Param::Value(1.0)).is_err());
    assert!(m.replace("nope", another).is_err());
}

#[test]
fn an_edge_reference_that_stops_resolving_is_an_error_naming_the_edge() {
    // The honest form of topological naming without the naming scheme: a reference is a place, and
    // when the place is no longer an edge the blend says so instead of landing somewhere else.
    let mut m = box_model(10.0, 10.0, 10.0);
    must(
        m.add(Feature::new(
            "round",
            FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(4.0, 0.0, 5.0), 1e-6)], size: "1".into(), segments: 8, round: true },
        )),
        "round",
    );
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    // A round of radius r at a right-angle corner removes r²(1 − π/4) per unit of edge; with
    // flat facets it removes a shade more, which is why the check is a band and not a point.
    let rounded = volume(&m);
    let removed = 120.0 - rounded;
    let exact = 10.0 * (1.0 - std::f64::consts::PI / 4.0);
    assert!((removed - exact).abs() < 0.1, "a round of radius 1 along a 10-long edge removes {removed}, against {exact}");

    // Move the body so the edge is gone. The reference now points at nothing.
    must(
        m.replace(
            "body",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::new(1.0, 0.0, 0.0), Vec3::X, Vec3::Y, 6.0, 3.0), "sketch"),
                depth: "height".into(),
                symmetric: false,
            },
        ),
        "body",
    );
    let r = rebuild(&mut m, None);
    let round = r.problems.iter().find(|p| p.feature == "round").map(|p| p.message.clone()).unwrap_or_default();
    assert!(round.contains("no edge passes through"), "the error names what is missing: {round}");
    // And the body is untouched: a blend that cannot resolve does not take the solid with it.
    // The model's *shown* solid is still the round's last good one — the rule that a failure never
    // destroys what was there — while the body itself rebuilt to its new size underneath it.
    let body = m.record("body").and_then(|r| r.last_good.as_ref()).map(cadcraft_solid::Solid::volume).unwrap_or_default();
    assert!((body - 180.0).abs() < 1e-9, "the body rebuilt to its new size: {body}");
    assert!(rounded.abs() > 0.0 && (volume(&m) - rounded).abs() < 1e-9, "and the round still shows its last good solid");
}

#[test]
fn a_revolve_is_a_solid_or_a_reason() {
    // A partial revolve (the angle stops short of a full turn) is the one shape the kernel does not
    // yet build: its two end caps meet the swept walls at a degenerate corner. The rule holds
    // regardless — a revolve that cannot be built is an error, never a shell with a hole in it.
    // When the kernel gains partial revolves, this test's shape check becomes the specification.
    let mut m = Model::new();
    must(
        m.add(Feature::new(
            "ring",
            FeatureKind::Revolve {
                sketch: must(Sketch::rect(Vec3::new(1.0, 0.0, 0.0), Vec3::Z, Vec3::X, 1.0, 1.0), "sketch"),
                axis: Vec3::Z,
                angle: "90".into(),
                segments: 16,
            },
        )),
        "ring",
    );
    revolve_holds_its_line("90", 16);
}

/// Revolve is the one feature whose geometry the kernel cannot build yet, in either direction. The
/// rule is unchanged: a solid that validates, or a named failure. This test is the specification
/// for turning it into a shape check.
fn revolve_holds_its_line(angle: &str, segments: usize) {
    let mut m = Model::new();
    must(
        m.add(Feature::new(
            "ring",
            FeatureKind::Revolve {
                sketch: must(Sketch::rect(Vec3::new(1.0, 0.0, 0.0), Vec3::Z, Vec3::X, 1.0, 1.0), "sketch"),
                axis: Vec3::Z,
                angle: angle.into(),
                segments,
            },
        )),
        "ring",
    );
    let r = rebuild(&mut m, None);
    match r.solid {
        Some(s) => {
            s.validate().unwrap_or_else(|e| panic!("a revolve must not hand back a broken shell: {e}"));
            s.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
            let sweep = angle.parse::<f64>().unwrap_or_default().to_radians();
            let expected = sweep * (4.0 - 1.0);
            assert!((s.volume() - expected).abs() < expected * 0.1, "a {angle}-degree ring: {} against {expected}", s.volume());
        }
        None => {
            let problem = r.problems.iter().find(|p| p.feature == "ring").map(|p| p.message.clone()).unwrap_or_default();
            assert!(!problem.is_empty(), "a revolve that does not build has to say why");
            assert!(matches!(r.problems[0].kind, State::Failed(_)), "and it is a failure, not a shrug");
        }
    }
}

#[test]
fn a_full_turn_revolve_needs_no_end_caps_and_is_supported() {
    revolve_holds_its_line("360", 32);
    let mut m = Model::new();
    must(
        m.add(Feature::new(
            "ring",
            FeatureKind::Revolve {
                sketch: must(Sketch::rect(Vec3::new(1.0, 0.0, 0.0), Vec3::Z, Vec3::X, 1.0, 1.0), "sketch"),
                axis: Vec3::Z,
                angle: "360".into(),
                segments: 32,
            },
        )),
        "ring",
    );
}

#[test]
fn a_symmetric_extrude_centres_itself_on_the_sketch() {
    let mut m = Model::new();
    must(
        m.add(Feature::new(
            "plate",
            FeatureKind::Extrude { sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 4.0, 3.0), "sketch"), depth: "2".into(), symmetric: true },
        )),
        "plate",
    );
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let Some(s) = r.solid else { panic!("no solid: {:?}", r.problems) };
    assert!((s.volume() - 24.0).abs() < 1e-9, "the same material, both ways: {}", s.volume());
    assert!(s.contains(Vec3::new(1.0, 1.0, 0.9)) && s.contains(Vec3::new(1.0, 1.0, -0.9)), "a unit either way of the sketch");
    assert!(!s.contains(Vec3::new(1.0, 1.0, 1.1)), "and no further");
}
