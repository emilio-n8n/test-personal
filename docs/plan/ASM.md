# ASM — Assemblies

Exhaustive task index for the `ASM` domain of [`docs/master-plan.md`](../master-plan.md)
(`ASM-001..006`, weight 12 %, owner `crates/asm/**` — new crate, package `cadcraft-asm`).

This file is a **specification, not code**. Nothing here has been compiled or executed.

---

## Conventions and invariants

Every task below is written against these. A task that contradicts one of them is wrong.

### 1. Coordinates, units, handedness

- Internal units are **model units**: f64, right-handed, `+Z` out of the sketch plane, origin at the
  component's own datum. They are the same units `cadcraft_geom::Vec3` and `cadcraft_model` use.
- `cadcraft-asm` performs **no implicit unit conversion**. A component loaded from another document
  records that document's `MEASUREMENT` and a `unit_scale` factor, and the conversion happens once,
  at insertion (`ASM-014`), never inside the solver. This is the rule `crates/io` already follows
  for `paper_unit_mm` — one conversion site, named.
- Every coordinate in any report, error message or export is in the **assembly root frame**, never
  in a component's own frame. A message that says "at (12, 0, 3)" and does not say which frame is
  not acceptable.

### 2. Orientation

- An occurrence's transform is a **proper** rigid transform plus a **uniform** scale:
  `|det(basis)| = +1`, basis columns unit and mutually orthogonal.
- A negative determinant is `Err(AsmError::Mirrored)`. It is **never** silently accepted: a mirrored
  solid inverts its volume sign and `Solid::validate` then rejects it as `NonPositiveVolume`, which
  is a far worse error message than the one that would have named the cause.
- Non-uniform scale is refused (`Err(AsmError::NonUniformScale)`). The kernel's tolerance model is
  diagonal-scaled (`cadcraft_solid::tolerance_for`), and a stretched body would be checked against
  a tolerance that is wrong in one axis only.

### 3. Tolerance

- The single tolerance function is `cadcraft_solid::tolerance_for(diagonal)`, i.e.
  `(diagonal * 1e-9).clamp(1e-12, 1e-3)` — reused, **not re-implemented**.
- Every predicate that compares geometry takes an explicit `tol: f64` parameter. There is no
  floating-point `==` on a geometric quantity anywhere in the crate.
- Interference tolerance is a **stated, caller-supplied** number, defaulting to the larger of the two
  operands' kernel tolerances, and it is always reported back with the result (see `ASM-098`).

### 4. The last-good rule

From `crates/model/src/rebuild.rs`, extended to assemblies without weakening it:

> **A failed rebuild never destroys the last good state.**

For an assembly this has three parts, and each is its own acceptance criterion below:

1. A **component** whose feature tree no longer builds keeps its last good `Solid` and is reported
   `State::Failed(msg)` — never an empty body, never a removed component (`ASM-011`).
2. An **occurrence** of a failed component keeps its last good `Transform` and keeps drawing the
   component's last good solid (`ASM-012`, `ASM-140`).
3. A **joint** whose reference no longer resolves is reported `Failed` with the occurrence still
   where it was; the occurrence is never moved to "somewhere that looks right" (`ASM-143`).

### 5. No silent wrong answers

- Every refusal is a **named** `AsmError` variant whose `Display` names what could not be done.
  A generic `Other(String)` is not acceptable for any case named in this file.
- A joint that cannot be satisfied is refused with one of exactly three reasons
  (`ASM-078`, `ASM-079`, `ASM-080`): `OverConstrained`, `Conflicting`, `ZeroDof`.
- "The solver converged to something" is never reported without the residual norm.

### 6. Determinism

- Anything that is *reported* (a solve report, an interference list, a BOM, a file listing) is
  ordered by an `Ord` key, never by `HashMap` iteration. `crates/solid/src/solid.rs` already makes
  this point for its error selection; the same rule applies here.
- Two solves of the same assembly from the same initial state produce bit-identical transforms.

### 7. Caps and safety

- `MAX_ASSEMBLY_DEPTH: usize = 16` — same value and same meaning as `cadcraft_doc::MAX_BLOCK_DEPTH`.
  Deeper nesting is `Err(AsmError::TooDeep)`, never truncated.
- `MAX_OCCURRENCES`, `MAX_COMPONENTS`, `MAX_JOINTS`, `MAX_SOLVE_ROWS` are public constants in
  `crates/asm/src/limits.rs`. Every one is checked before allocation.
- No `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`, `unsafe`
  (`#![deny(...)]` + `#![forbid(unsafe_code)]` at the crate root).
- Hostile input — a NaN transform, a zero-length joint axis, a component path of 100 000 bytes —
  is an `Err` naming the field. There is a property/fuzz task for it (`ASM-091`, `ASM-107`).

### 8. Layering

- `cadcraft-asm` is **L3**: it may use `cadcraft-geom` (L0), `cadcraft-solid` (L0),
  `cadcraft-constraints` (L2), `cadcraft-model` (L3). It must **not** use `cadcraft-render` (L2 with
  a heavier surface), `cadcraft-io` (L3), `cadcraft-doc` (L1) or anything at L4+.
- Nothing below L5 depends on egui/eframe/winit/wgpu. `cadcraft-asm` must stay wasm-clean.

### 9. Reuse ledger — what `cadcraft-asm` borrows and what it deliberately does not

**`cadcraft_doc::Entity` is NOT used for occurrences.** Evidence:

- `EntityKind` (`crates/doc/src/entity.rs:977`) is a closed enum whose every arm has a DXF group
  code (`dxf_name()`, line 1030) and an AutoCAD Properties-panel name (`type_name()`, line 1031).
  An assembly occurrence is not a DXF entity; adding an arm means an arm in
  `crates/io/src/dxf_write.rs`, `crates/io/src/dxf_read.rs`, `crates/render/src/lib.rs::build_entities`,
  `crates/doc/src/extents.rs::bounds_in` and `EntityKind::prims`/`grips` for an object no DXF file
  can contain.
- `crates/engine/src/lib.rs:674-680` states the rule in the code: `Session::solid_model` "is not part
  of the 2D drawing and shares none of its machinery". An occurrence belongs beside it.
- `Entity` has no slot for a `Solid`, and `prims()` (`crates/doc/src/entity.rs:1103`) returns 2D
  primitives; a solid has none.

**What *is* reused**, deliberately and with evidence:

