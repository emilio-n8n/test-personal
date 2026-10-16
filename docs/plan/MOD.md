# MOD — Parametric model and history: exhaustive task index

**Domain owner:** `MOD` — owns `crates/model/**` except `crates/model/src/sketch/**` (SKT).
**Waves:** wave 2 for `MOD-SKC`, wave 4 for `MOD-NAM`/`MOD-BOD`, wave 5 for `MOD-HIS`/`MOD-REC`/`MOD-FMT`/`MOD-VAL` (`docs/master-plan.md` §6).
**Seed ids:** master-plan `MOD-001..006` map to `MOD-SKC` (`MOD-001`), `MOD-NAM` (`MOD-002`), `MOD-HIS` (`MOD-003`), `MOD-BOD` (`MOD-004`), `MOD-REC` (`MOD-005`), `MOD-FMT` (`MOD-006`).

**Id scheme.** `MOD-<EPIC>-<NNN>`, per `docs/master-plan.md` §2 (`DOMAIN-EPIC-NNN`). The master plan's flat `MOD-001` is the *epic* id for `MOD-SKC`; the per-task ids below are the assignable units.

**Totals:** 155 tasks — `MOD-SKC` 25, `MOD-NAM` 25, `MOD-HIS` 25, `MOD-BOD` 20, `MOD-REC` 25, `MOD-FMT` 20, `MOD-VAL` 15.

---

## Conventions and invariants

Every task in this file inherits these. A task that needs to break one states the break in its own `Contract` line and says why in `Risk`.

1. **No panics.** `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]` and `#![forbid(unsafe_code)]` at the crate root, as `crates/model/src/lib.rs:43–44` already carries.
2. **No silent wrong answer.** An operation that cannot produce a valid solid returns an `Err` (`ModelError`) that names the feature and the reason. Never a silently re-oriented profile, a clipped loop, or a zero-volume shell.
3. **Units.** Parameters are unitless numbers. The document's length unit is the caller's concern (`DAT-005`); `MOD` converts nothing. Angles in revolve parameters are **degrees** at the parameter boundary, radians only inside `cadcraft_solid::revolve` (`crates/model/src/rebuild.rs:218`).
4. **Winding.** Sketch loops: outer counter-clockwise, holes clockwise, in the sketch plane's `u`/`v` frame (`crates/model/src/feature.rs:39`). Extrude pushes along `+normal`.
5. **Tolerance is never a magic constant.** The profile tolerance rule is `(diag * 1e-9).clamp(1e-12, 1e-3)` (`crates/model/src/feature.rs:91`). Every predicate states which tolerance it uses.
6. **The last good solid survives.** A feature that fails keeps its `last_good` solid. A feature above a failure is `Blocked`, not `Failed`. The model's shown solid is the last feature's `last_good` (`crates/model/src/rebuild.rs:5–10`). A failed rebuild is never a lost drawing.
7. **Blocked ≠ failed.** `State::Blocked` means "an upstream feature failed, so this one never ran." `State::Failed` means "this feature ran and could not produce a solid." The distinction is the point (`crates/model/src/rebuild.rs:8–10`).
8. **Reuse before writing.** `crates/model` already has a feature tree, dirty propagation, atomic rebuild and a topology map. A task that would duplicate one of these is `BLOCKED` on the existing code being generalised, not on a new module being written. The reuse table is in the next section.
9. **Layering.** `MOD` is L4 (`crates/model`), above `cadcraft-geom` (L0), `cadcraft-solid` (L0), `cadcraft-doc` (L1), `cadcraft-constraints` (L2), `cadcraft-fonts` (L2), `cadcraft-render` (L2). It must not depend on `cadcraft-io` (L3), `cadcraft-engine` (L5), or any UI crate. Enforced by `cargo xtask layers`.
10. **What `MOD` consumes from `SKT`.** After `SKT-ENT-020`, `sketch::regions(&SketchDoc, x, seg_tol) -> Result<Regions, SketchError>` is the one function `MOD` calls to get a profile from a constrained sketch. Until then, `MOD-SKC` tasks are `BLOCKED` on `SKT-ENT-020`.

## State legend

`READY` (all prerequisites validated, files unowned) · `BACKLOG` (specified, ordered, not ready) · `BLOCKED` (a named prerequisite is missing — the task says which and who owns it).

---

## The dependency chain that matters

```
SKT-ENT-020  sketch::regions()  ──▶  MOD-SKC  sketch-backed extrude/revolve
                                           │
                                           ▼
MOD-REC  partial rebuild  ──▶  MOD-NAM  persistent edge references
                                           │
                                           ▼
MOD-BOD  multi-body  ──▶  MOD-HIS  undo/redo as tree ops
                                           │
                                           ▼
                              MOD-FMT  .ccm v2  ──▶  MOD-VAL  load-time invariants
```

* **No feature reads a sketch's points.** Today `FeatureKind::Extrude { sketch: Sketch }` stores `loops: Vec<Vec<Vec2>>` (`crates/model/src/feature.rs:40`). After `MOD-SKC-001..010` it stores a `SketchDoc` + solved parameter vector, and the profile is `sketch::regions()`. Until `SKT-ENT-020` lands, `MOD-SKC` is `BLOCKED`.
* **No naming until rebuild is stable.** `MOD-NAM` needs a rebuild that produces consistent topology. It depends on `MOD-REC-005` (deterministic rebuild) and `KER-BOOL-004` (vertex welding).
* **No undo/redo until the tree is immutable.** `MOD-HIS` snapshots the `Model` (parameters + features), not the solids. It depends on `MOD-BOD-005` (multi-body model is still one `Model`).
* **No file format v2 until the tree shape is stable.** `MOD-FMT` depends on `MOD-BOD` and `MOD-HIS`; a v2 that does not carry multi-body or undo state is a v2 that will be a v3 in a year.

---

## What is reused from existing code

### Reused as code (public today)

| Item | Evidence | Used by |
|---|---|---|
| `Model`, `Feature`, `FeatureKind`, `Param`, `Dep`, `State`, `Record` | `crates/model/src/feature.rs:98–252` | all `MOD` |
| `Model::values()`, `value_of()`, `dirty_from()` | `crates/model/src/feature.rs:323–416` | `MOD-REC` |
| `rebuild()`, `rebuild_after()`, `Rebuild`, `Problem` | `crates/model/src/rebuild.rs:23–57` | `MOD-REC` |
| `expr::{parse, evaluate, names_of, Expr, BinOp}` | `crates/model/src/expr.rs:22–219` | `MOD-SKC`, `MOD-VAL` |
| `file::{Document, FORMAT_VERSION, TopologyMap, EdgeFingerprint}` | `crates/model/src/file.rs:79–94, 238–258` | `MOD-FMT`, `MOD-NAM` |
| `Sketch::{new, rect, plane, regions}` | `crates/model/src/feature.rs:53–89` | `MOD-SKC` |
| `ErrorRef`, `Dep::{Feature, Parameter}` | `crates/model/src/feature.rs:110–141` | `MOD-NAM`, `MOD-REC` |

### Reused in form only (must be generalised, not copied)

| Item | Evidence | Task |
|---|---|---|
| `Record { state, last_good }` | `crates/model/src/feature.rs:233–237` — one solid per feature | `MOD-BOD-001` multi-body record |
| `tool_features()` — the carried-solid rule | `crates/model/src/rebuild.rs:137–147` | `MOD-BOD-005`, `MOD-REC-010` |
| `EdgeFingerprint::of` / `matches` | `crates/model/src/file.rs:266–309` | `MOD-NAM-001` persistent fingerprint |
| `Model::remove` tail-truncation | `crates/model/src/feature.rs:309–317` | `MOD-HIS-005` undo of a delete |

### Not reused (and why)

| Item | Evidence | Replaced by |
|---|---|---|
| `cadcraft_constraints::solve` | takes `&mut Drawing` (`crates/constraints/src/lib.rs:392`) — a model is not a drawing | `SKT` owns the sketch solver; `MOD` consumes `sketch::regions()` |
| `cadcraft_doc::Constraint` | `crates/doc/src/constraint.rs:156` — drawing-level, handle-keyed | `MOD` parameters are named, not handle-keyed |
| `cadcraft_solid::fillet`/`chamfer` | `crates/model/src/rebuild.rs:242–246` — already called directly | nothing; this is the correct reuse |

---

## MOD-SKC — sketch-backed features: extrude and revolve that read parameters

Today a sketch's profile is static geometry. After this epic, `FeatureKind::Extrude` and `FeatureKind::Revolve` store a `SketchDoc` and read their parameters through the SKT solver. `MOD-SKC-001..010` are the gate for every other `MOD` epic.

### MOD-SKC-001 — Add a `SketchSource` enum to decouple features from sketch storage
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub enum SketchSource { Static(Sketch), Parametric(SketchDoc, Vec<f64>) }` with `pub fn SketchSource::profile(&self) -> Result<Regions, ModelError>` dispatching to `Sketch::regions()` for `Static` and `sketch::regions(&self.1, &self.0, self.1.len() as f64 * 1e-9)` for `Parametric`. The `seg_tol` is derived from the sketch extent, never a magic constant.
- **Behaviour:** This is the seam. `Static` is the v1 path; `Parametric` is the SKT path. A feature holding a `Parametric` sketch re-solves it on every rebuild; a feature holding a `Static` sketch does not. The enum is `Serialize`/`Deserialize` so a document round-trips.
- **Refusals:** `ModelError::Expression("the parametric sketch has no solved parameters")` when `Parametric` holds an empty vector; `ModelError::Kernel` for a kernel failure.
- **Tests:** `a_static_sketch_source_produces_the_same_regions_as_before`; `a_parametric_sketch_source_regions_to_the_solved_profile`.
- **Acceptance:** `FeatureKind::Extrude { sketch: SketchSource, .. }` compiles and the 18 existing tests pass unchanged.
- **Risk:** Changing `FeatureKind::Extrude`'s shape breaks every existing document. Fallback: keep `Extrude { sketch: Sketch }` as-is and add `ExtrudeParametric { doc, x, depth, symmetric }` as a new variant; migrate in `MOD-FMT`.

### MOD-SKC-002 — `FeatureKind::Extrude` accepts a `SketchSource`
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `FeatureKind::Extrude { sketch: SketchSource, depth: String, symmetric: bool }` — the `sketch` field type changes from `Sketch` to `SketchSource`. All constructors and pattern matches updated.
- **Behaviour:** The v1 `Sketch` is wrapped in `SketchSource::Static`. A document written before this change still deserialises because `SketchSource` has `#[serde(untagged)]` with `Static` first.
- **Refusals:** Deserialisation of a `SketchSource` that is neither a valid `Sketch` nor a valid `(SketchDoc, Vec<f64>)` is `Err(ModelError::CorruptHistory)`.
- **Tests:** `an_extrude_with_a_static_sketch_still_builds`; `an_extrude_with_a_parametric_sketch_builds_from_the_solved_profile`.
- **Acceptance:** The 18 existing tests pass; a new test proves the `Static` path is bit-identical to the old behaviour.
- **Risk:** `#[serde(untagged)]` silently tries `Static` first and may mis-parse a `Parametric` document. Fallback: a tag field `"source": "static"|"parametric"`.

### MOD-SKC-003 — `FeatureKind::Revolve` accepts a `SketchSource`
- **State:** READY
- **Depends on:** MOD-SKC-002
- **Files:** `crates/model/src/feature.rs` (MOD), `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `FeatureKind::Revolve { sketch: SketchSource, axis: Vec3, angle: String, segments: usize }`.
- **Behaviour:** The revolve path in `build()` (`crates/model/src/rebuild.rs:182–221`) calls `sketch.regions()` which now dispatches through `SketchSource::profile()`. The axis-in-plane check and the frame transform are unchanged.
- **Refusals:** As `MOD-SKC-002`, plus the existing revolve refusals (`crates/model/src/rebuild.rs:184–210`).
- **Tests:** `a_revolve_with_a_parametric_sketch_builds_from_the_solved_profile`.
- **Acceptance:** The revolve tests in `crates/model/tests/parametric.rs` pass unchanged.
- **Risk:** The revolve frame transform (`crates/model/src/rebuild.rs:212–213`) assumes `sketch.loops`; it must now call `derived_loops` on the solved profile. Fallback: extract a `profile_loops(&SketchSource) -> Result<Vec<Vec<Vec2>>, ModelError>` helper and use it in both extrude and revolve.

### MOD-SKC-004 — A parametric sketch's solved vector is stored, not re-solved on every access
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `SketchSource::Parametric(SketchDoc, Vec<f64>)` — the `Vec<f64>` is the last solved parameter vector, stored in the feature. `pub fn SketchSource::solved(&self) -> Option<&[f64]>`.
- **Behaviour:** Re-solving a sketch is expensive (LM iteration). The feature stores the vector that produced its last good solid so a rebuild that does not touch the sketch can skip the solve. A rebuild that *does* touch the sketch re-solves and overwrites the stored vector.
- **Refusals:** `ModelError::Expression("the stored parameter vector does not match the sketch")` when `x.len() != doc.parameter_count()`.
- **Tests:** `a_rebuild_that_does_not_touch_the_sketch_skips_the_solve`; `a_rebuild_that_touches_the_sketch_re_solves`.
- **Acceptance:** `Rebuild::rebuilt` counts the feature as rebuilt but the sketch solve is observably skipped (instrumented by a counter in test).
- **Risk:** Storing a stale vector that was solved against a different constraint set. Fallback: a `solution_hash: u64` on the feature, checked before reuse.

### MOD-SKC-005 — `Dep::Sketch(EntityId)` for dirty propagation from sketch to feature
- **State:** BLOCKED
- **Depends on:** SKT-ENT-009
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Dep::Sketch(String)` — a new variant naming the sketch document a feature reads. `FeatureKind::dependencies()` returns it for `Parametric` sketches.
- **Behaviour:** Changing a sketch's constraints or entities dirties every feature that reads that sketch. Without this, editing a sketch does not rebuild the extrusion that uses it. The variant is `Serialize`/`Deserialize` with `#[serde(default)]` so old documents (which have no sketch deps) still parse.
- **Refusals:** Deserialisation of a document with a `Dep::Naming` that names no feature is not an error — it is ignored (forward compatibility).
- **Tests:** `changing_a_sketch_dirties_the_feature_that_reads_it`; `a_static_sketch_adds_no_sketch_dependency`.
- **Acceptance:** `model.dirty_from(&[Dep::Sketch("profile".into())])` returns the indices of all features reading that sketch.
- **Risk:** SKT has not landed; the variant exists but is unconsumed until `SKT-ENT-009`. Fallback: none — this is a one-variant addition.

