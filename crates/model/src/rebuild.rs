//! Rebuilding: turning a feature tree into solids, and telling the truth when it cannot.
//!
//! Two rules, and they are the whole design:
//!
//! 1. **A failure never destroys the last good shape.** Every feature keeps the last solid that
//!    worked. A dimension that stops the model from building leaves the previous shape on screen
//!    with the error beside it, which is the difference between a model you fix and a model you lost.
//! 2. **A blocked feature is not a broken one.** If an upstream feature fails, everything above it
//!    is reported as blocked *by that feature*, with its own last good solid intact, so nobody
//!    mistakes "not rebuilt" for "rebuilt to nothing".

use cadcraft_geom::Vec2;
use cadcraft_solid::{Bool3, Plane, Solid, boolean, chamfer, extrude, fillet, revolve};
use std::collections::BTreeMap;

use crate::{
    error::ModelError,
    feature::{Dep, Feature, FeatureKind, Model, Record, State},
};

/// The result of a rebuild, ready to be shown or checked.
#[derive(Clone, Debug, Default)]
pub struct Rebuild {
    /// The solid the model now shows: the last feature's last good solid.
    pub solid: Option<Solid>,
    /// One line per feature that is not `Built`, empty when everything is fine.
    pub problems: Vec<Problem>,
    /// How many features actually ran.
    pub rebuilt: usize,
}

/// What went wrong, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub feature: String,
    pub kind: State,
    pub message: String,
}

/// Rebuild everything, or only what `dirty` says changed.
pub fn rebuild(model: &mut Model, dirty: Option<&[usize]>) -> Rebuild {
    let Some(values) = model.values().ok() else {
        // Parameters that do not resolve are reported by `problems` below, feature by feature.
        let values = BTreeMap::new();
        return run(model, &values, dirty);
    };
    run(model, &values, dirty)
}

/// Rebuild after a change, given what the change touched.
pub fn rebuild_after(model: &mut Model, changed: &[Dep]) -> Rebuild {
    let dirty = model.dirty_from(changed);
    let Some(values) = model.values().ok() else {
        return run(model, &BTreeMap::new(), Some(&dirty));
    };
    run(model, &values, Some(&dirty))
}

fn run(model: &mut Model, values: &BTreeMap<String, f64>, dirty: Option<&[usize]>) -> Rebuild {
    if model.records.len() != model.features.len() {
        model.records = model.features.iter().map(|_| Record::pending()).collect();
    }
    let mut out = Rebuild::default();
    // The solid in front of each feature: what a cut or a blend operates on. A feature some later
    // cut uses as its *tool* does not become this — it is consumed by the cut, and letting it
    // replace the body would leave the cut cutting the part with its own tool.
    let tools = tool_features(model);
    let mut carried: Option<Solid> = None;
    for i in 0..model.features.len() {
        // A feature that is not being rebuilt passes its own last solid along untouched. Using the
        // running solid here would hand the *body* to a tool that owes us a *cutter*, and the cut
        // that follows would cut the part with itself.
        if let Some(list) = dirty
            && !list.contains(&i)
        {
            carried = model.records.get(i).and_then(|r| r.last_good.clone()).or(carried);
            continue;
        }
        let feature = model.features[i].clone();
        if feature.suppressed {
            let keep = model.records.get(i).and_then(|r| r.last_good.clone()).or_else(|| carried.clone());
            model.records[i] = Record { state: State::Pending, last_good: keep.clone() };
            out.problems.push(Problem {
                feature: feature.name.clone(),
                kind: State::Pending,
                message: "suppressed: this feature did not run".into(),
            });
            carried = keep;
            continue;
        }
        // An upstream failure is not this feature's failure.
        if let Some(blocker) = out.problems.iter().find(|p| matches!(p.kind, State::Failed(_))).cloned() {
            let message = format!("not rebuilt: {} failed first", blocker.feature);
            let state = State::Blocked(message.clone());
            // Its own last solid, not the running one: this feature produced something of its own
            // last time, and that is what stays on screen until the thing under it is fixed.
            let keep = model.records.get(i).and_then(|r| r.last_good.clone()).or_else(|| carried.clone());
            model.records[i] = Record { state: state.clone(), last_good: keep.clone() };
            out.problems.push(Problem { feature: feature.name.clone(), kind: state, message });
            carried = keep;
            continue;
        }
        match build(&feature, model, values, &carried) {
            Ok(Some(solid)) => {
                model.records[i] = Record { state: State::Built, last_good: Some(solid.clone()) };
                if !tools.contains(&feature.name) {
                    carried = Some(solid);
                }
                out.rebuilt += 1;
            }
            // A feature that produces nothing new (a cut that removes nothing still produces a
            // solid; a feature with no input solid passes the running solid through).
            Ok(None) => {
                let s = carried.clone();
                model.records[i] = Record { state: State::Built, last_good: s.clone() };
                carried = s;
                out.rebuilt += 1;
            }
            Err(e) => {
                let message = e.to_string();
                let state = State::Failed(message.clone());
                // Its own last solid stays: this is the rule the whole module exists for. Keeping
                // the running one instead would quietly replace a cut's result with the body's.
                let keep = model.records.get(i).and_then(|r| r.last_good.clone()).or_else(|| carried.clone());
                model.records[i] = Record { state: state.clone(), last_good: keep.clone() };
                out.problems.push(Problem { feature: feature.name.clone(), kind: state, message });
                carried = keep;
            }
        }
    }
    // If nothing ran at all, the model still shows the last thing that did.
    out.solid = carried.or_else(|| model.records.iter().rev().find_map(|r| r.last_good.clone()));
    out
}

