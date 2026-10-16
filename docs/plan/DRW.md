# DRW — engineering drawing: views, sheets, dimensions, GD&T, hidden lines

Task index for the `DRW` domain (8 % of the programme). Owned crate: `crates/drawing/**` — **new**,
package `cadcraft-drawing`. Everything a view draws lands in the **existing** 2D drawing database
(`cadcraft_doc`), because the 2D annotation code in `crates/engine/src/cmd/annotate*.rs` is the base
and a second document model would be a defect, not a design.

**Status:** this file is a **specification**. Nothing in it has been compiled, tested or executed
(`docs/agents/SYSTEM_PROMPT.md` §2).

**Owner letters** are the wave-1 agents of `docs/master-plan.md` §6 — **A** kernel, **Q** quality,
**D** interchange, **U** ui/viewport, **C** orchestrator — plus the letters the sibling indexes use:
**DRW** (`crates/drawing/**`, this file), **MOD** (`crates/model/**`), **C2** 2D/doc
(`crates/doc/**`, `crates/dxf/**`, `crates/color/**`), **ENG** (`crates/engine/**`), **REN**
(`crates/render/**`), **QAT** (`crates/*/tests/**`, `xtask/**`), **PLT** (`docs/**`, `xtask/**`,
root `Cargo.toml`). A task whose **Files** field names another letter's file *and modifies it* is
`BLOCKED` on that owner (`master-plan.md` §6, `ASM.md` §11).

**Why the ids carry an epic segment.** `master-plan.md` §2 defines task identity as
`DOMAIN-EPIC-NNN` (`KER-BOOL-004`), and `docs/plan/DAT.md` refers forward to `PLT-ASSET-008`, which
only resolves under that form. `DAT.md` uses a flat per-domain numbering; this file and `PLT.md` use
the master-plan form so cross-index references are unambiguous. A `DAT-nnn` reference from here
keeps its own numbering.

---

## Invariants of this domain

1. **The 2D drawing database is the document.** A sheet is a `cadcraft_doc::Layout`; a view is a set
   of entities in `Space::Paper(layout_name)`; a dimension is `EntityKind::Dimension`; a feature
   control frame is `EntityKind::Tolerance`; a balloon is an `MLeader`; a title block is a `Block`
   and a parts list is a `Table`. There is no `Drawing3D`, no second entity store, no second
   handle space.
2. **The 3D model is an input, and it is read-only to a drawing.** A view is computed from
   `&cadcraft_solid::Solid` (plus names). The drawing never edits the model and never stores a
   second copy of the geometry — a projected edge on a sheet is a `Line` in paper space, exactly as
   AutoCAD's `PROJECT3D` writes one.
3. **The association is the fragile part, and is treated as fragile.** `Model` has no topological
   naming (`crates/model/src/feature.rs:110`, `EdgeRef` is a geometric point), so a dimension that
   follows a 3D face is a *geometric* reference that breaks when the face is replaced. Every
   dimension therefore carries its 3D target **and** its current 2D definition points, and a target
   that no longer resolves is reported, never silently re-pointed (`DRW-DIM-021`).
4. **Numbers are shown in the document's unit, and converted once.** The 3D model is millimetres
   (`crates/solid/src/lib.rs:38`, `tolerance_for`); the 2D drawing is inches or millimetres
   (`$INSUNITS`, `crates/doc/src/header.rs`). The single conversion is the view's model→paper
   factor (`DRW-VIEW-011`), which is exactly `AnnoScale::factor()` (`crates/doc/src/anno.rs:42`).
   Display formatting is `cadcraft_render::units::format_linear_value` (`crates/render/src/dim.rs:351`)
   plus `DAT-045`'s unit conversion — never a second formatter.
5. **Hidden-line removal has two paths, and the output says which one produced it.** The faceted
   path (tessellation + own depth buffer) works today and is *approximate*; the exact path (face
   normals, silhouette curves, surface-surface occlusion) is `BLOCKED` on the kernel. The default is
   the path that is available; a caller that asks for the exact path gets
   `Err(DrwError::ExactVisibilityUnavailable)` until it exists (`DRW-XRF-012`). A drawing never
   silently substitutes one for the other.
6. **No panics, every refusal named.** The crate root carries the house `#![deny(...)]` and
   `#![forbid(unsafe_code)]` (`AGENTS.md`). Every task below has a **Refusals** field; a task whose
   Refusals read "none" is incomplete.
7. **Layering.** `cadcraft-drawing` is **L2**: it may use `cadcraft-geom` (L0), `cadcraft-solid` (L0)
   and `cadcraft-doc` (L1). It must **not** use `cadcraft-render` (L2 — sideways is a violation;
   `INTRA_LAYER_ORDER` in `xtask/src/layers.rs:64` allows `fonts → render` only), `cadcraft-io` (L3),
   `cadcraft-model` (L3), `cadcraft-asm` (L3) or anything at L4+. Consequences, stated once:
   - Dimension/GDT geometry is produced as `cadcraft_doc` entities and **drawn by the existing
     renderer**; `cadcraft-drawing` must not call `render::dimension_geometry`
     (`crates/render/src/dim.rs:466`) — the renderer already calls it for any `Dimension` entity.
   - Hidden-line removal computes **geometry**, never pixels; rasterising it is `render`'s job.
   - The viewport's hidden-line visual style (`UI-VP-014`, `REN-003`) is served by `ENG`, which may
     hold both `drawing` (L2) and `render` (L2) — see `DRW-XRF-011`.
   - An assembly is consumed as a neutral list of placed parts (`ScenePart`, `DRW-VIEW-001`), so no
     L2 → L3 edge is created. **Interface request to `ASM`:** do not define a competing type; if ASM
     wants `cadcraft-drawing` to take `cadcraft_asm::Assembly`, `cadcraft-drawing` must move to L4,
     which is a coordinator decision, recorded in the report.
8. **Determinism.** Two runs of the same projection over the same solid produce the same entity
   sequence, in the same order, with the same handles relative to the view's base. No `HashMap`
   iteration reaches an output; every collection is sorted by an explicit key (`f64::total_cmp`,
   face index, item number). `DRW-VIEW-018` is the test.
9. **Tests live inside the modules.** `crates/*/tests/**` belongs to **QAT** (`master-plan.md` §3,
   `KER.md` §0.5), so a `DRW` task's unit tests go in the `#[cfg(test)] mod tests` block of the file
   it owns. A task that wants an integration or property test names that in its **Files** field and
   is `BLOCKED` on a QAT assignment.

### What is reused, and from where (evidence)

| Reused | Where | Why it is the right reuse |
|---|---|---|
| `Drawing`, `Layout`, `Space::Paper`, `EntityStore`, `add`, `modify_entity` | `crates/doc/src/lib.rs:59-339` | A sheet *is* a layout; there is no other place to put a view. |
| `PageSetup { paper, width_mm, height_mm, margins_mm, landscape, scale, … }` | `crates/doc/src/tables.rs:731-753` | Paper size, margins and the plot scale are already modelled and already written to DXF/DWG. |
| `PaperSize`/`PAPER_SIZES` list | `crates/render/src/paper.rs:14-70` | The one list of standard sheets. `DRW` **reads it through `PageSetup.paper`** — it must not depend on `render` (invariant 7). |
| `Dimension`, `DimKind`, `DimAssoc`, `AssocSnap`, `DimMarks`, `Leader`, `MLeader`, `Tolerance`, `Table` | `crates/doc/src/entity.rs:513-965` | Every annotation kind a drawing needs already exists as a DXF entity, with a group code and a writer. |
| `Tolerance`, `GdtSymbol`, `symbol_prims`, `MAX_FRAME_CHARS/ROWS/CELLS` | `crates/doc/src/gdt.rs:11-120` | GD&T frames are already modelled and drawn **from our own symbol geometry**, not a font. A 3D frame is the same entity. |
| `AnnoScale`, `anno_scaled`, `factor()`, `same_factor` | `crates/doc/src/anno.rs:15-53,154` | The view scale factor is an annotation scale; no second scale type. |
| `DimStyle` + `DIMVARS` + `dim_overrides()` | `crates/doc/src/tables.rs:147`, `crates/doc/src/lib.rs:221` | One dimension style for 2D and 3D, with per-dimension overrides already serialised. |
| `MAX_BLOCK_DEPTH` | `crates/doc/src/lib.rs:29` | The bound for a view nested in a view block and for leader/balloon block content. |
| `render::dimension_geometry`, `format_linear_value`, `gdt_points`, `tolerance_in` | `crates/render/src/dim.rs`, `render/src/tolerance.rs` | Called by the **renderer** for any dimension entity, so a projected dimension draws through the existing path. |
| `add_dim`, `DIMSTYLE`, `DIMOVERRIDE`, `DIMTEDIT`, `DIMSPACE`, `dim_overrides` commands | `crates/engine/src/cmd/annotate.rs:118-722` | The interactive 2D surface. A 3D dimension reuses the same style/override machinery through the same `Session`. |
| `cadcraft_solid::{Solid, Face, Edge, Plane, EdgeRef, Mesh, mesh_from_solid, section, tolerance_for, limits}` | `crates/solid/src/lib.rs:85-95,98-114,153` | The only 3D input, used unmodified. `Edge { a, b, faces: [usize;2] }` (`solid.rs:26-31`) is what exact visibility classification needs. |
| `cadcraft_geom::{Vec2, Vec3, Mat3, Mat4, Region, Regions, Bounds2, Bounds3, bool2d}` | L0 | Projection arithmetic and 2D region work; no new geometry crate. |
| `PlotOutput`, `PlotterConfig`, `DevicePaper`, the `plot` command | `crates/doc/src/plotter.rs:27-107`, `crates/engine/src/cmd/layout.rs:79` | A 3D sheet is plotted by the **existing** plot path; `DRW` adds sheet *content*, not a plotter. |
| `Session::solid_model` | `crates/engine/src/lib.rs:674-680` | The 3D model lives beside the 2D drawings, not inside one. Its own doc says it "shares none of its machinery" — this file respects that. |
| `IoError::{Format, Unsupported}`, `read_notes`, the extension dispatch | `crates/io/src/lib.rs:46,56-113` | The export seam for DXF/DWG/plot output, plus the existing "here is what we dropped" convention. |

### What `cadcraft-drawing` deliberately does not do

- It does **not** rasterise. Producing a PNG of a view is `render` + `io` (`png_framed`,
  `crates/io/src/lib.rs:168`).
- It does **not** own a file format. `.ccm` is `DAT`'s (`crates/model/src/file.rs`); the sheet's
  records are an **interface request** to that crate (`DRW-SHE-016`, `DRW-VIEW-007`).
- It does **not** replace the kernel's `section` (`crates/solid/src/section.rs:50`). A drawing
  section is *that* function's result, placed on a sheet and hatched by the existing hatch code.
- It does **not** implement hidden-line removal by calling `scene3`. The raster is its own
  (`DRW-XRF-004`), sized from the sheet, and is discarded after the edge sets are produced.

### The `KER` contracts this file needs — all `BLOCKED`, signatures stated

`KER.md` §0.7 forbids inventing a cross-domain type here. Each task below that needs the kernel says
**needs `KER-…`** and states the signature it requires. `KER-SRF-*`, `KER-FEAT-*`, `KER-TOL-*` and
`KER-PERF-*` are named in `KER.md` §1 but not yet written task by task, so the signatures below are
*requests*, to be confirmed by the kernel owner — not definitions made here.

| Needed by | Requires | The signature `DRW` assumes |
|---|---|---|
| `DRW-VIEW-014`, `DRW-XRF-003` | `KER-SRF-015` | `pub enum Curve3 { Line(Line3), Arc{..}, BSpline(Box<BSpline3>), Bezier(..) }` with `trim_by_parameter`, `eval`, `tangent` |
| `DRW-XRF-003`, `DRW-XRF-008`, `DRW-GDT-002`, `DRW-GDT-013` | `KER-SRF-018`, `KER-SRF-023` | `pub enum Surface { Plane(Plane), Cylinder{..}, Cone{..}, Sphere{..}, Torus{..}, Nurbs(Box<NurbsSurface>) }`, `impl Surface { fn eval(&self, u: f64, v: f64) -> Result<Vec3, SolidError>; fn normal(&self, u: f64, v: f64) -> Result<Vec3, SolidError>; fn domain(&self) -> (f64,f64,f64,f64); }` — plus the UV-frame rule of `KER.md` §0.3 (outward normal) |
| `DRW-XRF-005`, `DRW-XRF-008`, `DRW-GDT-009`, `DRW-GDT-011` | `KER-SRF-033` | `pub struct BFace { pub surface: Surface, pub outer: Vec<BCurve>, pub inner: Vec<BCurve> }` where `BCurve` carries the surface's 2D pcurve parameters and the 3D curve |
| `DRW-XRF-006`, `DRW-SHE-009` | `KER-SRF-035`, `KER-SRF-039`, `KER-SRF-042` | `pub fn intersect_surfaces(a: &Surface, b: &Surface, tol: f64) -> Result<Vec<Curve3>, SolidError>;` — the occlusion and section curves a hidden-line pass must test |
| `DRW-XRF-010`, `DRW-DIM-005` | `KER-SRF-046`, `KER-SRF-048` | `pub struct Brep { pub faces: Vec<BFace>, … }`, `pub fn tolerance(&self) -> Tol` (per-vertex/edge/face) |
| `DRW-SHE-009`, `DRW-VIEW-013` | `KER-BOOL-010` | re-export of `cadcraft_solid::section(&Solid, &Plane, f64) -> Result<Section, SolidError>` — exists today, used unmodified |
| `DRW-XRF-011`, `DRW-XRF-012` | `KER-PERF-003` | the uniform grid index used for per-triangle occlusion queries |

---

# DRW-VIEW — a view is a direction, a scale and a set of 2D entities

Covers `master-plan.md` `DRW-001` (orthographic views) with the machinery it needs: the frame, the
projection, the neutral output type every other epic consumes, and the record a view persists as.

### DRW-VIEW-001 — The `cadcraft-drawing` crate: error type, limits, and its two registration sites
- **State:** READY
- **Depends on:** —
- **Files:** `crates/drawing/Cargo.toml`, `crates/drawing/src/lib.rs`, `crates/drawing/src/error.rs`, `crates/drawing/src/limits.rs` (owner **DRW**); root `Cargo.toml` `[workspace.dependencies]` and `xtask/src/layers.rs` `TABLE` (owner **C**)
- **Contract:**
  ```rust
  pub type Result<T> = std::result::Result<T, DrwError>;

  #[derive(Debug, thiserror::Error)]
  pub enum DrwError {
      #[error("no such view `{0}`")] NoView(String),
      #[error("no such sheet `{0}`")] NoSheet(String),
      #[error("view direction is not a direction")] BadDirection,
      #[error("`up` is parallel to the view direction")] BadUp,
      #[error("scale {0} is not a finite positive number")] BadScale(f64),
      #[error("{what} is {count}; the limit is {max}")] Limit { what: &'static str, count: usize, max: usize },
      #[error("the 3D target no longer resolves: {0}")] Unresolved(String),
      #[error("exact visibility is not implemented; use HiddenLinesMode::Faceted")] ExactVisibilityUnavailable,
      #[error("needs the geometry kernel: {0}")] NeedsKernel(String),   // never silent
      #[error("{0}")] Refused(String),
  }

  pub mod limits {
      pub const MAX_SOLID_FACES: usize = 60_000;        // == cadcraft_solid::limits::MAX_FACES
      pub const MAX_VIEW_EDGES: usize = 400_000;
      pub const MAX_TESS_TRIS: usize = 2_000_000;       // == cadcraft_solid::limits::MAX_TRIANGLES
      pub const MAX_RASTER_PIXELS: usize = 16_777_216; // 4096 x 4096 at the sheet's own rule
      pub const MAX_VIEW_NESTING: usize = 16;           // == cadcraft_doc::MAX_BLOCK_DEPTH
  }

  /// One placed part of the thing being drawn: a name, a rigid placement, and its solid.
  /// Deliberately *not* `cadcraft_asm::Occurrence` — cadcraft-drawing is L2 and may not reach L3.
  #[derive(Clone, Debug, PartialEq)]
  pub struct ScenePart {
      pub name: String,
      /// Right-handed orthonormal basis columns with `det == +1`, and `origin`. Uniform scale 1.
      pub basis: [Vec3; 3],
      pub origin: Vec3,
      pub solid: cadcraft_solid::Solid,
  }
  impl ScenePart {
      pub fn new(name: &str, origin: Vec3, basis: [Vec3; 3], solid: Solid) -> Result<ScenePart>;
      pub fn to_world(&self, p: Vec3) -> Vec3;
  }
  ```
- **Behaviour:** The crate root carries the house `#![deny(clippy::unwrap_used, clippy::expect_used,
  clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]` and
  `#![forbid(unsafe_code)]`. `lib.rs` documents invariants 1–9 verbatim as crate docs, the way
  `crates/solid/src/lib.rs` documents I1–I5. Every `DrwError` message names what could not be done.
  `ScenePart` is the *only* way a 3D body enters the crate, which is what keeps the crate free of a
  model/assembly dependency. `cadcraft-drawing` is registered in `xtask/src/layers.rs` `TABLE` as
  `("drawing", Class::Layer(2))` **and** in root `Cargo.toml` as `cadcraft-drawing = { path =
  "crates/drawing" }` — **both are required**, or `cargo xtask layers` reports
  `Violation::Unregistered` and every dependent crate fails to resolve.
- **Refusals:** `ScenePart::new` refuses a non-finite field (`BadDirection`), a basis column that is
  not unit within `1e-9`, a non-orthogonal basis (`|dot| > 1e-9`) and a `det <= 0` (a mirror: the
  same refusal `ASM-004` makes, stated once rather than invented again).
- **Tests:** `error_messages_name_their_ids` — `DrwError::NoView("v3")` displays `no such view
  \`v3\``. `every_variant_has_a_display` — no variant renders empty. `a_mirrored_basis_is_refused`.
  `a_non_orthonormal_basis_is_refused`. `to_world_is_the_inverse_of_the_basis`.
  `the_limits_reuse_the_kernels_numbers` — each constant equals the named `cadcraft_solid::limits`
  value, so a cap can never drift between the two crates.
- **Acceptance:** `cargo build -p cadcraft-drawing` succeeds with no `unwrap` and no `unsafe`; `cargo
  xtask layers` prints `cadcraft-drawing  L2  geom, solid, doc` with zero violations; `cargo xtask
  wasm` includes it (L2 ≤ 4) and passes. (NOT EXECUTED here — the orchestrator runs these.)
- **Risk:** the orchestrator's workspace uses `members = ["crates/*", …]`, so the crate is picked up
  automatically; the two hand edits are the `TABLE` row and the `[workspace.dependencies]` line, and
  forgetting the first is a `layers` failure not a build failure. Fallback: none needed; both edits
  are in this task's **Files**.

### DRW-VIEW-002 — `ViewFrame`: a direction, an up vector, and the paper plane they imply
- **State:** READY
- **Depends on:** DRW-VIEW-001
- **Files:** `crates/drawing/src/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct ViewFrame {
      /// Unit: the direction the viewer looks **along** (from the model toward the paper).
      pub dir: Vec3,
      /// Unit, not parallel to `dir`.
      pub up: Vec3,
      /// The plane's own orthonormal `u,v` (the paper's X and Y).
      pub u: Vec3,
      pub v: Vec3,
      /// Model-space point that lands on paper origin.
      pub target: Vec3,
  }
  impl ViewFrame {
      pub fn new(dir: Vec3, up: Vec3, target: Vec3) -> Result<ViewFrame, DrwError>;
      pub fn plane(&self) -> cadcraft_solid::Plane;                 // Plane::with_u(target, dir, u)
      pub fn project(&self, p: Vec3) -> Vec2;                        // (dot(p-target,u), dot(p-target,v))
      pub fn depth(&self, p: Vec3) -> f64;                           // dot(p-target, dir); larger = nearer
      pub fn paper_matrix(&self, scale: f64) -> cadcraft_geom::Mat3; // view -> paper units
      pub fn rotation_from(&self, other: &ViewFrame) -> Result<f64, DrwError>;
  }
  ```
- **Behaviour:** `u = up × dir` normalised and `v = dir × u`, so `(u, v, dir)` is right-handed and
  the projected plane's normal is the view direction. Deterministic sign rules: when
  `|up × dir| < 1e-9` the frame is refused (`BadUp`), **not** silently replaced by `+Y`; when `dir`
  is not finite or is zero length, `BadDirection`. `paper_matrix(scale)` is
  `Mat3::translate(paper_origin) · Mat3::scale(s,s) · Mat3::from_angle(-u→+X)` composed as one
  matrix, and it is the *only* place a model coordinate becomes a paper coordinate (invariant 4).
- **Refusals:** `BadDirection`, `BadUp`, and `Refused("the view frame is degenerate: the projected
  diagonal is below the kernel tolerance")` when a non-zero solid projects to less than
  `tolerance_for(diagonal)` in both axes.
- **Tests:** `the_frame_is_right_handed` — `u × v == dir` to 1e-12. `project_and_depth_recover_the
  _point` — for `p`, `plane.to_world(x, y)` differs from `p` only along `dir`. `an_up_parallel_to
  _the_direction_is_refused`. `a_zero_direction_is_refused`. `a_nan_direction_is_refused`.
  `paper_matrix_maps_the_target_to_the_paper_origin`. `a_reflected_up_gives_a_mirrored_frame` —
  and the frame says so through `u`, never through a negative scale.
- **Acceptance:** every other `DRW-VIEW` task uses `ViewFrame` and none of them re-derives a basis;
  `grep -c 'let u =' crates/drawing` is 0 outside `frame.rs`.
- **Risk:** the sign convention differs from the kernel's `Plane::with_u`, which builds `(u, n×u, n)`.
  Fallback: `ViewFrame::plane()` is the single adapter and a test asserts the two agree on a plane
  rebuilt from the frame.

### DRW-VIEW-003 — Third-angle and first-angle placement, and the mirroring rule
- **State:** READY
- **Depends on:** DRW-VIEW-002
- **Files:** `crates/drawing/src/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum ProjectionAngle { First, Third }
  /// The paper-space offset of a view projected from `parent`, at `scale`.
  pub fn projected_offset(parent: &ViewFrame, parent_scale: f64,
                         child_dir: Vec3, angle: ProjectionAngle,
                         gap: f64, child_size: Vec2) -> Result<Vec2, DrwError>;
  /// `true` when this direction viewed with this up mirrors the model (a left or bottom view).
  pub fn is_mirrored(frame: &ViewFrame) -> bool;
  ```
- **Behaviour:** In **third angle** (ISO, the metric default) a right-side view sits to the right of
  the front view and a top view above it; in **first angle** (ANSI, the imperial default) they sit
  mirrored across. `is_mirrored` is a property of the frame, not a negative scale: a left view's
  paper matrix is a rotation plus a **reflection of the projected coordinates inside the view
  rectangle**, which is what the text on a sheet actually is. A view whose `up` was flipped to
  avoid a mirror is therefore shown with the mirror flag and the drawing's convention, and a test
  pins both to the same picture.
- **Refusals:** a `child_dir` within `1e-9` of `parent.dir` is `Refused("a projected view must look
  in a different direction (within 1e-9 of the parent)")`; a non-finite `gap`, `child_size` or
  scale is the matching named error.
- **Tests:** `third_angle_puts_the_right_view_on_the_right`. `first_angle_mirrors_it`. `a_left_view
  _is_flagged_mirrored_and_is_not_a_negative_scale`. `a_direction_equal_to_the_parent_is_refused`.
  `the_two_conventions_agree_on_a_top_view`.
- **Acceptance:** a sheet built in either convention passes `DRW-VIEW-009`'s picture test, and
  `docs/geometry-parity.md` gains a row for the projection convention (default = third, ISO).
- **Risk:** mirroring is the classic drawing bug — an assembly whose parts look right but read
  wrong. Fallback: `SheetView.mirrored: bool` is stored on the view, so a file says what it did.

### DRW-VIEW-004 — Project a point and an edge, with depth
- **State:** READY
- **Depends on:** DRW-VIEW-002
- **Files:** `crates/drawing/src/project.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct ProjectedSegment { pub a: Vec2, pub b: Vec2, /// nearest end's depth
                                pub depth_a: f64, pub depth_b: f64 }
  pub fn project_segment(frame: &ViewFrame, a: Vec3, b: Vec3) -> Option<ProjectedSegment>;
  pub fn clip_to_rect(seg: &ProjectedSegment, rect: cadcraft_geom::Bounds2) -> Option<ProjectedSegment>;
  ```
- **Behaviour:** `None` for a segment that projects to a point shorter than `1e-12` paper units
  (an edge seen exactly end-on); the caller decides whether that is a silhouette (DRW-XRF-003) or a
  discard. Depth is measured along `dir` so that "nearer" is a larger number, which is the same
  sign convention the depth buffer in DRW-XRF-004 uses — one convention, stated once.
  `clip_to_rect` clips with a Liang–Barsky against the view's printable rectangle so a long part
  costs the rectangle, not the whole line.
- **Refusals:** non-finite endpoints are rejected by the caller (`DRW-VIEW-018`); `project_segment`
  itself takes validated input and documents that as its contract.
- **Tests:** `a_segment_projects_to_its_endpoints`. `an_edge_seen_end_on_has_no_length`. `depth_is
  _larger_for_the_nearer_point`. `clipping_trims_to_the_rectangle`. `a_segment_entirely_outside
  _clips_to_nothing`. `clipping_is_exact_at_the_corners` — each of the four corners survives.
- **Acceptance:** `DRW-XRF` and `DRW-DIM` use only this function to map 3D to 2D; no other
  projection code exists in the crate.
- **Risk:** a segment whose depth changes sign across the view (it crosses the picture plane)
  produces a projected point where the edge crosses; nothing in a 2D projection can show that.
  Fallback: `depth_a`/`depth_b` are kept so DRW-XRF-004 sees the crossing and the exact path
  (DRW-XRF-006) resolves it.

### DRW-VIEW-005 — Project a planar face: its region and its boundary, in view space
- **State:** READY
- **Depends on:** DRW-VIEW-004
- **Files:** `crates/drawing/src/project.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct ProjectedFace {
      pub source_face: usize,
      /// The face's region in view-plane 2D, wound as the kernel winds it.
      pub region: cadcraft_geom::Regions,
      /// Front (normal towards the viewer), back or edge-on.
      pub facing: Facing,
      pub depth: f64,
  }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Facing { Front, Back, EdgeOn }
  pub fn project_face(frame: &ViewFrame, face: &cadcraft_solid::Face, tol: f64)
      -> Result<Option<ProjectedFace>, DrwError>;
  ```
- **Behaviour:** A face whose plane is parallel to the view plane projects affinely: the kernel's
  plane→world map is the projection restricted to the face, so `plane.to_local(p)` and then the
  frame's 2D axes is exact and needs no tolerance at all. `Facing` comes from
  `dot(face.plane.normal, frame.dir)` against `dir_tol = 1e-9` (the kernel's angular rule,
  `KER.md` §0.2) — **never** from the sign of a projected area, which is the mistake that flips a
  whole view. Winding is preserved by construction because the map is a rotation, not a reflection;
  a face whose `to_local` frame is left-handed relative to the view is converted by
  `cadcraft_solid::reverse_regions` (`solid.rs:962`) and the conversion is counted.
- **Refusals:** a face with a non-finite plane or an empty region is `Refused` naming the face index
  (never a skipped face); a face whose projected area is below `tol` is `Ok(None)` (an invisible
  sliver, not an error); `Err(NeedsKernel(..))` for a non-planar face, naming that the kernel has
  no curved surface yet.
- **Tests:** `a_planar_face_projects_to_its_own_region`. `facing_follows_the_normal_not_the_area`.
  `an_edge_on_face_is_flagged_edge_on`. `a_back_face_projects_with_the_same_winding_as_a_front_one`
  — the winding is the kernel's, not the view's. `a_sliver_face_is_none_not_an_error`.
  `a_mirrored_frame_keeps_the_region_positive`. `a_face_with_a_nan_plane_is_refused`.
- **Acceptance:** the test suite passes on a cube: 6 faces projected, 4 front, 1 back, 2 edge-on
  depending on direction, with the exact expected counts pinned.
- **Risk:** a curved face today is faceted by the kernel, so "a cylinder projects to n quads"
  silently. Fallback: `project_face` never sees a curved face — it takes `&Face` — and
  `DRW-XRF-002` states the faceting claim where it belongs.

### DRW-VIEW-006 — `ViewGeometry`: the one neutral output every other epic consumes
- **State:** READY
- **Depends on:** DRW-VIEW-005, DRW-XRF-001
- **Files:** `crates/drawing/src/lib.rs`, `crates/drawing/src/view.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ViewGeometry {
      pub edges: Vec<ViewEdge>,
      pub regions: Vec<ProjectedFace>,
      /// Which path produced it. Written onto the view record and into the report.
      pub mode: HiddenLinesMode,
  }
  #[derive(Clone, Debug, PartialEq)]
  pub struct ViewEdge {
      pub a: Vec2, pub b: Vec2,
      pub kind: EdgeKind,
      /// Index of the source face, or the pair, when the edge came from a real B-rep edge.
      pub source: EdgeSource,
  }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum EdgeKind { Visible, Hidden, Silhouette, Boundary, Crease }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum EdgeSource { Kernel(usize), Tessellated, Exact }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum HiddenLinesMode { Faceted, Exact }
  impl ViewGeometry {
      /// Every segment, in view space, ready for `paper_matrix`.
      pub fn segments(&self) -> Vec<Segment>;
      pub fn count(&self) -> (usize, usize, usize);  // visible, hidden, regions
  }
  ```
- **Behaviour:** This is the crate's **only** product type: geometry, not entities, not pixels. The
  engine (`ENG`) turns `segments()` into `cadcraft_doc` entities; `render` turns them into
  `scene3::Segment` for the viewport. `mode` is carried all the way to the sheet and into
  `docs/geometry-parity.md`, so a drawing records which path drew it (invariant 5). `EdgeKind`
  maps to layers one-for-one: `Visible`→`Visible` layer, `Hidden`→`Hidden` layer (a hidden-line
  linetype), `Silhouette`/`Boundary`→`Outline`, `Crease`→`Visible`.
- **Refusals:** a `ViewGeometry` with more than `limits::MAX_VIEW_EDGES` edges is
  `Err(Limit{ what: "view edges", .. })` — the type's `Default` is an *empty* result, never a
  truncated one; `push_edge` returns `Err` at the cap.
- **Tests:** `segments_are_emitted_in_edge_order` — deterministic order, asserted twice. `the_mode_is
  _carried_into_the_output`. `pushing_past_the_cap_is_a_limit_error`. `an_empty_geometry_has_no
  _segments`.
- **Acceptance:** `DRW-XRF`, `DRW-DIM`, `DRW-GDT` and `DRW-LEAD` all consume this type and none of
  them defines a second projection result.
- **Risk:** a neutral type that grows a field every epic wants. Fallback: fields that describe
  *drawing intent* (which entity to create) belong in `DRW`'s entity writers, not in this struct.

### DRW-VIEW-007 — `SheetView`: the record a view persists as, and where it must live
- **State:** BLOCKED — needs `DAT-069`/`DAT-070`'s file-side answer (`crates/model/src/file.rs`, owner **D**)
- **Depends on:** DRW-VIEW-006, DAT-055
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**); the `.ccm` field is an **interface
  request** to `crates/model/src/file.rs` (owner **D**) — see the contract note
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct SheetView {
      pub name: String,
      pub frame: ViewFrame,
      pub scale: f64,              // drawing units per paper unit; 1/2 for 1:2
      pub at: Vec2,                // paper-space position of the view's centre
      pub mirrored: bool,
      pub mode: HiddenLinesMode,
      /// The 2D entities this view owns, so updating or deleting it touches nothing else.
      pub owned: std::collections::BTreeSet<cadcraft_doc::Handle>,
      pub base: Option<String>,    // the view this one was projected from
  }
  impl SheetView {
      pub fn sheet_bounds(&self, geom: &ViewGeometry, paper: &cadcraft_doc::PageSetup) -> Result<cadcraft_geom::Bounds2, DrwError>;
  }

  // REQUEST to crates/model (owner D), in `Document`:
  // #[serde(default)] pub sheets: Vec<SheetRecord>,
  // #[serde(default)] pub views:  Vec<ViewRecord>,
  // with the same field names as `SheetView` plus `layout: String` and `revision: u32`.
  // `CadcraftModel::Model` is L3 and may NOT be reached from cadcraft-drawing (L2), so the
  // record lives in the model crate and `cadcraft-drawing` keeps its own copy of the *shape*.
  ```
- **Behaviour:** A view is stored as history (direction + scale + position), never as its projected
  geometry: the sheet regenerates it from the model, which is the only way a parameter change can
  reach the drawing. `owned` is the ownership set that makes `VIEWUPDATE` and `VIEWDELETE` safe —
  the same discipline `crates/doc` uses for `hidden_objects` and `read_notes` (session-only, never
  written). `sheet_bounds` computes the view's paper rectangle from the geometry and the page
  setup's margins; it is what `DRW-VIEW-012` fits.
- **Refusals:** a scale that is not finite and positive is `BadScale`; a name that is empty, over 120
  characters, or collides with another view on the same sheet is `Refused` naming both; a `base`
  that names a view not on the same sheet is `Refused`.
- **Tests:** `a_view_round_trips_through_its_record`. `a_duplicate_name_on_one_sheet_is_refused`.
  `a_scale_of_zero_is_refused`. `sheet_bounds_contains_every_segment`. `owned_is_never_empty_after
  _creation`. `the_record_names_the_sheet_it_belongs_to`.
- **Acceptance:** the interface request is written into the report and into `docs/plan/DAT.md`'s
  v2 field table (`DAT-054`), and **no** `.ccm` field is added by a `DRW` task.
- **Risk:** `Document` has no sheet or view field today and `crates/model/src/file.rs` belongs to
  **D** (master-plan §6). Until it lands, sheets exist only in the 2D drawing's entities and are
  lost on a model-only save. Fallback: the sheet's entities live in the `.dxf`/`.dwg` sheet itself
  (they are ordinary paper-space entities and save today), and `SheetView` is re-derived from them
  by `VIEWRECOVER`; the report says which mode it is in.

### DRW-VIEW-008 — Base view: a solid, a direction and a scale become entities
- **State:** READY
- **Depends on:** DRW-VIEW-006, DRW-XRF-002
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**); `crates/engine/src/cmd/drawing/view.rs`
  (owner **ENG** — co-sequenced)
- **Contract:**
  ```rust
  /// The entities a base view consists of, for the engine to add to `Space::Paper(layout)`.
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ViewEntities {
      pub lines: Vec<LineEnt>,       // { a: Vec2, b: Vec2, kind: EdgeKind }
      pub polylines: Vec<PolylineEnt>,
      pub arcs: Vec<ArcEnt>,
      pub handles_to_tag: Vec<usize>, // indices the engine turns into association handles
  }
  pub fn base_view(part: &ScenePart, frame: &ViewFrame, opts: &ViewOptions) -> Result<ViewEntities, DrwError>;
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct ViewOptions { pub mode: HiddenLinesMode, pub crease_degrees: f64, pub chord_mm: f64 }
  ```
- **Behaviour:** The output is *neutral* (`Vec2` in paper space), not `cadcraft_doc::Entity` — the
  engine adds them and stamps `owned`. This keeps `cadcraft-drawing` free of handle allocation and
  of the drawing's handle space, which is exactly what the layering rule wants. A base view's
  direction is quantised to the model's own axes when within `1e-9` (a "front" view of a part whose
  face lies in `XY`), so the recorded direction is reproducible after a save.
- **Refusals:** a solid with no faces is `Refused("the body has no faces to draw")`; a solid that
  fails `solid.validate()` is refused naming the kernel's error (a drawing never draws a broken
  body); `crease_degrees` outside `0..=180` or `chord_mm <= 0` is `BadScale`/named error.
- **Tests:** `a_cube_base_view_has_twelve_visible_edges`. `a_cylinder_base_view_says_faceted_in_its
  _mode`. `a_broken_solid_is_refused`. `a_crease_angle_of_zero_draws_no_creases`. `a_view_is_drawn
  _twice_identically`.
- **Acceptance:** `VIEWBASE {dir, up, scale, layout}` reaches the same result through the control
  channel as through the command line, and the sheet reports the mode.
- **Risk:** the engine's handle allocation (`Drawing::new_handle`, `crates/doc/src/lib.rs:190`) must
  stay outside this crate, or the same view added twice gets different handles and `owned` breaks.
  Fallback: `ViewEntities` carries no handles at all, which is the plan.

### DRW-VIEW-009 — The six standard views and the isometric, generated from the model's axes
- **State:** READY
- **Depends on:** DRW-VIEW-003, DRW-VIEW-008
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**); `crates/model/src/view.rs` (owner **D**)
  is the consumer of `DAT-066`'s `SavedView` — **read only**, no edit
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum StandardView { Top, Bottom, Front, Back, Left, Right, Isometric }
  impl StandardView {
      pub fn frame(&self, model_axes: &[Vec3; 3], target: Vec3) -> ViewFrame;
      pub fn parse(s: &str) -> Option<StandardView>;   // "top", "front", "iso", …
      pub fn name(self) -> &'static str;
      /// The direction to use for an auxiliary view of `edge`, or the refusal reason.
      pub fn auxiliary_from(edge_dir: Vec3, up: Vec3) -> Result<ViewFrame, DrwError>;
  }
  ```