### MOD-SKC-006 — A parametric extrude re-solves its sketch when a parameter changes
- **State:** BLOCKED
- **Depends on:** MOD-SKC-005, SKT-SLV-001, SKT-ENT-020
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** In `run()`, when a feature holds a `Parametric` sketch and the dirty set includes a `Dep::Parameter` that the sketch reads, the feature re-solves the sketch before building. The solve result replaces the stored vector atomically (all-or-nothing).
- **Behaviour:** This is the payoff of the whole epic: change `width` from 4 to 6, and the extruded box is 6 wide because the sketch re-solved and the profile changed. The solve failure path leaves the last good solid and reports `State::Failed`.
- **Refusals:** `ModelError::Expression("the sketch did not converge: …")` on a non-converged solve; the feature's `last_good` solid survives.
- **Tests:** `changing_a_parameter_re_solves_the_sketch_and_rebuilds_the_feature`; `a_sketch_that_does_not_converge_keeps_the_last_good_solid`.
- **Acceptance:** The box volume tracks the parameter after a sketch-backed extrude.
- **Risk:** Solve cost on every rebuild. Fallback: a `solve_budget: usize` (max iterations) per feature, refusing above it.

### MOD-SKC-007 — A parametric revolve re-solves its sketch when a parameter changes
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** As `MOD-SKC-006` but for `FeatureKind::Revolve`. The revolve's angle parameter is evaluated *after* the sketch solve so the angle can depend on a sketch parameter.
- **Behaviour:** The revolve path (`crates/model/src/rebuild.rs:182–221`) evaluates `angle` from `values`, then calls `sketch.regions()`. With a parametric sketch, the sketch solve happens first and may update `values` (if a parameter is driven by a sketch expression).
- **Refusals:** As `MOD-SKC-006`, plus the existing revolve refusals.
- **Tests:** `a_parametric_revolve_follows_its_sketch_and_angle_parameters`.
- **Acceptance:** A revolve whose sketch is driven by `width` and whose angle is `width * 10` produces a solid whose volume tracks `width`.
- **Risk:** Circular dependency: sketch parameter ← feature parameter → sketch. Fallback: `ModelError::Cyclic` from the expression evaluator catches it; the feature reports `Failed`.

### MOD-SKC-008 — A failed sketch solve reports the sketch's own diagnostics
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006, SKT-DIA-001
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** When a sketch solve fails, the `Problem::message` includes the SKT diagnostic (which constraint conflicts, which is redundant). The `ModelError::Expression` message is prefixed with the feature name.
- **Behaviour:** A user who sets `width = 0` and `width = 5` on the same sketch needs to see "constraints d1 and d2 conflict", not "the sketch did not converge". The `Problem` is the UI's window into the failure.
- **Refusals:** The underlying `SketchError` is propagated unchanged; `MOD` does not invent a second error type.
- **Tests:** `a_conflicting_sketch_reports_which_constraint_failed`.
- **Acceptance:** The `Problem::message` names the constraint by name.
- **Risk:** SKT diagnostics are not stable. Fallback: forward the `SketchError` Display string verbatim.

### MOD-SKC-009 — A parametric sketch's bounds are available without a full solve
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub fn SketchSource::bounds(&self) -> Option<(Vec2, Vec2)>` — for `Static`, delegates to `sketch_bounds` (`crates/model/src/feature.rs:420`); for `Parametric`, computes bounds from the stored parameter vector without solving.
- **Behaviour:** The viewport and the UI need bounds before the model is rebuilt. A parametric sketch's bounds come from its stored vector, which is the last solved state — good enough for a preview.
- **Refusals:** `None` for an empty sketch or an unsolved parametric sketch.
- **Tests:** `a_parametric_sketch_has_bounds_without_a_solve`.
- **Acceptance:** `bounds()` never triggers a solve.
- **Risk:** Bounds from a stale vector. Fallback: mark the bounds as "last solved" in the UI.

### MOD-SKC-010 — A parametric sketch is validated at feature creation
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub fn Feature::new_parametric(name: &str, kind: FeatureKind, doc: &SketchDoc, x: &[f64]) -> Result<Feature, ModelError>` — validates that `x.len() == doc.parameter_count()`, that every value is finite, and that the sketch solves.
- **Behaviour:** A feature holding an unsolved or mismatched vector is a bug that should fail at creation, not at rebuild. This is the constructor that `SKT-UI` and the engine call.
- **Refusals:** `ModelError::Expression("the parameter vector does not match the sketch")`; `ModelError::Expression("the sketch does not solve")`.
- **Tests:** `a_feature_with_a_mismatched_parameter_vector_is_refused_at_creation`.
- **Acceptance:** No feature with a bad vector can be added to a `Model`.
- **Risk:** The solve at creation cost. Fallback: `Feature::new_parametric_unchecked` for callers that know the vector is fresh.

### MOD-SKC-011 — A cut's tool can be a parametric feature
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `FeatureKind::Cut { tool: String }` — the tool feature's `last_good` solid is used as the cutter. When the tool is parametric, its solid is re-solved on every rebuild that dirties it.
- **Behaviour:** A bore whose diameter is a parameter cuts a different hole when the parameter changes. The cut path (`crates/model/src/rebuild.rs:222–232`) already reads `model.record(tool).last_good`, so no change is needed beyond the tool's own rebuild.
- **Refusals:** The tool's `last_good` is `None` → `Err(ModelError::NoSuchFeature(tool))` (existing, `crates/model/src/rebuild.rs:224`).
- **Tests:** `a_parametric_cut_tool_follows_its_parameter`.
- **Acceptance:** Changing the tool's parameter changes the cut.
- **Risk:** None material — this is a consequence of `MOD-SKC-006`, not new work.
- **Risk:** None material.

### MOD-SKC-012 — A blend's edges can reference a parametric feature's output
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006, MOD-NAM-001
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `FeatureKind::Blend { edges: Vec<EdgeRef>, .. }` — the edges are resolved against the solid the blend operates on, which may be a parametric feature's `last_good`.
- **Behaviour:** The blend path (`crates/model/src/rebuild.rs:233–249`) already resolves `EdgeRef` against `carried`. With a parametric upstream, the carried solid changes and the blend re-resolves. The topology map (`MOD-NAM`) keeps the reference stable across parameter changes.
- **Refusals:** `ModelError::Kernel` when no edge passes through the reference point (existing, `crates/model/tests/parametric.rs:294`).
- **Tests:** `a_blend_on_a_parametric_feature_follows_the_parameter`.
- **Acceptance:** A fillet on a parametric edge tracks the edge when the parameter changes.
- **Risk:** The edge reference breaks when the parameter change alters topology. Fallback: `MOD-NAM` fingerprint resolution.

### MOD-SKC-013 — A symmetric extrude of a parametric sketch
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The symmetric extrude path (`crates/model/src/rebuild.rs:169–177`) uses the parametric profile. The two half-extrudes and their union are unchanged.
- **Behaviour:** A symmetric extrude of a parametric profile produces a symmetric solid that tracks the parameter.
- **Refusals:** As the non-symmetric path.
- **Tests:** `a_symmetric_extrude_of_a_parametric_sketch_is_symmetric`.
- **Acceptance:** The solid contains points equidistant above and below the sketch plane.
- **Risk:** None material.

### MOD-SKC-014 — A revolve's axis can be driven by a parameter
- **State:** READY
- **Depends on:** MOD-SKC-003
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `FeatureKind::Revolve { axis: Vec3, .. }` — the axis is stored as a `Vec3` and validated to lie in the sketch plane. A parametric axis would need a new field `axis_param: Option<String>`; this task adds the field and validates it at creation.
- **Behaviour:** The axis is currently a constant. Driving it by a parameter (e.g. `axis_angle`) requires the rebuild to evaluate the parameter and construct the axis. This task adds the storage; `MOD-SKC-015` adds the evaluation.
- **Refusals:** `ModelError::Expression("the revolve axis parameter does not name a parameter or a number")`.
- **Tests:** `a_revolve_with_a_stored_axis_parameter_evaluates_it`.
- **Acceptance:** The axis field round-trips through serialisation.
- **Risk:** None material — storage only.

### MOD-SKC-015 — A parametric revolve axis is evaluated at rebuild
- **State:** BLOCKED
- **Depends on:** MOD-SKC-014, MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Behaviour:** The revolve path evaluates `axis_param` from `values`, constructs the `Vec3`, and proceeds. The axis-in-plane check uses the evaluated axis.
- **Refusals:** `ModelError::Expression("the evaluated axis is not a direction")` for a zero-length axis; the existing in-plane refusal.
- **Tests:** `a_revolve_axis_follows_its_parameter`.
- **Acceptance:** Changing the axis parameter changes the revolve plane and the resulting solid.
- **Risk:** The axis leaving the sketch plane mid-rebuild. Fallback: refuse with `ModelError::Expression`.

### MOD-SKC-016 — A sketch-backed feature's dependencies include the sketch's parameters
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `FeatureKind::dependencies()` for a `Parametric` sketch returns `Dep::Parameter(name)` for every parameter the sketch reads, in addition to the existing depth/angle/size deps.
- **Behaviour:** Dirty propagation must reach the feature when any parameter the sketch reads changes. Without this, changing `width` would dirty the sketch but not the extrusion that uses it.
- **Refusals:** None — this is a pure addition to `dependencies()`.
- **Tests:** `a_parametric_feature_lists_its_sketch_parameters_as_dependencies`.
- **Acceptance:** `dirty_from(&[Dep::Parameter("width".into())])` returns the feature index.
- **Risk:** None material.

### MOD-SKC-017 — A sketch-backed feature's record stores the solved vector
- **State:** READY
- **Depends on:** MOD-SKC-004
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Record { state, last_good, solved_x: Vec<f64> }` — a new field storing the parameter vector that produced `last_good`. `#[serde(default)]` for forward compatibility.
- **Behaviour:** The record is the feature's memory. A feature that solved with `x = [4.0, 3.0]` and then failed with `x = [0.0, 0.0]` keeps the last good solid *and* the vector that produced it, so the next rebuild can retry from a known-good starting point.
- **Refusals:** Deserialisation of a record with a mismatched `solved_x` is not an error — the vector is re-solved.
- **Tests:** `a_record_stores_the_solved_vector_that_produced_its_solid`.
- **Acceptance:** The record round-trips through serialisation.
- **Risk:** The vector doubles the record size. Fallback: store only a hash and re-solve on load.

### MOD-SKC-018 — A rebuild retries a failed feature from its last good vector
- **State:** READY
- **Depends on:** MOD-SKC-017
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Behaviour:** When a feature fails, the next rebuild tries the sketch solve from `record.solved_x` (the last good vector) rather than from the current (possibly broken) vector. This is the "warm start" that makes a fix-and-retry loop converge.
- **Refusals:** The retry failure is reported as a new `Problem`; the last good solid survives.
- **Tests:** `a_failed_feature_retries_from_its_last_good_vector`.
- **Acceptance:** A parameter that causes a solve failure, when fixed, recovers without a full re-solve.
- **Risk:** The warm start may not help if the failure is structural. Fallback: none — the warm start is best-effort.