| Reused | From | Why |
|---|---|---|
| `cadcraft_doc::Handle` | `crates/doc/src/entity.rs:14` | The same opaque `u64` newtype with `.hex()`. One handle type across the 2D and 3D worlds; reports print the same. |
| `cadcraft_doc::Attrib` shape | `crates/doc/src/entity.rs:340` | tag / text / props / multiline / modes. An occurrence's overrides are attributes. |
| `cadcraft_doc::XrefStatus` | `crates/doc/src/tables.rs:683` | The exact five-state load model a component reference needs. Reused verbatim, not re-invented (`ASM-145`). |
| `cadcraft_doc::Block` *shape* | `crates/doc/src/tables.rs:647` | name, base point, description, explodable, units, array params. `Component` mirrors this field for field. |
| `cadcraft_doc::EntityKind::Insert` *shape* | `crates/doc/src/entity.rs:480` | insert / scale / rotation / attribs. `Occurrence` is the 3D `INSERT`. |
| `cadcraft_solid::tolerance_for` | `crates/solid/src/lib.rs:153` | The one tolerance function. |
| `cadcraft_solid::{Solid, EdgeRef, Edge, Face, Plane, Bounds3, mass_properties, mesh_from_solid}` | `crates/solid/src/lib.rs:85-95` | All of it, unmodified. |
| `cadcraft_model::{Model, Record, State, rebuild, rebuild_after}` | `crates/model/src/lib.rs:54` | The last-good rule and dirty propagation, reused rather than reimplemented. |
| `cadcraft_constraints::lm` | `crates/constraints/src/lm.rs` | Damped Gauss-Newton + `rank_qr` + `null_space`. **Currently `pub(crate)` — see `ASM-066`, an interface request.** |

### 10. Core types these tasks agree on

Declared in `crates/asm/src/lib.rs` unless a task says otherwise. Every task's `Contract` writes
against exactly these shapes.

```rust
pub type Result<T> = std::result::Result<T, AsmError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OccId(pub u32);

/// A proper rigid transform plus a uniform positive scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub origin: Vec3,
    /// Columns of a right-handed orthonormal basis; `det == +1`.
    pub basis: [Vec3; 3],
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum State { Built, Pending, Failed(String), Blocked(String), Suppressed }

#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub id: OccId,
    pub component: ComponentId,
    pub transform: Transform,
    pub attribs: Vec<cadcraft_doc::Attrib>,
    pub suppressed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assembly {
    pub name: String,
    pub components: BTreeMap<ComponentId, Component>,
    pub occurrences: BTreeMap<OccId, Occurrence>,
    pub ground: OccId,
    pub joints: BTreeMap<JointId, Joint>,
    pub records: BTreeMap<OccId, OccRecord>,
}

#[derive(Clone, Debug, Default)]
pub struct OccRecord {
    pub state: State,
    /// Kept across a failure, exactly like `cadcraft_model::Record::last_good`.
    pub last_good: Option<Transform>,
}
```

### 11. File ownership

| Owner | Files |
|---|---|
| **ASM** | `crates/asm/**` |
| **A** (kernel) | `crates/solid/src/**`, `crates/solid/tests/**` — never touched by ASM |
| **C2** (2D/doc) | `crates/doc/src/**`, `crates/dxf/**`, `crates/color/**` |
| **MOD** | `crates/model/src/**` |
| **SOL** (2D solver) | `crates/constraints/src/**` |
| **D** (data) | `crates/io/src/**` |
| **U** (UI) | `crates/ui-egui/src/**`, `crates/render/src/scene3.rs` |
| **ENG** | `crates/engine/src/**` |
| **C** (orchestrator) | root `Cargo.toml`, `xtask/**`, `docs/**` |

Any task whose `Files` column names a file owned by someone else, and which *modifies* it, is
`BLOCKED` on that owner.

### 12. Task counts

| Epic | Tasks | Range |
|---|---:|---|
| `ASM-MDL` | 28 | 001–028 |
| `ASM-JNT` | 36 | 029–064 |
| `ASM-SLV` | 28 | 065–092 |
| `ASM-INF` | 18 | 093–110 |
| `ASM-MOT` | 14 | 111–124 |
| `ASM-BOM` | 14 | 125–138 |
| `ASM-PLT` | 14 | 139–152 |
| **Total** | **152** | |

---
---

## ASM-MDL — component, occurrence, instance, hierarchy, last-good build contract

### ASM-001 — Create the `cadcraft-asm` crate skeleton and its error type
- **State:** READY
- **Depends on:** —
- **Files:** `Cargo.toml` (owner **C**), `crates/asm/Cargo.toml`, `crates/asm/src/lib.rs`, `crates/asm/src/error.rs`, `crates/asm/src/limits.rs`, `crates/asm/tests/` (owner **ASM**)
- **Contract:**
  ```rust
  pub enum AsmError {
      EmptyName, Duplicate(String), NoSuchComponent(ComponentId), NoSuchOccurrence(OccId),
      NoSuchJoint(JointId), NonFinite, BadTolerance(f64), Mirrored, NonUniformScale,
      TooDeep { path: Vec<ComponentId> }, CyclicComponent { path: Vec<ComponentId> },
      OverConstrained { redundant: Vec<JointId> }, Conflicting { joints: Vec<JointId> },
      ZeroDof { occurrence: OccId, joints: Vec<JointId> }, NotConverged { residual: f64, iterations: usize },
      UnresolvedReference { joint: JointId, occurrence: OccId },
      InterferenceNotMeasurable(String), TooComplex { triangles: usize, max: usize },
      Refused(String),
  }
  pub type Result<T> = std::result::Result<T, AsmError>;
  // limits.rs
  pub const MAX_ASSEMBLY_DEPTH: usize = 16;
  pub const MAX_COMPONENTS: usize = 20_000;
  pub const MAX_OCCURRENCES: usize = 50_000;
  pub const MAX_JOINTS: usize = 100_000;
  pub const MAX_SOLVE_ROWS: usize = 4_000;
  ```
- **Behaviour:** The crate root carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]` and `#![forbid(unsafe_code)]`. Every `AsmError` variant's `thiserror` message **names** what could not be done and includes the offending ids. `lib.rs` documents the section-2 and section-3 invariants above verbatim as crate docs, the way `crates/solid/src/lib.rs` documents I1–I5.
- **Refusals:** A variant whose `Display` is empty or is only `Other`: rejected at review.
- **Tests:** `error_messages_name_their_ids` — asserts `AsmError::NoSuchOccurrence(OccId(7))` displays `"occurrence 7"`; `every_variant_has_a_display` — a table asserting no `to_string()` is empty.
- **Acceptance:** `cargo build -p cadcraft-asm` succeeds with no `unwrap` and no `unsafe` (NOT EXECUTED here — orchestrator runs it), and `cargo xtask layers` reports `cadcraft-asm` at L3 with dependencies only from L0/L2/L3.
- **Risk:** The orchestrator's workspace already uses `members = ["crates/*"]`, so the crate is picked up automatically; the only hand edit is a `cadcraft-asm = { path = "crates/asm" }` line in `[workspace.dependencies]`. Fallback: none needed.

