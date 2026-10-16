# CAM — manufacturing: tools, toolpaths, simulation, post-processors, machines, additive

Task index for the `CAM` domain (8 % of the programme). Owned crate: **`crates/cam/**` (new)**.

**Owner letters** are the wave-1 agents of `docs/master-plan.md` §6 — **A** kernel, **Q** quality,
**D** interchange, **U** ui/viewport, **C** orchestrator — plus the two domain owners this file needs:
**CAM** (`crates/cam/**`) and **PLT** (`docs/**`, `xtask/**`).

**Status:** specification only. Nothing in this file has been executed. `crates/cam` does not exist.

---

## 0. Invariants of this domain

1. **A post-processor may not exist without a machine-simulation validator.** G-code that a
   controller accepts can still crash a machine: it can exceed a soft limit, command a rotary axis
   through a forbidden range, feed into a clamp, or cut into the part. **Every task in `CAM-POST`
   therefore names a validator task in its *Depends on* list**, and the acceptance criterion of every
   dialect task is that it passes *both* gates: the internal round-trip
   (`CAM-POST-007`, read our own output back and re-check every move) and an independent simulator
   (`CAM-POST-009`, probe file). A dialect task with one gate and not the other is `BLOCKED`, not done.
2. **Validation is a value, not a warning.** `CAM-SIM-001` defines
   `Validation::{Passed, Failed, Unvalidated}`; `Unvalidated` **cannot** be posted
   (`CAM-SIM-021`). This mirrors `SIM-VAL-015` in `docs/plan/SIM.md` and follows the same rule: an
   unvalidated artefact is not a deliverable, it is a guess.
3. **Everything is in millimetres, millimetres per minute and degrees.** CAM works in the kernel's
   own unit (`step.rs:90-94`; DAT-039/DAT-042 own the document unit). A machine whose own units are
   inches posts `G20` and the conversion happens **once**, in the post (`CAM-POST-008`), never in
   the toolpath generator.
4. **Refuse, never approximate, where an approximation can hurt a person.** A gouge, a collision, an
   over-travel, a strategy the geometry does not admit, a machine we have no model for: each is a
   named `CamError`. The refusal names the move, the axis or the region and the number involved.
5. **No vendor data.** Tool and machine libraries are CADCraft's own definitions
   (`AGENTS.md`: no Autodesk/other vendor assets; the clean-room rule). A machine model is entered
   by the user or built from a generic template; it is never shipped as a vendor catalogue.
6. **Nothing here panics on hostile input.** Every public entry point returns
   `Result<_, CamError>`; every number that came from a file, a control channel or a UI is checked
   for finiteness and range before it reaches geometry; every loop is bounded by a named cap.

### What `crates/solid/src/mesh.rs` gives this domain — and what it does not

Read before planning any of `CAM-SIM`. This is the evidence base for the whole epic.

**What it gives, and it is good:**

| Reusable | Where | Why it is the right reuse |
|---|---|---|
| `mesh_from_solid(&Solid) -> Result<Mesh, SolidError>` | `mesh.rs:54` | The starting point of collision checking. A triangle mesh *is* what a swept-tool check consumes. |
| `Mesh { verts: Vec<Vec3>, tris: Vec<[usize;3]> }`, `tris` wound CCW seen from outside | `mesh.rs:34-38` | The winding invariant (kernel I4) means a removed-volume computation by signed tetrahedra has a **sign**: `the_mesh_volume_matches_the_exact_volume_of_the_solid` pins it to `1e-9` on a box. CAM-SIM depends on that sign being right. |
| Exact coverage of every planar face; holes triangulated separately with **reversed** winding | `mesh.rs:86-108` | Signed triangle area is exactly `outer − holes`. A mesh that lost a hole would show up immediately as a volume mismatch — which is why the area test exists. |
| Winding independence: `clip_loop_ccw` measures signed area and flips | `mesh.rs:115-124` | CAM-SIM may re-mesh at any time and get the same orientation. |
| `Triangle::normal/area/center` | `mesh.rs:21-29` | Gouge classification needs a normal and an area; both are already scale-safe. |
| Hostile input refused: `SolidError::Empty`, `EmptyProfile`, `TooManyTriangles`, non-finite points skipped | `mesh.rs:64-72,137` | The crash contract is inherited, not reinvented. |
| `stl_ascii` / `stl_binary` | `mesh.rs:278,297` | The mesh→STL path for "send this to a CAM package" and for fixtures. |

**What it does not give, and this is the whole problem:**

- **No face provenance.** `mesh_from_solid` iterates `for face in &solid.faces` and throws the
  index away (`mesh.rs:56-68`). **A gouge check that cannot say *which face* was cut is useless to
  a machinist.** This is `KER-MSH-001` (`TrackedMesh { tri_face, face_tris }`) and it blocks
  `CAM-SIM-004`.
- **The vertices are not welded.** Three fresh vertices per triangle (`mesh.rs:60-62`): the mesh has
  no adjacency, no edges and no boundary sets. Useful for a volume sum; useless for "is this
  boundary of the stock". `CAM-SIM-003` welds with `SIM-MSH-011`'s discipline.
- **No element types, no quality control, no refinement.** `drop_collinear` (`mesh.rs:168`)
  *deletes* vertices; ear clipping emits whatever quality the polygon admits. CAM does not need
  element quality, but it **does** need the tessellation density to be a chosen number, and
  `mesh.rs` has no density parameter: `MAX_TRIANGLES = 2_000_000` (`lib.rs:109`) is a safety cap,
  not a resolution. Faceted curves (a fillet is ≤ `MAX_ROUND_SEGMENTS = 64` facets) therefore bound
  every CAM accuracy claim: **a "true 3D" contour on a fillet is machined against a 64-gon.**
- **`Mesh::triangles()` is a `filter_map`.** An out-of-range index is **dropped silently**
  (`mesh.rs:42-45`). A mesh built by CAM-SIM with one bad index has a hole and reports no error, so
  every mesh CAM-SIM constructs is re-checked before it is trusted (`CAM-SIM-017`).
- **`stl_binary` casts `f64 → f32` unchecked** (`mesh.rs:306`). A coordinate beyond `f32` range
  becomes `±inf` and a NaN stays NaN, in a file a downstream tool may read. CAM never treats an
  exported STL as valid without re-reading it.

### The reusable numbers (all of them, once)

`crates/solid/src/lib.rs`: `MAX_FACES 60_000`, `MAX_LOOP_VERTS 60_000`, `MAX_WELD_VERTICES 20_000`,
`MAX_TRIANGLES 2_000_000`, `MAX_ROUND_SEGMENTS 64`, `MAX_BLENDS 2_000`,
`tolerance_for(d) = clamp(d·1e-9, 1e-12, 1e-3)`.
`crates/solid/src/mesh.rs:126`: `tolerance_of(region) = clamp(diagonal·1e-11, 1e-13, 1e-3)`.
Note the two tolerances differ by two orders of magnitude at the floor; CAM uses
`tolerance_for` and says so.

---

## 1. Blocked on `KER` — the exact contracts this plan needs

These are interfaces `crates/cam` consumes and cannot build. They are **not** requests to edit
`crates/solid`; they are contracts for the `KER` owner (agent **A**) to accept, reject or replace.
`docs/plan/SIM.md` §2 already proposes `KER-MSH-001..008` for the same reasons and **this file
adopts those IDs rather than inventing duplicates**. The two new ones below are CAM-specific.

| Req ID | Exact signature | Why CAM needs it | Evidence |
|---|---|---|---|
| `KER-MSH-001` | `pub fn mesh_from_solid_tracked(solid: &Solid) -> Result<TrackedMesh, SolidError>` with `pub struct TrackedMesh { pub mesh: Mesh, pub tri_face: Vec<usize>, pub face_tris: Vec<Vec<u32>> }` | **The gouge validator.** CAM-SIM must report *which face* a move removed material from. Without it every gouge is a point in space and the machinist cannot look at it. | `mesh.rs:56-68` — the face index is discarded. |
| `KER-MSH-003` | `pub struct FaceId(pub u64); pub struct EdgeId(pub u64);` `pub fn face_id(solid: &Solid, at: Vec3, tol: f64) -> Result<FaceId, SolidError>` `pub fn edge_id(solid: &Solid, r: &EdgeRef) -> Result<EdgeId, SolidError>` `pub fn faces_of_edge(solid: &Solid, e: EdgeId) -> Result<[FaceId; 2], SolidError>` `pub fn faces_of(solid: &Solid, ids: &[FaceId]) -> Result<Vec<Face>, SolidError>` | CAM operations are stored per operation and must **re-target after a rebuild**. Today an operation is a face *index* or an `EdgeRef` point, and both are destroyed by any topology-changing edit. Without this, every saved CAM setup is a guess after the user changes one dimension. | `master-plan.md` §4 "Topological naming: **None**"; `chamfer.rs:33-44`. |
| `KER-MSH-004` | `pub enum Containment { Inside, Outside, OnSurface { signed_distance: f64 } }` `pub fn locate(solid: &Solid, p: Vec3, tol: f64) -> Result<Containment, SolidError>` `pub fn signed_distance(solid: &Solid, p: Vec3) -> Result<f64, SolidError>` | The gouge depth (material removed below the surface) and the "is the floor of this pocket already at the tool" test. `Solid::contains` is a hard ray cast with a `1e-12` skip and no band. | `solid.rs:676`. |
| `KER-MSH-005` | Re-export `section`: `pub use section::{Section, section};` | **CAM's Z levels.** Pocket detection, facing, drilling from a face and the slicer (`CAM-ADD-003`) all need "what is the material's cross-section at height z". This is a **one-line change** and the cheapest unblock in the programme. | `lib.rs:93` exports only the `Section` struct. |
| `KER-MSH-006` | `pub fn closest_point(solid: &Solid, p: Vec3, tol: f64) -> Result<(Vec3, f64), SolidError>` | Generation-time gouge check (`CAM-PATH-022`) and the "is the tool already in the material" test before a rapid. There is no distance-to-shell query in the repository. | absence; `Plane::distance` per face only. |
| `KER-MSH-007` | `pub fn raycast(solid: &Solid, origin: Vec3, dir: Vec3, dir_tol: f64) -> Result<Vec<f64>, SolidError>` | Lead-in entry points onto a face, probe start positions, and the projection of a pick onto the part. `contains` casts exactly one fixed ray. | absence. |
| `KER-TOL-002` | `pub fn interference(a: &Solid, b: &Solid, tol: f64) -> Result<Interference, SolidError>` with `pub struct Interference { pub touching: bool, pub volume: f64, pub pairs: Vec<(usize, usize)> }` | Fixture interference (`CAM-MACH-003`) and the clamp/holder check. | `master-plan.md` §5. |
| **`KER-CAM-001`** | `pub fn offset_regions(r: &Regions, distance: f64, tol: f64) -> Result<Regions, SolidError>` with `pub enum OffsetJoin { Miter { limit: f64 }, Round { segments: usize } }` and `pub fn offset_regions_with(r: &Regions, distance: f64, join: OffsetJoin, tol: f64) -> Result<Regions, SolidError>` | **Tool-radius compensation.** Every finishing contour in `CAM-PATH` is the boundary of the model offset outward (or inward for a pocket) by the tool radius. `crates/geom` has `union/intersect/difference/xor` and **no offset at all** (`geom/src/lib.rs:22`). This is the single hardest blocker in CAM. Polygonal only: loops are straight-edged, so `Round` inserts arcs as segments. | `geom/src/bool2d.rs:882-985` — no offset; `geom/src/lib.rs:22` export list. |
| **`KER-CAM-002`** | `pub fn offset_solid(solid: &Solid, distance: f64, tol: f64) -> Result<Solid, SolidError>` | Rest-material computation for roughing and for the validator's "what is left" answer. `KER-FEAT-005` (thicken) owns it; CAM names it as a consumer. | `master-plan.md` §5 `KER-FEAT-005`. |
| `KER-BOOL-001..003` | the three open kernel defects (`revolve` shell, union weld, union commutativity) | CAM-SIM accumulates `stock − Σ swept`. A boolean that leaves a crack (defect 002) makes the validator's volume **wrong in the direction that hides a gouge**. No CAM validator may ship before all three are `VALIDATED`. | `crates/solid/tests/property.rs`, seeds named. |
| `KER-SRF-003` | analytic surfaces (master-plan §5) | Feature recognition of a real bore (`CAM-PATH-024`) and a ball-nose tool on a real sphere. Until it lands every round is ≤ 64 facets. | `lib.rs` "Planar faces only". |

**Rule:** a `CAM` task consuming one of these is `BLOCKED` until the `KER` contract is `VALIDATED`.
It is not `BACKLOG`, and it is not "assume and proceed".

---

## 2. Epics

| Epic | Name | Tasks | Gate |
|---|---|---|---|
| `CAM-TOOL` | Tool, stock and machine model | 22 | `KER-CAM-001` for anything that offsets |
| `CAM-MACH` | Machine model, digital twin, NC program lifecycle | 18 | `CAM-TOOL-013` |
| `CAM-PATH` | Toolpath generation | 30 | `KER-CAM-001`, `KER-MSH-001/005/006` |
| `CAM-SIM` | Simulation and **the validator** | 22 | **First among the generators' consumers; nothing may be posted without it** |
| `CAM-POST` | Post-processors | 24 | `CAM-SIM-001`, and per-dialect `CAM-POST-007` + `CAM-POST-009` |
| `CAM-ADD` | Additive (slice) toolpaths | 14 | `CAM-SIM-018` for its post |
| **total** | | **130** | |

**Order in the file is `TOOL → MACH → PATH → SIM → POST → ADD`**, which is the dependency order, not
the layer order. `CAM-SIM` appears before `CAM-POST` deliberately: the post is the last thing built
and the least safe thing to build.

---

# CAM-TOOL — tools, stock, machines (22 tasks)

### CAM-TOOL-001 — The crate, its layer and its dependency rule
- **State:** READY
- **Depends on:** —
- **Files:** `crates/cam/Cargo.toml`, `crates/cam/src/lib.rs` (new, owner CAM), `xtask/src/layers.rs::TABLE` (owner PLT — one row `("cam", Class::Layer(3))`, co-sequenced)
- **Contract:** `("cam", Class::Layer(3))` in `TABLE`; `crates/cam` depends on `cadcraft-geom`, `cadcraft-solid`, `thiserror`, `serde`, `serde_json` and nothing else in the workspace.
- **Behaviour:** `cam` sits at **L3**: it may use `geom`/`solid` (L0) and may be used by `engine` (L4), but must not depend on `render`, `model` or `ui-egui`. `layers.rs` reports `Unregistered` as a violation, so the row is not optional. The rationale is the same as `sim`: a toolpath generator that can read a document can be driven by a document, and the validator must be callable from a test with no session.
- **Refusals:** `cargo xtask layers` reporting an `Unregistered` or `Upward` violation fails the build — not a warning.
- **Tests:** `cam_is_registered_at_layer_3` — a test in `xtask/src/layers.rs` that `classify("cadcraft-cam") == Some(Class::Layer(3))`. `cam_has_no_upward_dependency` — asserts `cam`'s workspace deps are exactly the four above. `every_cam_error_variant_is_used`.
- **Acceptance:** `cargo xtask layers` green; `cargo xtask ci` green with the empty crate.
- **Risk:** someone adds `cadcraft-model` for convenience and the validator silently depends on a document. Fallback: the layer test pins the dep list.

### CAM-TOOL-002 — `CamError`: name every refusal before there is anything to refuse
- **State:** READY
- **Depends on:** CAM-TOOL-001
- **Files:** `crates/cam/src/error.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum CamError {
      Invalid(String),                                   // a value that cannot be a tool/move/machine
      Unsupported(String),                               // "5-axis is not implemented"
      Limit { what: &'static str, count: usize, max: usize },
      Gouge { op: String, move_index: usize, depth: f64, at: [f64; 3] },
      Collision { op: String, move_index: usize, bodies: [&'static str; 2] },
      MachineRefusal { axis: &'static str, value: f64, limit: (f64, f64) },
      Unvalidated { reason: String },                    // posting something the validator has not cleared
      PostRefusal(String),                               // the post cannot express this motion
      Kernel(cadcraft_solid::SolidError),
  }
  ```
- **Behaviour:** Every one of the task refusals in this file maps to exactly one variant, and every variant renders with the number involved: `Gouge` says the operation, the move index, the depth and the point; `MachineRefusal` says the axis and both numbers. `Unvalidated` is the gate value from invariant 2 and is produced by exactly one function (`CAM-SIM-021`).
- **Refusals:** a `Display` impl that cannot render its own variant is a compile error, not a runtime `unreachable!` — the crate denies `clippy::unreachable` (`AGENTS.md`).
- **Tests:** `every_variant_renders_its_numbers` — a table test over one value per variant asserting the rendered string contains the operation name, the axis, the count and the max. `gouge_names_the_move_and_the_depth`. `unvalidated_names_its_reason`.
- **Acceptance:** the enum is exhaustive over this file's refusals; a grep for `CamError::` in `crates/cam` finds no other error type in a public signature.
- **Risk:** an error type that grows a variant per bug. Fallback: `Invalid(String)` carries the message and the structured variants are only for the four cases a machine or a machinist must read as numbers.

### CAM-TOOL-003 — Units, and the one place they are converted
- **State:** READY
- **Depends on:** CAM-TOOL-002, DAT-039, DAT-042
- **Files:** `crates/cam/src/units.rs` (owner CAM)
- **Contract:**
  ```rust
  pub struct CamUnits { pub length: f64, pub feed: f64, pub angle: f64, pub spindle: f64 }
  impl CamUnits {
      pub fn mm() -> CamUnits;                       // 1.0, 1.0, deg→rad, rev→mm
      pub fn inch() -> CamUnits;                     // 25.4, 25.4, deg→rad, rev→in
      pub fn length_mm(&self, v: f64) -> f64;
      pub fn feed_mm_min(&self, v: f64) -> f64;
      pub fn degrees(&self, rad: f64) -> f64;
  }
  ```
- **Behaviour:** Inside `crates/cam` every number is millimetres, mm/min, degrees and revolutions per minute. `inch()` exists only so `CAM-POST-008` can convert **once, at the machine boundary**. It is never used by a generator: `grep` the crate for `inch()` and the only caller must be the post.
- **Refusals:** a non-finite conversion is `CamError::Invalid("a non-finite value cannot be converted")`. A `CamUnits` built with a non-finite factor is refused at construction.
- **Tests:** `inch_length_is_exactly_25_4` — `inch().length_mm(1.0) == 25.4` exactly. `generators_never_call_inch` — a source scan asserting `inch()` appears only in `post/`. `a_non_finite_conversion_is_refused`.
- **Acceptance:** every geometry function in `crates/cam` takes millimetres and its doc says so.
- **Risk:** an inch machine silently posting millimetre values — a crash, not a wrong number. Fallback: `CAM-POST-006` refuses a `G20`/`G21` disagreement on read, and `CAM-POST-008` prints the unit in the header.

### CAM-TOOL-004 — `ToolKind`, and which strategies each tool can be used for
- **State:** READY
- **Depends on:** CAM-TOOL-002
- **Files:** `crates/cam/src/tool/kind.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum ToolKind { FlatEndMill, BallNose, BullNose, ChamferMill, FaceMill, Drill, Tap, Reamer, BoringBar, Probe }
  pub struct ToolCaps { pub contour: bool, pub pocket: bool, pub face: bool, pub drill: bool, pub radial: bool }
  pub fn caps_of(kind: ToolKind) -> ToolCaps;
  ```
- **Behaviour:** One table, one source of truth for "can this tool do that". `BullNose` carries its corner radius in `Tool`, not in the kind. The strategy planner refuses an incompatible pair rather than approximating (`CAM-PATH-004`).
- **Refusals:** a strategy naming a tool whose `caps_of` lacks the capability is `CamError::Unsupported("a tap cannot finish a contour")` naming both.
- **Tests:** `every_kind_has_a_caps_table_row` — a test asserting all ten kinds are covered. `a_tap_cannot_pocket`. `a_probe_caps_everything`. `an_unknown_kind_is_not_invented` — the enum has no `_` variant; a document naming an unknown kind is refused at load.
- **Acceptance:** the table is asserted row by row and is the only place capabilities are decided.
- **Risk:** the table drifts from the generators. Fallback: `CAM-TOOL-020` asserts every strategy/tool pair in the registry against it.

### CAM-TOOL-005 — `Tool` geometry and its revolved envelope
- **State:** READY
- **Depends on:** CAM-TOOL-004
- **Files:** `crates/cam/src/tool/tool.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Tool {
      pub name: String, pub kind: ToolKind,
      pub diameter: f64, pub corner_radius: f64, pub flute_length: f64, pub overall_length: f64,
      pub shank_diameter: f64, pub flutes: u8, pub taper_deg: f64,
  }
  impl Tool {
      pub fn radius(&self) -> f64;                       // diameter / 2
      pub fn profile(&self) -> Result<Vec<[f64; 2]>, CamError>;  // (radius, height) half-profile, z=0 at the tip
      pub fn swept_prism(&self, a: Vec3, b: Vec3) -> Result<Regions, CamError>; // planar moves only
      pub fn validate(&self) -> Result<(), CamError>;
  }
  ```
- **Behaviour:** `profile()` is the tool's revolved outline sampled at a stated chord tolerance; `swept_prism` is the Minkowski sum of the move's segment with the tool's profile, which for a **planar** move is exact and is the primitive `CAM-SIM-002` materialises. This is the one place CAM decides what a cutter is.
- **Refusals:** `diameter <= 0`, `flute_length <= 0`, `overall_length < flute_length`, `corner_radius < 0` or `> radius`, `flutes == 0`, `taper_deg` outside `0..=30` are each `CamError::Invalid` naming the field and both numbers. A profile whose chord error exceeds `tol` is `CamError::Invalid`, not a coarser profile.
- **Tests:** `a_flat_end_mill_profile_is_a_cylinder` — asserts every profile radius equals `radius()` to `1e-12`. `the_swept_prism_contains_both_ends` — the region's area ≥ π·r²·length for a move of that length. `a_zero_diameter_is_refused`. `a_corner_radius_above_the_radius_is_refused`. `a_negative_flute_length_is_refused`. `every_refusal_names_its_field`.
- **Acceptance:** the profile is used by `CAM-SIM-002` and by `CAM-PATH-018`'s ramp-angle rule, and both consume this one function.
- **Risk:** a bull-nose corner sampled too coarsely changes the swept volume and hence the gouge answer. Fallback: the chord tolerance is a parameter of `profile()` and the default is recorded in every validator report.

### CAM-TOOL-006 — `ToolHolder`: the part of the tool that must never touch the part
- **State:** READY
- **Depends on:** CAM-TOOL-005
- **Files:** `crates/cam/src/tool/holder.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct ToolHolder { pub name: String, pub diameter: f64, pub length: f64, pub flange: Option<(f64, f64)> }
  impl ToolHolder {
      pub fn envelope(&self, tool: &Tool) -> Result<Regions, CamError>;  // the solid above the flute
      pub fn lowest_safe_z(&self, tool: &Tool) -> f64;                   // z at which the holder cannot collide
  }
  ```
- **Behaviour:** The holder is modelled as a cylinder (plus an optional flange) above the flute. `CAM-SIM-007` intersects its swept volume with the stock. `lowest_safe_z` is what every rapid in `CAM-PATH-018` must clear.
- **Refusals:** `lowest_safe_z` below the stock top for a given setup is `CamError::Collision` naming the tool and the height; a holder shorter than its flute is `CamError::Invalid`.
- **Tests:** `the_holder_starts_where_the_flute_ends`. `a_flange_raises_the_safe_height`. `a_holder_inside_the_flute_is_refused`. `the_safe_height_is_above_the_stock_top_for_a_normal_setup`.
- **Acceptance:** `CAM-SIM-007` consumes `envelope()` and no other holder geometry exists.
- **Risk:** a real holder has a taper and a coolant nozzle we do not model. Fallback: the holder is a *conservative* envelope (the smallest cylinder that contains the real one is not what we draw — we draw a cylinder **larger** than the tool shank by a stated margin) and the margin is a parameter named in the report.

### CAM-TOOL-007 — `ToolLibrary`, in the document, with our own tools only
- **State:** BACKLOG
- **Depends on:** CAM-TOOL-005, CAM-TOOL-019
- **Files:** `crates/cam/src/tool/library.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  pub struct ToolLibrary { #[serde(default)] pub tools: Vec<Tool> }
  impl ToolLibrary {
      pub fn builtin() -> ToolLibrary;                 // CADCraft's own 12 definitions, no vendor data
      pub fn get(&self, name: &str) -> Option<&Tool>;
      pub fn require(&self, name: &str) -> Result<&Tool, CamError>;   // names the library on miss
      pub fn validate(&self) -> Result<(), CamError>;
  }
  ```