### MOD-SKC-019 — A sketch-backed feature reports its solve cost
- **State:** READY
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Rebuild { solid, problems, rebuilt, solve_ms: Vec<(String, f64)> }` — a new field recording the wall-clock milliseconds each feature's sketch solve took.
- **Behaviour:** A cost model is the first step toward incremental rebuild (`MOD-REC`). The field is populated only when a parametric sketch is solved.
- **Refusals:** None.
- **Tests:** `a_rebuild_reports_solve_cost_per_feature`.
- **Acceptance:** `solve_ms` is non-empty for a parametric feature, empty for a static one.
- **Risk:** Timing is non-deterministic. Fallback: report iterations instead of milliseconds.

### MOD-SKC-020 — A parametric sketch can be shared by multiple features
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `SketchSource::Shared(String, SketchDoc, Vec<f64>)` — a new variant where multiple features reference the same sketch by name. The sketch is solved once per rebuild and the result shared.
- **Behaviour:** Two extrudes from the same sketch (e.g. a boss and a cut) should not solve the sketch twice. The shared sketch is solved at the top of `run()` and the result cached for the duration of the rebuild.
- **Refusals:** `ModelError::Expression("the shared sketch does not solve")`.
- **Tests:** `two_features_sharing_a_sketch_solve_it_once`.
- **Acceptance:** The solve counter increments once, not twice.
- **Risk:** The shared sketch's parameters are read by both features; a change to one dirties both. Fallback: `dirty_from` already handles transitivity.

### MOD-SKC-021 — A parametric sketch's solve is cancellable
- **State:** BACKLOG
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `pub fn rebuild_cancellable(model: &mut Model, dirty: Option<&[usize]>, cancel: &AtomicBool) -> Rebuild` — checks `cancel` before each feature's solve and returns early with the features so far.
- **Behaviour:** A solve on a large sketch can take seconds. The UI needs to cancel a rebuild when the user types another parameter. This task adds the cancellation hook; the UI wiring is `UI-003`.
- **Refusals:** A cancelled rebuild returns `Rebuild { solid: <last good>, problems: <so far>, rebuilt: <so far>, cancelled: true }`.
- **Tests:** `a_cancelled_rebuild_returns_partial_results`.
- **Acceptance:** A rebuild cancelled mid-solve leaves the model in a consistent state.
- **Risk:** Cancellation mid-write. Fallback: cancel only between features, never mid-feature.

### MOD-SKC-022 — A parametric sketch's convergence tolerance is per-sketch
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `SketchSource::Parametric(SketchDoc, Vec<f64>, f64)` — a third field for the convergence tolerance, defaulting to `1e-9 * max|x|` (the SKT rule). The tolerance is stored, not recomputed.
- **Behaviour:** A sketch with large coordinates needs a looser tolerance; a sketch with tiny coordinates needs a tighter one. The stored tolerance is the one the sketch was solved with.
- **Refusals:** `ModelError::Expression("the convergence tolerance must be positive")`.
- **Tests:** `a_sketch_with_a_loose_tolerance_solves_at_that_tolerance`.
- **Acceptance:** The tolerance round-trips through serialisation.
- **Risk:** None material.

### MOD-SKC-023 — A parametric sketch's segment tolerance is stored
- **State:** READY
- **Depends on:** MOD-SKC-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `SketchSource::Parametric(SketchDoc, Vec<f64>, f64, f64)` — a fourth field for the discretisation tolerance (`seg_tol`). The profile's chord error is bounded by this value.
- **Behaviour:** The `seg_tol` determines how faceted the extruded solid is. A tight `seg_tol` gives a smooth cylinder; a loose one gives a prism. The value is stored so the same profile produces the same solid on reopen.
- **Refusals:** `ModelError::Expression("the segment tolerance must be positive")`.
- **Tests:** `a_sketch_with_a_stored_segment_tolerance_produces_the_same_facets_on_reopen`.
- **Acceptance:** The `seg_tol` round-trips.
- **Risk:** None material.

### MOD-SKC-024 — A parametric sketch's solve is instrumented for the cost model
- **State:** READY
- **Depends on:** MOD-SKC-006
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Rebuild::solve_stats: Vec<SolveStats>` where `pub struct SolveStats { pub feature: String, pub iterations: usize, pub residual: f64, pub converged: bool }`.
- **Behaviour:** The cost model (`MOD-REC`) needs data, not guesses. This task records the solver's own report per feature.
- **Refusals:** None.
- **Tests:** `a_rebuild_records_solver_statistics_per_feature`.
- **Acceptance:** `solve_stats` is populated for every parametric feature.
- **Risk:** None material.

### MOD-SKC-025 — A parametric sketch's failure leaves a diagnostic on the feature
- **State:** READY
- **Depends on:** MOD-SKC-008
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Feature { name, kind, suppressed, last_error: String }` — a new field storing the last failure message. `#[serde(default)]` for forward compatibility.
- **Behaviour:** The UI shows the feature's last error without replaying the rebuild. The field is updated on every `State::Failed`.
- **Refusals:** None.
- **Tests:** `a_failed_feature_stores_its_error_message`.
- **Acceptance:** After a failed rebuild, `feature.last_error` is non-empty and names the reason.
- **Risk:** None material.

---

## MOD-NAM — topological naming: references that survive edits

There is no topological naming scheme in CADCraft today (`crates/model/src/feature.rs:9–15`). A blend refers to an edge by a point and a tolerance; when the point stops landing on an edge, the blend says so. This epic adds the smallest honest thing: a persistent fingerprint that survives translation, rotation and uniform scaling, and a resolver that reports which strategy it used.

### MOD-NAM-001 — A `PersistentEdgeRef` that wraps `EdgeRef` with a fingerprint
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub struct PersistentEdgeRef { pub point: EdgeRef, pub fingerprint: Option<EdgeFingerprint> }` with `pub fn PersistentEdgeRef::new(point: EdgeRef) -> Self`, `pub fn to_kernel(&self) -> EdgeRef`.
- **Behaviour:** The fingerprint is captured when the blend last resolved successfully. On the next rebuild, the resolver tries the fingerprint first and falls back to the geometric point. The type is `Serialize`/`Deserialize`.
- **Refusals:** `ModelError::CorruptHistory("the fingerprint is malformed")` for a deserialisation failure.
- **Tests:** `a_persistent_edge_ref_round_trips_through_serialisation`.
- **Acceptance:** `PersistentEdgeRef` can replace `EdgeRef` in `FeatureKind::Blend` without breaking existing documents.
- **Risk:** The fingerprint is 4× the size of an `EdgeRef`. Fallback: store only when the blend resolves.

### MOD-NAM-002 — A `Resolution` enum for the load-time report
- **State:** READY
- **Depends on:** MOD-NAM-001
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `Resolution` enum (`crates/model/src/file.rs:511–528`) is already public and serialised. This task confirms it is sufficient and adds no new variants.
- **Behaviour:** The three strategies — `Fingerprint`, `Fallback`, `Unresolved` — are the honest report. A document with no blends reports "no blend references to resolve".
- **Refusals:** None.
- **Tests:** `a_load_report_names_the_resolution_strategy_per_edge`.
- **Acceptance:** The `ResolutionReport` is part of `LoadedModel`.
- **Risk:** None material.

### MOD-NAM-003 — A fingerprint is captured when a blend resolves
- **State:** READY
- **Depends on:** MOD-NAM-001
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `TopologyMap::capture` already captures from `model.records` (`crates/model/src/file.rs:414–427`). This task extends it to write the captured fingerprint back into the blend feature's `PersistentEdgeRef`.
- **Behaviour:** After a successful rebuild, every blend feature's `PersistentEdgeRef.fingerprint` is `Some(...)`. The fingerprint is the edge the blend actually resolved to, not the edge the user clicked.
- **Refusals:** A blend that failed contributes nothing (existing rule, `crates/model/src/file.rs:418–420`).
- **Tests:** `a_successful_blend_captures_its_edge_fingerprint`.
- **Acceptance:** After `rebuild()` + `save_json()`, every blend feature has a fingerprint.
- **Risk:** None material — this is a consequence of the existing `TopologyMap::capture`.

### MOD-NAM-004 — A fingerprint survives a translation, rotation and uniform scaling
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A property test: build a box, blend an edge, translate the box by `(10, -5, 3)`, rotate it 30° about Z, scale it 2×, rebuild, and assert the blend still resolves by fingerprint.
- **Behaviour:** The fingerprint's matching rule (`crates/model/src/file.rs:300–309`) compares only unit vectors and length ratios, so it is scale-free. This test proves it.
- **Refusals:** None — a test.
- **Tests:** `a_fingerprint_survives_translation_rotation_and_scaling`.
- **Acceptance:** The blend resolves with `Resolution::Fingerprint` after the transform.
- **Risk:** The test is slow (three rebuilds). Fallback: test the fingerprint matcher directly, not the full pipeline.

### MOD-NAM-005 — A fingerprint does not survive a topology change
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: build a box, blend an edge, add a rib that splits one of the two faces, rebuild, and assert the blend resolves with `Resolution::Fallback` or `Unresolved`, never a silent wrong edge.
- **Behaviour:** The fingerprint's dihedral and face-normal components change when the edge's neighbourhood changes. The resolver must not match a fingerprint that is "close enough" — the tolerance is `1e-6`, far tighter than a topology change.
- **Refusals:** None — a test.
- **Tests:** `a_fingerprint_does_not_match_a_changed_edge`.
- **Acceptance:** The blend does not silently land on a different edge.
- **Risk:** None material.

### MOD-NAM-006 — A fingerprint is not unique: the resolver refuses to guess
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `EdgeFingerprint::matches` already returns `false` when two edges match equally well (`crates/model/src/file.rs:678`). This task adds a test proving that a part with two identical edges (e.g. a box with two fillets of the same radius) does not cause the resolver to swap them.
- **Refusals:** None — a test.
- **Tests:** `two_identical_edges_do_not_swap_fingerprints`.
- **Acceptance:** The resolver reports `Fallback` for both, not `Fingerprint` for the wrong one.
- **Risk:** None material.

### MOD-NAM-007 — A `TopologyMap` is versioned independently of the document
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `TOPOLOGY_VERSION` is already separate from `FORMAT_VERSION` (`crates/model/src/file.rs:69–76`). This task adds a test that a `TopologyMap` with a future version is refused with a named error, not silently mis-read.
- **Refusals:** `ModelError::TooNew { found: TOPOLOGY_VERSION + 1, supported: TOPOLOGY_VERSION }`.
- **Tests:** `a_topology_map_from_the_future_is_refused`.
- **Acceptance:** The error names both versions.
- **Risk:** None material.

### MOD-NAM-008 — A `TopologyMap` captures from a specific solid, not the model's records
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `TopologyMap::capture_from` already exists (`crates/model/src/file.rs:433–437`). This task adds a test that the engine can capture a blend's fingerprint from the exact solid it ran on, not a reconstructed one.
- **Refusals:** None — a test.
- **Tests:** `capture_from_produces_the_same_fingerprint_as_the_rebuild_path`.
- **Acceptance:** The fingerprint from `capture_from` matches the one from `capture`.
- **Risk:** None material.

### MOD-NAM-009 — A fingerprint's world coordinates are recorded but not matched
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `EdgeFingerprint.start` and `end` are already `#[serde(skip_serializing_if = "…")]`? No — they are serialised. This task adds a comment and a test proving they are ignored by `matches`.
- **Behaviour:** The world coordinates are what a person reads when a blend lands somewhere odd. They are the first thing to go stale. The matching rule ignores them (`crates/model/src/file.rs:300–309`).
- **Refusals:** None — a test.
- **Tests:** `world_coordinates_are_recorded_but_not_matched`.
- **Acceptance:** Two fingerprints differing only in `start`/`end` match.
- **Risk:** None material.

### MOD-NAM-010 — A `BlendTopology` captures all edges or none
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `BlendTopology::capture` already returns `None` on any failure (`crates/model/src/file.rs:360–371`). This task adds a test proving a partial list is never written.
- **Refusals:** None — a test.
- **Tests:** `a_blend_with_one_unfingerprintable_edge_captures_nothing`.
- **Acceptance:** `TopologyMap::get(feature)` is `None` when any edge fails.
- **Risk:** None material.

### MOD-NAM-011 — A fingerprint's length ratio is measured against the part diagonal
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `EdgeFingerprint::of` already computes `length_ratio = length / diagonal` (`crates/model/src/file.rs:288`). This task adds a test proving two parts with the same shape but different sizes produce the same ratio.
- **Refusals:** None — a test.
- **Tests:** `two_parts_of_different_sizes_have_the_same_length_ratio`.
- **Acceptance:** The ratio is scale-free.
- **Risk:** None material.

### MOD-NAM-012 — A fingerprint's dihedral is the cosine of the face-normal angle
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `EdgeFingerprint::matches` already compares `dihedral` (`crates/model/src/file.rs:305`). This task adds a test proving a 90° edge and a 45° edge do not match.
- **Refusals:** None — a test.
- **Tests:** `a_90_degree_edge_does_not_match_a_45_degree_edge`.
- **Acceptance:** The dihedral distinguishes the two.
- **Risk:** None material.

### MOD-NAM-013 — A fingerprint's `at` is an unordered pair
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `close_unordered` already compares the two face entries in either order (`crates/model/src/file.rs:338–341`). This task adds a test proving that swapping the two faces still matches.
- **Refusals:** None — a test.
- **Tests:** `swapping_the_two_faces_still_matches`.
- **Acceptance:** The match is order-independent.
- **Risk:** None material.