### ASM-002 — Opaque `ComponentId` and `OccId` with stable ordering
- **State:** READY
- **Depends on:** ASM-001
- **Files:** `crates/asm/src/id.rs` (owner **ASM**)
- **Contract:** `pub struct ComponentId(pub u32);` / `pub struct OccId(pub u32);`, both `Copy + Eq + Hash + PartialOrd + Ord + Serialize + Deserialize`, plus `pub fn hex(self) -> String` producing the same `{:X}` form as `cadcraft_doc::Handle::hex` (`crates/doc/src/entity.rs:17`).
- **Behaviour:** Ids are allocated from a monotonically increasing counter stored in the `Assembly`, never reused after a delete within a session, so an id in an undo snapshot always names the same object. Ordering is by the integer, which is allocation order — deterministic and stable across a save/load round trip because the counter is serialised. The `hex()` form exists so a control-channel or MCP reply can carry an id that is textually identical to a drawing handle's.
- **Refusals:** `OccId(0)` as an id — reserved for "no occurrence" and rejected by `Assembly::insert_occurrence` (`ASM-014`).
- **Tests:** `ids_order_by_allocation` — inserting three occurrences yields ids 1,2,3 and a `Vec` sorted by id matches insertion order; `hex_matches_the_drawing_handle_form` — `OccId(0x2A).hex() == "2A"`.
- **Acceptance:** No task outside `id.rs` constructs an id from anything but `Assembly::next_*_id`.
- **Risk:** A future parallel solve that allocates ids would break the ordering guarantee. Fallback: allocate under one `&mut Assembly` method, never in a worker.

### ASM-003 — `Transform`: origin, orthonormal basis, uniform scale
- **State:** READY
- **Depends on:** ASM-001, ASM-002
- **Files:** `crates/asm/src/transform.rs` (owner **ASM**)
- **Contract:**
  ```rust
  impl Transform {
      pub const IDENTITY: Transform;
      pub fn new(origin: Vec3, basis: [Vec3; 3], scale: f64) -> Result<Transform, AsmError>;
      pub fn from_axis_angle(origin: Vec3, axis: Vec3, angle: f64, scale: f64) -> Result<Transform, AsmError>;
      pub fn from_mat4(m: &cadcraft_geom::Mat4) -> Result<Transform, AsmError>;
      pub fn to_mat4(&self) -> cadcraft_geom::Mat4;
      pub fn apply(&self, p: Vec3) -> Vec3;
      pub fn inverse(&self) -> Result<Transform, AsmError>;
      pub fn then(&self, o: &Transform) -> Transform;      // self ∘ o
      pub fn det(&self) -> f64;
      pub fn is_rigid(&self) -> bool;                      // within 1e-12
      pub fn axis_angle(&self) -> Result<(Vec3, f64), AsmError>;
  }
  ```
- **Behaviour:** `new` re-orthonormalises the basis by Gram-Schmidt when it is within `1e-9` of orthonormal, and **refuses** when it is not — a caller who passed a sheared basis has a bug, and quietly fixing it hides it. Composition is `then`, and `Transform` composition is exactly the `cadcraft_geom::Mat4::mul` order (`self * o` applies `o` first). `scale` is a single f64, so composition multiplies scales and never produces a non-uniform one. `axis_angle` returns `Err(AsmError::Refused)` for the identity rotation and for a scale that is not 1 combined with a 0 or π rotation, where the decomposition is not unique.
- **Refusals:** non-finite origin/basis/scale → `NonFinite`; `scale <= 0` → `NonUniformScale`; `det < 0` → `Mirrored`; a basis whose columns are not within `1e-9` of unit and mutually perpendicular → `NonUniformScale`.
- **Tests:** `composition_matches_mat4_mul` — for 200 pseudo-random `(Transform, Transform)` pairs, `a.then(&b).to_mat4()` equals `a.to_mat4().mul(&b.to_mat4())` to 1e-12; `inverse_round_trips` — `t.apply(t.inverse()?.apply(p)) == p` within 1e-9 for 10⁴ points; `a_mirror_is_refused` — `Transform::new(ZERO, [-X, Y, Z], 1.0)` is `Err(Mirrored)`; `identity_is_the_identity` — `IDENTITY.apply(p) == p` exactly.
- **Acceptance:** `then` and `inverse` are property-tested against `Mat4`; no task outside this file builds a `Transform` by hand.
- **Risk:** Gram-Schmidt on a nearly-degenerate basis amplifies error. Fallback: reject anything below the `1e-9` threshold rather than trying to recover.

### ASM-004 — Transform validation is a named refusal, never a correction
- **State:** READY
- **Depends on:** ASM-003
- **Files:** `crates/asm/src/transform.rs` (owner **ASM**)
- **Contract:** `pub fn Transform::validate(&self) -> Result<(), AsmError>;` plus `pub fn Transform::try_new_lenient(origin: Vec3, basis: [Vec3; 3], scale: f64) -> Result<Transform, AsmError>;` (re-orthonormalises within tolerance, refuses outside it).
- **Behaviour:** `validate` is the strict form used on every load from a file and on every command parameter; `try_new_lenient` is used only where the input came from a matrix we ourselves composed, and is documented as such. The two are separate names on purpose: a caller who wants correction must ask for it, because the difference between "the user typed a scale of −1" and "the user typed a scale of 1.0000000004" is worth reporting.
- **Refusals:** `scale == -1.0` → `NonUniformScale`, not silently made positive. A basis with one column zero-length → `NonUniformScale`.
- **Tests:** `strict_and_lenient_agree_above_the_threshold` — a basis perturbed by 1e-10 validates and re-orthonormalises to within 1e-15; the same basis perturbed by 1e-6 is `Err` from both.
- **Acceptance:** Every `Assembly` load path calls `validate`, never `try_new_lenient`.
- **Risk:** A file written by another tool with a slightly sheared matrix would refuse to open. Fallback: the refusal message names the offending column and the residual, so the file is diagnosable.

### ASM-005 — `Occurrence`: a placement of a component, with no geometry
- **State:** READY
- **Depends on:** ASM-002, ASM-003
- **Files:** `crates/asm/src/model.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct Occurrence {
      pub id: OccId, pub component: ComponentId, pub transform: Transform,
      pub attribs: Vec<cadcraft_doc::Attrib>, pub suppressed: bool,
  }
  impl Occurrence {
      pub fn new(id: OccId, component: ComponentId, transform: Transform) -> Occurrence;
      pub fn placement(&self) -> &Transform;   // the user's intent, never the solved pose
  }
  ```
- **Behaviour:** **New type, not `cadcraft_doc::Entity`** — see preamble §9. An occurrence carries a `Solid`-bearing component id and a `Transform`; there is no `EntityKind`, no `Common`, no layer. Two fields earn their keep: `transform` is the **stored placement** the user set (never overwritten by a solve), and `placement()` exists so that a caller who means "where the user put it" and a caller who means "where the solver put it" (`ASM-065`) cannot be confused for one line of code. `attribs` is `Vec<cadcraft_doc::Attrib>` verbatim — the type is reused, not reimplemented.
- **Refusals:** An occurrence whose `component` names nothing is `Err(NoSuchComponent)` at insert time (`ASM-014`), not at solve time.
- **Tests:** `an_occurrence_holds_no_geometry` — asserts `std::mem::size_of::<Occurrence>()` has no `Vec<Face>` and that two occurrences of one component share the same `ComponentId` (asserted structurally via `Component::occurrences()` returning the list).
- **Acceptance:** `Occurrence` is `serde` round-trip exact, including `attribs`.
- **Risk:** Storing the solved pose back into `Occurrence::transform` would make "placement" and "pose" the same field and quietly rewrite user intent. Fallback: the solve writes `records[occ].last_good`, never `occurrence.transform`.

