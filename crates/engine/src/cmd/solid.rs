//! The 3D parametric model as engine commands: its parameters, its feature tree, and the files a
//! model lives in.
//!
//! JSON only, deliberately. The 2D commands prompt through `Vec2` machines and the two sweeps
//! that drive every registered command assume that; a 3D prompt machine would be a second prompt
//! system for no gain, since whoever builds a solid has a script, an agent or the UI driving it.
//! Everything here is reachable the same three ways — `engine.execute`, the command line and the
//! control channel — and all of them get this code.
//!
//! Two rules, both from the model layer and both load-bearing:
//!
//! - **A failure never destroys what was there.** A command that adds a feature which does not
//!   build reports the model's own message as an error and *leaves the feature in the tree*, with
//!   the last solid that worked still under it. A person fixing a model needs the broken feature
//!   and the error beside it, not a silent rollback to something that built.
//! - **Nothing is silent.** A feature that did not build is an error, never a success carrying a
//!   volume as if the edit had landed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cadcraft_geom::{Vec2, Vec3};
use cadcraft_model::{Dep, EdgeRef, Feature, FeatureKind, Model, Param, Problem, Record, Sketch, State, evaluate, parse, rebuild, rebuild_after};
use cadcraft_solid::{
    Solid,
    limits::{MAX_BLENDS, MAX_ROUND_SEGMENTS},
    mesh_from_solid, stl_ascii, stl_binary,
};
use serde_json::{Value, json};

use super::*;
use crate::{EngineError, Result, Session};

/// Loops one profile may have. The kernel caps the vertices per loop; this caps how many holes a
/// caller may hand it, checked on the JSON before anything is built.
const MAX_SKETCH_LOOPS: usize = 64;
/// Points one profile may have, counted across its loops.
const MAX_SKETCH_POINTS: usize = 20_000;
/// How close to the reference point an edge reference still counts, when `tolerance` is not given.
const DEFAULT_EDGE_TOLERANCE: f64 = 1e-6;
/// Segments of a round when `segments` is not given.
const DEFAULT_ROUND_SEGMENTS: usize = 8;
/// Segments of a revolve when `segments` is not given.
const DEFAULT_REVOLVE_SEGMENTS: usize = 32;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("solid.body.new", "New Solid Model", run_new).enabled(always).noundo().params("{} → {parameters, features}"),
        CommandSpec::new("solid.param.set", "Set Parameter", run_param_set).enabled(always).noundo().params(
            "{name, value} | {name, formula} → {name, value, formula?, parameters, volume, rebuilt}; a model that stops building is an error",
        ),
        CommandSpec::new("solid.param.list", "List Parameters", run_param_list)
            .enabled(always)
            .noundo()
            .params("{} → {parameters: [{name, value, formula?, error?}], count}"),
        CommandSpec::new("solid.feature.extrude", "Extrude", run_extrude).enabled(always).noundo().params(
            "{name?, depth, symmetric?, sketch: {origin?, normal?, u?, loops}} | {name?, w, h, at?, u?, v?, depth} → {name, volume, faces, rebuilt}",
        ),
        CommandSpec::new("solid.feature.revolve", "Revolve", run_revolve)
            .enabled(always)
            .noundo()
            .params("{name?, sketch, axis: [x,y,z], angle, segments?} → {name, volume, faces, rebuilt}"),
        CommandSpec::new("solid.feature.cut", "Cut", run_cut).enabled(always).noundo().params("{name?, tool} → {name, volume, faces, rebuilt}"),
        CommandSpec::new("solid.feature.fillet", "Fillet", run_fillet)
            .enabled(always)
            .noundo()
            .params("{name?, edges: [{point: [x,y,z], tolerance?}], size, segments?} → {name, volume, faces, rebuilt}"),
        CommandSpec::new("solid.feature.chamfer", "Chamfer", run_chamfer)
            .enabled(always)
            .noundo()
            .params("{name?, edges: [{point: [x,y,z], tolerance?}], size} → {name, volume, faces, rebuilt}"),
        CommandSpec::new("solid.feature.delete", "Delete", run_feature_delete)
            .enabled(always)
            .noundo()
            .params("{name} → {removed: [...], features, parameters}"),
        CommandSpec::new("solid.info", "Model Info", run_info)
            .enabled(always)
            .noundo()
            .params("{} → {features: [{name, kind, state, message, volume, faces}], volume, faces, hasSolid, problems}"),
        CommandSpec::new("solid.save", "Save", run_save).enabled(always).noundo().params("{path} → {path, bytes, features, parameters}"),
        CommandSpec::new("solid.open", "Open", run_open)
            .enabled(always)
            .noundo()
            .params("{path} → {path, features, parameters, volume, faces, hasSolid, problems}"),
        CommandSpec::new("solid.export.stl", "Export", run_export_stl)
            .enabled(always)
            .noundo()
            .params("{path, binary?} → {path, bytes, triangles, binary, volume}"),
    ]
}

// ---------------- the model on the session ----------------

fn run_new(s: &mut Session, _p: &Value) -> Result<Value> {
    s.solid_model = Model::new();
    s.echo("The 3D model is empty.");
    Ok(json!({ "parameters": 0, "features": 0 }))
}