- **Behaviour:** A library is document data, not a file on disk, and not a vendor catalogue: `builtin()` holds twelve tools (Ø3/6/10/12/16/20 flat end mills, Ø6/12 ball nose, Ø20 bull nose, Ø63 face mill, Ø6/10 drills, a probe) with the numbers written in the source. A user adds their own.
- **Refusals:** a duplicate tool name is `CamError::Invalid` naming both entries. `validate()` refuses every field `Tool::validate` refuses, with the tool name in the message. An unknown name from a document is `CamError::Invalid("no tool named {name}; the library has {n}")`.
- **Tests:** `the_builtin_library_validates`. `no_builtin_tool_carries_a_vendor_name` — a test asserting every builtin name starts with `cc-` or is a generic shape word, which is the clean-room check. `a_duplicate_name_is_refused`. `an_unknown_name_lists_the_library`.
- **Acceptance:** the builtin list is checked into the source, and `AGENTS.md`'s asset rule is satisfied by there being no asset at all.
- **Risk:** a user pastes a vendor catalogue in and we store vendor data. Fallback: the library is a list of *numbers*; names are free text and never used to select behaviour.

### CAM-TOOL-008 — Cutting speed, feed per tooth, feed per revolution
- **State:** READY
- **Depends on:** CAM-TOOL-005, CAM-TOOL-009
- **Files:** `crates/cam/src/tool/speeds.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct SpeedResult { pub rpm: f64, pub feed_mm_min: f64, pub doc_mm: f64, pub rpm_clamped: bool, pub feed_clamped: bool }
  pub fn surface_speed(tool: &Tool, material: &MatData, vc: f64) -> f64;              // rpm = 1000·vc/(π·D)
  pub fn speeds(tool: &Tool, material: &MatData, p: &SpeedParams, machine: &MachineLimits) -> Result<SpeedResult, CamError>;
  ```
- **Behaviour:** Two named formulas, written down: `n = 1000·v_c/(π·D)` for cutting speed, and `v_f = n·z·f_z` for tooth feed (or `v_f = n·f_rev` for a drill). Every clamp is recorded in `SpeedResult` and printed by `CAM-MACH-010`'s setup sheet, because "the machine capped my feed" is information a machinist needs.
- **Refusals:** `vc <= 0`, `fz <= 0`, `n < machine.min_rpm` (the machine cannot hold the speed → `CamError::MachineRefusal { axis: "spindle", .. }`), a diameter of 0, a non-finite material value.
- **Tests:** `rpm_matches_the_formula` — a Ø20 tool at `vc = 200` gives `1000·200/(π·20)` to `1e-9`. `tooth_feed_times_flutes` — Ø20, 2 flutes, `fz = 0.05`, 2000 rpm gives 200 mm/min. `a_machine_max_rpm_clamps_and_says_so`. `a_speed_below_the_machine_minimum_is_a_refusal`. `a_non_finite_material_is_refused`.
- **Acceptance:** the formulas appear once, here, and every other speeds computation calls them.
- **Risk:** a formula silently changed by a "better" heuristic. Fallback: the result carries `doc_mm` (the computed document value) and `rpm_clamped`, and the test asserts the number, not the shape.

### CAM-TOOL-009 — `MatData`: machining properties, our own table
- **State:** READY
- **Depends on:** CAM-TOOL-002
- **Files:** `crates/cam/src/tool/material.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct MatData { pub name: &'static str, pub vc: f64, pub kc: f64, pub feed_factor: f64, pub density: f64 }
  pub fn material(name: &str) -> Option<MatData>;         // aluminium, mild steel, stainless, brass, titanium, cast iron, plastics
  ```
- **Behaviour:** Seven own-definition rows with the numbers in the source, a citation comment naming them as handbook values, and a `feed_factor` multiplier applied by `CAM-TOOL-008`. Density is here for `CAM-MACH-010`'s mass estimate and nothing else.
- **Refusals:** an unknown material is `CamError::Unsupported("no cutting data for {name}; the library has {list}")` — never a default material, because a default feeds wrong numbers into a machine.
- **Tests:** `every_row_is_positive_and_finite`. `an_unknown_material_is_refused_not_defaulted`. `the_table_has_the_seven_documented_rows`. `feed_factor_scales_the_feed`.
- **Acceptance:** the table is in the source with the provenance comment, and no value comes from a file at runtime.
- **Risk:** handbook values that do not match a shop's practice. Fallback: `SpeedParams` overrides every field, and the report says which value was used.

### CAM-TOOL-010 — `Setup`: stock, work offset, fixture
- **State:** BACKLOG
- **Depends on:** CAM-TOOL-011, CAM-TOOL-012, CAM-MACH-004
- **Files:** `crates/cam/src/setup.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Setup {
      pub name: String,
      pub stock: Stock,
      pub origin: [f64; 3],
      pub work_offset: u8,          // 1..=6 (G54..G59), 0 = program coordinates
      pub fixture: Option<String>,  // a CAM-MACH-003 fixture name
      pub allowance: f64,           // stock left for finishing, ≥ 0
  }
  pub enum Stock { Box { min: [f64;3], max: [f64;3] }, FromModel { margin: f64 }, Cylinder { base: [f64;3], r: f64, h: f64 } }
  ```
- **Behaviour:** A setup is the unit an operation belongs to: stock, origin, offset, fixture, allowance. Operations from two setups never share a toolpath: the machine re-zeroes between them, which is exactly what `CAM-MACH-013`'s run order must express.
- **Refusals:** `work_offset > 6` → `CamError::Invalid`; a stock box with `max ≤ min` on any axis → `CamError::Invalid` naming the axis and both values; a negative `allowance` → `CamError::Invalid`; `FromModel { margin }` with a negative margin → refused (a stock smaller than the part cannot be machined).
- **Tests:** `a_setup_round_trips`. `an_inverted_stock_box_is_refused`. `a_work_offset_above_six_is_refused`. `a_negative_allowance_is_refused`. `a_stock_smaller_than_the_part_is_refused` — checked against the model's bounds at plan time.
- **Acceptance:** every operation names a setup and every setup names a machine.
- **Risk:** two setups sharing an offset silently collide. Fallback: `CAM-MACH-013` refuses a job list where two setups claim the same offset without an explicit flag.

### CAM-TOOL-011 — Stock from the model, and the allowance that is not free
- **State:** READY
- **Depends on:** CAM-TOOL-005
- **Files:** `crates/cam/src/setup.rs::stock` (owner CAM)
- **Contract:** `pub fn stock_from_model(model: &Solid, margin: f64, tol: f64) -> Result<Stock, CamError>;` and `pub fn stock_contains(stock: &Stock, solid: &Solid) -> Result<(), CamError>;`
- **Behaviour:** `Box` from `Solid::bounds()` expanded by `margin`, clamped so a stock below `tolerance_for(diagonal)` is refused. `stock_contains` is the check that must pass before **any** operation is planned: machining a part that is not in its stock is the definition of a crash.
- **Refusals:** `margin` such that a stock side is `< 2·tolerance_for(diagonal)` → `CamError::Invalid("the stock side {v} is below two kernel tolerances {t}")`. A model whose bounds are not finite → `CamError::Invalid`. A stock that does not contain the model → `CamError::Invalid` naming the axis where it fails.
- **Tests:** `the_stock_contains_the_model` — over 20 random kernel solids. `a_zero_margin_stock_is_accepted`. `a_negative_margin_is_refused`. `a_stock_that_excludes_the_part_is_refused_with_the_axis`. `a_huge_model_gives_a_huge_stock_not_an_overflow`.
- **Acceptance:** `CAM-PATH-005`'s `PlanContext` refuses to build without a containing stock.
- **Risk:** a part with a thin feature outside its bounds? Not possible — `Solid::bounds()` is exact over the faces. Fallback: none needed.