/// The names some feature uses as a cut's tool.
fn tool_features(model: &Model) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in &model.features {
        if let FeatureKind::Cut { tool } = &f.kind
            && !out.contains(tool)
        {
            out.push(tool.clone());
        }
    }
    out
}

fn number(values: &BTreeMap<String, f64>, formula: &str) -> Result<f64, ModelError> {
    match crate::expr::parse(formula) {
        Ok(e) => crate::expr::evaluate(&e, values),
        Err(_) => match formula.trim().parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            Ok(_) => Err(ModelError::Expression(format!("{formula:?} is not a number"))),
            Err(_) => Err(ModelError::Expression(format!("{formula:?} is not a parameter or a number"))),
        },
    }
}

fn build(feature: &Feature, model: &Model, values: &BTreeMap<String, f64>, carried: &Option<Solid>) -> Result<Option<Solid>, ModelError> {
    match &feature.kind {
        FeatureKind::Extrude { sketch, depth, symmetric } => {
            let profile = sketch.regions()?;
            let d = number(values, depth)?;
            if !d.is_finite() || d <= 0.0 {
                return Err(ModelError::Expression(format!("{}: the depth {depth:?} is {d}, which is not a positive distance", feature.name)));
            }
            let plane = sketch_plane(sketch)?;
            if *symmetric {
                let up = extrude(&profile, &plane, d / 2.0).map_err(|e| ModelError::in_feature(&feature.name, e))?;
                // Move the plane down rather than flipping it: a flipped plane reverses its `u`
                // axis too, which mirrors the profile and puts half the solid on the other side.
                let down = extrude(&profile, &plane.offset(-d / 2.0), d / 2.0).map_err(|e| ModelError::in_feature(&feature.name, e))?;
                let joined = boolean(&up, &down, Bool3::Union).map_err(|e| ModelError::in_feature(&feature.name, e))?;
                validate(&feature.name, joined.clone())?;
                return Ok(Some(joined));
            }
            let solid = extrude(&profile, &plane, d).map_err(|e| ModelError::in_feature(&feature.name, e))?;
            validate(&feature.name, solid.clone())?;
            Ok(Some(solid))
        }
        FeatureKind::Revolve { sketch, axis, angle, segments } => {
            let a = number(values, angle)?;
            if !a.is_finite() || a.abs() > 360.0 {
                return Err(ModelError::Expression(format!(
                    "{}: the revolve angle {angle:?} is {a}, which is not between -360 and 360",
                    feature.name
                )));
            }
            if *segments < 3 || *segments > 256 {
                return Err(ModelError::Expression(format!("{}: {segments} segments is not between 3 and 256", feature.name)));
            }
            // The axis has to lie in the sketch's own plane: revolving a profile about a line
            // through it at right angles sweeps nothing at all, and saying so here is better than a
            // zero-volume shell further down.
            let plane = sketch_plane(sketch)?;
            let axis_direction = axis.normalized();
            if axis_direction.len() < 0.5 {
                return Err(ModelError::Expression(format!("{}: the revolve axis is not a direction", feature.name)));
            }
            if plane.distance(plane.origin + axis_direction).abs() > plane_tolerance(&plane) {
                return Err(ModelError::Expression(format!("{}: the revolve axis has to lie in the sketch's plane", feature.name)));
            }
            // The kernel revolves about the plane's `v` axis, so the frame is chosen to make that
            // the axis, and the profile is carried across into it. Anything else and the check
            // inside the kernel refuses the shape for a reason that means nothing to a caller.
            let u = plane.normal.cross(axis_direction).normalized();
            if u.len() < 0.5 {
                return Err(ModelError::Expression(format!("{}: the revolve axis is along the sketch's normal, which sweeps nothing", feature.name)));
            }
            let revolve_plane = Plane::with_u(plane.origin, plane.normal, u).map_err(|e| ModelError::in_feature(&feature.name, e))?;
            let loops: Vec<Vec<Vec2>> =
                sketch.loops.iter().map(|l| l.iter().map(|p| revolve_plane.to_local(plane.to_world(p.x, p.y))).collect()).collect();
            let profile = cadcraft_geom::Regions::from_loops(loops, plane_tolerance(&revolve_plane))
                .filter(|r| !r.is_empty())
                .ok_or_else(|| ModelError::Expression(format!("{}: the sketch profile has no area", feature.name)))?;
            let solid =
                revolve(&profile, &revolve_plane, axis_direction, a.to_radians(), *segments).map_err(|e| ModelError::in_feature(&feature.name, e))?;
            validate(&feature.name, solid.clone())?;
            Ok(Some(solid))
        }
        FeatureKind::Cut { tool } => {
            let Some(cutter) = model.record(tool).and_then(|r| r.last_good.clone()) else {
                return Err(ModelError::NoSuchFeature(tool.clone()));
            };
            let Some(body) = carried else {
                return Err(ModelError::NoSolid);
            };
            let cut = boolean(body, &cutter, Bool3::Difference).map_err(|e| ModelError::in_feature(&feature.name, e))?;
            validate(&feature.name, cut.clone())?;
            Ok(Some(cut))
        }
        FeatureKind::Blend { edges, size, segments, round } => {
            let Some(body) = carried else {
                return Err(ModelError::NoSolid);
            };
            let s = number(values, size)?;
            if !s.is_finite() || s <= 0.0 {
                return Err(ModelError::Expression(format!("{}: the blend size {size:?} is {s}, which is not a positive distance", feature.name)));
            }
            let blended = if *round {
                fillet(body, &edges.iter().map(|e| e.to_kernel()).collect::<Vec<_>>(), s, *segments)
                    .map_err(|e| ModelError::in_feature(&feature.name, e))?
            } else {
                chamfer(body, &edges.iter().map(|e| e.to_kernel()).collect::<Vec<_>>(), s).map_err(|e| ModelError::in_feature(&feature.name, e))?
            };
            validate(&feature.name, blended.clone())?;
            Ok(Some(blended))
        }
    }
}

fn validate(name: &str, solid: Solid) -> Result<(), ModelError> {
    solid.validate().map_err(|e| ModelError::in_feature(name, e))?;
    solid.validate_vertices().map_err(|e| ModelError::in_feature(name, e))?;
    Ok(())
}

fn sketch_plane(sketch: &crate::feature::Sketch) -> Result<Plane, ModelError> {
    sketch.plane()
}

fn plane_tolerance(plane: &Plane) -> f64 {
    (plane.to_world(1.0, 0.0).dist(plane.origin) * 1e-9).clamp(1e-12, 1e-3)
}