/// Set a parameter, then rebuild what reads it.
///
/// The rebuild is the validation: a parameter that parses but drives a feature into a corner is
/// only wrong once something reads it, and waiting for a later command to notice would make the
/// error appear somewhere the edit was not.
fn run_param_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.param.set";
    let name = name_param(cmd, p)?;
    let param = match (p.get("value"), p.get("formula")) {
        (Some(v), _) => {
            Param::Value(v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| bad(cmd, "`value` must be a number (use `formula` for an expression)"))?)
        }
        (None, Some(Value::String(f))) => Param::Formula(f.clone()),
        _ => return Err(bad(cmd, "`value` (a number) or `formula` (an expression) is required")),
    };
    let model = &mut s.solid_model;
    // `set` parses the formula, so a typo is named here rather than by whatever reads it next.
    model.set(&name, param).map_err(|e| bad(cmd, e.to_string()))?;
    let result = rebuild_after(model, &[Dep::Parameter(name.clone())]);
    let values = parameter_values(model);
    let volume = volume_of(model);
    let parameters = model.parameters.len();
    let formula = model.parameters.get(&name).filter(|p| matches!(p, Param::Formula(_))).map(|p| match p {
        Param::Formula(text) => text.clone(),
        Param::Value(_) => String::new(),
    });
    if !result.problems.is_empty() {
        return Err(EngineError::Other(format!("the 3D model did not build: {}", problem_message(&result.problems))));
    }
    let value = match values.get(&name) {
        Some(Ok(v)) => *v,
        Some(Err(why)) => return Err(bad(cmd, why.clone())),
        None => return Err(bad(cmd, format!("{name} was not defined"))),
    };
    let rebuilt = result.rebuilt;
    s.echo(format!("{name} = {value}"));
    Ok(json!({ "name": name, "value": value, "formula": formula, "parameters": parameters, "volume": volume, "rebuilt": rebuilt }))
}

fn run_param_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let model = &s.solid_model;
    let values = parameter_values(model);
    let parameters: Vec<Value> = model
        .parameters
        .iter()
        .map(|(name, param)| {
            let formula = match param {
                Param::Formula(text) => Some(text.clone()),
                Param::Value(_) => None,
            };
            let mut row = json!({ "name": name, "formula": formula });
            match values.get(name) {
                Some(Ok(v)) => row["value"] = json!(v),
                // One parameter with no value does not hide the others: `value` reads `error`.
                _ => {
                    row["value"] = Value::Null;
                    row["error"] = json!(values.get(name).and_then(|r| r.clone().err()).unwrap_or_default());
                }
            }
            row
        })
        .collect();
    Ok(json!({ "parameters": parameters, "count": parameters.len() }))
}

/// Every parameter's value, or the reason one of them has none.
///
/// [`Model::values`] is all or nothing — one dimension that does not compute takes the whole table
/// with it — and a person editing a model needs the values that *do* compute beside the reason the
/// one that does not. This is the model's own arithmetic, one parameter at a time and in the order
/// the table stores them, with the failure kept where it happened instead of propagated.
fn parameter_values(model: &Model) -> BTreeMap<String, std::result::Result<f64, String>> {
    let mut known: BTreeMap<String, f64> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (name, param) in &model.parameters {
        let value = match param {
            Param::Value(v) => Ok(*v),
            Param::Formula(text) => parse(text).and_then(|e| evaluate(&e, &known)).map_err(|e| format!("{name}: {e}")),
        };
        if let Ok(v) = &value {
            known.insert(name.clone(), *v);
        }
        out.insert(name.clone(), value);
    }
    out
}

// ---------------- features ----------------

fn run_extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.feature.extrude";
    let sketch = match p.get("sketch") {
        Some(v) => sketch_value(cmd, v)?,
        // The convenience form: a box is `w`, `h` and a corner, and making a person spell out a
        // sketch to say that would be the command arguing with them.
        None if p.get("w").is_some() || p.get("h").is_some() => sketch_value(cmd, p)?,
        None => return Err(bad(cmd, "`sketch` is required (a rectangle may be given as `w` and `h`)")),
    };
    let depth = formula_param(p, "depth").ok_or_else(|| bad(cmd, "`depth` (a number, or the name of a parameter) is required"))?;
    let symmetric = bool_or(p, "symmetric", false);
    let name = free_name(&s.solid_model, &name_or(p, "body"));
    add_feature(cmd, s, &name, FeatureKind::Extrude { sketch, depth, symmetric })
}

fn run_revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.feature.revolve";
    let sketch = sketch_param(cmd, p, "sketch")?;
    let axis = p.get("axis").and_then(point3).ok_or_else(|| bad(cmd, "`axis` ([x,y,z]) is required and must lie in the sketch's plane"))?;
    let angle = formula_param(p, "angle").ok_or_else(|| bad(cmd, "`angle` (degrees, a number or an expression) is required"))?;
    let segments = segments_param(cmd, p, DEFAULT_REVOLVE_SEGMENTS, 3, 256)?;
    let name = free_name(&s.solid_model, &name_or(p, "ring"));
    add_feature(cmd, s, &name, FeatureKind::Revolve { sketch, axis, angle, segments })
}

fn run_cut(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.feature.cut";
    let tool = str_param(p, "tool")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| bad(cmd, "`tool` (the name of the feature whose solid is removed) is required"))?
        .to_string();
    let model = &mut s.solid_model;
    // Refused before the feature exists: naming a tool that is not there is a typo, and a cut in
    // the tree with no tool behind it is a model nobody can read.
    if model.feature(&tool).is_none() {
        return Err(bad(cmd, format!("`tool`: there is no feature called {tool:?}")));
    }
    let name = free_name(model, &name_or(p, "cut"));
    let kind = FeatureKind::Cut { tool };
    add_feature(cmd, s, &name, kind)
}

