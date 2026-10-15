//! The document format: what a save/load round trip promises, what survives an edit, and what a
//! file from before the topology map was still able to say for itself.
//!
//! Everything here goes through the crate's public API. The types the topology map introduces are
//! never named — every one of them is reached through a method or a field — so this file also
//! doubles as the statement that the map is usable from outside the crate without an export list
//! that has not been written yet.
//!
//! # Not executed
//!
//! Written by an agent with no terminal. Nothing below has been compiled or run; see the note at
//! the end of the file.

use cadcraft_geom::Vec3;
use cadcraft_model::{Document, EdgeRef, Feature, FeatureKind, Model, ModelError, Param, Sketch, rebuild};

fn must<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e:?}"),
    }
}

fn volume(m: &Model) -> f64 {
    m.solid().map(cadcraft_solid::Solid::volume).unwrap_or_default()
}

/// A body 10 x 10 x 10 with a round on the vertical edge at the origin.
///
/// The reference is `(0, 0, 5)`, the middle of that edge and of no other, which is the only kind
/// of reference the kernel accepts.
fn body_and_round() -> Model {
    let mut m = Model::new();
    must(m.set("height", Param::Value(10.0)), "height");
    must(
        m.add(Feature::new(
            "body",
            FeatureKind::Extrude {
                sketch: must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"),
                depth: "height".into(),
                symmetric: false,
            },
        )),
        "body",
    );
    must(
        m.add(Feature::new(
            "round",
            FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(0.0, 0.0, 5.0), 1e-6)], size: "1".into(), segments: 8, round: true },
        )),
        "round",
    );
    m
}

fn rebuilt() -> Model {
    let mut m = body_and_round();
    let r = rebuild(&mut m, None);
    assert!(r.problems.is_empty(), "the model has to build before any of this is a test of anything: {:?}", r.problems);
    m
}

/// An extrusion of the sketch `sketch`, by the depth `depth`.
fn extrude(sketch: Sketch, depth: &str) -> FeatureKind {
    FeatureKind::Extrude { sketch, depth: depth.into(), symmetric: false }
}

fn as_json(text: &str) -> serde_json::Value {
    must(serde_json::from_str(text), "parse a saved document")
}

// ---------------------------------------------------------------------------------------------
// The round trip.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_saved_and_reopened_document_resolves_its_blend_by_fingerprint() {
    let model = rebuilt();
    let text = model.save_json();
    assert!(text.contains("\"topology\""), "the document carries the fingerprints its blends resolved to");

    let loaded = must(Model::load_json_with_report(&text), "reopen");
    assert_eq!(loaded.resolution.strategies(), vec!["fingerprint"], "{}", loaded.resolution.summary());
    assert!(loaded.rebuild.problems.is_empty(), "and nothing failed to build: {:?}", loaded.rebuild.problems);
    assert!((volume(&loaded.model) - volume(&model)).abs() <= 1e-9, "the same geometry: {} against {}", volume(&loaded.model), volume(&model));
    // A fingerprint that confirmed the reference the document already had must not rewrite it: the
    // point is the one the person chose, and it is their document.
    assert_eq!(loaded.model.features, model.features, "a fingerprint that agreed with the reference changed nothing");
}

