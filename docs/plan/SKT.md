# SKT — Sketches and constraints: exhaustive task index

**Domain owner:** `SKT` — owns `crates/model/src/sketch/**` (L3).
**Waves:** wave 2 for `SKT-ENT/GEO/DIM`, wave 4 for `SKT-SLV/DIA` (`docs/master-plan.md` §6).
**Seed ids:** master-plan `SKT-001..006` map to `SKT-ENT` (`SKT-001`), `SKT-GEO`+`SKT-DIM` (`SKT-002`+`SKT-003`), `SKT-SLV` (`SKT-004`), `SKT-DIA` (`SKT-005`), `SKT-3D` (`SKT-006`), `SKT-UI` (`UI-004`).

**Id scheme.** `SKT-<EPIC>-<NNN>`, per `docs/master-plan.md` §2 (`DOMAIN-EPIC-NNN`). The
master plan's flat `SKT-004` is the *epic* id for `SKT-SLV`; the per-task ids below are the
assignable units.

**Totals:** 202 tasks — `SKT-ENT` 36, `SKT-GEO` 32, `SKT-DIM` 28, `SKT-SLV` 34, `SKT-DIA` 24,
`SKT-3D` 22, `SKT-UI` 26.

---

## Conventions and invariants

Every task in this file inherits these. A task that needs to break one states the break in its
own `Contract` line and says why in `Risk`.

1. **No panics.** `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
   clippy::unimplemented, clippy::todo, clippy::unreachable)]` and `#![forbid(unsafe_code)]` at
   the crate root, as every production crate already carries (`crates/constraints/src/lib.rs:22`,
   `crates/model/src/lib.rs:43`).
2. **No silent wrong answer.** An operation that cannot produce valid geometry returns an `Err`
   that names the reason. Never a silently re-oriented profile, a clipped loop, a zero-radius arc
   or a self-intersecting region that the kernel later "fixes".
3. **Units.** Sketch parameters are in the sketch plane's own 2D coordinates, in the document's
   length unit. Angles are **radians** everywhere inside `SKT`; degrees exist only at the UI and
   command-line string boundary. *Unresolved:* whether one model unit is one millimetre —
   `cadcraft_model` carries no unit tag and `cadcraft_doc` sketches are in "drawing units"
   (`crates/doc/src/constraint.rs:164`). `SKT-ENT-002` marks this `BLOCKED` on `DAT-005`.
4. **Winding.** Outer loop first, counter-clockwise in the plane's `u`/`v` frame, then holes,
   matching the existing `Sketch::loops` contract (`crates/model/src/feature.rs:39`). Arcs sweep
   counter-clockwise (`ccw_sweep`, `crates/constraints/src/model.rs:255`). An arc whose sweep
   reaches a full turn is normalised to `start == end`; a zero-sweep arc is degenerate, not a
   circle.
5. **Tolerance is never a magic constant.** It scales with the sketch's extent. The existing model
   rule is `(diag * 1e-9).clamp(1e-12, 1e-3)` (`crates/model/src/feature.rs:91`). Solver
   convergence tolerance is `1e-9 * max|x|` over the parameter vector
   (`crates/constraints/src/lib.rs:277`), and "satisfied" is 10× that
   (`crates/constraints/src/lib.rs:435`). Every predicate in `SKT` states which of these it uses.
6. **Reuse before writing.** A second 2D constraint solver in this repository would be a
   regression, not progress. Every task below that could use `crates/constraints` says so and
   cites the file and line. The reuse table is in the next section.
7. **Layering.** `SKT` is L3 and may depend on `cadcraft-geom` (L0), `cadcraft-solid` (L0),
   `cadcraft-doc` (L1) and `cadcraft-constraints` (L2). It must not depend on `cadcraft-io` (L3
   — same layer, no intra-layer order is declared for it), `cadcraft-engine` (L4) or any UI crate.
   Enforced by `cargo xtask layers`; the table is `xtask/src/layers.rs:42,46`.
8. **The last good shape survives.** `SKT` produces *parameters*, not documents. When a sketch
   stops solving, the parameters that last solved stay stored and the error is reported beside
   them — the same contract `crates/model/src/rebuild.rs:5` states for solids. `SKT` must never
   write a partial or non-converged solution back into the sketch.
9. **Blocked vs failed.** `SKT` reports one failure kind to `MOD`: `SketchError`. It has **no**
   `Blocked` variant, because blocking is a property of the feature tree, not of a sketch
   (`crates/model/src/rebuild.rs:8`). A sketch that cannot solve fails its feature; the features
   above it are blocked by that failure. Tasks must not invent a second blocking mechanism.

## State legend

`READY` (all prerequisites validated, files unowned) · `BACKLOG` (specified, ordered, not ready)
· `BLOCKED` (a named prerequisite is missing — the task says which and who owns it).

---

## The dependency chain that matters

This is the single biggest unlock in the domain and every arrow is a real contract:

```
SKT-ENT  entity params  ──▶  SKT-GEO/SKT-DIM  residual rows  ──▶  SKT-SLV  Jacobian + LM solve
                                                                 │
                                                                 ▼
                            MOD-SKC  ◀── SKT-ENT derived_loops() ◀── SKT-DIA redundancy/conflict
```

* **No stored points.** Today `Sketch.loops: Vec<Vec<Vec2>>` (`crates/model/src/feature.rs:40`)
  is static geometry. After `SKT-ENT-001..012` the sketch stores an entity parameter vector and
  a constraint list; the profile handed to the kernel is `derived_loops()`, a *function* of the
  solved parameters. Until `SKT-ENT-020` lands, no feature can read a parameter.
* **No solver until there are rows.** `SKT-SLV` consumes residual rows. It cannot start before
  `SKT-GEO-001` (the form catalogue) and `SKT-DIM-001` (dimensional targets) exist.
* **No feature until there is derived geometry.** `MOD-SKC-001` consumes
  `sketch::regions(&SketchDoc) -> Result<Regions, SketchError>`. It is `BLOCKED` on
  `SKT-ENT-020`.
* **`SKT-DIA` is not a diagnostic afterthought.** Redundancy and conflict detection need the
  Jacobian and rank machinery of `SKT-SLV`; `SKT-DIA-001` depends on `SKT-SLV-014`.

---

## What `crates/constraints` is reused from

`crates/constraints` (L2) is a finished, tested solver for the 2D drawing database. It is **not**
rebuilt here. Three categories, with evidence.

### Reused as code (public today, no `cadcraft_doc` coupling)

| Item | Evidence | Used by |
|---|---|---|
| `expr::{Env, ExprError, parse, eval_const, eval_with, names, valid_name}` | `crates/constraints/src/expr.rs` imports only `std::collections::BTreeMap`; `ExprError` distinguishes `Syntax/Unknown/Cycle/DivZero/TooDeep/NotFinite`; `MAX_LEN = 1024`, `MAX_DEPTH = 64` (lines 7–8, 12–25) | `SKT-DIM` |
| `format_value(v: f64) -> String` | `crates/constraints/src/lib.rs:621` | `SKT-DIM`, `SKT-UI` |
| `SolveOptions { keep, prefer_move }` and its weighting policy (1e4 / 1e-2 / 1.0) | `crates/constraints/src/lib.rs:79, 344–378` | `SKT-SLV` |
| `SolveReport { moved, skipped }` shape | `crates/constraints/src/lib.rs:87` | `SKT-SLV` |
| `auto::{InferOptions, INFER_TYPES, infer, infer_placed, autoconstrain}` | `crates/constraints/src/auto.rs:14, 102, 196, 211, 305` | `SKT-GEO`, `SKT-UI` |
| Limits `MAX_CONSTRAINTS 5000`, `MAX_ITER 200`, `MAX_CONFLICT_PROBES 48` | `crates/constraints/src/lib.rs:48, 50, 52` | all `SKT` |

### Reused in form only (typed over `Drawing`; must be generalised, not copied)

| Item | Evidence | Task |
|---|---|---|
| **Parameter layout** of the four shared entities: Line `ax ay bx by`, Circle `cx cy r`, Arc `cx cy r start end`, Point `x y` | `crates/constraints/src/model.rs:3–13` (module doc) | `SKT-ENT-004` |
| `Model::{point, line, circle, ellipse, end_frame, lock_indices}` accessors over a parameter slice | `crates/constraints/src/model.rs:240, 406, 421, 342, 440` | `SKT-ENT-006`, `SKT-SLV-005` |
| `Form` — the **complete residual-form catalogue**: `PP PL PC P2 L1 LL CC LC SymLines C1 ArcSweep Anchor Lock PE LE Contact EndOn Join`, with the row count each contributes | `crates/constraints/src/residual.rs:14–70, 123–…` | `SKT-GEO-001` |
| `add()`'s transaction: clone, build, solve, refuse non-convergence, refuse rank-deficiency, write back **only on success** (`*d = trial`, line 569) | `crates/constraints/src/lib.rs:481–571` | `SKT-SLV-012` |
| `redundant_with()` — O(n) rank-delta probe naming the constraints a redundant one duplicates | `crates/constraints/src/lib.rs:576–618` | `SKT-DIA-004` |
| `Comp::conflicts()` — "each constraint whose removal makes the rest solvable", with the honest unsatisfied-residual fallback | `crates/constraints/src/lib.rs:251–274` | `SKT-DIA-006` |
| `components()` — union-find over shared references | `crates/constraints/src/lib.rs:282–323` | `SKT-SLV-008` |
| `DofStatus { Unconstrained, Under, Fully, Over, Unknown }`, `ObjectDof`, `DofReport` | `crates/constraints/src/dof.rs:27, 55, 69` | `SKT-SLV-016` |
| `scale_tol` / 10× "satisfied" band | `crates/constraints/src/lib.rs:277, 435` | `SKT-SLV-013` |

### Not reused (and why)

