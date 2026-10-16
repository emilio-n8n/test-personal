//! Saving and loading a model document.
//!
//! A `.ccm` file holds the *history*, not the geometry: parameters, features and their sketches.
//! Opening one rebuilds it, exactly as if it had just been typed. That is the whole point of a
//! parametric document — and it has two consequences worth stating plainly:
//!
//! - The geometry in a file can never disagree with the features in it, because it is not stored.
//!   An old file opens with whatever the current kernel makes of it, and a new file opens on an
//!   old build with a version error rather than with geometry it cannot check.
//! - A file whose features no longer build still *loads*. The tree, the parameters and the last
//!   error come back, and the person who owns the document can fix it. Refusing to open a file
//!   because one dimension is wrong would be the worst possible answer.
//!
//! # The topology map, and what it is not
//!
//! The kernel has no topological naming: [`EdgeRef`] is a point and a tolerance, so an edit that
//! moves an edge breaks the reference and the blend says so rather than landing somewhere else.
//! That is honest, and it is also frustrating — move a part across the screen and every fillet on
//! it stops resolving. A file format can carry more than a point, so a document may also hold a
//! [`TopologyMap`]: for each blend feature, a *geometric fingerprint* of the edge the blend
//! actually resolved to, captured when it succeeded. On load the resolver tries the fingerprint
//! first and only falls back to the geometric reference, and it reports which one it used —
//! `Resolution::Fingerprint`, `Resolution::Fallback` or `Resolution::Unresolved` — so a document
//! never quietly blends an edge nobody asked for.
//!
//! **This is not topological naming.** A fingerprint survives a translation, a rotation and a
//! uniform scaling of the part, because everything it stores is either a unit vector or a length
//! measured against the part itself. It does *not* survive an edit that changes **which edges are
//! adjacent**: fillet the edge, add a rib, or cut the corner off, and the two faces that met
//! along it are not the two faces that meet along it now. It is also not unique — a part with two
//! identical edges has two identical fingerprints, and the resolver refuses to guess between them
//! rather than picking one. The full naming scheme (`MOD-002` in the master plan) is still not
//! done; what is here is the smallest thing the file format can honestly carry.

use std::collections::BTreeMap;
use std::fmt;

use cadcraft_geom::Vec3;
use cadcraft_solid::{Edge, Face, Solid};
use serde::{Deserialize, Serialize};

use crate::{
    error::ModelError,
    feature::{EdgeRef, Feature, FeatureKind, Model, Param, State},
    rebuild::{self, Rebuild},
};

/// The `.ccm` magic, so a wrong file is a clear error rather than a parse failure.
const MAGIC: &str = "CADCraft model";

/// The newest format this build reads.
///
/// **Why this did not move when the topology map was added.** Adding [`Document::topology`] is a
/// change to the on-disk shape, and the rule of thumb says bump the version. Two things make that
/// the wrong move *here*, and both belong to the work that owns `.ccm` v2 (`MOD-006` / `DAT-001`)
/// rather than to this one:
///
/// 1. **The addition is additive in both directions.** `topology` carries `#[serde(default)]`, so a
///    file written before the field existed still opens with an empty map; and serde ignores
///    fields it does not know, so a build that predates the field still opens a file that has one,
///    losing the fingerprints and keeping the document. Both halves are tested in
///    `tests/format.rs`, including a hand-written literal of the older shape.
/// 2. **A test pins this constant by string.** `tests/parametric.rs` builds a future version by
///    replacing `"version": 1` with `"version": 99`; bumping to 2 without touching that test makes
///    the replacement a no-op and the test a failure.
///
/// The map carries its own [`TOPOLOGY_VERSION`], so the sub-shape is versioned even while the
/// envelope is not, and the bump is two one-line changes when `DAT-001` makes it.
pub const FORMAT_VERSION: u32 = 1;

/// The schema of [`TopologyMap`]'s contents.
///
/// Separate from [`FORMAT_VERSION`] because the map is optional and self-describing: a file with
/// no `topology` key is a valid file, not an old one. Raise this when the *meaning* of a stored
/// fingerprint changes, and keep reading the previous one until the resolver says so.
pub const TOPOLOGY_VERSION: u32 = 1;