### MOD-NAM-014 — A fingerprint's `square` is compared as an unordered pair
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `close_unordered` is used for `square` too (`crates/model/src/file.rs:306`). This task adds a test proving that an edge with `square = [0.0, 1.0]` matches one with `square = [1.0, 0.0]`.
- **Refusals:** None — a test.
- **Tests:** `square_pairs_match_in_either_order`.
- **Acceptance:** The match is order-independent.
- **Risk:** None material.

### MOD-NAM-015 — A fingerprint's finiteness check matches nothing
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `EdgeFingerprint::finite` already returns `false` for any non-finite component (`crates/model/src/file.rs:313–316`). This task adds a test proving a fingerprint with a NaN length ratio matches nothing.
- **Refusals:** None — a test.
- **Tests:** `a_fingerprint_with_a_nan_matches_nothing`.
- **Acceptance:** The match returns `false`.
- **Risk:** None material.

### MOD-NAM-016 — A `ResolutionReport` summarises per feature
- **State:** READY
- **Depends on:** MOD-NAM-002
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `ResolutionReport::summary` already exists (`crates/model/src/file.rs:607–632`). This task adds a test proving the summary line names the feature and the counts.
- **Refusals:** None — a test.
- **Tests:** `a_resolution_report_summarises_per_feature`.
- **Acceptance:** The summary contains the feature name and the three counts.
- **Risk:** None material.

### MOD-NAM-017 — A `ResolutionReport` distinguishes "all fingerprints" from "nothing to resolve"
- **State:** READY
- **Depends on:** MOD-NAM-002
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `ResolutionReport::all_fingerprints` already returns `false` for an empty report (`crates/model/src/file.rs:581–583`). This task adds a test proving the distinction.
- **Refusals:** None — a test.
- **Tests:** `all_fingerprints_is_false_when_there_are_no_blends`.
- **Acceptance:** The empty report does not claim success.
- **Risk:** None material.

### MOD-NAM-018 — A load with no topology costs exactly one rebuild
- **State:** READY
- **Depends on:** MOD-NAM-002
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `Document::into_model_with_report` already rebuilds twice only when a reference was re-pointed (`crates/model/src/file.rs:126–136`). This task adds a test proving a document with no `topology` key rebuilds once.
- **Refusals:** None — a test.
- **Tests:** `a_document_with_no_topology_rebuilds_once`.
- **Acceptance:** The rebuild counter is 1.
- **Risk:** None material.

### MOD-NAM-019 — A load with a re-pointed reference rebuilds twice
- **State:** READY
- **Depends on:** MOD-NAM-018
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** A document with a topology map that re-points a reference triggers a second rebuild. This task adds a test proving the second rebuild happens and the resolution is `Fingerprint`.
- **Refusals:** None — a test.
- **Tests:** `a_re-pointed_reference_triggers_a_second_rebuild`.
- **Acceptance:** The rebuild counter is 2 and the resolution is `Fingerprint`.
- **Risk:** None material.

### MOD-NAM-020 — A `TopologyMap` is captured on save, not on rebuild
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Model::save_json` already calls `capture_topology` (`crates/model/src/file.rs:177–179`). This task adds a test proving the map is fresh (captures from the current records).
- **Refusals:** None — a test.
- **Tests:** `a_saved_topology_map_reflects_the_current_records`.
- **Acceptance:** A parameter change + save produces a new map.
- **Risk:** None material.

### MOD-NAM-021 — A `TopologyMap` is mergeable across branches
- **State:** BACKLOG
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `pub fn TopologyMap::merge(&mut self, other: &TopologyMap) -> Result<(), ModelError>` — for a version-control merge, two maps written from different branches are merged by feature name; a conflict (both sides changed the same feature) is a named `Err`.
- **Behaviour:** A document is a text file; two branches editing the same blend produce two different fingerprints. The merge must not silently pick one.
- **Refusals:** `ModelError::Expression("feature {name} has conflicting fingerprints on both branches")`.
- **Tests:** `a_merge_with_conflicting_fingerprints_is_refused`.
- **Acceptance:** A clean merge succeeds; a conflicting merge names the feature.
- **Risk:** None material.

### MOD-NAM-022 — A fingerprint's capture is all-or-nothing per feature
- **State:** READY
- **Depends on:** MOD-NAM-010
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `BlendTopology::capture` already returns `None` on any failure. This task adds a test proving a feature with three edges where only one fails captures nothing.
- **Refusals:** None — a test.
- **Tests:** `a_partial_capture_writes_nothing`.
- **Acceptance:** The map has no entry for the feature.
- **Risk:** None material.

### MOD-NAM-023 — A `PersistentEdgeRef` falls back to geometry when the fingerprint is absent
- **State:** READY
- **Depends on:** MOD-NAM-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Behaviour:** A document written before fingerprints existed has `None` in every `PersistentEdgeRef`. The resolver falls back to the geometric point. This task adds a test proving the fallback works.
- **Refusals:** None — a test.
- **Tests:** `a_blend_without_a_fingerprint_falls_back_to_geometry`.
- **Acceptance:** The blend resolves with `Resolution::Fallback`.
- **Risk:** None material.

### MOD-NAM-024 — A fingerprint's tolerance is looser than the kernel's welding tolerance
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Behaviour:** `POSITION = 1e-6` and `ANGLE = 1e-6` (`crates/model/src/file.rs:228–229`) are far looser than the kernel's `(diag * 1e-9).clamp(1e-12, 1e-3)`. This task adds a test proving a fingerprint still matches after a small kernel-tolerance move.
- **Refusals:** None — a test.
- **Tests:** `a_fingerprint_survives_a_small_kernel_tolerance_move`.
- **Acceptance:** The match succeeds.
- **Risk:** None material.

### MOD-NAM-025 — A `TopologyMap` is optional and self-describing
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Document::topology` already has `#[serde(default)]` (`crates/model/src/file.rs:93`). This task adds a test proving a document with no `topology` key loads with an empty map.
- **Refusals:** None — a test.
- **Tests:** `a_document_without_a_topology_key_loads_with_an_empty_map`.
- **Acceptance:** The map is empty and the document loads.
- **Risk:** None material.

---

## MOD-HIS — history: undo/redo, feature-tree operations

A `Model` is an ordered list of features with parameters. History is the operation graph that produced it. This epic adds undo/redo as tree operations, not a stack of solids.

### MOD-HIS-001 — A `History` struct that records operations
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/history.rs` (MOD, new)
- **Contract:** `pub struct History { pub undo: Vec<Operation>, pub redo: Vec<Operation> }` with `pub enum Operation { SetParameter(String, Param), AddFeature(Feature), RemoveFeature(String, Vec<Feature>), ReplaceFeature(String, FeatureKind), SuppressFeature(String, bool) }`.
- **Behaviour:** Every mutation to a `Model` is recorded as an `Operation`. The `History` is not serialised — it is session state. The model itself is the source of truth.
- **Refusals:** None.
- **Tests:** `an_operation_stack_records_every_mutation`.
- **Acceptance:** After a sequence of mutations, `undo.len()` equals the number of operations.
- **Risk:** None material.

### MOD-HIS-002 — `Model::set` records an undo operation
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/feature.rs` (MOD), `crates/model/src/history.rs` (MOD)
- **Contract:** `Model::set` takes `&mut History` (or a closure) and pushes `Operation::SetParameter(name, old_param)`. The old value is stored so undo can restore it.
- **Behaviour:** The signature change is the cost. Every caller (`engine`, `CLI`, `MCP`) passes `&mut History`. The 18 existing tests pass with a dummy history.
- **Refusals:** As the existing `Model::set` (`crates/model/src/feature.rs:273–282`).
- **Tests:** `setting_a_parameter_records_an_undo_operation`.
- **Acceptance:** After `set`, `undo` pops the old value and restores it.
- **Risk:** The signature change ripples to every caller. Fallback: a `Model::set_with_history(&mut self, history: &mut History, name, param)` that leaves `set` unchanged.

### MOD-HIS-003 — `Model::add` records an undo operation
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/feature.rs` (MOD), `crates/model/src/history.rs` (MOD)
- **Contract:** `Model::add` pushes `Operation::AddFeature(feature)`.
- **Behaviour:** Undo removes the feature by name.
- **Refusals:** As the existing `Model::add` (`crates/model/src/feature.rs:284–294`).
- **Tests:** `adding_a_feature_records_an_undo_operation`.
- **Acceptance:** After `add` + undo, the feature is gone.
- **Risk:** None material.

### MOD-HIS-004 — `Model::replace` records an undo operation
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/feature.rs` (MOD), `crates/model/src/history.rs` (MOD)
- **Contract:** `Model::replace` pushes `Operation::ReplaceFeature(name, old_kind)`.
- **Behaviour:** Undo restores the old `FeatureKind`.
- **Refusals:** As the existing `Model::replace` (`crates/model/src/feature.rs:297–303`).
- **Tests:** `replacing_a_feature_records_an_undo_operation`.
- **Acceptance:** After `replace` + undo, the kind is the original.
- **Risk:** None material.

### MOD-HIS-005 — `Model::remove` records an undo operation
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/feature.rs` (MOD), `crates/model/src/history.rs` (MOD)
- **Contract:** `Model::remove` pushes `Operation::RemoveFeature(name, removed)` where `removed` is the tail that was truncated (existing return value, `crates/model/src/feature.rs:309–317`).
- **Behaviour:** Undo re-inserts the tail at the same position.
- **Refusals:** As the existing `Model::remove`.
- **Tests:** `removing_a_feature_records_an_undo_operation_that_restores_the_tail`.
- **Acceptance:** After `remove` + undo, the tail is back in order.
- **Risk:** None material.

### MOD-HIS-006 — `History::undo` pops and reverses one operation
- **State:** READY
- **Depends on:** MOD-HIS-002, MOD-HIS-003, MOD-HIS-004, MOD-HIS-005
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn History::undo(&mut self, model: &mut Model) -> Result<(), ModelError>` — pops the last operation, applies its inverse, and pushes it to `redo`.
- **Behaviour:** The inverse of `SetParameter` is `SetParameter(name, old)`; the inverse of `AddFeature` is `RemoveFeature`; the inverse of `RemoveFeature` is `AddFeature` (re-inserting the tail); the inverse of `ReplaceFeature` is `ReplaceFeature(name, old_kind)`.
- **Refusals:** `ModelError::NoSuchFeature` when the inverse cannot be applied (the model was mutated outside the history).
- **Tests:** `undo_reverses_each_operation_kind`.
- **Acceptance:** After a mutation + undo, the model is byte-identical to before the mutation.
- **Risk:** The model being mutated outside the history (e.g. by a direct `model.features.push`). Fallback: a `History::is_valid(&Model) -> bool` check.

### MOD-HIS-007 — `History::redo` re-applies one operation
- **State:** READY
- **Depends on:** MOD-HIS-006
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn History::redo(&mut self, model: &mut Model) -> Result<(), ModelError>`.
- **Behaviour:** Redo pops from `redo` and re-applies the operation.
- **Refusals:** As `undo`.
- **Tests:** `redo_re_applies_the_undone_operation`.
- **Acceptance:** After undo + redo, the model is byte-identical to after the original mutation.
- **Risk:** None material.

### MOD-HIS-008 — A new mutation clears the redo stack
- **State:** READY
- **Depends on:** MOD-HIS-006
- **Files:** `crates/model/src/history.rs` (MOD)
- **Behaviour:** After an undo, a new mutation clears `redo` — the standard undo/redo contract.
- **Refusals:** None.
- **Tests:** `a_new_mutation_clears_the_redo_stack`.
- **Acceptance:** After undo + new mutation, `redo` is empty.
- **Risk:** None material.

### MOD-HIS-009 — A `History` has a configurable depth limit
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `History { undo: Vec<Operation>, redo: Vec<Operation>, limit: usize }` with `pub const DEFAULT_HISTORY_LIMIT: usize = 100`. When `undo.len() > limit`, the oldest operation is dropped.
- **Behaviour:** A long editing session would otherwise grow the history without bound. The limit is a soft cap, not a hard one — the oldest operation is dropped, not refused.
- **Refusals:** None.
- **Tests:** `a_history_beyond_its_limit_drops_the_oldest_operation`.
- **Acceptance:** After 101 mutations with limit 100, `undo.len() == 100`.
- **Risk:** Losing the oldest undo. Fallback: a warning in the UI.

### MOD-HIS-010 — A `History` is snapshot-based for the model, not the solids
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation` stores `Feature` (which is `Clone`), not `Solid`. The solids are rebuilt on undo, not stored.
- **Behaviour:** Storing solids would double the memory for no benefit — the rebuild is deterministic. The history stores the *intent*, not the result.
- **Refusals:** None.
- **Tests:** `a_history_operation_stores_no_solid`.
- **Acceptance:** `Operation` has no `Solid` field.
- **Risk:** None material.

