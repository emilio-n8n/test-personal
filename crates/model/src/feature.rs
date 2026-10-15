//! The feature tree: an ordered list of named features, the parameters they read, and the result of
//! the last rebuild of each.
//!
//! Features are stored, never their geometry. The solid a feature produces is derived, cached and
//! *kept even when it is out of date* — that is what makes a failed edit recoverable instead of
//! destructive: the last good shape is still there, and the error says what went wrong with the new
//! one.
//!
//! # Topological naming
//!
//! There is no topological naming scheme here, and pretending otherwise would be worse than
//! saying so. A blend refers to an edge by *where it was*, and after an edit that moves it the
//! reference either still resolves to that edge or fails loudly; it never silently lands on a
//! different one. A real naming scheme would keep references across edits that change the topology
//! itself, and that work has not been done.

use std::collections::BTreeMap;

use cadcraft_geom::{Region, Regions, Vec2, Vec3};

use cadcraft_solid::Solid;
use serde::{Deserialize, Serialize};

use crate::error::ModelError;

/// A profile to be extruded or revolved.
///
/// The loops are in the sketch's *own* frame — the plane's canonical in-plane axes — not in world
/// coordinates. That is what makes a saved document mean the same thing on a build whose canonical
/// frame is computed slightly differently, and what lets the kernel take the profile directly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    /// Where the sketch sits: origin and normal.
    pub origin: Vec3,
    pub normal: Vec3,
    /// The plane's in-plane axis, so the frame is the one the profile was drawn in and not one
    /// inferred again on load.
    pub u: Vec3,
    /// Counter-clockwise outer loop first, then holes, in the plane's frame.
    pub loops: Vec<Vec<Vec2>>,
}

impl Sketch {
    /// The plane this sketch is drawn on, with the frame it was drawn in.
    pub fn plane(&self) -> Result<cadcraft_solid::Plane, ModelError> {
        cadcraft_solid::Plane::with_u(self.origin, self.normal, self.u)
            .map_err(|e| ModelError::Expression(format!("the sketch plane cannot be built: {e}")))
    }
}

impl Sketch {
    /// A sketch from world-space loops, projected into the plane.
    pub fn new(origin: Vec3, normal: Vec3, u: Vec3, loops: Vec<Vec<Vec3>>) -> Result<Sketch, ModelError> {
        let plane =
            cadcraft_solid::Plane::with_u(origin, normal, u).map_err(|e| ModelError::Expression(format!("the sketch plane cannot be built: {e}")))?;
        let projected: Vec<Vec<Vec2>> = loops.iter().map(|l| l.iter().map(|p| plane.to_local(*p)).collect()).collect();
        Ok(Sketch { origin: plane.origin, normal: plane.normal, u: plane.u, loops: projected })
    }

    /// A single rectangle from the corner `at`, drawn `w` along `u` and `h` along `v`.
    ///
    /// The two directions are given rather than taken from the plane's own frame on purpose: a
    /// sketch is something a person placed, and a rectangle that silently came out the other way
    /// up would put the geometry somewhere nobody asked for.
    pub fn rect(at: Vec3, u: Vec3, v: Vec3, w: f64, h: f64) -> Result<Sketch, ModelError> {
        let normal = u.cross(v);
        if normal.len() < 1e-12 {
            return Err(ModelError::Expression("a rectangle needs two directions that are not parallel".into()));
        }
        let a = at;
        let b = at + u * w;
        let c = b + v * h;
        let d = a + v * h;
        Sketch::new(a, normal, u, vec![vec![a, b, c, d]])
    }

    /// The profile as the kernel wants it: loops, already in the plane's own frame.
    pub fn regions(&self) -> Result<Regions, ModelError> {
        let plane = self.plane()?;
        Regions::from_loops(self.loops.clone(), plane_tolerance(&plane))
            .filter(|r| !r.is_empty())
            .ok_or_else(|| ModelError::Expression(format!("the sketch profile has no area ({} loops)", self.loops.len())))
    }