/// The on-disk form. Everything here is stable, public and versioned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub magic: String,
    pub version: u32,
    #[serde(default)]
    pub parameters: BTreeMap<String, Param>,
    #[serde(default)]
    pub features: Vec<Feature>,
    /// What each blend resolved to, so a document that moved can still find its edges.
    ///
    /// Optional, and absent from every file written before this field existed — which is the
    /// point: an old document opens, reports `Fallback` for every blend, and behaves exactly as it
    /// did before.
    #[serde(default)]
    pub topology: TopologyMap,
}

impl Document {
    pub fn new(parameters: BTreeMap<String, Param>, features: Vec<Feature>) -> Document {
        Document { magic: MAGIC.to_string(), version: FORMAT_VERSION, parameters, features, topology: TopologyMap::empty() }
    }

    /// A document that carries fingerprints as well as history.
    pub fn with_topology(parameters: BTreeMap<String, Param>, features: Vec<Feature>, topology: TopologyMap) -> Document {
        Document { magic: MAGIC.to_string(), version: FORMAT_VERSION, parameters, features, topology }
    }

    /// Parse a document and rebuild it.
    ///
    /// A model whose features do not build still comes back: `rebuild.problems` says which, and
    /// `model.solid` is whatever last worked.
    ///
    /// The fingerprints in the document are used, and the strategy each reference resolved by is
    /// dropped — the signature is the one the engine and the CLI already call. Use
    /// [`Document::into_model_with_report`] to see it.
    pub fn into_model(self) -> Result<(Model, Rebuild), ModelError> {
        let loaded = self.into_model_with_report()?;
        Ok((loaded.model, loaded.rebuild))
    }

    /// Parse a document, resolve its blend references and rebuild it — with the report of how each
    /// reference resolved.
    ///
    /// Resolving a fingerprint needs the solid the blend ran on, and that solid only exists once
    /// the features *below* the blend are built. So a load rebuilds, resolves against what came
    /// out, and rebuilds again **only if a reference was actually re-pointed**. A document with no
    /// `topology` — every file written before this existed — costs exactly one rebuild, as before.
    pub fn into_model_with_report(self) -> Result<LoadedModel, ModelError> {
        self.check()?;
        let topology = self.topology;
        let mut model = Model { parameters: self.parameters, features: self.features, records: Vec::new() };
        let mut rebuild = rebuild::rebuild(&mut model, None);
        let (resolution, re_pointed) = resolve_fingerprints(&mut model, &topology);
        if re_pointed {
            rebuild = rebuild::rebuild(&mut model, None);
        }
        Ok(LoadedModel { model, rebuild, resolution })
    }

    /// The magic, the version and the feature names: everything that can be refused without
    /// touching the geometry.
    fn check(&self) -> Result<(), ModelError> {
        if self.magic != MAGIC {
            return Err(ModelError::NotAModel);
        }
        if self.version > FORMAT_VERSION {
            return Err(ModelError::TooNew { found: self.version, supported: FORMAT_VERSION });
        }
        // Two features with the same name would make a reference ambiguous, so a file that has one
        // is refused rather than quietly renamed.
        for (i, f) in self.features.iter().enumerate() {
            if self.features[..i].iter().any(|e| e.name == f.name) {
                return Err(ModelError::DuplicateFeature(f.name.clone()));
            }
        }
        Ok(())
    }
}

impl Model {
    /// The document this model is. Round-trips exactly.
    ///
    /// Carries no fingerprints: a [`Model`] does not hold a [`TopologyMap`] of its own, and the
    /// map is captured by whoever saves. Use [`Model::to_document_with_topology`] to keep them.
    pub fn to_document(&self) -> Document {
        Document::new(self.parameters.clone(), self.features.clone())
    }

    /// The document this model is, with the fingerprints it was rebuilt from.
    pub fn to_document_with_topology(&self, topology: &TopologyMap) -> Document {
        Document::with_topology(self.parameters.clone(), self.features.clone(), topology.clone())
    }

    /// Save, capturing the fingerprints of every blend that built.
    ///
    /// Capture reads the model's own records, so it is worth what they are worth: a model that has
    /// not been rebuilt has no fingerprints, and a blend that failed contributes none. Use
    /// [`Model::save_json_with_topology`] when the caller holds the exact solids its blends ran on.
    pub fn save_json(&self) -> String {
        self.save_json_with_topology(&self.capture_topology())
    }