### MOD-HIS-011 — A `History` supports transaction grouping
- **State:** BACKLOG
- **Depends on:** MOD-HIS-006
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn History::group(&mut self, ops: Vec<Operation>)` — pushes a group that is undone/redone as one unit.
- **Behaviour:** A single user action (e.g. "delete a feature and its dependents") may produce several operations. The user expects one undo, not five.
- **Refusals:** None.
- **Tests:** `a_grouped_transaction_undoes_as_one_unit`.
- **Acceptance:** One undo reverses all the operations in the group.
- **Risk:** None material.

### MOD-HIS-012 — A `History` labels groups for the UI
- **State:** BACKLOG
- **Depends on:** MOD-HIS-011
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation::Group(String, Vec<Operation>)` — the label is the UI's "Delete feature and dependents".
- **Behaviour:** The undo menu shows the label, not the raw operation.
- **Refusals:** None.
- **Tests:** `a_grouped_transaction_has_a_label_the_ui_can_show`.
- **Acceptance:** The label round-trips.
- **Risk:** None material.

### MOD-HIS-013 — A `History` is serialisable for crash recovery
- **State:** BACKLOG
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation` derives `Serialize`/`Deserialize`. A crashed session's history can be written to a sidecar file and replayed on next start.
- **Behaviour:** The history is session state, but a crash loses it. A sidecar `.ccm.hist` file lets the next session offer "undo the last session's edits".
- **Refusals:** `ModelError::CorruptHistory("the history sidecar is corrupt")`.
- **Tests:** `a_history_sidecar_round_trips_through_serialisation`.
- **Acceptance:** A history written to a sidecar and read back is identical.
- **Risk:** The sidecar is stale. Fallback: a timestamp check.

### MOD-HIS-014 — A `History` is cleared on document load
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `History::clear(&mut self)` — called when a new document is loaded, so the old document's undo stack does not leak into the new one.
- **Behaviour:** Loading a document is a new session; the undo stack starts empty.
- **Refusals:** None.
- **Tests:** `loading_a_document_clears_the_history`.
- **Acceptance:** After `load_json` + `clear`, `undo` and `redo` are empty.
- **Risk:** None material.

### MOD-HIS-015 — A `History` is per-document, not global
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** The `History` is stored on the `Session` (engine), not on the `Model`. Two documents open in two tabs have independent undo stacks.
- **Behaviour:** A global undo stack would let an undo in tab A corrupt tab B.
- **Refusals:** None.
- **Tests:** `two_documents_have_independent_histories`.
- **Acceptance:** An undo in document A does not affect document B.
- **Risk:** None material.

### MOD-HIS-016 — A `History` operation can be inverted without a model reference
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn Operation::inverse(&self) -> Operation` — the inverse is a pure function of the operation, not of the model.
- **Behaviour:** `RemoveFeature`'s inverse is `AddFeature` (re-inserting the tail); the tail is stored in the operation. No model lookup is needed.
- **Refusals:** None.
- **Tests:** `every_operation_has_a_pure_inverse`.
- **Acceptance:** `Operation::inverse` is a total function.
- **Risk:** None material.

### MOD-HIS-017 — A `History` validates that the model matches before undo
- **State:** READY
- **Depends on:** MOD-HIS-006
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `History::undo` checks that the model's current state matches the operation's expected post-state. If not, it returns `Err(ModelError::Expression("the model has changed outside the history"))`.
- **Behaviour:** A model mutated by a script (not through the history) would leave the undo stack pointing at the wrong state. The check catches it.
- **Refusals:** `ModelError::Expression("the model has changed outside the history")`.
- **Tests:** `undo_refuses_when_the_model_changed_outside_the_history`.
- **Acceptance:** An undo after an out-of-band mutation is refused.
- **Risk:** The check is O(n) over features. Fallback: a hash.

### MOD-HIS-018 — A `History` is bounded by memory, not just count
- **State:** BACKLOG
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `History { undo: Vec<Operation>, redo: Vec<Operation>, limit: usize, max_bytes: usize }` with `DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024`. When the total serialised size exceeds `max_bytes`, the oldest operations are dropped.
- **Behaviour:** A `Model` with 10,000 features has a `Feature` that is ~1 KB. 100 operations is 100 KB — fine. 10,000 operations is 10 MB — too much.
- **Refusals:** None.
- **Tests:** `a_history_beyond_its_memory_limit_drops_the_oldest_operations`.
- **Acceptance:** After enough mutations, `undo.len()` drops below the count limit.
- **Risk:** None material.

### MOD-HIS-019 — A `History` supports named checkpoints
- **State:** BACKLOG
- **Depends on:** MOD-HIS-006
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn History::checkpoint(&mut self, name: &str)` — records the current model state as a named checkpoint. `pub fn History::restore(&mut self, model: &mut Model, name: &str) -> Result<(), ModelError>`.
- **Behaviour:** Before a risky operation (a boolean that might fail), the user can set a checkpoint and return to it without undoing the whole session.
- **Refusals:** `ModelError::NoSuchFeature("checkpoint {name} does not exist")` — or a new `NoSuchCheckpoint(String)` variant.
- **Tests:** `a_checkpoint_can_be_restored`.
- **Acceptance:** After mutations + restore, the model matches the checkpoint.
- **Risk:** The checkpoint stores a full `Model` clone. Fallback: store only the delta.

### MOD-HIS-020 — A `History` is observable by the UI
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `pub fn History::can_undo(&self) -> bool`, `pub fn History::can_redo(&self) -> bool`, `pub fn History::undo_label(&self) -> Option<&str>`, `pub fn History::redo_label(&self) -> Option<&str>`.
- **Behaviour:** The UI greys out the undo button when `can_undo()` is false and shows the label when it is true.
- **Refusals:** None.
- **Tests:** `the_undo_label_names_the_operation_that_will_be_reversed`.
- **Acceptance:** The label is the operation's Display string.
- **Risk:** None material.

### MOD-HIS-021 — A `History` records a parameter rename
- **State:** BACKLOG
- **Depends on:** MOD-HIS-002
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation::RenameParameter(String, String)` — from and to. The rename is a single undoable operation, not a delete + add.
- **Behaviour:** Renaming `width` to `overall_width` should undo as one step and should not break features that read `width` (they are updated in the same operation).
- **Refusals:** `ModelError::DuplicateParameter(to)` when the new name already exists.
- **Tests:** `a_parameter_rename_is_one_undoable_operation`.
- **Acceptance:** After rename + undo, the old name is back and the features read it again.
- **Risk:** Features that read the old name must be updated. Fallback: a scan of all features' dependencies.

### MOD-HIS-022 — A `History` records a feature rename
- **State:** BACKLOG
- **Depends on:** MOD-HIS-003
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation::RenameFeature(String, String)` — from and to.
- **Behaviour:** Renaming a feature updates every `Cut { tool }` and `Dep::Feature` that references it. The rename is one undoable operation.
- **Refusals:** `ModelError::DuplicateFeature(to)` when the new name already exists.
- **Tests:** `a_feature_rename_updates_every_reference`.
- **Acceptance:** After rename + undo, the old name is back and the references are restored.
- **Risk:** Missing a reference. Fallback: a scan of all features' `dependencies()`.

### MOD-HIS-023 — A `History` records a feature reorder
- **State:** BACKLOG
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation::ReorderFeature(usize, usize)` — from index to index.
- **Behaviour:** Moving a feature up or down in the tree is one undoable operation. The reorder changes the build order and therefore the resulting solid.
- **Refusals:** `ModelError::Expression("the index is out of range")`.
- **Tests:** `a_feature_reorder_is_one_undoable_operation`.
- **Acceptance:** After reorder + undo, the feature is back at its original index.
- **Risk:** None material.

### MOD-HIS-024 — A `History` records a suppress toggle
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Operation::SuppressFeature(String, bool)` — the new suppressed state. Undo restores the old state.
- **Behaviour:** Suppressing a feature (`feature.suppressed = true`) is a UI action that should be undoable.
- **Refusals:** None.
- **Tests:** `a_suppress_toggle_is_one_undoable_operation`.
- **Acceptance:** After suppress + undo, the feature is unsuppressed.
- **Risk:** None material.

### MOD-HIS-025 — A `History` is not recorded for read-only operations
- **State:** READY
- **Depends on:** MOD-HIS-001
- **Files:** `crates/model/src/history.rs` (MOD)
- **Contract:** `Model::values()`, `Model::value()`, `Model::solid()`, `Model::feature()`, `Model::record()` — none of these take `&mut History` and none record an operation.
- **Behaviour:** Reading the model is not an operation and should not pollute the undo stack.
- **Refusals:** None.
- **Tests:** `reading_the_model_records_no_operation`.
- **Acceptance:** After any number of reads, `undo.is_empty()`.
- **Risk:** None material.

---

## MOD-BOD — multi-body parts: multiple solids in one part

Today a `Model` produces one solid. A real part has multiple bodies (a plate with a separate boss, a weldment with disjoint members). This epic adds multi-body support.

### MOD-BOD-001 — A `Body` struct that wraps a `Solid` with a name and a state
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/body.rs` (MOD, new)
- **Contract:** `pub struct Body { pub name: String, pub solid: Solid, pub state: BodyState }` with `pub enum BodyState { Built, Failed(String), Blocked(String) }`.
- **Behaviour:** A body is the multi-body equivalent of a `Record`. It carries the solid and the state. The `Model` gains `bodies: Vec<Body>`.
- **Refusals:** `ModelError::DuplicateFeature("a body with this name already exists")`.
- **Tests:** `a_body_wraps_a_solid_with_a_name_and_state`.
- **Acceptance:** `Body` is `Clone`, `Debug`, `PartialEq`, `Serialize`, `Deserialize`.
- **Risk:** None material.

### MOD-BOD-002 — A `Model` holds multiple bodies
- **State:** READY
- **Depends on:** MOD-BOD-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Model { parameters, features, records, bodies: Vec<Body> }` with `#[serde(default)]` on `bodies`. `pub fn Model::body(&self, name: &str) -> Option<&Body>`, `pub fn bodies(&self) -> &[Body]`.
- **Behaviour:** A model with no multi-body features has `bodies.is_empty()`. The existing `solid()` method returns the first body's solid for backward compatibility.
- **Refusals:** `ModelError::NoSuchFeature("no body with this name")` for a missing body.
- **Tests:** `a_model_with_no_multi_body_features_has_no_bodies`.
- **Acceptance:** The 18 existing tests pass unchanged.
- **Risk:** The `solid()` fallback hides the multi-body nature. Fallback: a deprecation comment.

### MOD-BOD-003 — A `FeatureKind::NewBody` that starts a new body
- **State:** READY
- **Depends on:** MOD-BOD-002
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `FeatureKind::NewBody { sketch: SketchSource }` — starts a new body from a sketch. The body's name is `format!("Body{}", bodies.len() + 1)`.
- **Behaviour:** The first body is the "base" body; subsequent `NewBody` features are additional bodies. The build creates a new `Body` and appends it.
- **Refusals:** As `Extrude` with a `Static` sketch.
- **Tests:** `a_new_body_feature_creates_a_second_body`.
- **Acceptance:** After a `NewBody` feature, `model.bodies().len() == 2`.
- **Risk:** None material.

### MOD-BOD-004 — A `FeatureKind::Boolean` that combines bodies
- **State:** BLOCKED
- **Depends on:** MOD-BOD-003, KER-BOOL-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `FeatureKind::Boolean { target: String, tool: String, op: Bool3 }` — `target` and `tool` are body names; `op` is `Union`, `Difference`, or `Intersect`.
- **Behaviour:** The build looks up the two bodies by name, applies the boolean, and replaces the target body's solid. The tool body is unchanged.
- **Refusals:** `ModelError::NoSuchFeature("no body with this name")`; `ModelError::Kernel` for a boolean failure.
- **Tests:** `a_boolean_combines_two_bodies`.
- **Acceptance:** After a `Union`, the target body's solid contains both bodies' material.
- **Risk:** The kernel's boolean is not robust enough (`KER-BOOL-001..003`). Fallback: refuse with a named error.

### MOD-BOD-005 — A body's dependencies include the bodies it reads
- **State:** READY
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Dep::Body(String)` — a new variant. `FeatureKind::dependencies()` returns it for `Boolean` features.
- **Behaviour:** Changing body A dirties every feature that reads body A as a target or tool.
- **Refusals:** None.
- **Tests:** `changing_a_body_dirties_the_features_that_read_it`.
- **Acceptance:** `dirty_from(&[Dep::Body("Body1".into())])` returns the boolean feature's index.
- **Risk:** None material.

### MOD-BOD-006 — A body's state is `Blocked` when an upstream body fails
- **State:** READY
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/src/body.rs` (MOD)
- **Behaviour:** If body A fails, every feature that reads body A is `Blocked`, and the bodies they produce are `Blocked`. The last good solid of each body survives.
- **Refusals:** None — this is the state propagation rule.
- **Tests:** `a_body_above_a_failure_is_blocked_not_broken`.
- **Acceptance:** The body's `state` is `Blocked` and its `solid` is the last good one.
- **Risk:** None material.

### MOD-BOD-007 — A body can be suppressed
- **State:** READY
- **Depends on:** MOD-BOD-001
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `Body { name, solid, state, suppressed: bool }` with `#[serde(default)]`.
- **Behaviour:** A suppressed body is hidden from the viewport but kept in the model. The rebuild skips it.
- **Refusals:** None.
- **Tests:** `a_suppressed_body_is_skipped_by_the_rebuild`.
- **Acceptance:** After suppressing, the body is not in the output solid.
- **Risk:** None material.