| Item | Evidence | Replaced by |
|---|---|---|
| `solve`, `add`, `set_parameter`, `measure`, `validate`, `purge_invalid`, `residuals`, `all_satisfied`, `glyphs`, `dof`, `env`, `parameter_table`, `next_name`, `next_id`, `name_in_use`, `constraint_value` | every one takes `&Drawing` / `&mut Drawing` (`crates/constraints/src/lib.rs:97, 392, 439, 453, 463, 481, 629, 412, 432, 678, 127, 131, 167, 173, 162, 111`; `crates/constraints/src/dof.rs:127`) — a sketch is not a drawing and has no `Handle` | sketch-local equivalents with the same signatures modulo `SketchDoc`/`EntityId` |
| `Model::build` / `Model::write_back` | `crates/constraints/src/model.rs:154, 466` — `&Drawing` in, `&mut Drawing` out | `SKT-ENT-007` builds a `ParamModel` from `SketchDoc`; `SKT-ENT-009` writes back to `SketchDoc` |
| `Shape::Ellipse`, `Shape::Spline`, `Shape::Poly`, `Shape::Insert` | `crates/constraints/src/model.rs:34–56`; spline needs `cadcraft_geom::Spline`, insert needs `d.block()` | not in sketch v1 (`SKT-ENT-003`); deferred to `SKT-3D-020`/`SKT-ENT-034` |
| `curves::{Ell, spline_end, clamped}` | `crates/constraints/src/curves.rs:44, 154, 137` — ellipse and spline only | `Frame` **is** reused (`curves.rs:18`), for tangency forms |
| `Glyph` | `crates/constraints/src/lib.rs:665` — 2D constraint bars; `Model::anchor` (`model.rs:384`) only covers ellipse/spline/insert | `SKT-UI-014` defines its own anchor rule |
| `crates/constraints::SolveError` variants | typed over `u32` ids into a `Drawing`'s constraint list | `SketchError` (`SKT-ENT-007`) — same *shape*, own type |

### Reuse that requires opening `crates/constraints` (BLOCKED, named owner)

`crates/constraints/src/lm.rs` is the numeric core and imports **only `std`**, but every entry
point is `pub(crate)`: `jacobian` (line 26), `solve` (line 226), `rank_qr` (line 194),
`null_space` (line 205), `qr` (line 109), `max_abs` (line 17), `MAX_ROWS` (line 10), `Outcome`
(line 12). Promoting them to a public `pub mod numeric` is a change to a crate this domain does
not own. `SKT-SLV-001` is that contract task and it is `BLOCKED` on the `constraints` crate
owner; until it lands, every other `SKT-SLV` task is `BLOCKED` behind it. **Writing
`crates/model/src/sketch/solver.rs` as a second implementation is explicitly not the fallback.**

---

## SKT-ENT — entities: line, arc, circle, point, fillet corner

The unit of work here is the change from *stored points* to *stored parameters plus constraints*.
`SKT-ENT-001..020` are the gate for `MOD-SKC`.

### SKT-ENT-001 — Add the `cadcraft-constraints` dependency to `cadcraft-model`
- **State:** BLOCKED
- **Depends on:** —
- **Files:** `crates/model/Cargo.toml` (orchestrator / manifest owner), `xtask/src/layers.rs` (read-only check)
- **Contract:** add `cadcraft-constraints = { workspace = true }` to `[dependencies]` of `cadcraft-model`. Layer rule: `constraints` is L2, `model` is L3 (`xtask/src/layers.rs:42,46`), so L3→L2 is legal. No new external crate.
- **Behaviour:** Adding a workspace dependency to a manifest is authorised work, not an
  engineering decision; per `docs/agents/SYSTEM_PROMPT.md` §4 manifests are only changed on
  authorisation. The coordinator authorises or refuses. Nothing else in `SKT` may start without
  this or without the coordinator ruling that `SKT` owns its own solver (which the master plan
  and `AGENTS.md` forbid).
- **Refusals:** A `cargo xtask layers` failure naming `model → constraints` is an immediate
  refusal to merge, not a warning.
- **Tests:** `cargo xtask layers` green; `cargo xtask ci` green.
- **Acceptance:** `cargo xtask ci` passes with `cadcraft_model` importing
  `cadcraft_constraints::expr::Env`.
- **Risk:** The workspace may forbid `model → doc`; if the coordinator instead wants `SKT`
  type-free of `cadcraft_doc`, the constraint is `SKT-ENT-002`. Fallback: none — this task has
  no fallback, it is a one-line authorisation.

### SKT-ENT-002 — Decide the reference vocabulary: reuse `cadcraft_doc::Sub` or a sketch-local `Sub`
- **State:** BLOCKED
- **Depends on:** SKT-ENT-001
- **Files:** `crates/model/src/sketch/mod.rs` (SKT), `docs/architecture.md` (coordinator decision)
- **Contract:** coordinator picks one. **(a) reuse** `cadcraft_doc::Sub` (`crates/doc/src/constraint.rs:13`: `Whole, Start, End, Mid, Center, Vertex(u32), Segment(u32)`); **(b) sketch-local** `pub enum Sub { Whole, Start, End, Mid, Center }`. Recommendation: **(b)**, because a sketch v1 has no polyline segments, and `Sub::Vertex(u32)`/`Segment(u32)` would be two permanently-unused variants in every match.
- **Behaviour:** The choice decides whether `cadcraft_model` gains a `cadcraft-doc` dependency
  (L1, legal) and whether the two constraint systems can ever share a `GeomRef`. It is a
  compatibility decision, not a performance one, so it is the coordinator's.
- **Refusals:** n/a — a decision task.
- **Tests:** With (b): `sketch_sub_has_no_unused_variants` asserts every `Sub` variant is
  constructible by some `SKT-ENT` code path.
- **Acceptance:** The choice is written into `plan/architecture.md` §3 and every later task uses
  it without re-deciding.
- **Risk:** Choosing (a) makes `cadcraft_doc::GeomRef { handle: Handle, sub: Sub }` unusable as
  is, because `Handle` is a `u64` into a drawing, not a sketch entity. The hybrid would need a
  new `GeomRef`-like type anyway. Fallback for (a): define `SketchRef` wrapping `cadcraft_doc::Sub`.

### SKT-ENT-003 — Declare the sketch v1 entity set
- **State:** READY
- **Depends on:** SKT-ENT-002
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub enum EntityKind { Line, Arc, Circle, Point, FilletCorner }` plus
  `pub fn EntityKind::type_name(self) -> &'static str`, `pub fn EntityKind::param_len(self) -> usize`
  (`Line 4, Arc 5, Circle 3, Point 2, FilletCorner 3`), `pub fn EntityKind::is_constrainable(self) -> bool`
  (`true` for all five; `FilletCorner` is construction geometry and reports `false` to the
  profile builder but `true` to the solver). Ellipse, spline, polyline and block-insert are
  explicitly absent.
- **Behaviour:** The five kinds are the AutoCAD 2D sketch set that matters for a part profile:
  line, arc, circle, point (construction), and a fillet corner. `FilletCorner` is a *derived*
  entity — it holds a radius and its solved centre and contributes a generated arc to the
  profile; it is not a free curve. Anything else is a later epic (`SKT-3D-020`).
- **Refusals:** `EntityKind::param_len` for an unknown discriminant is unreachable; construction
  code that maps a name to a kind returns `Err(SketchError::UnsupportedEntity { name })`.
- **Tests:** `every_entity_kind_has_a_declared_parameter_length` — asserts the five lengths and
  that `type_name` is non-empty and distinct for each.
- **Acceptance:** The five kinds and their parameter lengths are in the crate docs with the
  layout of `crates/constraints/src/model.rs:3–13` quoted as the source for four of them.
- **Risk:** Adding a sixth kind later is cheap; renaming one is not, because the parameter
  layout becomes file format (`MOD-FMT-004`). Risk: layout drift. Fallback: pin the layout in a
  doc test.

### SKT-ENT-004 — Pin the parameter layout to `crates/constraints`
- **State:** READY
- **Depends on:** SKT-ENT-003
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn EntityKind::layout(self) -> &'static str` returning the exact string
  documented at `crates/constraints/src/model.rs:3–13` — `"ax ay bx by"`, `"cx cy r start end"`,
  `"cx cy r"`, `"x y"` — and `"cx cy r"` for `FilletCorner` (centre and radius; the two lines it
  trims are references, not parameters).
- **Behaviour:** Four of the five sketches reuse the 2D drawing solver's parameter layout
  unchanged, so `Model::point/line/circle/end_frame/lock_indices`
  (`crates/constraints/src/model.rs:240,406,421,342,440`) apply verbatim. Diverging from that
  layout would mean forking the accessors, which is the thing we are trying not to do.
- **Refusals:** `layout()` returning a layout whose word count differs from `param_len` is an
  internal assertion; in non-test code the two are derived from one table, so they cannot
  differ.
- **Tests:** `sketch_parameter_layout_matches_the_2d_solver` — asserts the five layout strings
  byte-for-byte against the strings in `crates/constraints/src/model.rs:3–13`.
- **Acceptance:** One table drives both `param_len` and `layout`; the doc test pins it.
- **Risk:** `crates/constraints` changes its layout in a future release and the doc test fails
  in the `constraints` crate, not ours. Fallback: a `#[deprecated]` note and a migration task
  rather than a silent divergence.