fn run_fillet(s: &mut Session, p: &Value) -> Result<Value> {
    blend(s, p, true)
}

fn run_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    blend(s, p, false)
}

fn blend(s: &mut Session, p: &Value, round: bool) -> Result<Value> {
    let cmd = if round { "solid.feature.fillet" } else { "solid.feature.chamfer" };
    let edges = edges_param(cmd, p)?;
    let size = formula_param(p, "size").ok_or_else(|| bad(cmd, "`size` (a number, or the name of a parameter) is required"))?;
    let segments = segments_param(cmd, p, DEFAULT_ROUND_SEGMENTS, 1, MAX_ROUND_SEGMENTS)?;
    let stem = if round { "fillet" } else { "chamfer" };
    let name = free_name(&s.solid_model, &name_or(p, stem));
    add_feature(cmd, s, &name, FeatureKind::Blend { edges, size, segments, round })
}

fn run_feature_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.feature.delete";
    let name = name_param(cmd, p)?;
    let model = &mut s.solid_model;
    let removed = model.remove(&name).map_err(|e| bad(cmd, e.to_string()))?;
    // No rebuild: what was removed took everything built on it with it, and nothing left below can
    // have read it — a feature only ever reads the one before it and the tool a cut names.
    let features = model.features.len();
    let parameters = model.parameters.len();
    s.echo(format!("Removed {}.", removed.join(", ")));
    Ok(json!({ "removed": removed, "features": features, "parameters": parameters }))
}

// ---------------- reporting ----------------

fn run_info(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(info_json(&s.solid_model))
}

fn info_json(model: &Model) -> Value {
    let features: Vec<Value> = model
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            // A record missing for a feature is a model mid-edit, and "not built" is the truth
            // about it rather than a panic on an index.
            let record = model.records.get(i).cloned().unwrap_or_else(Record::pending);
            let solid = record.last_good.as_ref();
            json!({
                "name": f.name,
                "kind": f.kind.kind_name(),
                "suppressed": f.suppressed,
                "state": state_name(&record.state),
                "message": state_message(&record.state),
                "volume": solid.map(Solid::volume),
                "faces": solid.map(|s| s.faces.len()),
            })
        })
        .collect();
    let problems: Vec<Value> = model
        .features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            model.records.get(i).and_then(|r| match &r.state {
                State::Built => None,
                state => Some(json!({ "feature": f.name, "state": state_name(state), "message": state_message(state) })),
            })
        })
        .collect();
    let solid = model.solid();
    json!({
        "features": features,
        "volume": solid.map(Solid::volume),
        "faces": solid.map(|s| s.faces.len()),
        "hasSolid": solid.is_some(),
        "problems": problems,
    })
}

// ---------------- files ----------------

fn run_save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.save";
    let path = path_param(cmd, p)?;
    let model = &s.solid_model;
    let text = model.save_json();
    let features = model.features.len();
    let parameters = model.parameters.len();
    write_file(cmd, &path, text.as_bytes())?;
    s.echo(format!("Saved the 3D model to {path}."));
    Ok(json!({ "path": path, "bytes": text.len(), "features": features, "parameters": parameters }))
}

/// Open a model document and rebuild it.
///
/// A document whose features do not build still opens: the tree, the parameters and the errors are
/// exactly what the person who owns it needs. Refusing to open a file because one dimension is
/// wrong would throw those away.
fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.open";
    let path = path_param(cmd, p)?;
    let text = read_file(cmd, &path)?;
    let (model, result) = Model::load_json(&text).map_err(|e| bad(cmd, e.to_string()))?;
    let problems: Vec<Value> = result.problems.iter().map(problem_json).collect();
    let volume = model.solid().map(Solid::volume);
    let faces = model.solid().map(|s| s.faces.len());
    let has_solid = model.solid().is_some();
    let features = model.features.len();
    let parameters = model.parameters.len();
    s.solid_model = model;
    if problems.is_empty() {
        s.echo(format!("Opened {path}."));
    } else {
        s.echo(format!("Opened {path}: {}", problem_message(&result.problems)));
    }
    Ok(
        json!({ "path": path, "features": features, "parameters": parameters, "volume": volume, "faces": faces, "hasSolid": has_solid, "problems": problems }),
    )
}

fn run_export_stl(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.export.stl";
    let path = path_param(cmd, p)?;
    let Some(solid) = s.solid_model.solid().cloned() else {
        return Err(bad(cmd, "the 3D model has no solid to export"));
    };
    let mesh = mesh_from_solid(&solid).map_err(|e| bad(cmd, e.to_string()))?;
    let binary = bool_or(p, "binary", false);
    let bytes = if binary { stl_binary(&mesh) } else { stl_ascii(&mesh).into_bytes() };
    write_file(cmd, &path, &bytes)?;
    let triangles = mesh.tris.len();
    s.echo(format!("Exported {triangles} triangles to {path}."));
    Ok(json!({ "path": path, "bytes": bytes.len(), "triangles": triangles, "binary": binary, "volume": solid.volume() }))
}

// ---------------- adding a feature ----------------