### MOD-BOD-008 — A body can be deleted, taking its dependents with it
- **State:** READY
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::remove_body(&mut self, name: &str) -> Result<Vec<String>, ModelError>` — returns the names of the features and bodies that were removed.
- **Behaviour:** Deleting a body removes every feature that reads it, and every body that depends on those features. The tail goes with it, exactly as `Model::remove` does for features (`crates/model/src/feature.rs:309–317`).
- **Refusals:** `ModelError::NoSuchFeature("no body with this name")`.
- **Tests:** `deleting_a_body_takes_its_dependents_with_it`.
- **Acceptance:** After `remove_body`, the body and its dependents are gone.
- **Risk:** None material.

### MOD-BOD-009 — A multi-body model's shown solid is the union of all bodies
- **State:** READY
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `Model::solid()` returns `Option<Solid>` — the union of all non-suppressed bodies' solids, or `None` if there are none.
- **Behaviour:** The viewport shows the union. The union is computed once per rebuild and cached.
- **Refusals:** `None` for an empty model.
- **Tests:** `a_multi_body_model_shows_the_union_of_its_bodies`.
- **Acceptance:** `solid().volume()` equals the sum of the bodies' volumes (for disjoint bodies).
- **Risk:** The union is O(n²) for n bodies. Fallback: a lazy union.

### MOD-BOD-010 — A multi-body model's bounds are the union of all bodies' bounds
- **State:** READY
- **Depends on:** MOD-BOD-009
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub fn Model::bounds(&self) -> Option<Bounds3>` — the union of all bodies' bounds.
- **Behaviour:** The viewport fit needs the bounds of everything, not just the first body.
- **Refusals:** `None` for an empty model.
- **Tests:** `a_multi_model_bounds_cover_all_bodies`.
- **Acceptance:** The bounds contain every body.
- **Risk:** None material.

### MOD-BOD-011 — A body's topology map entry is keyed by body name
- **State:** READY
- **Depends on:** MOD-BOD-004, MOD-NAM-001
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `TopologyMap::features` gains a body-name prefix for multi-body features. The key is `format!("{body_name}/{feature_name}")`.
- **Behaviour:** A fillet on body A and a fillet on body B must not collide in the topology map.
- **Refusals:** None.
- **Tests:** `two_bodies_with_the_same_feature_name_do_not_collide_in_the_topology_map`.
- **Acceptance:** The map has two entries.
- **Risk:** None material.

### MOD-BOD-012 — A multi-body document round-trips through serialisation
- **State:** READY
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: build a two-body model, save, reopen, and assert the bodies and their solids are identical.
- **Refusals:** None — a test.
- **Tests:** `a_multi_body_model_round_trips_through_serialisation`.
- **Acceptance:** The reopened model has the same bodies.
- **Risk:** None material.

### MOD-BOD-013 — A body's mass properties are computed across all bodies
- **State:** READY
- **Depends on:** MOD-BOD-009
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::mass_properties(&self) -> Option<MassProperties>` — the combined mass properties of all bodies.
- **Behaviour:** The simulation and CAM domains need the total mass, centre of mass and inertia tensor. The computation sums the per-body values.
- **Refusals:** `None` for an empty model.
- **Tests:** `a_multi_body_models_mass_properties_are_the_sum_of_its_bodies`.
- **Acceptance:** The total mass equals the sum.
- **Risk:** None material.

### MOD-BOD-014 — A body can be renamed without breaking references
- **State:** BACKLOG
- **Depends on:** MOD-BOD-005
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::rename_body(&mut self, from: &str, to: &str) -> Result<(), ModelError>` — updates every `Dep::Body` and `FeatureKind::Boolean` that references the old name.
- **Behaviour:** A rename is one undoable operation (see `MOD-HIS-021`).
- **Refusals:** `ModelError::DuplicateFeature("a body with this name already exists")`.
- **Tests:** `a_body_rename_updates_every_reference`.
- **Acceptance:** After rename, no reference to the old name remains.
- **Risk:** Missing a reference. Fallback: a scan of all features' `dependencies()`.

### MOD-BOD-015 — A body's feature can be reordered within its body
- **State:** BACKLOG
- **Depends on:** MOD-BOD-004
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `Operation::ReorderBodyFeature(String, usize, usize)` — reorder a feature within one body's local feature list.
- **Behaviour:** A body's features are a sub-list of the model's features. Reordering within the sub-list changes the build order for that body only.
- **Refusals:** `ModelError::Expression("the index is out of range")`.
- **Tests:** `a_body_feature_reorder_changes_only_that_body`.
- **Acceptance:** After reorder, only the target body's solid changes.
- **Risk:** None material.

### MOD-BOD-016 — A body's feature list is derived from the model's feature list
- **State:** READY
- **Depends on:** MOD-BOD-003
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::body_features(&self, body: &str) -> Vec<&Feature>` — the features that belong to a body, in order.
- **Behaviour:** A body's features are those between its `NewBody` feature and the next `NewBody` feature (or the end of the list). The derivation is O(n) and cached.
- **Refusals:** `None` for an unknown body.
- **Tests:** `a_body_features_are_derived_from_the_model_feature_list`.
- **Acceptance:** The derived list matches the expected slice.
- **Risk:** None material.

### MOD-BOD-017 — A multi-body rebuild is incremental per body
- **State:** BLOCKED
- **Depends on:** MOD-BOD-005, MOD-REC-005
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Behaviour:** When body A changes, only body A's features are rebuilt. Body B's features are skipped because their `Dep::Body` is not dirty.
- **Refusals:** None.
- **Tests:** `changing_one_body_rebuilds_only_that_body`.
- **Acceptance:** `Rebuild::rebuilt` counts only the dirty body's features.
- **Risk:** The `dirty_from` walk does not yet understand body dependencies. Fallback: a per-body dirty filter.

### MOD-BOD-018 — A body's failure does not block other bodies
- **State:** READY
- **Depends on:** MOD-BOD-006
- **Files:** `crates/model/src/body.rs` (MOD)
- **Behaviour:** If body A fails, body B is still rebuilt. The failure is scoped to body A's dependency graph.
- **Refusals:** None.
- **Tests:** `a_failing_body_does_not_block_other_bodies`.
- **Acceptance:** Body B is `Built` while body A is `Failed`.
- **Risk:** None material.

### MOD-BOD-019 — A multi-body model's BOM lists all bodies
- **State:** BACKLOG
- **Depends on:** MOD-BOD-009
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::bom(&self) -> Vec<BomEntry>` where `pub struct BomEntry { pub name: String, pub volume: f64, pub bounds: Bounds3 }`.
- **Behaviour:** The bill of materials is a table of bodies with their volumes and bounds. The drawing domain (`DRW-SHE`) renders it.
- **Refusals:** None.
- **Tests:** `a_multi_body_models_bom_lists_all_bodies`.
- **Acceptance:** The BOM has one entry per body.
- **Risk:** None material.

### MOD-BOD-020 — A body's interference is checked against other bodies
- **State:** BLOCKED
- **Depends on:** MOD-BOD-004, KER-TOL-002
- **Files:** `crates/model/src/body.rs` (MOD)
- **Contract:** `pub fn Model::interference(&self) -> Result<Vec<(String, String, Vec3)>, ModelError>` — for each pair of bodies, the interference points at which they overlap.
- **Behaviour:** Two bodies that overlap in space are a manufacturing defect. The check uses the kernel's interference detection (`KER-TOL-002`).
- **Refusals:** `ModelError::Kernel` for a kernel failure.
- **Tests:** `two_overlapping_bodies_are_detected`.
- **Acceptance:** The interference list names both bodies and the overlap point.
- **Risk:** `KER-TOL-002` is not done. Fallback: a bounding-box pre-check.

---

## MOD-REC — rebuild: partial, on-demand, with a cost model

The existing `rebuild()` rebuilds everything or only what `dirty` says. This epic adds partial rebuild, on-demand rebuild, and a documented cost model.

### MOD-REC-001 — A `Rebuild` reports which features were skipped
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Rebuild { solid, problems, rebuilt, skipped: Vec<String> }` — a new field listing the features that were not rebuilt (because they were clean).
- **Behaviour:** The cost model needs to know what was skipped. The field is populated by `run()`.
- **Refusals:** None.
- **Tests:** `a_rebuild_reports_which_features_were_skipped`.
- **Acceptance:** After a partial rebuild, `skipped` lists the clean features.
- **Risk:** None material.

### MOD-REC-002 — A `Rebuild` reports the total wall-clock time
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Rebuild { …, elapsed_ms: f64 }`.
- **Behaviour:** The rebuild's total time is the headline number for the cost model.
- **Refusals:** None.
- **Tests:** `a_rebuild_reports_its_total_time`.
- **Acceptance:** `elapsed_ms > 0` for any non-trivial rebuild.
- **Risk:** Timing is non-deterministic. Fallback: report feature count instead.

### MOD-REC-003 — A rebuild is incremental when `dirty` is non-empty
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Behaviour:** `run()` already skips features not in `dirty` (`crates/model/src/rebuild.rs:73–78`). This task adds a test proving the skip is correct and that the skipped feature's `last_good` solid is carried through.
- **Refusals:** None — a test.
- **Tests:** `an_incremental_rebuild_skips_clean_features_and_carries_their_solids`.
- **Acceptance:** The skipped feature's record is unchanged.
- **Risk:** None material.

### MOD-REC-004 — A rebuild is atomic: either all dirty features rebuild or none do
- **State:** READY
- **Depends on:** MOD-REC-003
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Behaviour:** The existing `run()` applies each feature's result immediately. An atomic rebuild would apply all results only if every dirty feature succeeds. This task adds a `rebuild_atomic()` that stages the results and commits them at the end.
- **Refusals:** A failure in any dirty feature leaves all records unchanged.
- **Tests:** `an_atomic_rebuild_leaves_no_partial_state_on_failure`.
- **Acceptance:** After a failed atomic rebuild, every record is as it was before.
- **Risk:** Staging doubles the memory for the duration of the rebuild. Fallback: stage only the changed records.

### MOD-REC-005 — A rebuild is deterministic: the same inputs produce the same outputs
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** A test: build a model twice from the same parameters and features, and assert the two solids are byte-identical (same faces, same vertices, same order).
- **Behaviour:** Determinism is the foundation of `MOD-NAM`. A non-deterministic rebuild cannot have stable edge references.
- **Refusals:** None — a test.
- **Tests:** `a_rebuild_is_deterministic`.
- **Acceptance:** The two solids are identical.
- **Risk:** The kernel's boolean is not deterministic. Fallback: a canonicalisation pass.

### MOD-REC-006 — A rebuild is parallelisable over independent features
- **State:** BACKLOG
- **Depends on:** MOD-REC-005
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `pub fn rebuild_parallel(model: &mut Model, dirty: Option<&[usize]>, threads: usize) -> Rebuild` — features that do not depend on each other are built in parallel.
- **Behaviour:** A model with 100 independent extrudes can build them in parallel. The dependency graph (`Dep`) determines which features are independent.
- **Refusals:** `ModelError::Expression("the thread count must be positive")`.
- **Tests:** `a_parallel_rebuild_produces_the_same_solid_as_a_sequential_one`.
- **Acceptance:** The parallel solid is identical to the sequential one.
- **Risk:** The kernel's boolean is not thread-safe. Fallback: a mutex around the kernel calls.

### MOD-REC-007 — A rebuild's cost model is documented
- **State:** READY
- **Depends on:** MOD-REC-001, MOD-REC-002
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** A doc comment on `Rebuild` stating the cost model: `O(F + B + K)` where `F` is the number of dirty features, `B` is the number of boolean operations, and `K` is the kernel cost per boolean. The model is documented, not measured.
- **Behaviour:** The cost model is the contract the UI uses to decide whether to rebuild synchronously or in the background.
- **Refusals:** None.
- **Tests:** `the_cost_model_is_documented_in_the_rebuild_module`.
- **Acceptance:** The doc comment exists and names the three terms.
- **Risk:** None material.

### MOD-REC-008 — A rebuild can be deferred until the model is idle
- **State:** BACKLOG
- **Depends on:** MOD-REC-001
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `pub fn rebuild_deferred(model: &mut Model, dirty: Option<&[usize]>, idle_ms: u64) -> Rebuild` — waits `idle_ms` after the last change before rebuilding, so a burst of parameter changes produces one rebuild.
- **Behaviour:** The UI calls this after every parameter change; the rebuild fires only when the user stops typing.
- **Refusals:** None.
- **Tests:** `a_deferred_rebuild_fires_only_after_the_idle_period`.
- **Acceptance:** Three changes in 100 ms produce one rebuild.
- **Risk:** The idle timer is a thread. Fallback: a generation counter checked on the next frame.