### SKT-ENT-005 — `EntityId` that is never reused
- **State:** READY
- **Depends on:** SKT-ENT-003
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)] pub struct EntityId(pub u32);` with `pub fn EntityId::new(v: u32) -> EntityId`, `pub fn as_u32(self) -> u32`. A deleted id is tombstoned, never recycled.
- **Behaviour:** Ids are dense per sketch, assigned from `next_entity`, and monotonically
  increasing. Recycling is the bug that makes a constraint silently re-target a different
  entity after a delete-and-redraw — the exact failure mode `MOD-NAM-004` disprov es.
- **Refusals:** `SketchError::EntityIdExhausted` when `next_entity` reaches `u32::MAX`.
- **Tests:** `a_deleted_entity_id_is_never_reused` — delete entity 3, add a new entity, assert
  its id is 4 and not 3.
- **Acceptance:** No code path lowers `next_entity`.
- **Risk:** A sketch with thousands of edits and deletes gets a sparse id space. Harmless — ids
  are 4 bytes and never iterated densely.

### SKT-ENT-006 — `SketchEntity` and the `ParamModel` view over a slice
- **State:** READY
- **Depends on:** SKT-ENT-004, SKT-ENT-005
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub struct SketchEntity { pub id: EntityId, pub kind: EntityKind, pub params: Vec<f64>, pub construction: bool }`, `pub fn SketchEntity::new(id, kind, params, construction) -> Result<SketchEntity, SketchError>` checking `params.len() == kind.param_len()` and every parameter finite. Plus `pub fn param_model(sketch: &SketchDoc) -> ParamModel` returning a view with `pub fn point(&self, x: &[f64], r: &EntityRef) -> Option<Vec2>`, `line(...) -> Option<(Vec2,Vec2)>`, `circle(...) -> Option<(Vec2,f64)>`, `arc_sweep(...) -> Option<f64>`, `end_frame(...) -> Option<Frame>`, `lock_indices(...) -> Option<Vec<usize>>` — the signatures of `crates/constraints/src/model.rs:240,406,421,430,342,440` with `GeomRef` replaced by `EntityRef`.
- **Behaviour:** This is the single most important reuse task in `SKT`. The 2D solver's `Model`
  (`crates/constraints/src/model.rs:67`) is exactly a `(handle → offset, length, x0)` table plus
  six accessors. Copying the *accessor logic* into `cadcraft_model` is copying 300 lines of
  well-tested math; the alternative — a generic trait so `cadcraft_constraints` can solve a
  sketch — is a much larger change to a crate `SKT` does not own. **Decision: adapt the
  accessors, reuse the solver.** The adapter is `ParamModel`, and the port is mechanical and
  reviewable.
- **Refusals:** `SketchError::WrongParamCount { id, expected, got }` on construction;
  `SketchError::NonFinite { id }` for a NaN or infinite parameter; an `EntityRef` naming an
  absent id yields `None` from the accessors, never a panic.
- **Tests:** `param_model_point_line_circle_match_the_2d_solver` — for a line, a circle and an
  arc with the same parameters, assert the six accessors return the same values as
  `cadcraft_constraints::model` would for the same `EntityKind`; `a_nan_parameter_is_refused`
  asserts `Err(NonFinite)`.
- **Acceptance:** `crates/model/src/sketch/entity.rs` carries a header comment naming
  `crates/constraints/src/model.rs` as the source, and a doc test on each accessor quoting the
  upstream line number.
- **Risk:** The upstream `Model` handles polylines, splines, ellipses and inserts; a port that
  keeps those arms is dead code that will rot. Fallback: port only the arms for the five v1
  kinds and `unreachable`-free `_ => None` for the rest, with the upstream lines listed.

### SKT-ENT-007 — `SketchError`: every refusal has a name
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/sketch/error.rs` (SKT), `crates/model/src/lib.rs` (SKT export)
- **Contract:** `#[derive(Clone, Debug, PartialEq, thiserror::Error)] pub enum SketchError` with
  `NoSuchEntity(EntityId)`, `NoSuchConstraint(u32)`, `WrongParamCount { id: EntityId, expected: usize, got: usize }`,
  `NonFinite { id: EntityId }`, `UnsupportedRef { constraint: u32, why: String }`,
  `WrongArity { constraint: u32, need: usize, got: usize }`, `Conflict { ids: Vec<u32> }`,
  `Redundant { ids: Vec<u32> }`, `NotConverged { iterations: usize, residual: f64 }`,
  `Degenerate { id: EntityId, why: &'static str }`,
  `SelfIntersecting { loop_index: usize, at: Vec2 }`, `OpenLoop { loop_index: usize, gap: f64 }`,
  `NoArea`, `TooLarge { what: &'static str, got: usize, limit: usize }`,
  `Expression(ExprError)`, `Cycle(String)`.
  `ExprError` is `cadcraft_constraints::expr::ExprError` (reused, not re-declared).
- **Behaviour:** The shape mirrors `cadcraft_constraints::SolveError`
  (`crates/constraints/src/lib.rs:55`) and `ModelError`
  (`crates/model/src/error.rs:10`) — an `Err` that names the entity, the constraint and the
  reason. No variant is a bare string: a bare string is what makes a dialog say "something went
  wrong".
- **Refusals:** Adding a `#[error("…")]` that does not name what failed is refused in review.
- **Tests:** `every_sketch_error_names_its_subject` — asserts each variant's `Display` mentions
  the entity id, constraint id or loop index it carries.
- **Acceptance:** The enum compiles under the crate's `deny` lints and has no `_ =>` catch-all
  at its consumers.