    /// Save with an explicit map — for a caller that captured the fingerprints from the exact
    /// solids its blends ran on, which is more than [`Model::capture_topology`] can know.
    pub fn save_json_with_topology(&self, topology: &TopologyMap) -> String {
        serde_json::to_string_pretty(&self.to_document_with_topology(topology))
            .unwrap_or_else(|e| format!(r#"{{"magic":"{MAGIC}","version":{FORMAT_VERSION},"error":{e:?}}}"#))
    }

    /// The fingerprints of every blend in this model that built, captured from the solid each one
    /// last operated on.
    ///
    /// A model that has never been rebuilt has no records and therefore no fingerprints: capture
    /// after a rebuild, not before.
    pub fn capture_topology(&self) -> TopologyMap {
        TopologyMap::capture(self)
    }

    /// Load a document and rebuild it.
    pub fn load_json(text: &str) -> Result<(Model, Rebuild), ModelError> {
        let loaded = Model::load_json_with_report(text)?;
        Ok((loaded.model, loaded.rebuild))
    }

    /// Load a document and rebuild it, reporting how each blend reference resolved.
    pub fn load_json_with_report(text: &str) -> Result<LoadedModel, ModelError> {
        let doc: Document = serde_json::from_str(text).map_err(|e| ModelError::CorruptHistory(e.to_string()))?;
        doc.into_model_with_report()
    }
}

/// What came back from a load: the model, its rebuild, and how each reference resolved.
#[derive(Clone, Debug)]
pub struct LoadedModel {
    pub model: Model,
    pub rebuild: Rebuild,
    pub resolution: ResolutionReport,
}

// -------------------------------------------------------------------------------------------
// The topology map.
// -------------------------------------------------------------------------------------------

/// How far two fingerprints may differ and still be the same edge.
///
/// The numbers below are fractions, not lengths, so they are scale-free: `POSITION` is a millionth
/// of a face's own diagonal, `ANGLE` a millionth of a cosine. Both are far looser than the
/// kernel's welding tolerance — a fingerprint is meant to survive an edit, not to be a rounding
/// check — and far tighter than any two genuinely different edges of one part.
const POSITION: f64 = 1e-6;
const ANGLE: f64 = 1e-6;

/// A geometric fingerprint of the edge one blend resolved to.
///
/// The world coordinates at the top are **recorded, not matched**. They are what a person reads
/// when a blend lands somewhere odd ("which edge was this?"), and they are the first thing to go
/// stale: move the part and they are wrong. Everything [`matches`](EdgeFingerprint::matches)
/// compares is either a unit vector or a length measured against the part, which is what makes a
/// fingerprint survive a translation, a rotation and a uniform scaling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeFingerprint {
    /// Where the edge started, in world coordinates, when it was captured.
    pub start: [f64; 3],
    /// And where it ended.
    pub end: [f64; 3],
    /// The unit normal of one of the two faces that met along it.
    pub face_a: [f64; 3],
    /// The normal of the other.
    pub face_b: [f64; 3],
    /// The edge's length over the solid's bounding-box diagonal: unchanged by a move or a scale.
    pub length_ratio: f64,
    /// The edge's midpoint in each adjacent face's own 2D frame, over that face's in-plane
    /// diagonal — which edge of which face this is. Two entries, compared as an unordered pair,
    /// because which neighbour sorts first is an accident of face ordering and not of geometry.
    pub at: [[f64; 2]; 2],
    /// `|edge direction · face normal|` for each adjacent face: how square the edge is to each.
    pub square: [f64; 2],
    /// The cosine of the angle between the two adjacent faces' normals — the edge's dihedral.
    pub dihedral: f64,
}

impl EdgeFingerprint {
    /// The fingerprint of one edge of `solid`, or `None` when it cannot be described: a shell with
    /// no edge list, an edge of no length, a face of no area, or a non-finite number anywhere.
    ///
    /// A caller that gets `None` has learned that this edge is not fingerprintable, which is a
    /// fact worth reporting rather than papering over with a zeroed record.
    pub fn of(solid: &Solid, edge: &Edge) -> Option<EdgeFingerprint> {
        let (start, end) = (edge.a, edge.b);
        let length = start.dist(end);
        let diagonal = solid.bounds().diagonal();
        if !length.is_finite() || length <= 0.0 || !diagonal.is_finite() || diagonal <= 0.0 {
            return None;
        }
        let first = solid.faces.get(edge.faces[0])?;
        let second = solid.faces.get(edge.faces[1])?;
        let first_diagonal = in_plane_diagonal(first);
        let second_diagonal = in_plane_diagonal(second);
        if first_diagonal <= 0.0 || second_diagonal <= 0.0 {
            return None;
        }
        let direction = (end - start) / length;
        let middle = start.mid(end);
        Some(EdgeFingerprint {
            start: [start.x, start.y, start.z],
            end: [end.x, end.y, end.z],
            face_a: [first.plane.normal.x, first.plane.normal.y, first.plane.normal.z],
            face_b: [second.plane.normal.x, second.plane.normal.y, second.plane.normal.z],
            length_ratio: length / diagonal,
            at: [over(middle_in_face(first, middle), first_diagonal), over(middle_in_face(second, middle), second_diagonal)],
            square: [direction.dot(first.plane.normal).abs(), direction.dot(second.plane.normal).abs()],
            dihedral: first.plane.normal.dot(second.plane.normal).abs(),
        })
    }