### ASM-006 — Instance vs occurrence: one definition, many placements
- **State:** READY
- **Depends on:** ASM-005, ASM-007
- **Files:** `crates/asm/src/model.rs` (owner **ASM**)
- **Contract:**
  ```rust
  impl Component {
      pub fn occurrences<'a>(&'a self, a: &'a Assembly) -> Vec<OccId>;   // sorted
  }
  pub fn Assembly::shared_solid<'a>(&'a self, occ: OccId) -> Result<&'a Solid, AsmError>;
  ```
- **Behaviour:** A `Component` is the *definition*: one `cadcraft_model::Model`, one built `Solid`, one identity. An `Occurrence` is one *placement* of it. `shared_solid` is the whole point of the split — the solid is built **once** per component no matter how many occurrences there are, and every consumer (interference, HLR, mass, bounds) asks for it through this function so no consumer accidentally rebuilds. This is the direct analogue of `cadcraft_doc::Block` (one definition) and `EntityKind::Insert` (many placements), and it is what makes a 500-instance assembly cost one build.
- **Refusals:** `shared_solid` on a component that has never built → `Err(Refused("component N has no solid"))` naming the component's `State`.
- **Tests:** `one_build_serves_every_occurrence` — an assembly with one component and 100 occurrences reports `assembly.builds() == 1`; `the_solid_is_the_same_object` — `shared_solid(a).as_ptr() == shared_solid(b).as_ptr()` for two occurrences of one component.
- **Acceptance:** No path in the crate constructs a `Solid` per occurrence; a test asserts the build count.
- **Risk:** A future "make unique" operation would need to copy the component; not in scope here. Fallback: `Component::clone_as_new` added later with its own id.

### ASM-007 — `Component`: a model, its build record, and its last good solid
- **State:** READY
- **Depends on:** ASM-005
- **Files:** `crates/asm/src/component.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct Component {
      pub id: ComponentId,
      pub name: String,
      /// Origin the component's own geometry is measured from.
      pub base: Vec3,
      pub model: cadcraft_model::Model,
      pub material: Material,
      pub explodable: bool,
      pub xref: Option<Xref>,          // None = this document
  }
  pub struct Xref { pub path: String, pub status: cadcraft_doc::XrefStatus, pub revision: Option<String> }
  pub struct ComponentRecord {
      pub state: State,
      pub last_good: Option<Solid>,   // mirrors cadcraft_model::Record::last_good
      pub problems: Vec<cadcraft_model::Problem>,
  }
  ```
- **Behaviour:** **New type mirroring `cadcraft_doc::Block`'s field set** (name, base, description→problems, explodable, xref_path/status), because `cadcraft_doc::Block` stores 2D `Entity`s and this stores a `cadcraft_model::Model`. The field-for-field correspondence is deliberate and documented in the struct doc comment. `base` exists so a component's origin is the point its own geometry was built about, matching `cadcraft_model::Sketch`'s "the frame the profile was drawn in" rule (`crates/model/src/feature.rs:26-34`).
- **Refusals:** A component with an empty `name` → `Err(EmptyName)`.
- **Tests:** `a_component_is_built_once_per_record` — two `ComponentRecord`s for one `ComponentId` cannot coexist (asserted by construction: the record map is keyed by id); `last_good_survives_a_failed_rebuild` — after a rebuild that returns `problems`, `record.last_good` equals the pre-rebuild solid byte for byte.
- **Acceptance:** `Component` and `cadcraft_model::Document` do not duplicate parameter storage; the parameters live in `model.parameters` and are read through `Model::values()`.
- **Risk:** Two models means two parameter namespaces; an assembly-level parameter that a component reads needs a rule. Fallback and rule: **assembly parameters shadow nothing**; see `ASM-054` for the binding contract.

### ASM-008 — One solid per component, cached, rebuilt once
- **State:** READY
- **Depends on:** ASM-007, ASM-011
- **Files:** `crates/asm/src/build.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct BuildReport { pub built: usize, pub problems: Vec<cadcraft_model::Problem>, pub solid: Option<Solid> }
  pub fn Assembly::build(&mut self, dirty: Option<&[ComponentId]>) -> BuildReport;
  pub fn Assembly::rebuild_after(&mut self, changed: &[cadcraft_model::Dep]) -> BuildReport;
  ```
- **Behaviour:** Delegates straight to `cadcraft_model::rebuild` / `rebuild_after` per component, so the dirty-propagation rules (parameter → formula → dependent feature, and the tool-feature carve-out at `crates/model/src/rebuild.rs:137`) are **reused, not reimplemented**. A component not in `dirty` keeps its record and its solid untouched. The assembly's `solid` in the report is the **ground occurrence's** solid, which is the only one a caller can name without saying which.
- **Refusals:** `rebuild_after` with a `Dep::Feature(name)` naming no component feature → the component reports `Failed`, and the assembly report lists it. Never a panic.
- **Tests:** `a_untouched_component_is_not_rebuilt` — after `rebuild_after` on component A, B's `last_good` pointer is unchanged; `the_ground_solid_is_what_the_report_carries`; `hostile_parameters_do_not_panic` — a parameter set to `f64::NAN` through `Model::set` yields `Failed` and leaves `last_good` intact.
- **Acceptance:** `Assembly::build` contains no geometry code; it only calls `cadcraft_model`.
- **Risk:** `cadcraft_model::rebuild` is not re-entrant across components today because it takes `&mut Model` per model — which is fine here because each component owns its model. Fallback: if a future shared-parameter design needs a cross-component pass, that is `ASM-054`'s scope, not this one.

### ASM-009 — Dirty propagation from a component to its occurrences
- **State:** READY
- **Depends on:** ASM-008
- **Files:** `crates/asm/src/build.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::dirty_occurrences(&self, changed: &cadcraft_model::Dep) -> Vec<OccId>;` and `pub fn Assembly::occurrences_of(&self, c: ComponentId) -> Vec<OccId>;`
- **Behaviour:** A parameter changed in component `C` dirties `C`, and therefore every occurrence of `C`. It does **not** dirty anything else, and this task's whole content is proving that: the returned list is exactly `occurrences_of(C)`, sorted. It does *not* dirty occurrences of other components even when they are jointed to an occurrence of `C` — a joint constraint may need re-solving, but the occurrence's *placement* is user data and is not dirtied by a neighbour's geometry.
- **Refusals:** `occurrences_of` for an unknown component → `Err(NoSuchComponent)`.
- **Tests:** `changing_a_parameter_dirties_only_its_own_occurrences` — with components A and B each placed twice, changing A's parameter returns exactly A's two occurrences; `dirty_occurrences_is_sorted_and_deduplicated`.
- **Acceptance:** The jointed-neighbour case is explicitly *not* dirtied, and a test says so in words.
- **Risk:** Someone will want "re-solve everything when anything changes". Fallback: `ASM-089` re-solves on demand; dirtiness here is only about rebuilding geometry.