- **Risk:** Growing the enum as features land. Mitigation: `#[non_exhaustive]` is *not* used
  (it would break the engine's matches); growth is a normal minor-version event inside the
  workspace.

### SKT-ENT-008 — `EntityRef` and the sketch constraint record
- **State:** READY
- **Depends on:** SKT-ENT-002, SKT-ENT-005
- **Files:** `crates/model/src/sketch/constraint.rs` (SKT)
- **Contract:** `pub struct EntityRef { pub entity: EntityId, pub sub: Sub }`;
  `pub struct SketchConstraint { pub id: u32, pub kind: ConstraintKind, pub refs: Vec<EntityRef>, pub driving: bool, pub name: String, pub expr: String }`;
  `pub enum ConstraintKind` mirroring `cadcraft_doc::ConstraintKind` (`crates/doc/src/constraint.rs:91`) minus `Collinear` initially; `is_dimensional(self) -> bool`; `name(self) -> &'static str`; `name_prefix(self) -> &'static str`.
- **Behaviour:** `driving: false` is a *reference* dimension — measured and displayed, never
  enforced. AutoCAD distinguishes these and so must we, because a driving dimension that stops
  being driving changes what the solver does with no visible edit.
- **Refusals:** A constraint whose `id` exceeds `MAX_CONSTRAINTS` (`5000`, reused from
  `crates/constraints/src/lib.rs:48`) is refused as `TooLarge`.
- **Tests:** `constraint_kind_names_and_prefixes_match_the_2d_solver` — asserts `name()` and
  `name_prefix()` equal the upstream values for every shared kind.
- **Acceptance:** Every `ConstraintKind` has a non-empty AutoCAD-style name and a prefix.
- **Risk:** Divergence from `cadcraft_doc::ConstraintKind` when a kind is added to one side
  only. Fallback: `UnsupportedRef { why }` naming the kind rather than a silent skip.

### SKT-ENT-009 — `SketchDoc`: the whole sketch, stored as history not geometry
- **State:** READY
- **Depends on:** SKT-ENT-005, SKT-ENT-008
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct SketchDoc { pub plane: SketchPlane, pub entities: Vec<SketchEntity>, pub constraints: Vec<SketchConstraint>, pub next_entity: u32, pub next_constraint: u32 }`
  with `pub struct SketchPlane { pub origin: Vec3, pub normal: Vec3, pub u: Vec3 }`,
  `pub fn SketchDoc::new(plane: SketchPlane) -> Result<SketchDoc, SketchError>`,
  `pub fn entity(&self, id: EntityId) -> Option<&SketchEntity>`,
  `pub fn add_entity(&mut self, e: SketchEntity) -> Result<EntityId, SketchError>`,
  `pub fn remove_entity(&mut self, id: EntityId) -> Result<Vec<u32>, SketchError>` (returns the
  ids of constraints that referenced it),
  `pub fn plane(&self) -> Result<cadcraft_solid::Plane, SketchError>`.
- **Behaviour:** The document holds no loops. Everything geometric is derived. `plane()` reuses
  `cadcraft_solid::Plane::with_u` (`crates/solid/src/plane.rs:46`) exactly as
  `crate::feature::Sketch::plane` does (`crates/model/src/feature.rs:45`), so a sketch's frame is
  the one it was drawn in and never one re-inferred on load.
- **Refusals:** `remove_entity` with an id no entity holds is `Err(NoSuchEntity)`.
  `SketchPlane` with a degenerate normal or a `u` parallel to the normal is
  `Err(SketchError::Degenerate { id: EntityId::NONE, why: "the sketch plane has no in-plane axis" })`.
- **Tests:** `a_sketch_document_holds_no_points` — asserts the struct has no `Vec<Vec2>` field;
  `removing_an_entity_reports_the_constraints_that_died` — removes a line that two constraints
  reference and asserts both ids are returned and the constraints removed.
- **Acceptance:** `SketchDoc` serialises with no geometry field, and
  `cadcraft_constraints::model` is not reimplemented anywhere.
- **Risk:** `Serialize` on a struct that will become the `.ccm` v2 sketch payload locks its
  shape into the file format before `MOD-FMT` has designed it. Fallback: mark the module
  `#[doc(hidden)]`-stable and let `MOD-FMT-006` wrap it in a versioned envelope.

### SKT-ENT-010 — Create a line entity
- **State:** READY
- **Depends on:** SKT-ENT-004, SKT-ENT-006
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn line(id: EntityId, a: Vec2, b: Vec2) -> Result<SketchEntity, SketchError>` producing `params = [a.x, a.y, b.x, b.y]`, `construction = false`.
- **Behaviour:** A zero-length line is refused at creation, not at solve: a line whose two ends
  coincide has no direction, so every directional constraint on it is undefined and the solver
  would produce `None` from `end_frame` (`crates/constraints/src/model.rs:342`) forever.
  Threshold is `scale_len(a, b) * 1e-12` where `scale_len` is the sketch extent.
- **Refusals:** `Degenerate { why: "a line with no length" }` for `|b - a| <= tol`;
  `NonFinite` for a NaN end.
- **Tests:** `a_zero_length_line_is_refused_at_creation` — asserts the error names the reason
  and that no entity is appended to the document.
- **Acceptance:** No v1 code path can produce a zero-length line.
- **Risk:** A user legitimately drawing a point-degenerate line during a drag is refused; the
  editor must therefore only commit the entity on mouse-up. Fallback: allow it with a
  construction flag and refuse it at solve.

### SKT-ENT-011 — Create a circle entity
- **State:** READY
- **Depends on:** SKT-ENT-004, SKT-ENT-006
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn circle(id: EntityId, c: Vec2, r: f64) -> Result<SketchEntity, SketchError>` producing `params = [c.x, c.y, r.abs()]`, `construction = false`.
- **Behaviour:** The radius is stored positive and clamped to `max(r, sketch_tol)` — the 2D
  solver does exactly this on write-back (`crates/constraints/src/model.rs:498`). A negative
  radius is a sign error, not a direction, and a circle has no direction.
- **Refusals:** `Degenerate { why: "a circle with no radius" }` for `r <= tol`;
  `NonFinite` for a NaN centre or radius.
- **Tests:** `a_negative_radius_is_stored_as_its_magnitude` — asserts `params[2] > 0` and the
  value equals `r.abs()` clamped.
- **Acceptance:** Every circle in a `SketchDoc` has `params[2] > sketch_tol`.
- **Risk:** Clamping a legitimately tiny radius to `sketch_tol` invents geometry. Fallback:
  refuse below `sketch_tol * 1e-6` instead of clamping, and clamp only in the range where the
  clamp is below double precision.

### SKT-ENT-012 — Create an arc entity
- **State:** READY
- **Depends on:** SKT-ENT-004, SKT-ENT-006
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn arc(id: EntityId, c: Vec2, r: f64, start: f64, end: f64) -> Result<SketchEntity, SketchError>` producing `params = [c.x, c.y, r.abs(), start, end]`, angles normalised by `cadcraft_geom::norm_angle`, radius clamped as for a circle.
- **Behaviour:** The sweep is `ccw_sweep(start, end)` (`cadcraft_geom`), which is how
  `crates/constraints/src/model.rs:430` measures it. A sweep of zero or a full turn is not a
  circle here: zero is degenerate and must be refused, full is a `Circle`. An arc that turns
  clockwise is stored by *normalising the angles*, never by negating the radius — the 2D solver
  shows the other route (`turn = π` at `crates/constraints/src/model.rs:506`) and it costs a
  special case on every read.
- **Refusals:** `Degenerate { why: "an arc with no sweep" }` when
  `ccw_sweep(start,end) <= tol_angle`; `Degenerate { why: "this arc is a circle" }` when the
  sweep is within `tol_angle` of a full turn — the caller should make a `Circle`.
- **Tests:** `an_arc_with_no_sweep_is_refused` and `a_full_turn_arc_is_refused_as_a_circle` —
  assert both errors by name.
- **Acceptance:** No stored arc has a sweep below tolerance or within tolerance of 2π.
- **Risk:** `norm_angle` and `ccw_sweep` live in `cadcraft_geom`; if their semantics differ from
  `ccw_sweep(a0,a1)` at the wrap point, an arc crossing 0° measures wrong. Fallback: assert the
  wrap case in `SKT-SLV-021`.

### SKT-ENT-013 — Create a point entity
- **State:** READY
- **Depends on:** SKT-ENT-006
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn point(id: EntityId, p: Vec2, construction: bool) -> Result<SketchEntity, SketchError>` producing `params = [p.x, p.y]`.
- **Behaviour:** A point entity is always construction geometry in a profile sense — it is never
  part of a loop — but it is constrainable. The `construction` flag marks it as not
  contributing to `derived_loops`.
- **Refusals:** `NonFinite` for a NaN point.
- **Tests:** `a_point_is_construction_geometry_and_is_still_constrained` — asserts a point takes
  a coincident constraint and never appears in `derived_loops`.
- **Acceptance:** Points never contribute to a profile.
- **Risk:** None material.

### SKT-ENT-014 — `FilletCorner`: the derived corner entity
- **State:** BACKLOG
- **Depends on:** SKT-ENT-003, SKT-ENT-010
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub struct FilletCorner { pub line_a: EntityId, pub line_b: EntityId, pub corner: EntityId, pub radius: f64 }`; the entity's `params` are `[cx, cy, r]` and `kind` is `FilletCorner`, `construction = false`.
- **Behaviour:** A fillet corner is not a drawn curve: it is a request to round the corner where
  two lines meet. Its centre and radius are *solved*, not typed, so that a fillet stays a
  fillet when a line moves. `derived_loops` replaces the corner vertex with the two tangency
  points and the arc between them. This is the single entity that makes a sketch behave like a
  part rather than a drawing.
- **Refusals:** `Degenerate { why: "a fillet radius that is not positive" }` for `r <= tol`;
  `UnsupportedRef { why: "both lines of a fillet must be line entities" }` for a non-line;
  `Degenerate { why: "a fillet between parallel lines" }` when the two lines are parallel
  (their tangent points are not finite).
- **Tests:** `a_fillet_corner_rounds_the_profile_it_belongs_to` — two perpendicular lines with
  a fillet of radius 1 yield a profile whose corner arc has radius 1 and whose area equals the
  sharp area plus `(1 - π/4)·1²`; `a_fillet_on_parallel_lines_is_refused`; `a_fillet_radius_of_zero_is_refused`.
- **Acceptance:** Moving either line re-solves the fillet's centre and tangency points.
- **Risk:** A fillet whose radius exceeds half the shorter line's length has no solution and the
  solver must *move the lines*, which is the "no silent wrong answer" rule biting. Fallback:
  report `NotConverged` and leave the corner sharp rather than picking a side.

### SKT-ENT-015 — Write solved parameters back into the document, atomically
- **State:** READY
- **Depends on:** SKT-ENT-006
- **Files:** `crates/model/src/sketch/solve.rs` (SKT)
- **Contract:** `pub fn write_back(doc: &mut SketchDoc, x: &[f64]) -> Vec<EntityId>` returning the ids whose parameters changed. Non-finite entries in `x` are skipped per entity, never written.
- **Behaviour:** This is `Model::write_back` (`crates/constraints/src/model.rs:466`) with
  `&mut Drawing` replaced by `&mut SketchDoc`. It must keep all three upstream guards: the
  finiteness check (line 471), the relative change threshold `1e-13 * scale` (line 474), and the
  re-clamping of radius and angles (lines 498, 509).
- **Refusals:** A parameter that is non-finite in `x` is not written and its entity is not
  reported as moved — the caller sees the shortfall, not a poisoned document.
- **Tests:** `a_non_converged_solution_is_never_written_back` — solve a conflicting sketch, assert
  `doc` is byte-identical before and after; `write_back_skips_non_finite_parameters`.
- **Acceptance:** `doc == original` after any `Err` from the solve path.
- **Risk:** Partial write-back corrupts a sketch with no error. Fallback: stage the whole vector,
  validate, then commit in one `std::mem` swap of the params slice.

### SKT-ENT-016 — Entity accessors for the profile builder
- **State:** READY
- **Depends on:** SKT-ENT-006
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn SketchEntity::start(&self, x: &[f64]) -> Option<Vec2>`,
  `end(...) -> Option<Vec2>`, `center(...) -> Option<Vec2>`,
  `point_at(&self, x: &[f64], t: f64) -> Option<Vec2>` (arc: polar),
  `sweep(&self, x: &[f64]) -> Option<f64>`, `chord_endpoints(&self, x: &[f64]) -> Option<(Vec2, Vec2)>`,
  and `pub fn discretise(&self, x: &[f64], seg_tol: f64) -> Option<Vec<Vec2>>` returning the
  entity's polyline approximation with the last point **not** repeated (an open segment ends
  where the next begins).
- **Behaviour:** `discretise` is the only place sketch curves become polylines, and its chord
  error bound is `seg_tol` in sketch units. A line is 2 points; a point is 1; a circle is
  `ceil(2π / acos(1 - seg_tol/r))` segments; an arc is the same count over its sweep.
- **Refusals:** `TooLarge` when the required segment count exceeds `MAX_DISCRETISE`
  (`4096`, and the sketch extent is so large relative to `seg_tol` that the count would exceed
  it) — the caller is told to raise `seg_tol`, not silently handed a million points.
- **Tests:** `a_circle_discretises_within_the_chord_tolerance` — for `r = 10, seg_tol = 1e-3`,
  assert the max distance from each chord midpoint to the circle is below `seg_tol`;
  `discretisation_segments_do_not_overlap_at_the_joint` — asserts a line and an arc sharing a
  coincident endpoint produce no duplicated point.
- **Acceptance:** Chord error is bounded by `seg_tol` for every entity kind, proven by test.
- **Risk:** A very large circle with a tight `seg_tol` hits the count cap. Fallback: clamp the
  count and *report* the coarser tolerance actually achieved, so nothing is silently lost.

### SKT-ENT-017 — Chain entities into ordered loops
- **State:** BACKLOG
- **Depends on:** SKT-ENT-016
- **Files:** `crates/model/src/sketch/loops.rs` (SKT)
- **Contract:** `pub fn chain(sketch: &SketchDoc, x: &[f64], tol: f64) -> Result<Vec<Vec<EntityId>>, SketchError>` returning each loop as the entity chain that forms it, outer loops before holes.
- **Behaviour:** Endpoints within `tol` are the same point. Every non-construction curve entity
  belongs to exactly one chain; a chain that does not close is an open loop, not a profile.
  Winding is decided *after* assembly by signed area, not by the order entities were drawn in.