### MOD-REC-009 — A rebuild can be cancelled mid-feature
- **State:** BACKLOG
- **Depends on:** MOD-SKC-021
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `rebuild_cancellable` is already specified in `MOD-SKC-021`. This task adds a test proving a cancelled rebuild leaves the model consistent.
- **Refusals:** None — a test.
- **Tests:** `a_cancelled_rebuild_leaves_the_model_consistent`.
- **Acceptance:** After a cancellation, `model.solid()` returns the last good solid.
- **Risk:** None material.

### MOD-REC-010 — A rebuild's carried-solid rule is tested
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: a cut's tool feature is not the carried solid; the body is. The existing `tool_features` rule (`crates/model/src/rebuild.rs:137–147`) is tested.
- **Refusals:** None — a test.
- **Tests:** `a_cut_tool_is_not_the_carried_solid`.
- **Acceptance:** After body → tool → cut, the cut operates on the body, not the tool.
- **Risk:** None material.

### MOD-REC-011 — A rebuild's suppressed feature is skipped but its solid survives
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing test `a_suppressed_feature_does_not_run_but_keeps_what_it_built` (`crates/model/tests/parametric.rs:171–181`) covers this. This task adds a test proving the suppressed feature's solid is carried through to the next feature.
- **Refusals:** None — a test.
- **Tests:** `a_suppressed_features_solid_is_carried_to_the_next_feature`.
- **Acceptance:** The next feature's input includes the suppressed feature's solid.
- **Risk:** None material.

### MOD-REC-012 — A rebuild's blocked feature keeps its own last good solid
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing test `a_feature_above_a_failure_is_blocked_not_broken` (`crates/model/tests/parametric.rs:144–168`) covers this. This task adds a test proving the blocked feature's solid is its own, not the running one.
- **Refusals:** None — a test.
- **Tests:** `a_blocked_feature_keeps_its_own_last_good_solid`.
- **Acceptance:** The blocked feature's `last_good` is not the failed feature's solid.
- **Risk:** None material.

### MOD-REC-013 — A rebuild's dirty propagation is transitive
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing test `a_dimension_driven_by_another_recomputes_through_it` (`crates/model/tests/parametric.rs:58–69`) covers this. This task adds a test proving a change three levels up dirties everything above.
- **Refusals:** None — a test.
- **Tests:** `a_change_three_levels_up_dirties_the_whole_tail`.
- **Acceptance:** `dirty_from` returns all three indices.
- **Risk:** None material.

### MOD-REC-014 — A rebuild's parameter cycle is detected
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing `Model::value_of` cycle check (`crates/model/src/feature.rs:335–337`) returns `Err(ModelError::Cyclic)`. This task adds a test proving the cycle is reported, not stack-overflowed.
- **Refusals:** None — a test.
- **Tests:** `a_parameter_cycle_is_reported_not_fatal`.
- **Acceptance:** `Model::value("a")` returns `Err(Cyclic)` for `a = b + 1, b = a`.
- **Risk:** None material.

### MOD-REC-015 — A rebuild's expression error names the parameter
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing `value_of` wraps expression errors with the parameter name (`crates/model/src/feature.rs:346–349`). This task adds a test proving the name is present.
- **Refusals:** None — a test.
- **Tests:** `an_expression_error_names_the_parameter_it_belongs_to`.
- **Acceptance:** The error message contains the parameter name.
- **Risk:** None material.

### MOD-REC-016 — A rebuild's kernel error names the feature
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing `ModelError::in_feature` wrapper (`crates/model/src/error.rs:53–55`) names the feature. This task adds a test proving the name is present in the `Problem::message`.
- **Refusals:** None — a test.
- **Tests:** `a_kernel_error_names_the_feature_it_belonged_to`.
- **Acceptance:** The `Problem::message` contains the feature name.
- **Risk:** None material.

### MOD-REC-017 — A rebuild's non-finite parameter is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing `value_of` finiteness check (`crates/model/src/feature.rs:354–356`) returns `Err(Expression)`. This task adds a test proving a NaN parameter is refused.
- **Refusals:** None — a test.
- **Tests:** `a_nan_parameter_is_refused_with_a_named_error`.
- **Acceptance:** `Model::value("a")` returns `Err(Expression)` for `a = 0/0`.
- **Risk:** None material.

### MOD-REC-018 — A rebuild's empty model produces no solid and no error
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: a model with no features rebuilds to `Rebuild { solid: None, problems: [], rebuilt: 0 }`.
- **Refusals:** None — a test.
- **Tests:** `an_empty_model_rebuilds_to_no_solid_and_no_error`.
- **Acceptance:** `rebuild(&mut Model::new(), None).solid` is `None`.
- **Risk:** None material.

### MOD-REC-019 — A rebuild's records and features length mismatch is repaired
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing `run()` repairs a length mismatch (`crates/model/src/rebuild.rs:60–62`). This task adds a test proving the repair.
- **Refusals:** None — a test.
- **Tests:** `a_records_features_length_mismatch_is_repaired`.
- **Acceptance:** After a manual `model.records.push(Record::pending())`, the next rebuild truncates to match.
- **Risk:** None material.

### MOD-REC-020 — A rebuild's `Problem` is cloneable and matchable
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Problem` derives `Clone, Debug, PartialEq` (`crates/model/src/rebuild.rs:33–38`). This task adds a test proving the `State` is matchable.
- **Refusals:** None — a test.
- **Tests:** `a_problems_state_is_matchable`.
- **Acceptance:** `matches!(problem.kind, State::Failed(_))` compiles and is true for a failure.
- **Risk:** None material.

### MOD-REC-021 — A rebuild's `Rebuild` is default-constructible
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** `Rebuild` derives `Default` (`crates/model/src/rebuild.rs:22`). This task adds a test proving the default is empty.
- **Refusals:** None — a test.
- **Tests:** `a_default_rebuild_is_empty`.
- **Acceptance:** `Rebuild::default().solid` is `None`.
- **Risk:** None material.

### MOD-REC-022 — A rebuild's `Problem` message is non-empty
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** A test: every `Problem` produced by a failing feature has a non-empty message.
- **Refusals:** None — a test.
- **Tests:** `a_problems_message_is_never_empty`.
- **Acceptance:** `!problem.message.is_empty()` for every problem in a failing rebuild.
- **Risk:** None material.

### MOD-REC-023 — A rebuild's feature that produces nothing passes the solid through
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing `Ok(None)` path (`crates/model/src/rebuild.rs:113–118`) passes the carried solid through. This task adds a test proving it.
- **Refusals:** None — a test.
- **Tests:** `a_feature_that_produces_nothing_passes_the_solid_through`.
- **Acceptance:** After such a feature, the model's solid is the carried one.
- **Risk:** None material.

### MOD-REC-024 — A rebuild's tool feature does not become the carried solid
- **State:** READY
- **Depends on:** MOD-REC-010
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing `tool_features` rule (`crates/model/src/rebuild.rs:137–147`) is tested. This task adds a test proving the tool's solid is not carried even if it is the last feature to produce one.
- **Refusals:** None — a test.
- **Tests:** `a_tool_feature_is_never_the_carried_solid`.
- **Acceptance:** After body → tool → cut, the carried solid is the cut's result, not the tool's.
- **Risk:** None material.

### MOD-REC-025 — A rebuild's cost is measured and reported
- **State:** READY
- **Depends on:** MOD-REC-001, MOD-REC-002
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** A test: rebuild a model with 10 features and assert `elapsed_ms > 0` and `rebuilt == 10`.
- **Refusals:** None — a test.
- **Tests:** `a_rebuild_of_ten_features_reports_ten_rebuilt`.
- **Acceptance:** The counters are correct.
- **Risk:** None material.

---

## MOD-FMT — file format v2: versioned, migration-tested

The current `.ccm` is version 1 (`crates/model/src/file.rs:69`). This epic designs v2 with a migration path.

### MOD-FMT-001 — A `FORMAT_VERSION` bump policy is documented
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** A doc comment on `FORMAT_VERSION` stating: bump when the on-disk shape changes in a way that is not additive; never bump for an additive change (a new `#[serde(default)]` field).
- **Behaviour:** The policy is the contract `DAT-001` follows. The current version is 1; v2 is the next.
- **Refusals:** None.
- **Tests:** `the_format_version_policy_is_documented`.
- **Acceptance:** The doc comment exists and names the two rules.
- **Risk:** None material.

### MOD-FMT-002 — A `Document` is forward-compatible: unknown fields are ignored
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** Serde's default behaviour ignores unknown fields. This task adds a test proving a document with an extra unknown field loads.
- **Refusals:** None — a test.
- **Tests:** `a_document_with_an_unknown_field_loads`.
- **Acceptance:** The document loads and the unknown field is ignored.
- **Risk:** None material.

### MOD-FMT-003 — A `Document` is backward-compatible: missing fields get defaults
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `#[serde(default)]` on `parameters`, `features`, `topology` (`crates/model/src/file.rs:84–93`) provides this. This task adds a test proving a v1 document (no `topology`) loads with an empty map.
- **Refusals:** None — a test.
- **Tests:** `a_v1_document_with_no_topology_loads`.
- **Acceptance:** The document loads.
- **Risk:** None material.

### MOD-FMT-004 — A v2 `Document` carries a `units` field
- **State:** BACKLOG
- **Depends on:** DAT-005
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Document { …, units: String }` with `#[serde(default = "default_units")]`. The value is `"mm"` or `"in"`. **BLOCKED** on `DAT-005` (the unit system decision).
- **Behaviour:** A document's unit is part of its meaning. Without it, a document is a set of numbers with no scale.
- **Refusals:** `ModelError::CorruptHistory("the units field is not a known unit")`.
- **Tests:** `a_v2_document_carries_a_units_field`.
- **Acceptance:** The units round-trip.
- **Risk:** `DAT-005` may choose a different representation. Fallback: store the unit as a string and convert at the boundary.

### MOD-FMT-005 — A v2 `Document` carries a `schema` field for the feature tree
- **State:** READY
- **Depends on:** MOD-FMT-001
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Document { …, schema: u32 }` with `#[serde(default = "default_schema")]`. The value is 1 for the current `FeatureKind` set; 2 when a new variant is added.
- **Behaviour:** The schema version is separate from the format version: the format is the envelope, the schema is the feature tree's shape. A v2 format with schema 1 is a valid combination.
- **Refusals:** `ModelError::CorruptHistory("the schema version is not supported")`.
- **Tests:** `a_v2_document_carries_a_schema_field`.
- **Acceptance:** The schema round-trips.
- **Risk:** None material.

### MOD-FMT-006 — A v2 `Document` wraps the feature tree in a versioned envelope
- **State:** BACKLOG
- **Depends on:** MOD-FMT-005
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Document { magic, version, schema, units, parameters, features, topology, bodies }` — the v2 shape. The `bodies` field is `#[serde(default)]`.
- **Behaviour:** The envelope is the same for v1 and v2; the `schema` and `units` fields are what change. A v1 document is a v2 document with `schema = 1` and `units = "mm"`.
- **Refusals:** As the v1 refusals plus the new ones.
- **Tests:** `a_v2_document_round_trips_through_serialisation`.
- **Acceptance:** The v2 document saves and loads.
- **Risk:** The v1 → v2 migration is a no-op if the fields are `#[serde(default)]`. Fallback: a migration function.

### MOD-FMT-007 — A v1 document opens in a v2 build
- **State:** READY
- **Depends on:** MOD-FMT-006
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** A test: write a v1 document (with `FORMAT_VERSION = 1`), load it in a build that reads v2, and assert it loads with default schema and units.
- **Refusals:** None — a test.
- **Tests:** `a_v1_document_opens_in_a_v2_build`.
- **Acceptance:** The document loads and the model is correct.
- **Risk:** None material.

### MOD-FMT-008 — A v2 document is refused by a v1 build
- **State:** READY
- **Depends on:** MOD-FMT-006
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `check()` refuses `version > FORMAT_VERSION` (`crates/model/src/file.rs:144–146`). This task adds a test proving a v2 document is refused by a v1 build.
- **Refusals:** `ModelError::TooNew { found: 2, supported: 1 }`.
- **Tests:** `a_v2_document_is_refused_by_a_v1_build`.
- **Acceptance:** The error names both versions.
- **Risk:** None material.

### MOD-FMT-009 — A migration function converts v1 to v2
- **State:** BACKLOG
- **Depends on:** MOD-FMT-007
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `pub fn migrate_v1_to_v2(doc: Document) -> Result<Document, ModelError>` — fills in the default schema and units. The function is total (never fails) because the v1 shape is a subset of v2.
- **Behaviour:** The migration is a no-op today; it becomes real when v2 adds a required field.
- **Refusals:** `ModelError::CorruptHistory("the document is not a valid v1 document")`.
- **Tests:** `a_v1_document_migrates_to_v2`.
- **Acceptance:** The migrated document has `version = 2`.
- **Risk:** None material.

### MOD-FMT-010 — A `Document`'s magic is checked before the version
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `check()` checks magic first (`crates/model/src/file.rs:141–143`). This task adds a test proving a wrong magic is refused before the version.
- **Refusals:** `ModelError::NotAModel`.
- **Tests:** `a_wrong_magic_is_refused_before_the_version_is_checked`.
- **Acceptance:** The error is `NotAModel`, not `TooNew`.
- **Risk:** None material.