    /// Is `other` the same edge?
    ///
    /// Two fingerprints match when the edge's length over the part's size, the angle it makes with
    /// each of its two faces, the angle between those faces, and where it sits on each of them
    /// all agree. None of that changes when the part is moved, turned or scaled; all of it changes
    /// when the edge's *neighbourhood* does, which is the honest limit of the scheme.
    pub fn matches(&self, other: &EdgeFingerprint) -> bool {
        if !self.finite() || !other.finite() {
            return false;
        }
        (self.length_ratio - other.length_ratio).abs() <= POSITION
            && (self.dihedral - other.dihedral).abs() <= ANGLE
            && close_unordered(self.square, other.square, ANGLE)
            && close_unordered_pair(self.at, other.at, POSITION)
    }

    /// Every number here a real one: a fingerprint that cannot be compared matches nothing, which
    /// is the safe way round.
    fn finite(&self) -> bool {
        let ok = |v: f64| v.is_finite();
        ok(self.length_ratio) && ok(self.dihedral) && self.square.iter().all(|v| ok(*v)) && self.at.iter().all(|p| p.iter().all(|v| ok(*v)))
    }
}

fn over(point: [f64; 2], diagonal: f64) -> [f64; 2] {
    [point[0] / diagonal, point[1] / diagonal]
}

/// The in-plane midpoint of `point`, in `face`'s own frame.
fn middle_in_face(face: &Face, point: Vec3) -> [f64; 2] {
    let local = face.plane.to_local(point);
    [local.x, local.y]
}

/// The diagonal of a face in its own frame: the scale every in-plane measurement is against.
fn in_plane_diagonal(face: &Face) -> f64 {
    let bounds = face.region.bounds();
    bounds.width().hypot(bounds.height())
}

/// Do two pairs agree, in either order? Two adjacent faces have no order of their own that a
/// rebuild preserves: which one the kernel happened to number first is an accident of how the
/// solid was built, and a fingerprint that depended on it would break for no reason.
fn close_unordered(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
}

/// The same for a *pair* of points, which has no order of its own to preserve.
fn close_unordered_pair(a: [[f64; 2]; 2], b: [[f64; 2]; 2], tolerance: f64) -> bool {
    (close_unordered(a[0], b[0], tolerance) && close_unordered(a[1], b[1], tolerance))
        || (close_unordered(a[0], b[1], tolerance) && close_unordered(a[1], b[0], tolerance))
}

// -------------------------------------------------------------------------------------------
// Capturing.
// -------------------------------------------------------------------------------------------

/// What one blend feature's references resolved to, in the feature's own edge order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendTopology {
    /// One fingerprint per edge of the blend, positionally.
    #[serde(default)]
    pub edges: Vec<EdgeFingerprint>,
}

impl BlendTopology {
    /// Fingerprint every reference of `refs` against `input`.
    ///
    /// All of them or none: a partial list would misalign with the feature's own edge order, and a
    /// fingerprint that belongs to the wrong edge is worse than no fingerprint.
    pub fn capture(input: &Solid, refs: &[EdgeRef]) -> Option<BlendTopology> {
        let edges = input.edges()?;
        let tolerance = input.tolerance();
        let mut out = Vec::with_capacity(refs.len());
        for reference in refs {
            let index = unique_edge(&edges, reference, tolerance)?;
            let edge = edges.get(index)?;
            out.push(EdgeFingerprint::of(input, edge)?);
        }
        Some(BlendTopology { edges: out })
    }
}