- **Refusals:** `OpenLoop { loop_index, gap }` with `gap` the largest endpoint mismatch in the
  chain; `NoArea` when no chain closes; `TooLarge` above `MAX_LOOPS` (`1024`).
- **Tests:** `a_closed_rectangle_is_one_loop` — four lines chained into one loop of four;
  `an_open_chain_is_refused_with_its_gap` — three of four lines, assert the message names the
  gap; `two_loops_that_touch_at_a_point_are_not_merged` — assert the `tol` boundary decides and
  the test states which side of it the two points are on.
- **Acceptance:** The chain step is deterministic: the same document and vector always give the
  same loops in the same order.
- **Risk:** Tangent chains (a line meeting an arc with matching tangents) can join at *two*
  candidate points when a fillet corner is involved. Fallback: the fillet corner owns its two
  tangency points explicitly (`SKT-ENT-014`), so the join is not inferred.

### SKT-ENT-018 — Classify outer loops and holes by nesting
- **State:** BACKLOG
- **Depends on:** SKT-ENT-017
- **Files:** `crates/model/src/sketch/loops.rs` (SKT)
- **Contract:** `pub fn classify(loops: &[Vec<Vec2>], tol: f64) -> Result<Vec<LoopRole>, SketchError>` with `pub enum LoopRole { Outer, Hole }` and the returned vector ordered outer-first then holes, all counter-clockwise (`holes` therefore clockwise relative to the region they cut — see convention 4 and `SKT-ENT-021`).
- **Behaviour:** Nesting depth by point-in-polygon against every other loop, with `MAX_NEST_DEPTH`
  (`8`, matching the block-depth discipline in `AGENTS.md`). Two loops that cross without
  nesting are not a profile.
- **Refusals:** `SelfIntersecting { loop_index, at }` when two loops' boundaries cross;
  `TooLarge { what: "loop nesting depth" }` above `MAX_NEST_DEPTH`.
- **Tests:** `a_loop_inside_another_is_a_hole` — concentric squares, assert one `Outer` and one
  `Hole`; `two_crossing_loops_are_refused`.
- **Acceptance:** The output is ordered outer-first, matching `Sketch::loops`'s documented
  contract (`crates/model/src/feature.rs:39`).
- **Risk:** A hole exactly touching the outer boundary produces a zero-width wall that
  `Regions::from_loops` may accept. Fallback: refuse a gap below `tol` as `SelfIntersecting`.

### SKT-ENT-019 — `derived_loops`: solved parameters to polygon loops
- **State:** READY
- **Depends on:** SKT-ENT-017, SKT-ENT-018, SKT-ENT-014
- **Files:** `crates/model/src/sketch/loops.rs` (SKT)
- **Contract:** `pub fn derived_loops(doc: &SketchDoc, x: &[f64], seg_tol: f64) -> Result<Vec<Vec<Vec2>>, SketchError>` — the same type as `Sketch::loops`, built from the solved parameters. This is **the boundary between `SKT` and `MOD`**.
- **Behaviour:** Outer loop first, counter-clockwise, then holes; fillet corners contribute their
  generated arc; construction entities contribute nothing; the loop is closed (last point equals
  first) because `Regions::from_loops` (`crates/model/src/feature.rs:80`) wants a closed loop
  and a repeated point makes a zero-length segment.
- **Refusals:** `OpenLoop`, `SelfIntersecting`, `NoArea`, `TooLarge` as above. A sketch whose
  solved geometry has negative area in an outer loop is a sign error, not a valid profile —
  refused rather than reversed silently.
- **Tests:** `derived_loops_matches_a_hand_drawn_rectangle` — four constrained lines at
  `(0,0)-(4,0)-(4,3)-(0,3)` give one loop of four points in that order; `a_fillet_corner_adds_its_arc_to_the_loop`;
  `construction_geometry_never_reaches_the_loop`.
- **Acceptance:** For a sketch with no constraints, `derived_loops` reproduces the same points
  the equivalent `crate::feature::Sketch` was built with (round-trip property test).
- **Risk:** Arc discretisation means a circle profile is a polygon, so an extruded circle is a
  faceted prism — a known limit of the L0 kernel, not of `SKT`. Fallback: `MOD-SKC-004` records
  the chord tolerance in the feature so `KER` can be told what was asked for.

### SKT-ENT-020 — `sketch::regions`: the `MOD-SKC` entry point
- **State:** READY
- **Depends on:** SKT-ENT-019
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn regions(doc: &SketchDoc, x: &[f64], seg_tol: f64) -> Result<cadcraft_geom::Regions, SketchError>` calling `cadcraft_geom::Regions::from_loops(derived_loops(doc, x, seg_tol)?, plane_tolerance)`, filtered for non-empty, `Err(NoArea)` when nothing is left. `pub fn plane_tolerance(doc: &SketchDoc) -> f64` using the existing rule `(diag * 1e-9).clamp(1e-12, 1e-3)` from `crates/model/src/feature.rs:91`.
- **Behaviour:** This is the one function `MOD` consumes to build a sketch-driven feature. It is
  the whole point of the epic: after it, `width` is a parameter and the profile follows.
- **Refusals:** `NoArea`; `Expression` never reaches here; every `SketchError` from
  `derived_loops` propagates unchanged.
- **Tests:** `a_constrained_rectangle_regions_to_one_region_of_the_right_area` — 4×3 rectangle,
  assert area 12 within the chord tolerance; `an_empty_sketch_is_refused_as_no_area`.
- **Acceptance:** `MOD-SKC-001` can call exactly this signature.
- **Risk:** `Regions::from_loops` already has its own tolerance and its own empty-region
  filtering; passing a too-tight `seg_tol` produces a region whose edges are not the arcs. Fallback:
  clamp `seg_tol` from below by `plane_tolerance × 10`.

### SKT-ENT-021 — Keep `crate::feature::Sketch` working for v1 documents
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/feature.rs` (MOD — **not SKT**; coordinate), `crates/model/tests/parametric.rs` (Q)
- **Contract:** `crate::feature::Sketch` (`crates/model/src/feature.rs:32`) keeps its
  `loops: Vec<Vec<Vec2>>` field and its three constructors unchanged until `MOD-FMT-008`
  migrates v1 files. `FeatureKind::Extrude { sketch: Sketch }` is not changed by `SKT`; `MOD`
  adds a *new* variant or a `SketchSource` enum in `MOD-SKC-002`.
- **Behaviour:** The 18 existing tests in `crates/model/tests/parametric.rs` must stay green
  for the whole of `SKT`. A sketch domain that breaks the model crate's tests to add a feature
  has traded a working 2D application for a paper sketch solver.
- **Refusals:** n/a.
- **Tests:** The 18 tests in `crates/model/tests/parametric.rs` pass unchanged after every
  `SKT` merge.
- **Acceptance:** `cargo test -p cadcraft-model` green at every wave boundary.
- **Risk:** The orchestrator assigns `feature.rs` to `MOD`; a `SKT` agent editing it collides.
  Fallback: every `SKT` task touching `feature.rs` is `BLOCKED` on the `MOD` coordinator slot.
- **Note:** This task exists so the constraint is written down rather than discovered.

### SKT-ENT-022 — Rectangle tool that emits entities *and* constraints
- **State:** BACKLOG
- **Depends on:** SKT-ENT-010, SKT-GEO-002, SKT-GEO-006, SKT-GEO-008, SKT-GEO-007
- **Files:** `crates/model/src/sketch/tools.rs` (SKT)
- **Contract:** `pub fn rectangle(sketch: &mut SketchDoc, at: Vec2, u: Vec2, v: Vec2, w: f64, h: f64) -> Result<Vec<EntityId>, SketchError>` — four `Line` entities plus two coincident/parallel/horizontal constraints per side and the corner coincidences, all added in one transaction.
- **Behaviour:** The tool adds geometry and constraints together or not at all; a tool that adds
  four free lines and leaves the user to constrain them is not a parametric rectangle. The
  existing `Sketch::rect` (`crates/model/src/feature.rs:65`) is the geometric precedent and its
  refusal of parallel directions is copied.
- **Refusals:** `Degenerate { why: "a rectangle needs two directions that are not parallel" }`;
  `Degenerate { why: "a rectangle side is not a positive distance" }` for `w <= tol` or `h <= tol`.
- **Tests:** `a_tool_rectangle_is_fully_constrained_except_its_size` — assert the four lines and
  that `dof()` reports 2 remaining (position and size); `a_tool_rectangle_with_degenerate_sides_is_refused`.
- **Acceptance:** The tool's output round-trips through save/reopen with the same entity and
  constraint ids.
- **Risk:** The tool's implicit constraints could conflict with constraints the user adds later,
  producing a redundancy the user cannot see. Fallback: `SKT-UI-009` shows the tool's own
  constraints as selectable.

### SKT-ENT-023 — Construction geometry flag
- **State:** READY
- **Depends on:** SKT-ENT-013
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `pub fn SketchEntity::is_profile(&self) -> bool { !self.construction }`, honoured by `derived_loops` and `chain`. Toggling: `pub fn set_construction(&mut self, on: bool) -> Result<(), SketchError>` refusing to demote an entity that is already part of a closed loop used by a feature.
- **Behaviour:** Construction entities are drawn, constrained and solved like any other; they
  simply do not bound a profile. This is AutoCAD's construction geometry and it is the reason a
  point can be a datum without becoming a wall.
- **Refusals:** `Degenerate { why: "this entity bounds a profile" }` when demoting would open a
  loop.
- **Tests:** `construction_entities_solve_but_do_not_bound_a_profile` — a construction line
  constrained horizontal to a profile line moves with it and the profile area is unchanged.
- **Acceptance:** `derived_loops` never emits a construction entity's discretised points.
- **Risk:** None material.