    /// Does the profile enclose anything?
    pub fn is_empty(&self) -> bool {
        self.loops.iter().all(|l| l.len() < 3)
    }
}

fn plane_tolerance(plane: &cadcraft_solid::Plane) -> f64 {
    let diag = plane.to_world(1.0, 0.0).dist(plane.origin).max(cadcraft_geom::EPS);
    (diag * 1e-9).clamp(1e-12, 1e-3)
}

/// A named parameter: either a number, or an expression over other parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Value(f64),
    /// The text as typed, so the document says what the user wrote.
    Formula(String),
}

/// A blend's edge reference, in the form a document stores.
///
/// The kernel has its own [`cadcraft_solid::EdgeRef`] and this converts to it. Keeping the two
/// apart is deliberate: the kernel is L0 and knows nothing about files, while a reference that
/// cannot be written down is a reference that cannot survive a save.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    /// A point that should lie on the edge.
    pub point: [f64; 3],
    /// How far from that point still counts as this edge.
    pub tolerance: f64,
}

impl EdgeRef {
    pub fn new(point: Vec3, tolerance: f64) -> EdgeRef {
        EdgeRef { point: [point.x, point.y, point.z], tolerance }
    }

    /// The kernel's form.
    pub fn to_kernel(self) -> cadcraft_solid::EdgeRef {
        cadcraft_solid::EdgeRef::new(Vec3::new(self.point[0], self.point[1], self.point[2]), self.tolerance)
    }
}

impl From<cadcraft_solid::EdgeRef> for EdgeRef {
    fn from(e: cadcraft_solid::EdgeRef) -> EdgeRef {
        EdgeRef::new(e.point, e.tolerance)
    }
}

/// What one feature needs from another. Dirty propagation walks this.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dep {
    /// Another feature's solid.
    Feature(String),
    /// A parameter's value.
    Parameter(String),
}

/// What a feature does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    /// Push a sketch along its own normal. `symmetric` centres the run on the sketch.
    Extrude { sketch: Sketch, depth: String, symmetric: bool },
    /// Swing a sketch about `axis`, which must lie in the sketch's own plane.
    Revolve { sketch: Sketch, axis: Vec3, angle: String, segments: usize },
    /// Subtract another feature's solid from everything built so far.
    Cut { tool: String },
    /// Round or bevel edges of everything built so far.
    Blend { edges: Vec<EdgeRef>, size: String, segments: usize, round: bool },
}

impl FeatureKind {
    /// What this feature reads besides the running solid.
    ///
    /// A cut reads the tool's solid, so a change to the tool — or to anything the tool reads — has
    /// to reach it, and the walk in [`Model::dirty_from`] closes that over transitively.
    pub fn dependencies(&self) -> Vec<Dep> {
        match self {
            FeatureKind::Extrude { depth, .. } | FeatureKind::Revolve { angle: depth, .. } => {
                formula_names(depth).into_iter().map(Dep::Parameter).collect()
            }
            FeatureKind::Cut { tool } => vec![Dep::Feature(tool.clone())],
            FeatureKind::Blend { size, .. } => formula_names(size).into_iter().map(Dep::Parameter).collect(),
        }
    }

    /// The human-readable kind, for errors and the UI.
    pub fn kind_name(&self) -> &'static str {
        match self {
            FeatureKind::Extrude { .. } => "extrude",
            FeatureKind::Revolve { .. } => "revolve",
            FeatureKind::Cut { .. } => "cut",
            FeatureKind::Blend { round: true, .. } => "fillet",
            FeatureKind::Blend { round: false, .. } => "chamfer",
        }
    }
}

/// Parameter names an expression mentions, as far as they can be read without evaluating.
pub fn formula_names(text: &str) -> Vec<String> {
    match crate::expr::parse(text) {
        Ok(e) => {
            let mut out = Vec::new();
            crate::expr::names_of(&e, &mut out);
            out
        }
        // A formula that does not parse yet still has to dirty anything it *looks* like it reads,
        // or a typo would silently freeze the model.
        Err(_) => text
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty() && w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_'))
            .map(str::to_string)
            .collect(),
    }
}