### MOD-FMT-011 — A `Document`'s duplicate feature names are refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `check()` refuses duplicates (`crates/model/src/file.rs:148–153`). This task adds a test proving the refusal.
- **Refusals:** `ModelError::DuplicateFeature(name)`.
- **Tests:** `a_document_with_duplicate_feature_names_is_refused`.
- **Acceptance:** The error names the duplicate.
- **Risk:** None material.

### MOD-FMT-012 — A `Document`'s corrupt JSON is refused with the serde message
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `CorruptHistory(e.to_string())` wrapper (`crates/model/src/file.rs:205`). This task adds a test proving the message is present.
- **Refusals:** `ModelError::CorruptHistory`.
- **Tests:** `a_corrupt_json_document_is_refused_with_the_serde_message`.
- **Acceptance:** The error message contains the serde error.
- **Risk:** None material.

### MOD-FMT-013 — A `Document`'s features that do not build still load
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing test `a_document_whose_features_do_not_build_still_opens` (`crates/model/tests/parametric.rs:225–235`) covers this. This task adds a test proving the model's solid is `None` but the features are present.
- **Refusals:** None — a test.
- **Tests:** `a_document_with_failing_features_loads_with_the_features_intact`.
- **Acceptance:** The features round-trip.
- **Risk:** None material.

### MOD-FMT-014 — A `Document`'s parameters round-trip as written
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** The existing test `a_save_and_reopen_gives_the_same_geometry` (`crates/model/tests/parametric.rs:184–200`) asserts `reopened.parameters == m.parameters`. This task adds a test proving the `Param::Formula` text is preserved verbatim.
- **Refusals:** None — a test.
- **Tests:** `a_parameters_formula_text_round_trips_verbatim`.
- **Acceptance:** The formula text is byte-identical.
- **Risk:** None material.

### MOD-FMT-015 — A `Document`'s suppressed flag round-trips
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: suppress a feature, save, reopen, and assert the flag is still set.
- **Refusals:** None — a test.
- **Tests:** `a_suppressed_feature_round_trips_through_serialisation`.
- **Acceptance:** The flag is preserved.
- **Risk:** None material.

### MOD-FMT-016 — A `Document`'s topology map round-trips
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: build a model with a blend, save, reopen, and assert the topology map is present and the blend resolves by fingerprint.
- **Refusals:** None — a test.
- **Tests:** `a_topology_map_round_trips_through_serialisation`.
- **Acceptance:** The map is non-empty and the resolution is `Fingerprint`.
- **Risk:** None material.

### MOD-FMT-017 — A `Document`'s hostile input is refused at load
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** A test: feed a `.ccm` file with a million features, a NaN parameter, and a cyclic expression; assert each is refused with a named error, never a panic.
- **Refusals:** `ModelError::CorruptHistory`, `ModelError::Expression`, `ModelError::Cyclic`.
- **Tests:** `hostile_ccm_input_is_refused_not_fatal`.
- **Acceptance:** No panic.
- **Risk:** None material.

### MOD-FMT-018 — A `Document`'s save is pretty-printed JSON
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `save_json_with_topology` uses `to_string_pretty` (`crates/model/src/file.rs:184`). This task adds a test proving the output is multi-line.
- **Refusals:** None — a test.
- **Tests:** `a_saved_document_is_pretty_printed_json`.
- **Acceptance:** The output contains newlines.
- **Risk:** None material.

### MOD-FMT-019 — A `Document`'s save is deterministic
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** A test: save the same model twice and assert the two strings are byte-identical.
- **Refusals:** None — a test.
- **Tests:** `a_saved_document_is_deterministic`.
- **Acceptance:** The two strings match.
- **Risk:** The `TopologyMap` iteration order is non-deterministic. Fallback: a `BTreeMap` (already the case).

### MOD-FMT-020 — A `Document`'s save failure produces a valid JSON error document
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The existing `unwrap_or_else` in `save_json_with_topology` (`crates/model/src/file.rs:185`) produces a JSON object with the error. This task adds a test proving the output is parseable.
- **Refusals:** None — a test.
- **Tests:** `a_save_failure_produces_a_valid_json_error_document`.
- **Acceptance:** The output parses as JSON.
- **Risk:** None material.

---

## MOD-VAL — validation: load-time invariants and hostile input

### MOD-VAL-001 — A `Model::check_invariants` that validates the whole model
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** `pub fn Model::check_invariants(&self) -> Result<(), ModelError>` verifying: every feature name unique, every parameter name unique, every `Dep::Feature` and `Dep::Parameter` resolvable, every `FeatureKind::Cut { tool }` naming an existing feature, every `EdgeRef` finite.
- **Behaviour:** Invariants checked on load turn a corrupt file into a named `Err` instead of a rebuild that fails forty seconds later.
- **Refusals:** `ModelError::DuplicateFeature`, `ModelError::NoSuchFeature`, `ModelError::NoSuchParameter`, `ModelError::Expression("an edge reference is not finite")`.
- **Tests:** `a_model_with_a_dangling_feature_reference_is_refused`.
- **Acceptance:** `check_invariants` runs on every load.
- **Risk:** The check is O(n²) over features × dependencies. Fallback: a `BTreeSet` of names.

### MOD-VAL-002 — A `Model::check_invariants` that validates the parameter graph
- **State:** READY
- **Depends on:** MOD-VAL-001
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** The check also verifies the parameter graph is acyclic (via a DFS with a colour map).
- **Behaviour:** A cyclic parameter graph is a corruption, not a model — it would stack-overflow the evaluator.
- **Refusals:** `ModelError::Cyclic(name)`.
- **Tests:** `a_cyclic_parameter_graph_is_refused_at_load`.
- **Acceptance:** The check catches the cycle.
- **Risk:** None material.

### MOD-VAL-003 — A `Model::check_invariants` that validates the topology map
- **State:** READY
- **Depends on:** MOD-NAM-003
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** The check verifies every topology entry's feature name exists and the fingerprint count matches the feature's edge count.
- **Behaviour:** A topology map that does not match the feature tree is a corruption.
- **Refusals:** `ModelError::CorruptHistory("the topology map does not match the feature tree")`.
- **Tests:** `a_topology_map_that_does_not_match_the_tree_is_refused`.
- **Acceptance:** The check catches the mismatch.
- **Risk:** None material.

### MOD-VAL-004 — A `Model::check_invariants` that validates the bodies
- **State:** READY
- **Depends on:** MOD-BOD-002
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** The check verifies every body name is unique and every `Dep::Body` resolvable.
- **Behaviour:** A dangling body reference is a corruption.
- **Refusals:** `ModelError::DuplicateFeature("a body with this name already exists")`, `ModelError::NoSuchFeature`.
- **Tests:** `a_dangling_body_reference_is_refused`.
- **Acceptance:** The check catches the dangling reference.
- **Risk:** None material.

### MOD-VAL-005 — A `Model::check_invariants` that validates the sketch sources
- **State:** BLOCKED
- **Depends on:** MOD-SKC-006, SKT-ENT-036
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** The check verifies every `SketchSource::Parametric` has a matching parameter vector and a solvable sketch.
- **Behaviour:** A sketch that does not solve is a corruption, not a model.
- **Refusals:** `ModelError::Expression("the sketch does not solve")`.
- **Tests:** `an_unsolvable_sketch_is_refused_at_load`.
- **Acceptance:** The check catches the unsolvable sketch.
- **Risk:** SKT has not landed. Fallback: check only the vector length.

### MOD-VAL-006 — A `Model::check_invariants` runs on every load
- **State:** READY
- **Depends on:** MOD-VAL-001
- **Files:** `crates/model/src/file.rs` (MOD)
- **Contract:** `Document::into_model_with_report` calls `model.check_invariants()` after `check()`.
- **Behaviour:** The invariants are part of the load path, not an opt-in.
- **Refusals:** As `check_invariants`.
- **Tests:** `a_corrupt_model_is_refused_on_load`.
- **Acceptance:** The load fails before the rebuild.
- **Risk:** The check doubles the load cost. Fallback: a `Model::load_json_unchecked` for trusted files.

### MOD-VAL-007 — A hostile `.ccm` file never panics
- **State:** READY
- **Depends on:** MOD-VAL-001
- **Files:** `crates/model/tests/parametric.rs` (Q)
- **Contract:** A test: feed a `.ccm` file with every field set to a hostile value (NaN, infinity, negative, huge, empty) and assert no panic.
- **Refusals:** None — a test.
- **Tests:** `hostile_ccm_fields_never_panic`.
- **Acceptance:** No panic.
- **Risk:** None material.

### MOD-VAL-008 — A hostile parameter value is refused at evaluation
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** The existing `value_of` finiteness check (`crates/model/src/feature.rs:354–356`). This task adds a test proving a NaN parameter is refused.
- **Refusals:** None — a test.
- **Tests:** `a_nan_parameter_value_is_refused`.
- **Acceptance:** `Model::value("a")` returns `Err(Expression)`.
- **Risk:** None material.

### MOD-VAL-009 — A hostile edge reference is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD)
- **Contract:** A test: an `EdgeRef` with a NaN point or a negative tolerance is refused.
- **Refusals:** `ModelError::Expression("an edge reference is not finite")`.
- **Tests:** `a_nan_edge_reference_is_refused`.
- **Acceptance:** The blend fails with a named error.
- **Risk:** None material.

### MOD-VAL-010 — A hostile revolve axis is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing revolve axis check (`crates/model/src/rebuild.rs:197–210`). This task adds a test proving a zero-length axis is refused.
- **Refusals:** `ModelError::Expression("the revolve axis is not a direction")`.
- **Tests:** `a_zero_length_revolve_axis_is_refused`.
- **Acceptance:** The revolve fails with a named error.
- **Risk:** None material.

### MOD-VAL-011 — A hostile depth is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing depth check (`crates/model/src/rebuild.rs:165–167`). This task adds a test proving a negative depth is refused.
- **Refusals:** `ModelError::Expression("the depth … is …, which is not a positive distance")`.
- **Tests:** `a_negative_depth_is_refused`.
- **Acceptance:** The extrude fails with a named error.
- **Risk:** None material.

### MOD-VAL-012 — A hostile revolve angle is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing angle check (`crates/model/src/rebuild.rs:184–189`). This task adds a test proving a 400° angle is refused.
- **Refusals:** `ModelError::Expression("the revolve angle … is …, which is not between -360 and 360")`.
- **Tests:** `a_400_degree_revolve_angle_is_refused`.
- **Acceptance:** The revolve fails with a named error.
- **Risk:** None material.

### MOD-VAL-013 — A hostile segment count is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing segment check (`crates/model/src/rebuild.rs:190–192`). This task adds a test proving a segment count of 2 is refused.
- **Refusals:** `ModelError::Expression("2 segments is not between 3 and 256")`.
- **Tests:** `a_segment_count_of_two_is_refused`.
- **Acceptance:** The revolve fails with a named error.
- **Risk:** None material.

### MOD-VAL-014 — A hostile blend size is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing size check (`crates/model/src/rebuild.rs:238–240`). This task adds a test proving a negative size is refused.
- **Refusals:** `ModelError::Expression("the blend size … is …, which is not a positive distance")`.
- **Tests:** `a_negative_blend_size_is_refused`.
- **Acceptance:** The blend fails with a named error.
- **Risk:** None material.

### MOD-VAL-015 — A hostile cut tool name is refused
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/rebuild.rs` (MOD)
- **Contract:** The existing `NoSuchFeature` refusal (`crates/model/src/rebuild.rs:224`). This task adds a test proving a cut naming a non-existent tool is refused.
- **Refusals:** `ModelError::NoSuchFeature(tool)`.
- **Tests:** `a_cut_naming_a_nonexistent_tool_is_refused`.
- **Acceptance:** The cut fails with a named error.
- **Risk:** None material.

---

## Cross-domain contracts (BLOCKED on the right index)

| Task | Blocked on | Owner | Contract needed |
|---|---|---|---|
| `MOD-SKC-005` | `SKT-ENT-009` | SKT | `SketchDoc` public and serialisable |
| `MOD-SKC-006` | `SKT-SLV-001`, `SKT-ENT-020` | SKT | `sketch::regions(&SketchDoc, x, seg_tol) -> Result<Regions, SketchError>` |
| `MOD-SKC-007` | `MOD-SKC-006` | MOD | re-solve in rebuild |
| `MOD-SKC-011` | `MOD-SKC-006` | MOD | parametric tool solid |
| `MOD-SKC-012` | `MOD-NAM-001` | MOD | persistent edge reference |
| `MOD-BOD-004` | `KER-BOOL-001` | KER | robust boolean |
| `MOD-BOD-017` | `MOD-REC-005` | MOD | deterministic rebuild |
| `MOD-BOD-020` | `KER-TOL-002` | KER | interference detection |
| `MOD-REC-006` | `MOD-REC-005` | MOD | deterministic rebuild |
| `MOD-FMT-004` | `DAT-005` | DAT | unit system decision |
| `MOD-VAL-005` | `SKT-ENT-036` | SKT | sketch invariants |