fn add_feature(cmd: &str, s: &mut Session, name: &str, kind: FeatureKind) -> Result<Value> {
    let model = &mut s.solid_model;
    model.add(Feature::new(name, kind)).map_err(|e| bad(cmd, e.to_string()))?;
    let result = rebuild(model, None);
    if !result.problems.is_empty() {
        // The feature stays in the tree. A person fixing a model needs the broken feature and the
        // error beside it; taking it out again would leave nothing to point at.
        return Err(EngineError::Other(format!("the 3D model did not build: {}", problem_message(&result.problems))));
    }
    let volume = volume_of(model);
    let faces = model.solid().map(|x| x.faces.len()).unwrap_or_default();
    let rebuilt = result.rebuilt;
    s.echo(format!("{name}: volume {volume}."));
    Ok(json!({ "name": name, "volume": volume, "faces": faces, "rebuilt": rebuilt }))
}

fn volume_of(model: &Model) -> f64 {
    model.solid().map(Solid::volume).unwrap_or_default()
}

// ---------------- reading the model's own types ----------------

fn state_name(state: &State) -> &'static str {
    match state {
        State::Built => "Built",
        State::Pending => "Pending",
        State::Failed(_) => "Failed",
        State::Blocked(_) => "Blocked",
    }
}

fn state_message(state: &State) -> Option<String> {
    match state {
        State::Built => None,
        State::Pending => Some("not built yet".into()),
        State::Failed(m) | State::Blocked(m) => Some(m.clone()),
    }
}

fn problem_json(p: &Problem) -> Value {
    json!({ "feature": p.feature, "state": state_name(&p.kind), "message": p.message })
}

fn problem_message(problems: &[Problem]) -> String {
    problems.iter().map(|p| format!("{}: {}", p.feature, p.message)).collect::<Vec<_>>().join("; ")
}

// ---------------- reading the JSON ----------------

fn name_param(cmd: &str, p: &Value) -> Result<String> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?;
    Ok(name.to_string())
}

/// `name` as given, or the stem a new feature gets when the caller leaves it out.
fn name_or(p: &Value, stem: &str) -> String {
    str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| stem.to_string())
}

/// The name a new feature gets when none is given.
///
/// Nothing in this file edits a feature in place, so an unnamed second extrude cannot be `body`
/// again: the tree needs names that stay distinct, and `body2` is what a person would have called
/// it anyway.
fn free_name(model: &Model, stem: &str) -> String {
    if model.feature(stem).is_none() {
        return stem.to_string();
    }
    (2..=model.features.len() + 2)
        .map(|i| format!("{stem}{i}"))
        .find(|n| model.feature(n).is_none())
        .unwrap_or_else(|| format!("{stem}{}", model.features.len() + 2))
}

/// A dimension as the model stores it: the text of an expression, or a number written out. Both
/// are what a feature carries, and the kernel reads them the same way.
fn formula_param(p: &Value, key: &str) -> Option<String> {
    match p.get(key) {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(|f| f.to_string()),
        None => None,
    }
}

fn segments_param(cmd: &str, p: &Value, default: usize, min: usize, max: usize) -> Result<usize> {
    match p.get("segments") {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = usize::try_from(v.as_u64().ok_or_else(|| bad(cmd, "`segments` must be a whole number"))?).unwrap_or(usize::MAX);
            (min..=max).contains(&n).then_some(n).ok_or_else(|| bad(cmd, format!("`segments` must be {min}..{max}")))
        }
    }
}

fn path_param(cmd: &str, p: &Value) -> Result<String> {
    let path = str_param(p, "path").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    Ok(path.to_string())
}

fn point3(v: &Value) -> Option<Vec3> {
    if let Some(a) = v.as_array() {
        let x = a.first()?.as_f64()?;
        let y = a.get(1)?.as_f64()?;
        let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
        let p = Vec3::new(x, y, z);
        return p.is_finite().then_some(p);
    }
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    let z = v.get("z").and_then(Value::as_f64).unwrap_or(0.0);
    let p = Vec3::new(x, y, z);
    p.is_finite().then_some(p)
}

fn point2(v: &Value) -> Option<Vec2> {
    if let Some(a) = v.as_array() {
        let x = a.first()?.as_f64()?;
        let y = a.get(1)?.as_f64()?;
        let p = Vec2::new(x, y);
        return p.is_finite().then_some(p);
    }
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    let p = Vec2::new(x, y);
    p.is_finite().then_some(p)
}

/// A sketch, in either of the two forms a caller has one of.
///
/// `loops` is the stored form: a profile already in the plane's own frame, which is what the model
/// file holds. `w` and `h` are the drawn form: a rectangle from a corner along two directions,
/// which is what somebody describing a box actually has. The frame is given rather than inferred
/// so a sketch means the same thing here as it did when it was written down.
fn sketch_value(cmd: &str, v: &Value) -> Result<Sketch> {
    let Some(obj) = v.as_object() else {
        return Err(bad(cmd, "a sketch is an object: {origin, normal, u, loops} or {at, u, v, w, h}"));
    };
    if obj.contains_key("loops") {
        return stored_sketch(cmd, v);
    }
    let w = obj.get("w").and_then(Value::as_f64).filter(|f| f.is_finite());
    let h = obj.get("h").and_then(Value::as_f64).filter(|f| f.is_finite());
    if let (Some(w), Some(h)) = (w, h) {
        let at = obj.get("at").or_else(|| obj.get("origin")).and_then(point3).unwrap_or(Vec3::ZERO);
        let u = obj.get("u").and_then(point3).unwrap_or(Vec3::X);
        let v_axis = obj.get("v").and_then(point3).unwrap_or(Vec3::Y);
        return Sketch::rect(at, u, v_axis, w, h).map_err(|e| bad(cmd, e.to_string()));
    }
    Err(bad(cmd, "a sketch needs `loops`, or `w` and `h` for a rectangle"))
}