- **Behaviour:** Six views along the model's ±X/±Y/±Z plus the isometric at the standard
  `(1,1,1)` direction, all built from the **model's own axes** so a part authored in its own
  coordinate system draws the way its author meant. The `is_top`/`is_front`/… flags in
  `DAT-066`'s `SavedView` exist for exactly this and are reused (read-only) rather than re-decided.
  The isometric's `up` is `+Z` projected, which is the convention the sample drawings use.
- **Refusals:** `parse` of an unknown name is `None`, never a default. `auxiliary_from` on an edge
  direction within `1e-9` of the `up` vector is `Refused("the auxiliary direction is parallel to
  `up`")`. A `model_axes` basis that is not orthonormal is refused.
- **Tests:** `each_standard_view_looks_down_its_axis` — asserts the dot product of `dir` with the
  named axis is `±1` within `1e-12`. `the_six_views_are_the_six_signed_axes`. `an_unknown_name_is
  _none`. `auxiliary_from_is_perpendicular_to_the_edge`. `a_degenerate_up_is_refused`.
- **Acceptance:** `docs/geometry-parity.md`'s view row names the seven views and the convention;
  `cargo xtask parity` counts the seven menu items once they are registered (`DRW-VIEW-017`).
- **Risk:** a part whose "front" is not its `XY` face surprises a user. Fallback: the axis names are
  a *default*, and `VIEWBASE {dir}` always overrides them; the sheet's view label prints the
  direction's numbers so the drawing is self-describing.

### DRW-VIEW-010 — Projected views from a base view, and the 3D representation they come from
- **State:** READY
- **Depends on:** DRW-VIEW-008, DRW-VIEW-003
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn projected_view(parent: &SheetView, child_dir: Vec3, child_up: Option<Vec3>,
                        angle: ProjectionAngle, gap: f64, at: Option<Vec2>)
      -> Result<(ViewFrame, f64 /*scale*/, Vec2 /*at*/, bool /*mirrored*/), DrwError>;
  /// The 3D representation a view is projected from: the model's features up to `feature`.
  pub fn projection_source<'a>(model: &'a cadcraft_model::Model, feature: Option<&str>)
      -> Result<Vec<ScenePart>, DrwError>;   // needs `cadcraft-model` (L3) — see the note
  ```
- **Behaviour:** The offset and mirroring come from `projected_offset` (`DRW-VIEW-003`), the scale
  is inherited from the parent unless `gap`/`at` say otherwise, and the child's `up` defaults to
  the parent's `up` projected perpendicular to the new direction. **Note:** `projection_source`
  takes `&cadcraft_model::Model`, which is L3 and therefore **cannot** live in `cadcraft-drawing`
  (L2). Its real body belongs to `crates/engine/src/cmd/drawing/` (owner **ENG**); the version here
  is the seam that says what the engine must gather: one `ScenePart` per body, with `ModelError`
  converted to `DrwError::Refused`, and a feature that failed is reported, not skipped.
- **Refusals:** a child direction equal to the parent's is refused; a model with no built solid is
  `Refused("the model has no solid to project: <feature>: <message>")`, never the last good solid
  silently; a feature name that does not exist is refused naming it.
- **Tests:** `a_projected_view_sits_where_the_convention_says`. `the_child_inherits_the_parent
  _scale`. `a_child_direction_equal_to_the_parent_is_refused`. `a_model_with_no_solid_is_refused`.
  `a_failed_feature_is_reported_not_skipped` — uses the last-good rule of
  `crates/model/src/rebuild.rs`.
- **Acceptance:** the projection-source gather is an interface request to **ENG**, recorded in the
  report; no `DRW` file names `cadcraft_model`.
- **Risk:** a 2D↔3D split that puts the "gather" in the engine and the "draw" in the crate means
  the engine needs a small adapter for every view kind. Fallback: one adapter
  (`ENG::gather_parts(model, feature) -> Vec<ScenePart>`) used by all of them, which is what
  `projection_source`'s doc requires.

### DRW-VIEW-011 — Scale denominators, and the single model→paper factor
- **State:** READY
- **Depends on:** DRW-VIEW-002
- **Files:** `crates/drawing/src/scale.rs` (owner **DRW**); reuse `crates/doc/src/anno.rs` (read
  only)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct ViewScale(pub f64);        // drawing units per paper unit; 1/2 == 1:2, 2 == 2:1
  impl ViewScale {
      pub fn parse(s: &str) -> Option<ViewScale>;      // "1:2", "2:1", "1", "10:1", "1/2"
      pub fn factor(self) -> f64;                      // guarded: 1.0 for a non-positive
      pub fn is_valid(self) -> bool;
      pub fn display(self) -> String;                  // "1:2", "2:1"
      /// Paper millimetres per model millimetre, at `paper_unit_mm`.
      pub fn paper_mm_per_model_mm(self, paper_unit_mm: f64) -> Result<f64, DrwError>;
      /// A size in the document's drawing units, shown on paper.
      pub fn to_paper(self, v: f64) -> f64;
  }
  pub fn view_scale_for_fit(size: Vec2, printable: Bounds2, candidates: &[ViewScale])
      -> Result<ViewScale, DrwError>;
  ```