### ASM-010 — Component registry and the occurrence map
- **State:** READY
- **Depends on:** ASM-002, ASM-007
- **Files:** `crates/asm/src/model.rs` (owner **ASM**)
- **Contract:**
  ```rust
  impl Assembly {
      pub fn new(name: &str) -> Assembly;
      pub fn component(&self, id: ComponentId) -> Result<&Component, AsmError>;
      pub fn component_mut(&mut self, id: ComponentId) -> Result<&mut Component, AsmError>;
      pub fn occurrence(&self, id: OccId) -> Result<&Occurrence, AsmError>;
      pub fn occurrence_mut(&mut self, id: OccId) -> Result<&mut Occurrence, AsmError>;
      pub fn ids(&self) -> Vec<ComponentId>;   // sorted
      pub fn occurrence_ids(&self) -> Vec<OccId>;  // sorted
  }
  ```
- **Behaviour:** Both maps are `BTreeMap` keyed by the `Ord` id, so every listing is ordered without a sort call and a `Debug` print is stable. The counters `next_component`/`next_occurrence` live here and are the only allocation sites for ids.
- **Refusals:** Mutating through a lookup that failed leaves the assembly untouched — the `Result` is returned before any borrow is taken.
- **Tests:** `maps_are_btree_so_listing_is_ordered` — insert in reverse and assert `ids()` is ascending; `a_failed_lookup_does_not_mutate` — `component_mut(CompId(99))` is `Err` and a subsequent `ids()` is unchanged.
- **Acceptance:** No `HashMap` appears in `Assembly`.
- **Risk:** `BTreeMap` is slower for bulk insert. Irrelevant at these cardinalities; recorded so nobody "optimises" it into a `HashMap` and breaks the determinism rule.

### ASM-011 — **Last-good build contract for components**
- **State:** READY
- **Depends on:** ASM-007, ASM-008
- **Files:** `crates/asm/src/build.rs`, `crates/asm/tests/last_good.rs` (owner **ASM**)
- **Contract:** No new public API. The contract is on `ComponentRecord`: after any `Assembly::build`/`rebuild_after`, for every component `c`, if `c`'s rebuild did not succeed then `record[c].last_good` is **the same `Solid` as before the call** and `record[c].state` is `Failed(msg)` or `Blocked(msg)`.
- **Behaviour:** This is `crates/model/src/rebuild.rs`'s rule, restated at assembly level because an assembly has a second failure mode the model does not: a component that builds fine but whose *geometry* is unusable for this assembly (a transform that sends it to infinity is separate; a component whose model has an error is this). A `Blocked` component is one whose own build failed; `State` is copied from `cadcraft_model::State` verbatim so the two vocabularies never diverge.
- **Refusals:** A build that would overwrite a good solid with a broken one is refused by construction: `rebuild.rs` already returns the record with the old `last_good`. ASM's job is to *not* add a path that bypasses it.
- **Tests:** `a_failed_component_keeps_its_last_good_solid` — build ok, break the model (set a parameter so an extrude depth is negative), rebuild, assert `state == Failed` and `last_good` deep-equals the first solid; `a_blocked_component_keeps_its_last_good_solid` — break the *first* feature so the rest are `Blocked`, and assert each keeps its own last good; `the_assembly_still_shows_something` — after every component failing, `shared_solid(ground)` still returns the old solid.
- **Acceptance:** This is the acceptance criterion for the whole epic: a test exists, it is named, and it asserts byte equality of the solid, not just `is_some()`.
- **Risk:** Someone adds a "clear stale solids on error" convenience. Fallback: that convenience does not exist; the rule is in the crate docs.

### ASM-012 — **Last-good transform contract for occurrences**
- **State:** READY
- **Depends on:** ASM-005, ASM-011
- **Files:** `crates/asm/src/model.rs`, `crates/asm/tests/last_good.rs` (owner **ASM**)
- **Contract:**
  ```rust
  impl Assembly {
      pub fn set_transform(&mut self, occ: OccId, t: Transform) -> Result<(), AsmError>;
      /// The transform the assembly currently shows: the solved pose if there is one,
      /// otherwise the stored placement.
      pub fn effective_transform(&self, occ: OccId) -> Result<Transform, AsmError>;
      pub fn commit_solved(&mut self, occ: OccId, solved: Transform) -> Result<(), AsmError>;
  }
  ```
- **Behaviour:** `Occurrence::transform` is the **user's placement** and is written only by `set_transform` (a command or a script) and by `commit_solved`. The pose actually drawn is `effective_transform`, which reads `records[occ].last_good` and falls back to `occurrence.transform`. A component that fails to rebuild therefore leaves every one of its occurrences exactly where it was, drawing the last good geometry — the occurrence is *not* moved, *not* hidden, and *not* removed. This mirrors `cadcraft_model::Record` precisely: the stored value survives, the derived value fails.
- **Refusals:** `commit_solved` with a transform that fails `Transform::validate` → `Err`, and `last_good` is left at its previous value.
- **Tests:** `a_failed_component_does_not_move_its_occurrences` — snapshot every occurrence's `effective_transform`, break the component, rebuild, assert all are unchanged; `set_transform_does_not_touch_last_good`; `commit_solved_with_a_mirror_is_refused` — `Err(Mirrored)` and `last_good` unchanged.
- **Acceptance:** Stated verbatim in `crates/asm/src/lib.rs` crate docs as rule 2 of §4, and pinned by a test.
- **Risk:** A consumer reading `occurrence.transform` directly gets the placement, not the pose, and will draw the wrong thing during a failed rebuild. Fallback: make `transform` private to the crate and expose only `placement()` and `effective_transform()`; the `pub` field is granted in this task's `Contract` but §`ASM-005` documents the hazard.

### ASM-013 — Exactly one ground occurrence, and it is not removable
- **State:** READY
- **Depends on:** ASM-010
- **Files:** `crates/asm/src/model.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct GroundRef(pub OccId);
  impl Assembly { pub fn ground(&self) -> OccRef; }
  pub fn remove_occurrence(&mut self, id: OccId) -> Result<Vec<JointId>, AsmError>;
  ```
- **Behaviour:** `Assembly::new` creates no occurrences; the first `insert_occurrence` becomes the ground and is `Transform::IDENTITY`. The ground is what every other occurrence's pose is measured from, so it has no joint and cannot be deleted: removing it is `Err(Refused("the ground occurrence 1 cannot be removed"))`. Deleting any other occurrence returns the ids of the joints that referenced it, exactly as `cadcraft_model::Model::remove` returns the features it truncated (`crates/model/src/feature.rs:309`).
- **Refusals:** Removing the ground → `Err(Refused)` naming the id. Grounding a second occurrence → `Err(Refused)`.
- **Tests:** `the_first_occurrence_is_the_ground`; `the_ground_cannot_be_removed`; `removing_an_occurrence_names_the_joints_it_took` — two joints reference occ 3, removing it returns `[joint 1, joint 2]` sorted.
- **Acceptance:** Every `Assembly` has exactly one ground; a property test asserts `ground() != Err` on every constructed assembly.
- **Risk:** A "re-ground" operation (move the ground to another occurrence) is not in this index. Fallback: `Err(Refused("re-grounding is not supported; delete the assembly's joints first")`.