fn stored_sketch(cmd: &str, v: &Value) -> Result<Sketch> {
    let loops = v.get("loops").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`loops` must be an array of loops"))?;
    // Counted on the JSON, before any of it is read into a sketch: a hostile profile has to be
    // refused on its length, not after it has been allocated.
    if loops.len() > MAX_SKETCH_LOOPS {
        return Err(bad(cmd, format!("a sketch may not have more than {MAX_SKETCH_LOOPS} loops")));
    }
    let mut points = 0usize;
    let mut read: Vec<Vec<Vec2>> = Vec::with_capacity(loops.len());
    for loop_value in loops {
        let items = loop_value.as_array().ok_or_else(|| bad(cmd, "each loop is an array of points"))?;
        points = points.saturating_add(items.len());
        if points > MAX_SKETCH_POINTS {
            return Err(bad(cmd, format!("a sketch may not have more than {MAX_SKETCH_POINTS} points")));
        }
        let mut one = Vec::with_capacity(items.len());
        for item in items {
            one.push(point2(item).ok_or_else(|| bad(cmd, "each point is [x, y]"))?);
        }
        read.push(one);
    }
    let origin = v.get("origin").and_then(point3).unwrap_or(Vec3::ZERO);
    let normal = v.get("normal").and_then(point3).unwrap_or(Vec3::Z);
    let u = v.get("u").and_then(point3).unwrap_or(Vec3::X);
    // The kernel checks the frame when it builds the plane; a degenerate one here would be an
    // error about a number rather than about the sketch, so it is refused with the sketch's words.
    cadcraft_solid::Plane::with_u(origin, normal, u).map_err(|e| bad(cmd, format!("the sketch plane cannot be built: {e}")))?;
    Ok(Sketch { origin, normal, u, loops: read })
}

fn sketch_param(cmd: &str, p: &Value, key: &str) -> Result<Sketch> {
    let v = p.get(key).ok_or_else(|| bad(cmd, format!("`{key}` is required")))?;
    sketch_value(cmd, v)
}

/// The edges of a blend, each a place rather than an index.
///
/// There is no topological naming scheme yet, so a reference stops resolving the moment the model
/// moves it — an error naming the edge that is gone, never a reference that quietly lands on a
/// different one. That is the model's rule, and this is only how one is written down.
fn edges_param(cmd: &str, p: &Value) -> Result<Vec<EdgeRef>> {
    let list = p.get("edges").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`edges` is an array of {point, tolerance}"))?;
    if list.is_empty() {
        return Err(bad(cmd, "`edges` is empty: there is nothing to blend"));
    }
    if list.len() > MAX_BLENDS {
        return Err(bad(cmd, format!("a blend may not touch more than {MAX_BLENDS} edges")));
    }
    list.iter()
        .map(|v| {
            let point = v.get("point").and_then(point3).ok_or_else(|| bad(cmd, "each edge is {point: [x,y,z], tolerance}"))?;
            let tolerance = v.get("tolerance").and_then(Value::as_f64).filter(|t| t.is_finite() && *t > 0.0).unwrap_or(DEFAULT_EDGE_TOLERANCE);
            Ok(EdgeRef::new(point, tolerance))
        })
        .collect()
}

// ---------------- files on disk ----------------

/// Write through a temporary file and rename.
///
/// A model is somebody's work. Half a file where a whole one was is worse than no file, and the
/// rename is what makes the write atomic as far as anything else on the machine is concerned.
fn write_file(cmd: &str, path: &str, bytes: &[u8]) -> Result<()> {
    let target = Path::new(path);
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| bad(cmd, format!("{}: {e}", dir.display())))?;
    }
    let tmp = tmp_path(target);
    std::fs::write(&tmp, bytes).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(bad(cmd, format!("{path}: {e}")))
        }
    }
}

fn tmp_path(target: &Path) -> PathBuf {
    let mut tmp = target.to_path_buf();
    tmp.as_mut_os_string().push(".cadcraft-tmp");
    tmp
}