/// The fingerprints of every blend in a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TopologyMap {
    /// The schema of the entries below.
    #[serde(default)]
    pub version: u32,
    /// One entry per feature that holds blend references, by feature name.
    #[serde(default)]
    pub features: BTreeMap<String, BlendTopology>,
}

impl Default for TopologyMap {
    fn default() -> TopologyMap {
        TopologyMap::empty()
    }
}

impl TopologyMap {
    pub fn empty() -> TopologyMap {
        TopologyMap { version: TOPOLOGY_VERSION, features: BTreeMap::new() }
    }

    /// Is this map empty of everything a document would carry?
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// What `feature`'s edges were captured as.
    pub fn get(&self, feature: &str) -> Option<&BlendTopology> {
        self.features.get(feature)
    }

    /// Capture every blend in `model` that **built**, from the solid it last operated on.
    ///
    /// A blend that failed contributes nothing: it never resolved an edge, so there is nothing to
    /// remember, and a document that claimed otherwise would be re-targeting a guess.
    ///
    /// The solid a blend ran on is found with the same rule `rebuild` uses to carry one feature's
    /// solid to the next — the last feature above it that produced something and is not consumed
    /// as a cut's tool. A caller that knows the exact solid should use
    /// [`TopologyMap::capture_from`] instead of trusting this to be right.
    pub fn capture(model: &Model) -> TopologyMap {
        let mut map = TopologyMap::empty();
        for index in 0..model.features.len() {
            let FeatureKind::Blend { edges, .. } = &model.features[index].kind else { continue };
            if !matches!(model.records.get(index).map(|r| &r.state), Some(State::Built)) {
                continue;
            }
            let Some(input) = blend_input(model, index) else { continue };
            let Some(captured) = BlendTopology::capture(input, edges) else { continue };
            let name = model.features[index].name.clone();
            map.features.insert(name, captured);
        }
        map
    }

    /// Capture one feature's blend from the solid it is about to be applied to.
    ///
    /// The precise form, for a caller holding that solid: the engine sees what a blend operates on
    /// as it runs it, which is the only moment the capture cannot be reconstructed afterwards.
    pub fn capture_from(model: &Model, feature: &str, input: &Solid) -> Option<BlendTopology> {
        let index = model.features.iter().position(|f| f.name == feature)?;
        let FeatureKind::Blend { edges, .. } = &model.features[index].kind else { return None };
        BlendTopology::capture(input, edges)
    }

    /// Remember one feature's fingerprints.
    pub fn insert(&mut self, feature: &str, topology: BlendTopology) {
        self.version = TOPOLOGY_VERSION;
        self.features.insert(feature.to_string(), topology);
    }
}

/// The solid a blend at `index` last operated on.
///
/// This mirrors the rule `rebuild` follows: the running solid at feature `index` is the last solid
/// produced by a feature above it, and a feature used as a cut's tool is consumed by the cut
/// rather than becoming the body. It is duplicated here because `rebuild` does not expose it, and
/// a private rule that decides where a fingerprint came from would be worse than a copy of it.
fn blend_input(model: &Model, index: usize) -> Option<&Solid> {
    let tools: Vec<&str> = model
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Cut { tool } => Some(tool.as_str()),
            _ => None,
        })
        .collect();
    (0..index).rev().find_map(|i| {
        let name = model.features.get(i)?.name.as_str();
        if tools.contains(&name) {
            return None;
        }
        model.records.get(i)?.last_good.as_ref()
    })
}

/// Index of the one edge `reference` names, or `None` when it names none or more than one.
///
/// The rule — nearest segment within `max(reference.tolerance, tolerance)`, and exactly one hit —
/// is the kernel's own, copied because `chamfer::resolve` is private. A reference that lands on
/// two edges is a corner and is ambiguous; guessing which one was meant is exactly the failure a
/// fallback must not have.
fn unique_edge(edges: &[Edge], reference: &EdgeRef, tolerance: f64) -> Option<usize> {
    if !reference.point.iter().all(|v| v.is_finite()) || !reference.tolerance.is_finite() || reference.tolerance < 0.0 {
        return None;
    }
    let point = Vec3::new(reference.point[0], reference.point[1], reference.point[2]);
    let reach = reference.tolerance.max(tolerance);
    let mut hits: Vec<usize> = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        if distance_point_segment(point, edge.a, edge.b) <= reach {
            hits.push(index);
        }
    }
    match hits.len() {
        0 => None,
        1 => hits.first().copied(),
        _ => None,
    }
}