/// One entry of the feature tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub name: String,
    #[serde(flatten)]
    pub kind: FeatureKind,
    /// Whether the user has hidden it from the list. Purely presentational, but part of the file.
    #[serde(default)]
    pub suppressed: bool,
}

impl Feature {
    pub fn new(name: &str, kind: FeatureKind) -> Feature {
        Feature { name: name.to_string(), kind, suppressed: false }
    }
}

/// How one feature came out of the last rebuild.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Built, and up to date.
    Built,
    /// Not built yet, or deliberately skipped.
    Pending,
    /// Built once, but the current inputs no longer produce a solid. The solid is still there.
    Failed(String),
    /// An earlier feature failed, so this one never ran. The old solid, if any, is still there.
    Blocked(String),
}

/// What the last rebuild of one feature produced.
#[derive(Clone, Debug)]
pub struct Record {
    pub state: State,
    /// The last solid that *did* work. Kept across a failure on purpose.
    pub last_good: Option<Solid>,
}

impl Record {
    pub fn pending() -> Record {
        Record { state: State::Pending, last_good: None }
    }
}

/// A document: parameters, features, and what the last rebuild made of them.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub parameters: BTreeMap<String, Param>,
    pub features: Vec<Feature>,
    /// One record per feature, in the same order.
    pub records: Vec<Record>,
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    pub fn feature(&self, name: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.name == name)
    }

    pub fn record(&self, name: &str) -> Option<&Record> {
        let i = self.features.iter().position(|f| f.name == name)?;
        self.records.get(i)
    }

    /// The solid the model currently shows: the last feature's last good one.
    pub fn solid(&self) -> Option<&Solid> {
        self.records.iter().rev().find_map(|r| r.last_good.as_ref())
    }

    pub fn set(&mut self, name: &str, param: Param) -> Result<(), ModelError> {
        if name.trim().is_empty() {
            return Err(ModelError::EmptyName);
        }
        if let Param::Formula(text) = &param {
            crate::expr::parse(text)?;
        }
        self.parameters.insert(name.to_string(), param);
        Ok(())
    }

    pub fn add(&mut self, feature: Feature) -> Result<(), ModelError> {
        if feature.name.trim().is_empty() {
            return Err(ModelError::EmptyName);
        }
        if self.feature(&feature.name).is_some() {
            return Err(ModelError::DuplicateFeature(feature.name));
        }
        self.features.push(feature);
        self.records.push(Record::pending());
        Ok(())
    }

    /// Change a feature's definition, keeping its place in the tree.
    pub fn replace(&mut self, name: &str, kind: FeatureKind) -> Result<(), ModelError> {
        let Some(f) = self.features.iter_mut().find(|f| f.name == name) else {
            return Err(ModelError::NoSuchFeature(name.to_string()));
        };
        f.kind = kind;
        Ok(())
    }

    /// Remove a feature, and everything that was built on top of it.
    ///
    /// Deleting the *history* is not possible: the features above would still be there, describing
    /// geometry that no longer exists. So the tail goes with it, and the caller is told what.
    pub fn remove(&mut self, name: &str) -> Result<Vec<String>, ModelError> {
        let Some(i) = self.features.iter().position(|f| f.name == name) else {
            return Err(ModelError::NoSuchFeature(name.to_string()));
        };
        let removed: Vec<String> = self.features[i..].iter().map(|f| f.name.clone()).collect();
        self.features.truncate(i);
        self.records.truncate(i);
        Ok(removed)
    }

    /// Every parameter's value, or the reason one of them has none.
    ///
    /// Ordered so that a parameter may be defined in terms of another defined later: the
    /// evaluation is a DFS from each leaf, not a pass in declaration order.
    pub fn values(&self) -> Result<BTreeMap<String, f64>, ModelError> {
        let mut out = BTreeMap::new();
        for name in self.parameters.keys() {
            self.value_of(name, &mut out, &mut Vec::new())?;
        }
        Ok(out)
    }

    fn value_of(&self, name: &str, out: &mut BTreeMap<String, f64>, on_stack: &mut Vec<String>) -> Result<f64, ModelError> {
        if let Some(v) = out.get(name) {
            return Ok(*v);
        }
        if on_stack.iter().any(|n| n == name) {
            return Err(ModelError::Cyclic(name.to_string()));
        }
        let Some(param) = self.parameters.get(name) else {
            return Err(ModelError::NoSuchParameter(name.to_string()));
        };
        on_stack.push(name.to_string());
        let result = match param {
            Param::Value(v) => Ok(*v),
            Param::Formula(text) => {
                let expr = crate::expr::parse(text)?;
                crate::expr::evaluate(&expr, out).map_err(|e| match e {
                    ModelError::Expression(m) => ModelError::Expression(format!("{name}: {m}")),
                    other => other,
                })
            }
        };
        on_stack.pop();
        let value = result?;
        if !value.is_finite() {
            return Err(ModelError::Expression(format!("{name} is not a number")));
        }
        out.insert(name.to_string(), value);
        Ok(value)
    }

    /// One parameter's value, without needing the others to be valid.
    pub fn value(&self, name: &str) -> Result<f64, ModelError> {
        let mut out = BTreeMap::new();
        self.value_of(name, &mut out, &mut Vec::new())
    }

    /// Features whose inputs include `from`, directly.
    pub fn dependents(&self, from: &Dep) -> Vec<String> {
        self.features.iter().filter(|f| f.kind.dependencies().iter().any(|d| d == from)).map(|f| f.name.clone()).collect()
    }

    /// The features that have to be rebuilt after `changed`, in tree order.
    ///
    /// A change to a feature reaches everything above it that reads it, and to a parameter it
    /// reaches everything that reads the parameter. Transitive, because a feature three steps up
    /// depends on a feature two steps up just as much.
    pub fn dirty_from(&self, changed: &[Dep]) -> Vec<usize> {
        let mut dirty = vec![false; self.features.len()];
        let mut queue: Vec<Dep> = changed.to_vec();
        let mut seen: Vec<Dep> = Vec::new();
        while let Some(dep) = queue.pop() {
            if seen.contains(&dep) {
                continue;
            }
            seen.push(dep.clone());
            // Naming a feature is itself a change to it: nobody has to know what it reads for the
            // feature they just edited to come back.
            match &dep {
                Dep::Feature(name) => {
                    if let Some(i) = self.features.iter().position(|f| &f.name == name) {
                        dirty[i] = true;
                    }
                }
                Dep::Parameter(name) => {
                    // A parameter built from this one carries the change: `twice = height * 2`
                    // means changing `height` has to reach everything that reads `twice`.
                    for (other, param) in &self.parameters {
                        if other != name
                            && matches!(param, Param::Formula(text) if formula_names(text).iter().any(|n| n == name))
                            && !seen.contains(&Dep::Parameter(other.clone()))
                        {
                            queue.push(Dep::Parameter(other.clone()));
                        }
                    }
                }
            }
            for (i, f) in self.features.iter().enumerate() {
                if !dirty[i] && f.kind.dependencies().contains(&dep) {
                    dirty[i] = true;
                    queue.push(Dep::Feature(f.name.clone()));
                }
            }
        }
        // A cut reads the solid in front of it, so anything above the tool is downstream of it.
        dirty.into_iter().enumerate().filter_map(|(i, d)| d.then_some(i)).collect()
    }
}

/// The bounding box of a sketch's profile, for previews and for sanity checks.
pub fn sketch_bounds(sketch: &Sketch) -> Option<(Vec2, Vec2)> {
    let region: Region = sketch.regions().ok()?.iter().next().cloned()?;
    let b = region.bounds();
    Some((b.min, b.max))
}