/// Read a model document, refusing one too large to be a model before it is read.
///
/// The size is checked from the file's own metadata so a hostile path costs a `stat` and not an
/// allocation; the ceiling is the one the 2D support files already use.
fn read_file(cmd: &str, path: &str) -> Result<String> {
    let limit = super::helpers::MAX_SUPPORT_FILE_BYTES;
    let len = std::fs::metadata(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?.len();
    if len > limit {
        return Err(bad(cmd, format!("{path}: larger than {} MB", limit >> 20)));
    }
    let bytes = std::fs::read(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    String::from_utf8(bytes).map_err(|e| bad(cmd, format!("{path} is not UTF-8 text: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a command, panicking with its message if it fails — the message *is* the test.
    fn run(s: &mut Session, id: &str, p: &Value) -> Value {
        match s.execute(id, p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn err(s: &mut Session, id: &str, p: &Value) -> String {
        match s.execute(id, p) {
            Ok(v) => panic!("{id} {p} should have been refused, but gave {v}"),
            Err(e) => e.to_string(),
        }
    }

    fn f(v: &Value, key: &str) -> f64 {
        v.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
    }

    fn i(v: &Value, key: &str) -> i64 {
        v.get(key).and_then(Value::as_i64).unwrap_or(-1)
    }

    /// A path in the temp directory, unique to this test so a parallel run cannot collide with it.
    fn temp_path(stem: &str, ext: &str) -> String {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        std::env::temp_dir().join(format!("cadcraft-{stem}-{}-{n}.{ext}", std::process::id())).to_string_lossy().into_owned()
    }

    /// A 4 × 3 box driven by a `height` parameter.
    fn box_session() -> Session {
        let mut s = Session::new();
        run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 2 }));
        run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "w": 4, "h": 3, "depth": "height" }));
        s
    }

    #[test]
    fn a_parameter_is_set_read_and_computed_from_another() {
        let mut s = Session::new();
        let r = run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 2.0 }));
        assert_eq!(r["name"], json!("height"));
        assert_eq!(f(&r, "value"), 2.0, "{r}");

        // A formula that cannot resolve is refused where it was typed, not by whatever reads it.
        let m = err(&mut s, "solid.param.set", &json!({ "name": "bad", "formula": "height +*" }));
        assert!(!m.is_empty(), "{m}");

        run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "w": 4, "h": 3, "depth": "height" }));
        let r = run(&mut s, "solid.param.set", &json!({ "name": "twice", "formula": "height * 2" }));
        assert_eq!(f(&r, "value"), 4.0, "{r}");

        let l = run(&mut s, "solid.param.list", &json!({}));
        assert_eq!(i(&l, "count"), 2, "{l}");
        let rows = l["parameters"].as_array().map(Vec::len).unwrap_or_default();
        assert_eq!(rows, 2, "{l}");
        let twice = l["parameters"].as_array().and_then(|a| a.iter().find(|r| r["name"] == json!("twice"))).cloned().unwrap_or(Value::Null);
        assert_eq!(twice["formula"], json!("height * 2"), "the text as written: {twice}");
        assert_eq!(twice["value"], json!(4.0), "{twice}");
        assert!(twice.get("error").is_none(), "{twice}");
    }

    #[test]
    fn a_parameter_that_reads_nothing_is_listed_with_its_reason() {
        let mut s = Session::new();
        run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 2.0 }));
        // Nothing reads it yet, so nothing is wrong with it — the model only complains where the
        // failure is, which is what a person editing a dimension needs.
        s.solid_model.set("broken", Param::Formula("nope * 2".into())).ok();
        let l = run(&mut s, "solid.param.list", &json!({}));
        let broken = l["parameters"].as_array().and_then(|a| a.iter().find(|r| r["name"] == json!("broken"))).cloned().unwrap_or(Value::Null);
        assert_eq!(broken["value"], Value::Null, "{broken}");
        assert!(broken["error"].as_str().unwrap_or_default().contains("nope"), "{broken}");
        let height = l["parameters"].as_array().and_then(|a| a.iter().find(|r| r["name"] == json!("height"))).cloned().unwrap_or(Value::Null);
        assert_eq!(height["value"], json!(2.0), "the others still read: {l}");
    }

    #[test]
    fn an_extrude_is_a_solid_that_info_reports() {
        let mut s = box_session();
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(info["volume"], json!(24.0), "4 x 3 x 2 is 24: {info}");
        assert_eq!(info["faces"], json!(6), "{info}");
        assert_eq!(info["hasSolid"], json!(true), "{info}");
        assert_eq!(info["problems"].as_array().map(Vec::len), Some(0), "{info}");
        let body = info["features"].as_array().and_then(|a| a.first()).cloned().unwrap_or(Value::Null);
        assert_eq!(body["name"], json!("body"), "{body}");
        assert_eq!(body["kind"], json!("extrude"), "{body}");
        assert_eq!(body["state"], json!("Built"), "{body}");
        assert_eq!(body["message"], Value::Null, "{body}");
        assert_eq!(body["faces"], json!(6), "{body}");
    }

    #[test]
    fn a_cut_takes_material_away() {
        let mut s = box_session();
        run(&mut s, "solid.feature.extrude", &json!({ "name": "tool", "w": 1, "h": 1, "at": [1, 1, -1], "depth": 4 }));
        let r = run(&mut s, "solid.feature.cut", &json!({ "name": "bore", "tool": "tool" }));
        let cut = f(&r, "volume");
        assert!((cut - 22.0).abs() < 1e-9, "a 1 x 1 shaft through a 4 x 3 x 2 block is 22: {cut}");
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(f(&info, "volume"), cut, "{info}");
        assert_eq!(info["features"][2]["state"], json!("Built"), "{info}");
        assert_eq!(info["features"][2]["kind"], json!("cut"), "{info}");
    }

    #[test]
    fn changing_a_parameter_recomputes_the_model() {
        let mut s = box_session();
        assert_eq!(f(&run(&mut s, "solid.info", &json!({})), "volume"), 24.0);
        let r = run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 5.0 }));
        assert_eq!(f(&r, "volume"), 60.0, "4 x 3 x 5 is 60: {r}");
        assert_eq!(i(&r, "rebuilt"), 1, "only what reads it: {r}");
        assert_eq!(f(&run(&mut s, "solid.info", &json!({})), "volume"), 60.0);
    }

    #[test]
    fn a_broken_parameter_is_an_error_and_keeps_the_last_good_solid() {
        let mut s = box_session();
        let m = err(&mut s, "solid.param.set", &json!({ "name": "height", "value": 0 }));
        assert!(m.contains("height"), "the model's own words: {m}");
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(f(&info, "volume"), 24.0, "the box that worked is still there: {info}");
        assert_eq!(info["features"][0]["state"], json!("Failed"), "{info}");
        assert!(info["features"][0]["message"].as_str().unwrap_or_default().contains("positive distance"), "{info}");

        // And fixing the dimension needs no further ceremony.
        run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 3.0 }));
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(info["features"][0]["state"], json!("Built"), "{info}");
        assert_eq!(f(&info, "volume"), 36.0, "{info}");
    }

    #[test]
    fn a_feature_that_does_not_build_is_reported_and_stays_in_the_tree() {
        let mut s = box_session();
        // A cut of a tool that does not exist is a typo, refused before anything is added.
        assert!(err(&mut s, "solid.feature.cut", &json!({ "tool": "nothing" })).contains("nothing"));
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(info["features"].as_array().map(Vec::len), Some(1), "the refused cut left nothing behind: {info}");
        assert_eq!(s.solid_model.features.len(), 1);
    }

    #[test]
    fn a_new_model_is_empty() {
        let mut s = box_session();
        let r = run(&mut s, "solid.body.new", &json!({}));
        assert_eq!(i(&r, "features"), 0, "{r}");
        assert_eq!(i(&r, "parameters"), 0, "{r}");
        let info = run(&mut s, "solid.info", &json!({}));
        assert_eq!(info["hasSolid"], json!(false), "{info}");
    }

    #[test]
    fn deleting_a_feature_takes_what_was_built_on_it() {
        let mut s = box_session();
        let r = run(&mut s, "solid.feature.delete", &json!({ "name": "body" }));
        assert_eq!(r["removed"], json!(["body"]), "{r}");
        assert!(err(&mut s, "solid.feature.delete", &json!({ "name": "body" })).contains("body"));
    }

    #[test]
    fn a_saved_model_reopens_as_the_same_geometry() {
        let mut s = box_session();
        run(&mut s, "solid.param.set", &json!({ "name": "twice", "formula": "height * 2" }));
        let made = f(&run(&mut s, "solid.info", &json!({})), "volume");
        let path = temp_path("model", "ccm");
        let r = run(&mut s, "solid.save", &json!({ "path": path }));
        assert_eq!(r["path"], json!(path), "{r}");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("CADCraft model"), "the file says what it is");

        let mut s2 = Session::new();
        let opened = run(&mut s2, "solid.open", &json!({ "path": path }));
        assert_eq!(opened["problems"].as_array().map(Vec::len), Some(0), "{opened}");
        let info = run(&mut s2, "solid.info", &json!({}));
        assert_eq!(f(&info, "volume"), made, "the same volume after a round trip: {info}");
        assert_eq!(i(&opened, "features"), 1, "{opened}");
        assert_eq!(i(&opened, "parameters"), 2, "{opened}");
        assert!(std::fs::remove_file(&path).is_ok());
    }

    #[test]
    fn a_document_whose_features_do_not_build_still_opens() {
        let mut s = box_session();
        let path = temp_path("broken", "ccm");
        // The edit below is an error, so the tree is left with a broken feature: the file is
        // exactly the state a person needs to fix, and it has to open.
        let _ = s.execute("solid.param.set", &json!({ "name": "height", "value": 0.0 }));
        run(&mut s, "solid.save", &json!({ "path": path }));
        let mut s2 = Session::new();
        let opened = run(&mut s2, "solid.open", &json!({ "path": path }));
        assert_eq!(opened["features"], json!(1), "the history came back: {opened}");
        assert_eq!(opened["hasSolid"], json!(false), "with nothing pretending to be built: {opened}");
        let problems = opened["problems"].as_array().cloned().unwrap_or_default();
        assert_eq!(problems.len(), 1, "{opened}");
        assert_eq!(problems[0]["feature"], json!("body"), "{opened}");
        assert_eq!(problems[0]["state"], json!("Failed"), "{opened}");
        assert!(problems[0]["message"].as_str().unwrap_or_default().contains("height"), "{opened}");
        assert!(std::fs::remove_file(&path).is_ok());
    }

    #[test]
    fn a_file_that_is_not_a_model_is_refused() {
        let mut s = Session::new();
        let path = temp_path("junk", "ccm");
        std::fs::write(&path, "{ not a model").unwrap_or_default();
        assert!(err(&mut s, "solid.open", &json!({ "path": path })).contains("corrupt"));
        assert!(err(&mut s, "solid.open", &json!({ "path": "/nonexistent/nope.ccm" })).contains("nope.ccm"));
        assert!(std::fs::remove_file(&path).is_ok());
    }

    #[test]
    fn an_stl_export_writes_the_bytes_it_says_it_did() {
        let mut s = box_session();
        let ascii = temp_path("box", "stl");
        let r = run(&mut s, "solid.export.stl", &json!({ "path": ascii }));
        let text = std::fs::read_to_string(&ascii).unwrap_or_default();
        assert!(text.starts_with("solid"), "{text:.40}");
        assert!(text.ends_with("endsolid cadcraft\n"), "{}", &text[text.len().saturating_sub(20)..]);
        assert_eq!(i(&r, "bytes"), text.len() as i64, "{r}");
        assert_eq!(r["binary"], json!(false), "{r}");
        assert_eq!(i(&r, "triangles"), 12, "two triangles per face of a box: {r}");

        let binary = temp_path("box-bin", "stl");
        let r = run(&mut s, "solid.export.stl", &json!({ "path": binary, "binary": true }));
        let bytes = std::fs::read(&binary).unwrap_or_default();
        let n = bytes.len();
        assert_eq!(n, 84 + 50 * 12, "{n} bytes for 12 triangles");
        assert_eq!(r["bytes"], json!(n as i64), "{r}");
        assert_eq!(r["binary"], json!(true), "{r}");
        // The count the file carries is the one that says how long it is.
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap_or([0; 4])) as usize;
        assert_eq!(bytes.len(), 84 + 50 * count, "the header and the body agree");
        assert!(std::fs::remove_file(&ascii).is_ok());
        assert!(std::fs::remove_file(&binary).is_ok());
    }

    #[test]
    fn exporting_without_a_solid_is_refused() {
        let mut s = Session::new();
        let m = err(&mut s, "solid.export.stl", &json!({ "path": temp_path("none", "stl") }));
        assert!(m.contains("no solid"), "{m}");
    }

    #[test]
    fn a_fillet_and_a_chamfer_round_and_bevel_an_edge() {
        let mut s = Session::new();
        run(&mut s, "solid.param.set", &json!({ "name": "height", "value": 10.0 }));
        run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "w": 10, "h": 10, "depth": "height" }));
        let r = run(&mut s, "solid.feature.fillet", &json!({ "name": "round", "edges": [{ "point": [10, 0, 5] }], "size": 1 }));
        let removed = 1000.0 - f(&r, "volume");
        let exact = 10.0 * (1.0 - std::f64::consts::FRAC_PI_4);
        assert!((removed - exact).abs() < 0.1, "a round of 1 along a 10-long edge removes {removed}, against {exact}");

        let mut s = Session::new();
        run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "w": 10, "h": 10, "depth": 10 }));
        let r = run(&mut s, "solid.feature.chamfer", &json!({ "name": "bevel", "edges": [{ "point": [10, 0, 5] }], "size": 2 }));
        assert!((1000.0 - f(&r, "volume") - 20.0).abs() < 1e-6, "a chamfer of 2 removes 20: {r}");

        // An edge that is not there is named, not guessed at.
        let mut s = Session::new();
        run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "w": 4, "h": 3, "depth": 2 }));
        let m = err(&mut s, "solid.feature.fillet", &json!({ "edges": [{ "point": [99, 99, 99] }], "size": 1 }));
        assert!(m.contains("no edge passes through"), "{m}");
    }

    #[test]
    fn a_revolve_is_a_solid_or_a_named_failure() {
        let mut s = Session::new();
        // The kernel has no partial revolve yet, so this is either a ring or a reason — both of
        // which are the rule; the command must not turn the second into a success.
        let r = s.execute(
            "solid.feature.revolve",
            &json!({ "name": "ring", "sketch": { "at": [1, 0, 0], "u": [0, 0, 1], "v": [1, 0, 0], "w": 1, "h": 1 }, "axis": [0, 0, 1], "angle": 90, "segments": 16 }),
        );
        match r {
            Ok(v) => assert!(f(&v, "volume") > 0.0, "{v}"),
            Err(e) => assert!(!e.to_string().is_empty(), "{e}"),
        }
        // An axis that is not in the sketch's plane sweeps nothing, and says so.
        let mut s = Session::new();
        let m = err(&mut s, "solid.feature.revolve", &json!({ "sketch": { "w": 1, "h": 1 }, "axis": [0, 0, 1], "angle": 90 }));
        assert!(m.contains("plane"), "{m}");
    }

    #[test]
    fn a_sketch_can_be_given_as_loops() {
        let mut s = Session::new();
        let r =
            run(&mut s, "solid.feature.extrude", &json!({ "name": "body", "depth": 2, "sketch": { "loops": [[[0, 0], [4, 0], [4, 3], [0, 3]]] } }));
        assert_eq!(f(&r, "volume"), 24.0, "{r}");
        // More loops than a profile may have is refused on the JSON, before anything is built.
        let loops: Vec<Value> = (0..MAX_SKETCH_LOOPS + 1).map(|_| json!([[[0, 0], [1, 0], [1, 1]]])).collect();
        let m = err(&mut s, "solid.feature.extrude", &json!({ "depth": 1, "sketch": { "loops": loops } }));
        assert!(m.contains("loops"), "{m}");
    }

    #[test]
    fn the_commands_take_no_arguments_nobody_wanted() {
        let mut s = Session::new();
        for p in [json!(null), json!([]), json!("x"), json!({ "sketch": "no" }), json!({ "edges": [{ "point": "no" }] }), json!({ "segments": -1 })] {
            let _ = s.execute("solid.feature.extrude", &p);
            let _ = s.execute("solid.feature.revolve", &p);
            let _ = s.execute("solid.feature.cut", &p);
            let _ = s.execute("solid.feature.fillet", &p);
            let _ = s.execute("solid.feature.chamfer", &p);
            let _ = s.execute("solid.param.set", &p);
            let _ = s.execute("solid.feature.delete", &p);
            let _ = s.execute("solid.save", &p);
            let _ = s.execute("solid.open", &p);
            let _ = s.execute("solid.export.stl", &p);
        }
        // Nothing was written and nothing was added.
        assert_eq!(s.solid_model.features.len(), 0);
    }

    #[test]
    fn the_model_starts_empty_and_the_two_sweeps_take_them() {
        let s = Session::new();
        assert!(s.solid_model.features.is_empty(), "a session opens with no 3D model in it");
        assert_eq!(command_specs().iter().filter(|c| c.id.starts_with("solid.")).count(), specs().len());
        for c in specs() {
            assert!(c.interactive.is_none(), "{} is JSON-only", c.id);
            assert!((c.enabled)(&s).is_ok(), "{} is available with no drawing open", c.id);
        }
    }
}