/// Distance from a point to a segment. The kernel has the same five lines in `chamfer`, private.
fn distance_point_segment(point: Vec3, a: Vec3, b: Vec3) -> f64 {
    let along = b - a;
    let squared = along.dot(along);
    if squared <= 0.0 || !squared.is_finite() {
        return point.dist(a);
    }
    let t = ((point - a).dot(along) / squared).clamp(0.0, 1.0);
    (point - (a + along * t)).len()
}

// -------------------------------------------------------------------------------------------
// Resolving.
// -------------------------------------------------------------------------------------------

/// How one blend reference was found when a document was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// The stored fingerprint matched exactly one edge of the rebuilt solid.
    Fingerprint,
    /// No fingerprint matched, and the reference's own point still lands on one edge.
    Fallback,
    /// Neither: the edge this reference named is not there any more.
    Unresolved,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Resolution::Fingerprint => "fingerprint",
            Resolution::Fallback => "fallback",
            Resolution::Unresolved => "unresolved",
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reference's fate, named.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeResolution {
    pub feature: String,
    /// Which of that feature's edges.
    pub edge: usize,
    pub resolution: Resolution,
}

/// Every reference's fate, for a whole document.
///
/// The report is the point of the exercise: a fallback is a reference that only survived because
/// nothing moved, and a person who is not told cannot tell it from a fingerprint that is still
/// telling the truth.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolutionReport {
    pub edges: Vec<EdgeResolution>,
}

impl ResolutionReport {
    pub fn record(&mut self, feature: &str, edge: usize, resolution: Resolution) {
        self.edges.push(EdgeResolution { feature: feature.to_string(), edge, resolution });
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// Did any reference resolve by fingerprint? Not the same question as "was anything changed":
    /// a fingerprint that confirmed the reference the document already had changed nothing, and
    /// the resolver says so by leaving it alone.
    pub fn any_fingerprint(&self) -> bool {
        self.edges.iter().any(|e| e.resolution == Resolution::Fingerprint)
    }

    pub fn count(&self, resolution: Resolution) -> usize {
        self.edges.iter().filter(|e| e.resolution == resolution).count()
    }

    /// Every reference resolved by fingerprint. False when there were none: "everything worked"
    /// must never be reported for a document with no blends in it.
    pub fn all_fingerprints(&self) -> bool {
        !self.edges.is_empty() && self.edges.iter().all(|e| e.resolution == Resolution::Fingerprint)
    }

    /// Every reference resolved by its own geometric point — nothing more than the format has
    /// always guaranteed, and no less.
    pub fn all_fallback(&self) -> bool {
        !self.edges.is_empty() && self.edges.iter().all(|e| e.resolution == Resolution::Fallback)
    }

    /// Nothing resolved at all.
    pub fn all_unresolved(&self) -> bool {
        !self.edges.is_empty() && self.edges.iter().all(|e| e.resolution == Resolution::Unresolved)
    }

    /// One word per reference, in order.
    pub fn strategies(&self) -> Vec<&'static str> {
        self.edges.iter().map(|e| e.resolution.as_str()).collect()
    }

    /// What one feature's references did.
    pub fn for_feature(&self, feature: &str) -> Vec<&EdgeResolution> {
        self.edges.iter().filter(|e| e.feature == feature).collect()
    }

    /// One line a person can read: what each blend did, by name.
    pub fn summary(&self) -> String {
        if self.edges.is_empty() {
            return "no blend references to resolve".to_string();
        }
        let mut names: Vec<&str> = Vec::new();
        for edge in &self.edges {
            if !names.contains(&edge.feature.as_str()) {
                names.push(edge.feature.as_str());
            }
        }
        let mut out = String::new();
        for name in names {
            let here: Vec<&EdgeResolution> = self.for_feature(name);
            if !out.is_empty() {
                out.push_str("; ");
            }
            out.push_str(&format!(
                "{name}: {} fingerprint, {} fallback, {} unresolved",
                here.iter().filter(|e| e.resolution == Resolution::Fingerprint).count(),
                here.iter().filter(|e| e.resolution == Resolution::Fallback).count(),
                here.iter().filter(|e| e.resolution == Resolution::Unresolved).count()
            ));
        }
        out
    }
}