### SKT-ENT-024 — Reflect an entity and its constraints across a line
- **State:** BACKLOG
- **Depends on:** SKT-ENT-015, SKT-GEO-016
- **Files:** `crates/model/src/sketch/tools.rs` (SKT)
- **Contract:** `pub fn mirror(sketch: &mut SketchDoc, ids: &[EntityId], axis: (Vec2, Vec2), keep_original: bool) -> Result<Vec<EntityId>, SketchError>` — new entities with new ids, plus symmetric constraints pairing each new reference with its original.
- **Behaviour:** Mirroring must produce *constraints*, not a frozen copy. AutoCAD mirrors the
  constraint set too, which is what makes the copy follow the original when the original moves.
  A mirrored entity with no constraint to its source is a second, independent drawing.
- **Refusals:** `UnsupportedRef { why: "… is not a mirrorable entity" }` for `FilletCorner` in v1
  (`SKT-ENT-014` owns its tangencies and cannot be re-derived blindly);
  `WrongArity` for an axis that is a degenerate segment.
- **Tests:** `a_mirrored_pair_stays_mirrored_when_one_moves` — move the original by a dimension,
  assert the mirrored entity's distance from the axis equals the original's;
  `mirroring_never_reuses_an_id`.
- **Acceptance:** After mirroring, `dof()` is unchanged by the mirror's own constraints being
  added in the same pattern.
- **Risk:** Mirroring a *driving* dimensional constraint duplicates its name, which
  `cadcraft_constraints` refuses (`name_in_use`, `lib.rs:162`). Fallback: a mirrored dimensional
  constraint becomes a reference dimension or is dropped with a named `Redundant`.

### SKT-ENT-025 — Sketch bounds from solved parameters
- **State:** READY
- **Depends on:** SKT-ENT-016
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn bounds(doc: &SketchDoc, x: &[f64], include_construction: bool) -> Option<(Vec2, Vec2)>`, a port of `sketch_bounds` (`crates/model/src/feature.rs:420`) that does not need a region and therefore works on a sketch that does not yet enclose anything.
- **Behaviour:** Prefixes and viewport fit need bounds before the profile is valid, including for
  an under-constrained sketch. The existing helper goes through `regions()` and so returns
  `None` for an open sketch.
- **Refusals:** `None` for an empty sketch — a `Result` here would make callers handle a
  non-error as an error.
- **Tests:** `an_open_sketch_still_has_bounds` — three unclosed lines give bounds;
  `an_empty_sketch_has_no_bounds`.
- **Acceptance:** Bounds need no solve to be well defined once `x` is available, and no
  `Regions`.
- **Risk:** None material.

### SKT-ENT-026 — Hostile-input guards on entity construction
- **State:** READY
- **Depends on:** SKT-ENT-009
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub const MAX_ENTITIES: usize = 4096;` enforced in `add_entity` as `Err(SketchError::TooLarge { what: "entities", got, limit: MAX_ENTITIES })`; every constructor validates finiteness before it stores.
- **Behaviour:** A `.ccm` file is hostile input like any other (`AGENTS.md`, "Never crash"). The
  caps come from the 2D solver's own caps — `MAX_PARAMS = 4000`
  (`crates/constraints/src/model.rs:25`) and `MAX_CONSTRAINTS = 5000`
  (`crates/constraints/src/lib.rs:48`) — so a sketch cannot exceed what the shared solver can
  already hold.
- **Refusals:** `TooLarge`, `NonFinite`, `WrongParamCount`. A file with a million entities is
  refused at load, not at solve.
- **Tests:** `a_sketch_with_too_many_entities_is_refused` and `hostile_entity_parameters_never_panic`
  — feed NaN, `f64::INFINITY`, `-0.0` and `1e308` into every constructor and assert no panic.
- **Acceptance:** `MAX_ENTITIES` is checked before any allocation proportional to the input.
- **Risk:** A legitimate sketch above 4096 entities (an imported outline) is refused. Fallback:
  an explicit `SketchOptions { max_entities }` raised by the caller, capped at the kernel's
  own limit.