- **Behaviour:** `ViewScale` is a newtype over a factor, not a second scale *type*: the annotation
  scale of a view is `AnnoScale { paper_units: 1, drawing_units: 1/factor }`, so the view's scale and
  the drawing's annotation scale are the same number and `anno_scaled` (`crates/doc/src/anno.rs:154`)
  applies to a view's text at its own factor without a special case. `view_scale_for_fit` walks a
  stated candidate list (the drawing's own `scales`, `crates/doc/src/lib.rs:115`) and picks the
  largest that fits — i.e. the largest scale *number* ≤ the fitting scale, which is what AutoCAD's
  scale-to-fit does.
- **Refusals:** `parse` of `"0:0"`, `"1:0"`, `"a:b"` and `"1:0.5"` is `None` (a non-integer
  denominator is not a scale); a factor that is not finite and positive is `BadScale`; a paper unit
  that is not finite and positive is refused naming it; `view_scale_for_fit` with an empty candidate
  list is `Refused("no scale in the drawing's scale list fits")`.
- **Tests:** `one_to_two_parses_and_displays`. `two_to_one_is_a_doubling`. `a_scale_survives_a_round
  _trip_through_its_display`. `zero_and_inverted_scales_are_none`. `a_view_scale_is_an_annotation
  _scale` — the factor matches `AnnoScale::factor()` for both. `fit_picks_the_largest_that_fits` and
  `the_previous_smaller_scale_also_fits`. `a_scale_smaller_than_printable_still_fits`.
- **Acceptance:** `DRW-DIM-009` and `DRW-SHE-007` use `ViewScale::factor()` and no other code
  multiplies by a scale.
- **Risk:** a scale factor applied twice (once for geometry, once for text) is the classic drawing
  bug. Fallback: `ViewEntities` is produced in **view space** and multiplied by `paper_matrix`
  exactly once, at the engine; a test asserts a view's text height at 1:2 is half its paper size.

### DRW-VIEW-012 — Fit a view into the printable area, with the margins and centring stated
- **State:** READY
- **Depends on:** DRW-VIEW-005, DRW-VIEW-011
- **Files:** `crates/drawing/src/scale.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The paper-space rectangle a sheet's content may occupy, from `PageSetup`.
  pub fn printable_rect(page: &cadcraft_doc::PageSetup) -> Result<Bounds2, DrwError>;
  pub fn fit_into(geom_size: Vec2, printable: Bounds2, prefer: Option<ViewScale>, gap: f64)
      -> Result<(ViewScale, Vec2 /*size on paper*/), DrwError>;
  pub fn place_at(centred: bool, size: Vec2, printable: Bounds2) -> Vec2;
  ```
- **Behaviour:** `printable_rect` is `(0,0)-(w,h)` minus `margins_mm` converted through the paper
  unit, i.e. `crates/render/src/paper.rs`'s rule recomputed from `PageSetup`'s own fields — because
  `cadcraft-drawing` may not call `render` (invariant 7) and must not have two paper-unit
  conversions, so the conversion helper is one line calling the same `paper_unit_mm` rule. A view
  is fitted by its **sheet rectangle**, which includes its view label and dimension extents
  (`DRW-SHE-007`), never by the projected geometry alone: a view whose geometry fits but whose
  dimensions do not is the standard failure this prevents.
- **Refusals:** a page with non-finite or non-positive `width_mm`/`height_mm` is
  `Refused("page setup: paper size {w} x {h} mm is not a size")`; margins that leave no printable
  area (sum of left+right ≥ width) is `Refused("margins … leave no printable area")` naming the
  numbers; a geometry with a non-finite extent is refused.
- **Tests:** `margins_are_subtracted_from_all_four_sides`. `landscape_swaps_the_printed_rect` —
  `landscape: true` swaps width/height, as `PageSetup` documents. `margins_that_consume_the_sheet
  _are_refused`. `fit_returns_the_largest_scale_that_leaves_the_gap`. `place_at_centres_and_otherwise
  _uses_the_lower_left`. `a_fit_is_independent_of_the_view_position`.
- **Acceptance:** the same numbers appear in `printable_rect` and in the plot area DXF writes
  (`crates/io`'s layout writer); a test compares them by re-reading a written DXF's paper space.
- **Risk:** `PageSetup.scale` (paper units per drawing unit) and `ViewScale` are two fields meaning
  similar things. Fallback: the task states the rule — `PageSetup.scale` is the *sheet's* scale for
  a plot-to-fit page setup and `ViewScale` is a *view's* scale; when `scale_to_fit` is true the view
  scale wins and `PageSetup.scale` is reported as ignored.

### DRW-VIEW-013 — Detail views: a boundary on a parent view, a zoom, and the boundary drawn
- **State:** READY
- **Depends on:** DRW-VIEW-012, KER-BOOL-010
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum DetailBoundary { Circle { centre: Vec2, radius: f64 }, Box { min: Vec2, max: Vec2 } }
  pub fn detail_view(parent: &SheetView, boundary: DetailBoundary, scale: ViewScale, at: Vec2)
      -> Result<SheetView, DrwError>;
  /// What the boundary selects in the model, so the detail is drawn from real geometry.
  pub fn detail_source(part: &ScenePart, parent: &ViewFrame, boundary: DetailBoundary)
      -> Result<ScenePart, DrwError>;
  ```
- **Behaviour:** The detail is a second projection of a **clipped** copy of the solid, so it shows
  real geometry rather than an enlargement of the parent's pixels: the clipped `ScenePart` is a real
  boolean result (`cadcraft_solid::boolean` / a cut with a prism over the boundary), which is why it
  depends on `KER-BOOL-010`'s re-exported `section`/`boolean` surface. The boundary circle/box is
  drawn on the parent view in its own layer, and the detail is labelled with its scale.
- **Refusals:** a boundary not contained in the parent's projected geometry is
  `Refused("the detail boundary lies outside its parent view")`; a boundary smaller than
  `tolerance_for(parent diagonal)` is `Refused` naming both numbers; a detail whose scale is **finer**
  than its parent's (`scale >= parent.scale`, i.e. fewer drawing units per paper unit) is
  `Refused("a detail view must draw its region larger than the parent shows it")` with both numbers
  — a detail that shows *less* is not a detail.
- **Tests:** `a_detail_shows_the_clipped_geometry_not_a_zoom`. `the_boundary_is_drawn_on_the_parent`.
  `a_boundary_outside_the_parent_is_refused`. `a_detail_scale_finer_than_its_parent_is_refused`.
  `a_box_boundary_clips_with_the_same_rule_as_a_circle`. `a_detail_of_a_faceted_cylinder_says_faceted`.
- **Acceptance:** a detail view's geometry equals the parent's geometry clipped to the boundary, to
  `tolerance_for(diagonal)`; the test is the acceptance.
- **Risk:** clipping with a curved boundary needs a curved prism, which the kernel can only make
  faceted today. Fallback: a circular boundary clips with a faceted prism (segment count from
  `limits::MAX_ROUND_SEGMENTS`) and the view records `HiddenLinesMode::Faceted`; the exact boundary
  waits for `KER-SRF-018`/`KER-SRF-035`.

### DRW-VIEW-014 — Auxiliary views: perpendicular to an edge, and the refusal when the edge is curved
- **State:** BACKLOG
- **Depends on:** DRW-VIEW-009, KER-SRF-015
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn auxiliary_from_edge(edge: &[Vec3; 2], up_hint: Vec3) -> Result<ViewFrame, DrwError>;
  /// Needs KER-SRF-015 (`Curve3`): the direction of a curved edge at its midpoint.
  // pub fn auxiliary_from_curve(c: &cadcraft_solid::Curve3) -> Result<ViewFrame, DrwError>;
  ```
- **Behaviour:** The direction of a straight edge, the `up` from the parent view, and the label
  carrying the direction's degrees. The commented line is the shape the task will take once
  `KER-SRF-015` exports `Curve3` — it is written out here as an **interface request**, not as code.
- **Refusals:** an edge shorter than `tolerance_for` is refused naming both lengths; a direction
  parallel to `up_hint` is refused; a curved edge is `Err(NeedsKernel("an auxiliary view of a curved
  edge needs the kernel's Curve3 (KER-SRF-015)"))` — the named refusal, never a chord.
- **Tests:** `an_auxiliary_view_is_perpendicular_to_its_edge`. `a_degenerate_edge_is_refused`.
  `an_up_parallel_to_the_edge_is_refused`. `a_curved_edge_returns_the_needs_kernel_error`.
- **Acceptance:** the `NeedsKernel` error names the task id, so a reader can see exactly what is
  missing and when it lands.
- **Risk:** the refusal means a curved edge gets no auxiliary view, which is a real gap in a part
  full of fillets. Fallback: auxiliary of a *faceted* edge (the kernel's own prism edge) works
  today, because that edge is straight; the mode is recorded as `Faceted`.

### DRW-VIEW-015 — Half views and broken-out views, bounded
- **State:** BACKLOG
- **Depends on:** DRW-VIEW-005, KER-BOOL-006
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub enum PartialView {
      /// Half the view, cut along a vertical or horizontal line through its centre.
      Half { horizontal: bool },
      /// A closed boundary polygon, removed, with the cut face hatched.
      BrokenOut { boundary: Vec<Vec2>, max_vertices: usize },
  }
  pub fn partial_view(geom: &ViewGeometry, part: &ScenePart, p: PartialView, hatch: HatchStyle)
      -> Result<(ViewGeometry, Option<cadcraft_geom::Region>), DrwError>;
  ```
- **Behaviour:** A broken-out view removes material in the view plane: the region is cut from the
  solid with the existing 2D→3D machinery (a prism over the boundary polygon), and the exposed cut
  face is drawn **hatched** with the drawing's own hatch library (`crates/doc/src/library.rs`,
  `Hatch` entity + `render::hatch`). `max_vertices` is a real cap (default 128) because the cut is
  a boolean and booleans are quadratic in the vertex count.
- **Refusals:** a boundary that is not closed, self-intersecting, or above `max_vertices` is
  `Refused` naming the count; a boundary outside the view's geometry is refused; a boolean that
  fails is `Refused` with the kernel's own error, and the view falls back to **the whole view** with
  the reason recorded in the report, never to a half-drawn view.
- **Tests:** `a_broken_out_view_removes_the_region`. `the_cut_face_is_hatched`. `a_self_intersecting
  _boundary_is_refused`. `a_boundary_over_the_cap_is_refused`. `a_failed_boolean_draws_the_whole
  _view_and_says_why`. `a_half_view_is_mirrored_correctly`.
- **Acceptance:** the hatch pattern is one of `crates/doc/src/library.rs`'s own patterns, drawn by
  `symbol_prims`-style code, and `cargo xtask assets` has nothing to say about it (no file).
- **Risk:** the cut's boolean inherits the kernel's three open defects (`KER-BOOL-001..003`), so a
  broken-out view of a revolved part can fail. Fallback: the refusal path above, which is the
  last-good rule applied to a drawing.

### DRW-VIEW-016 — Update a view after a model rebuild, and report what moved
- **State:** BLOCKED — needs `MOD-005`'s rebuild record (`crates/model/src/rebuild.rs`, owner **MOD**)
- **Depends on:** DRW-VIEW-008, MOD-REC-001
- **Files:** `crates/drawing/src/view.rs` (owner **DRW**); the gather side is `ENG`
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ViewUpdate {
      pub view: String,
      pub edges_before: usize,
      pub edges_after: usize,
      pub unresolved: Vec<String>,     // 3D targets that no longer resolve
      pub kept_last_good: bool,        // the model was not rebuilt; the drawing says so
      pub failures: Vec<String>,       // per-feature reasons from the rebuild record
  }
  pub fn update_view(view: &SheetView, part: &ScenePart, frame: &ViewFrame) -> Result<ViewGeometry, DrwError>;
  ```
- **Behaviour:** The view regenerates from the model and **replaces only its `owned` handles**; every
  other entity on the sheet is untouched, which is what makes updating a view safe next to hand-made
  annotation. A model whose rebuild failed keeps the **last good solid** (`crates/model/src/rebuild.rs`,
  `Record::last_good`) and the update reports `kept_last_good: true` — the sheet is never silently
  drawn from a shape the user did not approve.
- **Refusals:** a view whose `owned` set contains a handle that is no longer in the drawing is
  reported, not repaired (`owned` is stale); a handle that has been re-used by another entity (a
  handle that exists but is not one of the view's) is `Refused("handle {h} no longer belongs to view
  {v}")` — the ownership check is what stops an update from eating a user's line.
- **Tests:** `updating_replaces_exactly_the_owned_handles`. `a_user_entity_next_to_a_view_survives`.
  `a_failed_rebuild_keeps_the_last_good_and_says_so`. `a_stale_owned_handle_is_reported`. `a_handle
  _reused_by_a_foreign_entity_is_refused`. `update_is_idempotent`.
- **Acceptance:** `VIEWUPDATE` on a sheet with hand annotation changes only the views; the test
  asserts the entity count and the untouched handles.
- **Risk:** ownership by handle is only as good as the handle space, and the 2D drawing's handle
  space is the drawing's. Fallback: every owned entity also carries the view's name in its
  `Common` layer? No — layer is user-visible. It carries the view name in `foreign`/xdata
  (`crates/doc/src/foreign.rs`), which is the `#528` "keep what we do not model" channel, and the
  report says the fallback is a *recovery* read, not the primary key.

### DRW-VIEW-017 — The view layer scheme, and the commands that reach it
- **State:** READY
- **Depends on:** DRW-VIEW-008
- **Files:** `crates/engine/src/cmd/drawing/view.rs` (owner **ENG** — co-sequenced); the layer
  names are data, in `crates/drawing/src/layers.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The layers a drawing creates, in creation order. `Default` layers are not touched.
  pub const VIEW_LAYERS: &[(&str, LineweightKinds)] = &[
      ("cadcraft-visible", LineweightKinds::Visible),
      ("cadcraft-hidden",   LineweightKinds::Hidden),
      ("cadcraft-outline",  LineweightKinds::Outline),
      ("cadcraft-centre",   LineweightKinds::Centre),
      ("cadcraft-section",  LineweightKinds::Section),
      ("cadcraft-hatch",    LineweightKinds::Hatch),
      ("cadcraft-dim",      LineweightKinds::Dimension),
  ];
  pub fn ensure_view_layers(d: &mut cadcraft_doc::Drawing) -> Vec<String>;   // created names only
  ```
  `CommandSpec` ids (owner **ENG**): `view.base`, `view.project`, `view.standard`,
  `view.detail`, `view.update`, `view.delete`, `view.list`, each `params` documented and each
  reachable from the menu and the control channel like every other command (`AGENTS.md`).
- **Behaviour:** Views draw on their own layers so a user can freeze, colour or plot them apart, and
  so the hidden-line linetype has somewhere to live. `ensure_view_layers` creates what is missing and
  returns only what it created, so it is idempotent and never renames or recolours a layer the user
  already had. Lineweights come from `cadcraft_doc::Lineweight` (`entity.rs:28-40`), which already
  has the 24 standard values.
- **Refusals:** a layer name that a user already uses with a different meaning is **reused**, not
  overwritten, and the report says so; `ensure_view_layers` refuses to change a frozen or locked
  layer and returns `Refused` naming it.
- **Tests:** `ensure_view_layers_is_idempotent`. `an_existing_layer_is_not_modified`. `the_returned
  _list_names_only_what_was_created`. `every_view_kind_has_a_layer`. `a_frozen_layer_is_refused`.
  `the_commands_appear_in_engine_commands`.
- **Acceptance:** `cargo xtask parity` counts the new menu items; the layers carry no asset (they
  are names and lineweights), so `cargo xtask assets` is unaffected.
- **Risk:** eight new layers in every drawing is clutter for a 2D-only user. Fallback: the layers
  are created on first use of the first view, not on drawing creation.

### DRW-VIEW-018 — Determinism and hostile input for the whole view path
- **State:** READY
- **Depends on:** DRW-VIEW-008, DRW-XRF-002
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn projection_is_deterministic(part: &ScenePart, frame: &ViewFrame) -> bool;` and
  the module's `hostile_views_never_panic()`.
- **Behaviour:** Two runs of the same projection produce byte-identical entity sequences, including
  the order of `ViewEdge`s and the order of the `regions`. Hostile input: a `NaN` direction, a
  zero-length `up`, a solid with a face at `1e300`, a solid with a 1e-300 diagonal, a solid with
  `MAX_FACES + 1` faces (hand-built, bypassing the cap), an empty `ScenePart`, a view scale of
  `1e-300` and of `1e300` — all must return `Ok` or `Err`, none may panic, index out of bounds or
  produce a NaN coordinate.
- **Refusals:** `Limit` is returned for the face/edge/raster caps with the count and the cap in the
  message; `Refused` for a non-finite scale; `BadDirection`/`BadUp` for the frame.
- **Tests:** `hostile_views_never_panics` (200 mutations plus the nine hand-built cases above).
  `projection_is_deterministic` — the same `Vec<ViewEdge>` twice, compared field by field, and the
  two runs' `to_string()` compared. `a_solid_at_the_face_cap_projects` and `one_past_it_is_a_limit
  _error`. `no_projected_coordinate_is_nan` — asserted over every output of the hostile run.
- **Acceptance:** this task is the epic's gate: `DRW-VIEW` is not done while it fails, and it is the
  test the master plan's rule (survives randomized input, refuses what it cannot make) asks for.
- **Risk:** the hostile solid with `MAX_FACES+1` faces must be built by hand, because every
  constructor refuses it. Fallback: build it by cloning a valid face and pushing it directly into a
  `Vec<Face>` before wrapping in `ScenePart`, which is what the task says.

---

# DRW-SHE — a sheet is a layout: page setup, border, title block, parts list, revision

A sheet is not a new container. It is a `cadcraft_doc::Layout` with a `PageSetup`, a border, a title
block, some views (`DRW-VIEW`) and some annotation. Everything that already draws paper space —
`render::build_plot`, `crates/io`'s layout writer, `plot`, the plot styles, the hatch and dimension
code — draws it without knowing it came from a 3D model.

### DRW-SHE-001 — `Sheet`: the layout, its page setup and its printable area, as one type
- **State:** READY
- **Depends on:** DRW-VIEW-012
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Sheet {
      pub layout: String,                    // the Layout's name; the sheet's identity
      pub page: cadcraft_doc::PageSetup,
      pub title_block: TitleBlock,
      pub views: Vec<SheetView>,
      pub notes: Vec<SheetNote>,
  }
  impl Sheet {
      pub fn new(layout: &str) -> Sheet;     // `Layout::new` defaults; the page is the caller's
      pub fn printable(&self) -> Result<cadcraft_geom::Bounds2, DrwError>;
      /// Paper millimetres per drawing unit for this sheet — the ONE unit conversion.
      pub fn paper_unit_mm(&self) -> Result<f64, DrwError>;
      pub fn validate(&self) -> Result<Vec<String>, DrwError>;   // findings, never a repair
  }
  ```
- **Behaviour:** `Sheet` is a **view** of an existing layout, not a container: it borrows the
  `PageSetup` the layout already carries (`crates/doc/src/tables.rs:731`), so `PLOT`, `PUBLISH` and
  the DXF/DWG layout writer keep working with no change. `paper_unit_mm` follows
  `crates/render/src/paper.rs:80`'s rule (`$INSUNITS`/measurement) recomputed from `PageSetup`, for
  the layering reason in invariant 7 — and a test asserts the two agree.
- **Refusals:** a layout name that is empty or over 255 characters is `Refused`; a page with
  non-finite size is `Refused` naming the field; `paper_unit_mm` for a non-positive unit is
  `Refused("the sheet's paper unit … is not a length")`.
- **Tests:** `a_sheet_borrows_the_layouts_page_setup`. `printable_rect_matches_the_renderers_rule` —
  the same numbers as `render::paper::printable` for the eight standard paper sizes. `validate
  _returns_findings_without_repairing`. `a_bad_page_is_refused`. `a_sheet_round_trips_through_its
  _fields`.
- **Acceptance:** `cargo xtask layers` still shows `cadcraft-drawing` at L2 with `geom, solid, doc`;
  the paper-unit test is the proof that not calling `render` did not fork the rule.
- **Risk:** two implementations of `paper_unit_mm` in two crates will drift. Fallback: the test above
  is the contract; if it ever fails the fix is in `PageSetup`, not in `DRW`.

### DRW-SHE-002 — Creating a sheet: paper choice, orientation, and the standard sizes
- **State:** READY
- **Depends on:** DRW-SHE-001
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**); `crates/engine/src/cmd/drawing/sheet.rs`
  (owner **ENG**)
- **Contract:**
  ```rust
  /// The standard paper names CADCraft offers, in the order the dialog lists them.
  pub fn paper_choices() -> &'static [(&'static str, f64, f64)];   // (name, width_mm, height_mm)
  pub fn set_paper(sheet: &mut Sheet, paper: &str, landscape: bool) -> Result<(), DrwError>;
  ```
  `CommandSpec` ids (owner **ENG**): `sheet.new {layout?, paper?, landscape?}`, `sheet.list`,
  `sheet.delete {layout}`.
- **Behaviour:** `paper_choices` is our own list of the ISO A/B series and the ANSI A/B/C/D/E sizes
  in millimetres — data, not assets, so `cargo xtask assets` has nothing to check and no font or
  template is copied. `set_paper` writes `width_mm`/`height_mm`/`landscape` on the `PageSetup`,
  matching the field docs ("Paper size in millimetres, portrait width/height before rotation"), so
  `layout: true` swaps them at plot time exactly as the existing plotter does.
- **Refusals:** a paper name not in the list is `Refused("unknown paper `{n}`; try one of …")` with
  the three nearest names, not a silent ISO-A; a paper with a non-positive side is `Refused`; a
  sheet that already has views and gets a **smaller** paper is allowed but the views are reported as
  out of area by `DRW-SHE-016`, never moved silently.
- **Tests:** `the_standard_sizes_match_their_standards` — A4 210×297, A3 420×297, ANSI A 215.9×279.4,
  ANSI C 431.8×279.4, each asserted with the standard named in the message. `landscape_swaps_the
  _stored_size_at_plot_time`. `an_unknown_paper_names_the_nearest_three`. `setting_a_paper_is
  _idempotent`. `sheet_new_and_delete_round_trip`.
- **Acceptance:** `PLOT` of a sheet created by `sheet.new` produces a page of exactly the requested
  size, asserted by reading the written PDF's `/MediaBox` (existing plot path) in the export task
  (`DRW-EXP-008`).
- **Risk:** the same paper names in `docs/plotters.md` and `docs/parity-checklist.md` could drift.
  Fallback: a test asserts the parity checklist's paper names resolve through `paper_choices`.

### DRW-SHE-003 — The border and the printable frame, with the margin rules
- **State:** READY
- **Depends on:** DRW-SHE-001
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Border {
      pub outer_inset: f64,   // drawing units inside the paper edge
      pub inner_inset: f64,   // drawing units from the outer line to the frame
      pub corners: bool,      // the trimming marks
  }
  pub fn border_lines(sheet: &Sheet, b: Border) -> Result<(Vec<([Vec2; 2])>), DrwError>;
  ```
- **Behaviour:** Two rectangles (outer, inner) plus optional corner marks, drawn on the
  `cadcraft-outline` layer's sheet sibling (`cadcraft-visible`). They are ordinary 2D `Line`
  entities added to the layout's paper space, so they plot, save to DXF/DWG and print through the
  existing path with no new code. The insets are in **paper millimetres** converted once, because a
  border is a paper-space object and its thickness must not scale with the view scale.
- **Refusals:** an inset larger than the printable area's smaller side is `Refused` naming both; a
  negative or non-finite inset is `Refused`; a paper whose margins already leave less than
  `outer + inner` is `Refused("margins and border do not fit")` with the four numbers.
- **Tests:** `the_border_is_two_rectangles`. `insets_are_millimetres_not_paper_units`. `corner_marks
  _appear_only_when_asked`. `a_border_that_does_not_fit_is_refused`. `the_border_does_not_move_with
  _the_view_scale`.
- **Acceptance:** the border entities survive a save/load of the `.dxf` sheet and are byte-identical
  on a second export.
- **Risk:** the border is stored twice (a `Sheet` field and as entities). Fallback: the entities are
  the truth; `Sheet.border` is a convenience for redrawing, and `sheet.list` reports whether the
  border entities are present.

### DRW-SHE-004 — The title block: its fields, and it is a block, not a table type of its own
- **State:** READY
- **Depends on:** DRW-SHE-003, DRW-VIEW-007
- **Files:** `crates/drawing/src/titleblock.rs` (owner **DRW**); the metadata fields are
  **interface requests** to `cadcraft_model::DocMeta` (`crates/model/src/file.rs`, owner **D**,
  `DAT-070`)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct TitleBlock {
      pub size: Vec2,                    // paper units, from the bottom-right corner
      pub fields: BTreeMap<TitleField, String>,
  }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
  pub enum TitleField {
      Company, Title, DrawnBy, CheckedBy, ApprovedBy, Date, Scale, Sheet,
      DrawingNumber, Revision, Material, Finish, Weight, Projection, Units, Model,
  }
  impl TitleBlock {
      /// Cells as a `Table`'s geometry, in the entity's own coordinate frame.
      pub fn cells(&self, meta: &DocMetaLike) -> Vec<(TitleField, Bounds2, String)>;
  }
  /// The subset of `cadcraft_model::DocMeta` a title block needs. Declared here so
  /// cadcraft-drawing (L2) never names cadcraft-model (L3); the engine converts.
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct DocMetaLike {
      pub title: String, pub author: String, pub company: String,
      pub part_number: String, pub revision: String, pub modified: String,
  }
  ```
- **Behaviour:** The title block is a **`cadcraft_doc::Block`** (`crates/doc/src/tables.rs:647`)
  inserted once per sheet: a block reference is what lets the user move the whole thing, and it is
  the entity the DXF/DWG writer already writes. The cell rectangles and text come from the existing
  `Table`/`TableCell`/`TableStyle` machinery (`crates/render/src/table.rs`), so the borders are
  plot-style-driven lineweights like every other table. `TitleField` is an ordered `BTreeMap`, so a
  title block's fields always read in the same order — the determinism rule.
- **Refusals:** a field value over 255 characters is truncated **with the name in the report**, not
  refused (a title is not worth failing a sheet over — the same rule `DAT-070` states for `DocMeta`);
  a block name collision is `Refused` naming both; a title block larger than the printable area is
  `Refused` with both sizes.
- **Tests:** `the_title_block_is_one_block_reference`. `cells_are_in_field_order`. `a_long_value_is
  _truncated_and_reported`. `a_title_block_larger_than_the_sheet_is_refused`. `the_block_is_an
  _ordinary_insert_so_it_saves_to_dxf`. `a_second_sheet_gets_its_own_block_name`.
- **Acceptance:** the block is written to a `.dxf` and read back with its attribute values intact
  (`crates/io`'s block writer and `ATTRIB` reader already do this).
- **Risk:** a title block as a block means editing a field edits the block definition, which affects
  every reference — correct for identical sheets, wrong when two sheets differ. Fallback: one block
  per distinct field set, named from a hash of the values (`DRW-SHE-005`).

### DRW-SHE-005 — One block per distinct title block, named deterministically
- **State:** READY
- **Depends on:** DRW-SHE-004
- **Files:** `crates/drawing/src/titleblock.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn title_block_name(meta: &DocMetaLike) -> String;      // "*T" + fnv1a64 of the values
  pub fn ensure_title_block(d: &mut cadcraft_doc::Drawing, sheet: &str, tb: &TitleBlock,
                            meta: &DocMetaLike) -> Result<cadcraft_doc::Handle, DrwError>;
  ```
- **Behaviour:** Sheets with identical title-block content share one block; sheets that differ get
  their own. The name is `*T` plus a 64-bit FNV-1a hash of the field values in `TitleField` order,
  so it is stable across saves, across machines and across field edits (a changed field changes the
  name). `anonymous_block_name` (`crates/doc/src/lib.rs:371`) is the naming convention reused.
- **Refusals:** a hash collision between two different field sets is detected by comparing the
  stored values and the block is renamed with a counter suffix — never silently shared; a block name
  already used by a *user* block (`Refused` naming it, since `*` names are conventionally
  anonymous).
- **Tests:** `identical_titles_share_one_block`. `one_different_field_creates_a_second_block`. `a
  _collision_is_detected_and_disambiguated`. `the_name_is_stable_across_two_runs`. `a_user_block
  _with_the_same_name_is_refused`.
- **Acceptance:** the hash is over values only, so a title block that has not changed is not
  rewritten; a test asserts the block's `last modified` equivalent (the definition's contents) is
  identical after a no-op save.
- **Risk:** an FNV hash in a name is opaque to a user browsing block names. Fallback: the name is
  `*T_ab12cd34` (a prefix plus 8 hex digits) and `BLOCK`/`-BLOCK` listing shows it with its
  description set to the title.

### DRW-SHE-006 — Where a view sits on the sheet, and the report when two overlap
- **State:** READY
- **Depends on:** DRW-VIEW-012, DRW-SHE-003
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Placement { pub at: Vec2, pub size: Vec2 }
  pub fn view_rect(v: &SheetView, geom: &ViewGeometry, label: Vec2) -> Result<Bounds2, DrwError>;
  pub fn overlaps_sheet(a: &Sheet, b: &Sheet) -> Vec<String>;   // view names, sorted
  pub fn layout_views(a: &mut Sheet, gap: f64, cols: Option<usize>)
      -> Result<Vec<(String, Vec2)>, DrwError>;                  // placed positions
  ```
- **Behaviour:** A view occupies its geometry rectangle **plus** its label and its dimension
  extents, and the rectangle is what is fitted, placed and checked for overlap. `layout_views` is a
  simple shelf/row packer with a stated gap, deterministic (views are laid out in the sheet's own
  order), and it returns the positions rather than applying them, so the caller can preview.
- **Refusals:** a view whose rectangle is not finite or not positive is `Refused` naming the view;
  a `gap` that is negative is `Refused`; a layout that cannot fit (`cols * row_height > printable`)
  is `Refused` with the numbers rather than a scale change the user did not ask for.
- **Tests:** `the_rectangle_includes_the_label_and_the_dimensions`. `overlap_is_reported_and_sorted`.
  `layout_views_is_deterministic`. `an_overfull_layout_is_refused`. `a_gap_of_zero_is_allowed`.
- **Acceptance:** `cargo xtask parity` sees the "Lay out views" menu item; the packer's output is
  asserted against a hand-computed layout in the test.
- **Risk:** auto-layout moves views a user placed by hand. Fallback: `layout_views` returns
  positions and never mutates; `SHEETLAYOUT {views?}` applies only what the caller passes, and a view
  the user placed keeps its position unless named in `views`.

### DRW-SHE-007 — Scale consistency across a sheet, and what to do about a mismatch
- **State:** READY
- **Depends on:** DRW-VIEW-011, DRW-SHE-006
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum SheetScaleVerdict {
      Uniform(ViewScale),
      Mixed { view: String, scale: ViewScale },
      DetailOnly { base: ViewScale, details: Vec<String> },   // allowed
  }
  pub fn check_scales(sheet: &Sheet) -> Result<SheetScaleVerdict, DrwError>;
  pub fn unify_scales(sheet: &mut Sheet, target: ViewScale) -> Result<Vec<String>, DrwError>;
  ```
- **Behaviour:** ISO practice is one scale per sheet, exceptions being details. The verdict is a
  three-state answer, so a sheet of details plus one base is *not* an error. `unify_scales` rescales
  and re-places each view (`at` moves by the ratio so the views keep their relative arrangement) and
  returns the names it changed.
- **Refusals:** a target scale that is not in the drawing's scale list is `Refused` naming the list;
  a view whose scale would need a re-fit is reported by name, not auto-refitted (a refit changes the
  drawn size, which is a user decision).
- **Tests:** `a_uniform_sheet_is_uniform`. `a_mixed_sheet_names_the_odd_view`. `details_may_differ`.
  `unify_rescales_and_repositions`. `an_unknown_target_is_refused`. `unify_is_idempotent`.
- **Acceptance:** the verdict is printed in `sheet.list` and in the sheet's own report.
- **Risk:** auto-unifying would silently rescale a drawing someone has annotated. Fallback:
  `unify_scales` is a command (`SHEETUNIFY {scale}`) that reports first; nothing applies it
  implicitly.

### DRW-SHE-008 — Annotation scale on a sheet: a view's scale is its annotation scale
- **State:** READY
- **Depends on:** DRW-VIEW-011, DRW-SHE-007
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**); read-only reuse of
  `crates/doc/src/anno.rs`
- **Contract:**
  ```rust
  /// The `AnnoScale` a view at `v` contributes, and the factor the renderer multiplies text by.
  pub fn view_anno_scale(v: &SheetView) -> cadcraft_doc::AnnoScale;
  pub fn sheet_anno_factor(view: &SheetView, sheet: &Sheet) -> Result<f64, DrwError>;
  ```
- **Behaviour:** A view at 1:2 draws its text at half paper size **relative to the drawing's
  1:1 annotation scale** — which is what `anno_scaled` (`crates/doc/src/anno.rs:154`) already does
  with a `Scaled(f)` factor. `view_anno_scale` returns `AnnoScale { paper_units: 1, drawing_units:
  1/factor }`, so a dimension inside the view is annotative at exactly the view's scale and the
  renderer needs no drawing-specific code. `ANNOALLVISIBLE` (`crates/doc/src/lib.rs:221`) keeps its
  meaning: a view whose scale is not in the drawing's scale list shows its annotation at the first
  supported scale rather than not at all, as the existing rule says.
- **Refusals:** a factor that is not finite and positive is `BadScale`; a view whose scale is not in
  the drawing's `scales` list is **reported** by `sheet_anno_factor`, not refused — because
  `anno_draw`'s `Hidden` case would otherwise make a whole view's text disappear, which is a data
  loss in appearance.
- **Tests:** `a_view_at_one_to_two_has_anno_factor_two`. `the_factor_equals_the_annotation_scale
  _factor`. `a_view_whose_scale_is_not_in_the_list_is_reported_not_hidden`. `a_non_finite_factor_is
  _refused`. `the_dimension_text_height_follows_the_view_scale`.
- **Acceptance:** a dimension on a 1:2 view measures the same paper height as the same dimension on
  a 1:1 view — asserted in the test, because that is the whole point of annotative drawing.
- **Risk:** a sheet at 1:1 whose text is annotative at 1:2 prints at half size in some plot paths.
  Fallback: the `Test` asserts against the rendered paper size through `render`'s `DisplayList`, and
  the sheet's report prints each view's annotation factor.

### DRW-SHE-009 — Drawing sections on a sheet: the cut plane, the arrows and the hatch
- **State:** BACKLOG
- **Depends on:** DRW-VIEW-013, KER-BOOL-010
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct SheetSection {
      pub label: String,                    // "A"
      pub plane: cadcraft_solid::Plane,     // in model space
      pub direction: Vec3,                  // which way the section looks
      pub view: String,                     // the section view it feeds
      pub half: bool,                       // half section
      pub hatch: String,                    // a name in cadcraft_doc::library
  }
  pub fn section_view(part: &ScenePart, s: &SheetSection, scale: ViewScale) -> Result<SheetView, DrwError>;
  pub fn cut_face_region(part: &ScenePart, s: &SheetSection) -> Result<cadcraft_geom::Region, DrwError>;
  pub fn section_arrows(s: &SheetSection, parent: &ViewFrame) -> Vec<([Vec2; 2])>;
  ```
- **Behaviour:** The cut face comes from `cadcraft_solid::section` (`crates/solid/src/section.rs:50`),
  which is **exact for a planar B-rep** and refuses the degenerate cases by name (a plane through a
  vertex, tangent to a face) — those refusals are the section view's refusals, verbatim. The cut
  face is hatched with a name from `crates/doc/src/library.rs` (our own patterns), the section line
  and arrows are drawn on the `cadcraft-section` layer with the label at the arrow, and a half
  section cuts one quadrant.
- **Refusals:** the kernel's `Section("…")` errors pass through unchanged (never caught and replaced
  by an empty section); a section plane with a non-finite point or normal is refused; a hatch name
  not in the library is `Refused("no hatch pattern named `{n}`")` — the library is the only source,
  so no pattern file is ever read; a half section on a solid whose cut is not in one quadrant is
  refused naming the quadrant.
- **Tests:** `a_section_view_shows_the_cut_face`. `the_cut_face_is_hatched_with_a_library_pattern`.
  `a_plane_through_a_vertex_is_the_kernels_refusal` (the `KER-BOOL-025` case). `a_tangent_plane_is
  _the_kernels_refusal`. `section_arrows_point_along_the_look_direction`. `a_half_section_removes_one
  _quadrant`. `an_unknown_hatch_is_refused`.
- **Acceptance:** the section's area equals `Section::region`'s area within `tolerance_for`, and the
  kernel's degenerate refusals are asserted to reach the user unchanged.
- **Risk:** for a **curved** solid the kernel's section is a faceted approximation, and a section
  view of a fillet will show facets. Fallback: `SheetSection` records `HiddenLinesMode::Faceted`
  and the sheet's report says so; the exact answer needs `KER-SRF-004`/`KER-SRF-035`.

### DRW-SHE-010 — The parts list on the sheet: a `Table`, and where the rows come from
- **State:** BLOCKED — needs `MOD-BOD-019`'s multi-body BOM and `ASM`'s component list
- **Depends on:** DRW-SHE-004, MOD-BOD-019
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**); the row source is an **interface
  request** to **MOD**/**ASM**
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct PartsRow { pub item: usize, pub part: String, pub description: String,
                        pub qty: u32, pub material: String, pub mass_kg: Option<f64>, pub drawing: String }
  pub fn parts_table(rows: &[PartsRow], style: &str, at: Vec2, height: f64) -> Result<TablePlan, DrwError>;
  /// REQUEST to MOD/ASM: the caller supplies the rows. `cadcraft-drawing` (L2) may not
  /// reach cadcraft-model (L3) or cadcraft-asm (L3), so this type is the seam.
  ```
- **Behaviour:** The parts list is a `cadcraft_doc::Table` (`crates/doc/src/entity.rs:933`) built
  from `TableCell`s, using an existing `TableStyle`, so it plots, breaks across pages and exports
  through the table code that already exists (including the CSV export `DRW-EXP-015`). Rows are
  sorted by **item number**, which is the numbering the balloons use (`DRW-LEAD-004`), so the
  balloon and the list cannot disagree.
- **Refusals:** two rows with the same item number are `Refused` naming both; a row whose item
  number is 0 (the balloon numbering starts at 1) is refused; a table taller than the printable area
  is `Refused` with both heights, and the engine may then split it — never clipping rows silently.
- **Tests:** `rows_are_sorted_by_item`. `a_duplicate_item_is_refused`. `the_table_is_an_ordinary
  _table_entity`. `the_table_saves_to_dxf_and_returns`. `a_table_taller_than_the_sheet_is_refused`.
  `mass_is_none_rather_than_zero_when_unknown`.
- **Acceptance:** `cargo xtask assets` sees nothing (a table is entities, not a file), and the table
  survives a `.dxf` round trip through the existing reader/writer tests.
- **Risk:** "one body is one part" is wrong for an assembly, so the row source is undecided until
  `ASM` lands. Fallback: rows come from whatever `ScenePart`s the view was built from, which is
  honest for a single body and is what the report says the sheet is in.

### DRW-SHE-011 — The revision block and its rules
- **State:** READY
- **Depends on:** DRW-SHE-005
- **Files:** `crates/drawing/src/titleblock.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Revision { pub rev: String, pub date: String, pub by: String, pub description: String }
  pub fn revision_table(revs: &[Revision], style: &str, at: Vec2) -> Result<TablePlan, DrwError>;
  pub fn next_revision(last: Option<&str>) -> Option<String>;
  ```
- **Behaviour:** Revisions are appended to a `Table` beside the title block, newest first, with the
  ISO date string the rest of the app uses (`DAT-070`: `YYYY-MM-DDTHH:MM:SS`, UTC, stored as text so
  no date crate enters the file). `next_revision` moves `A`→`B`, `Z`→`AA`; it is a pure function so
  it is testable without a clock.
- **Refusals:** a revision string that is empty, over 8 characters, or contains a character outside
  `A–Z0–9` is refused naming it; a date that is not ISO-8601 is a **warning** and the field is
  cleared (the `DAT-070` rule, applied without a new decision); a duplicate revision number is
  refused naming both entries.
- **Tests:** `next_revision_increments_letters_and_pairs`. `a_bad_date_is_a_warning`. `a_bad_revision
  _string_is_refused`. `a_duplicate_revision_is_refused`. `revisions_are_newest_first`. `the_table
  _grows_without_re_laying_out_the_sheet`.
- **Acceptance:** the sheet's own report says which revision block it carries, and a sheet save/load
  keeps the rows in order.
- **Risk:** revisions grow without bound. Fallback: the row count is capped at `MAX_REVISIONS = 64`
  and the oldest is dropped **with a report line**, because a sheet that shows a truncated history
  must say so.

### DRW-SHE-012 — Sheet list, sheet index and cross-references between sheets
- **State:** READY
- **Depends on:** DRW-SHE-002, DRW-VIEW-007
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct SheetIndex { pub sheets: Vec<SheetRef> }
  #[derive(Clone, Debug, PartialEq)]
  pub struct SheetRef { pub layout: String, pub title: String, pub number: String, pub revision: String }
  pub fn index_of(d: &cadcraft_doc::Drawing, sheets: &[String]) -> Result<SheetIndex, DrwError>;
  /// What a `SEEK`-style cross-reference resolves to, and the refusal when it cannot.
  pub fn resolve_ref(text: &str, index: &SheetIndex) -> Result<SheetRef, DrwError>;
  ```
- **Behaviour:** A sheet is identified by its layout name (the same `tab_name()` the status bar shows,
  `crates/doc/src/lib.rs:80`), so `SHEET 2` and the tab the user clicks are the same thing. The index
  is derived from the layouts that carry a title block, in tab order.
- **Refusals:** a reference naming a sheet that does not exist is `Refused("no sheet `{n}`; the
  drawing has …")` listing the numbers; a reference with more than 64 characters is refused; two
  sheets with the same number are refused naming both.
- **Tests:** `the_index_is_in_tab_order`. `resolve_ref_finds_a_sheet_by_number_and_by_name`. `an
  _unknown_reference_lists_the_known_ones`. `a_duplicate_sheet_number_is_refused`. `a_sheet_without
  _a_title_block_is_not_in_the_index`.
- **Acceptance:** `sheet.list` returns the index; the tab order comes from
  `Layout::tab_order` (`crates/doc/src/tables.rs:780`).
- **Risk:** sheet numbers are user data and can collide. Fallback: the layout name is always
  accepted as a second key, and the report says which key was used.

### DRW-SHE-013 — The sheet's general note block: material, tolerances, projection, units
- **State:** READY
- **Depends on:** DRW-SHE-004
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct GeneralNotes { pub lines: Vec<String>, pub title: String, pub at: Vec2 }
  pub fn default_notes(sheet: &Sheet, model_units: &str) -> Vec<String>;
  pub fn notes_table(n: &GeneralNotes, style: &str) -> Result<TablePlan, DrwError>;
  ```
- **Behaviour:** The default note block says, in this order: the material (from the model's
  appearance/material if there is one, else blank — never invented), the general tolerance (blank
  unless the user set it), the units **with the model→drawing conversion stated**, the projection
  method (third angle by default, `DRW-VIEW-003`), the sheet scale, and the first-angle/third-angle
  symbol drawn by our own code. Each line is a `Table` row so the block plots and wraps like the
  parts list.
- **Refusals:** a note line over 512 characters is refused; more than 64 lines is refused naming the
  count; a units string that is not one of `DAT-039`'s `UnitSystem::parse` codes is a **warning**
  and the line is dropped from the printed block while being listed in the sheet's report (never a
  wrong unit on a drawing).
- **Tests:** `the_default_notes_are_in_order`. `the_units_line_names_the_conversion`. `the_projection
  _method_follows_the_sheet`. `an_unparseable_unit_is_a_warning_not_a_wrong_unit`. `a_long_line_is
  _refused`. `the_symbol_is_drawn_by_our_code` — the projection symbol is `symbol_prims`-style
  geometry from `crates/doc/src/gdt.rs`, not a glyph from a font.
- **Acceptance:** no font file, no template file and no image is involved, so `cargo xtask assets`
  is unchanged and the brand/asset policy in `AGENTS.md` holds.
- **Risk:** a default note that says the wrong thing is worse than no note. Fallback: every field
  CADCraft cannot state is left **blank**, never defaulted to a plausible value, and the sheet's
  report counts the blanks as findings.

### DRW-SHE-014 — Sheet validation: what is inside the printable area, reported not repaired
- **State:** READY
- **Depends on:** DRW-SHE-006, DRW-SHE-008
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum Finding { OutsidePrintable { handle: cadcraft_doc::Handle, what: String },
                     Overlapping { a: String, b: String }, MixedScale { view: String },
                     AnnotationNotSupported { view: String }, EmptySheet, TitleBlockOffSheet }
  pub fn validate_sheet(d: &cadcraft_doc::Drawing, sheet: &Sheet) -> Result<Vec<Finding>, DrwError>;
  ```
- **Behaviour:** One function that answers "would this print the way it is meant to?", returning
  **findings**. It never moves, scales or deletes anything: an automatic repair of a sheet is how a
  drawing silently changes between saves, and the acceptance for this task is the opposite.
- **Refusals:** a sheet that does not exist is `NoSheet`; a `Finding` for an entity that is not on
  the sheet is not produced (the scan is per space); a finding list over `MAX_FINDINGS = 1000` is
  truncated **with a final finding saying how many were not listed**.
- **Tests:** `a_view_off_the_sheet_is_found`. `two_overlapping_views_are_found_once_each`. `a_mixed
  _scale_sheet_is_found`. `an_empty_sheet_is_found`. `findings_are_sorted_by_handle`. `validation
  _never_mutates_the_drawing` — the drawing is compared before and after. `the_finding_list_is
  _capped_and_says_so`.
- **Acceptance:** `SHEETAUDIT` runs it and prints the findings; the export task refuses to write a
  sheet with an `OutsidePrintable` finding unless the user asked for it anyway.
- **Risk:** a finding per entity on a big sheet is noisy. Fallback: findings are grouped by kind with
  counts, and the handle list per kind is capped at 20 with "and N more".

### DRW-SHE-015 — A sheet is printable with the existing plot path, with no plotting code added
- **State:** READY
- **Depends on:** DRW-SHE-001, DRW-SHE-003
- **Files:** `crates/engine/src/cmd/drawing/sheet.rs` (owner **ENG**); read-only reuse of
  `crates/doc/src/plotter.rs` and `crates/render/src/paper.rs`
- **Contract:** `CommandSpec` id `sheet.plot {layout, output?, path?, resolution?}` (owner **ENG**)
  returning `{path, sheets, width_mm, height_mm, devices}`; the body's call is the **existing**
  `run_plot`/`PlotOutput` path (`crates/engine/src/cmd/layout.rs:79`, `crates/doc/src/plotter.rs:27`),
  with the sheet's layout name in place of a window.
- **Behaviour:** A 3D sheet is a paper-space layout, so `PLOT` already prints it: the same plot
  styles, the same device papers, the same PDF/SVG/PNG writers. This task's whole content is that
  the *content* is right (border, title block, views, notes inside the printable area) and that the
  `layout` argument reaches the existing code path. No new plotter, no new writer, no new device.
- **Refusals:** a layout that does not exist is `NoSheet`; a device paper that does not exist is the
  existing plotter's refusal, passed through; `sheet.plot` with no layout and more than one layout is
  `Refused("name the layout: this drawing has N")`.
- **Tests:** `sheet_plot_produces_the_requested_paper_size` — the PDF's `/MediaBox` equals the page
  setup's size. `sheet_plot_uses_the_existing_writer` — the bytes are produced by `io`'s writer, not
  a new one. `plotting_an_unknown_layout_is_refused`. `a_sheet_with_findings_still_plots_but_prints
  _the_findings_in_the_log` (the same choice the 2D plot makes).
- **Acceptance:** `cargo xtask assets` has no new entry — a PDF a user prints is not a committed
  asset; only a **committed** export fixture is (see `DRW-EXP-016` and `PLT-ASSET-008`).
- **Risk:** someone later adds a "drawing plotter" and the sheet prints differently from a 2D sheet.
  Fallback: the task's acceptance is the test that the bytes come from `io`'s writer; a second
  implementation fails it.

### DRW-SHE-016 — Persisting the sheet list, and what a v1 file does with it
- **State:** BLOCKED — needs `DAT-055`/`DAT-056`'s migration chain (`crates/model/src/file.rs`, owner **D**)
- **Depends on:** DRW-SHE-012, DAT-056
- **Files:** `crates/model/src/file.rs` (owner **D**) — **interface request**, this task does not own
  the file; `crates/drawing/src/sheet.rs` (owner **DRW**) for the default
- **Contract:**
  ```rust
  // REQUEST to crates/model (owner D): in `Document`, both `#[serde(default)]`:
  // pub sheets: Vec<SheetRecord>,   // layout, title_block fields, revision rows
  // pub sheet_views: Vec<SheetView>, // DRW-VIEW-007's shape
  ```
  In `cadcraft-drawing`: `pub fn default_sheets() -> Vec<SheetRecord>;` and
  `pub fn migrate_v1_sheets(d: &Drawing) -> Vec<SheetRecord>;`
- **Behaviour:** A `.ccm` written before sheets existed opens with **one** sheet per layout that has
  paper-space entities (there is no such sheet, so: an empty list, reported). A sheet whose layout
  has been deleted is dropped with a report. The migration is idempotent, like every other in
  `DAT-056`.
- **Refusals:** a sheet record naming a layout that no longer exists is dropped **with a report**;
  a sheet record with a non-finite scale is refused at load naming the sheet; a document with more
  than `MAX_SHEETS = 256` sheets is refused at load.
- **Tests:** `a_v1_document_opens_with_no_sheets_and_says_so`. `a_sheet_whose_layout_vanished_is
  _dropped_reported`. `migrating_twice_is_the_same_as_migrating_once`. `a_non_finite_scale_is_refused
  _with_the_sheet_name`. `too_many_sheets_is_refused`.
- **Acceptance:** the interface request is recorded for **D** and added to `DAT-054`'s v2 field
  table; the `.ccm` round trip keeps a sheet's title-block values (the acceptance test reads them
  back through `DocMetaLike`).
- **Risk:** this is the third cross-domain request into `crates/model/src/file.rs` from three indexes
  (`DAT-069`, `DRW-VIEW-007`, this one). Fallback: collect them in one coordinator request, and
  until it lands the sheet exists in the `.dxf`/`.dwg` only, which the report states as the interim.

---

# DRW-DIM — dimensions that follow the model, drawn on the sheet with the 2D machinery

A dimension on a projected view is `cadcraft_doc::Dimension` — the same entity, the same style, the
same renderer, the same DXF groups. What is new is (a) where its **measured value** comes from (the
3D target, not two picked points) and (b) how it is **associated** with 3D geometry. Everything else
is reuse, and reuse is what this epic is for.

### DRW-DIM-001 — `Assoc3d`: what a dimension on a view is attached to
- **State:** READY
- **Depends on:** DRW-VIEW-006, DRW-VIEW-007
- **Files:** `crates/drawing/src/dim/assoc.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Assoc3d {
      pub part: String,                 // a `ScenePart::name`
      pub target: DimTarget,
      /// The view this association was made in; a dimension is associated per view.
      pub view: String,
      /// The kernel welding tolerance when the target was captured, for the report.
      pub tol: f64,
  }
  #[derive(Clone, Debug, PartialEq)]
  pub enum DimTarget {
      Vertex { point: Vec3 },
      Edge { a: Vec3, b: Vec3, faces: [usize; 2] },
      Face { index: usize, plane: cadcraft_solid::Plane, area: f64 },
      /// A face of another part, e.g. a hole in a mating body.
      Loop { edge: usize, at: f64 },   // `at` is the parameter along the edge
  }
  impl DimTarget {
      /// Distance from `p` to the target, for the resolution report.
      pub fn distance(&self, p: Vec3) -> f64;
      pub fn describe(&self) -> String;
  }
  ```
- **Behaviour:** This is the drawing's own association type, **not** `cadcraft_doc::DimAssoc`
  (which names 2D handles and 2D snaps) and **not** `cadcraft_model::EdgeRef` (a geometric point
  with no face or edge context). It is deliberately geometric and per-view: `Model` has no
  topological naming (`crates/model/src/feature.rs:110`), so a face index is only meaningful for the
  exact geometry that was there, and that limitation is stated in the module doc in the same words
  `crates/doc/src/lib.rs:42` uses. `Edge` stores the two faces so a later edge can be re-identified
  by its face pair when the indices move but the geometry is the same.
- **Refusals:** a target with a non-finite point is refused; a `Face` whose plane disagrees with the
  solid's face at `index` by more than `tolerance_for(diagonal)` is refused naming both; `tol` that
  is not finite and positive is refused.
- **Tests:** `an_edge_target_names_its_two_faces`. `a_face_target_records_its_plane_and_area`. `a
  _vertex_target_is_a_point`. `distance_uses_the_kernel_tolerance`. `a_non_finite_point_is_refused`.
  `a_face_target_that_disagrees_with_the_solid_is_refused`. `describe_names_the_kind`.
- **Acceptance:** `cargo xtask layers` unchanged; `grep -rn 'cadcraft_model' crates/drawing` is empty
  (invariant 7).
- **Risk:** an index-based face reference is exactly what `MOD-002` will make unnecessary. Fallback:
  carry the geometric fallback alongside the index from the start — `Face { index, plane, area }`
  already does — so a resolution can fall back to the plane, as `MOD-NAM-023` does.

### DRW-DIM-002 — Where the association is stored, and the one question that decides it
- **State:** BLOCKED — needs a coordinator decision, and `crates/doc`/`crates/model` are other
  owners (`C2`, **D**)
- **Depends on:** DRW-DIM-001, DRW-VIEW-007
- **Files:** `crates/drawing/src/dim/assoc.rs` (owner **DRW**); the two candidate homes are
  `crates/doc/src/entity.rs` (`DimAssoc`/`AssocSnap`, owner **C2**) and `crates/model/src/file.rs`
  (`Document`, owner **D**)
- **Contract (both options, stated so the decision can be made from facts):**
  ```rust
  // Option A — in the 2D entity, next to DimAssoc (crates/doc/src/entity.rs:656):
  pub enum AssocSnap { /* …existing… */ Model { part: String, target: DimTargetJson } }
  // Option B — in the 3D document (crates/model/src/file.rs), keyed by the 2D handle:
  pub struct DimLink { pub dim: cadcraft_doc::Handle, pub layout: String,
                       pub view: String, pub assoc: Assoc3dJson }
  ```
  `cadcraft-drawing` supports **neither** as a dependency: it must not depend on `doc`'s new variant
  until `C2` adds it, and must not depend on `model` at all.
- **Behaviour:** The decision and its reasons, which the report must state: **Option A** travels
  with the entity, so a sheet that goes to another CAD keeps the link in DXF's extension/xdata
  channel (`crates/doc/src/foreign.rs`, the `#528` convention) and survives a `.dxf` round trip; it
  costs a new `AssocSnap` arm in a closed enum used by 58 io test files. **Option B** keeps the
  2D entity clean and puts the link where the 3D model lives, but a sheet saved as `.dxf` loses it
  and a sheet in a drawing with no `.ccm` has nothing to attach to. The recommendation is **A with
  B's `Document` row as a mirror**, because the sheet and the model are usually two files that travel
  together.
- **Refusals:** whichever option is chosen, a stored association that does not resolve is reported
  (`DRW-DIM-021`), never silently re-pointed; a `DimTargetJson` that is malformed is dropped with a
  report line, because it came from a file.
- **Tests:** `an_association_survives_a_save_and_load` — whichever option wins, this test exists.
  `a_malformed_association_is_dropped_with_a_report`. `the_two_options_agree` — a test that reads
  the same sheet with only Option A populated and with only Option B populated and asserts the same
  resolved targets, so the transition is safe.
- **Acceptance:** the decision is written into `docs/plan/DAT.md`'s v2 field table and into the
  report; until then **no** `DRW` task adds a field to a `crates/doc` or `crates/model` struct.
- **Risk:** two indexes asking the same owner for the same file in the same wave. Fallback: the
  coordinator batches them; the interim storage is `DRW-VIEW-016`'s `foreign`/xdata channel, which
  already exists and needs no new variant.

### DRW-DIM-003 — The measured value comes from the 3D target, in millimetres
- **State:** READY
- **Depends on:** DRW-DIM-001
- **Files:** `crates/drawing/src/dim/measure.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum Measure {
      Length(f64), Angle(f64 /*radians*/), Radius(f64), Diameter(f64), ArcLength(f64),
  }
  pub fn measure_target(t: &DimTarget, part: &ScenePart) -> Result<Measure, DrwError>;
  /// Writes the value into the entity, in the entity's own unit (drawing units) and its own kind.
  pub fn apply_measurement(d: &mut cadcraft_doc::Dimension, m: &Measure, part_units_per_mm: f64)
      -> Result<(), DrwError>;
  ```
- **Behaviour:** A length is the distance between a target's two defining points **in model space**
  (millimetres), not between their projections — a dimension in an isometric view measures the same
  as in a front view, which is the single most important property of a projected dimension. A radius
  is the distance from a `Face`'s centre of curvature (a planar face has none, so it is refused) to
  any point on it. `apply_measurement` converts once — `part_units_per_mm` is the model→drawing unit
  factor (1.0 for a millimetre document) — and writes `Dimension::measurement`, the field DXF group 42
  already carries (`crates/io/src/dxf_read.rs:445`). The 2D path leaves `measurement: 0.0` and lets
  the renderer compute it (`crates/render/src/dim.rs:532`); a 3D dimension must carry the value,
  because the two definition points on a sheet are a *projection* and computing from them would
  measure the projection.
- **Refusals:** a length below `tolerance_for(diagonal)` is `Refused("… is below the kernel tolerance
  {t}")`; a non-finite measure is refused; a radius from a non-circular target is refused naming the
  kind; a conversion that overflows `f64` is refused.
- **Tests:** `a_length_is_measured_in_model_space_not_in_the_projection` — the same edge measured in
  three different views gives the same value to 1e-12. `the_measurement_lands_in_the_entity`.
  `a_projection_that_shortens_the_edge_does_not_change_the_value` — an isometric view is the test
  case. `a_sub_tolerance_length_is_refused`. `a_radius_needs_a_curved_face`. `apply_measurement
  _converts_once` — a 25.4 mm length in an inch drawing is 1.0.
- **Acceptance:** the three-view test is the acceptance; without it this epic's central claim is
  unproven.
- **Risk:** `Measurement` on a projected *chord* is wrong by construction and is a trap for the next
  agent. Fallback: `DimTarget::Edge { a, b }` always holds the true 3D endpoints; a caller wanting a
  projected length must project and measure `Vec2` itself, which is the existing 2D path.

### DRW-DIM-004 — Definition points: the 3D target mapped into the view, then into paper
- **State:** READY
- **Depends on:** DRW-DIM-003, DRW-VIEW-011
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Build the 2D `Dimension` entity for a linear dimension on a view, in paper space.
  pub fn linear_dimension(t: &DimTarget, part: &ScenePart, frame: &ViewFrame,
                          at: Vec2, style: &cadcraft_doc::DimStyle, kind: LinearKind)
      -> Result<cadcraft_doc::Dimension, DrwError>;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum LinearKind { Horizontal, Vertical, Rotated(f64 /*radians*/), Aligned }
  pub fn radial_dimension(t: &DimTarget, part: &ScenePart, frame: &ViewFrame, at: Vec2,
                          style: &DimStyle, diameter: bool) -> Result<cadcraft_doc::Dimension, DrwError>;
  pub fn angular_dimension(v: Vec3, a: Vec3, b: Vec3, frame: &ViewFrame, at: Vec2,
                          style: &DimStyle) -> Result<cadcraft_doc::Dimension, DrwError>;
  ```
- **Behaviour:** The same construction as `crates/engine/src/cmd/annotate.rs:122`'s `dim()` helper,
  with three differences and no others: the definition points are the **projections** of the 3D
  target; `DimKind` follows the view (an `Aligned` dimension in an isometric view, a
  `Linear { rotation }` in an orthographic one); and `overrides` is `[]` because a 3D dimension uses
  the sheet's style wholesale. The dimension line's `defpt` is the paper position the user picked, or
  the placement from `DRW-DIM-007`.
- **Refusals:** a target that does not project to two distinct paper points is `Refused("the target
  projects to a single point in this view")` — which is the honest answer for an edge seen end-on;
  a non-finite `at`; a `style` whose `text_height` is not positive is refused (the same guard
  `crates/doc/src/gdt.rs:52` applies).
- **Tests:** `a_linear_dimension_matches_the_2d_constructor` — the same inputs through
  `annotate.rs`'s helper and through this one produce equal `p13`/`p14`/`defpt` within 1e-12.
  `an_edge_seen_end_on_is_refused`. `an_aligned_dimension_in_an_isometric_view_is_aligned`. `a_radial
  _dimension_places_its_centre`. `the_angular_dimension_vertex_is_the_projected_vertex`.
- **Acceptance:** the parity test against `annotate.rs`'s helper is the acceptance: if the two
  disagree, one of them is wrong and the 2D tests say which.
- **Risk:** `annotate.rs` is **ENG**-owned and may change. Fallback: the parity test names the
  engine function; if it is refactored, the test is updated to the new name — the *values* are
  pinned, not the function.

### DRW-DIM-005 — Automatic dimensions: what the feature set actually is
- **State:** READY
- **Depends on:** DRW-DIM-004, DRW-VIEW-005
- **Files:** `crates/drawing/src/dim/auto.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct AutoDimParams {
      pub kinds: DimKinds,          // a bit set: Linear, Aligned, Radius, Diameter, Angular, Ordinate
      pub faces: bool, pub edges: bool, pub vertices: bool,
      pub max_per_face: usize,      // default 2
      pub tolerance_ratio: f64,     // parallelism grouping, default 1e-6
  }
  pub fn auto_dimensions(geom: &ViewGeometry, part: &ScenePart, frame: &ViewFrame,
                         p: AutoDimParams) -> Result<Vec<ProposedDim>, DrwError>;
  pub struct ProposedDim { pub target: DimTarget, pub kind: LinearKind, pub measure: Measure,
                          pub at: Vec2, pub view: String }
  ```
- **Behaviour:** The inference, from real geometry and nothing else:
  - **Face pairs**: two parallel planar faces → one linear dimension per pair, in the pair's own
    direction (`Aligned` when the view is rotated);
  - **Circular faces**: a face that is a disc (a planar face whose region is an n-gon with all
    vertices equidistant from the centroid, within `tolerance_for`) → a radius or diameter dimension;
  - **Edges**: an edge between two parallel faces, not already dimensioned by the face pair → a
    length; an edge whose two adjacent faces meet at `dir_tol`-significant angle → an angular
    dimension;
  - **Vertices**: ordinate chains only, never one ordinate per vertex (a view with 500 vertices is
    not a drawing).
  Each proposal carries its measured value so `DRW-DIM-003`'s claim is testable end to end.
- **Refusals:** a solid with more than `limits::MAX_SOLID_FACES` faces is `Limit`; a face whose
  region is not finite is refused naming the face; a disc test that fails on more than half the
  vertices is **not** a disc (a heuristic must not guess) and is skipped with a count in the report;
  a `max_per_face` above 32 is refused.
- **Tests:** `a_cube_gives_three_pairs_of_opposite_faces`. `a_cylinder_gives_one_diameter`. `a_face
  _pair_gives_one_linear_dimension_in_the_pairs_direction`. `an_edge_between_parallel_faces_is
  _dimensioned_once_not_twice`. `vertices_give_ordinate_chains_not_single_ordinates`. `a_non_disc_
  _polygonal_face_is_not_a_circle`. `proposals_are_sorted_deterministically`. `a_face_cap_overrun_is
  _a_limit_error`.
- **Acceptance:** the proposals for the sample drawing (`crates/engine/src/sample.rs`) are pinned as
  a literal list in the test, so any change to the inference is visible.
- **Risk:** automatic dimensioning is where a drawing becomes unreadable if the heuristics are
  sloppy. Fallback: `max_per_face` defaults to 2 and `AUTOALL` requires an explicit request; a
  proposal list is returned for review **before** any entity is created (`DRW-DIM-022`'s rule).

### DRW-DIM-006 — The filter: grouping, suppression and the duplicate rules
- **State:** READY
- **Depends on:** DRW-DIM-005
- **Files:** `crates/drawing/src/dim/auto.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct DimFilter { pub suppress: Vec<DimTarget>, pub keep_only: Option<DimKinds>,
                         pub chain: ChainRule }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum ChainRule { None, Linear, Ordinate }
  pub fn filter(props: Vec<ProposedDim>, f: &DimFilter) -> Result<Vec<ProposedDim>, DrwError>;
  pub fn duplicates(props: &[ProposedDim]) -> Vec<(usize, usize)>;   // pairs, sorted
  ```
- **Behaviour:** Three rules, all stated because each is a user-visible decision: a dimension whose
  measure is within `tolerance_for(diagonal)` of an existing dimension's is a **duplicate** and is
  suppressed; two dimensions on the same measure that differ only in direction are kept only if
  their angles differ by more than `dir_tol`; `ChainRule::Linear` groups consecutive equal-length
  dimensions into one chain for `DIMCONTINUE` (so the sheet shows one dimension and three
  sub-dimensions, the drafting convention).
- **Refusals:** a `suppress` entry that resolves to nothing is reported (not an error — the user may
  have suppressed a target the current geometry no longer has); a filter that would suppress
  everything is `Refused("this filter would remove every dimension")` rather than producing an empty
  drawing.
- **Tests:** `equal_measures_are_suppressed_as_duplicates`. `the_same_measure_in_two_directions_is_
  kept_when_the_angles_differ`. `a_linear_chain_groups_consecutive_equal_lengths`. `an_ordinate_chain
  _groups_consecutive_vertices`. `a_filter_that_empties_the_list_is_refused`. `a_suppressed_target
  _that_is_gone_is_reported`. `filter_is_deterministic`.
- **Acceptance:** the pinned proposal list of `DRW-DIM-005` shrinks by exactly the duplicate count
  the test asserts — no heuristics hide outside this function.
- **Risk:** a duplicate rule with too tight a tolerance deletes a dimension a drafter wanted.
  Fallback: `duplicates` **reports** every pair it found; the command prints the count and the
  suppression is a separate, explicit step.

### DRW-DIM-007 — Linear dimension placement: offset, extension direction, and outside placement
- **State:** READY
- **Depends on:** DRW-DIM-004, DRW-SHE-006
- **Files:** `crates/drawing/src/dim/place.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct LinearPlacement { pub at: Vec2, pub outside: bool }
  pub fn place_linear(a: Vec2, b: Vec2, kind: LinearKind, p: LinearPlacement)
      -> Result<cadcraft_doc::Dimension, DrwError>;
  /// The first offset clear of the projected geometry, searched outward in `step`s.
  pub fn place_clear(a: Vec2, b: Vec2, kind: LinearKind, geom: &ViewGeometry,
                     step: f64, max_steps: usize) -> Result<Vec2, DrwError>;
  ```
- **Behaviour:** `place_linear` is `annotate.rs`'s `dim()` plus the 2D rule for extension lines
  (`auto_rotation`, `crates/engine/src/cmd/annotate.rs:144`: horizontal unless the pick is outside in
  x) so a 3D dimension places exactly like a 2D one. `place_clear` walks the dimension line
  perpendicular to itself in `step` increments, testing each candidate against the view's own
  segments with a bounding-box test and the drawing's own `pickbox`-scale tolerance, and returns the
  first clear offset — deterministic, no random search, and bounded by `max_steps` (default 64).
- **Refusals:** `max_steps` of 0 or above 512 is refused; a dimension whose measured length is below
  `tolerance_for` is refused (there is nothing to draw); a search that exhausts `max_steps` is
  `Refused("no clear placement for the dimension between … after {n} steps")` naming the two points,
  never the last candidate offered as if it were clear.
- **Tests:** `a_dimension_line_sits_between_its_extensions`. `outside_placement_keeps_the_text_readable`.
  `the_extension_lines_are_perpendicular_unless_oblique`. `place_clear_moves_the_line_off_the_
  geometry`. `place_clear_is_deterministic`. `a_search_that_finds_nothing_is_refused`. `a_zero_length_
  _dimension_is_refused`. `place_linear_matches_the_2d_constructor`.
- **Acceptance:** the placement is asserted against a hand-placed 2D dimension in the parity test, so
  the sheet's dimensions look like the 2D ones.
- **Risk:** a bounding-box test is coarse and can leave a dimension crossing a thin line. Fallback:
  `place_clear` uses the segments' own distance (`Segment::closest`, as `annotate.rs:903` does), and
  a proposal that still crosses after `max_steps` is placed and **reported**, which is the 2D
  behaviour for a manual placement.

### DRW-DIM-008 — Aligned dimensions in a rotated view
- **State:** READY
- **Depends on:** DRW-DIM-007
- **Files:** `crates/drawing/src/dim/place.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The direction an aligned dimension takes in this view: perpendicular to the projected chord.
  pub fn aligned_rotation(a: Vec2, b: Vec2) -> Result<f64, DrwError>;
  pub fn is_aligned_required(frame: &ViewFrame, a: Vec3, b: Vec3, tol: f64) -> bool;
  ```
- **Behaviour:** In an isometric or auxiliary view, a horizontal dimension measures a length that is
  **not** the distance between the projected endpoints, and a drawing that does it is wrong in a way
  nobody notices until manufacture. `is_aligned_required` compares the projected chord's angle with
  the model's axis it represents and asks for an aligned dimension whenever the difference exceeds
  `dir_tol = 1e-9`; the auto-dimensioner then emits `LinearKind::Aligned`.
- **Refusals:** a chord whose projected length is below `tolerance_for` is refused; a rotation that is
  not finite is refused.
- **Tests:** `a_horizontal_dimension_in_an_isometric_view_would_be_wrong`. `an_isometric_view_forces
  _aligned_dimensions`. `an_orthographic_view_keeps_horizontal_dimensions`. `aligned_rotation_is_
  perpendicular_to_the_chord`. `a_degenerate_chord_is_refused`.
- **Acceptance:** the isometric-view test is the acceptance: the same edge gives the same measured
  value whatever the rotation (`DRW-DIM-003`'s property, seen from the placement side).
- **Risk:** the rule "aligned whenever rotated" makes a front view full of aligned dimensions if the
  tolerance is wrong. Fallback: the check is on the **model edge's** direction versus the view's
  axes, not on the projected angle alone; a cube in a front view stays horizontal.

### DRW-DIM-009 — Radius and diameter on a projected circle, and the ellipse problem
- **State:** READY
- **Depends on:** DRW-DIM-003, DRW-VIEW-005
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// A circular face, or the refusal that says it is elliptical in this view.
  pub fn circular_target(face: &cadcraft_solid::Face, frame: &ViewFrame)
      -> Result<(Vec3 /*centre*/, f64 /*radius*/, bool /*is_circle_in_this_view*/), DrwError>;
  ```
- **Behaviour:** A circle of radius `r` in a plane tilted by angle `α` to the view plane projects to
  an **ellipse** with semi-axes `r` and `r·|cos α|`. A diameter dimension on the projected ellipse
  would read `2r·cos α` — wrong. So `circular_target` returns the true centre and radius **and** the
  flag, and `radial_dimension` refuses to draw a radius/diameter dimension on a face that projects
  to an ellipse with `|cos α| < 1e-9`, naming the angle: the honest answer is an *aligned* pair of
  dimensions across the major axis, or a note. The dimension's **measured value** is `2r` from model
  space either way (`DRW-DIM-003`), so a reader of the number is right even where the drawing
  cannot show the symbol honestly.
- **Refusals:** a face that is not a disc is refused naming the face; a projection with
  `|cos α| < 1e-9` (the circle is edge-on) is refused with the angle; a radius below
  `tolerance_for` is refused.
- **Tests:** `a_circle_in_a_face_on_view_draws_a_radius_dimension`. `a_tilted_circle_is_refused_with_
  its_angle`. `an_edge_on_circle_is_refused`. `the_measured_value_is_the_true_diameter_in_every_view`.
  `the_projected_ellipse_ratio_matches_the_cosine` — `|cos α|` asserted numerically.
- **Acceptance:** the "true diameter in every view" test is the acceptance; it is the property a
  drafter would otherwise have to check by hand.
- **Risk:** refusing every diameter in an isometric view makes isometric views un-dimensioned, which
  is the standard practice anyway (dimensions go on the orthographic views). Fallback: the refusal
  message names the view and the ellipse ratio, and `DRW-DIM-018`'s search can place a *leadered*
  diameter in the orthographic view the same face appears in.

### DRW-DIM-010 — Ordinate dimensions and the chain
- **State:** READY
- **Depends on:** DRW-DIM-004
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn ordinate_dimension(feature: Vec3, leader: Vec2, x_type: bool, frame: &ViewFrame,
                            style: &DimStyle) -> Result<cadcraft_doc::Dimension, DrwError>;
  pub fn ordinate_chain(pts: &[Vec3], frame: &ViewFrame, x_type: bool, gap: f64, style: &DimStyle)
      -> Result<Vec<cadcraft_doc::Dimension>, DrwError>;
  ```
- **Behaviour:** `DimKind::Ordinate { x_type }` already exists (`crates/doc/src/entity.rs:525`) and
  the 2D command already builds it (`crates/engine/src/cmd/annotate.rs:333`); the 3D difference is
  that the feature point is a **model vertex** projected, and `ordinate_chain` spaces the leaders by
  the style's `baseline_spacing` (`DimStyle.baseline_spacing`, `crates/doc/src/tables.rs:147`) times
  the effective scale, exactly as `chained()` does at `annotate.rs:351`.
- **Refusals:** a chain over 1000 points is `Limit`; two points closer than `tolerance_for` are
  refused (the leaders would coincide); a non-finite feature point is refused.
- **Tests:** `an_ordinate_dimension_matches_the_2d_constructor`. `a_chain_spaces_by_the_style`.
  `a_chain_starts_at_the_first_point`. `a_chain_over_the_cap_is_refused`. `coincident_points_are_
  refused`. `the_feature_point_is_the_projection_of_a_model_vertex`.
- **Acceptance:** the parity test against `annotate.rs`'s `chained()` spacing rule; the ordinal
  spacing must equal the 2D rule at the same scale.
- **Risk:** the 2D chain uses `prev.p13`/`p14` from the last dimension, which a per-view 3D chain
  must replicate rather than re-derive. Fallback: the parity test pins it.

### DRW-DIM-011 — Baseline dimensions
- **State:** READY
- **Depends on:** DRW-DIM-010
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn baseline_dimension(base: Vec3, point: Vec3, base_at: Vec2, frame: &ViewFrame, style: &DimStyle)
      -> Result<cadcraft_doc::Dimension, DrwError>;
  ```
- **Behaviour:** Every dimension shares the first dimension's **first** extension origin, offset
  away from the measured points by `baseline_spacing × effective_scale` — the rule in `chained()`
  (`annotate.rs:363-373`), including its sign test (which side the dimension line is on). Copied,
  not reinvented; the 3D addition is projecting the two origins.
- **Refusals:** a baseline whose point equals the base is refused; a non-finite origin is refused.
- **Tests:** `every_baseline_shares_the_first_origin`. `the_offset_is_away_from_the_geometry`. `the
  _sign_follows_the_dimension_lines_side`. `a_baseline_matches_the_2d_constructor`.
- **Acceptance:** the parity test.
- **Risk:** low; this is a copy task and the risk is drift. Fallback: the parity test.

### DRW-DIM-012 — Angular dimensions between two projected edges
- **State:** READY
- **Depends on:** DRW-DIM-004
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The angle between two 3D edges as seen in this view, measured in the view plane.
  pub fn view_angle(a1: Vec3, a2: Vec3, b1: Vec3, b2: Vec3, frame: &ViewFrame)
      -> Result<f64, DrwError>;   // radians, 0..=2π, CCW from a to b
  pub fn angular_dimension(v: Vec3, a: Vec3, b: Vec3, frame: &ViewFrame, at: Vec2, style: &DimStyle)
      -> Result<cadcraft_doc::Dimension, DrwError>;
  ```
- **Behaviour:** The angle is measured **in the view plane**, between the two projected directions,
  because that is the angle the arc will show; the vertex is the projected intersection of the two
  edge *lines*, and it is the vertex that goes into `p15`, matching `DimKind::Angular3P`
  (`crates/engine/src/cmd/annotate.rs:203`). The quadrant is the one the pick `at` is in — the same
  rule `AngularM::located` uses.
- **Refusals:** two edges whose projected directions are parallel within `dir_tol` are refused
  ("the two edges are parallel in this view") — the model-space angle would exist, but this is a
  drawing of the view; two edges that do not meet in the view plane are refused with both points.
- **Tests:** `an_angle_is_measured_in_the_view_plane`. `the_vertex_is_the_projected_intersection`.
  `the_quadrant_follows_the_pick`. `parallel_edges_are_refused`. `an_angle_matches_the_2d_
  constructor`. `a_reflex_angle_is_measured_the_long_way`.
- **Acceptance:** the parity test against `annotate.rs`'s `angular_from_lines`.
- **Risk:** a view-space angle of two 3D edges is a genuinely different number from the model angle,
  and a sheet that mixes them is confusing. Fallback: the dimension's text is the view-space value
  (what is drawn) and `Dimension.measurement` carries the same number, so the two never disagree;
  the module doc says which it is.

### DRW-DIM-013 — Dimension text: the placeholder, prefixes, suffixes and the tolerance
- **State:** READY
- **Depends on:** DRW-DIM-003
- **Files:** `crates/drawing/src/dim/text.rs` (owner **DRW**); read-only reuse of
  `crates/render/src/dim.rs:351`
- **Contract:**
  ```rust
  /// Fill `<>` in a dimension's text with its measured value, formatted by the existing formatter.
  pub fn measure_text(d: &cadcraft_doc::Dimension, st: &cadcraft_doc::DimStyle, dimscale: f64)
      -> String;
  pub fn set_tolerance(d: &mut cadcraft_doc::Dimension, plus: f64, minus: f64) -> Result<(), DrwError>;
  pub fn set_limits(d: &mut cadcraft_doc::Dimension, upper: f64, lower: f64) -> Result<(), DrwError>;
  ```
- **Behaviour:** The `""` / `"<>"` / `" "` convention is `Dimension.text`'s own doc
  (`crates/doc/src/entity.rs:546`) and the rendering is `format_linear_value`
  (`crates/render/src/dim.rs:351`) — so CAD,DRW's text is **byte-identical** to a 2D dimension's
  with the same style and value, including engineering and architectural formats. `<>` is replaced
  by the measurement, a user prefix/suffix is kept, and `DIMPOST` (the style's own field) is applied
  by the existing formatter. Tolerance/limits text uses the style's own fields
  (`DimStyle::post_tolerance` family, `crates/render/src/dim.rs:355`), never a bespoke format.
- **Refusals:** a `plus`/`minus` that is negative or non-finite is refused; an upper limit below the
  lower is refused naming both; a text over 512 characters is refused.
- **Tests:** `an_empty_text_shows_the_measurement`. `a_placeholder_is_substituted`. `a_prefix_and_
  suffix_survive`. `the_format_is_byte_identical_to_a_2d_dimension`. `tolerance_and_limits_text_use_
  the_style_fields`. `a_negative_tolerance_is_refused`. `a_reversed_limit_is_refused`.
- **Acceptance:** the byte-identical test, run against a dimension the 2D path built and one this
  path built with the same style and value.
- **Risk:** a second formatter in `DRW` would drift from `render`'s. Fallback: this epic has no
  formatter at all; it only fills `<>` and calls the existing one (through the renderer, at draw
  time, as the 2D path does).

### DRW-DIM-014 — `DIMLFAC`: the number on the sheet is not the number in the model
- **State:** READY
- **Depends on:** DRW-DIM-013
- **Files:** `crates/drawing/src/dim/text.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The **stored** value: the model's, in drawing units. Never multiplied by a factor.
  pub fn true_value(m: &Measure, part_units_per_mm: f64) -> Result<f64, DrwError>;
  /// The **displayed** value: `true × DIMLFAC`, rounded by the style's decimals.
  pub fn display_value(true_value: f64, lfac: f64) -> Result<f64, DrwError>;
  ```
- **Behaviour:** `DIMLFAC` exists so a drawing can show inches on a millimetre model without changing
  the model. So `Dimension.measurement` stores the **true** value (this is what a machine reading the
  DXF group 42 needs) and the factor is applied at display by the existing formatter
  (`crates/render/src/dim.rs`), which already reads `DIMLFAC` from the style. The point of the task is
  that the *two* numbers are never confused.
- **Refusals:** a non-finite factor is refused; a factor of 0 is refused (`DIMLFAC 0` means "use the
  measured value" in AutoCAD, so 0 is legal but is then the identity — stated, not refused: the
  rule is `lfac == 0.0` means 1.0, matching AutoCAD).
- **Tests:** `the_stored_value_is_never_the_displayed_value`. `a_factor_of_one_changes_nothing`.
  `lfac_zero_means_identity`. `a_non_finite_factor_is_refused`. `a_factor_converts_units_correctly`
  — 25.4 mm with `DIMLFAC` for inches shows 1".
- **Acceptance:** the stored/displayed distinction is asserted in a test that reads the value back
  from a written DXF (group 42) and compares it with the rendered text.
- **Risk:** the reverse mistake — storing the displayed value — makes a DXF lie to a downstream tool.
  Fallback: the acceptance test reads group 42 from a written file; it fails loudly if it ever does.

### DRW-DIM-015 — Units in the text: one conversion, at one place
- **State:** BLOCKY READY — depends DAT-045
- **Depends on:** DRW-DIM-013, DAT-045
- **Files:** `crates/drawing/src/dim/text.rs` (owner **DRW**); the conversion is `DAT-045`'s
  `format_in` in `crates/render/src/units.rs` / `crates/ui-egui/src/parametric.rs` (owners **D**,
  **REN**, **UI**) — **read only**
- **Contract:**
  ```rust
  /// The model→drawing-unit factor for this sheet, named once so it cannot be applied twice.
  pub fn units_per_mm(sheet: &Sheet, doc_unit: DocUnitLike) -> Result<f64, DrwError>;
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum DocUnitLike { Millimetre, Inch, Foot, /* …the codes `DAT-039` defines */ }
  ```
- **Behaviour:** A millimetre model drawn on an inch sheet shows inches: `DimStyle`'s
  `DIMLFAC`/`DIMDEC`/`DIMDUNIT` do the shape and the factor, exactly as they do for 2D, and
  `DAT-045`'s `format_in` does the unit conversion at the call site. `units_per_mm` exists so that
  the factor is *computed once* per sheet and handed to `apply_measurement` (`DRW-DIM-003`) and to
  nothing else — two conversions is the bug this prevents.
- **Refusals:** an unknown unit code is `Refused("`{s}` is not a unit this build knows")` naming
  `DAT-039`'s `parse`; a non-finite factor is refused.
- **Tests:** `a_millimetre_model_on_an_inch_sheet_measures_1_inch`. `the_factor_is_applied_exactly
  _once`. `an_unknown_unit_is_refused`. `the_factor_matches_the_headers_insunits`. `an_inch_sheet_
  and_an_inch_model_agree_with_a_millimetre_sheet`.
- **Acceptance:** the four-way test (mm model/inch sheet, mm/mm, inch/inch, inch/mm) all agree with
  the same physical part.
- **Risk:** `DIMLFAC` and the document unit are two conversion mechanisms and could both apply.
  Fallback: the rule is written in the task and asserted: **if the sheet's unit differs from the
  model's, `DIMLFAC` is ignored and a report line says so** — one mechanism, never two.

### DRW-DIM-016 — The dimension style for 3D, and the per-view override
- **State:** READY
- **Depends on:** DRW-DIM-004, DRW-SHE-007
- **Files:** `crates/drawing/src/dim/style.rs` (owner **DRW**); `DimStyle` is `crates/doc/src/tables.rs`
  (owner **C2`) — **read only**
- **Contract:**
  ```rust
  /// The style a dimension on `view` uses: the sheet's, plus the view's own overrides.
  #[derive(Clone, Debug, PartialEq)]
  pub struct ViewDimStyle {
      pub base: String,
      pub overrides: serde_json::Map<String, serde_json::Value>,   // the same map `Dimension.overrides` holds
      pub scale: ViewScale,
  }
  pub fn view_style(sheet: &Sheet, view: &SheetView, d: &cadcraft_doc::Drawing)
      -> Result<ViewDimStyle, DrwError>;
  pub fn effective(st: &ViewDimStyle, d: &cadcraft_doc::Drawing) -> Result<cadcraft_doc::DimStyle, DrwError>;
  ```
- **Behaviour:** One dimension style for the whole sheet, overridable per view — the same
  mechanism as a per-dimension `DIMOVERRIDE`, using `Dimension::overrides`' `serde_json::Map`
  (`crates/doc/src/entity.rs:563`) and `DimStyle::with_overrides` (`annotate.rs:358`). `effective`
  resolves base + overrides through the **existing** merge, so a 3D dimension and a 2D dimension on
  the same sheet with the same overrides render identically.
- **Refusals:** a base style that does not exist is `Refused("no dimension style named `{s}`")`
  listing the known names; an override key that is not a `DimStyle` field is reported and ignored,
  the same rule `annotate.rs:863` applies at the command line.
- **Tests:** `a_view_style_is_the_sheet_style`. `a_view_override_changes_one_field`. `an_unknown_
  style_is_refused_and_lists_the_known_ones`. `an_unknown_override_key_is_reported_and_ignored`.
  `a_3d_dimension_renders_identically_to_a_2d_one_with_the_same_style`.
- **Acceptance:** the render-identical test is the acceptance: same style, same value, same paper
  size → identical `DisplayList` segment for `render`.
- **Risk:** a per-view style is a second place a style can be set. Fallback: `view_style` returns
  the sheet style with an override map; the map is stored on the `SheetView`, so there is exactly one
  place it comes from.

### DRW-DIM-017 — Text height, arrow size and the two scales that multiply them
- **State:** READY
- **Depends on:** DRW-DIM-016, DRW-SHE-008
- **Files:** `crates/drawing/src/dim/style.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The effective DIMSCALE for a view: the style's, or the drawing's when the style's is 0.
  pub fn effective_dimscale(st: &DimStyle, d: &Drawing) -> f64;
  /// Text height in paper units for a dimension on a view.
  pub fn text_height(st: &DimStyle, d: &Drawing, view: &SheetView) -> Result<f64, DrwError>;
  /// Arrow size in paper units.
  pub fn arrow_size(st: &DimStyle, d: &Drawing, view: &SheetView) -> Result<f64, DrwError>;
  ```
- **Behaviour:** The rule is the 2D one, stated: paper sizes in an annotative style are paper
  sizes, so they are multiplied by the **annotation** factor (`anno_draw`/`anno_scaled`,
  `crates/doc/src/anno.rs:137,154`), **not** by the view scale; a non-annotative style's sizes are
  multiplied by `DIMSCALE` (falling back to the drawing's when the style's is 0), as
  `crates/doc/src/gdt.rs:47` does for a tolerance frame. The view scale multiplies the *geometry*
  and nothing else. `mleader_kind` (`annotate.rs:429-438`) is the reference implementation of this
  exact rule.
- **Refusals:** a `text_height` or `arrow_size` that is not finite and positive is refused naming the
  field; a style whose `DIMSCALE` is negative is refused.
- **Tests:** `an_annotative_style_uses_the_annotation_factor_not_the_view_scale`. `a_non_annotative_
  style_uses_dimscale_with_the_drawings_fallback`. `a_zero_dimscale_falls_back_to_the_drawing`. `a
  _view_at_one_to_two_does_not_halve_its_text`. `text_height_matches_the_renderers_result`.
- **Acceptance:** the "1:2 does not halve the text" test is the acceptance, and it is checked
  against `render`'s own result, not against a second calculation.
- **Risk:** getting this wrong makes a 1:2 sheet's text double size, which is the classic annotative
  bug. Fallback: the test asserts against `render::dimension_geometry`'s own output, so there is one
  implementation in the loop.

### DRW-DIM-018 — Placement search against the view's geometry and its other dimensions
- **State:** READY
- **Depends on:** DRW-DIM-007, DRW-SHE-006
- **Files:** `crates/drawing/src/dim/place.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Occupancy { pub cells: std::collections::BTreeSet<(i32, i32)>, pub cell: f64 }
  impl Occupancy {
      pub fn from_geometry(g: &ViewGeometry, paper: Bounds2, cell: f64) -> Result<Occupancy, DrwError>;
      pub fn hits(&self, r: Bounds2) -> bool;
      pub fn insert(&mut self, r: Bounds2);
  }
  pub fn place_all(dims: &[ProposedDim], g: &ViewGeometry, paper: Bounds2,
                   cell: f64, budget: usize) -> Result<Vec<Option<Vec2>>, DrwError>;
  ```
- **Behaviour:** A uniform grid over the printable rectangle, `cell` in paper units (default the
  style's text height × 2), each dimension's text rectangle rasterised into cells, and each
  dimension's candidate offsets tried in a fixed order until one hits nothing. `place_all` returns
  `None` for a dimension it could not place, and the caller reports the count — a drawing where three
  dimensions are unplaced says so instead of overlapping them.
- **Refusals:** `cell` that is not finite and positive is refused; a grid over 4 000 000 cells is
  `Limit` (and the cell size is raised to fit, with the change reported); a `budget` of 0 is refused;
  a `None` from `place_all` is **never** replaced by a colliding position.
- **Tests:** `a_dimension_is_placed_clear_of_the_geometry`. `two_dimensions_do_not_overlap`. `an
  _unplaceable_dimension_is_none_not_a_collision`. `the_grid_is_deterministic`. `a_huge_grid_is_a_
  limit_error_with_a_coarser_cell`. `the_search_respects_the_budget`.
- **Acceptance:** on the sample drawing, every proposed dimension is placed clear, asserted by
  rasterising the sheet and checking the `hits` function.
- **Risk:** a grid-based search is O(dims × offsets × cells) and a sheet with 200 dimensions is
  slow. Fallback: `budget` caps the offsets tried; a dimension past the budget is `None` and is
  reported, which is faster and more honest than a slow search.

### DRW-DIM-019 — `DIMSPACE` and `DIMREASSOCIATE` for dimensions on views
- **State:** READY
- **Depends on:** DRW-DIM-018, DRW-DIM-021
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**); commands in
  `crates/engine/src/cmd/drawing/dim.rs` (owner **ENG**)
- **Contract:**
  ```rust
  pub fn space_parallel(dims: &mut [cadcraft_doc::Dimension], base: usize, spacing: f64)
      -> Result<Vec<usize>, DrwError>;   // the changed ones
  pub fn reassociate_3d(d: &mut cadcraft_doc::Dimension, part: &ScenePart, frame: &ViewFrame,
                        view: &str, tol: f64) -> Result<Assoc3d, DrwError>;
  ```
  `CommandSpec` ids (owner **ENG**): `drawing.dim.space {base, handles, spacing?}`,
  `drawing.dim.reassociate {handles?}`.
- **Behaviour:** `space_parallel` is `run_dimspace` (`crates/engine/src/cmd/annotate.rs:677`) with
  the 3D origin: the ordering is by each dimension's distance from the base line measured along the
  shared normal, and the moved dimension's `defpt` slides along that normal only. `reassociate_3d`
  re-binds a dimension to the 3D target under its definition points, using the projected point and
  `tol`, and returns the new association or refuses when nothing is within `tol`.
- **Refusals:** a base that is not a linear/aligned dimension is refused (the 2D message, verbatim);
  a re-association whose nearest 3D feature is farther than `tol` is
  `Refused("nothing within {tol} of the dimension's definition points")`; a spacing that is negative
  or non-finite is refused; a `None` re-association leaves the old one **in place** and reports it as
  stale — it is never cleared.
- **Tests:** `space_parallel_matches_the_2d_command`. `spacing_is_measured_along_the_shared_normal`.
  `reassociate_binds_to_the_face_under_the_points`. `reassociate_outside_tolerance_refuses_and_keeps
  _the_old_target`. `a_non_linear_base_is_refused`. `a_negative_spacing_is_refused`.
- **Acceptance:** the parity test against `run_dimspace`'s spacing rule; the numbers must match at
  the same scale.
- **Risk:** re-association silently re-points a dimension, which is the failure `DRW-DIM-021` exists
  to prevent. Fallback: it is an explicit command, never automatic, and it reports what it bound to.

### DRW-DIM-020 — Update a dimension after a model change, and report what moved
- **State:** BLOCKED — needs `MOD-REC-001`'s rebuild record (owner **MOD**)
- **Depends on:** DRW-DIM-019, DRW-VIEW-016
- **Files:** `crates/drawing/src/dim/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct DimUpdate {
      pub dim: String,
      pub before: f64, pub after: f64,
      pub resolved: bool,
      pub reason: Option<String>,   // why it did not resolve
      pub moved: bool,              // the view's geometry moved it
  }
  pub fn update_dimensions(view: &SheetView, part: &ScenePart, frame: &ViewFrame,
                           dims: &[(cadcraft_doc::Handle, Assoc3d)], tol: f64)
      -> Result<(Vec<cadcraft_doc::Dimension>, Vec<DimUpdate>), DrwError>;
  ```
- **Behaviour:** Each dimension re-measures from its 3D target, re-projects its definition points and
  re-runs `place_clear` (`DRW-DIM-007`) **only if the geometry moved far enough to collide**, so a
  parameter change that does not move the view's silhouette leaves the user's careful placement
  alone. The report lists every dimension with its before/after, so a sheet that changed is
  self-describing.
- **Refusals:** a target that no longer resolves within `tol` is `resolved: false` with a reason, and
  the dimension keeps its **last drawn value** with a `Stale` marker in the sheet's report — never a
  new value guessed from the new geometry; a non-finite new measure is refused and the old value
  stands.
- **Tests:** `a_parameter_change_updates_the_measurement`. `an_unchanged_silhouette_keeps_the
  _placement`. `a_moved_face_replaces_the_value_and_reports_it`. `an_unresolved_target_keeps_the
  _last_value_and_is_marked_stale`. `the_update_is_idempotent`. `the_report_is_sorted_by_dimension`.
- **Acceptance:** the sheet's report after a rebuild lists every changed dimension with its two
  numbers; the test asserts the report is empty for a rebuild that changed nothing.
- **Risk:** updating a dimension the user has moved by hand (`user_text_pos`) must not move it.
  Fallback: a dimension with `user_text_pos == true` keeps its `text_mid` and its `defpt`, and the
  update reports `moved: false` with the reason "text placed by hand".

### DRW-DIM-021 — Associativity on load: an unresolved target is reported, never re-pointed
- **State:** READY
- **Depends on:** DRW-DIM-001
- **Files:** `crates/drawing/src/dim/assoc.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum Resolution3d {
      Exact { distance: f64 },
      Approximate { distance: f64 },
      Stale { reason: String },
  }
  pub fn resolve(assoc: &Assoc3d, part: &ScenePart) -> Result<(DimTarget, Resolution3d), DrwError>;
  pub fn resolve_all(assocs: &[Assoc3d], part: &ScenePart, budget: usize)
      -> Result<Vec<(usize, Resolution3d)>, DrwError>;
  ```
- **Behaviour:** A face target resolves when the solid's face at `index` has the same plane within
  `tolerance_for(diagonal)` and the same area; an edge target when an edge with the same endpoints
  (within tolerance) and the same face pair exists. Anything else is `Stale` with a reason naming
  the distance and what was expected. **Nothing is re-pointed**: a dimension whose target changed
  identity keeps its drawn value and is listed as stale, because a silently re-targeted dimension is
  a wrong number on a drawing, which is the worst outcome this domain has.
- **Refusals:** a resolution whose nearest candidate is farther than `10 × tolerance_for` is `Stale`,
  not `Approximate`; a budget over 100 000 is refused; an association naming a part that is not in
  the scene is `Stale` naming the part.
- **Tests:** `an_unchanged_face_resolves_exact`. `a_face_that_moved_is_approximate_with_its_distance`.
  `a_replaced_face_is_stale_and_never_repointed`. `an_association_to_a_missing_part_is_stale`. `the
  _stale_dimension_keeps_its_value`. `resolve_all_is_deterministic` and capped by the budget.
- **Acceptance:** the "never re-pointed" test is the acceptance and is the one that must never be
  relaxed; it is listed here as the epic's governing test.
- **Risk:** a drafter will find stale dimensions annoying. Fallback: `DRAWINGDIMUPDATE {force}` may
  re-bind **explicitly**, showing the old and new target side by side and requiring confirmation —
  never by default.

### DRW-DIM-022 — The pipeline's refusals, and hostile input end to end
- **State:** READY
- **Depends on:** DRW-DIM-018, DRW-DIM-020
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn auto_dim_pipeline_never_panics() -> bool;` and
  `pub fn hostile_dims_never_panic() -> bool;`
- **Behaviour:** The full path — `auto_dimensions` → `filter` → `measure` → `place` → entity — over
  hostile input: a solid with `1e300` coordinates, a `1e-300` part, a target whose plane is `NaN`, a
  `ViewScale` of `1e-300` and of `1e300`, 200 mutations of a valid dimension's JSON, a
  `ProposedDim` with a non-finite `at`, a `DimTarget::Edge` whose two points are identical, a style
  with `text_height: 0` and with `-1`, and a `tol` of 0. Every case returns `Ok` or `Err` with a
  named variant; none panics, indexes out of bounds, allocates from an unchecked number or produces
  a NaN coordinate.
- **Refusals:** `Refused`/`Limit`/`BadScale`/`Unresolved`, with the offending number in the message.
  A `Refused` naming nothing is a failing test.
- **Tests:** `auto_dim_pipeline_never_panics` and `hostile_dims_never_panic`. `every_refusal_names_a
  _number_or_an_index` — a table over the error paths asserting the message contains the input.
  `no_dimension_output_holds_a_nan`. `a_dimension_is_never_created_for_a_refused_proposal`.
  `the_pipeline_is_deterministic` — two runs, same entities, same order.
- **Acceptance:** this task is the epic's gate; `DRW-DIM` is not done while it fails, and it is the
  `never-crash` standard of `AGENTS.md` applied to the drawing path.
- **Risk:** the pipeline is the one place where three heuristics and a placement search meet, so it
  is where a combination the unit tests do not have will surface. Fallback: the budget and the caps
  (`DRW-DIM-018`) bound it, and every loop in the path takes a bounded count.

---

# DRW-GDT — feature control frames on projected views, and what they point at

A feature control frame is `cadcraft_doc::Tolerance` (`crates/doc/src/gdt.rs:19`), its symbols are
drawn by `symbol_prims` from our own geometry, and its text height comes from a `DimStyle`
(`Tolerance::text_size`, `gdt.rs:47`). All of that is 2D, working, and reused verbatim. What GD&T
on a 3D view adds is **recognition** (which face is a datum, which is a hole) and **association**
(reuse `Assoc3d`, `DRW-DIM-001`), and an honest statement of what a *projected* view can say: a
cylindricity frame attached to a faceted cylinder describes a shape the kernel does not yet have.

### DRW-GDT-001 — Feature recognition: what each face can be called
- **State:** READY
- **Depends on:** DRW-VIEW-005, DRW-DIM-001
- **Files:** `crates/drawing/src/gdt/features.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum FeatureKind { Planar, Cylinder { axis: [Vec3; 2] }, Cone, Sphere, Torus, Prism, Other }
  #[derive(Clone, Debug, PartialEq)]
  pub struct Recognised {
      pub kind: FeatureKind,
      pub face: usize,
      pub area: f64,
      pub radius: Option<f64>,     // for a disc or a prism's inscribed circle
      pub axis: Option<[Vec3; 2]>,
      pub confidence: f64,          // 1.0 = exact, < 1 = a heuristic that passed its test
  }
  pub fn recognise(face: &cadcraft_solid::Face, index: usize, tol: f64) -> Result<Recognised, DrwError>;
  pub fn recognise_all(solid: &cadcraft_solid::Solid, tol: f64) -> Result<Vec<Recognised>, DrwError>;
  ```
- **Behaviour:** Recognition is **geometric, with the test stated**, not a guess: a planar face is
  `Planar`; a planar face whose loop vertices are all within `tol` of a common centre and whose
  radius spread is under `tol` is a **disc** and becomes `Cylinder { axis }` (or `Prism` when the
  adjacent faces are planar — a faceted cylinder is a prism and saying otherwise is a lie, so the
  type distinguishes them); everything else is `Other`. `confidence` is 1.0 for the exact tests and
  the measured ratio for a heuristic, and the value is printed in the report, because "we recognised
  this as a hole" with no number is a guess dressed as a fact.
- **Refusals:** a face with a non-finite plane or an empty region is refused naming the index; a
  radius spread above `tol` makes it `Other` with `confidence: 0.0`, **never** a cylinder; a solid
  over `limits::MAX_SOLID_FACES` is `Limit`; a face whose area is below `tolerance_for(diagonal)` is
  skipped with a count (too small to name).
- **Tests:** `a_planar_face_is_planar`. `a_disc_is_recognised_as_a_prism_not_a_cylinder` — the kernel's
  fillet is faceted and the test pins that it says so. `an_ellipse_is_not_a_disc`. `confidence_is_one_
  _for_an_exact_recognition`. `recognition_is_deterministic`. `a_face_cap_overrun_is_a_limit_error`.
- **Acceptance:** on the sample drawing, `recognise_all` returns a pinned list; the list is the
  acceptance and is what every later GD&T task's tests are written against.
- **Risk:** "prism vs cylinder" is a naming decision that will be wrong the day `KER-SRF-003` lands.
  Fallback: the type is `Cylinder { faceted: bool }`-shaped by having two variants now, and
  `KER-SRF-018`'s `Surface` gives the exact answer — the epic's later tasks are `BLOCKED` on it, and
  the faceted-first path is honest in the meantime.

### DRW-GDT-002 — The datum feature symbol: one block per letter, drawn by our own geometry
- **State:** READY
- **Depends on:** DRW-GDT-001
- **Files:** `crates/drawing/src/gdt/datum.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn datum_block_name(letter: char) -> String;             // "*D_A"
  pub fn datum_block(letter: char, size: f64) -> Result<cadcraft_doc::Block, DrwError>;
  pub fn datum_feature(letter: char, at: Vec3, dir: Vec2, size: f64, stem: f64)
      -> Result<(Vec<cadcraft_doc::EntityKind>, String /*block name*/), DrwError>;
  /// Where a datum's plane/triangle sits, given the face it names.
  pub fn datum_placement(r: &Recognised, frame: &ViewFrame, offset: f64)
      -> Result<(Vec2, f64 /*rotation*/), DrwError>;
  ```
- **Behaviour:** A filled triangle at the size the style's text height dictates, an extension line
  to the surface, and the letter in a `Block` named `*D_<letter>` — the same convention as
  `Drawing::anonymous_block_name` (`crates/doc/src/lib.rs:371`). The triangle is three of our own
  line segments, so no font and no asset is involved and `cargo xtask assets` has nothing to check.
  A **datum** (A, B, C) gets a triangle **filled**; a feature symbol (a diameter, a thread) gets the
  ISO frame variants, which are the same construction with a different prefix segment.
- **Refusals:** a letter outside `A..=Z` is `Refused("datum letters are A..Z; got `{c}`")`; a size
  that is not finite and positive is refused; a `Recognised` with no face (a zero-area face) is
  refused naming the face.
- **Tests:** `the_triangle_is_three_segments_of_our_geometry`. `the_block_name_is_star_d_underscore_
  letter`. `a_lowercase_letter_is_refused`. `the_symbol_sits_at_the_faces_projection`. `the_symbol_
  _rotates_with_the_view`. `two_datums_share_one_block`. `a_datum_on_a_cylindrical_face_uses_the
  _diameter_variant`.
- **Acceptance:** the symbol survives a `.dxf` round trip as a block reference with its attributes
  (the existing block writer/reader's job), and no file asset is produced.
- **Risk:** a filled triangle needs a hatch or a solid entity to look right in every plot style.
  Fallback: the triangle is drawn as a closed `LwPolyline` with a solid fill via the existing
  `SOLID`/`HATCH` entity, both of which the plot pipeline already handles.

### DRW-GDT-003 — Datum targets: a circular or rectangular target area, and the segment number
- **State:** READY
- **Depends on:** DRW-GDT-002
- **Files:** `crates/drawing/src/gdt/datum.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum TargetArea { Circle { radius: f64 }, Rectangle { w: f64, h: f64 } }
  pub fn datum_target(letter: char, segment: usize, area: TargetArea, at: Vec3, dir: Vec2, size: f64)
      -> Result<(Vec<cadcraft_doc::EntityKind>, String), DrwError>;
  ```
- **Behaviour:** The target is a divider line, the letter, and a segment number below — the ISO
  construction, drawn from our own geometry like every other symbol, with the segment number as an
  `MText`. Its 3D position is a point on the feature plus a direction, and it projects like everything
  else.
- **Refusals:** `segment` of 0 or above 99 is refused; a circle radius or rectangle side that is not
  finite and positive is refused; an area whose diameter exceeds the feature's own size by more than
  `tolerance_for` is `Refused("the target area is larger than the feature")` naming both.
- **Tests:** `a_circular_target_is_a_circle_segment_and_the_letter`. `a_rectangular_target_is_four_
  segments`. `the_segment_number_is_below_the_letter`. `a_segment_of_zero_is_refused`. `a_target_larger
  _than_the_feature_is_refused`. `the_target_projects_into_the_view`.
- **Acceptance:** the segment count is part of the block's identity, so two targets with different
  counts do not share a block; the test asserts that.
- **Risk:** a target on a curved face does not show its true shape in a projected view. Fallback:
  the view's `mode` is recorded with the target and the sheet's report says so.

### DRW-GDT-004 — Placing a feature control frame on a view
- **State:** READY
- **Depends on:** DRW-GDT-001, DRW-DIM-004
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum FrameAttach { BelowFeature { offset: f64 }, AtPoint { at: Vec2 }, Leadered { at: Vec2, elbow: f64 } }
  pub fn feature_control_frame(spec: &FrameSpec, assoc: Option<&Assoc3d>, frame: &ViewFrame,
                               attach: FrameAttach, style: &DimStyle)
      -> Result<cadcraft_doc::Tolerance, DrwError>;
  /// The point on the projected geometry a frame below the feature hangs from.
  pub fn feature_point(r: &Recognised, frame: &ViewFrame) -> Result<Vec2, DrwError>;
  ```
- **Behaviour:** A frame is attached **to the feature it describes**, not to a free-floating point:
  `feature_point` projects the recognisable point of the face (a cylinder's axis line's visible
  point, a plane's face centre, a prism's inscribed-circle centre) and the frame hangs below it at
  the style's `text_gap × scale`. `Leadered` places it anywhere with a `LEADER`, which is what a
  crowded view uses. The resulting entity is `EntityKind::Tolerance` with `insert`/`dir` set exactly
  as `crates/doc/src/gdt.rs` documents (insert = middle of the left edge of the first row).
- **Refusals:** an offset that is not finite and positive is refused; a `FeatureKind::Other` is
  refused naming the face ("this face is not a feature CADCraft can describe"); a frame whose layout
  does not fit inside the printable area is reported by the placement search (`DRW-DIM-018`), not
  drawn off the sheet.
- **Tests:** `a_frame_hangs_below_its_feature`. `a_leadered_frame_reaches_its_feature`. `a_frame_on_
  _a_cylinder_attaches_to_its_axis`. `an_unrecognised_face_is_refused_by_name`. `the_frame_entity_
  is_a_tolerance`. `the_layout_uses_the_dim_style_text_height`.
- **Acceptance:** the frame renders through the existing `render::tolerance_in` path with no change;
  the test asserts the produced entity's `text_size` equals `Tolerance::text_size`.
- **Risk:** a frame attached to a face that is *hidden* in the view still points at it, which reads
  as an error on the sheet. Fallback: `feature_point` returns the projection of the visible side of
  the feature when the face is `Back` (`DRW-VIEW-005`'s `Facing`), and a frame on a fully hidden
  feature is reported by `DRW-XRF` when the view updates.

### DRW-GDT-005 — The frame's contents from a structured spec, not a hand-typed string
- **State:** READY
- **Depends on:** DRW-GDT-004
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct FrameSpec {
      pub characteristic: cadcraft_doc::GdtSymbol,
      pub zone: Option<(f64, f64)>,            // (plus, minus) in the document's unit
      pub material: Option<cadcraft_doc::GdtSymbol>,   // Mmc | Lmc | Rfs | Projected
      pub datums: Vec<(cadcraft_doc::GdtSymbol, char)>,
      pub segment: Option<usize>,
      pub all_around: bool,                    // the ⌒ symbol
      pub at_mmc: bool,                        // (M) after a zone
  }
  pub fn frame_text(spec: &FrameSpec) -> Result<String, DrwError>;
  pub fn parse_frame_text(text: &str) -> Result<Vec<FrameSpec>, DrwError>;
  ```
- **Behaviour:** `frame_text` builds the DXF "visual representation string" — rows separated by
  `^J`, compartments by `%%v`, symbols as `{\Fgdt;…}` runs (`crates/doc/src/gdt.rs:4-6`) — from
  structure, and `parse_frame_text` is its inverse, so a frame read from a file and a frame written
  from a spec agree. This is the one place the format's string convention is produced, and it is
  produced by a function with a round-trip test.
- **Refusals:** more than `MAX_FRAME_ROWS = 8` rows, more than `MAX_FRAME_CELLS = 12` compartments or
  more than `MAX_FRAME_CHARS = 4096` characters is refused with the count (`gdt.rs:12-14` — the same
  caps the 2D path enforces); a datum letter outside `A..=Z` is refused; a zone with a negative
  tolerance or a non-finite one is refused; a characteristic with a zone that is not a
  position/diameter one is refused (a flatness zone is 0 — saying so beats drawing one).
- **Tests:** `a_frame_text_round_trips`. `a_position_frame_with_mmc_and_two_datums_serialises`. `an_
  all_around_symbol_round_trips`. `a_flatness_zone_of_zero_is_written_as_no_zone`. `rows_and_cells_
  _are_capped_with_the_count`. `a_bad_letter_is_refused`. `a_frame_from_a_file_parses_into_specs`.
- **Acceptance:** the round trip is byte-exact, and the caps match `gdt.rs`'s constants by
  construction (the test compares them).
- **Risk:** the `{\Fgdt;…}` run syntax is a DXF convention CADCraft reads but never uses the `gdt`
  font for — the point of `symbol_prims`. Fallback: the writer emits the codes and the *renderer*
  draws them from geometry; the font is never loaded, which the module doc already states.

### DRW-GDT-006 — Material conditions and the zone, in the right compartments
- **State:** READY
- **Depends on:** DRW-GDT-005
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn with_mmc(s: FrameSpec) -> FrameSpec;   /// adds Ⓜ after the zone
  pub fn datum_with_material(datum: (GdtSymbol, char), m: GdtSymbol) -> FrameSpec;
  pub fn zone_shape(m: GdtSymbol) -> Option<char>;   // M/L/S circled letters, P projected
  ```
- **Behaviour:** The compartment order of the ISO frame is fixed and this task is where it is
  encoded: characteristic, zone, material condition, datum references. MMC on the *zone* is `Ⓜ` in
  the zone's compartment (`(M)` when the projected zone is also shown); MMC on a *datum* is the
  circled letter after the datum letter. Both exist because they mean different things and putting
  one where the other belongs is a drawing that means the wrong tolerance.
- **Refusals:** a projected zone without a position characteristic is refused; an LMC on a zone is
  refused (ISO has no LMC zone — "least material condition" belongs to a datum reference only); a
  material condition on a characteristic with no zone is refused naming it.
- **Tests:** `mmc_after_a_zone_is_in_the_zones_compartment`. `mmc_on_a_datum_is_after_the_letter`.
  `a_projected_zone_requires_position`. `lmc_on_a_zone_is_refused`. `a_zone_without_material_is_
  accepted`. `every_combination_round_trips`.
- **Acceptance:** each combination's serialised text is pinned in the test.
- **Risk:** ISO 1101 and ASME Y14.5 differ in exactly this corner, and CADCraft cannot know which
  standard a drawing follows. Fallback: the standard is a **frame-style field on the sheet**
  (`TitleBlock.fields`/`Sheet`), and the default is ISO; `docs/geometry-parity.md` states it.

### DRW-GDT-007 — Datum references and the primary/secondary/tertiary brackets
- **State:** READY
- **Depends on:** DRW-GDT-005
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The brackets after a datum reference: none, one, two (primary/secondary/tertiary).
  pub fn reference_kind(c: GdtSymbol) -> DatumKind;
  pub fn datum_refs(datums: &[(GdtSymbol, char)]) -> Result<String, DrwError>;
  /// The datum reference frame's order on the sheet, and the refusal when it is not a total order.
  pub fn drf_order(datums: &[(GdtSymbol, char)]) -> Result<Vec<char>, DrwError>;
  ```
- **Behaviour:** The three datum kinds (positional `⌖`, concentricity/circularity, symmetry) decide
  the brackets, and the datum reference frame is a **total order** over the datum letters — which
  every drawing must state, because a frame with no order is not a frame.
- **Refusals:** two datums with the same letter is refused naming both; a datum letter repeated in
  the frame's datum list is refused; an empty datum list is refused; a chain that would form a cycle
  (`A|B` and `B|A`) is refused naming the pair.
- **Tests:** `a_positional_datum_has_no_brackets`. `a_circularity_datum_has_one`. `a_symmetry_datum_
  has_two`. `the_drf_is_a_total_order`. `a_repeated_datum_is_refused`. `a_cyclic_drf_is_refused`.
- **Acceptance:** the serialised text for each kind is pinned.
- **Risk:** n/a; this is a data task with a round-trip test.

### DRW-GDT-008 — Basic dimensions: the boxed value
- **State:** READY
- **Depends on:** DRW-DIM-013
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Mark a dimension as basic: its text is drawn inside a rectangle, per ISO.
  pub fn mark_basic(d: &mut cadcraft_doc::Dimension) -> Result<(), DrwError>;
  pub fn basic_box(d: &cadcraft_doc::Dimension, st: &DimStyle) -> Result<Bounds2, DrwError>;
  ```
- **Behaviour:** A basic dimension is an ordinary `Dimension` (the same entity, the same measurement,
  the same style) with a rectangle drawn around its text. The box is computed from the text's
  measured width through the existing `render::dim_text` measurement path, so it is the same box the
  renderer would need, not an estimate.
- **Refusals:** a dimension with a suppressed text (`" "`) cannot be basic — refused naming the rule;
  a box whose size is not finite and positive is refused; a basic dimension whose text is
  multi-line (an MText override with `\P`) is refused, since ISO's box is one line.
- **Tests:** `a_basic_dimension_gets_a_box_around_its_text`. `the_box_is_the_measured_text_plus_a_
  margin`. `a_suppressed_text_cannot_be_basic`. `a_multi_line_text_is_refused`. `the_box_survives_a_
  _style_change` (recomputed, not stored).
- **Acceptance:** the box is recomputed from the style at draw time, so a style change moves it; the
  test asserts that.
- **Risk:** the box is drawn as four extra segments, which a reader of the DXF sees as geometry.
  Fallback: it goes on the `cadcraft-dim` layer so it can be frozen off, and it is regenerated with
  the dimension, never stored separately.

### DRW-GDT-009 — The projected tolerance zone symbol and what it means in this view
- **State:** READY
- **Depends on:** DRW-GDT-006
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Adds the projected-zone ⓟ and the note the symbol requires.
  pub fn projected_zone(s: FrameSpec, tol: f64) -> Result<(FrameSpec, String /*note*/), DrwError>;
  ```
- **Behaviour:** `ⓟ` after the material condition plus a note stating the projected distance (the
  longest feature length by default, per ISO). The note is a `SheetNote` (`DRW-SHE-013`'s block) and
  the frame's projected zone is only honest when the drawing says how long the projection is — so
  the two are produced together and neither is produced alone.
- **Refusals:** a projected zone without a position characteristic is refused; a projected distance
  that is not finite and positive is refused; a projected distance larger than the feature is
  refused naming both.
- **Tests:** `the_symbol_and_the_note_are_produced_together`. `the_note_states_the_distance`. `a_
  non_position_characteristic_is_refused`. `a_distance_larger_than_the_feature_is_refused`.
- **Acceptance:** the note appears in the sheet's general notes with the frame's own reference, so a
  reader can find it.
- **Risk:** the feature's length is not always the longest edge (a cylinder's length is its axis
  extent). Fallback: the distance is a **parameter** with the feature's own dimension as the default,
  and the report prints what was used.

### DRW-GDT-010 — A tolerance value driven by a model parameter
- **State:** BLOCKED — needs `MOD`'s parameter store (`crates/model/src/feature.rs:98`, owner **MOD**)
- **Depends on:** DRW-GDT-005, MOD-PARAM (the `Param` store)
- **Files:** `crates/drawing/src/gdt/spec.rs` (owner **DRW**); the value source is an **interface
  request** to **MOD**
- **Contract:**
  ```rust
  /// A tolerance zone whose value comes from a named parameter, resolved by the caller.
  pub struct ParamZone { pub parameter: String, pub factor: f64 }
  pub fn param_zone_text(p: &ParamZone, values: &std::collections::BTreeMap<String, f64>)
      -> Result<String, DrwError>;
  // REQUEST to MOD: the engine resolves `model.values()` and hands the map in. cadcraft-drawing
  // may not reach cadcraft-model (L3).
  ```
- **Behaviour:** The tolerance is a named parameter times a factor, so a tolerance table (a common
  spreadsheet-driven practice) drives the frames. The **resolved number is also stored** in the
  frame text at draw time, because a drawing that shows `0.05` is readable by a shop and a drawing
  that shows `tol_a2` is not.
- **Refusals:** a parameter that does not exist is `Refused("no parameter named `{n}`")`; a value
  that is not finite and positive is refused naming the parameter; a factor that is not finite and
  positive is refused; a cyclic expression is refused by `ModelError::Cyclic`, passed through.
- **Tests:** `a_parameter_drives_the_zone`. `the_resolved_number_is_what_is_drawn`. `an_unknown_
  parameter_is_refused`. `a_non_finite_value_is_refused`. `a_parameter_that_changes_moves_the_frame`.
- **Acceptance:** the frame text contains the resolved number, asserted; the parameter name is kept
  in the sheet's report for traceability.
- **Risk:** the frame text then says `0.05` while the parameter says `0.05`, and a later parameter
  change makes the text stale until redrawn. Fallback: the parameter name is stored beside the frame
  (the `SheetNote` channel) and `DRW-GDT-015` re-resolves on every update.

### DRW-GDT-011 — A frame is associated to its 3D face, through `Assoc3d`
- **State:** READY
- **Depends on:** DRW-GDT-004, DRW-DIM-001
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct FrameLink { pub frame: cadcraft_doc::Handle, pub assoc: Assoc3d }
  pub fn link_frames(frames: &[FrameLink], part: &ScenePart) -> Result<Vec<Resolution3d>, DrwError>;
  ```
- **Behaviour:** Exactly `DRW-DIM-021`'s resolution, applied to frames: a face is recognised again
  after a rebuild and the frame re-attaches **only** on an exact match (same plane within tolerance,
  same area). A frame whose face moved is `Approximate` with the distance; a replaced face is
  `Stale`. Nothing is re-pointed automatically.
- **Refusals:** the same three as `DRW-DIM-021` — a candidate beyond `10 × tolerance_for` is stale;
  a part that is not in the scene is stale naming it; a budget over 100 000 is refused.
- **Tests:** `an_unchanged_face_keeps_its_frame`. `a_moved_face_is_approximate_with_a_distance`. `a_
  replaced_face_is_stale_and_never_repointed`. `link_frames_is_deterministic`.
- **Acceptance:** the "never re-pointed" test again — the epic's governing test applies here too.
- **Risk:** n/a; reuse.

### DRW-GDT-012 — Flatness and straightness on a projected planar face
- **State:** READY
- **Depends on:** DRW-GDT-011, DRW-VIEW-005
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The projected outline of a planar face, for a flatness frame's optional leader and for
  /// the check that the face really is planar.
  pub fn projected_outline(r: &Recognised, frame: &ViewFrame) -> Result<Vec<Vec2>, DrwError>;
  pub fn flatness_frame(face: &cadcraft_solid::Face, zone: f64, style: &DimStyle)
      -> Result<FrameSpec, DrwError>;
  ```
- **Behaviour:** Flatness applies to a planar face and is exactly expressible in the kernel's model:
  a face **is** a plane, so the frame's claim ("this surface lies between two planes `zone` apart")
  is true by construction and its area is `Face::area()` exactly. `projected_outline` gives the
  leader's attachment so the frame sits under the face it describes.
- **Refusals:** flatness on a non-planar (faceted) target is `Refused("flatness needs a planar face;
  face {i} is a {kind}")` — offering a flatness frame on a faceted cylinder is exactly the kind of
  lie this domain refuses; an area below `tolerance_for` is refused.
- **Tests:** `a_flatness_frame_on_a_planar_face`. `the_face_area_is_the_exact_area`. `flatness_on_a_
  faceted_cylinder_is_refused_by_kind`. `the_outline_is_the_projected_loop`. `an_outline_with_fewer_
  _than_three_points_is_refused`.
- **Acceptance:** the refused case is the acceptance: CADCraft will not annotate a shape it cannot
  measure.
- **Risk:** a **tolerance** flatness is a claim about a manufactured part, not about the model; a
  drawing can state a flatness on a faceted surface (a real part can be flat within tolerance). The
  refusal is therefore about *recognition*, not about the claim. Fallback: `flatness_frame` takes an
  explicit `Recognised`, so a user may annotate any face with flatness; only the automatic
  recognition refuses.

### DRW-GDT-013 — Cylindricity and circularity of a projected cylinder
- **State:** BLOCKED — needs `KER-SRF-018`/`KER-SRF-023` (`Surface`, evaluation and normals)
- **Depends on:** DRW-GDT-001, KER-SRF-018, KER-SRF-023
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  // NEEDS KER-SRF-018 + KER-SRF-023 (not defined here):
  // pub enum Surface { Plane(Plane), Cylinder { axis: [Vec3;2], radius: f64 },
  //                    Cone{..}, Sphere{..}, Torus{..}, Nurbs(Box<NurbsSurface>) }
  // impl Surface { pub fn eval(&self, u: f64, v: f64) -> Result<Vec3, SolidError>;
  //               pub fn normal(&self, u: f64, v: f64) -> Result<Vec3, SolidError>; }
  pub fn circularity_frame(face: &cadcraft_solid::Face, zone: f64) -> Result<FrameSpec, DrwError>;
  ```
- **Behaviour:** On a faceted cylinder, circularity is a claim the kernel **cannot measure**: the
  shape is an n-gon prism, so every section is exactly a polygon and "circularity 0.02" is
  unmeasurable against this model. Therefore this task refuses by default, and lands the annotation
  the day the kernel has a real `Surface`. What it ships *now* is the refusal, its message and the
  test that pins it, so the gap is visible rather than absent.
- **Refusals:** circularity or cylindricity on a faceted target is
  `Err(DrwError::NeedsKernel("circularity needs a curved face; the kernel is planar-faced (KER-SRF-018, KER-SRF-023)"))` — a **named** refusal, never a frame that means nothing.
- **Tests:** `circularity_on_a_faceted_cylinder_is_refused_with_the_task_ids`. `the_refusal_names_
  _what_was_asked`. `a_planar_face_takes_flatness_not_circularity`.
- **Acceptance:** the refusal exists and its message names the two kernel tasks; when they land,
  `KER-SRF-018` replaces this task's State and the frame is implemented against it.
- **Risk:** a mechanical part full of bores cannot be annotated at all until the kernel curves.
  Fallback: the user may place a **free** frame (`FrameAttach::AtPoint`, no association) with any
  text, which is what a drawing does today in every CAD; it is simply not *verified* against the
  model, and the sheet's report says which frames are associated and which are not.

### DRW-GDT-014 — A position tolerance on a hole pattern, and where its frame goes
- **State:** BACKLOG
- **Depends on:** DRW-GDT-011, DRW-GDT-009
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Group the recognised cylindrical/prismatic features into datum-referenced patterns.
  pub fn hole_patterns(rs: &[Recognised], tol: f64) -> Result<Vec<HolePattern>, DrwError>;
  #[derive(Clone, Debug, PartialEq)]
  pub struct HolePattern { pub axis: [Vec3; 2], pub faces: Vec<usize>, pub spacing: Vec<f64> }
  ```
- **Behaviour:** Faces whose axes are parallel within `dir_tol` and whose centres lie on a line
  perpendicular to the axis are a pattern; one position frame with a basic pitch covers the pattern,
  which is the drafting convention. The pattern is found from geometry, not from a feature record —
  the 3D model has no hole feature.
- **Refusals:** a group of coaxial faces is **not** a pattern (they are one hole, split by a boolean)
  and is refused naming them; a pattern whose spacing is not within `tol` of a common value is not a
  pattern; more than 256 faces in one group is `Limit`.
- **Tests:** `four_equally_spaced_holes_are_one_pattern`. `coaxial_faces_are_not_a_pattern`. `an_
  uneven_spacing_is_not_a_pattern`. `a_pattern_over_the_cap_is_refused`. `the_axis_is_reported_with_
  _its_angle`.
- **Acceptance:** the grouping is deterministic and its result is pinned for the sample drawing.
- **Risk:** a pattern of 4 holes and a row of 6 coincidental holes are the same geometry. Fallback:
  the grouping is reported with its confidence and the user confirms it by placing the frame, which
  is an explicit act.

### DRW-GDT-015 — Runout and total runout
- **State:** BLOCKED — needs `KER-SRF-018`, `KER-SRF-033` (a trimmed surface and its tangency)
- **Depends on:** DRW-GDT-013, KER-SRF-018, KER-SRF-033
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  // NEEDS KER-SRF-033: `pub struct BFace { surface: Surface, outer: Vec<BCurve>, inner: Vec<BCurve> }`
  pub fn runout_frame(surface_index: usize, datum: char, total: bool, zone: f64)
      -> Result<FrameSpec, DrwError>;
  ```
- **Behaviour:** Runout is a claim about a **tangent-continuous** surface meeting a datum; the
  kernel has no tangent datum (a face is a plane and a region), so the whole characteristic is
  `BACKLOG` behind the surface work. What lands now is the refusal and the parameter signature, so
  the epic's shape is fixed before the kernel is.
- **Refusals:** `Err(DrwError::NeedsKernel("runout needs a trimmed surface and its tangency (KER-SRF-033)"))`.
- **Tests:** `runout_is_refused_with_the_task_id`. `the_refusal_names_the_characteristic`.
- **Acceptance:** the refusal, pinned.
- **Risk:** runout is common on turned parts, which are exactly the parts CADCraft cannot model yet.
  Fallback: a free, unverified frame, as in `DRW-GDT-013`.

### DRW-GDT-016 — The datum reference frame table on the sheet
- **State:** BACKLOG
- **Depends on:** DRW-GDT-007, DRW-SHE-004
- **Files:** `crates/drawing/src/gdt/table.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn drf_table(frames: &[FrameSpec], datums: &[(GdtSymbol, char)], style: &str, at: Vec2)
      -> Result<TablePlan, DrwError>;
  ```
- **Behaviour:** A `Table` (as the parts list, `DRW-SHE-010`) listing each frame's reference, its
  characteristic, its zone and its datums, plus a header row naming the standard in use (ISO 1101 or
  ASME Y14.5, `DRW-GDT-006`). It is regenerated from the frames, so it cannot disagree with them.
- **Refusals:** a frame whose text does not parse (`parse_frame_text`) is listed as unparsed rather
  than dropped; a table taller than the printable area is refused with both heights.
- **Tests:** `the_table_lists_every_frame_once`. `the_table_is_regenerated_not_edited`. `an_unparsed_
  _frame_is_listed_as_unparsed`. `the_standard_is_in_the_header`. `a_table_taller_than_the_sheet_is_
  refused`.
- **Acceptance:** the table's content is asserted against the frames that produced it.
- **Risk:** low. Fallback: none needed.

### DRW-GDT-017 — GD&T update after a model change, and the stale report
- **State:** BLOCKED — needs `MOD-REC-001` (owner **MOD**), like `DRW-DIM-020`
- **Depends on:** DRW-GDT-011, DRW-GDT-010
- **Files:** `crates/drawing/src/gdt/frame.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct GdtUpdate { pub frame: String, pub zone_before: f64, pub zone_after: f64,
                         pub reattached: bool, pub stale: Option<String> }
  pub fn update_frames(links: &[FrameLink], part: &ScenePart, values: &BTreeMap<String, f64>,
                       tol: f64) -> Result<(Vec<FrameSpec>, Vec<GdtUpdate>), DrwError>;
  ```
- **Behaviour:** Parameter-driven zones re-resolve (`DRW-GDT-010`); associations re-resolve
  (`DRW-GDT-011`); a frame that cannot re-attach keeps its old spec and is reported stale. The
  report lists every frame with before/after, so a sheet that changed says how.
- **Refusals:** a parameter that no longer exists is refused naming it (the frame keeps its old
  value and is reported); an association past tolerance is stale, never re-pointed.
- **Tests:** `a_parameter_change_updates_the_zone`. `an_unchanged_face_keeps_its_frame`. `a_moved_
  face_is_reported_not_reattached`. `a_deleted_parameter_is_refused_and_reported`. `the_update_is_
  idempotent`. `the_report_is_sorted_by_frame`.
- **Acceptance:** the report is empty for a rebuild that changed nothing.
- **Risk:** n/a; reuse of `DRW-DIM-020`'s rule. The `BLOCKED` state above is the only state; it is
  blocked on `MOD-REC-001` exactly as `DRW-DIM-020` is.

### DRW-GDT-018 — The epic's refusals, and hostile frame strings
- **State:** READY
- **Depends on:** DRW-GDT-005, DRW-GDT-008
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn hostile_gdt_never_panics() -> bool;`
- **Behaviour:** The recognition → frame → link → update path over hostile input: 200 mutations of a
  valid frame string (including `{\Fgdt;` with no terminator, `^J` runs of 10 000 rows, a lone
  surrogate, an unpaired `%%v`, a zone of `1e400`), a `Recognised` with `confidence: 2.0`, a
  `FrameSpec` with 100 datums, a `tol` of `0` and of `-1`, a style with `text_height` of `1e-300`.
  Every case is `Ok` or `Err` with a named variant; none panics or allocates from an unchecked
  number.
- **Refusals:** the existing `MAX_FRAME_*` caps fire first, so a mutated string is refused rather than
  parsed; `BadTolerance`-class refusals name the number.
- **Tests:** `hostile_gdt_never_panics`. `an_unterminated_gdt_run_is_refused`. `ten_thousand_rows_
  are_refused_at_the_cap`. `a_lone_surrogate_is_refused`. `every_refusal_names_a_count_or_a_number`.
  `recognition_of_a_nan_face_is_refused`.
- **Acceptance:** this task is the epic's gate.
- **Risk:** `parse_frame_text` reads a file's string, so it is the most hostile input in the epic.
  Fallback: the caps are checked **before** any allocation, exactly as `crates/doc/src/gdt.rs:12`
  documents.

---

# DRW-LEAD — leaders, balloons, labels and the notes they point at

Every entity here is an existing 2D annotation: `Leader` (`entity.rs:690`), `MLeader` (`entity.rs:704`),
`MText` (`entity.rs:258`), `Table`. What the 3D adds is the **anchor**: a leader's arrowhead lands on
the projection of a 3D point or face, and a balloon's item number comes from the parts list.

### DRW-LEAD-001 — A leader whose arrow lands on the projection of a 3D point
- **State:** READY
- **Depends on:** DRW-VIEW-004, DRW-GDT-004
- **Files:** `crates/drawing/src/lead/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn leader_on_view(anchor3d: Vec3, points: &[Vec3], frame: &ViewFrame, style: &DimStyle)
      -> Result<cadcraft_doc::Leader, DrwError>;
  pub fn mleader_on_view(anchors: &[Vec3], landing: Vec3, frame: &ViewFrame,
                         style: &str) -> Result<cadcraft_doc::MLeader, DrwError>;
  ```
- **Behaviour:** The arrow vertex is `frame.project(anchor3d)` and the rest of the vertices are the
  user's picked points, projected. The entity is `EntityKind::Leader`/`MLeader` with
  `style = DIMSTYLE`, and the existing code that draws it (`render/src/lib.rs::build_entities`) is
  unchanged. The 3D anchor is kept as an `Assoc3d` on the view's association map, so the leader
  follows the model — which is what makes a leadered note worth having.
- **Refusals:** fewer than two points is `Refused("a leader needs 2+ points")` — the 2D message,
  verbatim (`annotate.rs:481`); an anchor that is not finite is refused; a point that projects to the
  same place as its neighbour is refused (`tolerance_for` of the view's diagonal); a style that does
  not exist is refused listing the known names.
- **Tests:** `the_arrow_lands_on_the_projection_of_the_3d_point`. `a_leader_needs_two_points`. `coincident_
  points_are_refused`. `a_leader_on_a_view_matches_the_2d_constructor`. `the_mleader_landing_is_the_
  picked_point`. `a_non_finite_anchor_is_refused`.
- **Acceptance:** the parity test against `annotate.rs::run_leader`'s entity shape.
- **Risk:** an anchor on a **hidden** face points at nothing visible. Fallback: `leader_on_view`
  takes the anchor's `Facing` (from `DRW-VIEW-005`) and the sheet's report lists leaders whose
  anchor is on a hidden face.

### DRW-LEAD-002 — Balloons: the circle, the leader, the item number
- **State:** READY
- **Depends on:** DRW-LEAD-001, DRW-SHE-010
- **Files:** `crates/drawing/src/lead/balloon.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Balloon { pub item: usize, pub centre: Vec2, pub radius: f64, pub anchor3d: Vec3 }
  pub fn balloon(b: &Balloon, frame: &ViewFrame, st: &DimStyle) -> Result<BalloonEntities, DrwError>;
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct BalloonEntities { pub circle: Vec<([Vec2; 2])>, pub leader: cadcraft_doc::Leader,
                                pub text: Option<cadcraft_doc::MText> }
  ```
- **Behaviour:** A balloon is a **circle plus a leader plus an item number**, drawn from our own
  geometry as `Line` segments (no font, no symbol font, no asset), with the number as `MText`
  centred. `radius` comes from the style's text height, so a balloon scales with the sheet's
  annotation scale like everything else (`DRW-SHE-008`).
- **Refusals:** an item number of 0 is refused (numbering starts at 1); a radius that is not finite
  and positive is refused; a balloon whose circle would extend outside the printable area is
  reported by `DRW-SHE-014`, not drawn off the sheet; a balloon with no anchor is refused.
- **Tests:** `a_balloon_is_a_circle_a_leader_and_a_number`. `the_number_is_centred`. `the_radius_
  follows_the_style_text_height`. `an_item_of_zero_is_refused`. `the_circle_is_our_own_geometry`. `a
  _balloon_at_scale_one_half_is_half_the_size`.
- **Acceptance:** the balloon's radius and text height are asserted against the style, and no file
  asset is created (so `cargo xtask assets` is unaffected).
- **Risk:** a balloon drawn with segments has visible facets at plot resolution. Fallback: the
  radius is in paper millimetres and the plot resolution is 300 dpi by default; the test asserts the
  chord error is below a tenth of a pixel.

### DRW-LEAD-003 — Balloon placement: a row along the view, horizontal text, clear of the geometry
- **State:** READY
- **Depends on:** DRW-LEAD-002, DRW-DIM-018
- **Files:** `crates/drawing/src/lead/balloon.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn place_balloons(anchors: &[Vec3], frame: &ViewFrame, at: Vec2, direction: Vec2,
                       gap: f64, st: &DimStyle) -> Result<Vec<Balloon>, DrwError>;
  /// Re-place the whole row after the view moved.
  pub fn reposition(view: &SheetView, balloons: &[Balloon], geom: &ViewGeometry,
                    paper: Bounds2, gap: f64) -> Result<Vec<Balloon>, DrwError>;
  ```
- **Behaviour:** Balloons are placed in **index order** along `direction` from `at`, each spaced by
  `gap` (default the style's text height × 3), with the text always horizontal regardless of the
  view's rotation — the drafting convention, and the reason a balloon is drawn as `MText` with an
  explicit rotation rather than as a dimension. The leader goes from the balloon's edge to the
  anchor, shortened by the radius so it starts on the circle.
- **Refusals:** a `direction` that is not a unit vector (or zero length) is refused; a `gap` that is
  not finite and positive is refused; a row longer than the printable area is refused with the
  length and the area, and the caller may then use two rows.
- **Tests:** `balloons_are_placed_in_index_order`. `the_text_is_horizontal_in_a_rotated_view`. `the
  _leader_starts_on_the_circle`. `the_row_spacing_is_three_text_heights`. `a_row_longer_than_the_
  sheet_is_refused`. `reposition_keeps_the_order_and_the_gap`.
- **Acceptance:** the placement is deterministic and the order is the item order
  (`DRW-LEAD-004`).
- **Risk:** a row of balloons across a view crosses the geometry. Fallback: the caller places rows
  per view, outside the view rectangle, which is the convention; `place_balloons` does not decide
  which side.

### DRW-LEAD-004 — Item numbering: a stated sort key, stable across every update
- **State:** READY
- **Depends on:** DRW-LEAD-002
- **Files:** `crates/drawing/src/lead/balloon.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Sort parts into item order. The key is stated, total, and never a hash order.
  pub fn number_parts(parts: &[String]) -> Result<Vec<(usize, String)>, DrwError>;
  /// Merge new parts into an existing numbering, keeping existing item numbers.
  pub fn renumber(parts: &[String], existing: &[(usize, String)]) -> Result<Vec<(usize, String)>, DrwError>;
  ```
- **Behaviour:** `number_parts` sorts by `(part name, then ScenePart order)` with
  `f64`-free `Ord` on the name — case-sensitive, byte order, stated — and assigns 1..=n. `renumber`
  keeps the item number every existing part already had and appends new parts after them, so adding
  a part to a drawing does not renumber the sheet (the rule a drafter depends on).
- **Refusals:** a duplicate part name is refused naming both; `existing` with a duplicate item
  number is refused; more than 10 000 parts is `Limit`; an empty part list returns an empty list,
  not an error.
- **Tests:** `numbering_is_sorted_by_name`. `renumber_keeps_existing_numbers`. `a_new_part_gets_the_
  next_number`. `a_removed_part_does_not_shift_the_others`. `numbering_is_byte_identical_twice` (the
  determinism rule). `a_duplicate_name_is_refused`. `two_runs_with_different_input_orders_agree`.
- **Acceptance:** the "two runs with different input orders agree" test is the acceptance: it is
  the `HashMap` iteration trap, named.
- **Risk:** a removed part's number is kept free, so numbering develops gaps. Fallback: `renumber`
  reports the gaps; `number_parts` (from scratch) has none.

### DRW-LEAD-005 — The parts list and the balloons are one numbering
- **State:** BLOCKY READY — depends DRW-SHE-010 (rows)
- **Depends on:** DRW-LEAD-004, DRW-SHE-010
- **Files:** `crates/drawing/src/lead/balloon.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn balloons_and_rows(parts: &[String], existing: &[(usize, String)])
      -> Result<(Vec<(usize, String)>, Vec<PartsRow>), DrwError>;
  /// Balloons with no matching row, and rows with no balloon: both are findings.
  pub fn linkage_report(balloons: &[Balloon], rows: &[PartsRow]) -> Vec<String>;
  ```
- **Behaviour:** One numbering, produced once, feeding both the balloons and the parts list
  (`DRW-SHE-010`'s `PartsRow { item, .. }`). `linkage_report` is the honest half: a balloon whose item
  has no row, and a row whose item has no balloon, are findings — a sheet where they disagree is a
  sheet a shop will query.
- **Refusals:** a row whose item is not in the numbering is refused naming both; two balloons with
  one item number are refused naming both anchors.
- **Tests:** `every_balloon_has_a_row`. `every_row_has_a_balloon`. `a_mismatch_is_a_finding_not_a_
  crash`. `the_numbering_is_computed_once`. `a_duplicate_item_is_refused`.
- **Acceptance:** `linkage_report` is empty for a well-formed sheet, asserted.
- **Risk:** the parts list's row source is undecided until `MOD-BOD-019`/`ASM` land
  (`DRW-SHE-010`). Fallback: rows come from the `ScenePart`s the view was built from; the report
  says the sheet is in single-body mode.

### DRW-LEAD-006 — View labels: the name, the scale and the projection symbol
- **State:** READY
- **Depends on:** DRW-SHE-003, DRW-VIEW-011
- **Files:** `crates/drawing/src/lead/label.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn view_label(view: &SheetView, sheet: &Sheet) -> Result<(String, String /*scale text*/), DrwError>;
  pub fn projection_symbol(angle: ProjectionAngle, at: Vec2, size: f64)
      -> Result<Vec<([Vec2; 2])>, DrwError>;
  ```
- **Behaviour:** Under each view, two `MText` lines: the view's name (`VIEW A`, or the standard
  view's name) and its scale in the form `SCALE 1:2`, using `ViewScale::display` (`DRW-VIEW-011`).
  The projection symbol is the truncated-cone frustum of ISO 5456, drawn from our own segments in
  the **first-angle** form when the sheet says first and in the mirrored form when it says third —
  the mirror is the whole point of the symbol and getting it wrong misleads the reader.
- **Refusals:** a scale that is not finite and positive is refused; a size that is not finite and
  positive is refused; a view whose label would fall outside the printable area is reported by
  `DRW-SHE-014`.
- **Tests:** `the_label_names_the_view_and_its_scale`. `the_projection_symbol_mirrors_between_
  conventions`. `the_symbol_is_drawn_from_our_geometry`. `a_view_scale_of_one_half_displays_as_one_
  to_two`. `a_non_finite_scale_is_refused`.
- **Acceptance:** the mirrored-symbol test, with the two variants asserted to be mirror images of
  each other (a geometric check, not a picture).
- **Risk:** the symbol's orientation conventions are a standard detail people get wrong. Fallback:
  the convention is a sheet field (`DRW-GDT-006`'s standard), ISO by default, and the label states
  the method in words as well as by symbol.

### DRW-LEAD-007 — Section marks: the cutting line, its arrows and the letter
- **State:** BACKLOG
- **Depends on:** DRW-SHE-009, DRW-LEAD-006
- **Files:** `crates/drawing/src/lead/label.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn section_mark(s: &SheetSection, parent: &SheetView, geom: &ViewGeometry,
                      letter: char) -> Result<MarkEntities, DrwError>;
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct MarkEntities { pub cut_line: Vec<([Vec2; 2])>, pub arrows: Vec<([Vec2; 2])>,
                            pub ends: Vec<([Vec2; 2])>, pub labels: Vec<cadcraft_doc::MText> }
  ```
- **Behaviour:** The cutting line is drawn across the parent view where the section plane crosses
  it (from `DRW-SHE-009`'s `SheetSection.plane` projected), with short ends, arrows pointing in the
  look direction, and the letter at each arrow — the section's own letter from the `SheetSection`.
  The cutting line is drawn with the `cadcraft-section` layer's centre linetype, which is already a
  table in `crates/doc/src/library.rs`.
- **Refusals:** a section plane parallel to the parent view produces no cut line and is
  `Refused("the section plane is parallel to view `{v}`")`; a letter outside `A..=Z` is refused; a
  section whose plane does not cross the view's geometry is refused naming the plane.
- **Tests:** `the_cut_line_is_where_the_plane_crosses_the_view`. `the_arrows_point_along_the_look_
  direction`. `the_letter_is_at_both_ends`. `a_parallel_plane_is_refused`. `the_cut_line_uses_the_
  section_linetype`.
- **Acceptance:** the cut line's endpoints are within `tolerance_for` of the plane/view
  intersection, asserted against `cadcraft_solid::section`'s own region.
- **Risk:** a section plane that misses the view's silhouette entirely. Fallback: refused with the
  plane's offset from the view's bounds.

### DRW-LEAD-008 — Keyed notes and the balloon that points at one
- **State:** READY
- **Depends on:** DRW-SHE-013, DRW-LEAD-002
- **Files:** `crates/drawing/src/lead/note.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct KeyedNote { pub key: usize, pub text: String, pub at: Vec2 }
  pub fn keyed_note(note: &KeyedNote, anchor: Option<Vec3>, frame: &ViewFrame, st: &DimStyle)
      -> Result<(Vec<cadcraft_doc::EntityKind>, Option<Balloon>), DrwError>;
  pub fn parse_key(text: &str) -> Result<usize, DrwError>;
  ```
- **Behaviour:** A note is a balloon whose text is a number and a row in the note block, the drafting
  convention for "4 × M6×1.0 DEPTH 12". `parse_key` accepts `(4)`, `4`, `NOTE 4` and nothing else —
  the strictness is what stops a note's own text from being read as a key.
- **Refusals:** a key of 0 or above 999 is refused; text over 512 characters is refused; a note whose
  key is already used by another note on the sheet is refused naming both; an anchor that is not
  finite is refused.
- **Tests:** `a_keyed_note_makes_a_numbered_balloon_and_a_row`. `parse_key_accepts_the_three_spellings`.
  `a_duplicate_key_is_refused`. `a_key_of_zero_is_refused`. `a_note_without_an_anchor_has_no_leader`.
  `the_note_text_survives_a_style_change`.
- **Acceptance:** the round trip from note text to key to balloon text to note row.
- **Risk:** a note balloon looks exactly like a part balloon, which is the convention's fault, not
  ours. Fallback: the note block lists its keys, and the sheet's report counts note balloons and
  part balloons separately.

### DRW-LEAD-009 — Symbol rows in the note block: weld, finish, and the ones we draw in code
- **State:** READY
- **Depends on:** DRW-SHE-013, DRW-GDT-002
- **Files:** `crates/drawing/src/lead/note.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub enum NoteSymbol { Weld { kind: WeldKind, size: f64, length: Option<f64> },
                        SurfaceFinish { roughness: f64, basic: bool },
                        AllAround }
  pub fn note_symbol_entities(s: &NoteSymbol, at: Vec2, st: &DimStyle)
      -> Result<Vec<cadcraft_doc::EntityKind>, DrwError>;
  ```
- **Behaviour:** Every symbol in a note is drawn from our own geometry — the same policy as
  `symbol_prims` in `crates/doc/src/gdt.rs` and as every icon in `crates/ui-egui/src/icons.rs`. No
  font, no image, no file: `cargo xtask assets` is unaffected, and `AGENTS.md`'s "every icon is drawn
  in code" rule holds for the drawing side too. A note row is therefore a mix of `MText` and line
  segments, assembled into one `Table` cell's content.
- **Refusals:** a roughness that is not finite and positive, or below `0.1`, is refused; a weld size
  that is not finite and positive is refused; a weld kind this build does not draw is
  `Refused("weld symbol `{k}` is not implemented")` **naming it**, never approximated with another
  symbol.
- **Tests:** `a_weld_symbol_is_drawn_from_segments`. `a_surface_finish_symbol_shows_its_value`. `the_
  all_around_symbol_is_drawn`. `an_unimplemented_weld_kind_is_refused_by_name`. `no_note_symbol_
  produces_a_file`. `the_symbol_scales_with_the_annotation_scale`.
- **Acceptance:** the test asserts that a note block with every symbol produces only `Line`/`MText`/
  `LwPolyline` entities — i.e. no entity type that would need an external asset.
- **Risk:** an unimplemented weld kind is a visible gap in a mechanical note. Fallback: the row is
  drawn with the **text** only (e.g. the ISO reference in words) and the report says the symbol is
  missing, which is honest and printable.

### DRW-LEAD-010 — Collision resolution for labels, balloons and frames together
- **State:** READY
- **Depends on:** DRW-LEAD-003, DRW-GDT-004, DRW-DIM-018
- **Files:** `crates/drawing/src/lead/label.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub enum Annotation { Balloon(Balloon), Label { rect: Bounds2 }, Frame { rect: Bounds2 }, Leader { rect: Bounds2 } }
  pub fn resolve_collisions(items: &mut [Annotation], geom: &ViewGeometry, paper: Bounds2,
                           gap: f64, budget: usize) -> Result<Vec<usize>, DrwError>;
  ```
- **Behaviour:** One placement search for **all** annotation kinds, not one per kind: candidates are
  tried in a fixed order per item (its own preference first, then offsets in `gap` steps, then along
  the item's own leader direction), against the view's geometry **and** the items already placed. The
  return value names the items that could not be placed. This is `DRW-DIM-018`'s `Occupancy` grid
  reused, which is the point: one algorithm, so a balloon never lands on a dimension because two
  searches did not know about each other.
- **Refusals:** `budget` of 0 is refused; a `gap` that is not finite and positive is refused; an
  unplaced item is **never** left overlapping — it is reported.
- **Tests:** `a_balloon_does_not_land_on_a_dimension`. `an_unplaceable_item_is_reported`. `the_
  resolution_is_deterministic`. `the_budget_is_respected`. `items_keep_their_preferred_position_
  when_it_is_clear`.
- **Acceptance:** on the sample sheet, every annotation is placed clear, asserted through `hits`.
- **Risk:** a greedy pass can paint itself into a corner. Fallback: `budget` bounds it and the
  report names the failures, so a user can move one item by hand and re-run.

### DRW-LEAD-011 — Leader styles on a sheet
- **State:** READY
- **Depends on:** DRW-LEAD-001, DRW-SHE-004
- **Files:** `crates/drawing/src/lead/mod.rs` (owner **DRW**); `MLeaderStyle` is
  `crates/doc/src/tables.rs:594` (owner **C2**) — **read only**
- **Contract:**
  ```rust
  pub fn sheet_leader_style(sheet: &Sheet, d: &Drawing) -> Result<MLeaderStyle, DrwError>;
  pub fn apply_leader_style(m: &mut cadcraft_doc::MLeader, st: &MLeaderStyle, dimscale: f64)
      -> Result<(), DrwError>;
  ```
- **Behaviour:** Leaders use the drawing's `CMLEADERSTYLE` (`crates/engine/src/cmd/annotate.rs:430`),
  which is an existing `MLeaderStyle` with its own annotative flag; a sheet may override the style per
  view the way `DRW-DIM-016` overrides dimensions. `apply_leader_style` sets the arrow size, dogleg
  and landing gap, honouring the annotative rule already in `mleader_kind` (`annotate.rs:429-438`).
- **Refusals:** a style that does not exist is refused listing the known names; an arrow size or
  dogleg that is not finite and non-negative is refused naming the field.
- **Tests:** `the_sheet_style_is_the_drawings_current_style`. `an_annotative_style_uses_the_
  annotation_factor`. `a_per_view_override_changes_only_the_overridden_field`. `an_unknown_style_is_
  refused`. `apply_leader_style_matches_the_2d_constructor`.
- **Acceptance:** the parity test against `mleader_kind`'s arithmetic.
- **Risk:** `MLeaderStyle` has five fields today (`docs/gaps.md` F10), so a leader on a 3D sheet is
  thinner than a full one. Fallback: the sheet's report says which style fields are at their default
  and cannot be changed, rather than pretending the style is complete.

### DRW-LEAD-012 — Arrowheads: which view gets which
- **State:** READY
- **Depends on:** DRW-LEAD-011, DRW-VIEW-003
- **Files:** `crates/drawing/src/lead/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The arrowhead block name for a view (`""` = none). `cadcraft-drawing` may not use
  /// `cadcraft_render::Arrowhead` (L2 sideways), so this returns the **style's block name**
  /// and the engine maps it onto `render::Arrowhead` — the same indirection the 2D path uses.
  pub fn arrow_block_name(view: &SheetView, st: &cadcraft_doc::DimStyle) -> Option<&'static str>;
  /// Why this view's cut line carries no arrow, when it carries none.
  pub fn no_arrow_reason(view: &SheetView) -> Option<&'static str>;
  ```
- **Behaviour:** A **section view** is drawn with no arrows at the cut line (ISO), a detail view with
  a thin arrow (`st.arrowBlock`), and a normal view with the style's arrow
  (`DimStyle.arrowBlock`, `crates/doc/src/tables.rs:147`) — every kind is drawn from our own geometry
  by `render::dim::arrowhead` (`crates/render/src/dim.rs:207`), which is unchanged.
  `no_arrow_reason` returns **why** an arrow is absent, so the absence is a stated convention rather
  than a missing line.
- **Refusals:** an arrow size that is not finite and positive is refused; a view kind this build does
  not know is `Refused("view `{v}` has no arrowhead convention")`.
- **Tests:** `a_section_view_has_no_arrow`. `a_normal_view_has_the_styles_arrow`. `a_thin_arrow_for_
  details`. `the_absent_arrow_explains_itself`. `a_non_finite_size_is_refused`.
- **Acceptance:** `no_arrow_reason` is `Some` exactly when `arrow_block_name` is `None`, asserted.
- **Risk:** the arrowhead *kind* (`closed`/`open`/`tick`/`dot`) lives in `render`'s `Arrowhead`, which
  this crate may not name; returning a name instead of an enum keeps the layering legal and is exactly
  how `DimStyle.arrow_block` already flows to the renderer. Fallback: none needed.

### DRW-LEAD-013 — Balloons on an assembly occurrence
- **State:** BLOCKED — needs `ASM-005`'s occurrence model (`crates/asm`, owner **ASM**); note the
  layering constraint in invariant 7
- **Depends on:** DRW-LEAD-002, ASM-005
- **Files:** `crates/drawing/src/lead/balloon.rs` (owner **DRW**); the occurrence source is
  `ScenePart` (`DRW-VIEW-001`), which **must not** become `cadcraft_asm::Occurrence`
- **Contract:**
  ```rust
  /// A balloon's anchor may name an occurrence, not just a body.
  #[derive(Clone, Debug, PartialEq)]
  pub enum Anchor {
      Part { part: String, point: Vec3 },
      /// Same part, projected through an occurrence's placement. The placement is carried by the
      /// `ScenePart` the view was built from, so this crate never names `cadcraft_asm`.
      Occurrence { part: String, occurrence: usize, point: Vec3 },
  }
  pub fn occurrence_anchor(parts: &[ScenePart], a: &Anchor) -> Result<Vec3, DrwError>;
  ```
- **Behaviour:** On an assembly sheet, each occurrence gets its own item number and the balloon points
  at **that occurrence's** face. Because the view was built from a list of placed `ScenePart`s, this
  works without the assembly crate; the caller (`ENG`) supplies one `ScenePart` per occurrence.
- **Refusals:** an occurrence index outside the parts list is refused naming both; two occurrences
  of the same component at the same place is `Refused("two occurrences share one position")`; an
  anchor naming a part that is not in the view is refused.
- **Tests:** `each_occurrence_gets_its_own_item`. `an_anchor_resolves_through_the_placement`. `an_
  occurrence_index_out_of_range_is_refused`. `two_occurrences_at_one_place_are_refused`. `the_numbering_
  is_stable_across_a_placement_change` (the item follows the *component*, not the position).
- **Acceptance:** the numbering-stability test is the acceptance: moving an occurrence must not
  renumber the parts list.
- **Risk:** `ASM` is L3 and `cadcraft-drawing` is L2, so the two can never depend on each other in
  this direction. Fallback: `ScenePart` is the seam, stated as an interface request in
  `DRW-VIEW-001`; if ASM wants the dependency the other way, `cadcraft-drawing` moves to L4 and the
  coordinator decides.

### DRW-LEAD-014 — The epic's refusals and caps
- **State:** READY
- **Depends on:** DRW-LEAD-010
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn hostile_leaders_never_panic() -> bool;`
- **Behaviour:** Balloon, label, note and frame construction over hostile input: 200 mutations of a
  valid balloon JSON, an item number of `usize::MAX`, a radius of `1e300`, a leader of 10 000 points,
  a key of `-1`, a text with control characters and with a 4000-character run, a `gap` of `1e-300`,
  a note block with 5000 rows. All `Ok` or `Err`, no panic, no unbounded allocation (the row/point
  caps fire first).
- **Refusals:** `Limit` with the count for the point and row caps; `Refused` naming the field for the
  rest.
- **Tests:** `hostile_leaders_never_panics`. `a_ten_thousand_point_leader_is_refused`. `five_
  thousand_note_rows_are_refused`. `control_characters_in_a_note_are_refused`. `no_annotation_
  coordinate_is_nan`. `the_epic_is_deterministic` — two runs, identical entities.
- **Acceptance:** this task is the epic's gate.
- **Risk:** a leader with 10 000 points is a hostile input a user can also make by hand. Fallback:
  `MAX_LEADER_POINTS = 1024`, refused with the count, which the 2D `Leader` entity can hold.

---

# DRW-XRF — hidden-line removal: the faceted path first, the exact path after the kernel

This is the hardest problem in the domain and the one most likely to be shipped as a lie, so the epic
is split in two and the split is visible in the product:

- **`DRW-XRF-001..011` — the faceted path.** Tessellate the solid at a stated chord tolerance, project
  the triangles, run an own depth buffer, extract edges. It works today, because today's kernel
  produces **planar-faceted** solids, and it is *exactly right* for a part made of planes. It is
  approximate for a curved one, and the output records which path produced it
  (`HiddenLinesMode::Faceted`, `DRW-VIEW-006`).
- **`DRW-XRF-012..024` — the exact path.** Per-face front/back classification, analytic silhouette
  curves on cylinders and cones, trimmed-surface boundaries, surface-surface occlusion. Every task
  that needs a curved surface is `BLOCKED` on a named `KER` contract with the signature stated.

The id `DRW-XRF` is the one the master plan's naming uses for this epic; the usual abbreviation for
hidden-line removal is *HLR*. If the orchestrator prefers `DRW-HLR`, the rename is one edit per
heading plus the cross-references in this file — no contract changes.

### DRW-XRF-001 — The decision: two paths, one output type, and what each may claim
- **State:** READY
- **Depends on:** DRW-VIEW-006
- **Files:** `crates/drawing/src/xrf/mod.rs` module doc, `crates/drawing/src/xrf/claim.rs` (owner
  **DRW**)
- **Contract:**
  ```rust
  /// What a view's edges can honestly be claimed to be.
  #[derive(Clone, Debug, PartialEq)]
  pub struct Claim {
      pub mode: HiddenLinesMode,
      /// The largest deviation, in model millimetres, between the drawn edge and the exact one.
      pub max_deviation_mm: f64,
      /// Faces whose exact surface CADCraft cannot evaluate, counted.
      pub unevaluated_faces: usize,
      pub text: String,
  }
  pub fn claim_for(solid: &cadcraft_solid::Solid, chord_mm: f64, mode: HiddenLinesMode) -> Result<Claim, DrwError>;
  pub fn claim_text(c: &Claim) -> String;
  ```
- **Behaviour:** The module doc opens with the honest sentence, in the style `crates/solid/src/lib.rs`
  writes its own limits: *"a hidden-line pass on a faceted solid is exact for the solid the kernel
  holds and approximate for the solid a machinist will make; the deviation is bounded by
  `max_deviation_mm` and printed on the sheet."* `claim_for` computes that bound from the chord
  tolerance and the solid's own geometry: a chord of length `c` on a circle of radius `r` deviates by
  `r − sqrt(r² − (c/2)²)` ≤ `c²/(8r)`, which is the number the claim carries — derived, not guessed.
  A solid whose every face is planar gets `max_deviation_mm = 0.0` and `mode = Exact`
  (a planar-faced solid's hidden-line answer is exact, and claiming otherwise would be *understating*
  the product).
- **Refusals:** a solid with a face count over the cap is `Limit`; a chord tolerance that is not
  finite and positive is refused; a non-finite radius in the deviation computation (a face's radius
  below `tolerance_for`, treated as planar) is handled by returning 0.0 rather than by producing a
  NaN.
- **Tests:** `a_planar_solid_claims_exact_with_zero_deviation`. `a_faceted_cylinder_claims_the_chord_
  deviation`. `the_deviation_matches_the_closed_form` — `c²/(8r)` to 1e-12. `the_claim_text_names_
  the_mode_and_the_number`. `a_non_finite_chord_is_refused`. `a_face_cap_overrun_is_refused`.
- **Acceptance:** `claim_text` is printed by `VIEWBASE` and stored on the sheet's report; the
  planar-solid zero case is asserted, because "exact for a planar part" is the product's strongest
  true claim and must not be lost.
- **Risk:** a deviation number nobody reads is decoration. Fallback: it is in the **title block**
  (`DRW-SHE-004`'s `TitleField`) for a drawing produced with a non-zero claim, so it prints by
  default, and the sheet's report repeats it.

### DRW-XRF-002 — Tessellation at a stated chord tolerance, using the kernel's own mesher
- **State:** READY
- **Depends on:** DRW-XRF-001, KER-BOOL-010
- **Files:** `crates/drawing/src/xrf/tessellate.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The tessellation, and the chord tolerance it was built at.
  pub fn tessellate(solid: &cadcraft_solid::Solid, chord_mm: f64, crease: f64)
      -> Result<Tessellation, DrwError>;
  #[derive(Clone, Debug, PartialEq)]
  pub struct Tessellation { pub triangles: Vec<[Vec3; 3]>, pub chord_mm: f64,
                            pub normals: Vec<[Vec3; 3]>, pub max_deviation_mm: f64 }
  ```
- **Behaviour:** `cadcraft_solid::mesh_from_solid` (`crates/solid/src/lib.rs:91`) is the tessellator
  and is used unmodified; this task adds the **chord** control the mesher does not take (it takes a
  segment count), converting it to the mesher's own parameter and back so the recorded `chord_mm` is
  the number that was asked for rather than the one that resulted. Normals are per-triangle, taken
  from the face's plane (the kernel's faces are planar, so a triangle normal *is* the face normal —
  no averaging, no smoothing, and the test asserts exact equality).
- **Refusals:** `chord_mm` not finite and positive is refused; `chord_mm` below
  `tolerance_for(diagonal)` is refused naming both (a chord under the kernel's own tolerance is a
  request it cannot keep); a triangle count over `limits::MAX_TESS_TRIS` is `Limit`; the mesher's own
  `TooManyTriangles` error passes through unchanged.
- **Tests:** `the_normals_equal_the_face_normals_exactly`. `a_smaller_chord_gives_more_triangles` —
  monotone and bounded. `a_chord_below_the_tolerance_is_refused`. `the_triangle_cap_is_a_limit_error`.
  `the_max_deviation_is_the_closed_form_of_the_chord`. `the_kernel_error_passes_through`.
- **Acceptance:** the deviation test compares against the closed form, and the triangle-normal test
  compares against `Face::plane.normal` — not against an averaged normal.
- **Risk:** the kernel's mesher takes a **segment count**, so a chord tolerance is a conversion and
  the conversion is a rounding. Fallback: the task records the *actual* maximum chord it produced
  (`Tessellation.chord_mm` is what was achieved) and the claim is computed from that, so the sheet
  never claims a chord the mesh does not have.

### DRW-XRF-003 — Projecting the triangles into the view, with clipping to the view rectangle
- **State:** READY
- **Depends on:** DRW-XRF-002, DRW-VIEW-004
- **Files:** `crates/drawing/src/xrf/proj.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct Tri2 { pub a: [Vec2; 3], pub depth: [f64; 3], pub n: Vec2 /* the projected normal, unnormalised */ }
  pub fn project_triangles(t: &Tessellation, frame: &ViewFrame, rect: Bounds2)
      -> Result<Vec<Tri2>, DrwError>;
  ```
- **Behaviour:** Each vertex is projected by `ViewFrame::project` (`DRW-VIEW-002`) with its depth
  kept, and triangles wholly outside the view rectangle are dropped before any rasterisation (the
  rectangle test is the reason the raster cost is bounded by the sheet, not by the model). A triangle
  that straddles the rectangle is kept whole — clipping a triangle and rasterising the pieces is
  correct but slower, and the edge extraction reads the *original* triangles' edges anyway, so
  clipping would change nothing about the answer.
- **Refusals:** a triangle with a non-finite vertex is refused naming the triangle index; a
  rectangle that is not finite and positive is refused; a projected triangle whose vertices coincide
  to `1e-12` is dropped and counted (a degenerate triangle cannot produce an edge).
- **Tests:** `vertices_project_with_their_depth`. `triangles_outside_the_rectangle_are_dropped`. `a
  _straddling_triangle_is_kept_whole`. `a_degenerate_triangle_is_dropped_and_counted`. `the_
  projected_normal_is_the_face_normal_projected`. `a_nan_vertex_is_refused`.
- **Acceptance:** the projected triangle count is asserted against the tessellation count and the
  dropped counts, and the sum is checked — a leak here is a silent perf bug.
- **Risk:** dropping straddlers and keeping wholes means the raster sees geometry outside the sheet.
  Fallback: the raster's own bounds test (`DRW-XRF-004`) clips per pixel row, so cost stays bounded.

### DRW-XRF-004 — The depth buffer: an own scanline raster, sized from the sheet
- **State:** READY
- **Depends on:** DRW-XRF-003
- **Files:** `crates/drawing/src/xrf/zbuffer.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Raster {
      pub width: usize, pub height: usize,
      pub depth: Vec<f32>,      // +∞ = empty; larger = nearer (the `ViewFrame::depth` convention)
      pub covered: Vec<bool>,
  }
  impl Raster {
      /// `pixels_per_mm` is derived from the plot resolution (`PlotConfig.resolution`, 50..=2400 dpi)
      /// so the visibility answer is **at the resolution the sheet prints at**.
      pub fn new(size_mm: Vec2, pixels_per_mm: f64) -> Result<Raster, DrwError>;
      pub fn clear(&mut self);
      /// Scanline fill of one projected triangle; returns the pixels it wrote.
      pub fn fill_triangle(&mut self, t: &Tri2) -> Result<usize, DrwError>;
      pub fn depth_at(&self, p: Vec2) -> f64;
      pub fn is_covered(&self, p: Vec2) -> bool;
  }
  ```
- **Behaviour:** A CPU z-buffer, allocated once per view, at a resolution derived from the **output**
  resolution (300 dpi at 1:1 is 11.8 pixels per paper mm; the cap `MAX_RASTER_PIXELS = 16 777 216`
  keeps the allocation at 64 MB). `fill_triangle` is a scanline with an incremental x span and
  barycentric depth interpolation — no GPU, no `render` crate (invariant 7), no `unsafe`. This is the
  only place in `DRW` that rasterises, and it rasterises to answer *which edges are visible*, not to
  make a picture.
- **Refusals:** `pixels_per_mm` not finite and positive is refused; a raster over `MAX_RASTER_PIXELS`
  is `Limit` naming the count and the cap; a triangle outside the raster is skipped and counted (not
  an error — clipping is normal); a `Vec` allocation is `try_reserve`-based so an over-large raster
  is an `Err`, never an abort.
- **Tests:** `a_larger_depth_wins`. `the_resolution_follows_the_plot_resolution`. `the_raster_cap_is_
  a_limit_error`. `a_triangle_outside_the_raster_is_skipped_and_counted`. `depth_at_and_covered_
  agree_with_what_was_filled`. `an_empty_raster_covers_nothing`. `the_fill_is_deterministic`.
- **Acceptance:** a cube's raster has the front face's pixels covered and the far face's not; the
  test checks a handful of known pixels.
- **Risk:** `f32` depth at a 300 dpi raster over a 1 m part is ~3 µm per step, near the precision
  limit; a z-fighting speck on a coplanar pair is a real artifact. Fallback: depth is stored as `f64`
  (double the memory, 128 MB at the cap) or the raster's span is offset by one `f32` ulp per triangle
  index, the standard bias trick; the choice is measured in `DRW-XRF-008` and pinned by a test that
  renders a cube with two coplanar faces.

### DRW-XRF-005 — Edge classification: boundary, crease and silhouette from a triangle soup
- **State:** READY
- **Depends on:** DRW-XRF-004
- **Files:** `crates/drawing/src/xrf/edges.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct Candidate {
      pub a: Vec2, pub b: Vec2,
      pub kind: EdgeKind,
      pub n1: Vec2, pub n2: Vec2,   // the adjacent triangle normals, unnormalised
      pub depth_a: f64, pub depth_b: f64,
  }
  pub fn classify(triangles: &[Tri2], crease: f64) -> Result<Vec<Candidate>, DrwError>;
  /// An edge used by exactly one triangle: a silhouette or an open boundary.
  pub fn is_silhouette(c: &Candidate, tol: f64) -> bool;
  ```
- **Behaviour:** The three cases, in this order: an edge used by **one** triangle is a silhouette or a
  boundary (`DRW-VIEW-005`'s `Facing` tells which, from the face's own plane); an edge used by **two**
  triangles whose normals differ by more than `crease` (the dihedral angle from
  `acos(dot(n1, n2))`) is a **crease**; otherwise it is a smooth edge and is **not** drawn (a smooth
  edge on a shading model shows as a gradient, on a drawing as nothing — ISO says so, and the test
  says so). An edge used by **three or more** triangles is a non-manifold mesh and is reported.
- **Refusals:** a non-finite normal or vertex is refused naming the triangle; a `crease` outside
  `0..=180` degrees is refused; a non-manifold edge is reported with its triangle count, **not**
  drawn as a crease; an edge list over `limits::MAX_VIEW_EDGES` is `Limit`.
- **Tests:** `a_boundary_edge_is_a_silhouette`. `a_dihedral_above_the_threshold_is_a_crease`. `a_
  smooth_edge_is_not_drawn`. `a_non_manifold_edge_is_reported_not_drawn`. `a_cube_has_twelve_creases`
  and no silhouettes. `classification_is_deterministic`. `a_crease_outside_the_range_is_refused`.
- **Acceptance:** the cube count (12 creases, 0 silhouettes, 0 smooth) is pinned, and it is the first
  test anyone runs after a change.
- **Risk:** the classification is per-**triangles**, so a cylinder's facets each produce a crease at
  `crease`-small angles and nothing at `crease`-large ones; the threshold is therefore a *drawing
  decision*, not a geometric fact. Fallback: `crease` defaults to 30° and the value is printed in the
  claim (`DRW-XRF-001`) so a sheet says which threshold drew it.

### DRW-XRF-006 — Visible and hidden: the raster's verdict per candidate edge
- **State:** READY
- **Depends on:** DRW-XRF-005
- **Files:** `crates/drawing/src/xrf/edges.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Sample the raster along each candidate and decide visible / hidden.
  pub fn resolve_visibility(cands: &[Candidate], r: &Raster, step_px: f64)
      -> Result<Vec<(usize, EdgeKind)>, DrwError>;
  /// Why an edge was called hidden — the report needs it.
  pub fn hidden_reason(r: &Raster, c: &Candidate) -> &'static str;
  ```
- **Behaviour:** An edge is **visible** when it is nearer than the raster's depth along its whole
  sampled length, **hidden** when it is farther at every sample, and **partially visible** when the
  samples differ — in which case it is **split at the transition points** and the pieces get
  different kinds, which is what makes a real drawing (a hole's far edge showing through the hole is
  the classic case). Samples are spaced `step_px` apart (default 1 pixel) plus a sample at each
  vertex and each endpoint's depth.
- **Refusals:** a `step_px` that is not finite and positive, or below 1/8 pixel, is refused (a
  finer step costs more and changes nothing at plot resolution); an edge whose sampled positions leave
  the raster entirely is reported as `OffSheet` and skipped, **not** called hidden; an edge with a
  non-finite depth is refused naming it.
- **Tests:** `a_fully_visible_edge_is_visible`. `a_fully_occluded_edge_is_hidden`. `a_partly_
  occluded_edge_is_split`. `an_edge_off_the_sheet_is_not_called_hidden`. `a_finer_step_changes_
  nothing_at_plot_resolution`. `resolution_is_deterministic`. `the_hidden_reason_names_the_cause`.
- **Acceptance:** the split case is the acceptance: a cube with a through-hole shows the hole's far
  edge as visible **inside** the hole and hidden outside it, asserted with a hand-placed sample
  point.
- **Risk:** a 1-pixel sampling can miss a sliver of visible edge between two samples. Fallback:
  sampling at every pixel the edge crosses (not every `step_px`), which is exact for a straight edge
  against a raster and costs one depth read per pixel of edge length.

### DRW-XRF-007 — The faceted path's output, with its mode recorded
- **State:** READY
- **Depends on:** DRW-XRF-006
- **Files:** `crates/drawing/src/xrf/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn hidden_lines_faceted(part: &ScenePart, frame: &ViewFrame, opts: &FacetedOptions)
      -> Result<ViewGeometry, DrwError>;
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct FacetedOptions { pub chord_mm: f64, pub crease_degrees: f64,
                              pub pixels_per_mm: f64, pub hidden: bool }
  ```
- **Behaviour:** The whole faceted path in one call, returning the **same** `ViewGeometry`
  (`DRW-VIEW-006`) the exact path will return, with `mode: Faceted`, `EdgeKind` set per edge and
  `EdgeSource::Tessellated` for every edge. `hidden: false` skips the raster entirely and marks every
  edge visible, which is the fast preview path for an interactive view.
- **Refusals:** `hidden: true` with a part over the raster cap is `Limit` naming the count and the
  cap; every refusal from `DRW-XRF-002..006` passes through unchanged; `opts` with a non-finite field
  is refused naming it.
- **Tests:** `the_faceted_path_produces_a_ViewGeometry_with_mode_faceted`. `the_hidden_option_
  toggles_the_kinds`. `every_faceted_edge_says_tessellated`. `the_cube_case_is_the_expected_twelve
  _visible_and_no_hidden`. `a_part_over_the_raster_cap_is_refused`. `the_path_is_deterministic`.
- **Acceptance:** this is the function `DRW-VIEW-008` calls; the cube's exact expected output
  (12 visible edges, 12 `Crease`, 0 hidden) is pinned.
- **Risk:** two implementations of the same output invites drift. Fallback: the exact path
  (`DRW-XRF-012`) must return an output that passes the **same** tests with different numbers only
  where the geometry is curved — enforced by `DRW-XRF-013`.

### DRW-XRF-008 — Performance, caps and the progressive path
- **State:** READY
- **Depends on:** DRW-XRF-007
- **Files:** `crates/drawing/src/xrf/mod.rs`, `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn hidden_lines_progressive(part: &ScenePart, frame: &ViewFrame, opts: &FacetedOptions,
                                  budget: Budget) -> Result<Progress, DrwError>;
  #[derive(Clone, Debug, PartialEq)]
  pub struct Progress { pub done: bool, pub edges: Vec<ViewEdge>, pub triangles_done: usize,
                        pub triangles_total: usize }
  ```
- **Behaviour:** The triangle set is processed in order in chunks that fit `budget.max_triangles_per_
  step` (default 200 000), so a 10⁵-triangle part produces a usable view after the first chunk rather
  than after the whole thing. Resuming continues from the raster state. This is the same shape as
  `DAT-051`'s paged verification and it is what keeps a 3D view interactive.
- **Refusals:** `budget` with a zero or non-finite step is refused; a part over
  `limits::MAX_TESS_TRIS` is `Limit` (never chunked past the cap — chunking is for speed, not for
  hiding an over-budget input); a raster over the cap is `Limit`.
- **Tests:** `the_first_chunk_produces_edges_before_the_whole_part_is_done`. `resuming_continues_where
  _it_stopped`. `the_final_result_equals_the_one_shot_result`. `a_zero_budget_is_refused`. `a_part_
  over_the_cap_is_refused_not_chunked`. `a_ten_thousand_triangle_part_finishes_within_the_budget`.
- **Acceptance:** the "final result equals the one-shot result" test — a progressive path that
  produces a *different* drawing is worse than no progressive path.
- **Risk:** the wall-time claim needs a machine, which this file cannot produce. Fallback: the task
  states the measurement method (a `std::time::Instant` around the call, asserted as a *ratio*
  against a fixed triangle count) and records the result in `docs/geometry-parity.md`; PLT-PERF
  owns the machine-independent baseline file.

### DRW-XRF-009 — Determinism of the faceted path
- **State:** READY
- **Depends on:** DRW-XRF-007
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn faceted_hidden_lines_are_deterministic() -> bool;`
- **Behaviour:** Two runs over the same solid and frame produce byte-identical edge sequences,
  including order, kinds and depths — no `HashMap`, no float accumulation order dependence, and the
  triangle order is the mesh's order, which the kernel already makes deterministic
  (`KER.md` §0.8). The test also runs the path with a deliberately **shuffled** triangle input and
  asserts the output set is identical (order may differ, which is why the entity writer sorts) — that
  is the "no hidden order dependence" property.
- **Refusals:** n/a.
- **Tests:** `faceted_hidden_lines_are_deterministic`. `a_shuffled_input_gives_the_same_edge_set`.
  `the_edge_kinds_do_not_depend_on_order`. `no_edge_carries_a_nan`.
- **Acceptance:** the determinism test is the acceptance; a drawing that changes between two saves is
  a defect a user will report as "the drawing moved".
- **Risk:** n/a; this is the cheapest test in the epic and the most valuable.

### DRW-XRF-010 — A circle drawn as `n` segments, and the rule that decides `n`
- **State:** READY
- **Depends on:** DRW-XRF-002
- **Files:** `crates/drawing/src/xrf/tessellate.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// The segment count for a round edge of radius `r` at `chord_mm`, from the sagitta formula.
  pub fn segments_for(radius: f64, chord_mm: f64, max_segments: usize) -> Result<usize, DrwError>;
  pub fn facet_deviation(segments: usize, radius: f64) -> f64;   // r − r cos(π/n)
  ```
- **Behaviour:** `segments = ceil(π / acos(1 − chord/(2r)))`, clamped to
  `cadcraft_solid::limits::MAX_ROUND_SEGMENTS` (64). The deviation is reported in the claim
  (`DRW-XRF-001`) and **the clamp is honest**: a small radius whose chord would need 200 segments is
  drawn with 64 and the claim says its real deviation is larger than requested. A drawing that shows
  a 16-segment circle at a radius small enough to need 64 is a wrong circle; saying so is the fix.
- **Refusals:** a radius that is not finite and positive, or below `tolerance_for`, is refused
  (a round edge under tolerance is not resolvable); a chord that is not finite and positive is
  refused; `max_segments` of 0 is refused.
- **Tests:** `the_segment_count_follows_the_sagitta_formula`. `a_large_radius_needs_few_segments`. `a
  _small_radius_hits_the_clamp_and_the_claim_grows`. `the_deviation_matches_r_minus_r_cos`. `a_radius_
  below_the_tolerance_is_refused`.
- **Acceptance:** the clamp case's claim number is asserted to be **larger** than the requested
  chord — the test fails if the claim ever under-reports.
- **Risk:** 64 segments on a small bore is visibly a polygon. Fallback: the exact path
  (`DRW-XRF-013`) draws a true circle for a cylinder's silhouette; until then the claim is printed.

### DRW-XRF-011 — The viewport's hidden-line visual style, served by the engine
- **State:** BLOCKED — needs `REN-003`/`UI-VP-014` (the renderer and the viewport panel)
- **Depends on:** DRW-XRF-007, KER-PERF-003
- **Files:** `crates/engine/src/cmd/drawing/view.rs` (owner **ENG**); the panel is
  `crates/ui-egui/src/viewport3.rs` (owner **U**)
- **Contract:**
  ```rust
  /// The adapter the engine owns. `cadcraft-render` is L2 and may not depend on
  /// `cadcraft-drawing` (also L2), so the conversion lives in the one crate that may hold both.
  // in crates/engine/src/cmd/drawing/view.rs:
  pub fn hidden_line_segments(model: &cadcraft_model::Model, cam: &cadcraft_render::scene3::Camera)
      -> Result<Vec<cadcraft_render::scene3::Segment>, String>;
  ```
- **Behaviour:** The viewport's "hidden line" visual style needs the same computation a sheet does, and
  the layering rule puts the conversion in `ENG` (L4), which may depend on `drawing` (L2) and
  `render` (L2). The camera's direction and target become a `ViewFrame`; `hidden_lines_faceted` runs
  at the viewport's pixel resolution; the `ViewEdge`s become `scene3::Segment`s, whose type
  (`crates/render/src/scene3.rs:424`) is what the renderer already draws as wireframe. Hidden edges
  become `Segment`s with the hidden layer's colour/linetype. **This is why `cadcraft-drawing` is not
  L2-dependent on render** (invariant 7).
- **Refusals:** a camera with a non-finite position or direction is refused naming it; a model with
  no solid is refused with the feature's message; a part over the raster cap is refused with the cap.
- **Tests:** `the_viewport_hidden_line_mode_produces_segments`. `hidden_edges_use_the_hidden_colour`.
  `a_camera_with_a_non_finite_target_is_refused`. `the_viewport_and_the_sheet_agree` — the same
  frame through both paths gives the same edge count.
- **Acceptance:** `UI-VP-014`'s selector gains a working hidden-line mode with this adapter, and the
  "viewport and sheet agree" test is the acceptance.
- **Risk:** the viewport runs every frame and the sheet runs once, so a slow pass in a frame loop is
  an unusable app. Fallback: the engine caches by `(solid generation, frame rounded to 1e-4)`, which
  is `DRW-XRF-021`'s cache used from the other direction; the interactive path uses
  `hidden: false` (`DRW-XRF-007`) until the geometry settles.

---

# DRW-XRF-012..024 — the exact path

Every task from here needs surface or topology the kernel does not have yet, except where noted. The
`KER` signatures they need are listed in the table at the top of this file; each task repeats the one
it depends on so it can be handed to an agent on its own.

### DRW-XRF-012 — The exact visibility contract, and the front/back classification it rests on
- **State:** READY (the contract and the planar classification work today; the signature is written
  for the `BREP` it will take)
- **Depends on:** DRW-XRF-001, KER-SRF-049
- **Files:** `crates/drawing/src/xrf/exact.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn visible_edges(solid: &cadcraft_solid::Solid, frame: &ViewFrame, tol: f64)
      -> Result<Vec<VisibleEdge>, DrwError>;
  #[derive(Clone, Debug, PartialEq)]
  pub struct VisibleEdge {
      pub faces: [usize; 2],
      pub kind: EdgeKind,       // Visible | Hidden | Silhouette | Boundary | Crease
      pub a: Vec2, pub b: Vec2, // in view space
      pub curve: ExactCurve,    // the exact curve, when the face is curved
  }
  #[derive(Clone, Debug, PartialEq)]
  pub enum ExactCurve {
      Segment { a: Vec3, b: Vec3 },
      /// NEEDS KER-SRF-018/023: the analytic curve on an analytic surface.
      Analytic { kind: AnalyticKind, params: [f64; 2] },
  }
  ```
- **Behaviour:** The front/back test is `dot(face.plane.normal, frame.dir)`, signed against
  `dir_tol` (`DRW-VIEW-005`), and the classification of an **edge between two faces** follows from the
  two faces' facing: front/front → crease, front/back → **silhouette** (visible), back/back → hidden,
  and an edge with one face (a boundary) → boundary. For a **planar B-rep this is exact and complete**,
  and it is the whole of the exact path today — the reason a planar part's claim is `max_deviation_mm
  = 0`. The `ExactCurve::Analytic` arm is the shape the curved case will take.
- **Refusals:** an edge whose two faces' normals are both within `dir_tol` of perpendicular to the
  view (both edge-on) is `Boundary`, not a crease, and is reported; a non-finite normal is refused;
  `tol` that is not finite and positive is refused; an edge with `faces == [i, i]` (a face with
  itself) is refused naming the edge.
- **Tests:** `a_cube_has_twelve_visible_creases`. `a_cube_has_no_silhouettes`. `an_edge_between_a_
  front_and_a_back_face_is_a_silhouette`. `two_back_faces_give_a_hidden_edge`. `a_boundary_edge_is_
  a_boundary`. `an_edge_on_edge_is_flagged_boundary_and_reported`. `the_planar_result_matches_the_
  faceted_result` — this is `DRW-XRF-013` and it is cross-referenced there.
- **Acceptance:** on a cube (and on the sample drawing) the exact path's output equals the faceted
  path's edge for edge, which is `DRW-XRF-013`.
- **Risk:** the classification assumes two faces per edge, which the kernel's `Edge` already carries
  (`faces: [usize; 2]`, `crates/solid/src/solid.rs:26-31`). Fallback: an edge with three or more faces
  is reported and drawn as hidden, never guessed.

### DRW-XRF-013 — The faceted↔exact agreement test: the proof the faceted path is not a lie
- **State:** READY
- **Depends on:** DRW-XRF-012, DRW-XRF-007
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn faceted_agrees_with_exact() -> Result<Agreement, DrwError>;`
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Agreement { pub missing_from_faceted: usize, pub extra_in_faceted: usize,
                         pub misclassified: usize, pub max_position_delta_mm: f64 }
  ```
- **Behaviour:** Over a corpus of **planar-faced** solids (a cube, a washer, a stepped block, the
  sample drawing, and every solid in `crates/solid/tests/property.rs`'s generators), the two paths must
  produce the same edge set: same count, same kinds, positions within `tolerance_for(diagonal)`. The
  report is the acceptance artefact, and it is the *only* thing that makes "faceted is exact for a
  planar part" a measured claim rather than a sentence.
- **Refusals:** a disagreement is **not** auto-fixed: the test fails with the numbers and names the
  first differing edge; a solid over the caps is skipped with the reason and counted, never hidden.
- **Tests:** `faceted_agrees_with_exact` — `missing_from_faceted == 0 && extra_in_faceted == 0 &&
  misclassified == 0`. `the_agreement_is_exact_for_a_planar_part`. `a_curved_solid_is_not_expected_
  to_agree` — asserted only for the *count* being finite, and the test says why. `the_report_names_
  the_first_difference`.
- **Acceptance:** this task is the gate of the whole epic: **until it passes, the faceted path may not
  be described as exact anywhere**, and `DRW-XRF-001`'s zero-deviation claim is unproven.
- **Risk:** a disagreement is a bug in one of the two paths and the test cannot say which.
  Fallback: the report's per-edge delta localises it to the faceted path when the exact path's
  classification is provable from two face normals, which is a two-line check in the test's message.

### DRW-XRF-014 — The exact silhouette of a cylinder: the generator line, trimmed
- **State:** BLOCKED — needs `KER-SRF-018`, `KER-SRF-023`, `KER-SRF-033`
- **Depends on:** DRW-XRF-012, KER-SRF-018, KER-SRF-023, KER-SRF-033
- **Files:** `crates/drawing/src/xrf/exact.rs` (owner **DRW**)
- **Contract:**
  ```rust
  // NEEDS KER-SRF-033 (not defined here):
  // pub struct BFace { pub surface: Surface, pub outer: Vec<BCurve>, pub inner: Vec<BCurve> }
  // `BCurve` = the 3D curve plus its 2D parameters in the surface's domain.
  pub fn cylinder_silhouette(f: &BFace, frame: &ViewFrame, tol: f64)
      -> Result<ExactCurve, DrwError>;
  ```
- **Behaviour:** The silhouette of a cylinder seen from any direction other than along its axis is
  **two generator lines**, at parameters where the surface normal is perpendicular to the view
  direction: `dot(n(u, v), dir) = 0` solves to a constant `u`, and the two generators run along `v`
  trimmed to the face's own parametric region. So the exact silhouette is an analytic curve, not a
  sampled polyline — which is the entire point of the exact path and the reason it is worth its cost.
- **Refusals:** a view direction **exactly** along the axis (`|dot(axis, dir)| < 1e-12`) is the
  degenerate case: the silhouette is a full circle, not two generators, and is handled by
  `DRW-XRF-016`; a face whose surface is not a cylinder is refused naming the face; a `BFace` whose
  trimming curves do not lie on its surface (pcurve not on the face) is refused — the kernel's
  obligation, surfaced.
- **Tests (written now, they will not compile until `BFace` exists — that is the block):**
  `a_cylinder_in_a_tilted_view_has_two_generators`. `the_generators_run_along_the_axis`. `the_
  generators_are_trimmed_to_the_face`. `the_silhouette_is_not_a_polyline` — the curve's extent is a
  straight line in view space. `a_view_along_the_axis_is_the_degenerate_case`.
- **Acceptance:** the task is unblocked when `KER-SRF-033` exports `BFace`; its signature is
  re-checked against the kernel's actual type at that point, and **not before**.
- **Risk:** `BFace` is the kernel's biggest pending type and it may land with different field names.
  Fallback: none — a signature mismatch is a coordinator decision, and the fallback is to re-derive
  the silhouette from a sampled surface plus the tangency condition, which is *sampling*, and that is
  the other path.

### DRW-XRF-015 — Silhouettes on cones, spheres and tori
- **State:** BLOCKED — needs `KER-SRF-018`, `KER-SRF-023`
- **Depends on:** DRW-XRF-014, KER-SRF-018
- **Files:** `crates/drawing/src/xrf/exact.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn analytic_silhouette(f: &BFace, frame: &ViewFrame, tol: f64)
      -> Result<Vec<ExactCurve>, DrwError>;
  ```
- **Behaviour:** The same equation, different surface: on a **cone** the silhouette is a conic
  (hyperbola/ellipse/parabola depending on the view angle — the degenerate cone tangent case is a
  parallel line pair); on a **sphere** it is a great circle; on a **torus** it is one or two circles.
  Each is a closed-form curve, and each has a **degenerate** view direction that changes the answer,
  which is why the degenerate matrix is `DRW-XRF-016`.
- **Refusals:** a cone with a zero half-angle is refused; a torus with `R <= r` (a self-intersecting
  spindle torus, which has no well-defined silhouette) is refused naming both radii; a surface whose
  kind the caller does not handle is `Refused("no silhouette for {kind}")` naming it.
- **Tests:** `a_sphere_silhouette_is_a_great_circle`. `a_cone_silhouette_is_a_conic`. `a_torus_silhouette_
  is_one_or_two_circles`. `a_spindle_torus_is_refused`. `a_zero_half_angle_cone_is_refused`.
- **Acceptance:** the closed forms are asserted against their parametric equations at sample
  parameters — a numeric test that the curve satisfies the surface equation and the tangency
  condition.
- **Risk:** a cone's silhouette near its tangent direction is numerically ill-conditioned, and a
  drawing with a flickering cone silhouette is worse than a sampled one. Fallback: where the
  condition number exceeds a stated bound, the task returns a **sampled** curve with the claim's
  deviation raised accordingly (`DRW-XRF-001`) — an honest downgrade, never a wrong analytic curve.

### DRW-XRF-016 — The degenerate silhouette matrix
- **State:** BLOCKED — needs `KER-SRF-018`, `KER-SRF-023` (as `DRW-XRF-014`/`015`)
- **Depends on:** DRW-XRF-015
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn degenerate_silhouettes_are_named() -> Result<(), DrwError>;`
- **Behaviour:** Each case is a **named** outcome, in the style of `KER.md`'s degenerate matrix:
  view along a cylinder's axis → one full circle; view exactly tangent to a cone → a parallel pair or a
  parabola; a silhouette passing exactly through a face's corner → split at the trim boundary; a view
  direction within `dir_tol` of a face's normal → the face is edge-on and contributes no silhouette;
  two surfaces' silhouettes coincident within `tolerance_for` → merged with a report; a zero-radius
  cylinder → refused.
- **Refusals:** every unhandled degeneracy is `Refused` naming the case, never a picture.
- **Tests:** `a_view_along_the_axis_gives_a_circle`. `a_tangent_cone_is_a_parallel_pair_or_a_parabola`.
  `a_silhouette_through_a_corner_is_split`. `an_edge_on_face_contributes_nothing`. `coincident_
  silhouettes_are_merged_with_a_report`. `a_zero_radius_cylinder_is_refused`.
- **Acceptance:** every case in the matrix has a test whose name is the case; adding a case without a
  test fails review.
- **Risk:** the matrix is where an exact path usually breaks. Fallback: the table lives in the module
  doc as the matrix `KER-BOOL-006` has for booleans, so an agent adding a case updates one table.

### DRW-XRF-017 — A trimmed surface's boundary curves, projected exactly
- **State:** BLOCKED — needs `KER-SRF-033`
- **Depends on:** DRW-XRF-014, KER-SRF-033
- **Files:** `crates/drawing/src/xrf/exact.rs` (owner **DRW**)
- **Contract:**
  ```rust
  // NEEDS KER-SRF-033: `BCurve` carries the 3D curve and its 2D pcurve parameters.
  pub fn boundary_curves(f: &BFace, frame: &ViewFrame, tol: f64)
      -> Result<Vec<ExactCurve>, DrwError>;
  ```
- **Behaviour:** A face's outer and inner boundaries are the trimming curves; in the exact path they
  are evaluated as curves, not as faceted polylines. Their visibility is decided the same way as an
  edge's (front/back of the adjacent faces), so a hole's rim on a curved face is exact too.
- **Refusals:** a boundary curve whose pcurve leaves its surface's domain is refused (the kernel's
  obligation, surfaced); a self-intersecting trimming loop is reported.
- **Tests:** `a_hole_rim_is_an_exact_curve`. `the_rim_is_trimmed_to_the_face`. `a_pcurve_off_the_
  surface_is_refused`. `an_inner_loop_is_reversed_relative_to_the_outer`.
- **Acceptance:** the block is lifted when `BFace` lands.
- **Risk:** n/a beyond the kernel's.

### DRW-XRF-018 — Occlusion by another face: the surface-surface intersection test
- **State:** BLOCKED — needs `KER-SRF-035`, `KER-SRF-039`, `KER-SRF-042`
- **Depends on:** DRW-XRF-017, KER-SRF-035
- **Files:** `crates/drawing/src/xrf/exact.rs` (owner **DRW**)
- **Contract:**
  ```rust
  // NEEDS KER-SRF-035 (+039 refinement, +042 the pcurve on the second face):
  // pub fn intersect_surfaces(a: &Surface, b: &Surface, tol: f64) -> Result<Vec<Curve3>, SolidError>;
  pub fn occludes(a: &ExactCurve, b_face: &BFace, frame: &ViewFrame, tol: f64)
      -> Result<bool, DrwError>;
  ```
- **Behaviour:** Whether a face hides a curve is decided by intersecting the curve's host surface with
  every potentially-occluding face's surface and testing whether any intersection point lies in front
  of the curve within the occluding face's region. This is the reason the exact path needs
  `KER-SRF-004`: **hidden-line removal on a B-rep is surface-surface intersection**, and nothing else.
  The interval structure (`DRW-XRF-020`) is what keeps it from being O(edges × faces).
- **Refusals:** a face count or edge count over the caps is `Limit`; an intersection that does not
  converge within the kernel's iteration budget is refused naming the pair (never assumed to be empty
  — "empty" would draw a hidden line that should be visible).
- **Tests:** `a_curve_behind_a_face_is_occluded`. `a_curve_in_front_is_not`. `a_curve_through_a_hole_
  is_visible`. `a_non_converging_intersection_is_refused_not_assumed_empty`. `occlusion_is_
  deterministic`.
- **Acceptance:** the block is lifted when `intersect_surfaces` exists; until then the faceted path's
  raster answers the same questions, and the agreement test (`DRW-XRF-013`) covers the planar case.
- **Risk:** this is the single most expensive thing in the whole master plan. Fallback: keep it
  `BLOCKED` and honest, and never ship the exact path without it — a "hidden-line removal" that does
  not test occlusion is a wireframe with a filter.

### DRW-XRF-019 — Sampling an exact curve for display, with a declared deviation
- **State:** READY — works today for segments and, after `KER-SRF-033`, for curves
- **Depends on:** DRW-XRF-012
- **Files:** `crates/drawing/src/xrf/curve.rs` (owner **DRW**)
- **Contract:**
  ```rust
  /// Adaptive polyline sampling of an exact curve, to a **paper-millimetre** chord tolerance.
  pub fn sample(c: &ExactCurve, view_scale: ViewScale, plot_pixels_per_mm: f64)
      -> Result<(Vec<Vec2>, f64 /*achieved deviation in paper mm*/), DrwError>;
  /// The polyline an `ExactCurve` becomes on paper: segments produce none.
  pub fn deviation_mm(poly: &[Vec2], c: &ExactCurve) -> f64;
  ```
- **Behaviour:** The tolerance is in **paper millimetres at the output resolution** — one pixel of
  the plot — because that is the only scale at which "looks right" has a meaning. Adaptive: split a
  parameter interval, keep it when the chord's midpoint deviates by more than a tenth of a pixel,
  halve it otherwise, and stop at `MAX_CURVE_SAMPLES = 4096` per curve. `achieved` is measured, not
  assumed, and it is what the claim prints.
- **Refusals:** `plot_pixels_per_mm` not finite and positive is refused; a curve needing more than
  `MAX_CURVE_SAMPLES` is refused with the count (a curve that cannot be drawn at plot resolution is
  not drawn wrong); a `view_scale` not finite and positive is refused.
- **Tests:** `a_straight_curve_samples_to_two_points`. `a_circle_samples_to_the_plot_resolution`. `the_
  _achieved_deviation_is_measured`. `a_curve_over_the_sample_cap_is_refused`. `sampling_is_
  deterministic`. `a_tenth_of_a_pixel_is_the_tolerance`.
- **Acceptance:** the achieved-deviation test is the acceptance; a sampler that reports its error and
  is wrong is worse than one that reports it.
- **Risk:** sampling an analytic curve duplicates `KER-SRF`'s own evaluation. Fallback: the kernel's
  `Curve3` evaluates; this only chooses the parameter steps, which is a drawing decision, and the
  module doc says so.

### DRW-XRF-020 — The interval structure: AABBs, so occlusion is not O(edges × faces)
- **State:** READY (the structure works on the planar case today)
- **Depends on:** DRW-XRF-012, KER-PERF-003
- **Files:** `crates/drawing/src/xrf/index.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub struct Bounds { pub min: [f64; 3], pub max: [f64; 3] }
  pub struct Index {
      pub cells: usize, pub edges_per_cell: usize,
      pub edge_index: Vec<usize>,   // per cell, sorted
      pub face_index: Vec<usize>,
  }
  impl Index {
      pub fn build(solid: &cadcraft_solid::Solid, cells: usize) -> Result<Index, DrwError>;
      /// The faces whose AABB, inflated by `depth`, can hide `p`.
      pub fn occluders(&self, p: Vec3, dir: Vec3, depth: f64) -> Vec<usize>;
  }
  ```
- **Behaviour:** A uniform grid over the solid's AABB in model space, sized so the average cell holds
  the target number of faces, each face indexed by the cells its AABB touches. `occluders` walks the
  three cells along `dir` from `p` outward (the classic 3D-DDA) and returns the candidate faces only.
  The cell size is derived from `cadcraft_solid::limits::MAX_FACES` and the requested granularity, and
  the index is built once per solid version (`DRW-XRF-021`).
- **Refusals:** a solid whose AABB has a non-finite or non-positive extent is refused; `cells` of 0 or
  above 4096 is refused; a grid whose memory would exceed `MAX_INDEX_CELLS = 4_194_304` is refused
  naming the count.
- **Tests:** `occluders_contains_the_true_occluder`. `occluders_excludes_a_face_behind_the_point`.
  `a_solid_at_the_face_cap_builds_an_index`. `a_degenerate_aabb_is_refused`. `the_cell_count_follows_
  the_granularity`. `the_index_is_deterministic`.
- **Acceptance:** the "contains the true occluder" test is the acceptance; an index that misses an
  occluder draws a hidden line that should be visible.
- **Risk:** an AABB index is useless for a long thin part along a diagonal. Fallback: the grid is
  built on the solid's own principal extents (its AABB axes), and a part whose aspect ratio exceeds
  1000 falls back to a sorted-axis sweep (`DRW-XRF-008`'s budget) with the cost reported.

### DRW-XRF-021 — The cache: visibility per (solid generation, frame), invalidated on rebuild
- **State:** BLOCKED — needs `MOD`'s rebuild record to know the generation (owner **MOD**)
- **Depends on:** DRW-XRF-020, DRW-VIEW-016
- **Files:** `crates/drawing/src/xrf/index.rs` (owner **DRW**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub struct ViewKey { pub solid_generation: u64, pub dir: [i64; 3], pub up: [i64; 3], pub scale_q: i64 }
  pub fn key_for(frame: &ViewFrame, scale: ViewScale, generation: u64) -> ViewKey;  // quantised to 1e-4
  pub struct Cache { pub entries: BTreeMap<ViewKey, Arc<ViewGeometry>>, pub capacity: usize }
  impl Cache {
      pub fn get(&self, k: &ViewKey) -> Option<Arc<ViewGeometry>>;
      pub fn put(&mut self, k: ViewKey, g: ViewGeometry) -> Result<(), DrwError>;
      pub fn invalidate_generation(&mut self, generation: u64);
  }
  ```
- **Behaviour:** The direction and up vector are **quantised to 1e-4** before hashing, because an orbit
  camera produces a slightly different direction every frame and an unquantised key never hits. A
  `BTreeMap`, not a `HashMap`, so eviction is deterministic (the lowest generation first). Capacity is
  `MAX_CACHED_VIEWS = 8` by default; the entry is `Arc`d so a view can be drawn while it is evicted.
- **Refusals:** a capacity of 0 is refused; a `solid_generation` of 0 (no generation yet) is refused —
  it would collide with the "nothing cached" state; an entry above the cap is evicted, never refused.
- **Tests:** `the_same_frame_hits_the_cache`. `a_tiny_direction_change_still_hits` (quantisation).
  `a_rebuild_invalidates_the_generation`. `eviction_is_deterministic`. `the_capacity_is_respected`.
  `a_zero_generation_is_refused`.
- **Acceptance:** the quantisation test is the acceptance — without it the viewport recomputes every
  frame and nothing else in this epic matters for interactivity.
- **Risk:** the generation must come from the model's rebuild counter, which does not exist today.
  Fallback: the caller passes any monotonic counter it has (a parameter edit count, a document
  revision), and the sheet's `VIEWUPDATE` passes a bump of its own; the key is the caller's business.

### DRW-XRF-022 — Hidden lines take the drawing's linetype and layer
- **State:** READY
- **Depends on:** DRW-XRF-006, DRW-VIEW-017
- **Files:** `crates/drawing/src/xrf/mod.rs` (owner **DRW**)
- **Contract:**
  ```rust
  pub fn layer_for(kind: EdgeKind, scheme: &LayerScheme) -> &'static str;
  /// The linetype a hidden edge gets: the drawing's own, by name, from `crates/doc/src/library.rs`.
  pub fn linetype_for(kind: EdgeKind, d: &cadcraft_doc::Drawing, scheme: &LayerScheme)
      -> Result<String, DrwError>;
  ```
- **Behaviour:** The engine adds the `ViewEdge`s to `cadcraft-visible`/`cadcraft-hidden`/
  `cadcraft-outline` (`DRW-VIEW-017`) with the drawing's linetype table — a hidden line is a
  `HIDDEN` linetype from `crates/doc/src/library.rs`, which is **our own** dash pattern (industry-
  names are common; the values are ours, per `ATTRIBUTION.md`). A drawing with no `HIDDEN` linetype
  in its table gets one created, reported, because a hidden line drawn as a continuous line is a
  wrong drawing.
- **Refusals:** a linetype name that does not exist and cannot be created is refused naming it; a
  hidden-line pattern whose dash total is not positive is refused.
- **Tests:** `a_hidden_edge_gets_the_hidden_linetype`. `a_missing_linetype_is_created_and_reported`.
  `a_visible_edge_gets_by_layer`. `each_edge_kind_maps_to_one_layer`. `a_non_positive_dash_is_refused`.
- **Acceptance:** the linetype comes from the drawing's own library, so `cargo xtask assets` has no
  file to check and the asset policy holds.
- **Risk:** a plot style may override the linetype (`crates/render/src/plotstyle.rs`), so a printed
  sheet can differ from the model space. Fallback: the sheet's report says the plot style is applied,
  which `PageSetup.show_plot_styles` already carries.

### DRW-XRF-023 — Hostile input for the hidden-line path
- **State:** READY
- **Depends on:** DRW-XRF-012, DRW-XRF-020
- **Files:** `crates/drawing/src/tests.rs` (owner **DRW**)
- **Contract:** `pub fn hostile_hidden_lines_never_panic() -> bool;`
- **Behaviour:** The full path over hostile input: a solid with a `NaN` face normal, a solid with a
  face whose region has 200 000 vertices, a triangle soup with 2 000 000 triangles, a raster at
  4096 × 4096, a direction within `1e-15` of the surface normal (the degenerate tangent), a chord of
  `1e-300`, an edge of length 0, a non-manifold edge (four faces), a solid with an AABB of zero
  extent in two axes, and 200 mutations of a valid `ViewGeometry`. All `Ok` or `Err`; none panics,
  indexes out of bounds, allocates unboundedly (the caps fire first) or produces a NaN.
- **Refusals:** `Limit` with counts; `Refused` naming the number; `BadDirection`/`BadUp` from the
  frame.
- **Tests:** `hostile_hidden_lines_never_panic`. `a_zero_extent_aabb_is_refused`. `a_200000_vertex_
  face_is_refused_or_bounded`. `a_degenerate_tangent_view_is_named`. `a_zero_length_edge_is_refused`.
  `no_output_edge_holds_a_nan`. `every_refusal_names_a_number`.
- **Acceptance:** this task is the exact path's gate, alongside `DRW-XRF-013`.
- **Risk:** the exact path's cost grows with face count in a way the faceted path's does not, so a
  hostile face count is a **denial of service** as well as a crash risk. Fallback: `MAX_SOLID_FACES`
  and `MAX_VIEW_EDGES` are checked before any work, and `DRW-XRF-020`'s index bounds the rest.

### DRW-XRF-024 — Close-out: the claim table, written down and kept true
- **State:** READY
- **Depends on:** DRW-XRF-013, DRW-XRF-018, DRW-XRF-019
- **Files:** `crates/drawing/src/xrf/mod.rs` module doc, `docs/geometry-parity.md` (owner **PLT**
  for the doc), `docs/gaps.md` (owner **PLT**)
- **Contract:** The module doc contains one table, and `DRW-XRF-013` asserts it:
  ```rust
  /// | geometry | faceted path | exact path | max deviation |
  /// |---|---|---|---|
  /// | planar faces | exact (verified by `faceted_agrees_with_exact`) | exact | 0 |
  /// | faceted cylinder | approximate (`n` segments, `DRW-XRF-010`) | exact silhouette | `c²/8r` |
  /// | analytic curved face | refuses (`NeedsKernel`) until `KER-SRF-018` | exact | 0 |
  /// | NURBS face | refuses | refuses until `KER-SRF-002..023` | — |
  ```
  `pub fn claim_table_is_current() -> bool;`
- **Behaviour:** One table, in the module doc, in the house style of `crates/solid/src/lib.rs`'s
  "Limits" section. `claim_table_is_current` asserts every row's status against what the code
  actually does today — the faceted row's "exact" must be backed by `DRW-XRF-013` passing, and the
  NURBS row's "refuses" by a refusal test. So the table cannot drift into optimism.
- **Refusals:** a row claiming more than the tests prove is a failing test, not a doc edit.
- **Tests:** `claim_table_is_current` — parses the table out of the module doc and checks each row's
  word against the compiled behaviour (`Exact` only when the agreement test's claim is 0; `refuses`
  only when the refusal test refuses). `the_table_names_the_kernel_task_it_waits_for`.
- **Acceptance:** the table is in the module doc, `docs/geometry-parity.md` has a row with the same
  content, and the test prevents them from diverging.
- **Risk:** the table says "exact" for planar faces and a user reads "hidden-line removal works" —
  which, for CADCraft's current kernel (planar-faceted), is **true**. Fallback: the sheet prints its
  own claim (`DRW-XRF-001`), so the user sees the truth on the drawing, not only in the docs.

---

# DRW-EXP — exporting a sheet: the existing writers, the content they carry, the truth about them

There is **no new writer** in this epic. A sheet is a `Drawing` with a `Layout` full of entities, and
`crates/io` already writes DXF, DWG, PDF, SVG and PNG from exactly that. What this epic fixes: what
goes on the sheet before it is written, the paper geometry each writer must honour, the refusals a
malformed sheet produces, and the honest report an export carries.

### DRW-EXP-001 — The export seam: a sheet is a drawing, and `io` writes it
- **State:** READY
- **Depends on:** DRW-SHE-015, DAT-018
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `crates/io/src/lib.rs::{write,
  write_with, write_framed}` (owner **D**) — **read only**
- **Contract:**
  ```rust
  // in cadcraft-drawing:
  pub fn sheet_entities(d: &cadcraft_doc::Drawing, layout: &str) -> Result<Vec<cadcraft_doc::EntityKind>, DrwError>;
  /// The paper geometry a writer must honour, in paper millimetres.
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct SheetGeometry { pub width_mm: f64, pub height_mm: f64, pub margins_mm: [f64; 4] }
  pub fn sheet_geometry(sheet: &Sheet) -> Result<SheetGeometry, DrwError>;
  ```
  `CommandSpec` id `drawing.export {layout?, format?, path?, notes?: bool}` (owner **ENG**) →
  `{path, bytes, layouts, findings}`.
- **Behaviour:** `sheet_entities` returns the layout's paper-space entities in handle order, which is
  what a writer needs; `sheet_geometry` returns the page in millimetres. The engine calls
  `cadcraft_io::write_with(&doc, path, &SaveOptions)` — the **one** dispatch point (`crates/io/src/
  lib.rs:56-113`) — so `.dxf`, `.dwg`, `.pdf`, `.svg` and `.png` all go through the writers that
  already exist, with their own validation, and the sheet's content is what this epic is about.
- **Refusals:** a layout that does not exist is `NoSheet`; a format `io` does not write is
  `Err(IoError::Unsupported)` passed through unchanged, naming the extension; a sheet with findings
  (`DRW-SHE-014`) is exported with the findings in the reply, **not** refused, unless the caller
  passes `strict: true`.
- **Tests:** `the_export_uses_the_single_dispatch_point`. `an_unknown_layout_is_refused`. `an_
  unsupported_format_names_the_extension`. `the_geometry_matches_the_page_setup`. `sheet_entities_is_
  in_handle_order`.
- **Acceptance:** the bytes come from `io`'s writer — a test that swaps in a counting writer proves no
  second path exists.
- **Risk:** "PDF" means two different things: a plot (`PlotOutput::Pdf`, a raster of the plotted sheet)
  and a vector PDF. Fallback: the export names which it is, and the decision is recorded in the
  report: **a 3D sheet exports as a plot (raster) until a vector PDF writer exists**, because a
  raster PDF of a line drawing at 300 dpi is what CADCraft's plot path already produces for 2D
  layouts and is honest about its resolution.

### DRW-EXP-002 — The sheet's DXF: paper space, viewports or exploded views
- **State:** READY
- **Depends on:** DRW-EXP-001, DRW-XRF-007
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); the layout writer is
  `crates/io/src/dxf_write.rs` (owner **D**) — **read only**
- **Contract:** `drawing.export {layout, format: "dxf", mode?: "exploded" | "viewport"}` →
  `{path, bytes, viewports, entities, linetypes}`.
- **Behaviour:** Two modes, both honest and both standard: **exploded** writes each view's edges as
  ordinary paper-space entities (`LINE`/`LWPOLYLINE`/`ARC`) — the drawing is portable to any CAD and
  loses nothing but the association, which travels in the `#528` xdata channel where the 2D path
  already keeps unknown data; **viewport** writes each view as a `VIEWPORT` entity over a model-space
  projection, which keeps the association but requires the reader to support 3D viewports (which most
  do not). The default is exploded, and the choice is stated in the reply.
- **Refusals:** `mode: "viewport"` for a view whose scale is not `1:1`-able into the layout scale is
  refused naming both; a view with more edges than `io`'s layout writer accepts is refused with the
  count; an unknown mode is `Refused` naming the two.
- **Tests:** `exploded_mode_writes_the_edges_as_entities`. `viewport_mode_writes_a_viewport_entity`.
  `a_non_one_to_one_view_refuses_viewport_mode`. `the_default_is_exploded`. `the_written_dxf_reloads_
  with_the_same_geometry` — read back through `io`'s reader and compare the line count and extents.
- **Acceptance:** the reload test is the acceptance: an exported sheet re-read by CADCraft is the
  same drawing.
- **Risk:** an exploded sheet's DXF is large for a detailed view. Fallback: the reply reports the byte
  count and the entity count, and the caps are named.

### DRW-EXP-003 — Hidden lines in the export, and the linetype table row they need
- **State:** READY
- **Depends on:** DRW-XRF-022, DRW-EXP-002
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `crates/io`'s linetype writer
  (owner **D**) — **read only**
- **Contract:** `drawing.export {layout, format: "dxf", hidden?: bool}` → `{path, hidden_edges, linetype}`.
- **Behaviour:** With `hidden: true` the hidden edges are written with the drawing's `HIDDEN`
  linetype, and if the table has no such row the export **adds it** (`DRW-XRF-022`) so the file is
  self-describing. With `hidden: false` the hidden edges are dropped, which is a legitimate smaller
  file — and the reply says how many edges were dropped, because "our sheet had no hidden lines" and
  "we omitted them" are different facts.
- **Refusals:** a linetype that cannot be created or found is refused naming it; `hidden: true` with
  no hidden edges is `Ok` with `hidden_edges: 0`, not an error.
- **Tests:** `hidden_edges_survive_the_export_with_their_linetype`. `a_missing_linetype_row_is_added`.
  `omitting_hidden_edges_reports_the_count`. `the_written_dxf_declares_the_linetype`. `the_reloaded_
  hidden_edges_keep_their_kind`.
- **Acceptance:** the reload test compares visible and hidden counts separately.
- **Risk:** a reader that ignores the hidden linetype shows a solid drawing. Fallback: hidden edges
  also go on their own layer (`cadcraft-hidden`), so a reader that ignores linetypes still separates
  them by colour or layer.

### DRW-EXP-004 — Export a view to model space (the `PROJECT3D` direction)
- **State:** BACKLOG
- **Depends on:** DRW-VIEW-008
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**)
- **Contract:** `drawing.export.view {view, space?: "model", z: number, as?: "lines" | "polylines" | "faces"}`
  → `{handles, entities}`.
- **Behaviour:** The projected geometry written into the **2D drawing's model space** at a stated `z`
  elevation, as 3D polylines (`Polyline3d`) or as 2D lines — the 2D `PROJECT3D` behaviour, so a user
  can measure, section and continue to draft on a projection in the drawing they already work in.
  `as: "faces"` writes the projected regions as closed hatches (a filled projection), which is what
  a drafter uses for a check drawing.
- **Refusals:** a `z` that is not finite is refused; a view with no geometry is refused naming it; a
  `z` that would make the projection coincide with existing geometry within `tolerance_for` is
  refused with both numbers.
- **Tests:** `the_projection_lands_in_model_space_at_the_stated_z`. `the_lines_reload_as_entities`.
  `a_non_finite_z_is_refused`. `a_z_that_coincides_with_geometry_is_refused`. `faces_writes_closed_
  hatches`.
- **Acceptance:** the reloaded entity count and extents match.
- **Risk:** the association to the 3D model is not carried into the 2D drawing's own xdata here.
  Fallback: the 3D target goes in the `#528` foreign channel, and `VIEWIMPORT` reads it back.

### DRW-EXP-005 — PDF: paper size, orientation and the plot area, through the existing plotter
- **State:** READY
- **Depends on:** DRW-SHE-002, DRW-SHE-015
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `crates/doc/src/plotter.rs`
  and `PlotOutput::Pdf` (owner **C2**) — **read only**
- **Contract:** `drawing.export {layout, format: "pdf", resolution?: number, transparency?: bool}`
  → `{path, bytes, width_mm, height_mm, dpi}`.
- **Behaviour:** The existing plot pipeline (`run_plot` → `PlotOutput::Pdf`) with the layout's page
  setup. The 2D plot honours `plot_area` (`"layout" | "extents" | "display" | "window" | "limits"`,
  `crates/doc/src/tables.rs:740`) and the device paper's margins; for a 3D sheet the plot area is
  always `layout` (the sheet is the paper), and a `window` is refused with a reason.
- **Refusals:** a device paper not in the plotter's list is refused naming it; a `resolution` outside
  the plotter's own `50..=2400` clamp is clamped **and reported** (the clamp is the plotter's rule,
  `crates/doc/src/plotter.rs:157`); transparency follows `PageSetup.plot_transparency`.
- **Tests:** `the_pdf_media_box_is_the_paper_size`. `a_landscape_sheet_exports_landscape`. `an_
  unsupported_paper_is_refused_by_name`. `a_resolution_outside_the_range_is_clamped_and_reported`. `a_
  window_plot_area_is_refused_for_a_sheet`.
- **Acceptance:** the media box test reads the written PDF's `/MediaBox` — the file's own statement of
  its size.
- **Risk:** a raster PDF's line quality is resolution-bound and a 0.13 mm line at 300 dpi is one pixel.
  Fallback: the reply reports the DPI and the plot style, and the sheet's report says the export was
  a plot, not a vector PDF (`DRW-EXP-001`'s decision).

### DRW-EXP-006 — SVG export
- **State:** READY
- **Depends on:** DRW-EXP-001
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `io`'s SVG writer (owner
  **D**) — **read only**
- **Contract:** `drawing.export {layout, format: "svg"}` → `{path, bytes, width_mm, height_mm, entities}`.
- **Behaviour:** The existing writer, fed the layout's paper space. SVG is vector, so a 3D sheet
  exports better to SVG than to PDF today; the module doc of the writer says what it does not support
  and this task does not change it.
- **Refusals:** `io`'s existing refusals pass through unchanged; a sheet with findings exports with
  them in the reply.
- **Tests:** `the_svg_matches_the_paper_size`. `the_svg_round_trips_through_the_reader`. `hidden_edges_
  carry_their_linetype_or_colour`. `an_export_of_an_empty_sheet_is_a_valid_svg`.
- **Acceptance:** the reader round trip (`io`'s SVG reader exists — `quick-xml` is a dependency,
  `Cargo.toml:48`).
- **Risk:** **`.svg` and `.png` and `.pdf` are all in `xtask/src/assets.rs::ASSET_EXT`**
  (`assets.rs:10-13`), so *committing* an exported sheet as a test fixture raises the attribution
  question. Fallback: `DRW-EXP-016` and `PLT-ASSET-008` decide that rule in writing; until then no
  export fixture is committed.

### DRW-EXP-007 — PNG export at a stated resolution
- **State:** READY
- **Depends on:** DRW-EXP-005
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `io`'s `png_framed`
  (`crates/io/src/lib.rs:168`) — **read only**
- **Contract:** `drawing.export {layout, format: "png", dpi?: number}` →
  `{path, bytes, width_px, height_px, dpi}`.
- **Behaviour:** The existing framed PNG path with the plot resolution. The pixel dimensions come from
  the paper size and the DPI, and the reply carries both so a caller can check the image is what it
  asked for without opening it.
- **Refusals:** a DPI or a pixel count over the PNG writer's own cap (`scene3::MAX_IMAGE_PIXELS`,
  `crates/render/src/scene3.rs:102`) is refused naming the count and the cap; a DPI outside
  `50..=2400` is clamped and reported.
- **Tests:** `the_png_size_follows_the_paper_and_the_dpi`. `a_dpi_over_the_cap_is_refused`. `a_clamped_
  dpi_is_reported`. `the_png_is_not_blank` — the histogram has more than one value, which catches an
  empty sheet exported as a white rectangle.
- **Acceptance:** the pixel-dimension test, computed from the paper size in millimetres.
- **Risk:** same asset question as `DRW-EXP-006`. Fallback: as above.

### DRW-EXP-008 — A whole sheet set, and an index
- **State:** BACKLOG
- **Depends on:** DRW-EXP-001, DRW-SHE-012
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**)
- **Contract:** `drawing.export.set {dir, format, layouts?}` →
  `{files: [{layout, path, bytes}], index: String, failed: Vec<String>}`.
- **Behaviour:** One file per layout into a directory, plus an index file (`index.json` or an
  `index.dxf`, chosen by the format) listing the sheets in tab order with their numbers, titles and
  revisions (`DRW-SHE-012`'s `SheetIndex`). A layout that fails is **listed in `failed`** and the rest
  still export — a batch that stops at the first bad sheet is a batch that never finishes.
- **Refusals:** a `dir` that is not an existing directory is refused naming it; a layout whose export
  fails is reported, not fatal; a file name collision in the directory (two layouts sanitizing to one
  name) is refused naming both, never silently overwritten.
- **Tests:** `every_layout_exports_one_file`. `the_index_lists_the_sheets_in_tab_order`. `one_failing_
  layout_is_reported_and_the_rest_export`. `a_name_collision_is_refused`. `an_existing_file_is_
  overwritten_only_with_confirm` (the writer's atomic-save rule, `DAT-057`).
- **Acceptance:** every file in the reply exists with the stated byte count.
- **Risk:** writing into a user's directory is destructive. Fallback: the command refuses a
  directory that is not empty unless `overwrite: true`, and it writes through `io`'s atomic writer.

### DRW-EXP-009 — Headless batch export from the CLI
- **State:** BLOCKED — needs the CLI's argument surface (`apps/cadcraft-cli`, owner **C**)
- **Depends on:** DRW-EXP-008, DRW-EXP-004
- **Files:** `apps/cadcraft-cli/src/**` (owner **C**) — **co-sequenced**; the body is the engine's
  `drawing.export` command
- **Contract:** `cadcraft-cli draw export --layout Layout1 --format dxf --out sheet.dxf`, and
  `cadcraft-cli draw render --script '…' --export out.png` following the existing
  `cadcraft-cli run --sample --script … --export` shape (`AGENTS.md`).
- **Behaviour:** The CLI reaches the **same command** as the UI and the control channel, with no
  dialog (the rule in `AGENTS.md`: "Programmatic calls never open dialogs"). That is what makes a CI
  job able to export a sheet.
- **Refusals:** a missing `--layout` with several layouts is refused listing them; an unknown flag is
  refused naming it; a non-zero exit with the error on stderr.
- **Tests:** `the_cli_exports_a_sheet_headlessly`. `the_cli_and_the_command_agree_byte_for_byte`. `a_
  missing_layout_lists_the_known_ones`. `the_exit_code_is_non_zero_on_failure`.
- **Acceptance:** a CI job can produce a sheet's DXF with one command, which is what `DRW-EXP-016`'s
  fixture regeneration needs.
- **Risk:** `apps/cadcraft-cli` is another owner's file. Fallback: `cadcraft-cli run --script
  'DRAWINGEXPORT …'` already reaches any command through the existing script path, so no CLI change is
  strictly needed — the flag is sugar.

### DRW-EXP-010 — Export determinism: two exports are byte-identical
- **State:** READY
- **Depends on:** DRW-EXP-001
- **Files:** `crates/engine/tests/drawing_export.rs` (owner **ENG** — co-sequenced; a `DRW` unit
  version lives in `crates/drawing/src/tests.rs`)
- **Contract:** `pub fn two_exports_are_identical() -> bool;`
- **Behaviour:** The same sheet exported twice produces identical bytes, for every format. Sources of
  non-determinism to close, each with a test: entity order (handle order — asserted), a timestamp in
  a header (`provenance: false` by default, as `DAT-012` decides), a `HashMap` iteration in a writer,
  and the drawing's `handseed` (which must not advance during an export).
- **Refusals:** a format that **cannot** be byte-identical (a container format that stamps entries —
  `.3mf`'s zip timestamps, `DAT-027`) is listed in the reply as `reproducible: false` with the reason,
  not silently claimed.
- **Tests:** `two_exports_are_identical` for DXF, SVG, PNG and PDF. `an_export_does_not_advance_the_
  handseed`. `an_export_does_not_mutate_the_drawing`. `provenance_false_leaves_no_timestamp`.
- **Acceptance:** all four format tests pass; a non-reproducible format is reported, not claimed.
- **Risk:** a PDF writer that embeds a creation date breaks this. Fallback: `provenance: false`
  writes the fixed date the 2D plot path already uses, and the test proves it.

### DRW-EXP-011 — Units and scale in the exported file
- **State:** BLOCKY READY — depends DAT-043
- **Depends on:** DRW-EXP-002, DAT-043
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**); `$INSUNITS` is `io`'s
  (owner **D**) — **read only**
- **Contract:**
  ```rust
  /// The unit the exported file declares, and the conversion it applied.
  pub fn export_unit(sheet: &Sheet, doc: &Drawing) -> Result<(cadcraft_geom::UnitSystem, f64), DrwError>;
  ```
- **Behaviour:** The exported sheet's coordinates are in the **drawing's** units, so `$INSUNITS` is
  set to the sheet's unit and no conversion happens inside the export at all — the view's paper
  coordinates are already drawing units. `DAT-043`'s `apply_unit`/`doc_unit` are the 2D side of the
  same rule and are reused (read-only).
- **Refusals:** a drawing with no `$INSUNITS` is exported as millimetres **and the reply says so**
  (`DAT-043`'s rule); a unit CADCraft does not model is read as millimetres with a note in
  `d.read_notes`; a non-finite conversion factor is refused.
- **Tests:** `the_exported_file_declares_the_sheets_unit`. `no_conversion_happens_in_the_export`. `a_
  drawing_without_insunits_exports_as_millimetres_and_says_so`. `an_inch_sheet_exports_inches`. `the_
  2d_default_is_unchanged`.
- **Acceptance:** the 58 2D io tests still pass unchanged, and the exported header says the right unit.
- **Risk:** the layout's `PageSetup.scale` and the view's scale could be applied twice by a writer.
  Fallback: `export_unit` returns both the unit and the factor it applied, and the test asserts the
  factor is 1.0 for an unscaled export.

### DRW-EXP-012 — Export the parts list and the notes as data
- **State:** BLOCKY READY — depends DRW-SHE-010
- **Depends on:** DRW-SHE-010, DRW-EXP-008
- **Files:** `crates/drawing/src/sheet.rs` (owner **DRW**); the CSV writer is `io`'s table export
  (owner **D**) — **read only**
- **Contract:** `drawing.export.bom {layout, path, format?: "csv" | "json"}` →
  `{path, rows, bytes, columns}`.
- **Behaviour:** The parts list and the notes as **data**, through the existing table export (CSV) and
  `serde_json` (JSON). The JSON carries the item number, the part, the description, the quantity, the
  material, the mass and the drawing number — the machine-readable version of the sheet's table, which
  is what a purchasing system wants.
- **Refusals:** a layout with no parts list is refused naming it; a row with a non-finite mass is
  exported as `null` (`DRW-SHE-010`'s rule), never as 0.0; a comma in a field is quoted by the
  existing CSV writer, and the test proves a round trip.
- **Tests:** `the_csv_matches_the_table`. `the_json_carries_every_column`. `a_missing_mass_is_null_not_
  zero`. `a_comma_in_a_field_survives_the_round_trip`. `an_empty_parts_list_is_refused`.
- **Acceptance:** the CSV round trip through the existing table export is byte-stable.
- **Risk:** a BOM exported with the wrong unit for mass is a purchasing error. Fallback: the JSON
  carries the unit keys (`mass_kg`, `qty`), so the unit is in the field name, not in a footnote.

### DRW-EXP-013 — Refuse to write a sheet whose content is off the sheet
- **State:** READY
- **Depends on:** DRW-SHE-014, DRW-EXP-001
- **Files:** `crates/engine/src/cmd/drawing/export.rs` (owner **ENG**)
- **Contract:** `drawing.export {layout, strict?: bool}` → `{path, bytes, findings, exported}` with
  `strict: true` refusing on any `Finding`.
- **Behaviour:** By default the sheet exports with its findings in the reply (`DRW-SHE-014`), because
  refusing a user's print because a dimension overhangs the border is hostile. With `strict: true` — a
  CI job, a release sheet — it is refused, and the refusal lists the findings, which is what a
  publish workflow wants.
- **Refusals:** `strict: true` with any finding is refused with the findings as the error's payload; a
  finding list over 100 is truncated with a count.
- **Tests:** `a_default_export_writes_the_findings_in_the_reply`. `strict_refuses_on_a_finding`. `a_
  clean_sheet_exports_under_strict`. `the_refusal_lists_the_findings`.
- **Acceptance:** both modes are tested and the reply's shape is pinned.
- **Risk:** a user needs "print anyway" to be easy. Fallback: it is the default; `strict` is the
  opt-in.

### DRW-EXP-014 — Hostile layouts on export
- **State:** READY
- **Depends on:** DRW-EXP-013
- **Files:** `crates/engine/tests/drawing_export.rs` (owner **ENG** — co-sequenced)
- **Contract:** `pub fn hostile_sheets_never_panic() -> bool;`
- **Behaviour:** Export over hostile sheets: a negative paper size, margins larger than the sheet, a
  view at `at` of `1e300`, a scale of `1e-300`, 500 000 entities on one layout, a title block with a
  10 000-character field, a page setup of `NaN` degrees of rotation (there is none — `landscape` is a
  bool), and a layout whose name contains path separators and `..`. Every case returns `Ok` with
  findings or a named `Err`; none panics, writes outside the target directory, or allocates from an
  unchecked number.
- **Refusals:** `Refused` naming the field; `Limit` for the entity count; a path that escapes the
  output directory is refused (`DAT-027`'s texture-path rule, applied to the file name).
- **Tests:** `hostile_sheets_never_panic`. `a_negative_paper_is_refused`. `margins_larger_than_the_
  sheet_are_refused`. `a_view_at_1e300_is_refused`. `a_layout_name_with_path_separators_is_refused`.
  `a_five_hundred_thousand_entity_sheet_is_refused_or_bounded`. `no_export_writes_outside_the_
  target_directory`.
- **Acceptance:** this task is the epic's gate.
- **Risk:** a layout name is user data and reaches the file system. Fallback: the name is sanitised
  and the sanitised form is in the reply, so the user can see what the file is called.

### DRW-EXP-015 — What each export format does not support, written down
- **State:** READY
- **Depends on:** DRW-EXP-006, DRW-EXP-007
- **Files:** `crates/drawing/src/export.rs` module doc (owner **DRW**), linked from
  `docs/file-format-parity.md` (owner **PLT**)
- **Contract:**
  ```rust
  /// | format | carries the association | vector | raster | linetypes | notes |
  /// |---|---|---|---|---|---|
  /// | dxf | via xdata (#528) | yes | no | yes | yes |
  /// | dwg | via acadrust, lossy | yes | no | yes | yes |
  /// | pdf | no | no (plot) | yes | as plotted | no |
  /// | svg | no | yes | no | via dasharray | no |
  /// | png | no | no | yes | as drawn | no |
  ```
  `pub fn every_loss_is_documented() -> bool;`
- **Behaviour:** One table in the module doc, in the house style of `crates/doc/src/plotter.rs` and
  `DAT-020`'s "every writer states what it does not support". `every_loss_is_documented` collects the
  literals pushed into the export's `lost`/findings and asserts each appears in the table — the same
  anti-drift test `DAT-020` uses.
- **Refusals:** n/a (documentation), but the test is not: an undocumented loss fails the build.
- **Tests:** `every_loss_is_documented`. `every_format_row_names_its_limits`. `the_table_is_linked_
  from_the_parity_doc`.
- **Acceptance:** the test passes; the parity doc links the module doc.
- **Risk:** a writer's real limits live in `io` and could change without this table changing. Fallback:
  the test greps the strings this crate pushes, and the parity doc row carries the same text; a
  change in `io` shows up as a finding the user reads at export time.

### DRW-EXP-016 — The external-reader check for an exported sheet
- **State:** READY
- **Depends on:** DRW-EXP-002, DAT-014
- **Files:** `crates/engine/tests/data/sheet-reader-report.md` (owner **ENG**); the harness shape is
  `DAT-014`'s `ProbeReport`
- **Contract:** as `DAT-014`: `pub fn probe(bytes: &[u8], reader: &dyn Fn(&[u8]) -> Result<ProbeReport, String>) -> Result<(), String>;`
- **Behaviour:** The same honesty rule as DAT: a sheet export is finished only when **an independent
  implementation has opened it and agreed on the geometry**. The harness writes the fixture, shells
  out to a reader named by an environment variable, and compares accepted, entity count, extents and
  line count. The checked-in report names the reader, its version, the exact command, the fixture list
  and a verdict per fixture. **A report that was not produced by a run is not written.**
- **Refusals:** no reader configured → the test is `#[ignore]`d and says so; a reader that exits
  non-zero fails with its stderr; a line-count or extents disagreement fails with both numbers.
- **Tests:** `the_probe_requires_a_reader`. `a_line_count_mismatch_fails_with_both_numbers`. `an_
  extents_mismatch_fails`. `the_report_records_the_reader_version`.
- **Acceptance:** **the report exists and names at least one real independent reader, its version and
  a pass count equal to the fixture count.** Until then this task is `CODE WRITTEN` and
  `docs/file-format-parity.md`'s sheet row stays blank.
- **Risk:** the dev machine may have no second CAD. Fallback: as `DAT-014` — the harness is written
  and ignored; the first agent with a reader runs it. Do not fabricate a report.
- **Asset note:** every fixture this harness writes is a `.dxf`/`.png`, both in
  `xtask/src/assets.rs::ASSET_EXT` — so **committing** one needs an `ATTRIBUTION.md` row unless
  `PLT-ASSET-008` decides otherwise. This task does not commit a fixture until that decision is made.

---

## Task count

| Epic | Tasks | IDs | Entry point |
|---|---:|---|---|
| `DRW-VIEW` | 18 | `DRW-VIEW-001..018` | the frame, the projection, the neutral output, base/projected/detail/auxiliary views |
| `DRW-SHE` | 16 | `DRW-SHE-001..016` | a sheet is a layout: page setup, border, title block, notes, sections, parts list |
| `DRW-DIM` | 22 | `DRW-DIM-001..022` | 3D association, measurement in model space, auto-dimensioning, placement |
| `DRW-GDT` | 18 | `DRW-GDT-001..018` | feature recognition, frames, datums, materials, DRF, and three `BLOCKED` refusals |
| `DRW-LEAD` | 14 | `DRW-LEAD-001..014` | leaders, balloons, view and section labels, keyed notes, collision resolution |
| `DRW-XRF` | 24 | `DRW-XRF-001..024` | hidden-line removal: faceted path 001–011, exact path 012–024 |
| `DRW-EXP` | 16 | `DRW-EXP-001..016` | the sheet through the existing writers, determinism, refusals, the reader check |
| **total** | **128** | | |

States at the time of writing: 97 `READY`, 8 `BACKLOG`, 19 `BLOCKED`, 4 `BLOCKY READY` (a task
whose contract is written and whose single external prerequisite is named — the `DAT-045`/`DAT-043`
units work). Every `BLOCKED` task names its prerequisite in **Depends on** — and most name a `KER`
contract with the exact signature required, in the table at the top of this file. Nothing here has
been compiled or executed.