/// Resolve every blend reference in `model` against the solid each blend last operated on.
///
/// The fingerprint is tried first, and it is only allowed to do one thing: name the edge. If the
/// reference's own point already lands on that edge, nothing is rewritten — the fingerprint
/// confirmed the document rather than correcting it. If it does not, the reference is re-pointed
/// at the edge the fingerprint identified, which is the whole reason the map exists. A fingerprint
/// that matches nothing, or more than one edge, is not a match: the reference's own point decides,
/// and if that decides nothing the reference stays unresolved and the rebuild says so.
///
/// Returns the report and whether anything was re-pointed, which is the only thing worth a second
/// rebuild.
fn resolve_fingerprints(model: &mut Model, map: &TopologyMap) -> (ResolutionReport, bool) {
    let mut report = ResolutionReport::default();
    let mut re_point: Vec<(usize, usize, EdgeRef)> = Vec::new();
    for index in 0..model.features.len() {
        let references: Vec<EdgeRef> = match &model.features[index].kind {
            FeatureKind::Blend { edges, .. } => edges.clone(),
            _ => continue,
        };
        let name = model.features[index].name.clone();
        // The map is positional: edge `k` of the feature against entry `k` of the map. A feature
        // whose edge list has changed length since capture simply has no fingerprint for its later
        // edges, and `Option` above a short list is how that says so.
        let entry = map.get(&name);
        let Some(input) = blend_input(model, index) else {
            // Nothing in front of the blend: every reference is unresolved, and the rebuild says
            // so in its own words a moment later.
            for (edge, _) in references.iter().enumerate() {
                report.record(&name, edge, Resolution::Unresolved);
            }
            continue;
        };
        let edges = input.edges();
        let tolerance = input.tolerance();
        for (edge, reference) in references.iter().enumerate() {
            let fingerprint = entry.and_then(|t| t.edges.get(edge));
            let matched = match (&edges, fingerprint) {
                (Some(list), Some(fp)) => {
                    let mut hits: Vec<usize> = list
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| EdgeFingerprint::of(input, e).is_some_and(|f| f.matches(fp)))
                        .map(|(i, _)| i)
                        .collect();
                    if hits.len() == 1 { hits.pop() } else { None }
                }
                _ => None,
            };
            let landed = edges.as_ref().and_then(|list| unique_edge(list, reference, tolerance));
            match matched {
                // The fingerprint found the edge, and the reference already names that same edge:
                // nothing is rewritten. The person's chosen point is kept, because the person chose
                // it, and the fingerprint has confirmed it rather than replaced it.
                Some(found) if landed == Some(found) => {
                    report.record(&name, edge, Resolution::Fingerprint);
                }
                // The fingerprint found the edge and the reference does not name it — the part
                // moved. The reference is re-pointed at the edge the fingerprint identified, which
                // is the one thing the format can do that a bare point cannot.
                Some(found) => {
                    let middle = edges.as_ref().and_then(|list| list.get(found)).map(|e| e.a.mid(e.b));
                    match middle {
                        Some(middle) => {
                            let keep = if reference.tolerance.is_finite() && reference.tolerance > 0.0 { reference.tolerance } else { tolerance };
                            re_point.push((index, edge, EdgeRef::new(middle, keep)));
                            report.record(&name, edge, Resolution::Fingerprint);
                        }
                        None => report.record(&name, edge, Resolution::Unresolved),
                    }
                }
                // No fingerprint matched. Whatever the point still finds is what the document has
                // always been able to promise, and no more.
                None => report.record(&name, edge, if landed.is_some() { Resolution::Fallback } else { Resolution::Unresolved }),
            }
        }
    }
    let changed = !re_point.is_empty();
    for (index, edge, reference) in re_point {
        if let FeatureKind::Blend { edges, .. } = &mut model.features[index].kind
            && let Some(slot) = edges.get_mut(edge)
        {
            *slot = reference;
        }
    }
    (report, changed)
}