### SKT-ENT-027 — Purge constraints whose entities are gone
- **State:** READY
- **Depends on:** SKT-ENT-009
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn purge_invalid(doc: &mut SketchDoc) -> Vec<u32>` — a port of `purge_invalid` (`crates/constraints/src/lib.rs:463`) returning the ids removed.
- **Behaviour:** Deleting an entity takes its constraints with it, so this is a repair path for
  documents written by an older or buggy build, not a normal operation. It returns the removed
  ids so the caller can tell the user what was dropped rather than silently losing a dimension.
- **Refusals:** none — the function repairs, it does not fail. A constraint that survives but is
  unsatisfiable is left for `SKT-DIA` to report.
- **Tests:** `purging_removes_dangling_constraints_and_reports_them` — asserts the ids come back
  and the document re-solves.
- **Acceptance:** After `purge_invalid`, every constraint references an existing entity.
- **Risk:** Silent loss of a driving dimension. Mitigation: the return value is mandatory at the
  call site; `SKT-VAL-003` (in `MOD.md`) surfaces it.

### SKT-ENT-028 — Arc ↔ line conversion
- **State:** BACKLOG
- **Depends on:** SKT-ENT-012, SKT-GEO-021
- **Files:** `crates/model/src/sketch/tools.rs` (SKT)
- **Contract:** `pub fn arc_to_line(doc: &mut SketchDoc, id: EntityId) -> Result<EntityId, SketchError>` (two lines through the centre) and `pub fn line_to_arc(doc: &mut SketchDoc, id: EntityId) -> Result<EntityId, SketchError>` (chord plus a bulge implied by collinearity) — each preserving every constraint that still applies and reporting the ones it cannot.
- **Behaviour:** Conversions are how a user changes their mind, and each must say what it lost.
  `Coincident` on the arc's start transfers to the lines' outer ends; `Tangent` on the arc does
  not survive and is reported.
- **Refusals:** `UnsupportedRef { constraint, why: "… does not survive the conversion" }` for each
  constraint that would be lost — returned as a list, and the conversion is refused unless the
  caller passes `force: true`.
- **Tests:** `converting_an_arc_reports_the_constraints_it_cannot_keep` — assert the tangent
  constraint id is named and the conversion is refused by default.
- **Acceptance:** No conversion silently drops a constraint.
- **Risk:** `ArcSweep` dimensional constraints (degrees) become meaningless. Fallback: refuse
  the conversion outright when an `ArcSweep` dimension targets the arc.

### SKT-ENT-029 — A fillet corner's radius is driven by an expression
- **State:** BACKLOG
- **Depends on:** SKT-ENT-014, SKT-DIM-012
- **Files:** `crates/model/src/sketch/entity.rs` (SKT), `crates/model/src/sketch/constraint.rs` (SKT)
- **Contract:** A `FilletCorner`'s `radius` is a parameter read through the same `ExprEnv` as every other dimensional value: `pub fn fillet_radius(doc: &SketchDoc, id: EntityId, env: &mut ExprEnv) -> Result<f64, SketchError>`, with the entity carrying `radius_expr: String` (empty = literal).
- **Behaviour:** A fillet that cannot be driven is a decoration. The expression lives on the
  entity rather than in a `Radius` constraint because the corner's radius is a property of the
  corner, not of a curve it does not own.
- **Refusals:** `Expression(ExprError)` for a parse or evaluation failure; `Degenerate` for a
  non-positive evaluated radius.
- **Tests:** `a_fillet_radius_follows_the_parameter_it_names` — set `r = 2`, assert the profile's
  removed area is `(2 - π/2)·…`; `a_fillet_radius_expression_that_fails_names_the_entity`.
- **Acceptance:** Changing the parameter changes the solved corner centre and the derived arc.
- **Risk:** The radius participates in `Dep::Parameter` dirty propagation, which `MOD-REC`
  owns. Fallback: `SKT-ENT-009` exposes `SketchDoc::parameters_read() -> Vec<String>` and `MOD`
  merges it.

### SKT-ENT-030 — Place a sketch on a model face or edge
- **State:** BLOCKED
- **Depends on:** SKT-ENT-009, `KER-TOL-001`
- **Files:** `crates/model/src/sketch/attach.rs` (SKT), `crates/solid/src/**` (KER — **not SKT**)
- **Contract:** `pub fn plane_from_face(face: &cadcraft_solid::Face, u_hint: Option<Vec3>) -> Result<SketchPlane, SketchError>` and `pub fn plane_from_edge(a: Vec3, b: Vec3, up: Vec3) -> Result<SketchPlane, SketchError>`. **BLOCKED** on the kernel exposing a stable, typed face/edge identity — today `Solid::edges()` returns `Option<Vec<Edge>>` (`crates/solid/src/solid.rs:518`) rebuilt from geometry on every call, and a face's `in_plane_u` is chosen by `Face::new`'s own canonical rule (`crates/solid/src/solid.rs:35`), which is not a user choice.
- **Behaviour:** "Sketch on face" is the most common way a part starts, and it needs the kernel
  to hand back an `u` axis the *user* can see and that survives the next boolean. Without a
  naming scheme (`MOD-NAM`) this is a placement with no stable target: the plane is right today
  and points at the wrong face after the next cut.
- **Refusals:** `NoSuchEntity` equivalent — `Err(SketchError::Degenerate { why: "this face has no usable in-plane axis" })` for a face whose plane cannot be built; for a zero-length edge, `Degenerate { why: "an edge with no direction" }`.
- **Tests:** `a_plane_from_an_edge_is_orthonormal_and_right_handed` — assert
  `|(normal × u)·v - 1| < 1e-12`; `a_plane_from_a_degenerate_edge_is_refused`.
- **Acceptance:** The kernel contract required is written down and owned by the `KER` agent
  before this task starts.
- **Risk:** If `KER` never provides it, sketch-on-face stays impossible and the whole workflow
  is "sketch on a datum plane". Fallback: datum-plane-only sketching (`SKT-ENT-031`), which is a
  real product but a poorer one.

### SKT-ENT-031 — Datum planes: XY, XZ, YZ and an arbitrary plane
- **State:** READY
- **Depends on:** SKT-ENT-009
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn datum(name: &str) -> Result<SketchPlane, SketchError>` returning `SketchPlane` for `"xy"`, `"xz"`, `"yz"` via `cadcraft_solid::Plane::{xy,xz,yz}` (`crates/solid/src/plane.rs:78,83,88`), and `pub fn arbitrary(origin: Vec3, normal: Vec3, u: Vec3) -> Result<SketchPlane, SketchError>` for a chosen plane. Unknown names are `Err(SketchError::UnsupportedRef { why })`.
- **Behaviour:** This is the fallback that keeps `SKT` shippable while `SKT-ENT-030` waits on
  the kernel. It is also what `cadcraft_solid::Plane` already does, so it is close to free.
- **Refusals:** `Degenerate` for a degenerate normal or a `u` parallel to it; `UnsupportedRef`
  for an unknown datum name.
- **Tests:** `the_three_datum_planes_are_orthonormal` — assert each normal is a unit axis and
  `u` is perpendicular to it; `an_arbitrary_plane_with_a_degenerate_normal_is_refused`.
- **Acceptance:** A sketch can be created on any of the three datum planes with no kernel work.
- **Risk:** None material.

### SKT-ENT-032 — The sketch's `u` axis is stored, never re-inferred
- **State:** READY
- **Depends on:** SKT-ENT-009
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `SketchPlane.u` is part of the serialised document and `SketchDoc::plane()` passes it verbatim to `cadcraft_solid::Plane::with_u`. No load-time code recomputes it from the normal.
- **Behaviour:** This is the exact lesson the existing `Sketch` already learned
  (`crates/model/src/feature.rs:35–38`: "The plane's in-plane axis, so the frame is the one the
  profile was drawn in and not one inferred again on load"). A sketch whose `u` is re-derived on
  load by a build with a slightly different canonical rule comes back mirrored or rotated, and
  every stored profile in it is wrong in a way nobody can see.
- **Refusals:** `Degenerate` if the stored `u` is not perpendicular to the stored normal — that
  is a corrupt document, refused rather than repaired.
- **Tests:** `a_sketch_survives_a_save_and_reopen_with_the_same_plane` — assert
  `plane.origin`, `normal` and `u` are bit-identical after a round trip.
- **Acceptance:** No code path derives `u` from `normal`.
- **Risk:** None material.

### SKT-ENT-033 — Copy a sketch between documents with an id remap
- **State:** BACKLOG
- **Depends on:** SKT-ENT-009, MOD-FMT-002
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn copy_into(src: &SketchDoc, dst: &mut SketchDoc, plane: SketchPlane) -> Result<IdMap, SketchError>` with `pub struct IdMap { pub entities: BTreeMap<EntityId, EntityId>, pub constraints: BTreeMap<u32, u32> }`.
- **Behaviour:** Ids are per-sketch and never reused (`SKT-ENT-005`), so a copy *must* remap.
  The returned map is what `MOD-SKC` needs when a pattern is instanced.
- **Refusals:** `TooLarge { what: "entities" }` if the destination would exceed `MAX_ENTITIES`;
  a constraint in `src` naming a constraint id not in `src` is `Err(NoSuchConstraint(id))`.
- **Tests:** `copying_a_sketch_remaps_every_id` — copy a 12-entity, 9-constraint sketch twice
  into one document, assert no collision and that `dst` re-solves to the same loops.
- **Acceptance:** Two copies in one document have disjoint id sets and identical derived loops.
- **Risk:** Forgetting to remap inside a constraint's `refs`. Fallback: the doc test walks every
  constraint and asserts each ref is in `IdMap::entities`.

### SKT-ENT-034 — Polyline entity (deferred)
- **State:** BACKLOG
- **Depends on:** SKT-ENT-003, SKT-GEO-032
- **Files:** `crates/model/src/sketch/entity.rs` (SKT)
- **Contract:** `EntityKind::Poly { n: usize, closed: bool }` with layout `x0 y0 …`, mirroring `Shape::Poly` (`crates/constraints/src/model.rs:38`).
- **Behaviour:** A polyline is needed for imported outlines and for slot profiles with many
  vertices. It is *deferred*, not designed, because it interacts with `Sub::Vertex`/`Segment`
  and therefore with `SKT-ENT-002`'s decision.
- **Refusals:** `TooLarge` above `MAX_SPLINE_POINTS`-scale (`500`, reused from
  `crates/constraints/src/model.rs:27`); a polyline with fewer than two vertices is refused.
- **Tests:** deferred with the feature.
- **Acceptance:** Nothing — this task is a placeholder recording the decision to defer.
- **Risk:** Slot and rounded-slot profiles are common and a polyline-free sketch cannot make
  them. Fallback: a slot is four lines and two arcs, which is expressible today.

### SKT-ENT-035 — Property tests for entity geometry
- **State:** READY
- **Depends on:** SKT-ENT-016, SKT-ENT-017
- **Files:** `crates/model/tests/sketch_entity.rs` (Q)
- **Contract:** `proptest!` cases: (a) `discretise_never_exceeds_the_chord_tolerance` for random
  circles/arcs/radii/tolerances; (b) `every_closed_chain_is_a_closed_loop`; (c)
  `entity_params_round_trip_through_serde`; (d) `a_random_finite_parameter_vector_never_panics`.
- **Behaviour:** Mirrors the existing 2D approach (`crates/geom/src/property_tests.rs`, the model
  named in `docs/master-plan.md` §5 `QAT-001`). Cases (a) and (d) are the ones that matter:
  discretisation and hostile input.
- **Refusals:** Cases that would produce `NaN` are shrunk, not skipped silently — a skip is a
  hidden hole.
- **Tests:** The four cases above.
- **Acceptance:** 1000 cases each, deterministic seed recorded.
- **Risk:** Property tests that pass because the generator never reaches a hard case. Fallback:
  targeted regression tests alongside, one per reported failure.

### SKT-ENT-036 — Invariants asserted on every stored sketch
- **State:** READY
- **Depends on:** SKT-ENT-009, SKT-ENT-026
- **Files:** `crates/model/src/sketch/mod.rs` (SKT)
- **Contract:** `pub fn check_invariants(doc: &SketchDoc) -> Result<(), SketchError>` verifying: every entity id unique, every constraint reference resolvable, every `params.len() == kind.param_len()`, every parameter finite, every circle radius `> tol`, every arc sweep in `(tol, 2π - tol)`, `next_entity > max(id)`, `next_constraint > max(id)`. Called from `MOD-FMT`'s load path and from every `SKT` mutation's debug assertions.
- **Behaviour:** Invariants checked on load turn a corrupt file into a named `Err` at the
  boundary instead of a solver that diverges forty seconds later.
- **Refusals:** each violated invariant maps to its own `SketchError` variant, not to one
  `CorruptSketch(String)`.
- **Tests:** `invariant_check_catches_each_violation` — mutate a valid document seven ways and
  assert seven distinct errors.
- **Acceptance:** `check_invariants` runs on every load and its failures are named.
- **Risk:** The check is O(n·m) on constraints × entities if written naively. Fallback: a
  `BTreeSet` of ids built once.

---

## SKT-GEO — geometric constraints

Every geometric constraint `SKT` needs already exists as a residual *form* in the 2D solver
(`crates/constraints/src/residual.rs:14–70`). `SKT-GEO` is a port of those forms, not a design.

### SKT-GEO-001 — Map `Form` to the sketch's residual registry
- **State:** READY
- **Depends on:** SKT-ENT-006, SKT-SLV-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `pub(crate) enum Form { PP, PL { swap: bool }, PC { swap: bool }, P2, L1, LL, CC, LC { swap: bool }, SymLines { crossed: bool }, C1, ArcSweep, Anchor(Vec2), Lock }` and `pub(crate) fn interpret(c: &SketchConstraint, m: &ParamModel, x: &[f64], target: f64) -> Result<Built, String>`. Row counts copied from `crates/constraints/src/residual.rs:123–…`: `PP 2, PL 1, PC 1, P2 1, L1 1, LL 1 (Collinear 2), CC 2, LC 1, SymLines 2, C1 1, ArcSweep 1, Anchor 2, Lock 0`.
- **Behaviour:** This is the load-bearing task of the whole epic. The upstream `Form` list has
  nineteen variants; the sketch v1 needs thirteen of them. The port is mechanical — the `match`
  on `ConstraintKind` at `crates/constraints/src/residual.rs:123` and its helper closures
  (`pt`, `ln`, `ci`, `el`) translate one-for-one once `Model` becomes `ParamModel` and
  `GeomRef` becomes `EntityRef`.
- **Refusals:** an interpretation failure is `Err(SketchError::UnsupportedRef { constraint, why })`
  with the upstream's own message text (e.g. `"Coincident needs two references"`).
- **Tests:** `form_row_counts_match_the_2d_solver` — asserts each shared form's row count equals
  the upstream value.
- **Acceptance:** `Form` has no variant that `SKT-GEO-002..021` does not use, and every variant
  used is reachable from a `ConstraintKind`.
- **Risk:** Silent divergence of row counts changes the rank and therefore the reported degrees
  of freedom. Fallback: the row-count doc test above, which fails loudly.

### SKT-GEO-002 — Coincident, point to point
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `ConstraintKind::Coincident` on two point references → `Form::PP`, 2 rows: `px0 - px1`, `py0 - py1`.
- **Behaviour:** The simplest and the most common. Two rows because it fixes two coordinates;
  a sketch whose DOF count is wrong is almost always a PP miscounted as one row.
- **Refusals:** `WrongArity { need: 2, got }` otherwise; `UnsupportedRef` when either reference
  is not point-like for its entity.
- **Tests:** `coincident_points_hold_to_the_solver_tolerance` — two points 1 apart, assert the
  residual is `< scale_tol`; `coincident_needs_two_references`.
- **Acceptance:** Adding it reduces DOF by exactly 2 in a free sketch.
- **Risk:** None material.

### SKT-GEO-003 — Coincident, point on line
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Coincident` with one point reference and one whole-line reference → `Form::PL { swap }`, 1 row: the signed perpendicular distance from the point to the line, normalised by the line's length so the residual is in sketch units.
- **Behaviour:** A point on a line costs one DOF, not two. The normalising by line length is what
  makes the residual scale-free, which is what makes the shared `scale_tol` meaningful.
- **Refusals:** `Degenerate { why: "a line with no length" }` when the line's length is below
  tolerance (so the normalising is defined); `WrongArity` for other arities.
- **Tests:** `a_point_on_a_line_costs_one_degree_of_freedom`; `point_on_a_zero_length_line_is_refused`.
- **Acceptance:** DOF drops by 1.
- **Risk:** Upstream `PL` uses the un-normalised cross product divided by nothing
  (`crates/constraints/src/residual.rs` `Form::PL`). Fallback: match upstream exactly and let
  `scale_tol` absorb the scale, or keep the normalisation and document the deliberate
  difference in `Balance`.

### SKT-GEO-004 — Coincident, point on circle or arc
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Coincident` with a point and a whole circle/arc → `Form::PC { swap }`, 1 row: `|p - c| - r`, clamped to the circle when the reference is an arc (a point on the circle, not inside it).
- **Behaviour:** One row. The arc case must use the arc's own radius parameter, so a point
  constrained to an arc's circle moves along the circle and can end up outside the sweep — which
  `SKT-GEO-020` handles with an `ArcSweep`-style clamp or a reported failure, never silently.
- **Refusals:** `Degenerate { why: "a circle with no radius" }`; `UnsupportedRef` for a non-circle
  second reference.
- **Tests:** `a_point_on_a_circle_costs_one_degree_of_freedom`; `a_point_on_a_circle_lands_on_the_circle_not_inside_it`.
- **Acceptance:** After solving, `|(p - c)| - r|` is below tolerance.
- **Risk:** The point escaping the arc's angular range. Fallback: report it as a conflict rather
  than allowing the arc to grow.

### SKT-GEO-005 — Concentric
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `ConstraintKind::Concentric` on two circles, arcs or ellipses → `Form::CC`, 2 rows: `cx0 - cx1`, `cy0 - cy1`. Two rows, like PP — concentricity is a point equality.
- **Behaviour:** Copied from `crates/constraints/src/residual.rs:167–175`.
- **Refusals:** `UnsupportedRef { why: "Concentric needs two circles, arcs or ellipses" }`.
- **Tests:** `concentric_circles_cost_two_degrees_of_freedom`.
- **Acceptance:** Centres coincide to tolerance after solving.
- **Risk:** Concentric on two arcs of different radii is legal but useless; not an error.

### SKT-GEO-006 — Horizontal on a line
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Horizontal` on one line reference → `Form::L1`, 1 row: `by - ay` (the y difference of the line's ends).
- **Behaviour:** One row fixes one DOF. Using `by - ay` rather than a cross product against the
  global x-axis keeps the residual linear, which the numerical Jacobian differentiates exactly.
- **Refusals:** `UnsupportedRef` for a non-line; `WrongArity` for other arities.
- **Tests:** `horizontal_line_costs_one_degree_of_freedom`; `a_horizontal_line_solves_with_zero_residual_in_y`.
- **Acceptance:** `|b.y - a.y| < scale_tol`.
- **Risk:** None material.

### SKT-GEO-007 — Horizontal / vertical on two points
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Horizontal` or `Vertical` on two point references → `Form::P2`, 1 row: `y1 - y0` (resp. `x1 - x0`). Shares the `ConstraintKind` with the line form and is distinguished by arity, exactly as upstream does (`crates/constraints/src/residual.rs:150–159`).
- **Behaviour:** Two points and a line are the same equation; the difference is only which
  parameters it touches. Distinguishing them by arity rather than by a separate kind keeps
  `ConstraintKind` identical to AutoCAD's for interchange.
- **Refusals:** `UnsupportedRef { why: "Horizontal needs a line or two points" }`.
- **Tests:** `horizontal_between_two_points_costs_one_degree_of_freedom`.
- **Acceptance:** Identical residual to the line form.
- **Risk:** None material.

### SKT-GEO-008 — Vertical
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Vertical` on a line → `Form::L1`, 1 row: `bx - ax`.
- **Behaviour:** The mirror of `SKT-GEO-006`; stated separately because a combined implementation
  invites an asymmetric bug that shows up only as a mirrored profile.
- **Refusals:** as `SKT-GEO-006`.
- **Tests:** `vertical_line_costs_one_degree_of_freedom`.
- **Acceptance:** `|b.x - a.x| < scale_tol`.
- **Risk:** None material.

### SKT-GEO-009 — Parallel
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Parallel` on two lines → `Form::LL`, 1 row: `cross(d0, d1) = 0`, scaled by the product of the two direction lengths so the residual is dimensionless.
- **Behaviour:** `cross = 0` admits *anti*-parallel lines, which is correct — parallel includes
  antiparallel and a residual that distinguishes them would flip an edge during the solve.
- **Refusals:** `UnsupportedRef { why: "Parallel needs two lines" }`;
  `Degenerate { why: "a line with no length" }` for the scaling denominator.
- **Tests:** `parallel_costs_one_degree_of_freedom`; `antiparallel_lines_count_as_parallel`.
- **Acceptance:** Cross product below tolerance.
- **Risk:** At exactly parallel the Jacobian row is near-zero, which the rank computation may
  call rank-deficient. Fallback: `RANK_TOL` in `lm.rs:190` already handles it; add a test at
  the wrap point.

### SKT-GEO-010 — Perpendicular
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Perpendicular` on two lines → `Form::LL`, 1 row: `dot(d0, d1) = 0`, scaled by the product of direction lengths.
- **Behaviour:** Orthogonality is orientation-free by construction: `dot = 0` is symmetric, so a
  solver that has to choose a sign here has a bug.
- **Refusals:** `UnsupportedRef`; `Degenerate` for a zero-length line.
- **Tests:** `perpendicular_costs_one_degree_of_freedom`; `perpendicular_is_symmetric_in_its_arguments`.
- **Acceptance:** `|dot| < scale_tol`.
- **Risk:** A line with `|d| ~ 0` has an undefined direction; the `Degenerate` guard covers it.

### SKT-GEO-011 — Tangent, line to circle or arc
- **State:** READY
- **Depends on:** SKT-GEO-001
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Tangent` with a line and a circle/arc → `Form::LC { swap }`, 1 row: `dot(p_line - c, n_line) = 0` with `sign` (±1) chosen from the initial geometry as `sgn(cross(dir_n(a,b), c - a))`, copied from `crates/constraints/src/residual.rs:197–200`.
- **Behaviour:** The sign fixes *which side* of the line the circle sits on. It is chosen once,
  from the starting geometry, and never re-chosen — a solver that re-picks the sign mid-iter is a
  solver that can jump a circle from one side of a line to the other, which is the worst
  possible outcome for a part.
- **Refusals:** `Degenerate { why: "a line with no length" }`;
  `UnsupportedRef { why: "… needs a line and a circle or arc" }`.
- **Tests:** `tangent_line_circle_costs_one_degree_of_freedom`; `the_tangent_side_is_chosen_once_and_kept` —
  perturb the start geometry far enough that the sign would flip if re-chosen, assert it does not.
- **Acceptance:** Distance from the circle's centre to the line equals `r` to tolerance, with
  the sign recorded in `Built::sign`.
- **Risk:** A circle exactly touching the line at the start has `sign` = `+1` by
  `sgn(0) = 1` (`crates/constraints/src/residual.rs:94`), an arbitrary choice. Fallback: report
  a conflict when the initial geometry is exactly tangent rather than guessing a side.

### SKT-GEO-012 — Tangent, circle to circle
- **State:** READY
- **Depends on:** SKT-GEO-001, SKT-SLV-006
- **Files:** `crates/model/src/sketch/residual.rs` (SKT)
- **Contract:** `Tangent` between two curves → `Form::Contact`, with one auxiliary contact parameter per curve, `aux_init` from the nearest approach sampled at `CONTACT_SAMPLES = 72` (upstream, `crates/constraints/src/residual.rs:10`), settled before the main solve by a short LM pass (`interpret`, `crates/constraints/src/lib.rs:181–201`).
- **Behaviour:** Curve-to-curve tangency is the one constraint with an *auxiliary unknown* — the
  contact point is not given by any stored parameter. The upstream design settles the contact
  parameters with geometry held still, then solves everything together; that two-phase approach
  is the contract and is ported, not reinvented.
- **Refusals:** `TooLarge` when `x0.len() + aux.len() > MAX_PARAMS + 1000` (upstream,
  `crates/constraints/src/lib.rs:187`); `UnsupportedRef` when either reference is a line or a
  point.
- **Tests:** `tangent_circles_keep_their_initial_side` — two overlapping circles, assert the
  solve does not invert them; `tangent_circle_circle_costs_two_degrees_of_freedom`;
  `auxiliary_contact_parameters_are_never_written_back`.
- **Acceptance:** After solving, the distance between centres equals `r0 + r1` (external) or
  `|r0 - r1|` (internal) to tolerance, with the sign chosen once at interpretation.
- **Risk:** The contact parameter can converge to the far side of a circle, giving an internal
  tangency the user did not ask for. Fallback: the settle pass is the place to detect a
  contact point on the wrong half and report `Conflict`.

<!-- CHUNK-2-END -->