### ASM-014 — Insert an occurrence
- **State:** READY
- **Depends on:** ASM-005, ASM-013
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub fn Assembly::insert_occurrence(&mut self, component: ComponentId, transform: Transform) -> Result<OccId, AsmError>;
  ```
- **Behaviour:** Allocates the id, validates the transform, records `OccRecord { state: Pending, last_good: transform }`, and if this is the first occurrence makes it the ground with `Transform::IDENTITY` regardless of the transform passed (a ground that is not at the origin is a surprise; the transform of the first occurrence is applied to its *component*, not to the frame). Documented on the method.
- **Refusals:** unknown component → `NoSuchComponent`; non-finite transform → `NonFinite`; mirror → `Mirrored`; at `MAX_OCCURRENCES` → `Refused("50000 occurrences is the limit")`.
- **Tests:** `insert_returns_a_fresh_id_and_a_pending_record`; `hostile_transforms_are_refused` — a table over NaN, ∞, zero scale, mirrored basis, and each is the named variant.
- **Acceptance:** Command `asm.occurrence.insert` exists with a JSON `run` and its own test (`ASM-152`).
- **Risk:** none material.

### ASM-015 — Move an occurrence
- **State:** READY
- **Depends on:** ASM-014, ASM-003
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::move_occurrence(&mut self, occ: OccId, origin: Vec3) -> Result<(), AsmError>;` — translation only, rotation preserved.
- **Behaviour:** Writes `occurrence.transform.origin` and `records[occ].last_good`, preserving `basis` and `scale`. If the occurrence is jointed and the joints are solved, the move is applied **and** the assembly is marked for re-solve (`ASM-092` owns the re-solve trigger); until it is solved, the stored placement is the one shown.
- **Refusals:** non-finite origin → `NonFinite`; unknown id → `NoSuchOccurrence`; moving the ground is allowed (it moves the frame of reference, which is legal) but is documented as such.
- **Tests:** `moving_preserves_the_rotation`; `moving_the_ground_moves_the_frame` — a ground moved by `(10,0,0)` changes every other occurrence's `effective_transform` by the same translation once re-solved, and does not change it before.
- **Acceptance:** Move is rotation-preserving and scale-preserving, tested.
- **Risk:** Moving the ground silently changes every joint's meaning. Fallback: the ground's move is allowed but the solve report names the ground as the frame change (`ASM-088`).