### CAM-TOOL-012 — Work offset and the Z-zero convention
- **State:** READY
- **Depends on:** CAM-TOOL-010
- **Files:** `crates/cam/src/setup.rs::offsets` (owner CAM)
- **Contract:** `pub fn machine_coords(setup: &Setup, machine_origin: [f64; 3], p: [f64; 3]) -> Result<[f64; 3], CamError>;` and `pub fn validate_zero(setup: &Setup, stock: &Stock) -> Result<(), CamError>;`
- **Behaviour:** Toolpaths are generated in **work coordinates** (the setup's origin at the stock top). The conversion to machine coordinates happens once, in the post, through the machine's zero offsets (`CAM-MACH-004`). `validate_zero` refuses a Z-zero **above** the stock top, because every operation then gouges on its first move.
- **Refusals:** a non-finite origin or offset → `CamError::Invalid`. A Z-zero above the stock top by more than `tolerance_for` → `CamError::Invalid("the work zero {z} is above the stock top {top}")`. An offset number outside `1..=6` → `CamError::Invalid`.
- **Tests:** `work_coords_add_the_origin`. `a_zero_above_the_stock_top_is_refused`. `a_zero_at_the_stock_top_is_accepted`. `a_non_finite_offset_is_refused`.
- **Acceptance:** the header comment of every posted program prints the work offset and the stock top (`CAM-POST-004`).
- **Risk:** a shop's Z-zero convention is the part's *top* or its *centre*; we assume top and say so in the setup sheet. Fallback: the origin is a field, so both conventions are expressible.

### CAM-TOOL-013 — `MachineModel`: axes, limits, rates, tool change
- **State:** READY
- **Depends on:** CAM-TOOL-003
- **Files:** `crates/cam/src/machine/model.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct MachineModel {
      pub name: String,
      pub axes: Vec<Axis>,
      pub rapid_mm_min: f64, pub max_feed_mm_min: f64,
      pub accel_mm_s2: f64,
      pub spindle: SpindleRange,           // { min_rpm, max_rpm, max_kw }
      pub tool_change: ToolChange,         // Manual | Atc | Magazine { stations: u8 }
      pub coolant: bool, pub units: CamUnits,
  }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Axis { pub name: String, pub kind: AxisKind, pub min: f64, pub max: f64, pub rotary: bool }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum AxisKind { Linear, Rotary }
  impl MachineModel {
      pub fn within_limits(&self, p: &[f64; 6]) -> Result<(), CamError>;   // MachineRefusal per axis
      pub fn clamp_feed(&self, f: f64) -> (f64, bool);
      pub fn travel_time_s(&self, a: &[f64; 6], b: &[f64; 6]) -> Result<f64, CamError>;
      pub fn validate(&self) -> Result<(), CamError>;
  }
  ```
- **Behaviour:** The machine model is the authority for **limits, rates and the spindle range**, and `within_limits` is what `CAM-PATH-002` and `CAM-SIM-009` both call. `travel_time_s` uses `rapid_mm_min` and the distance with zero acceleration — the honest simple number, with `accel_mm_s2` recorded for later and labelled as unused.
- **Refusals:** `min ≥ max` on an axis → `CamError::Invalid` naming the axis; fewer than 3 linear axes → `CamError::Unsupported("a machine needs at least three linear axes")`; `stations == 0` for a magazine → `CamError::Invalid`; a rotary axis with `min == max` → refused (no range, no rotation).
- **Tests:** `limits_are_enforced_per_axis`. `a_machine_with_no_rotary_axis_is_valid`. `a_magazine_with_zero_stations_is_refused`. `a_point_on_the_limit_is_accepted` and `1e-6` past it is refused. `travel_time_uses_the_rapid_rate`. `a_two_axis_machine_is_refused`.
- **Acceptance:** `within_limits` is the only place a limit is checked; no generator compares against a limit by hand.
- **Risk:** a soft limit is not a hard limit and a real controller will alarm at different numbers. Fallback: the model carries the numbers the user typed, and the setup sheet prints them next to the machine's own specification.

### CAM-TOOL-014 — A built-in machine library, generic by name
- **State:** READY
- **Depends on:** CAM-TOOL-013
- **Files:** `crates/cam/src/machine/library.rs` (owner CAM)
- **Contract:** `pub fn builtin_machine(name: &str) -> Option<MachineModel>;` with five own-definition 3-axis templates: `cc-router-300x200`, `cc-bridge-600x400`, `cc-bench-1000x600`, `cc-small-250x250`, `cc-vertical-800x500`.
- **Behaviour:** Generic machines with stated envelopes, rates and a manual tool change. **No vendor machine names, no vendor numbers** (`AGENTS.md`). A user machine is entered by copying a template and editing it.
- **Refusals:** an unknown name is `None`; `MachineModel::from_template` returns `CamError::Unsupported("no machine named {name}; the templates are {list}")`.
- **Tests:** `every_template_validates`. `no_template_carries_a_vendor_name` — the clean-room assertion. `an_unknown_template_lists_the_five`. `templates_have_distinct_envelopes`.
- **Acceptance:** the five templates are in the source with their numbers visible.
- **Risk:** a user assumes a template is a real machine. Fallback: every template's name starts with `cc-` and the setup sheet prints the numbers the template actually holds.

### CAM-TOOL-015 — Rotary axes: what we support and where it stops
- **State:** BACKLOG
- **Depends on:** CAM-TOOL-013, CAM-PATH-023
- **Files:** `crates/cam/src/machine/rotary.rs` (owner CAM)
- **Contract:**
  ```rust
  pub struct RotaryRule { pub wrap_degrees: f64, pub max_simultaneous_rad_s: f64 }
  pub fn check_rotary(m: &MachineModel, rule: &RotaryRule, a: &[f64;6], b: &[f64;6], dt_s: f64) -> Result<(), CamError>;
  ```
- **Behaviour:** Positioning only (3+2). The rule answers two questions: does a move exceed the rotary axis's travel, and does the rotary rate exceed `max_simultaneous_rad_s`. Full 5-axis simultaneous path generation is **refused** (`CAM-PATH-023`).
- **Refusals:** a rotary move beyond the axis range → `CamError::MachineRefusal { axis: "A", .. }`. A rotary rate above the rule → `CamError::MachineRefusal`. A machine with no rotary axis given a rotary move → `CamError::Unsupported`.
- **Tests:** `a_rotary_move_within_range_is_accepted`. `a_360_degree_wrap_is_refused_when_the_axis_spans_360`. `the_rotary_rate_rule_fires_at_the_boundary`. `a_machine_without_a_rotary_axis_refuses_the_move`.
- **Acceptance:** the 3+2 rule is in the machine model and in the post's modal state.
- **Risk:** a controller that wraps A at 360° while ours refuses. Fallback: `RotaryRule.wrap_degrees` is the user's number and the setup sheet prints it.

### CAM-TOOL-016 — Tool change model: manual, ATC, magazine
- **State:** READY
- **Depends on:** CAM-TOOL-013
- **Files:** `crates/cam/src/machine/toolchange.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn tool_change_move(m: &MachineModel, t: &Tool, at: &[f64;6]) -> Result<ToolChangePlan, CamError>;
  pub struct ToolChangePlan { pub safe_z: f64, pub to_pocket: Option<u8>, pub words: Vec<Move> }
  ```
- **Behaviour:** The change is a **planned sequence of moves**, not a magic M-code: raise to `safe_z` (the stock top plus a clearance from `CAM-TOOL-006`), optionally move to a magazine pocket, then the change itself is a `Move::Change` the post spells. The tool number is the index in the library, 1-based, and is stable for the whole program.
- **Refusals:** a tool change while below `safe_z` → `CamError::Collision` naming the tool and the Z. A magazine station index past `stations` → `CamError::Invalid`. A machine with `Manual` and more than one tool in a program → allowed, with a manual pause word the post must emit.
- **Tests:** `a_change_raises_to_the_safe_height_first`. `a_change_below_the_safe_height_is_refused`. `a_pocket_index_past_the_stations_is_refused`. `tool_numbers_are_one_based_and_stable`.
- **Acceptance:** the post never invents a change move; it spells the `Move::Change` the plan produced.
- **Risk:** the safe Z is a *motion* limit, and a low-profile job has no safe Z. Fallback: `ToolChangePlan` is data and the caller may supply its own sequence.

### CAM-TOOL-017 — Probing as a plan, not a protocol
- **State:** BACKLOG
- **Depends on:** CAM-TOOL-016, CAM-MACH-009
- **Files:** `crates/cam/src/machine/probe.rs` (owner CAM)
- **Contract:** `pub fn probe_plan(m: &MachineModel, targets: &[ProbeTarget]) -> Result<Vec<Move>, CamError>;` with `pub struct ProbeTarget { pub kind: WorkOffset | ToolLength, pub axis: usize, pub at: [f64; 3], pub expected: f64, pub tolerance: f64 }`
- **Behaviour:** The plan is the move sequence and the expected measurement with a tolerance. **The measurement itself is not read**: that needs a machine connection, which `CAM-MACH-014` refuses. The plan is exported so a user can run it and type the number back.
- **Refusals:** a probe target on an axis the machine cannot probe → `CamError::Unsupported`. A `tolerance <= 0` → `CamError::Invalid`. A target inside the stock (below the top) → `CamError::Collision`, because a probe stroke into the part is a broken probe.
- **Tests:** `the_plan_starts_above_the_stock`. `a_target_below_the_stock_top_is_refused`. `a_zero_tolerance_is_refused`. `the_plan_is_deterministic`.
- **Acceptance:** no network, no socket, no vendor protocol appears in this file.
- **Risk:** a user reads the plan as "it measured". Fallback: the type is named `probe_plan` and its doc says the measurement is not taken.

### CAM-TOOL-018 — Machine clamps applied at generation time, not at post time
- **State:** READY
- **Depends on:** CAM-TOOL-013, CAM-TOOL-008
- **Files:** `crates/cam/src/machine/clamp.rs` (owner CAM)
- **Contract:** `pub fn clamp_move(m: &MachineModel, mv: &Move, s: &SpeedResult) -> Result<Move, CamError>;` — returns the move with `feed` and `spindle` clamped and `clamped: bool` set.
- **Behaviour:** A feed above `max_feed_mm_min` or a spindle speed above `max_rpm` is reduced **at generation time** and the move is marked. If the reduction is below the machine's minimum feed the move is refused rather than posted.
- **Refusals:** a clamped feed below `min_feed_mm_min` → `CamError::MachineRefusal { axis: "feed", .. }` naming both. A spindle speed below `min_rpm` → `CamError::MachineRefusal { axis: "spindle", .. }`. An unclampable move (a rotary rate) → the same, naming the axis.
- **Tests:** `a_feed_above_the_maximum_is_clamped_and_marked`. `a_feed_that_would_clamp_below_the_minimum_is_refused`. `a_spindle_above_the_maximum_is_clamped`. `an_acceptable_move_is_unchanged`. `every_clamp_names_the_axis_and_both_numbers`.
- **Acceptance:** no generator posts a feed or an rpm it did not clamp; the setup sheet lists every clamped move.
- **Risk:** a machine whose real limit is a soft alarm and ours is a hard refusal. Fallback: the refusal is the safe direction and the number is in the message so the user can change the model.

### CAM-TOOL-019 — Where CAM data lives in the document
- **State:** BLOCKED
- **Depends on:** DAT-055, DAT-054, MOD-004, ASM-001
- **Files:** `crates/model/src/cam.rs` (new; owner D — **not CAM's file**), `crates/cam/src/model_io.rs` (owner CAM)
- **Contract:**
  ```rust
  // in `cadcraft_model`, behind the v2 `#[serde(default)]` rule of DAT-054:
  pub struct CamDocument {
      pub setups: Vec<cam::Setup>,
      pub library: cam::ToolLibrary,
      pub machines: Vec<cam::MachineModel>,
      pub operations: Vec<cam::Operation>,
  }
  ```
- **Behaviour:** The document owns CAM data; `cam` owns the types and never reads a `Document` (the `Exchange3::write(model, …)` discipline of `DAT-018`). Every field has `#[serde(default)]`, so a v1 file loads and simply has no CAM section. Multi-body (MOD-004) is why an operation names a *body selector*, not a solid.
- **Refusals:** a `.ccm` whose `cam` section names a tool the library does not have is refused at load with the tool name. `version > FORMAT_VERSION` is `TooNew` as everywhere (`DAT-055`).
- **Tests:** `a_document_without_a_cam_section_loads`. `a_cam_section_round_trips`. `an_operation_naming_an_unknown_tool_is_refused_with_its_name`. `cam_does_not_depend_on_model` — a layer test.
- **Acceptance:** the `.ccm` v2 field table in `docs/plan/DAT-FMT-v2.md` gains the `cam` row.
- **Risk:** `crates/model` is another owner's file and DAT-055 is not landed. Fallback: `cam::Operation` is serialisable on its own and the document field lands when DAT does; the tasks below that need persistence stay `BLOCKED` on this.

### CAM-TOOL-020 — The strategy × tool compatibility matrix is one table
- **State:** READY
- **Depends on:** CAM-TOOL-004, CAM-PATH-004
- **Files:** `crates/cam/src/strategy/registry.rs` (owner CAM)
- **Contract:** `pub fn compatible(strategy: StrategyKind, tool: ToolKind) -> bool;` and `pub fn check_all(op: &Operation) -> Result<(), CamError>;`
- **Behaviour:** One table decides which strategies may name which tools, asserted row by row. `check_all` runs it over a whole program before any path is generated, so the user learns about an incompatible pair before the first operation is planned.
- **Refusals:** an incompatible pair is `CamError::Unsupported("{strategy} needs a tool with {capability}; {tool} has none")` naming both.
- **Tests:** `every_registered_pair_is_in_the_table`. `a_tap_with_a_finishing_contour_is_refused`. `a_probe_with_a_strategies_refused`. `check_all_reports_the_first_offender_in_a_fixed_order`.
- **Acceptance:** the table is the only place a strategy/tool pair is judged.
- **Risk:** a new strategy added without a row. Fallback: the registry test enumerates strategies and fails on a missing row.

### CAM-TOOL-021 — Cutting data provenance in every number
- **State:** READY
- **Depends on:** CAM-TOOL-008, CAM-TOOL-009
- **Files:** `crates/cam/src/tool/speeds.rs::provenance` (owner CAM)
- **Contract:** `pub fn provenance(s: &SpeedResult, m: &MatData, p: &SpeedParams) -> Vec<String>;` — one line per number: formula, inputs, whether it was clamped, and by what.
- **Behaviour:** The post's header (`CAM-POST-004`) prints these lines. A machinist reading a program can see "n = 1000·200/(π·20) = 3183 rpm, clamped to 3000 by cc-router-300x200" and know exactly which number to change.
- **Refusals:** n/a (pure formatting); a non-finite input to `provenance` is formatted as the literal `non-finite` rather than `NaN`.
- **Tests:** `each_line_names_its_formula`. `a_clamped_value_names_the_limit`. `the_provenance_round_trips_through_a_post_header`.
- **Acceptance:** every rpm and feed in a posted program is traceable to a line of provenance.
- **Risk:** a long header annoys a shop. Fallback: a `verbose` flag; the header always carries the tool, the units and the spindle direction, which are the three a controller needs.

### CAM-TOOL-022 — Hostile tool values are refused at every entry point
- **State:** READY
- **Depends on:** CAM-TOOL-005, CAM-TOOL-007, CAM-TOOL-013
- **Files:** `crates/cam/tests/hostile_tools.rs` (owner Q, tests of CAM code)
- **Contract:** the file's test list is its contract: `hostile_tool_values_are_err_not_panic`, `a_machine_with_nan_limits_is_err`, `a_tool_library_from_a_hostile_document_is_err`, `a_speed_computation_on_a_nan_material_is_err`.
- **Behaviour:** 200 mutations of a valid tool JSON, a valid machine JSON and a valid setup JSON, each of which must return `Ok` or a named `Err`. This is the crash contract for a domain whose inputs are user-typed numbers on a machine.
- **Refusals:** every path returns `Err(CamError::Invalid(..))` or `Err(Limit { .. })`; no panic, no `NaN` in an output, no infinite loop.
- **Tests:** the four named above, plus `no_public_entry_point_panics` — a sweep over every `pub fn` of `crates/cam` with a hostile argument.
- **Acceptance:** green with the crate's `#![deny(clippy::unwrap_used, …)]` and `#![forbid(unsafe_code)]`.
- **Risk:** a hostile value that is *finite* but absurd (a `1e300` diameter) reaching geometry. Fallback: every field is range-checked against the machine limits before use, not only for finiteness.

---

# CAM-MACH — the machine as a subject: twin, offsets, programs, connection (18 tasks)

### CAM-MACH-001 — The machine file, and hostile parsing of it
- **State:** READY
- **Depends on:** CAM-TOOL-013
- **Files:** `crates/cam/src/machine/file.rs` (owner CAM)
- **Contract:** `pub fn machine_from_json(bytes: &[u8]) -> Result<MachineModel, CamError>;` and `pub fn machine_to_json(m: &MachineModel) -> Result<Vec<u8>, CamError>;`
- **Behaviour:** One machine per file, `serde_json` (already a workspace dependency). The file is the interchange form for a user machine, and it round-trips: a machine saved and re-read is identical, which is what lets a shop keep its machines beside its drawings.
- **Refusals:** a file over `MAX_MACHINE_BYTES = 1 MiB` → `CamError::Limit`. A JSON array of 10⁶ machines → `CamError::Limit { what: "machines", count, max: 64 }`. An axis with `min ≥ max`, a `NaN` in JSON (`serde_json` rejects it, and the message is passed through), an unknown `ToolChange` variant → `CamError::Invalid` naming the field.
- **Tests:** `a_machine_round_trips_through_its_file`. `a_machine_file_over_the_cap_is_refused`. `a_thousand_machine_array_is_refused`. `an_inverted_axis_is_refused_by_name`. `hostile_machine_files_never_panic` — 200 mutations.
- **Acceptance:** `machine_from_json(machine_to_json(m)?) == m` for every builtin template.
- **Risk:** a user edits the file and puts a plausible-but-wrong limit in. Fallback: the setup sheet prints the limits the program was checked against (`CAM-MACH-010`), so the file's numbers are visible at the machine.

### CAM-MACH-002 — The machine's working envelope as a solid
- **State:** READY
- **Depends on:** CAM-TOOL-013
- **Files:** `crates/cam/src/machine/envelope.rs` (owner CAM)
- **Contract:** `pub fn working_envelope(m: &MachineModel) -> Result<Solid, CamError>;`
- **Behaviour:** A box (or a cylinder, for a rotary machine's safe zone) built from the axis limits, used to lay out fixtures and to catch a program that leaves the envelope. It is a *conservative* volume: it contains everything the machine can reach, not the material.
- **Refusals:** a machine whose envelope cannot be built (non-finite limit) → `CamError::Invalid`. An envelope with a side below `2·tolerance_for(diagonal)` → `CamError::Invalid` naming the axis.
- **Tests:** `the_envelope_contains_the_stated_limits`. `a_rotary_machine_gets_a_cylinder`. `a_degenerate_envelope_is_refused`. `the_envelope_is_deterministic`.
- **Acceptance:** `CAM-MACH-003` places fixtures by intersecting with it.
- **Risk:** a machine with a non-box envelope (a gantry with a bridge) is modelled as a box. Fallback: documented; the envelope is a limit test, not a collision authority — collisions use `CAM-SIM-007` against the actual stock.

### CAM-MACH-003 — Fixtures and clamps as obstacles
- **State:** READY
- **Depends on:** CAM-MACH-002, KER-TOL-002
- **Files:** `crates/cam/src/machine/fixture.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub enum Fixture { Vise { jaws: [f64; 2], height: f64 }, Clamp { at: [f64; 3], size: [f64; 3] }, Tab { at: [f64; 3], height: f64 } }
  pub fn fixture_solid(f: &Fixture, origin: [f64; 3]) -> Result<Solid, CamError>;
  pub fn interference_report(s: &Solid, fixtures: &[Fixture], origin: [f64; 3]) -> Result<InterferenceReport, CamError>;
  ```
- **Behaviour:** A fixture is an obstacle solid placed in the setup. `interference_report` names which fixture solids intersect the part. This is `KER-TOL-002`'s first consumer and the reason it is on the blocked list.
- **Refusals:** a fixture intersecting the part is **reported, not refused** (a real job has a part in a vise and the *tool* must not hit the vise), but a fixture intersecting the **stock** is `CamError::Collision` naming the fixture and the volume, because there is no toolpath that machines around a clamp through the stock.
- **Tests:** `a_vise_does_not_intersect_the_part_but_does_the_stock`. `a_clamp_intersecting_the_stock_is_refused`. `a_fixture_outside_the_envelope_is_refused`. `interference_is_deterministic`.
- **Acceptance:** every setup with a fixture runs this check before any path is planned.
- **Risk:** our box clamp is not the shop's clamp. Fallback: the fixture is a user-entered solid with the dimensions printed on the setup sheet.

### CAM-MACH-004 — Work offsets per setup
- **State:** READY
- **Depends on:** CAM-TOOL-012
- **Files:** `crates/cam/src/machine/offsets.rs` (owner CAM)
- **Contract:** `pub struct OffsetTable { pub offsets: Vec<WorkOffset> }` with `pub struct WorkOffset { pub g: u8, pub machine_origin: [f64; 3] }` and `pub fn set_offset(t: &mut OffsetTable, setup: &Setup) -> Result<(), CamError>;` and `pub fn offset_g_code(t: &OffsetTable, g: u8) -> Option<String>;`
- **Behaviour:** The table maps `G54..G59` to machine origins; the post emits the code. A setup claims one G-code and the post emits it once, at the start, with a comment naming the stock top.
- **Refusals:** a G-code outside `1..=6` → `CamError::Invalid`. Two setups claiming the same G-code in one program → `CamError::Invalid("setups {a} and {b} both claim G{g}")` — the machine would re-zero mid-program.
- **Tests:** `an_offset_is_emitted_once_at_the_program_start`. `two_setups_one_g_code_is_refused`. `g59_is_the_highest_supported`. `an_offset_below_the_stock_top_is_allowed_and_marked`.
- **Acceptance:** every posted program has exactly one offset per setup and no two setups share one.
- **Risk:** the shop's G54 is not the stock top. Fallback: `machine_origin` is user-entered and printed in the header.

### CAM-MACH-005 — The NC program library, with the toolpath it came from
- **State:** READY
- **Depends on:** CAM-MACH-001, CAM-POST-003
- **Files:** `crates/cam/src/machine/program.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct NcProgram { pub name: String, pub machine: String, pub post: String, pub gcode: String, pub toolpath_hash: String, pub posted_with: String }
  impl NcProgram { pub fn verify_hash(&self) -> bool; }
  ```
- **Behaviour:** A posted program is stored with the machine and post that made it and a hash of the toolpath, so "this program was made from a toolpath that has since changed" is a fact, not a suspicion. The library is a folder the user points at; nothing is uploaded anywhere.
- **Refusals:** `verify_hash() == false` is a **refusal to run** the program (`CamError::Invalid("the program does not match the current toolpath")`), not a warning. A program over `MAX_PROGRAM_BYTES = 32 MiB` → `CamError::Limit`.
- **Tests:** `a_program_round_trips_with_its_hash`. `editing_the_toolpath_invalidates_the_program`. `a_tampered_program_fails_its_hash`. `an_oversized_program_is_refused`.
- **Acceptance:** the hash is over the toolpath's canonical text, so it is stable across runs.
- **Risk:** the hash is over *our* representation, so a change in CAM's serialisation invalidates every stored program. Fallback: the hash input is versioned (`cam-toolpath-v1:` prefix) and a mismatch is a re-post prompt, never a silent run.

### CAM-MACH-006 — Uploading a program to a machine: refused, with the reason
- **State:** READY
- **Depends on:** CAM-MACH-005
- **Files:** `crates/cam/src/machine/upload.rs` (owner CAM, module doc only)
- **Contract:** `pub fn upload_to_machine(_: &NcProgram, _: &MachineModel) -> Result<(), CamError> { Err(CamError::Unsupported("uploading a program needs a machine connection, which is refused in this version")) }`
- **Behaviour:** The function exists so the refusal is a **named error at the call site** rather than a missing feature discovered at the machine. The module doc states what a real implementation needs: a transport (FTP, SMB, or a vendor protocol), an authentication story and a `never crash` story for a socket that hangs mid-transfer. CADCraft does not have a network layer, and adding one is a platform decision (`CAM-MACH-014`).
- **Refusals:** the function's only behaviour is the refusal above. No socket, no DNS, no retry loop, no partial write.
- **Tests:** `upload_is_a_named_refusal` — asserts the error text contains "machine connection". `the_crate_opens_no_socket` — a test that `crates/cam` depends on no networking crate (a `Cargo.toml` assertion).
- **Acceptance:** the refusal is in the module doc, in `docs/plan/CAM.md`'s task list and in the error message.
- **Risk:** a user expects a DNC button. Fallback: "write to folder" is the whole story and is stated in the UI string.

### CAM-MACH-007 — MTConnect / OPC-UA machine status: refused by decision
- **State:** READY
- **Depends on:** CAM-MACH-006
- **Files:** `docs/plan/CAM-adr-0001-no-machine-connection.md` (new; owner PLT)
- **Contract:** the ADR text, no code: it states that machine status requires an HTTP/TCP client, an XML or JSON schema implementation, a device identity story and a polling loop that must never block the UI or panic on a half-open socket; that none of these exist in CADCraft; and that the revisit condition is a machine that has to be watched *during* a cut rather than before it.
- **Behaviour:** A decision record, in the house style of `step.rs`'s own limitations. `CADCraft` therefore has no machine connectivity, and the CAM UI never shows a "connected" light.
- **Refusals:** n/a (documentation); the *behavioural* refusal already exists as `CAM-MACH-006`.
- **Tests:** `the_adr_is_linked_from_cam_plan` — a test asserting the path resolves.
- **Acceptance:** the ADR exists; `CAM-SIM`'s "virtual machine" (`CAM-MACH-017`) is the answer given instead.
- **Risk:** someone adds a socket to the CAM crate. Fallback: the `no_new_dependencies` test in `CAM-MACH-006` plus the layer test in `CAM-TOOL-001`.

### CAM-MACH-008 — Tool life: minutes and cuts, and what a worn tool does
- **State:** BACKLOG
- **Depends on:** CAM-TOOL-007, CAM-MACH-005
- **Files:** `crates/cam/src/machine/life.rs` (owner CAM)
- **Contract:** `pub struct ToolLife { pub used_minutes: f64, pub used_cuts: u32, pub limit_minutes: f64, pub limit_cuts: u32 }` and `pub fn wear_factor(l: &ToolLife) -> Result<f64, CamError>;` (1.0 new, 0.5 at 100 % wear)
- **Behaviour:** Life accumulates from the *validated* cutting time (`CAM-SIM-010`) and the tool change count. Past a limit the tool is flagged and `wear_factor` scales the feed, and the warning appears in the run order (`CAM-MACH-013`) before the job starts.
- **Refusals:** a negative elapsed value → `CamError::Invalid`. A limit of 0 means "no limit" and is refused as ambiguous (`CamError::Invalid("a tool limit of 0 means no limit; use None")`) so a typo cannot silently disable the check.
- **Tests:** `life_accumulates_from_validated_time`. `a_worn_tool_scales_its_feed`. `a_zero_limit_is_refused_as_ambiguous`. `the_warning_names_the_tool_and_the_percentage`.
- **Acceptance:** the wear factor is applied when speeds are computed and appears in the provenance lines.
- **Risk:** the accumulated life is only as good as the time estimate. Fallback: the number carries the estimate's own uncertainty (`CAM-SIM-010`).

### CAM-MACH-009 — In-process verification: probe, measure, compare
- **State:** BLOCKED
- **Depends on:** CAM-TOOL-017, CAM-MACH-005, KER-MSH-006
- **Files:** `crates/cam/src/machine/verify.rs` (owner CAM)
- **Contract:** `pub fn verify_plan(m: &MachineModel, checks: &[Check]) -> Result<Vec<Move>, CamError>;` with `pub enum Check { HolePosition { at: [f64;3], expected: [f64;3], tol: f64 }, FeatureSize { edge: EdgeRef, expected: f64, tol: f64 }, SurfaceFlatness { face: usize, tol: f64 } }`
- **Behaviour:** Generates the probe moves and the pass/fail arithmetic. The **measurement is not taken** (no connection, `CAM-MACH-006`); the user runs the plan and enters numbers, and the comparison happens here. BLOCKED on `KER-MSH-006` for the distance query that a size check needs.
- **Refusals:** a check with `tol <= 0` → `CamError::Invalid`. A check on a feature the model does not have → `CamError::Invalid` naming it. A measurement the user enters as non-finite → `CamError::Invalid("the measurement is not a number")`, never a pass.
- **Tests:** `the_plan_is_generated`. `a_within_tolerance_check_passes`. `an_out_of_tolerance_check_fails_with_both_numbers`. `a_non_finite_measurement_is_refused_not_a_pass`. `a_check_on_a_missing_feature_is_refused`.
- **Acceptance:** the pass/fail table is printable and names the tolerance it used.
- **Risk:** a user types a passing number. Fallback: the entered number and the timestamp are recorded with the job, and the record says it was typed.

### CAM-MACH-010 — The setup sheet
- **State:** READY
- **Depends on:** CAM-TOOL-008, CAM-TOOL-021, CAM-MACH-003, CAM-MACH-004
- **Files:** `crates/cam/src/machine/sheet.rs` (owner CAM)
- **Contract:** `pub fn setup_sheet(job: &Job) -> Result<String, CamError>;` — plain text, no HTML renderer, no PDF library (no new dependencies).
- **Behaviour:** One page per setup: machine and its limits, stock and its allowance, work offset and the machine origin, the fixture and its interference report, the operations in order with tool, rpm, feed, feeds-and-sources lines, validation verdict, estimated time, and every clamped value. It is the document a machinist takes to the machine.
- **Refusals:** a job whose validation is `Unvalidated` prints the watermark in capitals and the word `DO NOT RUN` (`CAM-SIM-021`). A job with a `Failed` validation prints the gouge list. Never a clean sheet for an unvalidated job.
- **Tests:** `the_sheet_names_the_machine_limits`. `the_sheet_names_every_tool_and_its_feed`. `an_unvalidated_job_says_do_not_run`. `a_gouged_job_lists_the_gouges`. `the_sheet_is_deterministic`.
- **Acceptance:** the sheet is generated for every job and is the artefact attached to the NC program.
- **Risk:** a plain-text sheet is not what a shop prints. Fallback: the text is plain enough to paste anywhere; a PDF renderer is a UI concern, not CAM's.

### CAM-MACH-011 — Program provenance in the file itself
- **State:** READY
- **Depends on:** CAM-POST-004, CAM-MACH-005
- **Files:** `crates/cam/src/machine/provenance.rs` (owner CAM)
- **Contract:** `pub fn provenance_lines(job: &Job) -> Vec<String>;` — machine, post, post version, units, toolpath hash, document hash, date (or nothing, when provenance is off).
- **Behaviour:** With `provenance: false` the header is byte-reproducible (the DAT-012 property, reused). With `true` it carries the machine, the post and the hashes, so a program on a shop floor can be traced to a document revision.
- **Refusals:** a non-monotonic clock reading is refused (`CamError::Invalid("the clock went backwards")`) rather than written as a 1970 date — the same rule as `DAT-012`.
- **Tests:** `two_runs_without_provenance_are_byte_identical`. `provenance_names_the_machine_and_the_post`. `the_header_carries_the_toolpath_hash`. `a_backwards_clock_is_refused`.
- **Acceptance:** the property is the same as `DAT-012`'s: reproducibility by default.
- **Risk:** the date makes two exports differ. Fallback: it is off by default and the flag is per-post.

### CAM-MACH-012 — Dry run against the machine's kinematics
- **State:** READY
- **Depends on:** CAM-SIM-009, CAM-MACH-002
- **Files:** `crates/cam/src/machine/dryrun.rs` (owner CAM)
- **Contract:** `pub fn dry_run(prog: &NcProgram, m: &MachineModel) -> Result<DryRunReport, CamError>;` with `pub struct DryRunReport { pub blocks: usize, pub max_axis: [f64; 6], pub max_feed: f64, pub violations: Vec<CamError> }`
- **Behaviour:** Reads the program back (the reader from `CAM-POST-006`) and replays it through `MachineModel::within_limits`, reporting the extremes reached. This is the last gate before a program is considered runnable: it does not know what the part is, only what the machine can do.
- **Refusals:** a program whose units disagree with the machine's → `CamError::Invalid("the program is in inches and the machine is metric")`. Any axis violation → the report carries it and the function returns `Err` with the first, in axis order.
- **Tests:** `a_valid_program_dry_runs_clean`. `the_reported_max_feed_matches_the_program`. `a_unit_mismatch_is_refused`. `a_program_that_exceeds_a_limit_is_refused_with_the_axis`. `dry_run_is_deterministic`.
- **Acceptance:** a program cannot be marked runnable without a clean dry run, and the report is stored with it.
- **Risk:** the reader and the writer agree with each other and both are wrong about the controller. Fallback: `CAM-POST-009`'s independent simulator is the external check.

### CAM-MACH-013 — The job list: order, and the rules that refuse it
- **State:** READY
- **Depends on:** CAM-TOOL-016, CAM-MACH-004
- **Files:** `crates/cam/src/machine/jobs.rs` (owner CAM)
- **Contract:** `pub fn order_jobs(jobs: &[Job]) -> Result<Vec<JobOrder>, CamError>;` with `pub struct JobOrder { pub index: usize, pub tool: String, pub setup: String, pub est_time_s: f64, pub validation: Validation }`
- **Behaviour:** Groups by setup, orders by tool change count, and prints the run order. A job with a `Failed` validation is **excluded and named**; a job with `Unvalidated` is refused outright.
- **Refusals:** two setups sharing a G-code without an explicit `allow_offset_reuse` → `CAM-MACH-004`'s refusal. A job whose tool is not in the machine's magazine and whose change model is `Magazine` → `CamError::Invalid` naming the tool and the station count. Any `Unvalidated` job in the list → `CamError::Unvalidated` naming it.
- **Tests:** `jobs_are_ordered_by_setup_then_tool`. `a_failed_job_is_excluded_and_named`. `an_unvalidated_job_refuses_the_list`. `a_tool_beyond_the_magazine_is_refused`. `the_order_is_deterministic`.
- **Acceptance:** the order is a value that can be printed, saved and compared between runs.
- **Risk:** the "optimal" order is a scheduling problem we do not solve. Fallback: the order is a stated heuristic and the tool-change count it produced is printed.

### CAM-MACH-014 — The machine-connection decision, restated as code
- **State:** READY
- **Depends on:** CAM-MACH-006, CAM-MACH-007
- **Files:** `crates/cam/src/machine/connect.rs` (owner CAM)
- **Contract:** `pub fn connection_kind() -> ConnectionKind { ConnectionKind::None }` and `pub enum ConnectionKind { None }` — a non-empty enum on purpose.
- **Behaviour:** The type exists so that when a connection is added, the UI has a place to ask and the code has a place to match. Today the only variant is `None`, so every `match` is exhaustive and the compiler will name the places to change.
- **Refusals:** any code path that needs a live connection returns `CamError::Unsupported` naming `CAM-MACH-006`.
- **Tests:** `there_is_exactly_one_connection_kind`. `nothing_in_the_crate_names_a_socket`.
- **Acceptance:** the enum is exhaustive everywhere; adding a variant is a compile error until every site handles it.
- **Risk:** a future agent adds a variant without a transport. Fallback: the ADR's revisit condition (`CAM-MACH-007`) states what a real implementation must satisfy first.

### CAM-MACH-015 — A machine-model change invalidates the program
- **State:** READY
- **Depends on:** CAM-MACH-005, CAM-MACH-012
- **Files:** `crates/cam/src/machine/invalidate.rs` (owner CAM)
- **Contract:** `pub fn revalidate(p: &NcProgram, m: &MachineModel) -> Result<Revalidation, CamError>;` with `pub enum Revalidation { Still { dry_run: DryRunReport }, MustRepost { reason: String } }`
- **Behaviour:** Widening an axis limit does **not** re-validate a program (it is still safe); *narrowing* one does, because a move that was inside the old limit may be outside the new. The distinction is stated and tested, because "any change re-posts" trains users to ignore the warning.
- **Refusals:** a program whose machine is not found → `CamError::Invalid`. A narrowed limit that the program violates → `MustRepost` naming the axis, the program's extreme and the new limit.
- **Tests:** `widening_a_limit_keeps_the_program_valid`. `narrowing_a_limit_below_a_move_forces_a_repost`. `the_reason_names_the_axis_and_both_numbers`. `a_missing_machine_is_refused`.
- **Acceptance:** the rule is one function with one table test.
- **Risk:** a machine file edited by hand at the machine. Fallback: the program's header carries the limits it was checked against, so a mismatch is visible in the file.

### CAM-MACH-016 — Machine units and the post's units are one decision
- **State:** READY
- **Depends on:** CAM-TOOL-003, CAM-POST-008
- **Files:** `crates/cam/src/machine/units.rs` (owner CAM)
- **Contract:** `pub fn machine_units(m: &MachineModel) -> CamUnits;` and `pub fn assert_units_agree(m: &MachineModel, p: &NcProgram) -> Result<(), CamError>;`
- **Behaviour:** One place answers "what unit is this machine in", and one place asserts the program agrees. An inch machine posts `G20`; a metric machine posts `G21`; a disagreement is refused before the program reaches the machine.
- **Refusals:** a mismatch → `CamError::Invalid("the program is {a} and the machine is {b}")` naming both. A machine with a non-finite unit factor → `CamError::Invalid`.
- **Tests:** `an_inch_machine_posts_g20`. `a_metric_machine_posts_g21`. `a_mismatch_is_refused_with_both`. `the_reader_agrees_with_the_writer` — the cross-check with `CAM-POST-006`.
- **Acceptance:** exactly one `G20`/`G21` in a program, and it agrees with the model.
- **Risk:** the classic inch crash, and it is the one failure this domain cannot have. Fallback: `CAM-POST-006`'s reader refuses a unit disagreement at parse time, and `CAM-MACH-012` re-checks it from the file.

### CAM-MACH-017 — The virtual machine: run the program as fast as the CPU allows
- **State:** BACKLOG
- **Depends on:** CAM-SIM-004, CAM-MACH-012
- **Files:** `crates/cam/src/machine/virtual.rs` (owner CAM)
- **Contract:** `pub fn run_virtual(prog: &NcProgram, stock: &Solid, ctx: &SimContext) -> Result<VirtualRun, CamError>;` with `pub struct VirtualRun { pub removed: Solid, pub gouges: Vec<Gouge>, pub cut_time_s: f64, pub frames: Vec<Frame> }`
- **Behaviour:** Replays a posted program (not the toolpath) against a stock solid through the material-removal simulator, so the user sees what the **posted program** does rather than what the toolpath intended. This is the answer to `CAM-MACH-006`: the machine cannot be reached, so the program is executed by a machine that cannot hurt anyone.
- **Refusals:** a program whose validation fails → the run still happens and reports the gouge, because *seeing* the gouge is the point. A program with a move the reader refused → `CamError::PostRefusal` naming the word.
- **Tests:** `a_virtual_run_removes_the_expected_volume`. `a_program_that_gouges_reports_the_gouge`. `the_virtual_run_of_a_valid_program_has_no_gouges`. `a_word_the_reader_refuses_stops_the_run_with_its_name`.
- **Acceptance:** the virtual run is reachable from the job list and is the default preview.
- **Risk:** the simulator is only as good as the swept-volume primitive. Fallback: the run reports the chord tolerance it used (`CAM-TOOL-005`) and refuses a program whose tool profile cannot be sampled within it.

### CAM-MACH-018 — Machine-model property tests and determinism
- **State:** READY
- **Depends on:** CAM-TOOL-013, CAM-MACH-001, CAM-MACH-013
- **Files:** `crates/cam/tests/machine_properties.rs` (owner Q)
- **Contract:** `machine_within_limits_agrees_with_the_axis_range`; `a_point_inside_every_limit_is_accepted`; `travel_time_is_zero_for_a_zero_move`; `the_job_order_is_stable_under_permutation`; `hostile_machines_never_panic`.
- **Behaviour:** Randomised over generated machines: a point inside every axis range is accepted, a point outside any is refused naming that axis, and the job order is unchanged by the input permutation (or the tie-break is documented).
- **Refusals:** every generated machine with a contradictory spec is refused at `validate()`, so the property tests only run on well-formed machines and the malformed ones are refused by name.
- **Tests:** the five above, plus `within_limits_never_panics_on_a_non_finite_axis`.
- **Acceptance:** green, and the property tests are the ones `QAT-001` will re-run at every wave boundary.
- **Risk:** a flaky order under permutation. Fallback: the tie-break is `index`, which is stable, and the test pins it.

---

# CAM-PATH — toolpath generation (30 tasks)

### CAM-PATH-001 — The toolpath types: `Move`, `Motion`, `ArcMove`, `Toolpath`
- **State:** READY
- **Depends on:** CAM-TOOL-002, CAM-TOOL-003
- **Files:** `crates/cam/src/path/types.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum Motion { Rapid { to: Vec3 }, Feed { to: Vec3, feed: f64 }, Arc { to: Vec3, centre: Vec3, ccw: bool }, Drill { to: Vec3, cycle: Cycle } }
  #[derive(Clone, Debug, PartialEq)]
  pub struct Move { pub motion: Motion, pub tool: u8, pub spindle_rpm: f64, pub coolant: Option<bool> }
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct Toolpath { pub moves: Vec<Move>, pub params: PathParams }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Cycle { Peck { peck_depth: f64, dwell_s: f64 }, Tapping { pitch: f64, feed_per_rev: f64 } }
  ```
- **Behaviour:** One representation, consumed by the validator, the post, the preview and the time estimate. It stores **work coordinates in millimetres** and a feed in mm/min, and nothing else — no G-code words, no machine dialect, no vendor anything. A `Rapid` carries no feed because a rapid is not cut at a feed; the post spells the rapid rate separately.
- **Refusals:** a `Feed` with `feed <= 0` or non-finite → `CamError::Invalid` at construction (`Toolpath::checked`). An `Arc` whose `centre` is within `1e-9` of `to` or of the start → `CamError::Invalid("an arc of zero radius")`. A `Move` past `MAX_MOVES = 5_000_000` → `CamError::Limit`.
- **Tests:** `a_toolpath_round_trips_through_serde`. `a_zero_feed_move_is_refused`. `a_zero_radius_arc_is_refused`. `a_too_long_toolpath_is_refused`. `moves_carry_the_tool_number`.
- **Acceptance:** every other path task produces `Toolpath` and nothing else; the type is the seam.
- **Risk:** a dialect-specific move leaking into the neutral type. Fallback: the post is the only module that may name a G-word, and a source scan in `CAM-POST-002` asserts it.

### CAM-PATH-002 — Kinematic validation of a finished toolpath
- **State:** READY
- **Depends on:** CAM-PATH-001, CAM-TOOL-013
- **Files:** `crates/cam/src/path/validate.rs` (owner CAM)
- **Contract:** `pub fn validate_kinematics(tp: &Toolpath, m: &MachineModel) -> Result<PathReport, CamError>;` with `pub struct PathReport { pub moves: usize, pub length_mm: f64, pub cut_time_s: f64, pub violations: Vec<CamError> }`
- **Behaviour:** Walks the moves, converts each end point through the machine origin and calls `MachineModel::within_limits`. A violation is reported **per move**, in move order, so the generator can fix the one that is wrong rather than the first that trips.
- **Refusals:** any violation → the *report* lists all of them and the function returns `Err` with the first, in move order. A move whose end point is non-finite → `CamError::Invalid("move {i} is not a point")`.
- **Tests:** `a_path_within_the_limits_validates`. `a_move_past_a_limit_is_reported_with_its_index`. `every_violation_is_reported_not_just_the_first`. `a_non_finite_endpoint_is_refused`. `validate_is_deterministic`.
- **Acceptance:** no generator returns a `Toolpath` that has not been through this.
- **Risk:** a soft limit on the real machine. Fallback: the machine model holds the user's numbers and prints them (`CAM-MACH-010`).

### CAM-PATH-003 — `Operation`: what the user sets up
- **State:** BLOCKED
- **State note:** the type is ready to write; persistence is `BLOCKED` on `CAM-TOOL-019`.
- **Depends on:** CAM-TOOL-010, CAM-PATH-001
- **Files:** `crates/cam/src/path/operation.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Operation { pub name: String, pub setup: String, pub strategy: StrategyKind, pub tool: String, pub params: PathParams, pub target: Target, pub tolerance: f64 }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct PathParams { pub stepover: f64, pub stepdown: f64, pub feed_factor: f64, pub stock_to_leave: f64, pub clearance: f64 }
  ```
- **Behaviour:** An operation is the user's unit of intent (a pocket, a contour, a drill pattern) with its parameters. It is serialisable, and it is what the UI edits, the validator checks and the post spells. Parameters that do not apply to a strategy are **present but ignored**, and the sheet says so, rather than being a per-strategy struct zoo.
- **Refusals:** `stepover <= 0`, `stepdown <= 0`, `feed_factor <= 0`, `stock_to_leave < 0`, `clearance <= 0`, `tolerance <= 0` → `CamError::Invalid` naming the field and both numbers. A `stepover` larger than the tool diameter is **allowed** and flagged (it is a user's choice), but a `stepdown` larger than the machine's Z travel is refused.
- **Tests:** `an_operation_round_trips`. `a_zero_stepover_is_refused`. `a_negative_stock_to_leave_is_refused`. `unused_parameters_are_visible_not_hidden`.
- **Acceptance:** every parameter has a default, a range and a message.
- **Risk:** a parameter that means different things in two strategies. Fallback: each strategy documents which parameters it reads and a test asserts the set.

### CAM-PATH-004 — The strategy registry: one trait, one table
- **State:** READY
- **Depends on:** CAM-PATH-003, CAM-TOOL-004, CAM-TOOL-020
- **Files:** `crates/cam/src/strategy/mod.rs` (owner CAM)
- **Contract:**
  ```rust
  pub trait Strategy: Send + Sync {
      fn kind(&self) -> StrategyKind;
      fn caps(&self) -> ToolCaps;
      fn plan(&self, ctx: &PlanContext) -> Result<Toolpath, CamError>;
      fn params(&self) -> Vec<(&'static str, f64, f64)>;   // name, default, max
  }
  pub fn strategy(kind: StrategyKind) -> Option<Box<dyn Strategy>>;
  ```
- **Behaviour:** One trait, one registry, one table of `StrategyKind → box`. A new strategy is one module and one row. `plan` receives everything through `PlanContext` and returns a `Toolpath` or a refusal.
- **Refusals:** `strategy(kind)` for an unimplemented kind is `None`, and the caller turns that into `CamError::Unsupported("no strategy named {kind}")` listing what exists. A strategy that needs a capability its tool lacks is refused by `CAM-TOOL-020` before `plan` runs.
- **Tests:** `every_kind_resolves_to_a_strategy`. `an_unimplemented_kind_is_none_and_named`. `every_strategy_declares_its_parameters`. `no_strategy_returns_an_empty_toolpath_without_a_reason`.
- **Acceptance:** adding a strategy touches one module, one enum variant and one table row.
- **Risk:** a strategy reaching into the session or the document. Fallback: `PlanContext` is the only input, and the layer test keeps `cam` at L3.

### CAM-PATH-005 — `PlanContext`: everything a strategy may see
- **State:** READY
- **Depends on:** CAM-PATH-004, CAM-TOOL-010, CAM-TOOL-011
- **Files:** `crates/cam/src/path/context.rs` (owner CAM)
- **Contract:**
  ```rust
  pub struct PlanContext<'a> {
      pub solid: &'a Solid, pub stock: &'a Stock, pub setup: &'a Setup, pub tool: &'a Tool,
      pub machine: &'a MachineModel, pub op: &'a Operation, pub speed: SpeedResult,
      pub gouge: &'a mut GougeSink,      // generation-time checks write here
  }
  impl<'a> PlanContext<'a> { pub fn new(..) -> Result<PlanContext<'a>, CamError>; }
  ```
- **Behaviour:** Built by one constructor that checks the invariants a strategy must not have to check: the stock contains the part (`CAM-TOOL-011`), the tool validates, the tool fits the machine's spindle, the tool is in the library. A strategy that wants something else does not get it.
- **Refusals:** stock not containing the part → `CamError::Invalid`; a tool whose diameter exceeds the machine's capacity (a stated field) → `CamError::MachineRefusal`; a tool not in the setup's library → `CamError::Invalid` naming it.
- **Tests:** `a_context_over_a_valid_setup_builds`. `a_stock_that_excludes_the_part_is_refused`. `an_oversized_tool_is_refused`. `the_context_cannot_be_built_twice_with_different_tools`.
- **Acceptance:** no strategy constructs its own context; the invariants live in one constructor.
- **Risk:** a strategy that needs to see the document. Fallback: the document reaches CAM through `Operation`, never through the context.

### CAM-PATH-006 — The Z levels of a 2.5D job
- **State:** BLOCKED
- **Depends on:** KER-MSH-005, CAM-TOOL-005
- **Files:** `crates/cam/src/path/levels.rs` (owner CAM)
- **Contract:** `pub fn z_levels(ctx: &PlanContext, floor: f64) -> Result<Vec<f64>, CamError>;` — from the stock top down to the operation's floor, in `stepdown` increments, with the last pass shorter than the others and never below the floor.
- **Behaviour:** The level list is derived from the stock top and the part's own Z extent, so a part 3 mm tall in a 50 mm block gets two levels, not forty. The lowest level is exactly `floor` (or the part's minimum Z, whichever is lower).
- **Refusals:** a `stepdown` larger than the machine's Z travel → `CamError::MachineRefusal { axis: "Z", .. }`. A level below the machine's Z minimum → the same. More levels than `MAX_LEVELS = 100_000` → `CamError::Limit`. A floor above the part's top → `CamError::Invalid`.
- **Tests:** `levels_step_down_by_stepdown`. `the_last_level_lands_on_the_floor`. `a_short_part_gets_few_levels`. `a_stepdown_beyond_the_z_travel_is_refused`. `levels_are_deterministic`.
- **Acceptance:** every 2.5D strategy takes its levels from here.
- **Risk:** `KER-MSH-005` is one line in the kernel and it blocks this, `CAM-PATH-008` and the slicer. Fallback: the part's Z extent comes from `Solid::bounds()` and can substitute today, but the **pocket regions** cannot — those need the real section.

### CAM-PATH-007 — Profile offset: the tool-radius compensation primitive
- **State:** BLOCKED
- **Depends on:** KER-CAM-001, CAM-PATH-005
- **Files:** `crates/cam/src/path/offset.rs` (owner CAM, **adapter**)
- **Contract:** `pub fn offset_profile(r: &Regions, radius: f64, join: OffsetJoin, tol: f64) -> Result<Regions, CamError>;` — a thin adapter over `KER-CAM-001`, adding the CAM-specific join policy and the refusal text.
- **Behaviour:** Every finishing contour in CAM is "the model outline, offset by the tool radius, in the direction the material is". The adapter's only job is to choose the join (round for internal corners, miter for external) and to name the failure. It adds no geometry of its own.
- **Refusals:** an inward offset that **collapses the region** (a pocket narrower than the tool) is `CamError::Invalid("the pocket is {w} mm wide and the tool is {d} mm: it cannot be machined")` naming both numbers — this single message is the difference between a user learning why and a crash. A self-intersecting offset → `CamError::Invalid`. A non-finite radius → `CamError::Invalid`.
- **Tests:** `an_outset_of_a_square_grows_by_the_radius_on_each_side`. `an_inward_offset_shrinks_by_the_radius`. `an_offset_that_collapses_the_region_names_both_numbers`. `a_round_join_adds_arcs_as_segments`. `the_offset_is_deterministic`. `offsets_of_the_kernel_profiles_match_the_known_answers` — a table over the `support::` shapes.
- **Acceptance:** the table over the kernel's own test shapes passes; every finishing strategy uses this adapter.
- **Risk:** the kernel's offset is the hardest single contract in this file. Fallback: a polygonal offset by straight-segment displacement with a miter join is implementable inside `cam` **as a fallback**, is less robust at spikes, and must say so in the report when it is used.

### CAM-PATH-008 — Pocket detection: what is material at each level
- **State:** BLOCKED
- **Depends on:** CAM-PATH-006, KER-MSH-005, KER-MSH-001
- **Files:** `crates/cam/src/path/pockets.rs` (owner CAM)
- **Contract:** `pub fn pocket_at(solid: &Solid, z: f64, tol: f64) -> Result<Regions, CamError>;` — the regions of material whose cross-section at height `z` is **not** open to the air above `z`.
- **Behaviour:** For each Z level, section the solid (`KER-MSH-005`) and keep the regions that the stock does not already contain — that is the pocket at that level. This is what makes a pocket *a pocket* instead of a guessed rectangle.
- **Refusals:** a level where the section fails → `CamError::Kernel`. A level below the part's minimum → `Err(Unsupported("there is no material at z"))` rather than an empty region that a later stage would machine into thin air.
- **Tests:** `a_prism_gives_its_footprint_at_every_level`. `a_block_with_a_hole_gives_a_ring`. `a_level_above_the_part_is_refused`. `the_pocket_area_matches_the_section_area`. `pockets_are_deterministic`.
- **Acceptance:** every 2.5D strategy gets its regions here, never from a screen-space guess.
- **Risk:** a plane exactly on a face plane makes the section merge coplanar loops (`section.rs:38-49` explains this) and the result is right; a plane through an exact edge is the tolerance's job. Fallback: levels are offset from the face planes by `stepdown/2` when the two coincide, which is a standard trick and is tested.

### CAM-PATH-009 — Contour finishing: the side wall of a part
- **State:** BLOCKED
- **Depends on:** CAM-PATH-007, CAM-PATH-008, KER-CAM-001
- **Files:** `crates/cam/src/path/contour.rs` (owner CAM)
- **Contract:** `pub fn contour(ctx: &PlanContext, region: &Regions, climb: bool) -> Result<Toolpath, CamError>;`
- **Behaviour:** The boundary of the material at a level, offset **outward** by the tool radius, walked as one closed loop (climb or conventional). Multi-pass when `stepover` demands it — which for a contour means splitting the wall vertically, and that vertical step is `stepdown`. Lead-in and lead-out come from `CAM-PATH-018`.
- **Refusals:** a wall whose height exceeds the machine's Z travel → `CamError::MachineRefusal { axis: "Z", .. }`. A contour whose offset collapses (a wall thinner than the tool) → `CAM-PATH-007`'s refusal with the wall thickness. A non-closed loop → `CamError::Invalid("the contour is open")`.
- **Tests:** `a_contour_is_the_material_boundary_offset_by_the_radius`. `a_climb_and_a_conventional_contour_are_the_same_loop_reversed`. `a_multi_pass_wall_steps_by_stepdown`. `a_wall_thinner_than_the_tool_names_both_numbers`. `the_contour_is_closed`.
- **Acceptance:** the removed volume of one contour pass equals the swept volume the validator computes.
- **Risk:** a contour that cuts the top face because the offset is outward. Fallback: the lead-in/lead-out is a separate move (`CAM-PATH-018`) and the validator catches any residual (`CAM-SIM-004`).

### CAM-PATH-010 — Pocket finishing: an inward spiral to the floor
- **State:** BLOCKED
- **Depends on:** CAM-PATH-007, CAM-PATH-008
- **Files:** `crates/cam/src/path/pocket.rs` (owner CAM)
- **Contract:** `pub fn pocket_finish(ctx: &PlanContext, region: &Regions) -> Result<Toolpath, CamError>;`
- **Behaviour:** Offset the pocket boundary inward by the tool radius, then spiral inward by `stepover` per pass until the remaining region is smaller than the tool; the last pass follows the region's own boundary exactly, and a floor pass at the level below finishes the bottom.
- **Refusals:** a pocket narrower than the tool diameter → `CAM-PATH-007`'s refusal naming the width and the diameter. A remaining region smaller than `min_region_area` (`0.01 mm²`) is **reported and dropped**, never machined as a zero-length move.
- **Tests:** `a_square_pocket_spirals_inward_by_stepover`. `the_last_pass_follows_the_boundary`. `a_narrow_pocket_names_both_numbers`. `a_sliver_region_is_dropped_and_reported`. `pocket_area_is_fully_covered` — the union of the swept passes equals the pocket area to `1e-6`.
- **Acceptance:** the pocket-finish test asserts coverage, which is the property that catches a missing pass.
- **Risk:** an inward spiral on a non-convex pocket self-intersects. Fallback: `KER-CAM-001`'s result is a valid region at every step, and a failed step is refused rather than drawn.

### CAM-PATH-011 — Facing: clearing the top of the stock
- **State:** READY
- **Depends on:** CAM-PATH-005, CAM-TOOL-005
- **Files:** `crates/cam/src/path/face.rs` (owner CAM)
- **Contract:** `pub fn face(ctx: &PlanContext, mode: FaceMode) -> Result<Toolpath, CamError>;` with `pub enum FaceMode { ZigZag { angle_deg: f64, overlap: f64 }, OneWay { angle_deg: f64 }, Spiral { inset: f64 } }`
- **Behaviour:** A raster over the stock's top rectangle at the Z level that removes `allowance` from it. The angle, the overlap and the direction are parameters; the pass count is derived, not typed in.
- **Refusals:** a stock top above the machine's Z maximum → `CamError::MachineRefusal { axis: "Z", .. }`. An `overlap < 0` or `> 0.9·diameter` → `CamError::Invalid` naming the field and both numbers. A face mode on a stock with no top face (an inverted box) → `CamError::Invalid`.
- **Tests:** `zigzag_covers_the_stock_top`. `the_overlap_widens_each_pass`. `a_face_angle_outside_0_90_is_refused`. `the_number_of_passes_matches_the_area_and_the_stepover`. `the_face_leaves_exactly_the_allowance`.
- **Acceptance:** the removed volume equals the stock-top area × the allowance depth, to `1e-6`.
- **Risk:** a facing pass over a clamp. Fallback: `CAM-MACH-003`'s fixture solids are obstacles and the validator reports a face that runs into one.

### CAM-PATH-012 — Adaptive clearing (2D) and its honest boundary
- **State:** BLOCKED
- **Depends on:** CAM-PATH-007, CAM-PATH-010
- **Files:** `crates/cam/src/path/adaptive.rs` (owner CAM)
- **Contract:** `pub fn adaptive_clear(ctx: &PlanContext) -> Result<Toolpath, CamError>;`
- **Behaviour:** Repeatedly offset the remaining region inward by a *variable* stepover derived from the region's local width (a narrow channel gets fine passes, a wide floor gets coarse ones), emitting one pass per offset ring and re-testing the remainder against the previous operation's toolpath (`CAM-PATH-017`).
- **Refusals:** a remaining region smaller than `min_region_area` → reported and dropped. An offset that self-intersects → `CamError::Invalid("the offset ring {i} self-intersects; the pocket has a narrow neck")`. An op that exceeds `MAX_MOVES` → `CamError::Limit` naming the count and the cap.
- **Tests:** `the_remainder_shrinks_monotonically`. `a_wide_floor_gets_fewer_passes_than_a_narrow_channel`. `the_removal_is_complete_to_the_stored_stock`. `a_narrow_neck_is_refused_with_the_ring_number`. `adaptive_is_deterministic`.
- **Acceptance:** the remaining-area assertion is the test; without it a clearing strategy that leaves material looks fine on screen.
- **Risk:** adaptive clearing is where most CAM products' quality claims live, and ours is a 2D offset recursion. Fallback: the strategy reports its own parameters and the store simulation measures the result — a claim is never made without a number.

### CAM-PATH-013 — Drilling: positions, cycles, pecking, breakthrough
- **State:** READY
- **Depends on:** CAM-TOOL-004, CAM-PATH-005
- **Files:** `crates/cam/src/path/drill.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn drill_plan(ctx: &PlanContext, holes: &[Hole], cycle: Cycle) -> Result<Toolpath, CamError>;
  pub struct Hole { pub at: Vec3, pub diameter: f64, pub depth: f64, pub through: bool }
  ```
- **Behaviour:** Each hole becomes a sequence: rapid to a safe Z, feed to the first peck depth, rapid (or a dwell) back, repeat, then a final feed to the full depth and a **retract past the top** when `through` is set. Peck depth is clamped so the last peck is not a zero-length move.
- **Refusals:** a hole diameter smaller than the tool → `CamError::Invalid("hole {i} is {d} mm and the drill is {t} mm")`. A peck depth ≤ 0 → `CamError::Invalid`. A depth beyond the machine's Z travel → `CAM-PATH-002`'s refusal. A `Tapping` cycle whose `pitch` does not match the thread's pitch is refused by name — **a wrong pitch shears the tap**.
- **Tests:** `a_hole_pecks_to_its_depth`. `the_last_peck_is_not_zero_length`. `a_through_hole_retracts_past_the_top`. `a_tap_pitch_mismatch_is_refused`. `the_number_of_pecks_matches_the_depth_and_the_peck`. `drilling_is_deterministic`.
- **Acceptance:** the drill positions are generated from geometry elsewhere (`CAM-PATH-024`, `CAM-PATH-031` patterns) and this task only turns positions into moves.
- **Risk:** a drill's point angle means the hole is deeper than the tip travel. Fallback: the tip angle is a `Tool` field and the drill depth is measured from the tip; the difference is printed.

### CAM-PATH-014 — Hole ordering: nearest neighbour with a 2-opt pass
- **State:** READY
- **Depends on:** CAM-PATH-013
- **Files:** `crates/cam/src/path/order.rs` (owner CAM)
- **Contract:** `pub fn order_holes(holes: &[Hole], from: Vec3) -> Result<Vec<usize>, CamError>;`
- **Behaviour:** Nearest-neighbour from the start point, then a 2-opt improvement pass until no swap shortens the tour by more than `tol`. Deterministic: ties break on the index.
- **Refusals:** more holes than `MAX_HOLES = 1_000_000` → `CamError::Limit`. A non-finite hole position → `CamError::Invalid` naming the index. A zero-length tour (`n == 0`) → `Ok(vec![])`, not an error.
- **Tests:** `the_order_is_shorter_than_the_input_order`. `two_opt_does_not_lengthen_the_tour`. `the_order_is_stable_under_a_rotation_of_the_input`. `every_hole_appears_exactly_once`. `an_empty_list_is_empty`.
- **Acceptance:** the ordering is deterministic, which is the property the test pins.
- **Risk:** 2-opt on 10⁶ holes is O(n²) per pass. Fallback: the pass count is bounded and the improvement threshold is `1e-6` of the tour length; beyond `MAX_HOLES` the strategy is refused rather than truncated.

### CAM-PATH-015 — 3D contour: parallel passes at Z levels
- **State:** READY
- **Depends on:** CAM-PATH-006, CAM-PATH-005
- **Files:** `crates/cam/src/path/contour3d.rs` (owner CAM)
- **Contract:** `pub fn contour_3d(ctx: &PlanContext, angle_deg: f64, mode: RasterMode) -> Result<Toolpath, CamError>;` with `pub enum RasterMode { ZigZag, OneWay, Spiral }`
- **Behaviour:** Slices the part's bounding box at `stepdown`, and at each level rasters the material's cross-section (`CAM-PATH-008`) with passes at `stepover` spacing, clipping each pass to the regions. This is the honest 3D parallel strategy; there is no claim of zig-zag scallop optimisation, and the scallop height is **computed and printed** (`error = stepover²/(8·tool_radius)`).
- **Refusals:** a level outside the machine's Z range → `CAM-PATH-002`'s refusal. A part whose Z extent exceeds the machine's travel → `CamError::Unsupported("the part is {h} mm tall and the machine travels {t} mm")` naming both. More than `MAX_LEVELS` levels → `CamError::Limit`.
- **Tests:** `every_material_point_is_covered_within_the_scallop_error`. `the_scallop_error_matches_the_formula`. `a_part_taller_than_the_machine_is_refused_with_both_numbers`. `a_spiral_uses_one_pass_per_ring`.
- **Acceptance:** the coverage assertion is the acceptance criterion; a strategy that leaves a region unscanned fails.
- **Risk:** ball-nose scallop is not the flat-end formula. Fallback: the tool kind selects the formula and the module doc names which tools each applies to.

### CAM-PATH-016 — 3D roughing: refused, by name, with what to use instead
- **State:** READY
- **Depends on:** CAM-PATH-015
- **Files:** `crates/cam/src/path/rough3d.rs` (owner CAM)
- **Contract:** `pub fn rough_3d(_: &PlanContext) -> Result<Toolpath, CamError> { Err(CamError::Unsupported("3D roughing is not implemented; use 2.5D clearing (CAM-PATH-012) or 3D parallel finishing (CAM-PATH-015)")) }`
- **Behaviour:** The function exists so the refusal is a **named error with an alternative** rather than a strategy missing from a menu. The module doc explains what a 3D roughing strategy needs (a voxel decomposition of the model, a stock-aware distance field, and clearance-aware linking) and that none of it exists.
- **Refusals:** the function's only behaviour is the refusal. The UI's strategy list shows it greyed with the reason, never hidden.
- **Tests:** `roughing_3d_is_a_named_refusal` — the message names both alternatives. `the_strategy_is_registered_but_refuses` — so the list shows it.
- **Acceptance:** no code path can produce a "3D roughing" toolpath.
- **Risk:** users see a missing feature. Fallback: the refusal names what exists, which is the honest answer.

### CAM-PATH-017 — Rest machining: what is left after the previous operation
- **State:** BLOCKED
- **Depends on:** CAM-SIM-003, CAM-PATH-012
- **Files:** `crates/cam/src/path/rest.rs` (owner CAM)
- **Contract:** `pub fn rest_machining(ctx: &PlanContext, previous: &[&Toolpath]) -> Result<Toolpath, CamError>;`
- **Behaviour:** Takes the stock, subtracts the previous operations' **validated** swept volumes, and clears only what remains. This is why it depends on the simulator and not on the toolpath: the remainder is what the material actually is, not what a previous path was supposed to remove.
- **Refusals:** a previous operation whose validation is `Failed` → `CamError::Unvalidated { reason: "operation {i} gouged; the remainder is not trustworthy" }`. A remainder with no material → `Ok(empty)` with a note, not an error.
- **Tests:** `rest_machining_removes_nothing_the_previous_pass_did`. `a_gouged_previous_operation_refuses_the_rest`. `an_empty_remainder_is_an_empty_path`. `rest_is_deterministic`.
- **Acceptance:** the removed volume of a roughing + finishing pair equals the pocket volume to `1e-6`.
- **Risk:** the remainder inherits every error of the material simulation, including the boolean defects. Fallback: `KER-BOOL-001..003` are a hard gate on `CAM-SIM-003`, which this depends on.

### CAM-PATH-018 — Linking moves: lead-in, lead-out, clearance, safe Z
- **State:** READY
- **Depends on:** CAM-PATH-001, CAM-TOOL-006
- **Files:** `crates/cam/src/path/link.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Lead { None, Linear { length: f64 }, Arc { radius: f64, ccw: bool }, Helical { radius: f64, pitch: f64 } }
  pub fn link(a: Vec3, b: Vec3, params: &LinkParams) -> Result<Vec<Move>, CamError>;
  pub struct LinkParams { pub clearance: f64, pub safe_z: f64, pub lead_in: Lead, pub lead_out: Lead }
  ```
- **Behaviour:** Between two cutting sequences: rapid to `safe_z`, traverse at feed height, and lead in/out with the chosen shape. A helical lead-in's pitch is clamped so the tool never advances more than `0.5·diameter` per revolution.
- **Refusals:** a clearance below `tolerance_for(diagonal)` → `CamError::Invalid`. A safe Z below the stock top → `CamError::Collision` naming the height and the top. A helical pitch over `0.5·diameter` → clamped and reported, never refused (it is a user's setting and the clamp is safe). A lead-in longer than the clearance allows → refused, naming both.
- **Tests:** `a_link_raises_to_the_safe_height`. `a_helical_lead_in_advances_half_a_diameter_per_turn`. `a_lead_in_longer_than_the_clearance_is_refused`. `a_safe_z_below_the_stock_is_refused`.
- **Acceptance:** every strategy emits its links through this function; none hand-rolls a rapid.
- **Risk:** a lead-in that gouges the neighbouring wall. Fallback: the gouge validator runs over the whole toolpath including links.

### CAM-PATH-019 — Arc fitting: polylines that were arcs
- **State:** READY
- **Depends on:** CAM-PATH-001, CAM-TOOL-007
- **Files:** `crates/cam/src/path/arcfit.rs` (owner CAM)
- **Contract:** `pub fn fit_arcs(pts: &[Vec3], tol: f64) -> Result<PathParams2, CamError>;` with `pub struct PathParams2 { pub arcs: Vec<ArcSeg>, pub lines: Vec<(Vec3, Vec3)> }` and `pub struct ArcSeg { pub centre: Vec3, pub radius: f64, pub start: f64, pub end: f64, pub ccw: bool }`
- **Behaviour:** Recognises a run of points as one arc when the three-point circle fit's **maximum radial deviation** is within `tol`, and emits `Motion::Arc`. The number of points per revolution of a faceted curve is bounded by the tessellation that produced them, so this recovers the arc the kernel faceted rather than inventing one.
- **Refusals:** a run whose deviation exceeds `tol` → kept as lines (a **decision**, recorded in the report's line count), never a forced arc. A sweep over 180° → `CamError::Invalid("an arc may not exceed half a turn")`, because `CAM-POST-018`'s `R` word cannot express more.
- **Tests:** `a_circle_sampled_at_16_points_fits_one_arc`. `a_polyline_stays_lines`. `the_deviation_is_the_maximum_radial_distance_not_the_mean`. `a_fit_is_deterministic`. `an_arc_over_half_a_turn_is_refused`.
- **Acceptance:** a posted program contains arcs exactly where the fit found them, and the read-back test agrees.
- **Risk:** fitting an arc to a *different* arc that happens to pass near three points. Fallback: the max-deviation criterion and the tolerance recorded in the operation.

### CAM-PATH-020 — Corner smoothing at internal corners
- **State:** READY
- **Depends on:** CAM-PATH-019, KER-CAM-001
- **Files:** `crates/cam/src/path/smooth.rs` (owner CAM)
- **Contract:** `pub fn smooth_corners(pts: &[Vec3], radius: f64, min_ratio: f64) -> Result<Vec<Vec3>, CamError>;`
- **Behaviour:** Rounds an internal corner by the offset intersection, and **refuses any radius smaller than `min_ratio · tool_radius`** (default 0.5): a fillet tighter than half the tool radius cannot be cut by that tool and the shop needs to know before the program exists.
- **Refusals:** `radius < min_ratio·tool_radius` → `CamError::Invalid("the corner radius {r} is under {k}× the tool radius {t}")` naming all three. A radius larger than the two legs it joins → `CamError::Invalid`. A corner that is convex (external) is left sharp and reported.
- **Tests:** `an_internal_corner_gets_a_tangent_arc`. `a_too_small_radius_is_refused_with_three_numbers`. `a_too_large_radius_is_refused`. `an_external_corner_is_left_sharp`. `smoothing_is_deterministic`.
- **Acceptance:** the refusal names all three numbers, which is what makes it actionable.
- **Risk:** smoothing changes the part. Fallback: it is a *toolpath* operation on a path, never on the model; the model is untouched and the difference is shown in the preview.

### CAM-PATH-021 — Entry: straight, ramp, helical, with the angle from the tool
- **State:** READY
- **Depends on:** CAM-PATH-005, CAM-PATH-018
- **Files:** `crates/cam/src/path/entry.rs` (owner CAM)
- **Contract:** `pub fn max_ramp_angle(tool: &Tool, fz_factor: f64) -> Result<f64, CamError>;` and `pub fn entry_move(ctx: &PlanContext, kind: EntryKind) -> Result<Vec<Move>, CamError>;`
- **Behaviour:** The maximum ramp angle is **derived from the tool's flute count and the feed factor**, not a constant: `tan(a) ≤ z·fz_factor·sin(θ)/D` is not available without a cutter model, so the rule is the published one, written down and cited: a flat end mill enters at `atan(1/(z·cos(helocut)))`. Helical entry for a ball nose descends along a helix of the tool's own radius.
- **Refusals:** an angle above the derived maximum → `CamError::Invalid` naming the angle and the maximum. A helical entry on a tool with no defined nose radius → `CamError::Unsupported`. A straight plunge deeper than `min_plunge_depth` (default `0.1·diameter`) → `CamError::Invalid`.
- **Tests:** `the_max_angle_is_derived_not_a_constant` — a 2-flute and a 4-flute give different angles. `an_angle_above_the_maximum_is_refused_with_both`. `a_deep_plunge_is_refused`. `a_helical_entry_wraps_the_tool_radius`.
- **Acceptance:** the formula is in the source with its citation and one test asserts the numbers.
- **Risk:** the formula is a rule of thumb. Fallback: the store simulation (`CAM-SIM-003`) is what measures whether the entry worked.

### CAM-PATH-022 — Generation-time gouge check
- **State:** BLOCKED
- **Depends on:** KER-MSH-006, KER-MSH-001, CAM-PATH-002
- **Files:** `crates/cam/src/path/gouge_guard.rs` (owner CAM)
- **Contract:** `pub fn check_gouge(tp: &Toolpath, solid: &Solid, tool: &Tool, tol: f64) -> Result<GougeReport, CamError>;`
- **Behaviour:** For each move, the distance from the cutter's swept envelope to the solid is compared with zero; a negative distance is material removed. It is the **same arithmetic** as `CAM-SIM-004`, at planning time, so a generator can refuse before a program exists. It reports into `GougeSink` on the context rather than returning an error, so several strategies can accumulate.
- **Refusals:** any gouge → the report is returned and the caller decides; `PlanContext`'s gouge sink **makes the operation fail validation** (`CAM-SIM-004`), which makes posting impossible. A non-finite tolerance → `CamError::Invalid`.
- **Tests:** `a_path_that_touches_the_surface_does_not_gouge`. `a_path_0_01_mm_inside_gouges_by_0_01`. `the_report_names_the_move_and_the_depth`. `a_gouged_path_fails_validation`. `check_gouge_is_deterministic`.
- **Acceptance:** the same numbers as the simulator, asserted by a shared test over the same fixtures.
- **Risk:** a distance query to 60 000 faces is slow. Fallback: it runs once at plan time, off the frame path, with the `CAM-SIM-016` budget applying.

### CAM-PATH-023 — 5-axis: the scope decision
- **State:** READY
- **Depends on:** CAM-TOOL-015
- **Files:** `docs/plan/CAM-adr-0002-positioning-only.md` (new; owner PLT), `crates/cam/src/path/five_axis.rs` (owner CAM)
- **Contract:** `pub fn five_axis(_: &PlanContext) -> Result<Toolpath, CamError> { Err(CamError::Unsupported("only 3-axis and 3+2 positioning toolpaths are implemented")) }`
- **Behaviour:** The ADR records: simultaneous 5-axis needs a machine's kinematic chain (the controller's own interpolation, not ours), a non-trivial-continuous tool-contact model, and a collision model that is not a set of convex sweeps — none of which exist. **3+2 positioning is supported** (`CAM-TOOL-015`) and every rotary value is constant within an operation.
- **Refusals:** an operation asking for a rotary move *within* a cutting sequence → `CamError::Unsupported("rotary motion inside a cutting move is not implemented")`.
- **Tests:** `five_axis_is_a_named_refusal`. `three_plus_two_is_accepted`. `a_rotary_move_inside_a_cut_is_refused`. `the_adr_exists`.
- **Acceptance:** the ADR is linked from this file and from `docs/file-format-parity.md`-style parity docs when they exist.
- **Risk:** positioning-only is not what a 5-axis shop expects. Fallback: the refusal names what is supported.

### CAM-PATH-024 — Feature recognition from the B-rep
- **State:** BLOCKED
- **Depends on:** KER-SRF-003, KER-MSH-003, CAM-PATH-013
- **Files:** `crates/cam/src/path/features.rs` (owner CAM)
- **Contract:** `pub fn recognise(solid: &Solid) -> Result<Vec<Feature>, CamError>;` with `pub enum Feature { Hole { centre: Vec3, radius: f64, axis: Vec3, depth: f64 }, Boss { .. }, Slot { .. }, Pocket { face: FaceId, outline: Regions } }`
- **Behaviour:** Recognises a hole as **a set of coplanar side faces whose centres are equidistant from a common axis** — which, on a faceted kernel, means a bore is an n-gon prism and is recognised as such (with `n` in the result, so the user knows the model is a 64-gon hole, not a circle). Anything that is not confidently recognised is returned in a `unrecognised` list rather than guessed.
- **Refusals:** a candidate whose centres are not equidistant within `tolerance_for(diagonal)` → not a feature, and it appears in `unrecognised` with the measured spread. A recognition that would produce a radius smaller than `tolerance_for` → `CamError::Kernel`.
- **Tests:** `a_prismatic_bore_is_recognised_with_its_facet_count`. `an_off_centre_set_of_faces_is_not_a_hole`. `a_recognised_hole_drills_correctly`. `recognition_is_deterministic`. `unrecognised_faces_are_listed_not_guessed`.
- **Acceptance:** every recognised feature names the faces it came from (needs `KER-MSH-001`), so a wrong recognition is traceable.
- **Risk:** recognising a *faceted* bore as a hole and drilling a circle leaves the facets. Fallback: the feature carries `facets: u8` and the drill path is a polygon unless the user picks a circular path with a stated chord error.

### CAM-PATH-025 — Operation ordering and its cost model
- **State:** READY
- **Depends on:** CAM-TOOL-016, CAM-PATH-004
- **Files:** `crates/cam/src/path/sequence.rs` (owner CAM)
- **Contract:** `pub fn order_operations(ops: &[Operation], machine: &MachineModel) -> Result<Vec<usize>, CamError>;` and `pub struct SeqCost { pub tool_changes: usize, pub rapid_mm: f64, pub est_time_s: f64 }`
- **Behaviour:** Orders by setup, then by tool, then by strategy priority (roughing before finishing within a tool), and reports the tool-change count it achieved. The cost model is the honest one: `time = Σ(length_i/feed_i) + rapid_length/rapid_rate + changes·change_time`.
- **Refusals:** an operation whose tool is not in the machine's magazine → `CamError::Invalid` naming the tool and the stations. Two operations in one setup with different work offsets → `CamError::Invalid`.
- **Tests:** `roughing_precedes_finishing_with_one_tool`. `the_tool_change_count_is_reported`. `the_cost_is_the_formula`. `an_order_with_a_missing_tool_is_refused`. `ordering_is_deterministic`.
- **Acceptance:** the cost is a number in the sheet, with the formula next to it.
- **Risk:** a "best" order is a real optimisation problem. Fallback: the heuristic is named and its output is shown before the user accepts it.

### CAM-PATH-026 — Editing a toolpath after the fact
- **State:** READY
- **Depends on:** CAM-PATH-001, CAM-PATH-002
- **Files:** `crates/cam/src/path/edit.rs` (owner CAM)
- **Contract:**
  ```rust
  pub enum Edit { RemoveRange { from: usize, to: usize }, Reverse, Truncate { at: usize }, Retract { at: Vec3 }, ReplaceMove { index: usize, with: Move } }
  pub fn apply(tp: &Toolpath, e: &Edit) -> Result<Toolpath, CamError>;
  ```
- **Behaviour:** Every edit returns a **new** `Toolpath` and re-runs `validate_kinematics` before returning it. Truncating a path mid-sequence leaves a tool change to make; `Edit::Truncate` records that so the post emits it.
- **Refusals:** an out-of-range `from`/`to`/`index` → `CamError::Invalid` naming the index and the count. An edit producing a path that fails kinematic validation → `Err`, never returned. Reversing a path containing an arc → the arc's sense flips, and if the result is not geometrically the same path it is refused.
- **Tests:** `removing_a_range_leaves_the_rest`. `truncating_records_the_tool_change`. `an_out_of_range_edit_is_refused`. `an_edit_that_breaks_a_limit_is_refused`. `reverse_is_the_same_path_backwards`.
- **Acceptance:** no path leaves this module unvalidated.
- **Risk:** a hand-edited path is how a machinist fixes a real job. Fallback: edits are first-class, recorded in the operation's log and re-validated.

### CAM-PATH-027 — Time estimation from the toolpath
- **State:** READY
- **Depends on:** CAM-PATH-002, CAM-TOOL-008
- **Files:** `crates/cam/src/path/time.rs` (owner CAM)
- **Contract:** `pub fn estimate(tp: &Toolpath, m: &MachineModel) -> Result<TimeEstimate, CamError>;` with `pub struct TimeEstimate { pub cut_s: f64, pub rapid_s: f64, pub change_s: f64, pub total_s: f64, pub uncertainty: f64 }`
- **Behaviour:** `cut = Σ length_i / feed_i` over feed moves, `rapid = Σ length_i / rapid_rate` over rapids, `change = n · change_time`. `uncertainty` is stated, not computed to false precision: it is the ratio of the acceleration time we do **not** model to the total, and it says so.
- **Refusals:** a feed of 0 on a feed move → `CamError::Invalid("move {i} has no feed")`. A machine with `rapid_mm_min <= 0` → `CamError::Invalid`. A non-finite feed → refused.
- **Tests:** `the_time_is_the_sum_of_the_formulas`. `an_arcs_length_uses_its_arc_length_not_its_chord`. `a_zero_feed_is_refused`. `the_uncertainty_is_reported`. `estimation_is_deterministic`.
- **Acceptance:** the same estimate drives the sheet, the job list and the wear accumulator, and all three read one function.
- **Risk:** the number is optimistic because acceleration is not modelled. Fallback: `uncertainty` is in the sheet next to it, and the field's doc says what is missing.

### CAM-PATH-028 — Per-move feeds: a table the user can lock
- **State:** READY
- **Depends on:** CAM-TOOL-008, CAM-PATH-001
- **Files:** `crates/cam/src/path/feeds.rs` (owner CAM)
- **Contract:** `pub fn per_move(tp: &Toolpath, table: &FeedTable) -> Result<Toolpath, CamError>;` with `pub struct FeedTable { pub entry: f64, pub ramp_factor: f64, pub small_radius_factor: f64, pub lock: Option<Vec<f64>> }`
- **Behaviour:** Scales the feed by motion type: full feed in the cut, a reduced feed on a ramp, a further reduction where the tool's engaged radius is below the radius the feed was computed for (`small_radius_factor`), and the tool change at zero. The user's lock overrides everything, per move.
- **Refusals:** a lock of the wrong length → `CamError::Invalid("the lock has {n} feeds and the path has {m} moves")`. A factor ≤ 0 or > 1 (except the ramp factor) → `CamError::Invalid`.
- **Tests:** `a_ramp_move_gets_the_ramp_factor`. `a_tight_corner_gets_the_small_radius_factor`. `a_lock_overrides_everything`. `a_lock_of_the_wrong_length_is_refused`. `the_table_is_deterministic`.
- **Acceptance:** every feed in a path is the result of this function or a lock.
- **Risk:** a user locks a feed and the machine clamps it. Fallback: `CAM-TOOL-018`'s clamp runs **after** the lock and marks the move.

### CAM-PATH-029 — Toolpath persistence through the document
- **State:** BLOCKED
- **Depends on:** CAM-TOOL-019, CAM-PATH-001
- **Files:** `crates/cam/src/path/serde_io.rs` (owner CAM)
- **Contract:** `pub fn toolpath_to_json(tp: &Toolpath) -> Result<Vec<u8>, CamError>;` and `pub fn toolpath_from_json(b: &[u8]) -> Result<Toolpath, CamError>;`
- **Behaviour:** The toolpath is serialised into the document (`CAM-TOOL-019`) so a reopened file shows the same paths. The format is versioned (`cam-toolpath-v1`), because `CAM-MACH-005` hashes it.
- **Refusals:** a file over `MAX_TOOLPATH_BYTES = 64 MiB` → `CamError::Limit`. A version above the supported one → `CamError::Unsupported("toolpath version {n}; this build reads {max}")`. An index out of range in the JSON → `CamError::Invalid` naming the index. A move with a non-finite coordinate → refused.
- **Tests:** `a_toolpath_round_trips`. `a_newer_version_is_refused`. `an_out_of_range_index_is_refused`. `hostile_toolpath_json_never_panics` — 200 mutations.
- **Acceptance:** `CAM-MACH-005`'s hash is over the output of `toolpath_to_json`, so the two agree by construction.
- **Risk:** the file grows with every path. Fallback: paths may be regenerated from operations (`CAM-PATH-004`) instead of stored; the stored form is a cache and its hash detects staleness.

### CAM-PATH-030 — Toolpath property tests over random solids
- **State:** READY
- **Depends on:** CAM-PATH-002, CAM-PATH-026
- **Files:** `crates/cam/tests/path_properties.rs` (owner Q)
- **Contract:** `paths_never_panic_on_random_solids`; `every_move_is_finite_after_planning`; `kinematic_validation_is_idempotent`; `an_edit_sequence_ending_in_the_original_path_is_identity`; `hostile_paths_never_panic`.
- **Behaviour:** Over the kernel's random solid generator, planning a strategy must return `Ok` or a named `Err` — never a panic — and every move in an `Ok` path is finite. The idempotence property catches a generator whose output depends on hash iteration order.
- **Refusals:** a strategy that panics or returns a non-finite move fails the test, and the fix is a refusal at the site.
- **Tests:** the five above plus `a_strategy_refuses_rather_than_producing_a_degenerate_path`.
- **Acceptance:** green, and re-run at every wave boundary by `QAT-001`.
- **Risk:** a strategy that is only correct on the test shapes. Fallback: the random generator is the kernel's own, so the shapes are the kernel's, not the test author's.

---

# CAM-SIM — simulation and **the validator** (22 tasks)

> **Nothing is posted without this epic.** `CAM-POST`'s acceptance criteria are written against
> the tasks here.

### CAM-SIM-001 — `Validation`: the value every artefact carries
- **State:** READY
- **Depends on:** CAM-TOOL-002
- **Files:** `crates/cam/src/sim/validation.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum Validation {
      Passed { checks: Vec<CheckName>, gouge_count: usize, residual_mm3: f64 },
      Failed { checks: Vec<(CheckName, String)> },
      Unvalidated { reason: String },
  }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum CheckName { Kinematics, Gouge, Collision, ResidualStock, FloorStock, RapidThroughStock, Units, FeedLimit, SpindleLimit }
  impl Validation { pub fn is_passed(&self) -> bool; pub fn may_be_posted(&self) -> bool { matches!(self, Validation::Passed { .. }) } }
  ```
- **Behaviour:** `may_be_posted()` is the single gate `CAM-POST-001` calls before writing a byte. `Unvalidated` exists so that "we did not check" is a **distinct state** from "we checked and it passed" — the distinction `SIM-VAL-015` makes for analysis results and that this domain needs more, because the failure mode is a crashed machine.
- **Refusals:** a `Validation::Failed` naming the failing check and its detail. A `Validation::Unvalidated` naming why it was not checked. There is no "passed with warnings" state: a warning that matters is a check, a warning that does not is dropped.
- **Tests:** `only_passed_may_be_posted`. `failed_names_the_check_and_the_detail`. `unvalidated_is_not_passed`. `validation_is_serialisable`. `the_check_list_is_the_one_the_validator_runs` — a test asserting every `CheckName` variant has a task.
- **Acceptance:** `Validation` is in every `Operation`, every `NcProgram` and every sheet.
- **Risk:** a check added without a task. Fallback: the last test fails on an unbacked variant.

### CAM-SIM-002 — The swept volume of one move (the primitive everything rests on)
- **State:** BLOCKED
- **Depends on:** CAM-TOOL-005, KER-CAM-001
- **Files:** `crates/cam/src/sim/sweep.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn swept_volume(mv: &Move, tool: &Tool, tol: f64) -> Result<Solid, CamError>;
  pub fn swept_volume_of(tp: &Toolpath, tool: &Tool, tol: f64) -> Result<Solid, CamError>;
  ```
- **Behaviour:** For a **planar** move, the swept volume of a flat end mill is exactly the Minkowski sum of the segment with the tool's disc, which `KER-CAM-001` computes as a region and `extrude` turns into a `Solid` — exact, not sampled. For an **arc** move, the swept region is the union of the swept volumes of the arc's chord segments, refined until the chord's deviation is within `tol`; the refinement count is reported. For a **non-planar** move (3D linear at an angle to the tool axis) the volume is the convex hull of the two end discs, again exact via a region.
- **Refusals:** `tol <= 0` or non-finite → `CamError::Invalid`. An arc needing more than `MAX_ARC_SEGMENTS = 512` chords within `tol` → `CamError::Limit` naming the count and the cap. A tool profile that cannot be sampled within `tol` → `CamError::Invalid`.
- **Tests:** `a_flat_move_sweeps_a_capsule_of_the_right_length` — asserts the volume equals `π·r²·L + (4/3)·π·r³` to `1e-9`. `an_arc_sweep_is_within_tol_of_the_analytic_annulus_sector`. `a_non_planar_move_sweeps_the_convex_hull_of_two_discs`. `a_zero_tolerance_is_refused`. `too_fine_an_arc_tolerance_is_refused`.
- **Acceptance:** the analytic volume formula is a test; everything downstream uses this.
- **Risk:** the two-disc convex hull for a non-planar move is a hull of discs, which is a hull of circles in 3D — not a region. Fallback: a non-planar move's sweep is sampled into chords and the error is reported; the *planar* case (which is 95 % of a 3-axis job) is exact.

### CAM-SIM-003 — Material removal: stock minus every sweep
- **State:** BLOCKED
- **Depends on:** CAM-SIM-002, KER-BOOL-001, KER-BOOL-002, KER-BOOL-003
- **Files:** `crates/cam/src/sim/removal.rs` (owner CAM)
- **Contract:** `pub fn remove(stock: &Solid, tp: &Toolpath, tool: &Tool, tol: f64) -> Result<RemovalResult, CamError>;` with `pub struct RemovalResult { pub remaining: Solid, pub removed_mm3: f64, pub ops: usize, pub per_move: Vec<f64> }`
- **Behaviour:** Accumulates `stock − Σ sweeps` with the kernel's boolean, in move order, so `per_move` says what each move removed. This is the simulator. It uses `mass_properties` for the volumes (the reference-point discipline in `mass.rs:33` makes it stable far from the origin) and cross-checks against a signed-tetrahedron sum over the remaining mesh — **two independent routes to the same number**, the `mass.rs` pattern.
- **Refusals:** a boolean refusal (a cracked shell, `KER-BOOL-002`) is `CamError::Kernel` naming the kernel's error, never a "removed 0". A removal past `MAX_SWEEP_SOLIDS = 200_000` primitives → `CamError::Limit`.
- **Tests:** `the_removed_volume_is_the_difference_of_the_two`. `removing_nothing_removes_nothing`. `a_boolean_defect_does_not_report_zero` — a fixture from `crates/solid/tests/property.rs` must produce `Err`, not a wrong number. `per_move_sums_to_the_total`. `removal_is_deterministic`.
- **Acceptance:** the two volume routes agree to `1e-9` on every fixture.
- **Risk:** the three open boolean defects make this wrong in the direction that **hides a gouge**. Fallback: this task is hard-blocked on `KER-BOOL-001..003`, which is the plan working as intended.
- **Risk:** a 10⁴-move toolpath is 10⁴ booleans. Fallback: `CAM-SIM-016`'s budget and the `MAX_SWEEP_SOLIDS` cap refuse rather than truncate.

### CAM-SIM-004 — Gouge detection: material removed that should have stayed
- **State:** BLOCKED
- **Depends on:** CAM-SIM-003, KER-MSH-001, KER-MSH-004
- **Files:** `crates/cam/src/sim/gouge.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn find_gouges(stock_removed: &Solid, model: &Solid, tol: f64) -> Result<Vec<Gouge>, CamError>;
  pub struct Gouge { pub at: Vec3, pub depth: f64, pub area_mm2: f64, pub face: Option<usize>, pub move_index: Option<usize> }
  ```
- **Behaviour:** `removed ∖ model` is the gouge. Each connected piece becomes a `Gouge` with its deepest point, its area and **the face of the model it came from** (needs `KER-MSH-001` to map the piece to a triangle and therefore to a face). Depth is the maximum `|signed_distance|` (needs `KER-MSH-004`), which is the number a machinist judges a cut by.
- **Refusals:** a piece smaller than `min_gouge_volume` (`1e-3 mm³`) is reported as a **count only** (it is below any machinist's feel) — reported, never hidden. A non-finite depth → the gouge is recorded with `depth = INFINITY` and `ok = false`, and the validation fails.
- **Tests:** `a_path_that_stays_outside_gouges_nothing`. `a_path_0_05_mm_inside_reports_0_05`. `the_gouge_names_the_face_it_cut`. `a_gouge_smaller_than_the_floor_is_a_count`. `gouges_are_deterministic`.
- **Acceptance:** **this task is the gouge check every `CAM-POST` task cites.** It fails loudly.
- **Risk:** a *legitimate* cut into the model's own air pocket reads as a gouge. Fallback: the comparison is against the model solid, so a pocket interior is not material and is not gouged — the test `a_path_that_stays_outside_gouges_nothing` covers the mirrored case.

### CAM-SIM-005 — Over-cut and under-cut, named apart
- **State:** BLOCKED
- **Depends on:** CAM-SIM-004
- **Files:** `crates/cam/src/sim/classify.rs` (owner CAM)
- **Contract:** `pub fn classify(removed: &Solid, model: &Solid) -> Result<(Vec<Gouge>, Vec<Residual>), CamError>;` with `pub struct Residual { pub at: Vec3, pub volume_mm3: f64, pub face: Option<usize> }`
- **Behaviour:** Two distinct verdicts: **over** (removed where material should remain — dangerous) and **under** (material left where none should be — scrap, not a crash). They are reported with different weights: an over-cut fails validation, an under-cut above `residual_tol` fails, a small under-cut is a warning in the report. A CAM product that reports only one of them is hiding half the answer.
- **Refusals:** an under-cut whose volume exceeds `residual_tol` (default 1 % of the removed volume) → validation fails. Both lists are returned together; neither is ever empty-by-default.
- **Tests:** `a_path_that_leaves_material_reports_an_under_cut`. `a_path_that_cuts_too_deep_reports_an_over_cut`. `a_small_under_cut_is_a_warning_not_a_failure`. `the_two_lists_are_both_present`.
- **Acceptance:** the report shows both numbers, always.
- **Risk:** residual tolerance is a policy. Fallback: it is an operation parameter printed in the sheet.

### CAM-SIM-006 — Floor stock: material left below the bottom of a pocket
- **State:** READY
- **Depends on:** CAM-SIM-003, CAM-PATH-003
- **Files:** `crates/cam/src/sim/floor.rs` (owner CAM)
- **Contract:** `pub fn check_floor(remaining: &Solid, op: &Operation, tol: f64) -> Result<FloorReport, CamError>;` with `pub struct FloorReport { pub min_mm: f64, pub required_mm: f64, pub ok: bool }`
- **Behaviour:** Measures the thinnest remaining web under every pocket and compares it with `stock_to_leave`. A floor pass that leaves less than the tool's own runout is the classic scrap cause.
- **Refusals:** `stock_to_leave < tool_runout` → `CamError::Invalid("the stock to leave {s} is under the tool runout {r}")` naming both. A floor that cannot be measured (the remaining solid is empty) → `Err`, not `ok = true`.
- **Tests:** `a_floor_at_the_nominal_depth_passes`. `a_floor_left_above_the_nominal_fails_with_both_numbers`. `stock_below_the_tool_runout_is_refused`. `an_empty_remaining_is_not_a_pass`.
- **Acceptance:** the floor check is in every pocket operation's validation.
- **Risk:** the runout figure is a tool property we do not model per flute. Fallback: it is a stated conservative constant per tool kind.

### CAM-SIM-007 — Holder collision
- **State:** READY
- **Depends on:** CAM-TOOL-006, CAM-SIM-003, KER-TOL-002
- **Files:** `crates/cam/src/sim/collision.rs` (owner CAM)
- **Contract:** `pub fn check_holder(tp: &Toolpath, tool: &Tool, holder: &ToolHolder, obstacles: &[Solid], tol: f64) -> Result<Vec<Collision>, CamError>;`
- **Behaviour:** Intersects the holder's swept envelope with every obstacle (the stock, the fixtures from `CAM-MACH-003`, the other tools in the magazine). A collision is a failure with the move index and the two bodies' names — the same shape as `KER-TOL-002`'s `Interference`.
- **Refusals:** a holder shorter than its flute → `CamError::Invalid`. An intersection volume below `min_collision_volume` (`1e-6 mm³`) is reported as a count, not a solid. An obstacle list over `MAX_OBSTACLES = 256` → `CamError::Limit`.
- **Tests:** `a_holder_that_clears_the_stock_passes`. `a_holder_that_touches_the_stock_fails_with_the_move`. `the_safe_height_prevents_the_collision` — the same path with a higher safe Z passes. `a_tiny_intersection_is_a_count`.
- **Acceptance:** every operation's validation includes the holder check.
- **Risk:** our holder envelope is a cylinder. Fallback: it is drawn **larger** than the real holder by a named margin, so a pass is conservative; a false pass would need the real holder to be *smaller* than our cylinder.

### CAM-SIM-008 — Dynamic collision: what we refuse, precisely
- **State:** READY
- **Depends on:** CAM-SIM-007
- **Files:** `crates/cam/src/sim/dynamic.rs` (owner CAM)
- **Contract:** `pub fn check_rapids(tp: &Toolpath, obstacles: &[Solid], tol: f64) -> Result<Vec<Collision>, CamError>;` and `pub fn multi_axis_collision(_) -> Result<(), CamError> { Err(CamError::Unsupported("dynamic collision needs a kinematic model of the machine, which is not written")) }`
- **Behaviour:** What we *can* check on a 3-axis machine is static and is checked: a rapid segment through the stock, a rapid through a fixture, a rapid below the safe Z. What we **cannot** check is a servo following error, a collision between two tools during a magazine change, or any 5-axis swept motion with a moving part — and each of those is refused by name rather than assumed absent.
- **Refusals:** a rapid whose segment intersects an obstacle → `CamError::Collision` with the move index. `multi_axis_collision` always refuses, naming the three missing models.
- **Tests:** `a_rapid_through_the_stock_is_refused`. `a_rapid_above_the_stock_passes`. `multi_axis_collision_is_a_named_refusal`. `dynamic_collision_does_not_claim_to_cover_servo_error`.
- **Acceptance:** the module doc lists what is checked and what is not, in two separate lists.
- **Risk:** "dynamic collision" in a marketing sense. Fallback: the refusal names the three models that are missing.

### CAM-SIM-009 — Machine limits pass (the kinematic validator)
- **State:** READY
- **Depends on:** CAM-PATH-002, CAM-TOOL-015
- **Files:** `crates/cam/src/sim/limits.rs` (owner CAM)
- **Contract:** `pub fn check_limits(tp: &Toolpath, m: &MachineModel) -> Result<Vec<CamError>, CamError>;`
- **Behaviour:** The validator's version of `CAM-PATH-002`, kept separate because it reports **all** violations per axis in a deterministic order and stamps `CheckName::Kinematics`. It is what `Validation::Passed { checks }` records.
- **Refusals:** any violation → the list, with the axis, the value and the limit. A rotary move violating the rate rule → `MachineRefusal` naming the axis.
- **Tests:** `a_clean_path_passes`. `every_axis_is_reported_in_axis_order`. `the_first_violation_is_stable`. `a_rate_violation_is_reported`.
- **Acceptance:** every `Validation::Passed` lists `Kinematics`.
- **Risk:** divergence from the generator's own check. Fallback: both call `MachineModel::within_limits`; the generator's wrapper adds the move index, the validator's adds the full list.

### CAM-SIM-010 — Time from the validated toolpath
- **State:** READY
- **Depends on:** CAM-PATH-027
- **Files:** `crates/cam/src/sim/time.rs` (owner CAM)
- **Contract:** `pub fn validated_time(tp: &Toolpath, m: &MachineModel, v: &Validation) -> Result<TimeEstimate, CamError>;`
- **Behaviour:** The same arithmetic as `CAM-PATH-027`, but it **refuses unless `v.is_passed()`** — so a time is only quoted for a path that passed. That is what makes the time in the sheet trustworthy, and what makes `CAM-MACH-008`'s wear accumulator honest.
- **Refusals:** `Validation::Failed` or `Unvalidated` → `CamError::Unvalidated { reason }` naming the check that failed.
- **Tests:** `a_failed_path_has_no_time`. `the_time_equals_the_estimate_for_a_passed_path`. `uncertainty_is_carried_through`.
- **Acceptance:** the sheet's time comes from here and nowhere else.
- **Risk:** a validated path's time is still an estimate. Fallback: `uncertainty` is printed beside it.

### CAM-SIM-011 — Fixture interference in the validator
- **State:** READY
- **Depends on:** CAM-MACH-003, CAM-SIM-007
- **Files:** `crates/cam/src/sim/fixtures.rs` (owner CAM)
- **Contract:** `pub fn check_fixtures(tp: &Toolpath, tool: &Tool, setup: &Setup) -> Result<Vec<Collision>, CamError>;`
- **Behaviour:** The holder and the cutter against the setup's fixture solids. Separate from `CAM-SIM-007` because the fixtures come from the setup, not from the machine, and the sheet reports them separately ("the tool hits the vise" is a different conversation from "the tool hits a clamp").
- **Refusals:** a fixture not in the machine's library → `CamError::Invalid` naming it. A collision → `CamError::Collision` naming the fixture.
- **Tests:** `a_path_that_clears_the_vise_passes`. `a_path_into_the_vise_is_refused_with_the_fixture`. `an_unknown_fixture_is_refused`.
- **Acceptance:** the sheet lists fixture collisions separately from stock collisions.
- **Risk:** a fixture's own geometry is a user entry. Fallback: the fixture's dimensions are printed on the sheet.

### CAM-SIM-012 — The known-answer suite: the validator proves itself first
- **State:** READY
- **Depends on:** CAM-SIM-002, CAM-SIM-003
- **Files:** `crates/cam/tests/validator_known_answers.rs` (owner Q)
- **Contract:** the file's test list: `a_flat_raster_removes_exactly_the_swept_area`; `a_pocket_of_known_area_is_fully_removed`; `a_contour_removes_the_wall_volume`; `a_half_depth_pass_leaves_half`; `a_gouge_of_known_depth_is_measured_exactly`.
- **Behaviour:** Five closed-form answers, computed by hand in the test's comment, asserted to a relative `1e-9`: a raster pass removes `width × depth × pass_length`; a pocket of area `A` and depth `h` removes `A·h`; a contour at radius `r` around a pocket of area `A` removes `A·h − (A − π(r−R)²)·h`; a half-depth pass removes half; a gouge of depth `d` measures `d`.
- **Refusals:** none — this is the gate. If any of the five fails, **no post ships**.
- **Tests:** the five above, each with the closed form written in the test.
- **Acceptance:** green, and referenced by name in `CAM-POST`'s acceptance criteria.
- **Risk:** a validator that agrees with itself and not with arithmetic. Fallback: the five answers are in the test source in full, so a reader can check them by hand.

### CAM-SIM-013 — The validator's public entry point
- **State:** BLOCKED
- **Depends on:** CAM-SIM-004, CAM-SIM-005, CAM-SIM-006, CAM-SIM-007, CAM-SIM-009
- **Files:** `crates/cam/src/sim/mod.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn validate(op: &Operation, ctx: &SimContext) -> Result<ValidationReport, CamError>;
  pub struct SimContext<'a> { pub machine: &'a MachineModel, pub tool: &'a Tool, pub holder: &'a ToolHolder, pub stock: &'a Solid, pub model: &'a Solid, pub fixtures: &'a [Fixture] }
  pub struct ValidationReport { pub validation: Validation, pub gouges: Vec<Gouge>, pub residuals: Vec<Residual>, pub floor: FloorReport, pub time: Option<TimeEstimate>, pub sim_s: f64 }
  ```
- **Behaviour:** One call runs every check, collects them into one `Validation`, and returns the detail. The order of the checks is fixed and is the order they appear in the report, so two runs produce the same text.
- **Refusals:** an internal refusal (`CamError::Kernel`, `Limit`) is **caught and turned into `Validation::Unvalidated { reason }`** — the report is still produced and says the check could not run. That is the difference between "the simulator said no" and "the simulator could not answer", and both block posting.
- **Tests:** `a_clean_operation_passes_every_check`. `an_unrunnable_check_yields_unvalidated_not_passed`. `the_report_order_is_fixed`. `validate_never_panics`.
- **Acceptance:** `CAM-POST-001` calls this and nothing else before writing.
- **Risk:** a check that fails to run is reported as unrun rather than failing loudly. Fallback: the report's `sim_s` and the `Unvalidated` reason make it impossible to miss, and `CAM-SIM-021` refuses to post it.

### CAM-SIM-014 — Validation over a whole program
- **State:** BLOCKED
- **Depends on:** CAM-SIM-013
- **Files:** `crates/cam/src/sim/program.rs` (owner CAM)
- **Contract:** `pub fn validate_program(jobs: &[Job], ctx: &ProgramContext) -> Result<ProgramValidation, CamError>;` with `pub struct ProgramValidation { pub operations: Vec<(String, Validation)>, pub order_ok: bool, pub may_run: bool }`
- **Behaviour:** Runs the per-operation validation in the run order, checks that each operation's stock is the previous one's **validated remainder** (so operation 2 does not plan against material operation 1 already removed), and produces the run-order verdict. This catches the mistake that a per-op validator cannot: a job where each operation is individually fine and the sequence is not.
- **Refusals:** a job list whose stock chain is broken → `Validation::Failed { checks: [StockChain] }` naming the two operations. A duplicated operation name → `CamError::Invalid`.
- **Tests:** `each_operation_is_validated_in_run_order`. `a_broken_stock_chain_fails_the_program`. `a_program_of_valid_operations_passes`. `the_run_order_is_in_the_report`.
- **Acceptance:** a program may be marked runnable only through this function.
- **Risk:** re-simulating every operation for a chain is `n` simulations. Fallback: the remainder from operation *k* is the stock for *k+1*, so it is one sequential simulation, not `n`.

### CAM-SIM-015 — Determinism of the validator
- **State:** READY
- **Depends on:** CAM-SIM-013
- **Files:** `crates/cam/tests/validation_determinism.rs` (owner Q)
- **Contract:** `two_validations_of_one_operation_are_identical`; `the_report_text_is_byte_identical_twice`; `the_order_of_gouges_is_stable`; `a_reordered_operation_list_does_not_change_a_verdict`.
- **Behaviour:** Two runs of the validator on the same inputs produce byte-identical reports. This is the `scene3.rs` determinism discipline (no `HashMap` iteration, sorted maps, index-ordered accumulations) applied to a domain where a non-deterministic verdict is a safety problem.
- **Refusals:** any nondeterminism is a test failure with the two texts; there is no "sort before comparing" escape.
- **Tests:** the four above, over 20 random operations.
- **Acceptance:** green, and the report's text is what the sheet prints.
- **Risk:** a `HashMap` sneaks into the simulation. Fallback: the determinism test catches it immediately, since two runs in one process would differ.

### CAM-SIM-016 — The validator's time budget, and paging
- **State:** READY
- **Depends on:** CAM-SIM-013
- **Files:** `crates/cam/src/sim/budget.rs` (owner CAM), `crates/cam/tests/sim_budget.rs` (owner Q)
- **Contract:** `pub const MAX_VALIDATION_SECONDS: f64 = 30.0;` and `pub fn validate_bounded(op: &Operation, ctx: &SimContext, max_s: f64) -> Result<ValidationReport, CamError>;`
- **Behaviour:** A validation that would exceed `max_s` **stops and returns `Unvalidated { reason: "the validation exceeded {max} s at move {i}" }`**, naming the move it reached. It never returns a partial `Passed`. The budget is a wall-clock check between moves; it is the one place CAM reads a clock.
- **Refusals:** over budget → `Unvalidated`, never `Passed`. A `max_s <= 0` → `CamError::Invalid`.
- **Tests:** `a_validation_over_the_budget_is_unvalidated_not_passed`. `the_budget_is_reported_with_the_move_it_reached`. `a_small_operation_validates_within_the_budget`. `a_zero_budget_is_refused`.
- **Acceptance:** no validation can hang the UI; over-budget is a normal, reportable outcome.
- **Risk:** a slow machine fails validations that a fast one passes. Fallback: `max_s` is a parameter, the default is generous, and the report states the time it used (`sim_s`).

### CAM-SIM-017 — Hostile toolpaths into the validator
- **State:** READY
- **Depends on:** CAM-SIM-013
- **Files:** `crates/cam/tests/hostile_toolpaths.rs` (owner Q)
- **Contract:** `hostile_toolpaths_never_panic`; `a_zero_radius_arc_is_refused_by_the_validator`; `a_nan_feed_is_refused`; `a_move_below_the_stock_is_a_gouge_not_a_crash`; `an_empty_toolpath_is_an_empty_removal`.
- **Behaviour:** 200 mutations of a valid toolpath, plus the specific adversarial shapes: an arc with a centre on the end point, a feed of `NaN`, a move 10 m below the stock, a toolpath of 10⁶ identical moves (must hit `MAX_MOVES` or finish, never hang).
- **Refusals:** every case returns `Ok` with a verdict or a named `Err`.
- **Tests:** the five above.
- **Acceptance:** green, and re-run by `QAT-003`'s fuzz sweep.
- **Risk:** a hostile path that produces a gigantic intermediate solid. Fallback: `MAX_SWEEP_SOLIDS` refuses before the allocation, with the count in the message.

### CAM-SIM-018 — The G-code validator: parse back, re-check every move
- **State:** BLOCKED
- **Depends on:** CAM-SIM-013, CAM-POST-006
- **Files:** `crates/cam/src/sim/gcode_check.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn validate_gcode(text: &str, m: &MachineModel, tol: f64) -> Result<ProgramValidation, CamError>;
  pub struct ParsedProgram { pub units: CamUnits, pub blocks: Vec<GBlock>, pub tools: Vec<u8>, pub cycles: Vec<Cycle> }
  ```
- **Behaviour:** The reader parses the program into blocks, replays the modal state (G0/G1/G2/G3, feed, spindle, tool, offset), converts every endpoint to work coordinates and re-runs **the same checks the toolpath validator ran**: limits, feed, spindle, units, rapid-through-stock. **This is the gate for every post**, because it checks the *output*, not the intent.
- **Refusals:** a word the reader does not implement → `CamError::PostRefusal` naming the word and the block number. A units disagreement with the machine → `CamError::Invalid` naming both. A program with no end (`M30`/`M2` missing) → `CamError::PostRefusal("the program has no end")`. A block with an unknown axis letter → named.
- **Tests:** `a_valid_program_validates`. `a_program_that_exceeds_a_limit_is_caught_by_the_reader`. `a_word_the_reader_does_not_know_names_itself_and_its_block`. `a_missing_program_end_is_refused`. `inch_in_a_metric_machine_is_refused`.
- **Acceptance:** **no dialect task is done without this passing**, which is why every one of them lists it in *Depends on*.
- **Risk:** the reader and the writer share the author's misunderstanding. Fallback: `CAM-POST-009`'s independent simulator, and `CAM-POST-022`'s differential test between dialects.

### CAM-SIM-019 — Block-level parsing with a real modal machine
- **State:** BLOCKED
- **Depends on:** CAM-POST-006
- **Files:** `crates/cam/src/gcode/parse.rs` (owner CAM)
- **Contract:** `pub struct GBlock { pub line: usize, pub words: Vec<GWord>, pub comment: String }` and `pub enum GWord { G(usize), M(usize), X(f64), Y(f64), Z(f64), I(f64), J(f64), K(f64), R(f64), F(f64), S(f64), T(u8), N(u32), Paren(String) }` and `pub fn parse(text: &str) -> Result<Vec<GBlock>, CamError>;`
- **Behaviour:** A real reader, not a regex: it tracks line numbers, distinguishes a comment from a word, rejects a malformed number rather than defaulting it to 0, and preserves the block for error messages. Modal state lives in `CAM-SIM-018`'s replayer, not in the parser, so the parser stays a pure function of its input.
- **Refusals:** an unparsable number → `CamError::PostRefusal` naming the line and the text. An unterminated comment → the same. A block over `MAX_LINE_CHARS = 4_096` → `CamError::Limit`. A program over `MAX_PROGRAM_BLOCKS = 2_000_000` → `CamError::Limit`.
- **Tests:** `a_block_is_parsed_into_its_words`. `a_comment_is_not_a_word`. `a_malformed_number_is_refused_with_its_line`. `an_unterminated_comment_is_refused`. `a_very_long_line_is_refused`. `parsing_is_deterministic`.
- **Acceptance:** one parser serves the validator, the dry run and the diff tool.
- **Risk:** a dialect with words we do not model. Fallback: the refusal names the word; the list of modelled words is in the module doc.

### CAM-SIM-020 — The dry-run report a user can read
- **State:** READY
- **Depends on:** CAM-SIM-013, CAM-SIM-014
- **Files:** `crates/cam/src/sim/report.rs` (owner CAM)
- **Contract:** `pub fn report_text(p: &ProgramValidation) -> Result<String, CamError>;` — plain text, one line per check per operation, and a final verdict line.
- **Behaviour:** A table: operation, tool, cut time, removed volume, gouge count, residual volume, floor thickness, verdict. The last line is `PASS`, `FAIL` or `DO NOT RUN` and there is no fourth possibility.
- **Refusals:** an `Unvalidated` operation prints `DO NOT RUN` in the verdict column and the reason in the row. Never a `PASS` for an unchecked operation.
- **Tests:** `the_report_has_one_row_per_operation`. `the_verdict_line_is_one_of_three_strings`. `an_unvalidated_operation_says_do_not_run`. `the_report_is_deterministic`.
- **Acceptance:** the report and the setup sheet (`CAM-MACH-010`) show the same numbers.
- **Risk:** two reports that disagree. Fallback: both call `ProgramValidation` and neither formats a number of its own.

### CAM-SIM-021 — The `DO NOT RUN` watermark and the posting gate
- **State:** READY
- **Depends on:** CAM-SIM-001, CAM-SIM-020
- **Files:** `crates/cam/src/sim/gate.rs` (owner CAM)
- **Contract:** `pub fn posting_gate(v: &Validation) -> Result<(), CamError>;` — `Err(CamError::Unvalidated { reason })` for `Unvalidated` and for `Failed`, with the failing checks named; `Ok(())` only for `Passed`.
- **Behaviour:** The one function `CAM-POST-001` calls first. It is deliberately tiny and deliberately total: there is no path from a `Toolpath` to a string of G-code that does not pass through it, and the module doc says so in the first paragraph.
- **Refusals:** `Failed` → `Unvalidated { reason: "failed: Gouge (2), FloorStock" }`; `Unvalidated` → its own reason; `Passed` → `Ok(())`.
- **Tests:** `passed_may_be_posted`. `failed_may_not`. `unvalidated_may_not`. `the_reason_names_the_failing_checks`. `the_gate_is_the_only_way_to_post` — a source scan over `crates/cam/src/post/` asserting every writer calls `posting_gate`.
- **Acceptance:** the source-scan test fails if a post writer is added without the gate.
- **Risk:** an internal caller bypassing the gate. Fallback: the source scan, run in CI.

### CAM-SIM-022 — Validator property tests
- **State:** READY
- **Depends on:** CAM-SIM-013
- **Files:** `crates/cam/tests/validator_properties.rs` (owner Q)
- **Contract:** `the_removed_volume_never_exceeds_the_stock_volume`; `removal_is_monotone_in_the_path_length`; `adding_a_move_never_adds_material`; `gouge_count_is_zero_for_a_path_outside_the_model`; `validator_never_panics_on_random_paths`.
- **Behaviour:** Monotonicity is the load-bearing property: a longer toolpath can only remove more material, never add it. It catches a boolean that flips (`KER-BOOL-003`'s defect class) far more sharply than a single fixture.
- **Refusals:** a violation of monotonicity is a test failure naming the two paths.
- **Tests:** the five above over 20 random paths.
- **Acceptance:** green, re-run at each wave boundary.
- **Risk:** the kernel's boolean defects make monotonicity fail for the wrong reason. Fallback: it is hard-blocked behind `KER-BOOL-001..003` through `CAM-SIM-003`.

---

# CAM-POST — post-processors (24 tasks)

> **The rule of this epic: no dialect task ships without both gates.** `CAM-POST-007` (our reader
> accepts what our writer wrote, and every move matches) and `CAM-POST-009` (an independent
> simulator accepts it). Each dialect below names both in *Depends on*, and its acceptance
> criterion requires the report file from `CAM-POST-009` to name that dialect.

### CAM-POST-001 — The post engine: data-driven, one code path
- **State:** READY
- **Depends on:** CAM-SIM-021, CAM-PATH-001
- **Files:** `crates/cam/src/post/mod.rs` (owner CAM)
- **Contract:**
  ```rust
  pub fn post(job: &Job, def: &PostDef) -> Result<String, CamError>;
  pub fn post_moves(tp: &Toolpath, def: &PostDef) -> Result<String, CamError>;
  ```
- **Behaviour:** One engine, one `PostDef`, eight dialects as data. The engine walks the toolpath, calls `posting_gate` **before writing anything**, maintains the modal state, emits words per the definition and flushes. The doc's first line says: nothing is written until the validation has passed, because there is no way to un-write a program that has already left the process.
- **Refusals:** `posting_gate` failing → the whole post refuses and returns `Err(CamError::Unvalidated { .. })` with **no partial output**. A dialect missing a required field → `CamError::Invalid` naming the field. A move the dialect cannot express → `CamError::PostRefusal` naming the move index.
- **Tests:** `posting_nothing_produces_the_header_only`. `a_failed_validation_produces_no_output`. `the_output_is_byte_identical_twice_with_provenance_off`. `post_never_panics`.
- **Acceptance:** one engine, eight definitions, no per-dialect code.
- **Risk:** a data-driven post that cannot express a real dialect. Fallback: the definition carries an optional `linearise_cycles: bool` and the read-back test proves the linearisation is equivalent.

### CAM-POST-002 — The post definition format, and a validator for it
- **State:** READY
- **Depends on:** CAM-POST-001
- **Files:** `crates/cam/src/post/def.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct PostDef {
      pub name: String, pub version: String,
      pub units: CamUnits, pub decimals: usize,
      pub line_start: &'static str, pub line_end: &'static str,
      pub comment: &'static str,
      pub program_start: Vec<String>, pub program_end: Vec<String>,
      pub words: WordMap,           // motion, feed, spindle, tool, offset, cycle words per event
      pub arc: ArcStyle,            // Ijk | Radius
      pub cycles: CycleStyle,       // Native | Linearise
      pub tool_change: ToolChangeStyle,
      pub linearise_cycles: bool,
  }
  pub fn validate_def(d: &PostDef) -> Result<(), CamError>;
  ```
- **Behaviour:** The whole dialect is this struct: header, footer, word letters, arc style, cycle style, tool change style, decimals. A user adds a dialect by writing this, not by writing Rust. `decimals` matters: a controller with 3 decimals rounds 0.0005 mm to nothing, so the *rounding* is a dialect fact and the read-back test measures it.
- **Refusals:** `decimals > 6` → `CamError::Invalid` (a controller that takes 6 decimals is not a real controller and the rounding is pointless). A missing word letter for a motion → `CamError::Invalid` naming the event. An empty `program_end` → `CamError::Invalid("a program with no end will run off the end of the file")`.
- **Tests:** `every_builtin_definition_validates`. `a_definition_without_a_footer_is_refused`. `seven_decimals_is_refused`. `a_definition_round_trips_through_json`.
- **Acceptance:** all eight dialects are `PostDef` values, and `validate_def` is the only place a dialect is judged.
- **Risk:** a dialect that needs conditional logic no data can express. Fallback: the `cycles: Linearise` mode moves the logic into the neutral toolpath, where it is testable.

### CAM-POST-003 — The internal dialect
- **State:** READY
- **Depends on:** CAM-POST-001, CAM-POST-002
- **Files:** `crates/cam/src/post/internal.rs` (owner CAM)
- **Contract:** `pub fn internal_def() -> PostDef;` — dialect `cadcraft-internal`, metric, `G0/G1/G2/G3`, `I/J/K` arcs, native cycles off (`Linearise`), explicit `G21`, `T`/`M6`, `G54`, `M30`.
- **Behaviour:** The dialect the validator's own tests use: the one whose output our reader definitely understands, so a failure in a real dialect can be attributed to the dialect rather than to the engine. Every other dialect is written as a **delta** from this one and tested against it (`CAM-POST-022`).
- **Refusals:** same as the engine; the internal dialect has no refusal of its own.
- **Tests:** `the_internal_dialect_posts_a_cube`. `it_round_trips_through_the_reader`. `every_move_in_the_input_appears_in_the_output`. `the_output_is_reproducible`.
- **Acceptance:** green before any other dialect starts.
- **Risk:** none; this is the reference.
- **Note:** this is the dialect a user can run on a simulator and read by eye, which is why it is worth writing first.

### CAM-POST-004 — Header and footer, with provenance off by default
- **State:** READY
- **Depends on:** CAM-POST-003
- **Files:** `crates/cam/src/post/header.rs` (owner CAM)
- **Contract:** `pub fn header(job: &Job, d: &PostDef, provenance: bool) -> Vec<String>;` and `pub fn footer(job: &Job, d: &PostDef) -> Vec<String>;`
- **Behaviour:** The header carries what a controller needs (units, offset, spindle direction) and what a person needs (machine, post + version, toolpath hash, and the feeds-and-sources lines from `CAM-TOOL-021` when `provenance` is on). The footer always carries an end: `M30`, `%`, or whatever the dialect says — **a program with no end is refused by `validate_def`**.
- **Refusals:** a non-monotonic clock → `CamError::Invalid("the clock went backwards")` rather than a 1970 date.
- **Tests:** `two_runs_without_provenance_are_byte_identical`. `the_header_names_the_machine_and_the_post`. `the_header_states_the_units`. `the_footer_ends_the_program`. `a_backwards_clock_is_refused`.
- **Acceptance:** the reproducibility property of `DAT-012` holds for a G-code file.
- **Risk:** a long header annoys a shop. Fallback: `provenance` is a flag; the mandatory part is three lines.

### CAM-POST-005 — Line numbering, comments, and the unknown-word refusal
- **State:** READY
- **Depends on:** CAM-POST-002
- **Files:** `crates/cam/src/post/format.rs` (owner CAM)
- **Contract:** `pub fn format_number(v: f64, decimals: usize) -> String;` and `pub fn line_no(n: u32, enabled: bool) -> String;` and `pub fn comment(d: &PostDef, text: &str) -> String;`
- **Behaviour:** Number formatting is **fixed-decimal with trailing zeros trimmed to the dialect's `decimals`** — never scientific notation, never `f64::to_string` (which prints `1e-7` and no controller parses it). A comment containing a character the dialect's comment syntax cannot carry is truncated at the syntax, not escaped.
- **Refusals:** a value whose magnitude overflows the dialect's word size at `decimals` (e.g. `1e12` at 3 decimals) → `CamError::Invalid("the value {v} does not fit the controller's {d}-decimal word")` naming both. A `NaN` → `CamError::Invalid`, never `0`.
- **Tests:** `a_value_prints_in_fixed_decimals`. `no_line_ever_contains_scientific_notation`. `a_comment_is_prefixed_per_the_dialect`. `a_value_too_large_for_the_decimals_is_refused`. `a_nan_is_refused_not_zero`.
- **Acceptance:** no scientific notation can appear in any output; a test scans every fixture.
- **Risk:** a machine with 4 decimals and a coordinate of 1e7 needs 11 digits plus the point — some controllers cap the line at 80 characters. Fallback: the overflow refusal names the limit, and the user changes the machine's units or origin.

### CAM-POST-006 — The G-code reader
- **State:** READY
- **Depends on:** CAM-POST-002, CAM-SIM-019
- **Files:** `crates/cam/src/gcode/mod.rs` (owner CAM)
- **Contract:** `pub fn read(text: &str) -> Result<ParsedProgram, CamError>;` and `pub fn read_moves(p: &ParsedProgram) -> Result<Toolpath, CamError>;`
- **Behaviour:** Reads a program, replays its modal state and reconstructs the `Toolpath` it came from. This is the **first gate** for every dialect: our own output must come back as the same moves. It is also what lets a user import a program someone else wrote — and the same checks apply to that.
- **Refusals:** a word not in the modelled list → `CamError::PostRefusal` naming the word and the block. A `G20` in a program whose post is metric → `CamError::Invalid` naming both. A move with no preceding feed → `CamError::PostRefusal("move {n} has no feed")`. A cycle the reader does not model → named.
- **Tests:** `a_program_round_trips_through_the_reader`. `the_reconstructed_toolpath_matches_the_input_to_1e-6`. `an_inch_program_in_a_metric_machine_is_refused`. `an_unmodelled_word_names_itself`. `the_reader_never_panics` — 200 mutations.
- **Acceptance:** `post_moves(tp) → read → read_moves` gives `tp` for every fixture and every dialect.
- **Risk:** the reader is our reading of the dialect, so a dialect we misread is self-consistently wrong. Fallback: `CAM-POST-009`.

### CAM-POST-007 — **Gate 1**: our own output re-validated move by move
- **State:** READY
- **Depends on:** CAM-POST-006, CAM-SIM-018
- **Files:** `crates/cam/tests/post_roundtrip.rs` (owner Q)
- **Contract:** `the_output_of_every_dialect_round_trips`; `every_move_survives_within_the_decimals`; `a_dialect_that_drops_a_move_is_caught`; `a_cyclic_move_linearises_to_the_same_volume`.
- **Behaviour:** For each of the eight dialects: post a fixture operation, read it back, and assert the reconstructed `Toolpath` matches the input **move for move**, with distances within the dialect's own `decimals` and a chord tolerance for arcs. A dialect that drops a move, reverses a direction, or loses a tool number fails here.
- **Refusals:** a mismatch → the test names the dialect, the move index, the expected and the actual coordinate.
- **Tests:** the four above, over the fixtures in `crates/cam/tests/data/post/`.
- **Acceptance:** **this is the first gate. No dialect task may be accepted without it green.**
- **Risk:** a dialect that is correct but whose arcs differ by more than the chord tolerance. Fallback: the tolerance is derived from the arc's radius and the decimals, not a constant.

### CAM-POST-008 — Units at the machine boundary, converted once
- **State:** READY
- **Depends on:** CAM-POST-003, CAM-TOOL-003
- **Files:** `crates/cam/src/post/units.rs` (owner CAM)
- **Contract:** `pub fn apply_units(v: f64, from: CamUnits, to: CamUnits) -> Result<f64, CamError>;`
- **Behaviour:** The **only** conversion in the whole domain: millimetres in, the machine's unit out, once, at the last step before a word is written. The header carries `G20`/`G21` to match.
- **Refusals:** a conversion that overflows the dialect's decimals → `CAM-POST-005`'s refusal. A non-finite value → `CamError::Invalid`.
- **Tests:** `an_inch_machine_gets_inch_words_and_g20`. `the_conversion_happens_once` — a source scan asserting `apply_units` has exactly one caller in `post/`. `the_round_trip_through_inches_preserves_the_position_within_1e-6`. `a_machine_with_no_supported_unit_is_refused`.
- **Acceptance:** the source scan is the acceptance; it fails if a second conversion appears.
- **Risk:** converting in the generator by mistake. Fallback: `CAM-TOOL-003`'s `generators_never_call_inch` test.

### CAM-POST-009 — **Gate 2**: the independent simulator probe and its report
- **State:** READY
- **Depends on:** CAM-POST-006
- **Files:** `crates/cam/tests/post_external.rs`, `crates/cam/tests/data/post-simulator-report.md` (owner Q)
- **Contract:** as `DAT-014`'s `ProbeReport`: `pub fn probe(bytes: &[u8], reader: &dyn Fn(&[u8]) -> Result<ProbeReport, String>) -> Result<(), String>;` with `pub struct ProbeReport { pub accepted: bool, pub lines: usize, pub moves: usize, pub max_feed: Option<f64>, pub max_rpm: Option<f64>, pub errors: Vec<String>, pub warnings: Vec<String> }`
- **Behaviour:** Shells out to an independent simulator named by `CADCRAFT_CAM_SIM` (a LinuxCNC/GraphicalGCode-class tool), compares the block and move counts, the maximum feed and spindle speed, and prints the simulator's own warnings. The checked-in report names the simulator, its version, the exact command, the fixture list and a verdict per dialect.
- **Refusals:** no simulator configured → the test is `#[ignore]`d and **says so**; it does not pass silently. A non-zero exit → fail with stderr. A move count mismatch → fail with both numbers. **A report that was not produced by a run must not be written.**
- **Tests:** `the_probe_requires_a_simulator` — with no environment variable it names the variable. `a_move_count_mismatch_fails_with_both_numbers`. `the_report_records_the_simulator_version`.
- **Acceptance:** the report names at least one real independent simulator, its version, and a pass count equal to the dialect count. **Until it exists, every dialect below is `CODE WRITTEN` and `docs/roadmap.md`'s CAM row says so.**
- **Risk:** the dev machine may have no simulator. Fallback: the harness is written, documented and ignored; the first agent with a simulator runs it and writes the report. This is the same rule as `DAT-014` and it is not negotiable.

### CAM-POST-010 — Dialect: generic 3-axis router (LinuxCNC / Shapeoko class)
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/generic3.rs` (owner CAM), fixture + report row
- **Contract:** `pub fn generic3_def() -> PostDef;` — metric, `G0/G1/G2/G3`, `G20/G21`, `G54`, `M3/M5`, `T…M6`, `I/J` arcs, `G94` feed per minute, `M30`, `( )` comments.
- **Behaviour:** The most common open dialect, written first because it is the one a hobby machine takes and the one a simulator supports. Everything it can express is expressed natively; cycles linearise.
- **Refusals:** inherited from the engine. The dialect itself refuses nothing extra.
- **Tests:** `the_generic3_output_round_trips`. `the_simulator_report_has_a_row_for_generic3`. `a_feed_appears_once_per_change`. `the_end_of_program_word_is_present`.
- **Acceptance:** Gate 1 and Gate 2 green; the report names this dialect as accepted.
- **Risk:** a real LinuxCNC setup differs in arc-centre conventions on rotary machines. Fallback: the position words and units are checked; a 3-axis job is unaffected.

### CAM-POST-011 — Dialect: GRBL-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/grbl.rs` (owner CAM)
- **Contract:** `pub fn grbl_def() -> PostDef;` — as `generic3` but: **no tool change word**, spindle by `S` with a documented maximum, no `M6`, and `$` settings are not emitted.
- **Behaviour:** The dialect where our refusal is *most* important: a machine that cannot change tools has no place for a tool number, and the operation ordering must collapse to one tool per program. `CAM-PATH-025`'s ordering refuses a multi-tool job for this machine.
- **Refusals:** a job needing more than one tool → `CamError::Unsupported("this machine has no tool changer; split the job")` naming the machine.
- **Tests:** `the_grbl_output_has_no_tool_change`. `a_multi_tool_job_is_refused_for_this_machine`. `it_round_trips`. `the_simulator_report_has_a_row_for_grbl`.
- **Acceptance:** both gates green.
- **Risk:** GRBL's real limits live in `$` settings we do not model. Fallback: the machine model carries the spindle maximum and the feed maximum, and the sheet prints them.

### CAM-POST-012 — Dialect: Fanuc 0i-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/fanuc.rs` (owner CAM)
- **Contract:** `pub fn fanuc_def() -> PostDef;` — `G0/G1/G2/G3` with `I/J/K`, `G54..G59`, `M3/M4/M5`, `T…M6`, `S…M3`, `G94`, `M30`, `/` block delete markers, `%` markers.
- **Behaviour:** The most common industrial dialect. `M4` (reverse spindle) is emitted for a left-hand tool, and the machine model's spindle direction decides which. Block-delete markers (`/`) go on the safety lines so an operator can delete them.
- **Refusals:** a left-hand tool on a machine with no `M4` → `CamError::Unsupported` naming the machine.
- **Tests:** `a_left_hand_tool_posts_m4`. `a_right_hand_tool_posts_m3`. `the_block_delete_markers_are_present`. `it_round_trips`. `the_simulator_report_has_a_row_for_fanuc`.
- **Acceptance:** both gates green.
- **Risk:** Fanuc's real dialect varies by control (0i, 0i-F, 31i). Fallback: `PostDef` is data, so a variant is a new definition with its own name, and the report records which control the simulator emulated.

### CAM-POST-013 — Dialect: Haas-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/haas.rs` (owner CAM)
- **Contract:** `pub fn haas_def() -> PostDef;` — as Fanuc with `T…M6`, `G43` tool length offset, `H` number in the `G43` block, and `G96/G97` constant surface speed.
- **Behaviour:** `G96` (constant surface speed) is the interesting part: it means the **post must not compute an rpm at all** for a facing operation, and the feed-per-tooth logic changes. The dialect records `spindle_mode: ConstantRpm | ConstantSurface` in `PostDef` and the speeds task honours it.
- **Refusals:** `G96` with a `T` word and no `H` → refused by the engine's word check, naming the block.
- **Tests:** `constant_surface_speed_suppresses_the_rpm`. `a_tool_length_offset_is_emitted_with_its_h_number`. `it_round_trips`. `the_simulator_report_has_a_row_for_haas`.
- **Acceptance:** both gates green.
- **Risk:** `G96` with our per-move feed table is a different feed model. Fallback: with `ConstantSurface` the post emits a single feed per operation and `CAM-PATH-028`'s table is bypassed, and the sheet says so.

### CAM-POST-014 — Dialect: Siemens 802/828-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/siemens.rs` (owner CAM)
- **Contract:** `pub fn siemens_def() -> PostDef;` — `%_N_...` program header, `_` address prefix, `G1/G2/G3/G70`, `G17/G18/G19` plane selection, `T…M6`, `S…M3/M6`, `M30`, `;` comments.
- **Behaviour:** A dialect whose **program structure** differs (numbered blocks, `_` prefixes, a mandatory header block) more than its words do, which is why `PostDef` carries the header as strings. Plane selection matters for arc centres: `G17`/`G18`/`G19` changes what `I/J/K` mean, so the post must emit the plane word with the correct coordinates.
- **Refusals:** an arc in a plane the definition does not declare → `CamError::PostRefusal`.
- **Tests:** `the_header_block_is_emitted`. `the_plane_word_is_emitted_before_the_first_arc`. `addresses_carry_the_underscore_prefix`. `it_round_trips`. `the_simulator_report_has_a_row_for_siemens`.
- **Acceptance:** both gates green.
- **Risk:** a real 828 has a different cycle syntax. Fallback: `cycles: Linearise` is the default for this dialect, so no cycle syntax is assumed.

### CAM-POST-015 — Dialect: Heidenhain TNC-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/heidenhain.rs` (owner CAM)
- **Contract:** `pub fn heidenhain_def() -> PostDef;` — function names (`FN1: FN10:`), plain-language words (`FN0: GOTO`, `FN26: CYCL`), `%` header, `FN30: END`, `;` comments.
- **Behaviour:** The dialect that most needs the **linearised** path: its cycles are named program constructs that a generic linearisation would express as hundreds of blocks, and its reader in any simulator is the least likely to exist. So this dialect is written with `cycles: Linearise` and its own acceptance is a *volume and time* comparison against the internal dialect rather than a move-for-move comparison.
- **Refusals:** a cycle that the lineariser cannot express → `CamError::PostRefusal` naming it.
- **Tests:** `the_end_word_is_em30`. `functions_carry_the_fn_prefix`. `the_linearised_cycles_have_the_same_removed_volume_as_the_internal_dialect`. `it_round_trips`. `the_simulator_report_has_a_row_for_heidenhain`.
- **Acceptance:** both gates green, **plus** the volume/time comparison, because a move-for-move comparison is not meaningful for a dialect with named cycles.
- **Risk:** the least external validation available. Fallback: the volume and time comparison is arithmetic we own and is written into the test.

### CAM-POST-016 — Dialect: Tormach / PathPilot-class
- **State:** BLOCKED
- **Depends on:** CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/tormach.rs` (owner CAM)
- **Contract:** `pub fn tormach_def() -> PostDef;` — a Fanuc-like dialect with a mandatory tool-change preamble and a per-tool header block.
- **Behaviour:** Its distinguishing feature is that the program is a **sequence of per-tool subprograms** rather than one linear stream, which exercises `CAM-POST-021`'s subprogram model.
- **Refusals:** a job with more subprograms than the control's limit (`MAX_SUBPROGRAMS = 999`) → `CamError::Limit`.
- **Tests:** `each_tool_gets_a_subprogram`. `the_subprogram_count_limit_is_refused`. `it_round_trips`. `the_simulator_report_has_a_row_for_tormach`.
- **Acceptance:** both gates green.
- **Risk:** an unknown exact variant. Fallback: the report records the variant the simulator emulated.

### CAM-POST-017 — Arcs: centre words or radius word, and the half-turn rule
- **State:** READY
- **Depends on:** CAM-POST-002, CAM-PATH-019
- **Files:** `crates/cam/src/post/arcs.rs` (owner CAM)
- **Contract:** `pub fn arc_words(mv: &Move, style: ArcStyle) -> Result<Vec<GWord>, CamError>;`
- **Behaviour:** `Ijk` emits signed offsets from the arc's start to the centre; `Radius` emits `R`, choosing the sign for the minor arc. **A sweep above half a turn cannot be expressed as `R`** and is refused when the dialect is `Radius` — the refusal is what stops a controller from cutting the long way round.
- **Refusals:** a sweep ≥ 180° with `ArcStyle::Radius` → `CamError::PostRefusal("an R word cannot express a {deg}° arc")` naming the degree count. An `I/J/K` centre within `1e-9` of the start → `CamError::Invalid`. A non-finite centre → refused.
- **Tests:** `the_ijk_words_are_signed_offsets_from_the_start`. `the_radius_sign_follows_the_minor_arc`. `an_arc_over_half_a_turn_with_r_is_refused_with_its_degrees`. `a_full_circle_is_expressed_as_two_arcs`. `the_two_styles_describe_the_same_arc`.
- **Acceptance:** the two styles are tested against each other, so a sign error cannot survive.
- **Risk:** a controller's `R` sign convention differs. Fallback: the report records the convention used per dialect and the round-trip test proves ours is self-consistent.

### CAM-POST-018 — Canned cycles, and what linearisation must preserve
- **State:** READY
- **Depends on:** CAM-POST-002, CAM-PATH-013
- **Files:** `crates/cam/src/post/cycles.rs` (owner CAM)
- **Contract:** `pub fn linearise(cycle: &Cycle, at: Vec3, tool: &Tool, depth: f64) -> Result<Vec<Move>, CamError>;` and `pub fn native_cycle(c: &Cycle, style: &CycleStyle, m: &MachineModel) -> Result<Vec<String>, CamError>;`
- **Behaviour:** `Cycle::Peck` and `Cycle::Tapping` are *semantics*, not words. A dialect with `Native` emits its own spelling; a dialect with `Linearise` expands the cycle into feed and retract moves. **The acceptance test is that both produce the same removed volume and the same cut time**, because that is what a cycle is.
- **Refusals:** a `Tapping` cycle whose pitch does not match the machine's thread table (when one is given) → `CamError::PostRefusal` naming the pitch and the table entry. A `Linearise` of a peck that exceeds `MAX_MOVES` → `CamError::Limit`.
- **Tests:** `a_linearised_peck_removes_the_same_volume_as_the_native_cycle`. `a_linearised_peck_has_the_same_cut_time`. `a_tapping_pitch_mismatch_is_refused`. `a_native_cycle_uses_the_dialect_words`.
- **Acceptance:** the volume and time comparison for both dialects.
- **Risk:** a controller's canned cycle has an optional G98/G99 return-mode that changes the retract. Fallback: the cycle type carries the return mode and `PostDef` declares which word spells it.

### CAM-POST-019 — Tool change: what each dialect can spell
- **State:** READY
- **Depends on:** CAM-POST-016, CAM-TOOL-016
- **Files:** `crates/cam/src/post/toolchange.rs` (owner CAM)
- **Contract:** `pub fn tool_change_words(plan: &ToolChangePlan, d: &PostDef) -> Result<Vec<String>, CamError>;`
- **Behaviour:** The plan from `CAM-TOOL-016` is data; the dialect spells it: `T3 M6` for Fanuc, a tool change with a pocket move for a magazine, and for `Manual` a dwell or a message word. A dialect that cannot express the machine's change model **refuses at post time**, naming what it can express.
- **Refusals:** a magazine machine posted with a dialect whose `tool_change` is `Manual` → `CamError::PostRefusal("this dialect cannot address a magazine; it would change the tool at the wrong place")`. A tool number above 99 with a 2-digit `T` word → `CamError::PostRefusal`.
- **Tests:** `the_tool_change_is_emitted_after_the_safe_height`. `a_magazine_machine_refuses_a_manual_dialect`. `a_tool_number_above_the_word_capacity_is_refused`. `the_change_appears_once_per_tool`.
- **Acceptance:** every dialect's tool change comes from the plan, never from the generator.
- **Risk:** a controller that changes the tool with the spindle running. Fallback: `CAM-TOOL-016`'s plan includes the spindle stop and the post spells it if the dialect can.

### CAM-POST-020 — Subprograms and the program structure
- **State:** READY
- **Depends on:** CAM-POST-002
- **Files:** `crates/cam/src/post/structure.rs` (owner CAM)
- **Contract:** `pub fn emit_structure(d: &PostDef, parts: &[String]) -> Result<String, CamError>;`
- **Behaviour:** One linear program, or one subprogram per tool (`CAM-POST-016`), or a numbered-block program (`CAM-POST-014`). The structure is a `PostDef` field, so a dialect is a choice, not a code path.
- **Refusals:** more subprograms than `MAX_SUBPROGRAMS` → `CamError::Limit`. A dialect with no structure support and more than one part → `CamError::PostRefusal`.
- **Tests:** `a_single_program_is_linear`. `a_subprogram_dialect_emits_one_per_tool`. `a_subprogram_limit_is_refused`. `every_structure_ends_with_the_end_word`.
- **Acceptance:** the structure is data; the engine does not branch on dialect names.
- **Risk:** a control that requires a main program calling the subs. Fallback: `PostDef` names the main program's words.

### CAM-POST-021 — The differential test between two dialects
- **State:** READY
- **Depends on:** CAM-POST-007
- **Files:** `crates/cam/tests/post_differential.rs` (owner Q)
- **Contract:** `two_dialects_produce_the_same_motion`; `two_dialects_produce_the_same_removed_volume`; `a_dialect_difference_is_named_in_the_failure`.
- **Behaviour:** Posts one fixture through two dialects, reads both back and compares the reconstructed toolpaths move for move. A difference is reported as a **named difference** (a missing word, a different arc style, a linearised cycle), so a dialect's differences are measured facts rather than assumptions. This is the test that keeps eight dialects from drifting apart.
- **Refusals:** a difference that is not in the expected-differences table → the test fails, naming the two blocks.
- **Tests:** the three above, over every dialect pair and every fixture.
- **Acceptance:** the expected-differences table is in the test and is reviewed whenever a dialect is added.
- **Risk:** the table grows. Fallback: it is data, in the test, and the failure message prints it.

### CAM-POST-022 — The external simulator harness for every dialect
- **State:** READY
- **Depends on:** CAM-POST-009
- **Files:** `crates/cam/tests/post_external.rs` (owner Q)
- **Contract:** `pub fn probe_all(bytes: &[(&str, Vec<u8>)], reader: &dyn Fn(&[u8]) -> Result<ProbeReport, String>) -> Result<(), String>;` — one call, all dialects, one report.
- **Behaviour:** Runs the probe for each dialect, collects the verdicts, and writes the summary the report file holds. The report is **the acceptance artefact for every dialect task** and lists reader name, version, command, fixture list and one verdict per dialect.
- **Refusals:** no reader → `#[ignore]` with the variable named. A failing dialect → the whole probe fails, naming the dialect.
- **Tests:** `the_probe_covers_every_registered_dialect` — asserts the dialect count matches `PostDef` registrations, so adding a dialect without probing fails. `a_failing_dialect_names_itself`.
- **Acceptance:** **the checked-in report exists, names a real independent simulator with a version, and passes on every registered dialect.**
- **Risk:** no simulator on the dev machine. Fallback: as `CAM-POST-009` — write the harness, leave it ignored, fabricate nothing.

### CAM-POST-023 — The dialect registry and its conformance requirement
- **State:** READY
- **Depends on:** CAM-POST-022
- **Files:** `crates/cam/src/post/registry.rs` (owner CAM)
- **Contract:** `pub fn dialect(name: &str) -> Option<PostDef>;` and `pub fn all_dialects() -> Vec<(&'static str, PostDef)>;` and `pub fn conformance(name: &str) -> Result<Conformance, CamError>;` with `pub enum Conformance { Verified { simulator: String, date: String }, Unverified }
- **Behaviour:** One registry, and each dialect carries a **conformance record**: which simulator verified it and when. `conformance` returns `Unverified` until the report exists, and the UI shows that next to the dialect — so a user can see which of eight dialects have been checked by something other than us.
- **Refusals:** posting with an `Unverified` dialect is **allowed with a warning** (the machine is the last check and the user may have no simulator) — but the warning says exactly that. Posting with a `Failed` dialect is refused.
- **Tests:** `every_registered_dialect_has_a_conformance_record`. `an_unverified_dialect_reports_unverified`. `a_failed_dialect_is_refused`. `the_registry_count_matches_the_dialect_task_list`.
- **Acceptance:** the registry's count and the task list agree, asserted by a test.
- **Risk:** a user posts an unverified dialect to a real machine. Fallback: the warning names the simulator that was never run.

### CAM-POST-024 — A dialect we do not have: refused, with the list we do have
- **State:** READY
- **Depends on:** CAM-POST-023
- **Files:** `crates/cam/src/post/unknown.rs` (owner CAM)
- **Contract:** `pub fn dialect_for_machine(m: &MachineModel) -> Result<PostDef, CamError>;`
- **Behaviour:** Maps a machine's dialect name to a `PostDef`; an unknown name is a **named refusal listing the eight we have**, because "we do not have your controller" is a support question with a one-line answer.
- **Refusals:** `Err(CamError::Unsupported("no post for {machine}; the available dialects are {list}"))` naming the machine and the list.
- **Tests:** `a_known_machine_resolves`. `an_unknown_machine_lists_the_dialects`. `an_empty_machine_name_is_refused`.
- **Acceptance:** no code path posts with a guessed dialect.
- **Risk:** none; this is the desired behaviour.

---

# CAM-ADD — additive manufacture (14 tasks)

### CAM-ADD-001 — The ADR: slicing is not toolpathing
- **State:** READY
- **Depends on:** —
- **Files:** `docs/plan/CAM-adr-0003-slicing-is-not-toolpathing.md` (new; owner PLT)
- **Contract:** the ADR states that a slice is a *sequence of 2D regions plus extrusion parameters*, not a sequence of `Move`s, and that reusing `Toolpath` would force a printer's vocabulary into a mill's (which is why `CAM-ADD` has its own types).
- **Behaviour:** Records the decision, the alternative (one toolpath type with printer moves), and the shared part (both consume `KER-MSH-005`'s section, both refuse rather than approximate).
- **Refusals:** n/a (documentation).
- **Tests:** `the_adr_exists_and_is_linked`.
- **Acceptance:** the ADR exists; `CAM-ADD`'s types do not reference `Toolpath`.
- **Risk:** two parallel data models. Fallback: the shared vocabulary is `Solid`, `Regions`, `Section` and the error style.

### CAM-ADD-002 — The slice model
- **State:** BACKLOG
- **Depends on:** CAM-ADD-001
- **Files:** `crates/cam/src/add/types.rs` (owner CAM)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Slice { pub z: f64, pub regions: Regions }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Sliced { pub layers: Vec<Slice>, pub params: PrintParams }
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
  pub struct PrintParams { pub layer_height: f64, pub extrusion_width: f64, pub wall_count: u8, pub infill: Infill, pub top_layers: u8, pub bottom_layers: u8, pub solid_threshold_mm2: f64 }
  ```
- **Behaviour:** A layer per `layer_height`, each carrying the material's cross-section at that Z, plus the parameters that turn a region into a path. Zero-volume layers (above or below the part) are not emitted.
- **Refusals:** `layer_height <= 0`, `extrusion_width <= 0`, `wall_count == 0` → `CamError::Invalid` naming the field. A layer count over `MAX_LAYERS = 100_000` → `CamError::Limit`. A layer height over a stated fraction of the extrusion width (default 0.75) is refused, because a layer thicker than the nozzle can extrude is unmachinable.
- **Tests:** `a_box_slices_into_layers_of_the_right_height`. `empty_layers_are_not_emitted`. `a_layer_thicker_than_the_nozzle_is_refused`. `the_layer_count_matches_the_height_ratio`.
- **Acceptance:** every slice carries regions, not moves.
- **Risk:** a very tall part with a small layer height is a 100 000-layer program. Fallback: the cap refuses with the count and the cap in the message.

### CAM-ADD-003 — Slicing a B-rep at each layer
- **State:** BLOCKED
- **Depends on:** CAM-ADD-002, KER-MSH-005
- **Files:** `crates/cam/src/add/slice.rs` (owner CAM)
- **Contract:** `pub fn slice(solid: &Solid, params: &PrintParams) -> Result<Sliced, CamError>;`
- **Behaviour:** `section(solid, Plane::xy(z), tol)` per layer. BLOCKED on the one-line `KER-MSH-005`: **without it, CADCraft cannot slice, and neither can CAM's pocket detection** (`CAM-PATH-008`). This is the highest-leverage unblock in the programme and it is one line in `crates/solid/src/lib.rs`.
- **Refusals:** a section failure → `CamError::Kernel` naming the kernel error. A layer whose section is empty while the part's bounds contain the layer → `CamError::Kernel("the section came back empty inside the part's bounds")` — a silent empty layer is a missing layer.
- **Tests:** `a_prism_slices_into_congruent_layers`. `a_prism_with_a_hole_slices_into_rings`. `an_empty_layer_inside_the_bounds_is_an_error_not_a_gap`. `the_layer_volumes_sum_to_the_part_volume` to `1e-9`.
- **Acceptance:** the volume-sum test, which is the slicer's own correctness criterion.
- **Risk:** a layer exactly on a face plane (see `section.rs:38-49`). Fallback: layers are offset by half a layer height from the face planes, tested.

### CAM-ADD-004 — Volume and mass from the slice
- **State:** READY
- **Depends on:** CAM-ADD-002
- **Files:** `crates/cam/src/add/volume.rs` (owner CAM)
- **Contract:** `pub fn volume(s: &Sliced) -> Result<f64, CamError>;` and `pub fn mass(s: &Sliced, density_g_cm3: f64) -> Result<f64, CamError>;`
- **Behaviour:** `Σ area_i · layer_height` for the ideal volume, and the **actual** printed volume from the paths (wall lines + infill area), which is larger. Both numbers, because a user comparing a quoted weight to a spool's uses the second.
- **Refusals:** a negative layer area → `CamError::Invalid` naming the layer. A non-finite density → refused. A `density` outside `0.1..=30 g/cm³` → `CamError::Invalid` naming both numbers.
- **Tests:** `the_ideal_volume_is_area_times_layer_height`. `the_printed_volume_exceeds_the_ideal`. `the_mass_uses_the_printed_volume`. `a_density_out_of_range_is_refused`.
- **Acceptance:** the sheet shows both numbers.
- **Risk:** the "actual" volume depends on the extrusion model's assumptions. Fallback: it is labelled an estimate with the wall/infill assumptions printed.

### CAM-ADD-005 — Infill patterns
- **State:** READY
- **Depends on:** CAM-ADD-002
- **Files:** `crates/cam/src/add/infill.rs` (owner CAM)
- **Contract:** `pub fn infill(region: &Regions, pattern: Infill, params: &PrintParams) -> Result<Vec<Polyline>, CamError>;` with `pub enum Infill { Lines { angle_deg: f64, spacing: f64 }, ZigZag { angle_deg: f64, spacing: f64 }, Concentric { spacing: f64 }, None }`
- **Behaviour:** Three patterns, all raster-based on the layer's region, all clipped to it. `Concentric` offsets the region's boundary inward by `spacing` per ring — which needs `KER-CAM-001`, so it is **BLOCKED** on that and the other two are not.
- **Refusals:** `Gyroid` is not offered; a document naming it is `CamError::Unsupported("gyroid infill is a maths surface and is not implemented")` — named, not approximated by lines. A `spacing <= 0` → `CamError::Invalid`.
- **Tests:** `lines_cover_the_region_at_the_spacing`. `a_larger_angle_changes_the_pattern`. `the_lines_are_clipped_to_the_region`. `gyroid_is_a_named_refusal`. `a_zero_spacing_is_refused`.
- **Acceptance:** every path is inside its layer's region (asserted by a containment test).
- **Risk:** the classic "sparse infill" percentage confusion. Fallback: the spacing is the parameter and the sheet prints the resulting coverage percentage.

### CAM-ADD-006 — Perimeters and top/bottom solid layers
- **State:** READY
- **Depends on:** CAM-ADD-005, KER-CAM-001
- **Files:** `crates/cam/src/add/walls.rs` (owner CAM)
- **Contract:** `pub fn walls(region: &Regions, params: &PrintParams) -> Result<Vec<Polyline>, CamError>;` and `pub fn solid_layers(s: &Sliced) -> Result<Vec<usize>, CamError>;`
- **Behaviour:** `wall_count` perimeters inset by `extrusion_width` each (again `KER-CAM-001`), and the first `bottom_layers` and last `top_layers` filled solid, selected by layer area above `solid_threshold_mm2`.
- **Refusals:** a region smaller than `wall_count · extrusion_width` → `CamError::Invalid("the layer is {w} mm wide and {n} walls of {e} mm do not fit")` naming all three. A top/bottom layer index outside the slice → `CamError::Invalid` naming it.
- **Tests:** `the_wall_count_is_exactly_the_parameter`. `the_walls_are_inset_by_the_extrusion_width`. `a_layer_too_narrow_for_the_walls_is_refused_with_three_numbers`. `the_top_and_bottom_layers_are_solid`.
- **Acceptance:** a slice's total width equals its walls plus its infill, asserted to `1e-6`.
- **Risk:** an inset on a non-convex layer. Fallback: `KER-CAM-001`'s refusal rather than a wrong inset.

### CAM-ADD-007 — Skirt and brim
- **State:** READY
- **Depends on:** CAM-ADD-005
- **Files:** `crates/cam/src/add/adhesion.rs` (owner CAM)
- **Contract:** `pub fn skirt(bed: &Regions, layers: usize, offset: f64) -> Result<Vec<Polyline>, CamError>;` and `pub fn brim(region: &Regions, width: f64) -> Result<Vec<Polyline>, CamError>;`
- **Behaviour:** A skirt of `n` loops around the part's footprint at a stated offset; a brim as extra perimeters outside the part. **Raft is refused** — it is a separate extrusion surface with its own path model, and a partial raft is worse than none.
- **Refusals:** `CamError::Unsupported("a raft is not implemented")`. A brim wider than a stated fraction of the part (`0.25`) → refused, since a brim that wide is a raft in all but name.
- **Tests:** `the_skirt_has_the_requested_loop_count`. `the_brim_offsets_outward`. `a_raft_is_a_named_refusal`. `an_over_wide_brim_is_refused`.
- **Acceptance:** the adhesion paths are excluded from the part's volume calculation.
- **Risk:** a skirt offset inside the part. Fallback: the offset is outward by construction and a containment test asserts it.

### CAM-ADD-008 — Supports for overhangs
- **State:** BLOCKED
- **Depends on:** KER-MSH-006, CAM-ADD-005
- **Files:** `crates/cam/src/add/supports.rs` (owner CAM)
- **Contract:** `pub fn supports(s: &Sliced, params: &SupportParams) -> Result<Supports, CamError>;` with `pub struct SupportParams { pub max_overhang_deg: f64, pub style: SupportStyle, pub min_thickness: f64 }` and `pub enum SupportStyle { Block { width: f64 }, Tree { trunk: f64, branch: f64 } }`
- **Behaviour:** Finds layer regions whose *underside* area is exposed (the area of the region at `z` minus the area of the region at `z − layer_height`, above a threshold) and builds supports down to the bed. The **overhang angle** is computed from that exposed area per region, not guessed.
- **Refusals:** an overhang that cannot be reached from the bed (a floating island) → `CamError::Unsupported("a floating region at layer {i} needs support from above; only bed-anchored supports are implemented")`. `max_overhang_deg` outside `0..=90` → `CamError::Invalid`.
- **Tests:** `a_flat_overhang_gets_no_support`. `a_45_degree_overhang_gets_support_at_the_set_angle`. `a_floating_region_is_refused_with_its_layer`. `the_support_volume_is_reported`.
- **Acceptance:** the support material volume is on the sheet with a removal note.
- **Risk:** a floating island is common and users expect support. Fallback: the refusal names the layer, which is the information they need.

### CAM-ADD-009 — FDM process words: temperature, retraction, speed
- **State:** READY
- **Depends on:** CAM-ADD-002
- **Files:** `crates/cam/src/add/process.rs` (owner CAM)
- **Contract:** `pub struct ProcessProfile { pub nozzle_mm: f64, pub bed_c: f64, pub nozzle_c: f64, pub print_mm_s: f64, pub travel_mm_s: f64, pub retract_mm: f64, pub start_gcode: String, pub end_gcode: String }` and `pub fn validate_profile(p: &ProcessProfile) -> Result<(), CamError>;`
- **Behaviour:** A profile of **our own defaults**, not a vendor filament profile: one PLA profile and one ABS profile with the numbers in the source, and every field editable. Start/end G-code are user text, validated as non-empty and passed through verbatim.
- **Refusals:** a nozzle of 0 or negative → `CamError::Invalid`. A temperature outside `0..=350 °C` → refused naming the value. An empty `start_gcode` when the profile needs one → `CamError::Invalid`.
- **Tests:** `a_pla_profile_validates`. `a_temperature_out_of_range_is_refused`. `the_start_gcode_is_passed_through_verbatim`. `the_retraction_distance_is_a_positive_number`.
- **Acceptance:** the profile is data with a validator, and no vendor file is read.
- **Risk:** a vendor profile file (`.ini`) exists and users have one. Fallback: the file is a naming convention we do not read; the fields are entered, and a refused value names itself.

### CAM-ADD-010 — The slicer validator
- **State:** READY
- **Depends on:** CAM-ADD-003, CAM-ADD-004
- **Files:** `crates/cam/tests/slicer_known_answers.rs` (owner Q)
- **Contract:** the file's test list: `a_cube_slices_to_its_volume_within_1e_9`; `a_prism_with_a_hole_slices_to_its_volume_within_1e_9`; `the_layer_count_is_height_over_layer_height`; `the_extrusion_length_is_lines_times_width`; `the_nozzle_hits_bed_at_z_zero`.
- **Behaviour:** Five closed-form answers, asserted to a relative `1e-9` on the ideal volume (which the kernel can give exactly) and to `1e-6` on the printed volume. A slicer that silently drops a layer passes on screen and fails here.
- **Refusals:** none — this is the gate. A failure blocks `CAM-ADD-014`.
- **Tests:** the five above, with the closed forms written in the test.
- **Acceptance:** green before any FDM post is written.
- **Risk:** none; this is the analogue of `CAM-SIM-012`.
- **Note:** the slicer has no *machine* to crash, so its gate is a volume identity rather than `CAM-SIM-004`'s gouge check.

### CAM-ADD-011 — Print time from the paths
- **State:** READY
- **Depends on:** CAM-ADD-002, CAM-PATH-027
- **Files:** `crates/cam/src/add/time.rs` (owner CAM)
- **Contract:** `pub fn print_time(s: &Sliced, p: &ProcessProfile) -> Result<TimeEstimate, CamError>;`
- **Behaviour:** `extrusion_sections · length / print_speed + travel_length / travel_speed + (layers · accel allowance)`. Same formula discipline as `CAM-PATH-027`, and the same honesty about what is not modelled.
- **Refusals:** a print or travel speed ≤ 0 → `CamError::Invalid`. A non-finite result → refused.
- **Tests:** `the_time_is_the_formula`. `a_slower_profile_gives_a_longer_time`. `the_acceleration_allowance_is_reported_as_an_uncertainty`.
- **Acceptance:** the number appears on the sheet with its uncertainty.
- **Risk:** real print times vary with acceleration and cooling. Fallback: the uncertainty is printed, exactly as `CAM-PATH-027` does.

### CAM-ADD-012 — Adhesion and overhang warnings with their angles
- **State:** READY
- **Depends on:** CAM-ADD-008, CAM-ADD-002
- **Files:** `crates/cam/src/add/warnings.rs` (owner CAM)
- **Contract:** `pub fn warnings(s: &Sliced, params: &PrintParams) -> Result<Vec<Warning>, CamError>;` with `pub enum Warning { Overhang { layer: usize, angle_deg: f64 }, FloatingRegion { layer: usize }, FirstLayerTooSmall { area_mm2: f64 }, NarrowBridge { layer: usize, span_mm: f64 } }`
- **Behaviour:** Four checks, each with the number that triggered it. The angle is computed from the exposed area, so it is a measurement, not a rule of thumb.
- **Refusals:** an area or angle that is not finite → the warning records `f64::INFINITY` and the check is reported as failed rather than passing.
- **Tests:** `a_45_degree_overhang_warns_with_its_angle`. `a_floating_region_warns_with_its_layer`. `a_normal_part_has_no_warnings`. `every_warning_names_a_number`.
- **Acceptance:** the warnings print on the sheet and block nothing — they are advice, and the file says so.
- **Risk:** a warning the user cannot act on. Fallback: each warning names the parameter to change.

### CAM-ADD-013 — Support volume and removal notes
- **State:** BLOCKED
- **Depends on:** CAM-ADD-008
- **Files:** `crates/cam/src/add/support_volume.rs` (owner CAM)
- **Contract:** `pub fn support_volume(s: &Supports) -> Result<(f64, Vec<RemovalNote>), CamError>;` with `pub struct RemovalNote { pub layer: usize, pub kind: RemovalKind, pub hint: String }`
- **Behaviour:** The support material's volume and, per support, how it comes off (a tree support pulls, a block support needs a tool) — the distinction a user needs before starting a long print.
- **Refusals:** a support that cannot be reached by the tool the user has → the note says so by name (`"a 6 mm block support needs a 6 mm tool; the setup has {tool}"`).
- **Tests:** `the_support_volume_is_the_sum_of_its_paths`. `a_tree_support_gets_a_pull_note`. `a_block_support_needs_a_tool_note`. `the_volume_is_on_the_sheet`.
- **Acceptance:** the sheet shows the support volume next to the part's.
- **Risk:** the tool set is not known at slice time. Fallback: the note names the setup's tools when there is one and is generic otherwise.

### CAM-ADD-014 — The FDM dialect (Marlin / RepRap class)
- **State:** BLOCKED
- **Depends on:** CAM-ADD-010, CAM-POST-007, CAM-POST-009
- **Files:** `crates/cam/src/post/dialects/fdm.rs` (owner CAM)
- **Contract:** `pub fn fdm_def() -> PostDef;` — metric, `G0/G1`, `G92 E0` for extrusion reset, `M104`/`M140` for temperatures, `M104 S0`/`M140 S0` at the end, `G28` home at the start, `M84` at the end, `;` comments, no arcs.
- **Behaviour:** The only additive dialect, and it is a **post** like any other: it passes through the same gate. Its moves come from the slicer's paths converted to `Move`s, so the read-back test applies unchanged.
- **Refusals:** an arc in a printer's path → `CamError::PostRefusal("the FDM dialect has no arc words")`; the slicer emits lines only. A missing retraction word after a travel over `max_travel_without_retract` → refused, naming the length.
- **Tests:** `the_start_gcode_runs_before_any_move`. `the_temperatures_appear_and_are_turned_off_at_the_end`. `the_extrusion_is_reset_per_layer`. `the_output_round_trips`. `the_simulator_report_has_a_row_for_fdm`.
- **Acceptance:** both gates green **and** `CAM-ADD-010`'s five answers green.
- **Risk:** firmware variants (Klipper, Duet) differ in the start sequence. Fallback: `start_gcode`/`end_gcode` are user text and the dialect's own words are the minimum.

---

## 3. Task count

| Epic | Tasks | Ready now | Blocked on KER / cross-domain | Backlog |
|---|---|---|---|---|
| `CAM-TOOL` | 22 | 16 | 2 | 4 |
| `CAM-MACH` | 18 | 14 | 2 | 2 |
| `CAM-PATH` | 30 | 8 | 17 | 5 |
| `CAM-SIM` | 22 | 12 | 8 | 2 |
| `CAM-POST` | 24 | 14 | 10 | 0 |
| `CAM-ADD` | 14 | 6 | 7 | 1 |
| **total** | **130** | **70** | **46** | **14** |

### The blockers, in the order they should be cleared

1. **`KER-CAM-001`, `offset_regions`** — every finishing contour, every pocket finish, every rest-machining
   remainder and every concentric infill ring. `crates/geom` has booleans and **no offset**. This is
   the hardest single contract in CAM.
2. **`KER-MSH-005`, re-export `section`** — **one line** in `crates/solid/src/lib.rs:93`. It unblocks
   `CAM-PATH-006`, `CAM-PATH-008`, `CAM-ADD-003` and, in `SIM.md`, five more tasks. The cheapest
   unblock in the programme.
3. **`KER-BOOL-001..003`** — the three open boolean defects. They gate `CAM-SIM-003`, and a boolean
   that leaves a crack makes the validator's volume **wrong in the direction that hides a gouge**.
4. **`KER-MSH-001`, tracked mesh** — without face provenance, `CAM-SIM-004`'s gouge cannot name a
   face and a machinist cannot look at what was cut.
5. **`KER-MSH-006`, `closest_point`** — the generation-time gouge guard and `CAM-MACH-009`.
6. **`KER-MSH-003`, stable ids** — every saved CAM setup re-targets after a rebuild until this lands.
7. **`CAM-POST-009`** — an independent simulator and a written report. Until it exists, **no dialect
   in this file is more than `CODE WRITTEN`** and `ROADMAP.md`'s CAM row must say so.

### What this file does not claim

- **No 5-axis simultaneous machining.** 3+2 positioning is specified; anything else is refused by
  name (`CAM-PATH-023`).
- **No 3D roughing.** A parallel finishing strategy is specified and a roughing strategy is a
  refusal with two alternatives named (`CAM-PATH-016`).
- **No machine connection.** No sockets, no MTConnect, no DNC upload — two refusals and one ADR
  (`CAM-MACH-006`, `CAM-MACH-007`, `CAM-MACH-014`).
- **No vendor tool or machine data.** Every library row is our own (`CAM-TOOL-007`,
  `CAM-TOOL-014`).
- **No post processor without a report from an independent simulator.** This is the epic's rule and
  it is written into each dialect's *Depends on* and *Acceptance*.