#[test]
fn the_saved_document_describes_its_edges_rather_than_its_points() {
    let model = rebuilt();
    let text = model.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");
    assert!(!doc.topology.is_empty(), "one feature's blend contributed a fingerprint");
    let round = must(doc.topology.get("round"), "the round's entry");
    assert_eq!(round.edges.len(), 1, "one reference, one fingerprint");
    let edge = &round.edges[0];
    // The endpoints are recorded so a person can read the file and see which edge it meant.
    assert!((edge.start[0]).abs() <= 1e-9 && (edge.start[1]).abs() <= 1e-9, "the recorded start is {edge:?}");
    assert!((edge.end[2] - 10.0).abs() <= 1e-9, "the recorded end is {edge:?}");
    assert!(edge.length_ratio > 0.0 && edge.length_ratio <= 1.0, "a length over the part's diagonal is a fraction: {}", edge.length_ratio);
    assert!((edge.dihedral).abs() <= 1e-9, "a round corner's two faces are square to each other: {}", edge.dihedral);
    for face in [edge.face_a, edge.face_b] {
        let length = face[0] * face[0] + face[1] * face[1] + face[2] * face[2];
        assert!((length.sqrt() - 1.0).abs() <= 1e-12, "a face normal is a unit vector, got {face:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// What the fingerprint survives, and what it does not.
// ---------------------------------------------------------------------------------------------

/// Move the body so the blended edge is somewhere the stored point does not name, and check that
/// the fingerprint is what finds it.
fn moved_part_resolves(model: &Model, doc: &Document, body: FeatureKind) -> Model {
    let mut moved = Model::new();
    moved.parameters = model.parameters.clone();
    moved.features = model.features.clone();
    moved.records = Vec::new();
    must(moved.replace("body", body), "move the body");
    let text = moved.save_json_with_topology(&doc.topology);
    let loaded = must(Model::load_json_with_report(&text), "reopen the moved part");
    assert_eq!(loaded.resolution.strategies(), vec!["fingerprint"], "{}", loaded.resolution.summary());
    assert!(loaded.rebuild.problems.is_empty(), "the round rebuilt on the moved part: {:?}", loaded.rebuild.problems);
    // The same part, built the ordinary way, for the same number.
    let mut straight = Model::new();
    straight.parameters = moved.parameters.clone();
    straight.features = moved.features.clone();
    straight.records = Vec::new();
    rebuild(&mut straight, None);
    assert!((volume(&loaded.model) - volume(&straight)).abs() <= 1e-9, "same shape: {} against {}", volume(&loaded.model), volume(&straight));
    loaded.model
}

#[test]
fn a_translated_part_still_resolves_by_its_fingerprint() {
    let model = rebuilt();
    let text = model.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");
    // The part moves across the screen. The point the file stored is now in the middle of a face.
    let moved = extrude(must(Sketch::rect(Vec3::new(4.0, 3.0, 0.0), Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"), "height");
    moved_part_resolves(&model, &doc, moved);
}

#[test]
fn a_scaled_part_still_resolves_by_its_fingerprint() {
    let model = rebuilt();
    let text = model.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");
    // Every length in the model multiplied by three. A fingerprint that stored a world coordinate
    // would be wrong here; one that stores a ratio is not.
    let scaled = extrude(must(Sketch::rect(Vec3::new(2.0, 1.0, 0.0), Vec3::X, Vec3::Y, 30.0, 30.0), "sketch"), "30");
    let loaded = moved_part_resolves(&model, &doc, scaled);
    // Twenty-seven times the material, less the round, which removes three times the length's worth
    // of a corner of the same radius. The exact shape is compared inside the helper; this is only
    // here to say the part really did grow.
    let original = volume(&rebuilt());
    assert!(volume(&loaded) > 26.0 * original, "three times the size is a lot more material: {}", volume(&loaded));
    assert!(volume(&loaded) < 27.0 * original, "and the round still took its corner off: {}", volume(&loaded));
}

#[test]
fn a_rotated_part_still_resolves_by_its_fingerprint() {
    let model = rebuilt();
    let text = model.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");
    // Thirty degrees about Z, away from the origin, so the stored point cannot find the edge and
    // the fingerprint is the only thing that can.
    let angle = 30.0f64.to_radians();
    let u = Vec3::new(angle.cos(), angle.sin(), 0.0);
    let v = Vec3::new(-angle.sin(), angle.cos(), 0.0);
    let turned = extrude(must(Sketch::rect(Vec3::new(2.0, 1.0, 0.0), u, v, 10.0, 10.0), "sketch"), "height");
    moved_part_resolves(&model, &doc, turned);
}

#[test]
fn an_edge_that_is_gone_is_reported_unresolved_and_is_not_silently_moved() {
    let model = rebuilt();
    let text = model.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");

    // Away: a different block, whose blended edge simply is not there. The fingerprint does not
    // match anything and the stored point matches nothing either.
    let mut away = Model::new();
    away.parameters = model.parameters.clone();
    away.features = model.features.clone();
    away.records = Vec::new();
    must(away.replace("body", extrude(must(Sketch::rect(Vec3::new(1.0, 0.0, 0.0), Vec3::X, Vec3::Y, 6.0, 3.0), "sketch"), "height")), "away");
    let gone = must(Model::load_json_with_report(&away.save_json_with_topology(&doc.topology)), "reopen the gone part");
    assert_eq!(gone.resolution.strategies(), vec!["unresolved"], "{}", gone.resolution.summary());
    assert!(!gone.resolution.any_fingerprint(), "nothing was matched and nothing was guessed");
    let message = gone.rebuild.problems.iter().find(|p| p.feature == "round").map(|p| p.message.clone()).unwrap_or_default();
    assert!(message.contains("no edge passes through"), "and the rebuild says the honest thing: {message}");
    // The body underneath is untouched: a blend that cannot resolve takes nothing with it.
    let body = gone.model.record("body").and_then(|r| r.last_good.as_ref()).map(cadcraft_solid::Solid::volume).unwrap_or_default();
    assert!((body - 6.0 * 3.0 * 10.0).abs() <= 1e-9, "the body rebuilt to its own size: {body}");

    // And back: the original body returns, and the fingerprint finds the edge again.
    let mut back = Model::new();
    back.parameters = away.parameters.clone();
    back.features = away.features.clone();
    back.records = Vec::new();
    must(back.replace("body", extrude(must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"), "height")), "back");
    let returned = must(Model::load_json_with_report(&back.save_json_with_topology(&doc.topology)), "reopen the returned part");
    assert_eq!(returned.resolution.strategies(), vec!["fingerprint"], "{}", returned.resolution.summary());
    assert!(returned.rebuild.problems.is_empty(), "and the round is back: {:?}", returned.rebuild.problems);
    assert!((volume(&returned.model) - volume(&model)).abs() <= 1e-9, "the same material it started with");
}

#[test]
fn a_reference_that_names_a_corner_is_unresolved_and_not_a_guess() {
    // The point lies on two edges, which is a corner and not an edge. The rule is one hit, and
    // the honest answer to two is "I do not know", not "the first one".
    let mut m = Model::new();
    must(m.set("height", Param::Value(10.0)), "height");
    must(m.add(Feature::new("body", extrude(must(Sketch::rect(Vec3::ZERO, Vec3::X, Vec3::Y, 10.0, 10.0), "sketch"), "height"))), "body");
    must(
        m.add(Feature::new(
            "round",
            FeatureKind::Blend { edges: vec![EdgeRef::new(Vec3::new(0.0, 0.0, 0.0), 1e-6)], size: "1".into(), segments: 8, round: true },
        )),
        "round",
    );
    let built = rebuild(&mut m, None);
    assert!(!built.problems.is_empty(), "a corner reference does not build: {:?}", built.problems);
    // Nothing was built, so nothing is remembered, and a saved document says so by omission.
    let text = m.save_json();
    let doc: Document = must(serde_json::from_str(&text), "the saved document");
    assert!(doc.topology.is_empty(), "a blend that never resolved an edge contributes no fingerprint");
    let loaded = must(Model::load_json_with_report(&text), "reopen");
    assert_eq!(loaded.resolution.strategies(), vec!["unresolved"], "{}", loaded.resolution.summary());
    let message = loaded.rebuild.problems.iter().find(|p| p.feature == "round").map(|p| p.message.clone()).unwrap_or_default();
    assert!(message.contains("ambiguous"), "the kernel's own reason is kept: {message}");
}

// ---------------------------------------------------------------------------------------------
// The version story.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_file_from_before_the_topology_map_still_opens() {
    // Hand-written, the shape of a document from before the field existed: no `topology` key at
    // all, and the version that was current then. It has to open, and it has to be *useful* — a
    // blend whose edge has not moved still resolves, through its own point, and the report says
    // `fallback` rather than pretending a fingerprint was involved.
    let old = r#"{
  "magic": "CADCraft model",
  "version": 1,
  "parameters": { "height": { "Value": 10.0 } },
  "features": [
    {
      "name": "body",
      "Extrude": {
        "sketch": {
          "origin": { "x": 0.0, "y": 0.0, "z": 0.0 },
          "normal": { "x": 0.0, "y": 0.0, "z": 1.0 },
          "u": { "x": 1.0, "y": 0.0, "z": 0.0 },
          "loops": [ [ { "x": 0.0, "y": 0.0 }, { "x": 10.0, "y": 0.0 }, { "x": 10.0, "y": 10.0 }, { "x": 0.0, "y": 10.0 } ] ]
        },
        "depth": "height",
        "symmetric": false
      }
    },
    {
      "name": "round",
      "Blend": { "edges": [ { "point": [ 0.0, 0.0, 5.0 ], "tolerance": 1e-6 } ], "size": "1", "segments": 8, "round": true }
    }
  ]
}"#;
    // The literal is also read as a `Document` first, so a shape mistake in it is reported as a
    // shape mistake rather than as a mysterious rebuild failure further down.
    let doc: Document = must(serde_json::from_str(old), "the hand-written older shape");
    assert!(doc.topology.is_empty(), "an older document simply has no fingerprints");

    let loaded = must(Model::load_json_with_report(old), "reopen the older document");
    assert!(loaded.rebuild.problems.is_empty(), "it builds: {:?}", loaded.rebuild.problems);
    assert_eq!(loaded.resolution.strategies(), vec!["fallback"], "{}", loaded.resolution.summary());
    assert!(loaded.resolution.all_fallback(), "and it says so rather than claiming a fingerprint");
    assert!((volume(&loaded.model) - volume(&rebuilt())).abs() <= 1e-9, "the same geometry as the same model written today");
}

#[test]
fn the_oldest_document_there_is_still_a_document() {
    // The smallest thing that can be a file: a magic, the version, and nothing else. Every field
    // has defaulted since before this format existed.
    let minimal = r#"{"magic":"CADCraft model","version":1}"#;
    let (model, rebuild) = must(Model::load_json(minimal), "reopen the minimal document");
    assert!(model.features.is_empty());
    assert!(model.parameters.is_empty());
    assert!(rebuild.problems.is_empty(), "there is nothing in it that can fail");
    assert!(rebuild.solid.is_none());
}

#[test]
fn a_document_from_the_future_is_still_refused_with_both_numbers() {
    let model = rebuilt();
    let text = model.save_json();
    let mut future = as_json(&text);
    future["version"] = serde_json::Value::from(99);
    match Model::load_json_with_report(&future.to_string()) {
        Err(ModelError::TooNew { found, supported }) => {
            assert_eq!(found, 99);
            assert_eq!(supported, cadcraft_model::FORMAT_VERSION);
        }
        Err(other) => panic!("a document from the future is refused for being corrupt: {other}"),
        Ok(_) => panic!("a document from the future must not be read as if it were the present"),
    }
    // And the current version is of course accepted.
    assert!(Model::load_json_with_report(&text).is_ok());
}

#[test]
fn a_document_with_a_future_topology_version_still_opens() {
    // The map is optional and self-describing, so a build that does not know its `version` still
    // reads it: the fingerprints either match or they do not, and a map it cannot trust costs it
    // the fallback it has always had.
    let model = rebuilt();
    let text = model.save_json();
    let mut ahead = as_json(&text);
    ahead["topology"]["version"] = serde_json::Value::from(7);
    let loaded = must(Model::load_json_with_report(&ahead.to_string()), "reopen");
    assert!(loaded.rebuild.problems.is_empty(), "it still builds: {:?}", loaded.rebuild.problems);
    assert_eq!(loaded.resolution.strategies(), vec!["fingerprint"], "{}", loaded.resolution.summary());
}

// ---------------------------------------------------------------------------------------------
// Hostile maps.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_map_that_describes_nothing_falls_back_instead_of_guessing() {
    let model = rebuilt();
    let text = model.save_json();
    for (what, replacement) in [
        ("no entry at all", serde_json::json!({ "version": 1, "features": {} })),
        ("an entry with no fingerprints", serde_json::json!({ "version": 1, "features": { "round": { "edges": [] } } })),
        (
            "a fingerprint that describes nothing",
            serde_json::json!({
                "version": 1,
                "features": { "round": { "edges": [ {
                    "start": [ 0.0, 0.0, 0.0 ], "end": [ 0.0, 0.0, 0.0 ],
                    "face_a": [ 0.0, 0.0, 0.0 ], "face_b": [ 0.0, 0.0, 0.0 ],
                    "length_ratio": 0.0, "at": [ [ 0.0, 0.0 ], [ 0.0, 0.0 ] ],
                    "square": [ 0.0, 0.0 ], "dihedral": 0.0
                } ] } }
            }),
        ),
        ("a feature nobody has heard of", serde_json::json!({ "version": 1, "features": { "ghost": { "edges": [ { "start": [ 0.0, 0.0, 0.0 ], "end": [ 0.0, 0.0, 0.0 ], "face_a": [ 0.0, 0.0, 0.0 ], "face_b": [ 0.0, 0.0, 0.0 ], "length_ratio": 0.0, "at": [ [ 0.0, 0.0 ], [ 0.0, 0.0 ] ], "square": [ 0.0, 0.0 ], "dihedral": 0.0 } ] } } })),
    ] {
        let mut hostile = as_json(&text);
        hostile["topology"] = replacement;
        let loaded = must(Model::load_json_with_report(&hostile.to_string()), what);
        assert!(loaded.rebuild.problems.is_empty(), "{what}: the model still builds: {:?}", loaded.rebuild.problems);
        assert_eq!(loaded.resolution.strategies(), vec!["fallback"], "{what}: {}", loaded.resolution.summary());
        assert!((volume(&loaded.model) - volume(&model)).abs() <= 1e-9, "{what}: and the geometry is untouched");
    }
}

#[test]
fn a_map_with_more_fingerprints_than_references_changes_nothing() {
    // The map is positional, so surplus entries belong to nothing and are ignored rather than
    // matched against an edge somebody did not ask for.
    let model = rebuilt();
    let text = model.save_json();
    let mut surplus = as_json(&text);
    let first = must(surplus["topology"]["features"]["round"]["edges"][0].clone(), "the first fingerprint");
    surplus["topology"]["features"]["round"]["edges"] = serde_json::Value::Array(vec![first.clone(), first]);
    let loaded = must(Model::load_json_with_report(&surplus.to_string()), "reopen");
    assert_eq!(loaded.resolution.strategies(), vec!["fingerprint"], "{}", loaded.resolution.summary());
    assert!((volume(&loaded.model) - volume(&model)).abs() <= 1e-9, "the same geometry");
}

#[test]
fn a_file_that_is_not_a_document_is_still_not_a_document() {
    assert!(matches!(Model::load_json("{ not json"), Err(ModelError::CorruptHistory(_))));
    assert!(matches!(Model::load_json(r#"{"magic":"something else","version":1}"#), Err(ModelError::NotAModel)));
    // And a duplicate feature name is refused before anything is built, fingerprints or not.
    let model = rebuilt();
    let mut clash = as_json(&model.save_json());
    let body = must(clash["features"][0].clone(), "the body feature");
    clash["features"] = serde_json::Value::Array(vec![body.clone(), body]);
    assert!(matches!(Model::load_json_with_report(&clash.to_string()), Err(ModelError::DuplicateFeature(_))));
}

// ---------------------------------------------------------------------------------------------
// Not executed
// ---------------------------------------------------------------------------------------------
//
// Every test here is `NOT EXECUTED`: written by an agent with no terminal, never compiled, never
// run. `cargo test -p cadcraft-model --test format` is what turns them into evidence.
//
// One shape here is a bet that could not be checked without running it: the hand-written literal
// in `a_file_from_before_the_topology_map_still_opens` assumes `#[serde(flatten)]` on a feature's
// externally tagged kind serialises as `{"name": ..., "Extrude": {...}}`. If that assumption is
// wrong the test fails at its first line with a serde error naming the field, which is exactly the
// information needed to correct it — and the other tests in this file, which build their documents
// in code, do not depend on it.