### ASM-016 — Rotate an occurrence
- **State:** READY
- **Depends on:** ASM-015
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::rotate_occurrence(&mut self, occ: OccId, axis: Vec3, angle: f64) -> Result<(), AsmError>;` — right-handed, radians, about the occurrence's own origin.
- **Behaviour:** Post-multiplies the basis by the rotation: `basis' = R · basis`, so the rotation is about the component's own origin, not the world origin. An axis of length `< 1e-12` or a non-finite angle is refused. A full turn is allowed (the solver's revolute joints produce them); there is no angle clamp, because clamping a rotation is exactly how an assembly silently drifts.
- **Refusals:** zero-length axis → `BadDirection`; non-finite angle → `NonFinite`.
- **Tests:** `rotation_is_about_the_occurrence_origin` — rotating by π about Z maps `(1,0,0)` in component space to `(-1,0,0)` regardless of where the occurrence sits; `four_pi_is_the_identity` — 4π gives a basis within 1e-12 of the start.
- **Acceptance:** Rotation order is documented as right-handed about the component origin, and a test pins it.
- **Risk:** A negative angle and a positive angle about a different axis are the same rotation; ambiguity is resolved by the right-hand rule, tested.

### ASM-017 — Scale an occurrence (uniform only)
- **State:** READY
- **Depends on:** ASM-016
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::scale_occurrence(&mut self, occ: OccId, factor: f64) -> Result<(), AsmError>;` — `factor > 0`, multiplied into `transform.scale`.
- **Behaviour:** Uniform only. A non-uniform request is a different command in the sense that it does not exist: there is no `non_uniform: bool` parameter to silently ignore. Documented on the method: the kernel's tolerance model is diagonal-scaled (`tolerance_for`), and a body stretched in one axis would be validated against a tolerance that is right in two axes and wrong in the third.
- **Refusals:** `factor <= 0` → `NonUniformScale`; non-finite → `NonFinite`; a resulting absolute scale below `1e-12` or above `1e12` → `Refused` naming both bounds (a scale of `1e-300` makes every tolerance meaningless).
- **Tests:** `scale_multiplies_and_composes` — scaling by 2 twice equals scaling by 4 to 1e-12; `zero_and_negative_scales_are_refused`; `an_extreme_scale_is_refused_not_accepted`.
- **Acceptance:** Non-uniform scale is refused with a named error, and the method name does not admit one.
- **Risk:** A user wanting a shrink-fit will scale to a tiny value and get `Refused`. Fallback: the message names the bounds.

### ASM-018 — Delete an occurrence and report the joints it took
- **State:** READY
- **Depends on:** ASM-013, ASM-031
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::remove_occurrence(&mut self, id: OccId) -> Result<Vec<JointId>, AsmError>;` (declared in ASM-013, implemented here) — and it **removes** those joints, returning their ids sorted.
- **Behaviour:** Deleting an occurrence removes the joints that referenced it, because a joint to a thing that is no longer there has no meaning; the caller is told exactly which, so nothing disappears silently. The ground cannot be deleted (`ASM-013`). Any occurrence that is an *assembly component* (this occurrence is itself a component whose own occurrences exist) is deleted with its whole subtree, and the returned list includes the joints of every occurrence removed, sorted by `JointId`.
- **Refusals:** ground → `Refused`; unknown id → `NoSuchOccurrence`.
- **Tests:** `deleting_reports_and_removes_its_joints`; `deleting_a_subassembly_deletes_the_subtree` — an assembly-in-assembly two deep, deleting the top returns 1 occurrence id plus its joints; `the_joint_ids_are_sorted`.
- **Acceptance:** The returned id list is complete: `assert_eq!(returned, expected_sorted)`, not `contains`.
- **Risk:** Deleting a big subtree is one call and cannot be partially undone by the caller. Fallback: the engine wraps it in a single undo snapshot (`ASM-152`).

### ASM-019 — Suppress and unsuppress an occurrence
- **State:** READY
- **Depends on:** ASM-014
- **Files:** `crates/asm/src/ops.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::suppress(&mut self, occ: OccId, on: bool) -> Result<(), AsmError>;`
- **Behaviour:** A suppressed occurrence keeps its transform and its joints but is **not solved**, is **not** rendered, and is **not** counted for interference or for the BOM. This mirrors `cadcraft_model::Feature::suppressed` (`crates/model/src/feature.rs:209`) exactly, including the wording of the problem message: `"suppressed: this occurrence did not solve"`. A joint whose *first* reference is suppressed is reported `Blocked`, following `rebuild.rs`'s "a blocked feature is not a broken one".
- **Refusals:** unknown id → `NoSuchOccurrence`; suppressing the ground → `Refused` (there would be no frame).
- **Tests:** `a_suppressed_occurrence_keeps_its_placement`; `its_joints_report_blocked_not_failed` — assert the state is `Blocked` and the message names the suppressed occurrence; `the_ground_cannot_be_suppressed`.
- **Acceptance:** Suppress/unsuppress is a round trip with no state loss; tested.
- **Risk:** Suppressing a jointed occurrence changes the DOF; the solve must be re-run. Fallback: `ASM-092`.

### ASM-020 — Mirror is refused, with a reason that names the cause
- **State:** READY
- **Depends on:** ASM-004
- **Files:** `crates/asm/src/transform.rs`, `crates/asm/tests/hostile.rs` (owner **ASM**)
- **Contract:** No new API. Contract: no public function in `cadcraft-asm` accepts a negative determinant. `Transform::new` is the only constructor from raw numbers and it refuses; `from_mat4` refuses; `apply_scaling(&mut self, [f64;3])` does not exist.
- **Behaviour:** The refusal exists because a mirrored shell inverts its signed volume, and `Solid::validate` then returns `SolidError::NonPositiveVolume` (`crates/solid/src/solid.rs:615`) — an error message that names the symptom and not the cause, three layers away from the user's keystroke. The doc comment on `AsmError::Mirrored` says exactly this, and the mirror test says it too.
- **Refusals:** `Err(Mirrored)` for `det < 0`; `Err(Mirrored)` for `det == 0` (degenerate, reported as a mirror because that is what a caller who built a rank-2 basis did).
- **Tests:** `every_negative_determinant_is_refused` — 200 pseudo-random bases with one column negated, all `Err(Mirrored)`; `the_message_points_at_the_cause` — the `Display` of `Mirrored` contains the word "mirror".
- **Acceptance:** No reachable path produces a `Transform` with `det < 0`; a property test says so.
- **Risk:** A future import path (STEP with a negative-determinant placement) will hit this. Fallback: that import reports `Refused("STEP placement N has a mirrored transform")` and refuses the file, which is the correct answer.

### ASM-021 — Hierarchical assembly: an occurrence whose component is an assembly
- **State:** READY
- **Depends on:** ASM-007, ASM-010
- **Files:** `crates/asm/src/assembly_file.rs` (owner **ASM**)
- **Contract:**
  ```rust
  /// A component that is itself an assembly: its definition is a nested `Assembly`,
  /// not a `Model`.
  pub enum Definition { Part(cadcraft_model::Model), Assembly(Box<Assembly>) }
  impl Component { pub fn definition(&self) -> &Definition; }
  pub fn Assembly::flatten(&self) -> Vec<(OccId, PathBuf_in_assembly)>;  // leaf occurrences
  ```
- **Behaviour:** A sub-assembly is a `Component` whose `Definition::Assembly` holds a nested `Assembly` with its own ground. World transform composition is `parent.effective_transform().then(&child.effective_transform())` and happens in exactly one function, `ASM-024`. Flattening yields leaf occurrences (those whose component is a `Part`) with the path of occurrence ids above them, so the BOM (`ASM-125`) and the interference pass (`ASM-095`) each walk leaves once.
- **Refusals:** An `Assembly` nesting deeper than `MAX_ASSEMBLY_DEPTH` → `Err(TooDeep { path })` naming the path of component ids.
- **Tests:** `a_nested_assembly_composes_world_transforms` — a two-level assembly, the leaf's world transform equals the product of the two; `flatten_is_sorted_and_bounded` — `flatten()` on a 4-deep assembly returns leaves in `OccId` order and terminates; `too_deep_is_refused_not_truncated`.
- **Acceptance:** Nesting is supported and depth-bounded; a test at `MAX_ASSEMBLY_DEPTH + 1` is `Err(TooDeep)` with the path in the message.
- **Risk:** Exponential blow-up in a wide×deep tree. Fallback: `flatten` memoises the world transform per occurrence and is asserted to be linear in the number of occurrences in a property test (`ASM-152`).

### ASM-022 — `MAX_ASSEMBLY_DEPTH` and the refusal that names the path
- **State:** READY
- **Depends on:** ASM-021
- **Files:** `crates/asm/src/assembly_file.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::depth_of(&self, occ: OccId) -> Result<usize, AsmError>;` and the `TooDeep` check in `flatten`.
- **Behaviour:** The limit is 16, the same number and the same intent as `cadcraft_doc::MAX_BLOCK_DEPTH` (`crates/doc/src/extents.rs:10`), and the reasoning is the same one that comment gives: a cyclic or hostile file must be cut short rather than followed forever. Exceeding it is an `Err` naming the path that would have gone deeper, so the person with the file can see which sub-assembly is the culprit.
- **Refusals:** `Err(TooDeep { path })` when `depth_of > MAX_ASSEMBLY_DEPTH`.
- **Tests:** `depth_counts_nesting_levels` — a three-deep tree reports 3; `a_depth_of_seventeen_is_refused_with_its_path`.
- **Acceptance:** The constant lives in `limits.rs` next to the others and is documented next to `MAX_BLOCK_DEPTH` with the cross-reference.
- **Risk:** A legitimate machine with a 20-deep BOM (common in furniture) hits the limit. Fallback: `MAX_ASSEMBLY_DEPTH` is a constant and raising it is a one-line change with a test; the record is that it was a decision.

### ASM-023 — Cycle detection in the component graph
- **State:** READY
- **Depends on:** ASM-021
- **Files:** `crates/asm/src/assembly_file.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::check_acyclic(&self) -> Result<(), AsmError>;` returning `Err(CyclicComponent { path: Vec<ComponentId> })`.
- **Behaviour:** A component that contains itself, directly or through any chain, is refused with the path of component ids that closes the loop. The check runs on **load** (before anything is built) and on **insert** (before the occurrence is added). It is a depth-first walk with an on-stack set, not a recursion-depth guess, so a 16-deep acyclic tree is fine and a 2-deep cycle is caught.
- **Refusals:** `Err(CyclicComponent { path })` naming the whole loop.
- **Tests:** `a_self_containing_component_is_refused`; `a_two_component_cycle_is_refused_with_both_ids`; `a_diamond_is_not_a_cycle` — A→B, A→C, B→D, C→D is accepted.
- **Acceptance:** A hand-written `.cca` with a cycle (a hostile-input fixture) is refused at load and never reaches the builder.
- **Risk:** A file where the same component appears twice as siblings is not a cycle and must load. Fallback: path set, not a global visited set — stated in the code and pinned by the diamond test.

### ASM-024 — World transform resolution through the hierarchy
- **State:** READY
- **Depends on:** ASM-021
- **Files:** `crates/asm/src/world.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub fn Assembly::world_transform(&self, occ: OccId) -> Result<Transform, AsmError>;
  pub fn Assembly::world_point(&self, occ: OccId, p: Vec3) -> Result<Vec3, AsmError>;
  ```
- **Behaviour:** Walks from the root down to `occ`, composing `then`. It returns the **effective** transform at each level (`ASM-012`), so an occurrence under a failed component reports the last good world pose. The walk is iterative with an explicit stack, depth-bounded by `MAX_ASSEMBLY_DEPTH`, and does **not** recurse into Rust's stack beyond that (a hostile file cannot blow the stack). Composition uses `Transform::then`, which is the same order as `Mat4::mul`, so the answer can be cross-checked against `to_mat4().apply(p)` in a test.
- **Refusals:** unknown occurrence → `NoSuchOccurrence`; deeper than the cap → `TooDeep`.
- **Tests:** `world_transform_matches_matrix_composition` — 200 pseudo-random 2-level assemblies, `world_point(occ, p) == world_transform(occ)?.to_mat4().apply(p)` to 1e-9; `a_failed_ancestor_gives_the_last_good_world_pose`; `hostile_depth_does_not_overflow_the_stack` — a synthetic 10^5-deep `Vec` is `Err(TooDeep)`, not a stack overflow.
- **Acceptance:** The composition order is pinned by a test against `Mat4`, which is the only way to know a right-handed transform chain is not inverted.
- **Risk:** Composition order inverted (the classic bug) would place parts correctly relative to each other only when all scales are 1. Fallback: the `Mat4` cross-check test above, which uses non-unit scales.

### ASM-025 — World bounds of one occurrence
- **State:** READY
- **Depends on:** ASM-024, ASM-008
- **Files:** `crates/asm/src/world.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::occurrence_bounds(&self, occ: OccId) -> Result<cadcraft_geom::Bounds3, AsmError>;`
- **Behaviour:** Transforms the eight corners of the component solid's `bounds()` and takes their `Bounds3` (`crates/geom/src/bounds.rs:179` `corners()`). For a rotated occurrence this is the *axis-aligned* box of the rotated box, which is conservative and correct; the doc comment says so rather than pretending it is tight. Returns `Bounds3::EMPTY` (not an `Err`) when the component has no solid yet — "no geometry" is an answer here, because a caller asking for bounds of an unbuilt component wants an empty box, not a failure.
- **Refusals:** none. Unknown occurrence → `NoSuchOccurrence`. Unbuilt component → `Ok(Bounds3::EMPTY)`.
- **Tests:** `an_axis_aligned_occurrence_bounds_exactly` — a box at the origin placed with an identity transform returns exactly its own bounds; `a_rotated_occurrence_is_conservative` — the returned box contains the transformed solid's eight corners; `an_empty_solid_gives_EMPTY_not_an_error`.
- **Acceptance:** `EMPTY` for no geometry, `Err` only for no such occurrence; both pinned.
- **Risk:** A tight (oriented) bounds would be better for interference broad phase. Fallback: `ASM-094` uses the AABB for the broad phase and can be improved to an OBB later without changing this signature.

### ASM-026 — World bounds of the whole assembly
- **State:** READY
- **Depends on:** ASM-025, ASM-019
- **Files:** `crates/asm/src/world.rs` (owner **ASM**)
- **Contract:** `pub fn Assembly::bounds(&self) -> cadcraft_geom::Bounds3;` — infallible, `Bounds3::EMPTY` when there is nothing visible.
- **Behaviour:** Unions `occurrence_bounds` over every non-suppressed occurrence whose component has a solid. Infallible because an assembly with no built geometry legitimately has empty bounds, and because `Bounds3::union` already handles `EMPTY` on both sides (`crates/geom/src/bounds.rs:120`). Suppressed occurrences contribute nothing, matching `ASM-019`.
- **Refusals:** none.
- **Tests:** `bounds_of_two_disjoint_occurrences_is_their_union`; `a_suppressed_occurrence_contributes_nothing`; `an_empty_assembly_is_EMPTY`.
- **Acceptance:** Infallible and total; a property test asserts it never panics on any generated assembly.
- **Risk:** A 50 000-occurrence bounds call is O(n). Fallback: cached per component and invalidated by `set_transform`; the cache is a later optimisation, not a correctness matter.

### ASM-027 — Component properties: material, density, appearance, part number
- **State:** READY
- **Depends on:** ASM-007
- **Files:** `crates/asm/src/material.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct Material { pub name: String, pub density_kg_m3: f64, pub appearance: String }
  pub fn Assembly::component_mass_kg(&self, occ: OccId) -> Result<f64, AsmError>;
  ```
- **Behaviour:** **New type.** `cadcraft_doc` has no material (a `Layer` carries colour, not density), so a new struct is needed; the reason is stated on it. `density_kg_m3` is stored in **kg/m³**, a unit-independent quantity, and the component's volume is in model units, so the mass conversion needs a model-units-to-metres factor. That factor is the component's `unit_scale` (from `ASM-007`) and **must** be present: `component_mass_kg` returns `Err(Refused("component N has no unit scale; mass cannot be reported"))` when it is absent, rather than guessing that a drawing is millimetres. Reuses `cadcraft_solid::Solid::mass_properties` for the volume.
- **Refusals:** density ≤ 0 or non-finite → `NonFinite` at construction; missing unit scale → `Refused` at mass; volume ≤ 0 (a solid that failed validation) → `Refused` naming the volume.
- **Tests:** `a_steel_block_of_known_volume_has_the_right_mass` — 0.001 m³ at 7850 kg/m³ is 7.85 kg within 1e-9; `mass_without_a_unit_scale_is_refused`; `a_failed_component_keeps_reporting_its_last_good_mass`.
- **Acceptance:** Mass is only ever reported with both a volume and a unit scale, and the missing-scale case is an `Err` naming the component.
- **Risk:** Assuming millimetres is the single most likely wrong answer here and it is refused rather than assumed.

### ASM-028 — Assembly mass properties and centre of mass
- **State:** READY
- **Depends on:** ASM-027, ASM-025
- **Files:** `crates/asm/src/world.rs` (owner **ASM**)
- **Contract:**
  ```rust
  pub struct AssemblyMass { pub total_kg: f64, pub centroid: Vec3, pub volume: f64, pub parts: usize }
  pub fn Assembly::mass_properties(&self) -> Result<AssemblyMass, AsmError>;
  ```
- **Behaviour:** Sums `volume × density × unit_scale³` per leaf occurrence, and accumulates the centroid with the same weighting. One occurrence's transform applied to `cadcraft_solid::MassProperties::centroid` gives the leaf's world centroid. The aggregation is done in **sorted occurrence order** so the floating-point sum is reproducible; a `HashMap`-ordered sum would not be, and the difference would be in the last bits. Documented on the method.
- **Refusals:** any leaf without a unit scale or with a non-positive volume → `Refused` naming the occurrence; an assembly with no leaves → `Err(Refused("the assembly has no parts"))`.
- **Tests:** `two_equal_parts_give_a_centroid_between_them` — two 1 kg blocks 2 m apart have a centroid at the midpoint; `the_sum_is_order_independent` — computing over a shuffled `Vec<OccId>` gives the same bits; `a_missing_unit_scale_names_the_occurrence`.
- **Acceptance:** Reproducible bit-for-bit over occurrence ordering; pinned by the order-independence test.
- **Risk:** Summing thousands of volumes in f64 accumulates error. Fallback: Kahan summation, used unconditionally and noted in the doc comment.

### ASM-PLT-PLACEHOLDER — (reserved; see `ASM-PLT`)
<!-- ASM-JNT-SECTION -->

---
---

## ASM-JNT — joint types

<!-- ASM-JNT-SECTION -->

<!-- ASM-SLV-SECTION -->

<!-- ASM-INF-SECTION -->

<!-- ASM-MOT-SECTION -->

<!-- ASM-BOM-SECTION -->

<!-- ASM-PLT-SECTION -->
