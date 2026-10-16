# REN — rendering and the 3D viewport

Task index for the `REN` domain (6 % of the programme). Owned files: `crates/render/src/**`, and (for
the GPU and the panels) `crates/ui-egui/src/{viewport3,viewport_state,gpu}.rs`, `crates/ui-egui/src/panels3/**`.

**Owner letters** are the wave-1 agents of `docs/master-plan.md` §6 — **U** ui/viewport (owner of
`crates/ui-egui/src/viewport3.rs` and `crates/render/src/scene3.rs` in wave 1), **Q** quality,
**D** interchange/units, **C** orchestrator — plus **REN** (`crates/render/src/**`, excluding
`scene3.rs` in wave 1), **A** kernel, and **PLT** (`docs/**`, `xtask/**`).

**Status:** specification only. Nothing here has been executed. No task in this file has been run.

---

## 0. Invariants of this domain

1. **A budget is a number and a measurement, never an adjective.** Every task that can cost frame
   time carries a budget in **milliseconds** and a **triangle count**, and names the harness that
   measures it. The words "fast", "cheap", "acceptable", "smooth" are not accepted anywhere in this
   file. **No measured baseline exists today** — the agent that wrote this file could not execute
   anything (`docs/agents/SYSTEM_PROMPT.md` §0), so every budget below is a *requirement with a
   named measurement*, and `REN-PERF-001..008` is the work that turns them into numbers.
2. **`scene3` is a validation renderer, not a viewport.** Its own module doc (`scene3.rs:1-33`) says
   it does not do shadows, PBR, transparency blending, culling acceleration, quality
   anti-aliasing, or performance. That sentence is the reason this file exists and it is repeated
   verbatim in `REN-VIS-003` and `REN-GPU-001`.
3. **One projection.** The GPU path and the CPU path must use **the same** `scene3::Camera`
   matrices. Two projections is two answers to "where is this point", and they will disagree.
   `REN-GPU-004` enforces it with a test.
4. **The renderer is not allowed to name a GPU.** `xtask/src/layers.rs:75` puts `wgpu`, `egui`,
`eframe` and `egui-wgpu` in `UI_CRATES` and `UI_MIN_LAYER = 5`, so only an **L5** crate may depend on
   them. `cadcraft-render` is **L2** (`layers.rs:44`). Therefore `crates/render` produces **bytes and
   mathematics** and `crates/ui-egui` uploads and draws them. Any task that wants wgpu inside
   `cadcraft-render` is BLOCKED with that citation.
5. **A picture is never silently wrong.** Every refusal is a named error carrying the number
   (`SceneError` today; `RenError` from `REN-GPU-023`). A renderer that draws *something* instead of
   failing is worse than one that refuses, because a broken projection looks like geometry.
6. **Determinism is a property, not a hope.** `scene3.rs:73-80` commits to byte-identical renders in
   one process and a test asserts it. Any new path (GPU, section, overlay) either keeps that property
   for its own outputs or says explicitly that it cannot, with the reason.

### What exists today, quoted from the files

**`crates/render/src/scene3.rs`** — a CPU rasteriser, 17 tests, declared (`render/src/lib.rs:27`):

| Fact | Value | Where |
|---|---|---|
| Image cap | `MAX_IMAGE_PIXELS = 4_194_304` (2048 × 2048) | `scene3.rs:102` |
| Supersampled raster cap | `MAX_RASTER_PIXELS = 4_194_304` (so supersample² = 1 only at full size) | `scene3.rs:105` |
| Triangle cap per scene | `MAX_TRIANGLES = 1_000_000` (its **own**; the kernel's is 2 000 000) | `scene3.rs:108` |
| Supersample cap | `MAX_SUPERSAMPLE = 3`, clamped not refused | `scene3.rs:111`, `factor()` `:627` |
| Grid lines per workplane axis | `MAX_GRID_LINES = 512` | `scene3.rs:114` |
| Light model | `rgb = base·(ambient + diffuse·(1−metallic)·max(dot(n,l),0))`, flat, `DEFAULT_AMBIENT 0.15`, `DEFAULT_DIFFUSE 0.85` | `scene3.rs:57-60`, `:117`, `:120`, `Material::shade :281` |
| `roughness` | **carried and ignored** by the renderer | `scene3.rs:242-256` |
| Transparency | alpha written, **never blended**, nothing sorted | `scene3.rs:20-22` |
| Clipping | near plane only, in view space; far plane is a depth range, not a clip | `scene3.rs:712-715` |
| Raster pixel cost | 5 bytes (RGBA8 + `f32` depth) | `scene3.rs:95-96` |
| Depth convention | OpenGL: device depth 0 at near, 1 at far | `scene3.rs:39-41` |
| Culling | front faces are **negative** image-space area (`cull_back_faces`) | `scene3.rs:44-48`, `:906` |
| Acceleration | **none**: every triangle transformed, clipped and scanned; O(triangles + covered pixels), single-threaded | `scene3.rs:22-25`, `:716-763` |
| Determinism | two renders in one process are byte-identical, asserted | `scene3.rs:73-80` |

**`crates/render/src/picking.rs`** — 18 tests, **not in the build** (see §1.1):

| Fact | Value | Where |
|---|---|---|
| Parallel threshold | `PARALLEL_EPS = 1e-9`, dimensionless (a sine of the angle) | `picking.rs:64` |
| Exact path | `ray_faces_intersection`: plane crossing + `face.contains_local`, tolerance `solid.tolerance()` | `picking.rs:136-177` |
| Mesh path | `ray_mesh_intersection`: Möller–Trumbore, scale-aware `det` test | `picking.rs:192-207`, `:220-253` |
| Cost | O(faces) for the exact path, O(triangles) + O(faces) for `face_at` | `picking.rs:142`, `:262` |
| The two disagree | a region with a hole is triangulated as its outer loop plus reversed triangles over the hole, so **the mesh path reports the hole as surface** | `picking.rs:18-25`, `mesh.rs:86-108` |
| Ties | lower face index wins (`b.distance <= t` → continue); `pick_scene` keeps the earlier solid on a strict `<` | `picking.rs:167`, `:284` |
| Graze rule | a ray **in** a face's plane is refused; a ray **parallel** to it is refused | `picking.rs:34-40`, `:151`, `:159` |
| Pixel → ray | `ray_through_pixel` reads the basis back out of `Camera::view_matrix`; a test projects the hit back onto the pixel | `picking.rs:299`, `:577` |

**`crates/ui-egui/src/viewport3.rs`** — 13 tests, **not in the build**. A pure function over host-owned
data: it takes a `Scene`, a `Frame { revision: u64, image: Image }` and a `ViewportState`, and returns
`egui::Response` + `ViewportDelta { camera_changed, pick_requested }`. It uploads the texture **once
per revision** (`texture()`), not per frame. The picture therefore **lags the camera by one frame**
(`viewport3.rs:13-20`).

**`crates/ui-egui/src/viewport_state.rs`** — 23 tests, **not in the build**. Owns `OrbitCamera`
(`MIN_DISTANCE 1e-6`, `MAX_DISTANCE 1e12`, `ORBIT_RADIANS_PER_PIXEL 0.008`, `FRAME_MARGIN 1.25`,
`MIN/MAX_FOV_DEGREES 1..179`, `POLE_LIMIT = π/2 − 1°`), `Gesture`, `PickRequest`, `Selection`,
`world_per_pixel(distance, fov, height_points)`. It has **no egui in it**, which is why it is
unit-testable without a window.

**`crates/ui-egui/src/panels3/{feature_tree,parameters}.rs`** — 18 tests, **not in the build**.

**`crates/ui-egui/src/gpu.rs`** — the existing 2D GPU seam, the pattern every GPU task copies:

| Fact | Value | Where |
|---|---|---|
| Entry | `install(rs: &egui_wgpu::RenderState, faults, stencil) -> GpuTarget` | `gpu.rs:681` |
| Vertex data | `MeshData { lines, tris, quads, breaks }`, `build_mesh(list, origin, bg, lwdisplay)` | `gpu.rs:104`, `:185` |
| Upload cost, measured by a previous author | "about 0.35 s for a million-primitive drawing" | `gpu.rs:126` |
| Uniform | one 80-byte buffer, written per frame | `gpu.rs:402`, `:726` |
| Shader | WGSL as a `const &str`; only vertex buffers + one uniform, so it runs on WebGPU **and** WebGL2 | `gpu.rs:404`, `:12-13` |
| Alpha | premultiplied, `PREMULTIPLIED_ALPHA_BLENDING` | `gpu.rs:575`, `:179` |
| Faults | `GpuFaults` records uncaptured errors and device loss instead of panicking (#602) | `gpu.rs:616-675` |
| Attachments | `paint` receives egui-wgpu's pass: **one colour attachment, no depth** | `gpu.rs:731-742` |
| Device loss | the handler never closes the app; the app autosaves and tells the user | `gpu.rs:612-615` |

### 1.1 What does not exist: the 3D UI is written and not compiled

This is the first fact of this file and it is checkable in one grep:

| File | Declared as a module? | `#[test]` functions | Consequence |
|---|---|---|---|
| `crates/render/src/scene3.rs` | **yes** (`render/src/lib.rs:27`) | 17 | compiled and tested; **no production caller anywhere** |
| `crates/render/src/picking.rs` | **no** — no `mod picking;` in `render/src/lib.rs` | 18 | **never compiled, never run** |
| `crates/ui-egui/src/viewport3.rs` | **no** — not in `ui-egui/src/lib.rs` | 13 | **never compiled, never run** |
| `crates/ui-egui/src/viewport_state.rs` | **no** | 23 | **never compiled, never run** |
| `crates/ui-egui/src/panels3/**` | **no** | 18 | **never compiled, never run** |

**72 test functions of 3D code have never been executed.** `render::scene3` has a re-export at
`lib.rs:42` and no user. `viewport3::show` has no caller at all: there is no host, no call site, no
application wiring. `REN-VIS-001` is therefore the first task of the whole domain, and the "17 tests"
that `master-plan.md` §4 reports for the renderer is the number that actually runs.

---

## 1. Blocked on other domains — the exact contracts needed

### 1.2 Blocked on `KER`

Same discipline as `docs/plan/SIM.md` §2 and `docs/plan/CAM.md` §1: these are contracts for the
kernel owner (**A**) to accept or replace, not requests to edit `crates/solid`.

| Req ID | Exact signature | Why REN needs it | Evidence |
|---|---|---|---|
| `KER-MSH-005` | Re-export `section`: `pub use section::{Section, section};` | `REN-SECT-008`: the crossing curve drawn where the viewport's section plane cuts the **model** (not the mesh) comes from `section`. **One line** in `lib.rs:93`; it also unblocks five SIM tasks and six CAM tasks. | `lib.rs:93`; `SIM.md` §2. |
| `KER-MSH-003` | `pub struct FaceId(pub u64); pub struct EdgeId(pub u64);` `pub fn face_id(solid: &Solid, at: Vec3, tol: f64) -> Result<FaceId, SolidError>` `pub fn edge_id(solid: &Solid, r: &EdgeRef) -> Result<EdgeId, SolidError>` | `REN-PICK-011` / `REN-VIS-010`: a selection or highlight that survives a rebuild. Today an edge is a geometric point (`chamfer.rs:33-44`) and a face is an index, and both are destroyed by any topology-changing edit. | `master-plan.md` §4 "Topological naming: **None.**" |
| `KER-MSH-006` | `pub fn closest_point(solid: &Solid, p: Vec3, tol: f64) -> Result<(Vec3, f64), SolidError>` | `REN-PICK-013`: snap-to-nearest-surface under the cursor, and the "is the cursor over the part" test for the 3D cursor. No distance-to-shell query exists. | absence in `crates/solid`; only `Plane::distance` per face. |
| `KER-MSH-001` | `pub fn mesh_from_solid_tracked(solid: &Solid) -> Result<TrackedMesh, SolidError>` with `pub struct TrackedMesh { pub mesh: Mesh, pub tri_face: Vec<usize>, pub face_tris: Vec<Vec<u32>> }` | `REN-PICK-002`: mapping a hit triangle to a face in O(1) instead of `face_at`'s O(faces) loop. | `mesh.rs:56-68`. |
| `KER-SRF-003` | analytic surfaces (master-plan §5) | `REN-VIS-005`: adaptive tessellation with a screen-space error band needs real curvature; `REN-MAT-005`: real normals. Until it lands every round is ≤ `MAX_ROUND_SEGMENTS = 64` facets. | `crates/solid/src/lib.rs` "Planar faces only". |
| `KER-BOOL-001..003` | the three open kernel defects | A shell that is open (`KER-BOOL-002`) **shows through**: the renderer draws what is behind it and picking returns the far face. Neither is a rendering bug and both look like one. | `crates/solid/tests/property.rs`. |

### 1.3 Blocked on cross-domain type decisions (not on code)

| Req ID | Exact signature | Owner | Why |
|---|---|---|---|
| `REN-EXT-001` | `pub struct BodyVisibility { pub feature: String, pub visible: bool, pub ghosted: bool }` in `crates/model/src/lib.rs`, plus `Model::body_visibility(&str) -> BodyVisibility` | D / MOD | CADCraft 3D has **no layers and no per-body visibility** (`Model { parameters, features, records }`). `REN-PICK-015` (pick skips a hidden body) and `REN-VIS-003` (a visstyle can hide a body) are both `BLOCKED` on this. |
| `REN-EXT-002` | `pub struct ToolpathOverlay { pub moves: Vec<[f64; 6]>, pub kind: u8 }` consumed by `cadcraft_render::scene3` as an overlay type | CAM + REN | CAM's toolpath preview needs 3D line drawing with per-move colour by cut kind (`CAM-PATH-001`'s `Move::motion`). The renderer must be able to draw polylines in 3D; today `scene3` draws only `Workplane` grid segments. |
| `REN-EXT-003` | `ResultField` adapter: `crates/sim` exposes a scalar field on triangles and the renderer colours by it | SIM + REN | `docs/plan/SIM.md` §11 (`SIM-VIZ-001..009`) needs a viewport result layer. Blocked until `SIM-VIZ-001` exists. |
| `REN-EXT-004` | the drawing→3D bridge: `pub fn scene_from_drawing(d: &Drawing, solid: &Solid) -> Scene` | C | The 2D display list and the 3D scene are separate types today and the 2D canvas and the 3D viewport are separate panels. One viewport showing both is a decision, not a function. |

---

## 2. Epics

| Epic | Name | Tasks | Gate |
|---|---|---|---|
| `REN-GPU` | The GPU 3D path | 24 | `REN-VIS-001` (the panel exists), `REN-GPU-002` (offscreen pass) |
| `REN-PICK` | Picking, selection, snapping | 16 | `KER-MSH-001`, `KER-MSH-003` |
| `REN-SECT` | Section planes and the cut view | 14 | `KER-MSH-005` |
| `REN-VIS` | The viewport: wiring, modes, visstyles, adaptive tessellation | 22 | `REN-VIS-001` |
| `REN-MAT` | Materials, colour, alpha | 18 | `DAT-068` (`DocMaterial`) |
| `REN-LIGHT` | Lighting and shadows | 16 | `REN-GPU-005` (the shader) |
| `REN-PERF` | The measurement harness and the budgets | 18 | **first**: every other budget is a number it produces |
| **total** | | **128** | |

**`REN-PERF` is listed last but must run first.** Every millisecond in this file is currently a
requirement with no measurement behind it; `REN-PERF-001..008` is what makes them facts.

---

# REN-PERF — the measurement harness and every budget (18 tasks)

> **Read this section first.** Nothing else in this file has a measured number. These tasks are what
> turns the budgets in `REN-GPU`, `REN-VIS` and `REN-PICK` into recorded facts.

### REN-PERF-001 — A timing harness, with no new dependency
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/bench.rs` (new; owner PLT), `xtask/src/main.rs` (add the `bench` subcommand)
- **Contract:** `cargo xtask bench --crate render --filter scene3:: --json docs/render-parity-baseline.json`; the module exposes `pub fn run(filter: &str, iters: u32) -> Result<BenchReport, String>` with `pub struct BenchReport { pub name: String, pub iters: u32, pub median_ms: f64, pub p95_ms: f64, pub min_ms: f64 }`
- **Behaviour:** `std::time::Instant`, median and p95 over `iters` after a warm-up of 3. **No
  criterion, no iai, no divan** — none is in `Cargo.toml` and adding one is a dependency, which this
  file does not do. One JSON file per surface area, checked in, read by the regression test.
- **Refusals:** a filter matching no benchmark → `Err` naming the filter and the list. `iters < 5` → `Err("a timing with fewer than 5 iterations is noise")`. A benchmark that takes over `MAX_BENCH_S = 60` → the harness aborts it and says so.
- **Tests:** `the_harness_rejects_a_filter_that_matches_nothing`. `a_median_of_known_durations_is_the_middle_one` — over a synthetic set. `fewer_than_five_iterations_is_refused`. `bench_appears_in_the_xtask_usage`.
- **Acceptance:** `cargo xtask bench --list` prints every benchmark; the JSON is written and committed.
- **Risk:** timing in CI is noisy. Fallback: CI asserts the **counts** (triangles, vertices, bytes) always, and the times only on the reference machine — `REN-PERF-018`.

### REN-PERF-002 — The baseline file and the regression rule
- **State:** READY
- **Depends on:** REN-PERF-001
- **Files:** `docs/render-parity-baseline.json` (new; owner PLT), `xtask/src/bench.rs::check`
- **Contract:** `pub fn check(report: &BenchReport, baseline: &Baseline, max_regression: f64) -> Result<(), String>;` with `max_regression = 0.10` (`QAT-004`'s "> 10 % fails the build"), and `pub struct Baseline { pub machine: String, pub date: String, pub entries: Vec<(String, f64)> }`
- **Behaviour:** A measurement above `1.10 ×` the recorded baseline fails with both numbers. `machine` records CPU model, core count, OS, build profile and compiler version; a baseline from a different machine class is **refused**, not compared, because a number from a laptop is not a number from a workstation.
- **Refusals:** a baseline with a different `machine` → `Err("the baseline was recorded on {machine}; re-record it on this machine")`. A benchmark with no baseline entry → `Err` naming it, not a pass.
- **Tests:** `a_10_percent_regression_fails`. `a_9_percent_regression_passes`. `a_baseline_from_another_machine_is_refused_not_compared`. `a_benchmark_with_no_baseline_is_refused`.
- **Acceptance:** the baseline file names its machine and its date, and the rule is 10 %.
- **Risk:** a baseline recorded on a noisy machine. Fallback: p95, not mean; and the median of 20 iterations.

### REN-PERF-003 — Scene-build budget: document → scene
- **State:** READY
- **Depends on:** REN-PERF-001, REN-VIS-002
- **Files:** `crates/render/tests/perf_scene_build.rs` (owner Q)
- **Contract:** `scene_build_ms_budget`: **≤ 50 ms** to build the scene (tessellate + materials + overlays) for a **20 000-triangle** model at `supersample 1`; and **≤ 5 ms** incremental when only the camera changed (which must not rebuild the scene at all).
- **Behaviour:** The measurement is `scene3::render`'s *setup*, isolated from the raster: tessellation time (`mesh_from_solid`) and scene assembly. The budget is asserted by the test and recorded by `REN-PERF-001`.
- **Refusals:** a model over `MAX_VIEWPORT_TRIS` is refused before the build (not truncated), so the budget has a stated domain.
- **Tests:** `a_20000_triangle_scene_builds_within_50_ms` — asserts the median of 20. `a_camera_move_does_not_rebuild_the_scene` — asserts the build counter is unchanged. `a_model_over_the_triangle_cap_is_refused_not_truncated`.
- **Acceptance:** the measured value is in `docs/render-parity-baseline.json` with its machine.
- **Risk:** tessellation cost depends on the kernel. Fallback: the budget is per-triangle and the count is always asserted, so a kernel regression shows as a count change before it shows as a time.

### REN-PERF-004 — CPU raster budget, per (pixels, triangles) cell
- **State:** READY
- **Depends on:** REN-PERF-001
- **Files:** `crates/render/tests/perf_raster.rs` (owner Q)
- **Contract:** `raster_ms_budget`, a table of six cells, each asserted and recorded:

  | pixels | triangles | budget (ms, median of 20) |
  |---|---|---|
  | 320 × 240 | 500 | ≤ 8 |
  | 320 × 240 | 5 000 | ≤ 25 |
  | 320 × 240 | 20 000 | ≤ 60 |
  | 800 × 600 | 20 000 | ≤ 150 |
  | 1920 × 1080 | 20 000 | ≤ 900 |
  | 1920 × 1080 | 1 000 000 (the `MAX_TRIANGLES` cap) | refused |

- **Behaviour:** The raster is `O(triangles + covered pixels)` with **no acceleration and one thread**
  (`scene3.rs:22-25`), so the table is the honest shape of the cost: linear in triangles, linear in
  covered pixels. The last row is not a slow measurement, it is the documented refusal: past
  `MAX_IMAGE_PIXELS` at `supersample 1` a full-size image cannot be drawn at all, because the
  supersampled buffer would be the same size and a factor of 2 would exceed it (`scene3.rs:95-96`).
- **Refusals:** a cell outside the table is not asserted; the test says so rather than extrapolating.
- **Tests:** `every_raster_cell_meets_its_budget`. `the_table_has_the_six_documented_cells`. `a_supersampled_full_size_image_is_refused` — 1920×1080 at factor 2 exceeds `MAX_RASTER_PIXELS`.
- **Acceptance:** six numbers in the baseline file, each with a machine and a date.
- **Risk:** the last row's refusal may be read as a bug. Fallback: the row is in the table *as a refusal* and the module doc (`scene3.rs:95`) is linked from it.

### REN-PERF-005 — GPU encode, submit and present, in milliseconds
- **State:** BLOCKED
- **Depends on:** REN-GPU-002, REN-PERF-001
- **Files:** `crates/ui-egui/tests/perf_gpu.rs` (owner Q)
- **Contract:** `gpu_frame_budget`: **≤ 8 ms** CPU-side encode + submit at 1920 × 1080 with 2 000 000 triangles, measured with a wgpu timestamp query when `TIMESTAMP_QUERY` is available and with `Instant` around `queue.submit` otherwise.
- **Behaviour:** Three numbers, not one: **encode** (CPU building the command buffer), **submit** (CPU→GPU handoff), **present** (GPU work, from the timestamp query or a readback fence). Reporting one number called "frame time" hides which half moved.
- **Refusals:** an adapter without `TIMESTAMP_QUERY` → the report says `present: unmeasured (no timestamp query)` rather than estimating it. A device that is lost mid-measurement → the measurement is discarded with the fault.
- **Tests:** `the_three_numbers_are_reported`. `a_device_without_timestamp_query_says_so`. `an_encode_over_budget_fails_the_measurement`. `gpu_benchmarks_are_ignored_without_a_gpu`.
- **Acceptance:** three numbers in the baseline file, or an explicit "unmeasured" with the reason.
- **Risk:** the web build has no timestamp query. Fallback: the CPU-side two are still measured and the third is named as unmeasured.

### REN-PERF-006 — The frame budget: 16.7 ms at 60 Hz, 8.3 ms at 120 Hz
- **State:** BACKLOG
- **Depends on:** REN-PERF-005, REN-VIS-015
- **Files:** `crates/ui-egui/tests/perf_frame.rs` (owner Q)
- **Contract:** `frame_budget_ms(hz: u32) -> f64 { 1000.0 / f64::from(hz) }` with the target machine's budget = **0.9 ×** that (**15.0 ms at 60 Hz**, **7.5 ms at 120 Hz**) to leave room for the UI.
- **Behaviour:** A frame is: pick up the input → update the camera → update the scene (incremental) → encode → present → upload to egui. Each stage is measured; the frame budget applies to the sum.
- **Refusals:** a frame that misses the budget is recorded as a miss with the per-stage breakdown, not averaged away.
- **Tests:** `the_frame_budget_is_ninety_percent_of_the_refresh_interval`. `a_missed_frame_is_recorded_with_its_stages`. `the_frame_time_is_the_sum_of_the_stages`.
- **Acceptance:** the budget is derived from the display's refresh rate, not chosen.
- **Risk:** a 60 Hz budget on a 144 Hz panel looks generous. Fallback: the refresh rate is read from egui and the budget computed.

### REN-PERF-007 — Upload budget
- **State:** BLOCKED
- **Depends on:** REN-GPU-003, REN-PERF-001
- **Files:** `crates/ui-egui/tests/perf_upload.rs` (owner Q)
- **Contract:** `upload_ms_budget`: **≤ 350 ms per 1 000 000 primitives**, which is the figure the existing 2D path's own doc states (`gpu.rs:126`), and **≤ 4 ms** for a camera-only frame (no geometry upload at all).
- **Behaviour:** `MeshData::upload_now` is already called off the UI thread by `canvas::rebuild`; the 3D path copies that discipline exactly, so the budget is the same number, not a new one.
- **Refusals:** a scene over the byte budget (`REN-GPU-023`) is refused, not streamed slowly.
- **Tests:** `a_million_primitive_upload_is_within_350_ms`. `a_camera_only_frame_uploads_nothing` — asserts the byte counter is zero. `an_oversized_scene_is_refused`.
- **Acceptance:** the number matches or beats the 2D path's recorded figure, and the two are in the same table.
- **Risk:** a slow disk or driver makes the buffer creation slow. Fallback: the buffer creation is already the dominant cost (`gpu.rs:126`) and it is done once per revision, not per frame.

### REN-PERF-008 — Pick budget
- **State:** BLOCKED
- **Depends on:** REN-PICK-002, REN-PERF-001
- **Files:** `crates/render/tests/perf_pick.rs` (owner Q)
- **Contract:** `pick_ms_budget`: **≤ 2 ms** for one `pick_at` on a solid with **60 000 faces** (`limits::MAX_FACES`).
- **Behaviour:** The current exact path is `O(faces)` (`picking.rs:142`), so at the cap it is 60 000 plane tests plus a `contains_local` per candidate. This budget is why `REN-PICK-001` exists.
- **Refusals:** a pick over the cap is refused (`SceneError`-style) rather than run slowly.
- **Tests:** `a_60000_face_pick_is_within_2_ms`. `the_pick_budget_is_asserted_not_measured_only`. `a_pick_over_the_face_cap_is_refused`.
- **Acceptance:** the number in the baseline file, with the BVH in place.
- **Risk:** the exact path must stay exact. Fallback: the BVH is a *culling* structure over faces, not an approximation of the intersection; `REN-PICK-002`'s test asserts the accelerated answer equals the brute-force one on random rays.

### REN-PERF-009 — Memory budgets, in bytes
- **State:** READY
- **Depends on:** REN-GPU-003, REN-PERF-001
- **Files:** `crates/render/tests/perf_memory.rs` (owner Q)
- **Contract:** `memory_bytes_budget`: CPU image **≤ 5 bytes/pixel** (RGBA8 + `f32` depth, `scene3.rs:95`); supersampled buffer **≤ 5 · s² bytes/pixel** where `s` is the factor; GPU vertex buffer **≤ 32 bytes/triangle** for the 3D vertex format `REN-GPU-003` defines; and a scene's total CPU+GPU bytes **≤ 256 MiB** over `MAX_VIEWPORT_TRIS`.
- **Behaviour:** Counts, not wall time: these are **always** asserted in CI (unlike the times), because a count is deterministic.
- **Refusals:** over budget → `Err(CamError/RenError::Limit)` naming the count and the cap.
- **Tests:** `the_cpu_image_is_five_bytes_per_pixel`. `the_supersampled_buffer_scales_by_the_square_of_the_factor`. `the_gpu_vertex_format_is_at_most_32_bytes_per_triangle`. `a_scene_over_the_memory_budget_is_refused`.
- **Acceptance:** all four counts asserted on every CI run.
- **Risk:** a vertex format that grows. Fallback: the 32-byte test fails when the format grows, which is the moment to decide whether it is worth it.

### REN-PERF-010 — The triangle-count table: what our own models produce
- **State:** READY
- **Depends on:** REN-PERF-003
- **Files:** `crates/render/tests/perf_triangle_counts.rs` (owner Q), `docs/render-parity-baseline.json`
- **Contract:** a table over the kernel's `tests_support` shapes and three named models, recording `faces`, `tris` and `verts`, asserted to the numbers the table holds.
- **Behaviour:** The reference numbers every other budget is expressed against: the unit cube, a 64-facet cylinder, a box with a hole, a chamfered box, and the three corpus models. A **count** regression is caught here, in CI, with no timing involved — and it is the cheapest way to catch a tessellation change.
- **Refusals:** a shape over `limits::MAX_TRIANGLES` → `Err(TooManyTriangles)`, asserted.
- **Tests:** `every_shape_matches_its_recorded_face_and_triangle_count`. `a_cylinder_is_sixty_four_facets` — the `MAX_ROUND_SEGMENTS` bound, asserted as a fact. `a_shape_over_the_cap_is_refused`.
- **Acceptance:** the table is in the baseline file and in the parity doc.
- **Risk:** the counts change when the tessellator changes. Fallback: the test names the old and the new count, so the change is a reviewed diff rather than a silent regression.

### REN-PERF-011 — Thread scaling for the CPU raster
- **State:** READY
- **Depends on:** REN-PERF-004
- **Files:** `crates/render/src/scene3.rs` (parallel `Raster::scan`), `crates/render/tests/perf_threads.rs` (owner Q)
- **Contract:** `pub fn render_threads(scene: &Scene, opts: &RenderOptions, threads: usize) -> Result<Image, SceneError>;` — tiles of `TILE = 64 × 64` raster pixels, one tile per task, `rayon` (already a `cadcraft-render` dependency, `Cargo.toml:23`, non-wasm only) as the executor.
- **Behaviour:** Tiles are **independent** because the depth buffer is per-pixel: a tile reads and writes its own pixels, and the result is byte-identical to the single-threaded path because the depth rule is a strict `<` and the tile boundaries do not change any pixel's value. The 2D build already does this (`lib.rs:551-579`, `build_parallel`), so the pattern exists in this crate.
- **Refusals:** `threads == 0` or > `rayon::current_num_threads()` → clamped, not refused. A tile past the raster → skipped. On wasm (`cfg(target_arch = "wasm32")`) the one-thread path is the only path, and the doc says so.
- **Tests:** `the_parallel_path_is_byte_identical_to_the_single_threaded_path` — the determinism property of `scene3.rs:73-80`, re-asserted. `tiles_do_not_change_a_pixel` — a render at tile size 64 and at 1 024 differs nowhere. `the_speedup_is_measured_and_recorded`. `wasm_uses_one_thread`.
- **Acceptance:** the speedup factor (median of 20, on 1/2/4/8 threads) is in the baseline file, and the byte-identity test is green.
- **Risk:** tiles need a per-tile vertex list, or every thread transforms every triangle. Fallback: a binning pass assigns triangles to tile AABBs first; that is the task's second half and its cost is measured too.

### REN-PERF-012 — The wasm budget
- **State:** BACKLOG
- **Depends on:** REN-GPU-019, REN-PERF-001
- **Files:** `docs/render-parity-baseline.json` (a `wasm` section), `apps/cadcraft-web/` (owner C)
- **Contract:** measured frame time in a browser, recorded with the browser, the GPU and the method, by hand: **target ≤ 33 ms** (30 Hz) at 1280 × 720 with 200 000 triangles on WebGL2.
- **Behaviour:** `cargo xtask wasm` proves it *builds*; this task records what it *does*. The number is obtained by running the web app with a frame-time overlay and is written into the baseline file with the date and the browser name.
- **Refusals:** a number without a browser, a GPU and a date is not a number — the report says `unmeasured` instead.
- **Tests:** `the_wasm_section_is_present_and_names_its_browser`. `an_unmeasured_wasm_budget_says_so`.
- **Acceptance:** the section exists, with a value or an explicit "unmeasured".
- **Risk:** nobody runs the web app with a profiler. Fallback: the section says so, and the CPU-side budgets still hold because the same `scene3` code runs.

### REN-PERF-013 — The LOD ladder, measured at the point it degrades
- **State:** BLOCKED
- **Depends on:** REN-VIS-005, REN-PERF-004
- **Files:** `crates/render/tests/perf_lod.rs` (owner Q)
- **Contract:** `lod_floor_budget`: the coarsest LOD that meets the 16.7 ms frame budget is **recorded per model**, along with the triangle count at which the budget is missed. `pub struct LodPoint { pub model: &'static str, pub max_tris_that_met_budget: usize, pub budget_ms: f64 }`
- **Behaviour:** Measures where the ladder must step down, so `REN-VIS-006`'s thresholds are **measured** rather than guessed, and so the UI can show "displayed at 1/4 detail" when it is.
- **Refusals:** a model whose coarsest LOD still misses the budget → `LodPoint { max_tris_that_met_budget: 0 }` and the model is reported as "cannot be displayed interactively".
- **Tests:** `the_ladder_has_a_measured_step_for_every_model`. `a_coarsest_lod_that_misses_is_reported_as_zero`. `each_lod_step_reduces_the_triangle_count_by_at_least_half`.
- **Acceptance:** the table is in the baseline file, one row per corpus model.
- **Risk:** the ladder depends on the GPU path, which does not exist yet. Fallback: measured on the CPU raster now, re-measured after `REN-GPU` lands, both rows kept.

### REN-PERF-014 — Camera-only move budget
- **State:** BLOCKED
- **Depends on:** REN-GPU-015, REN-PERF-006
- **Files:** `crates/ui-egui/tests/perf_camera.rs` (owner Q)
- **Contract:** `camera_move_budget`: **≤ 2 ms** CPU for a camera-only frame at 1920 × 1080 with 2 000 000 triangles, with **zero** bytes uploaded and **zero** scene rebuilds.
- **Behaviour:** Dragging the mouse must not rebuild geometry or re-upload buffers; only the 80-byte-style uniform changes. This is the budget that makes orbiting feel attached to the cursor.
- **Refusals:** a camera move that uploads a buffer is a failure, not a slow frame — the test counts bytes.
- **Tests:** `a_camera_move_uploads_no_bytes`. `a_camera_move_rebuilds_no_geometry`. `a_camera_move_is_within_2_ms`.
- **Acceptance:** the byte counter is 0 and the time is recorded.
- **Risk:** egui's own repaint of the panel costs something. Fallback: the budget measures the render crate's share and says so.

### REN-PERF-015 — Measurement hygiene
- **State:** READY
- **Depends on:** REN-PERF-001
- **Files:** `xtask/src/bench.rs::hygiene` (owner PLT)
- **Contract:** `pub struct Hygiene { pub warmup: u32, pub iters: u32, pub machine: String, pub profile: String, pub compiler: String }` with the defaults **warmup 3, iters 20**; `pub fn hygiene() -> Hygiene;`
- **Behaviour:** Records, in every baseline entry: warm-up, iteration count, machine class, build profile (`release`, because a `debug` number is not a number), and the compiler version. A baseline without these five fields is refused by `REN-PERF-002`.
- **Refusals:** a `debug`-profile measurement → `Err("a debug measurement is not a baseline")`. Fewer than 5 iterations → refused.
- **Tests:** `a_baseline_without_the_five_fields_is_refused`. `a_debug_measurement_is_refused`. `the_defaults_are_3_and_20`.
- **Acceptance:** every entry in the baseline file has the five fields.
- **Risk:** a baseline recorded with the app closed vs open. Fallback: the machine string includes the load state the reporter typed.

### REN-PERF-016 — Render regression fixtures and a pixel threshold
- **State:** READY
- **Depends on:** REN-PERF-001
- **Files:** `crates/render/tests/fixtures/`, `crates/render/tests/image_diff.rs` (owner Q)
- **Contract:** `pub fn diff(a: &Image, b: &Image, max_channel_delta: u8, max_changed_pct: f64) -> Result<(), String>;` with the thresholds **≤ 2 per channel** and **≤ 0.1 %** of pixels.
- **Behaviour:** Reference PNGs for eight scenes (cube, cylinder, chamfered box, holed plate, grid, camera at a pole, a degenerate solid, an empty scene). The thresholds absorb the two libm calls `scene3.rs:77-80` names (`tan` and `powf`) across platforms while still catching a moved pixel. **Assets:** these PNGs are our own renders, so they need an `ATTRIBUTION.md` row under `PLT-ASSET-008`'s rule — recorded here, not decided here.
- **Refusals:** a different image size → `Err` naming both sizes. More than `max_changed_pct` → `Err` with the count and the two file paths.
- **Tests:** `an_unchanged_fixture_passes`. `a_one_pixel_shift_fails`. `a_two_channel_drift_passes`. `a_size_mismatch_is_refused`.
- **Acceptance:** eight fixtures, each with a verdict, and the PNG diff written out on failure so a human can look.
- **Risk:** a libm difference on another platform fails every fixture. Fallback: the thresholds are per-channel and percentage based, and `scene3.rs:77-80` says the guarantee is per-build, not cross-platform.

### REN-PERF-017 — The numbers into the parity doc
- **State:** BACKLOG
- **Depends on:** REN-PERF-002, REN-PERF-006, REN-PERF-010
- **Files:** `docs/ui-parity.md` (owner PLT), `docs/roadmap.md` (owner PLT)
- **Contract:** a "Rendering" section in `docs/ui-parity.md` with one row per measured budget: name, value, unit, machine, date, and the harness command that produced it.
- **Behaviour:** The row is **the claim**. A budget without a row is a requirement; a row without a command is a rumour. The doc also states the maturity scale (`master-plan.md` §7) for rendering, which today is **1 (demo)** because nothing interactive runs.
- **Refusals:** an unmeasured budget appears as `unmeasured`, never as an estimate.
- **Tests:** `every_baseline_entry_has_a_row`. `every_row_names_its_command`. `the_maturity_scale_is_stated`.
- **Acceptance:** the section exists and a missing row fails the test.
- **Risk:** the doc goes stale. Fallback: the test reads the baseline file, so deleting a row fails CI.

### REN-PERF-018 — What CI asserts, and what only the reference machine asserts
- **State:** READY
- **Depends on:** REN-PERF-002, REN-PERF-009
- **Files:** `crates/render/tests/budgets.rs` (owner Q), `xtask/src/main.rs` (the `ci` step list)
- **Contract:** `pub fn counts_are_ci_safe() -> bool;` — **always** asserted: triangle counts, vertex counts, byte counts, cap refusals, and determinism. **Never** asserted in CI: wall-clock times (they are `#[ignore]`d with the variable `CADCRAFT_PERF=1` that un-ignores them).
- **Behaviour:** This is the task that makes the rest of the budget system honest: CI is shared and noisy, so CI asserts **counts** (deterministic) and the reference machine asserts **times**. `master-plan.md` §5's `QAT-004` asks for a > 10 % regression to fail the build; the way that works is `CADCRAFT_PERF=1 cargo test -p cadcraft-render` on the reference machine, wired into the same `xtask ci` step as an opt-in.
- **Refusals:** a timing assertion in the always-run suite → a test failure (`the_time_tests_must_be_ignored`), because a shared runner's speed is not the product's.
- **Tests:** `the_time_tests_are_ignored_by_default`. `the_count_tests_are_not_ignored`. `cadcraft_perf_enables_the_time_tests`.
- **Acceptance:** `cargo xtask ci` is green and fast; `CADCRAFT_PERF=1 cargo test -p cadcraft-render` is the performance gate.
- **Risk:** nobody sets the variable. Fallback: the baseline file's date is visible in the parity doc, so a stale measurement is obvious.

---

# REN-GPU — the GPU 3D path (24 tasks)

### REN-GPU-001 — The ADR: where wgpu may live, and why
- **State:** READY
- **Depends on:** —
- **Files:** `docs/plan/REN-adr-0001-gpu-lives-in-l5.md` (new; owner PLT)
- **Contract:** the ADR text: `cadcraft-render` is **L2** (`layers.rs:44`) and `UI_MIN_LAYER = 5` with `wgpu` in `UI_CRATES` (`layers.rs:75-78`), so **wgpu may not be named in `crates/render`**. The 3D GPU path therefore lives in `crates/ui-egui/src/gpu3.rs`, beside the 2D path it copies, and `cadcraft-render` supplies vertices, matrices and materials as plain data.
- **Behaviour:** Records the decision, the alternative (a new L3 crate depending on wgpu, which the layer table forbids), and the revisit condition (if `wgpu` is ever allowed below L5, the split can move).
- **Refusals:** n/a (documentation); the *behavioural* rule is `REN-GPU-002`'s file ownership and the layer test.
- **Tests:** `the_adr_is_linked_from_ren_plan`. `render_names_no_gpu_crate` — a test asserting `cadcraft-render/Cargo.toml` has no `wgpu`/`egui` dependency, which is what `cargo xtask layers` enforces.
- **Acceptance:** the ADR exists; `cargo xtask layers` is green with `gpu3.rs` at L5.
- **Risk:** someone adds `wgpu` to `render` for convenience. Fallback: the layer test fails the build.

### REN-GPU-002 — The offscreen pass, because egui's pass has no depth
- **State:** BLOCKED
- **Depends on:** REN-GPU-001, REN-VIS-001
- **Files:** `crates/ui-egui/src/gpu3.rs` (new; owner U)
- **Contract:**
  ```rust
  pub struct Scene3Target { pub colour: wgpu::Texture, pub resolve: wgpu::Texture, pub depth: wgpu::Texture, pub size: [u32; 2], pub samples: u32 }
  impl Scene3Target {
      pub fn ensure(device: &wgpu::Device, size: [u32; 2], samples: u32) -> Result<Scene3Target, RenError>;
      pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, view: &egui_wgpu::ScreenDescriptor, m: &Scene3Mesh, cam: &Camera) -> Result<(), RenError>;
  }
  ```
- **Behaviour:** **`CallbackTrait::paint` receives a pass whose attachments are already fixed** (`gpu.rs:731-742`), so a depth buffer cannot be added to it. The 3D pass is therefore encoded in **`prepare`**, which receives `&mut wgpu::CommandEncoder` (`gpu.rs:699`), into our own colour + depth textures sized to the panel in device pixels, MSAA ×4 with a resolve target, and `paint` then draws one full-screen quad sampling it. This is the whole architectural crux and it is one sentence of design.
- **Refusals:** a panel size of 0 → no target and no draw (`RenError::ZeroSized`). A size past `MAX_VIEWPORT_PIXELS = 8_388_608` (2048 × 4096) → `RenError::Limit`. `samples` not in `1..=4` → clamped to 1 and the report says so. A depth format the adapter does not support → `RenError::Unsupported` naming the format.
- **Tests:** `the_target_is_recreated_when_the_size_changes`. `a_zero_sized_panel_encodes_nothing`. `the_depth_attachment_is_present` — a test reading back the depth texture's usage flags. `samples_of_one_skips_the_resolve_texture`. `an_unsupported_sample_count_is_clamped_and_reported`.
- **Acceptance:** a depth attachment exists, which `egui_wgpu::ScreenDescriptor` cannot give us.
- **Risk:** offscreen means one extra full-screen textured quad per frame. Fallback: it is one quad at the panel's pixel count; the cost is in `REN-PERF-005`.

### REN-GPU-003 — The 3D vertex format and the mesh builder in `render`
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/render/src/scene3_gpu.rs` (new; owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  #[repr(C)]
  pub struct Vertex3 { pub pos: [f32; 3], pub nrm: [f32; 3], pub mat: u16, pub flags: u16 }   // 28 bytes
  #[derive(Clone, Debug, Default)]
  pub struct Scene3Mesh { pub verts: Vec<Vertex3>, pub indices: Vec<u32>, pub materials: Vec<Material3>, pub bounds: Bounds3, pub breaks: Vec<[u32; 3]> }
  pub fn build_scene3(scene: &Scene, opts: &Scene3Options) -> Result<Scene3Mesh, SceneError>;
  ```
- **Behaviour:** Positions are `f32` **relative to a per-scene origin** — the same trick the 2D path uses (`gpu.rs:5-8`, `choose_origin :294`) — so a body at 1e6 mm keeps its precision. `mat` indexes a material array so one draw call can carry many materials. The builder is in `render` and is pure data: **no wgpu type appears in this struct**.
- **Refusals:** more than `MAX_VIEWPORT_TRIS = 4_000_000` triangles → `SceneError::TooManyTriangles { count, max }` (the GPU cap is 4× the kernel's 2 M and 4× `scene3`'s 1 M, and the three numbers are all written down). A coordinate that overflows `f32` after the origin subtraction → `SceneError::Invalid` naming the body. A degenerate triangle (zero cross product, `scene3.rs:754`) is dropped and counted.
- **Tests:** `the_vertex_is_28_bytes`. `positions_are_relative_to_the_origin`. `a_body_at_1e6_keeps_its_precision` — asserts a 10 mm feature is still 10 mm at 1e6. `more_than_the_cap_is_refused`. `a_coordinate_beyond_f32_is_refused`. `the_builder_is_deterministic`.
- **Acceptance:** the 28-byte size is asserted (it is what `REN-PERF-009`'s 32-byte budget depends on).
- **Risk:** a 28-byte vertex is not 32-byte aligned, which some drivers prefer. Fallback: pad to 32 and re-record the budget; the test names the change.

### REN-GPU-004 — One camera, one projection
- **State:** READY
- **Depends on:** REN-GPU-003
- **Files:** `crates/render/src/scene3_gpu.rs::uniform` (owner REN)
- **Contract:** `pub fn camera_uniform(cam: &Camera, aspect: f64) -> [[f32; 4]; 4];` — exactly `cam.view_matrix()` and `cam.projection(aspect)` cast to `f32`, in that order, and nothing else.
- **Behaviour:** The GPU transforms vertices with the **same matrices** the CPU rasteriser uses (`scene3.rs:329-353`). No second projection matrix exists in the repository. This is invariant 3.
- **Refusals:** a camera that fails `Camera::validate` → `SceneError::BadCamera` naming the field (`scene3.rs:356-380`), before a byte is written. A non-finite aspect → the same.
- **Tests:** `the_gpu_uniform_equals_the_cpu_matrix_cast_to_f32`. `an_invalid_camera_is_refused_before_encoding`. `the_uniform_is_the_same_object_every_frame` — the buffer is rewritten, not rebuilt.
- **Acceptance:** `REN-GPU-024`'s parity test is the end-to-end proof.
- **Risk:** `f32` rounding makes the GPU and CPU differ in the last bit. Fallback: the parity test allows 1 px of edge difference and the matrices are identical up to the cast.

### REN-GPU-005 — The shader: flat first, then PBR behind a flag
- **State:** READY
- **Depends on:** REN-GPU-004
- **Files:** `crates/ui-egui/src/gpu3.rs::SHADER3` (owner U), `crates/ui-egui/tests/wgsl.rs` (extend)
- **Contract:** WGSL as a `const &str`, with `vs_main`/`fs_flat` and `fs_pbr` entry points, one uniform and vertex buffers only — so it compiles to WebGPU **and** GLSL ES 3.0, the same constraint the 2D shader honours (`gpu.rs:12-13`).
- **Behaviour:** `fs_flat` reproduces `Material::shade` (`scene3.rs:281-294`) exactly — `rgb = base·(ambient + diffuse·(1−metallic)·max(dot(n,l),0))` — so the two paths agree before PBR is switched on. `fs_pbr` is the Cook–Torrance GGX path from `REN-MAT-005`.
- **Refusals:** a shader that fails to compile → `RenError::Unsupported` naming the entry point and wgpu's message, and the app falls back to the CPU picture rather than drawing nothing. The validation test runs the WGSL **and** its GLSL translation, as `crates/ui-egui` already does for the 2D shader.
- **Tests:** `the_shader_compiles` (the existing naga-based validation, extended). `the_shader_translates_to_gles3`. `fs_flat_matches_the_cpu_equation` — per-pixel comparison against `Material::shade`. `a_shader_error_is_reported_not_swallowed`.
- **Acceptance:** both entry points compile in both backends, and `fs_flat` matches the CPU equation within 1/255 per channel.
- **Risk:** `powf` and the sRGB curve on the GPU differ from libm in the last bit. Fallback: the tolerance is per-channel 1, asserted, and the doc says it is a tolerance and not an exactness claim.

### REN-GPU-006 — Depth test, depth write and back-face culling with the same rule
- **State:** READY
- **Depends on:** REN-GPU-002, REN-GPU-005
- **Files:** `crates/ui-egui/src/gpu3.rs` (owner U)
- **Contract:** `wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), .. }` and `depth_stencil: Some(wgpu::DepthStencilState { format: Depth24Plus, depth_write_enabled: true, depth_compare: Less, .. })`.
- **Behaviour:** The cull rule is `scene3`'s: the kernel winds counter-clockwise seen from outside, which lands **clockwise in image space** because the projection flips `y` and handedness with it (`scene3.rs:44-48`), so the front faces are the negative-area ones and the GPU's `Back` cull matches. The depth compare is `Less`, matching the CPU's strict `<` (`scene3.rs:873`) and therefore the byte-identity determinism.
- **Refusals:** a pipeline whose depth format is not `Depth24Plus` → `RenError::Unsupported` naming the format. `cull_back_faces: false` in the options disables culling on both paths (they must agree).
- **Tests:** `the_cull_mode_matches_the_cpu_rule` — a box seen from outside shows three faces on both paths. `a_closed_solid_shows_no_interior_faces`. `depth_compare_is_less`. `disabling_cull_shows_the_far_faces_on_both_paths`.
- **Acceptance:** a solid renders identically (within the edge tolerance) on both paths.
- **Risk:** getting the cull mode backwards makes every model inside-out. Fallback: the parity test in `REN-GPU-024` fails loudly and names which path disagrees.

### REN-GPU-007 — Near-plane clipping in the vertex shader
- **State:** READY
- **Depends on:** REN-GPU-005
- **Files:** `crates/ui-egui/src/gpu3.rs::SHADER3` (owner U)
- **Contract:** the shader clips against `w > ε` by moving a vertex behind the eye onto the near plane and setting its position to the guard value, which the rasteriser discards; `fs_*` receives a `guard` flag through the provoking vertex.
- **Behaviour:** The CPU path clips in **view space, before projection** (`scene3.rs:1002-1027`, Sutherland–Hodgman on one plane). Without a matching clip the GPU produces inverted triangles across `w = 0`, which is the classic "a face turns inside out when you fly through it".
- **Refusals:** a `near <= 0` camera → `Camera::validate` refuses first (`scene3.rs:372-374`). A vertex whose clip produces a non-finite position → the shader discards it; the host cannot see it, so the parity test is the guard.
- **Tests:** `a_triangle_crossing_the_near_plane_is_clipped_not_inverted`. `a_camera_inside_a_face_draws_nothing`. `the_gpu_and_cpu_agree_with_the_camera_inside_the_model`.
- **Acceptance:** flying the camera through a box shows no inverted geometry on either path.
- **Risk:** the guard-vertex approach differs between wgpu and WebGL2. Fallback: the clip is done by a conservative "collapse to a degenerate triangle" and the parity test runs on both backends in CI.

### REN-GPU-008 — The composite: sampling the offscreen colour in `paint`
- **State:** BLOCKED
- **Depends on:** REN-GPU-002
- **Files:** `crates/ui-egui/src/gpu3.rs::Scene3Callback` (owner U)
- **Contract:** `impl egui_wgpu::CallbackTrait for Scene3Callback` — `prepare` encodes the 3D pass and copies the resolved texture into a staging texture bound to a bind group; `paint` draws one full-screen triangle sampling it, inside egui's scissor.
- **Behaviour:** Honours `linear_out` (`gpu.rs:595`): when the target format is `srgb`, the offscreen colour texture is `rgba16float` and the shader writes linear; otherwise it is `rgba8unorm` and the shader encodes. Getting this backwards is a washed-out or over-contrasted viewport, and it is silent.
- **Refusals:** a missing or stale target → `paint` draws **nothing** and returns (the egui canvas shows through), rather than sampling an unwritten texture. A zero-size screen → the same (`gpu.rs:738-741` is the precedent).
- **Tests:** `the_composite_respects_the_scissor_rect`. `an_absent_target_draws_nothing`. `linear_out_matches_the_cpu_picture`. `the_composite_does_not_write_depth`.
- **Acceptance:** the offscreen texture appears inside the panel and nowhere else.
- **Risk:** sampling an unwritten texture is undefined in wgpu. Fallback: the target is cleared at creation and re-cleared on resize, and the "absent" case is a real branch.

### REN-GPU-009 — Resize and device-pixel ratio
- **State:** BLOCKED
- **Depends on:** REN-GPU-008
- **Files:** `crates/ui-egui/src/gpu3.rs` (owner U)
- **Contract:** `pub fn target_size(screen: &egui_wgpu::ScreenDescriptor, rect: egui::Rect, ppp: f32) -> [u32; 2];`
- **Behaviour:** The target is `rect.size() × ppp`, rounded up, and **recreated when it changes** (`REN-GPU-002`'s `ensure`). A fractional scale factor must not produce a target 1 px smaller than the panel, which shows as a 1-px seam.
- **Refusals:** a computed size of 0 → `RenError::ZeroSized`; over `MAX_VIEWPORT_PIXELS` → `RenError::Limit` naming the pixels and the cap.
- **Tests:** `a_resized_panel_recreates_the_target`. `a_fractional_scale_does_not_leave_a_seam`. `a_panel_that_fits_without_resizing_reuses_the_target`. `an_oversized_panel_is_refused`.
- **Acceptance:** resizing the dock from 400 px to 1600 px redraws correctly with no leak of the old texture.
- **Risk:** rapid resizing recreates textures every frame. Fallback: a one-frame debounce is not needed if `ensure` is cheap; the task measures it.

### REN-GPU-010 — Upload budget and the off-thread build
- **State:** BLOCKED
- **Depends on:** REN-GPU-003, REN-PERF-007
- **Files:** `crates/ui-egui/src/gpu3.rs::upload` (owner U)
- **Contract:** `pub fn upload_now(device: &wgpu::Device, mesh: &mut Scene3Mesh) -> Result<Uploaded3, RenError>;` with the CPU arrays dropped afterwards, exactly as `MeshData::upload_now` does (`gpu.rs:123-137`).
- **Behaviour:** Buffer creation costs about **0.35 s per million primitives** on the 2D path (`gpu.rs:126`, measured by a previous author). The 3D path inherits that budget unchanged and builds on a worker thread, with the slot/`Arc<Mutex<Option<…>>>` handoff the 2D path already uses (`MeshSlot`, `gpu.rs:308`).
- **Refusals:** a scene over the memory budget (`REN-PERF-009`) is refused before any allocation; a failed upload returns `RenError` and leaves the previous frame on screen.
- **Tests:** `a_million_triangle_upload_is_within_the_budget`. `the_cpu_arrays_are_released_after_upload`. `a_failed_upload_leaves_the_previous_frame`. `a_camera_only_frame_uploads_nothing`.
- **Acceptance:** the same 350 ms/10⁶ figure as the 2D path, in the same table.
- **Risk:** a 4 M-triangle scene is 1.4 s of buffer creation. Fallback: the LOD ladder (`REN-VIS-006`) keeps the count below 1 M in practice, and the refusal is honest above it.

### REN-GPU-011 — Device loss and the CPU fallback
- **State:** READY
- **Depends on:** REN-GPU-001
- **Files:** `crates/ui-egui/src/gpu3.rs`, `crates/ui-egui/tests/gpu_faults.rs` (extend; owner U)
- **Contract:** reuses `GpuFaults` (`gpu.rs:616`) exactly: the handlers `install` registers (`gpu.rs:683-685`) already record uncaptured errors and device loss instead of panicking, and `CadApp::gpu_fault_tick` autosaves and tells the user (`gpu.rs:612-615`). No new fault type.
- **Behaviour:** On a device loss the 3D path **falls back to the `scene3` CPU picture** so the viewport keeps working (slowly) until the app restarts, and the panel says the GPU path is off. Never a blank panel and never a panic (#602's rule).
- **Refusals:** a lost device with **no** CPU fallback available (wasm without WebGL2) → the panel draws the "no renderer" message, not an empty rectangle.
- **Tests:** `a_device_loss_falls_back_to_the_cpu_picture`. `a_device_loss_is_recorded_not_panicked`. `the_fault_message_names_the_device_lost_reason`. `a_loss_with_no_fallback_shows_the_message`.
- **Acceptance:** the existing `gpu_faults.rs` test covers the 2D path and this extends it to the 3D path.
- **Risk:** the fallback needs the tessellation the GPU path was avoiding. Fallback: `scene3` already tessellates on demand (`scene3.rs:747`), so the fallback has a cost, not a gap.

### REN-GPU-012 — MSAA, and what it costs
- **State:** BLOCKED
- **Depends on:** REN-GPU-002
- **Files:** `crates/ui-egui/src/gpu3.rs` (owner U)
- **Contract:** `samples: 1 | 4`, a `resolve_target` when 4, and `pub fn msaa_cost(samples: u32) -> f64 { 1.0 + 0.6 * f64::from(samples == 4) }` as the *recorded* cost model.
- **Behaviour:** 4× MSAA on the offscreen target, resolved into the sampled texture. The cost model is a number to be **measured** in `REN-PERF-005`, not asserted; the task's job is to make it measurable and to let it be switched off.
- **Refusals:** `samples = 4` on an adapter with no multisampled `rgba8unorm` colour attachment → falls back to `samples = 1` and the report says so.
- **Tests:** `samples_of_four_creates_a_resolve_target`. `an_adapter_without_msaa_falls_back_to_one`. `msaa_does_not_change_the_silhouette_pixels`. `the_sample_count_is_reported`.
- **Acceptance:** both sample counts render and the difference is measured.
- **Risk:** 4× MSAA at 1920 × 1080 is 32 MiB of colour plus 32 MiB of depth. Fallback: it is counted by `REN-PERF-009` and refused above the memory budget.

### REN-GPU-013 — Depth precision, and near/far from the scene
- **State:** READY
- **Depends on:** REN-GPU-004
- **Files:** `crates/render/src/scene3.rs::depth_range` (owner REN — **the change is in `render`, not in the GPU code**)
- **Contract:** `pub fn depth_range(bounds: &Bounds3, fov_degrees: f64) -> (f64, f64);` returning a near and a far for the depth buffer, and a `pub const MIN_DEPTH_RATIO: f64 = 1.0e4;`
- **Behaviour:** A fixed `near = 0.1`, `far = 1000` (today's default, `scene3.rs:318`) gives a far/near ratio of **10⁴**, which is most of a 24-bit depth buffer's precision wasted on empty space. `depth_range` sets `near` from the camera's distance to the scene bounds and `far` to `distance + diagonal·2`, and **refuses** when the ratio exceeds `MIN_DEPTH_RATIO`, naming both numbers.
- **Refusals:** a ratio over `MIN_DEPTH_RATIO` → `SceneError::BadCamera("far/near is {r}; the limit is {max}")`. A non-finite bound → `SceneError::NonFinite`. This is the same class of refusal as `check_window` in `crates/io` (`lib.rs:137`).
- **Tests:** `the_depth_range_follows_the_scene`. `a_scene_far_from_the_origin_gets_a_proportional_far`. `a_ratio_over_the_limit_is_refused_with_both_numbers`. `a_tiny_scene_gets_a_tight_range`. `z_fighting_is_absent_within_the_range` — a 0.01 mm gap at 1 m resolves.
- **Acceptance:** a part at 1e6 with a 10 mm feature shows no z-fighting, which the fixed range does not.
- **Risk:** changing the near plane changes the projection matrix, which is shared. Fallback: the camera is updated, not the projection function; `REN-GPU-004`'s one-projection invariant still holds.

### REN-GPU-014 — Instancing repeated bodies
- **State:** BLOCKED
- **Depends on:** REN-GPU-003, ASM-001
- **Files:** `crates/render/src/scene3_gpu.rs::instances` (owner REN)
- **Contract:** `pub struct Instance { pub transform: [[f32; 4]; 4], pub mat: u16 }` and `pub fn build_instances(scene: &Scene, groups: &[(usize, usize)]) -> Result<Vec<Instance>, SceneError>;`
- **Behaviour:** An assembly places the same component N times; one mesh, N transforms in an instance buffer, one draw call. BLOCKED on `ASM-001` (`cadcraft-asm` does not exist) — the renderer takes the grouping as data so it does not depend on the assembly type.
- **Refusals:** more than `MAX_INSTANCES = 65_535` → `SceneError::Limit` naming both. A non-finite transform entry → `SceneError::NonFinite("instance {i}")`. A grouping that names a body index outside `Scene::solids` → `SceneError::Invalid`.
- **Tests:** `one_mesh_and_n_instances_is_one_draw_call`. `a_non_finite_instance_transform_is_refused`. `the_instance_limit_is_refused`. `an_instanced_body_picks_to_the_right_instance` — feeds `REN-PICK-008`.
- **Acceptance:** a 64-occurrence assembly is one draw call, measured in `REN-PERF-005`.
- **Risk:** a non-uniform scale in an instance transform makes the normals wrong. Fallback: the instance transform is rigid (rotation + translation) and a scale is refused by name.

### REN-GPU-015 — A camera-only frame uploads nothing
- **State:** BLOCKED
- **Depends on:** REN-GPU-004, REN-GPU-010
- **Files:** `crates/ui-egui/src/gpu3.rs` (owner U), `crates/ui-egui/tests/perf_camera.rs`
- **Contract:** the upload byte counter is a field on the callback's state: `pub struct UploadCounters { pub bytes: u64, pub builds: u64 }` with `pub fn counters(&self) -> UploadCounters;`
- **Behaviour:** Dragging the camera writes the uniform and re-encodes; it does not rebuild the mesh, re-tessellate or create a buffer. This is what makes the viewport feel attached to the cursor, and it is asserted by **counting bytes**, not by timing.
- **Refusals:** a camera move that rebuilds the scene is a test failure with the counters printed.
- **Tests:** `a_camera_move_writes_only_the_uniform`. `a_model_change_rebuilds_once`. `the_counters_are_zero_for_an_orbiting_frame`.
- **Acceptance:** counters 0 bytes / 0 builds while orbiting.
- **Risk:** a per-frame uniform write is a `queue.write_buffer` of 64–80 bytes (`gpu.rs:402`), which is free. Fallback: none needed.

### REN-GPU-016 — Whole-body frustum culling, and the culled fraction
- **State:** BLOCKED
- **Depends on:** REN-PERF-001, REN-VIS-002
- **Files:** `crates/render/src/scene3_cull.rs` (owner REN)
- **Contract:** `pub fn cull(scene: &Scene, cam: &Camera) -> Result<CullReport, SceneError>;` with `pub struct CullReport { pub drawn: Vec<usize>, pub culled: Vec<usize>, pub culled_tris: usize, pub total_tris: usize }`
- **Behaviour:** `scene3` culls **nothing**: every triangle of every body is transformed, clipped and scanned (`scene3.rs:22-25`). A per-body frustum test against the body's `Bounds3` (`Scene::bounds` already exists, `scene3.rs:561`) is the cheapest acceleration available and the measurement `scene3.rs` says it lacks. The **culled fraction** is reported and recorded, because "how much of the model is off screen" is the number that tells you whether culling is worth anything.
- **Refusals:** a camera that fails `validate` → `SceneError::BadCamera`. A body with a non-finite bound → `SceneError::NonFinite`, not silently drawn.
- **Tests:** `a_body_outside_the_frustum_is_culled`. `a_body_straddling_the_frustum_is_drawn`. `the_culled_fraction_is_reported`. `culling_does_not_change_the_pixels` — the image is identical, which is the correctness property.
- **Acceptance:** identical pixels, with the fraction recorded in `REN-PERF-004`'s table.
- **Risk:** culling by AABB rejects a body whose *bound* is outside while its geometry is not (a rotated thin part). Fallback: the test uses a rotated part and the AABB is conservative.

### REN-GPU-017 — LOD selection at build time
- **State:** BLOCKED
- **Depends on:** REN-VIS-005, REN-GPU-003
- **Files:** `crates/render/src/scene3_lod.rs` (owner REN)
- **Contract:** `pub struct LodBand { pub screen_error_mm: f64 }` and `pub fn select_lod(scene: &Scene, cam: &Camera, ppp: f32) -> Result<Vec<lod::Level>, SceneError>;` with `pub const LOD_LEVELS: [f64; 5] = [0.05, 0.1, 0.25, 0.6, 1.5];` (millimetres of screen error)
- **Behaviour:** Picks a tessellation band per body from the projected size and the requested screen error. It can only pick between **existing** resolutions, because the kernel has no tessellation-density parameter (`mesh.rs:54` takes none) — so the levels are *our* refinements of the same kernel mesh, computed once and cached.
- **Refusals:** a `screen_error_mm` ≤ 0 → `SceneError::BadCamera`. A body whose finest level is still over `MAX_VIEWPORT_TRIS` → `SceneError::TooManyTriangles`, not a silent coarse level.
- **Tests:** `a_distant_body_gets_a_coarser_level`. `a_near_body_gets_a_finer_level`. `the_finest_level_matches_the_default_mesh`. `a_scene_over_the_cap_is_refused`.
- **Acceptance:** the level table is in `REN-PERF-013`'s measured ladder.
- **Risk:** popping when the level changes. Fallback: the level is a hysteresis band and the camera's own rate is known; the test asserts no level flip inside the band.

### REN-GPU-018 — The transparency pass
- **State:** BLOCKED
- **Depends on:** REN-GPU-002, REN-MAT-011
- **Files:** `crates/ui-egui/src/gpu3.rs` (owner U)
- **Contract:** two passes inside the one encoder: opaques first with `depth_write_enabled: true`, then transparents sorted back-to-front with `depth_write_enabled: false` and `blend: PREMULTIPLIED_ALPHA_BLENDING` (`gpu.rs:575`).
- **Behaviour:** `scene3` writes alpha and **never blends**, and nothing is sorted (`scene3.rs:20-22`) — so a transparent body on the CPU path draws as its last triangle wins the depth test, which is not what transparency means. The GPU path does it properly; the CPU path's behaviour is documented as wrong rather than fixed here.
- **Refusals:** a transparent material with no depth sort key → `RenError::Unsupported`. More than `MAX_TRANSPARENT_BODIES = 256` → `RenError::Limit`.
- **Tests:** `transparents_are_sorted_back_to_front`. `a_transparent_body_does_not_write_depth`. `two_overlapping_transparents_composite_in_order`. `the_cpu_path_documents_that_it_does_not_blend`.
- **Acceptance:** the transparency test is the acceptance; the CPU divergence is a documented difference, not a silent one.
- **Risk:** sorting per object is wrong for intersecting geometry. Fallback: per-**triangle** sorting for small meshes is out of scope; the doc says sorting is per body and a self-intersecting transparent body is not correctly composited.

### REN-GPU-019 — WebGL2: the shader must translate
- **State:** BLOCKED
- **Depends on:** REN-GPU-005
- **Files:** `crates/ui-egui/tests/wgsl.rs` (extend; owner U)
- **Contract:** the existing test validates the 2D shader's WGSL and its **GLSL ES 3.0** translation; this extends it to `fs_flat`, `fs_pbr`, `vs_main` and the composite.
- **Behaviour:** WebGL2 has no compute, no storage buffers and no timestamp queries, and `MSAA` is core but `sample count 4` on a renderbuffer may not be. The shader is therefore written with vertex buffers and one uniform only (`gpu.rs:12-13`), and the test proves the translation.
- **Refusals:** a shader construct that does not translate → the test fails with naga's message; the fallback is to simplify the shader, never to disable the web build.
- **Tests:** `every_3d_entry_point_translates_to_gles3`. `the_composite_shader_translates`. `no_storage_buffers_are_used`.
- **Acceptance:** `cargo xtask wasm` green and the translation test green.
- **Risk:** a WebGL2 driver rejecting MSAA 4×. Fallback: `samples = 1` on that backend, already handled by `REN-GPU-012`.

### REN-GPU-020 — Picking from an id buffer
- **State:** BLOCKED
- **Depends on:** REN-GPU-002
- **Files:** `crates/ui-egui/src/gpu3.rs::id_pass`, `crates/render/src/scene3_gpu.rs::id` (owner U + REN)
- **Contract:** `pub struct IdTarget { pub texture: wgpu::Texture, pub size: [u32; 2] }` and `pub fn encode_id(encoder: &mut wgpu::CommandEncoder, target: &IdTarget, m: &Scene3Mesh) -> Result<(), RenError>;` — a second render pass writing `u32` body ids to an `r32uint` attachment.
- **Behaviour:** The click's pixel is read back from the id texture, so picking costs **one pixel copy** instead of a ray cast over every face. This is the GPU answer to the O(faces) problem in `picking.rs:142`.
- **Refusals:** a readback that is not complete → the frame's pick returns "not ready" rather than a wrong id. A pixel with id `0` → no hit, which is the background. A body index over `MAX_VIEWPORT_BODIES = 65_535` → `RenError::Limit` at build time.
- **Tests:** `an_id_buffer_read_names_the_body`. `the_background_reads_as_zero`. `a_readback_that_is_not_ready_returns_none_not_a_wrong_id`. `the_gpu_pick_agrees_with_the_cpu_pick` — the decisive test.
- **Acceptance:** the GPU and CPU picks agree on all fixtures.
- **Risk:** a readback stalls the pipeline. Fallback: the id pass runs only on a click, not per frame, and the copy is one pixel.

### REN-GPU-021 — Attachments that fail, and what is reported
- **State:** READY
- **Depends on:** REN-GPU-002
- **Files:** `crates/ui-egui/src/gpu3.rs`, `crates/ui-egui/tests/gpu_faults.rs` (owner U)
- **Contract:** `pub enum RenError { ZeroSized, Limit { what: &'static str, count: usize, max: usize }, BadCamera(&'static str), Unsupported(String), DepthFormat(String), Encode(String), NotReady }` and `impl std::fmt::Display for RenError`.
- **Behaviour:** The refusal type for every GPU task. Every variant renders with the number or the name involved, exactly as `SceneError` does (`scene3.rs:147-161`). A texture allocation failure is caught **before** the call by the memory budget (`REN-PERF-009`), so the common case is a refusal and not a wgpu out-of-memory error.
- **Refusals:** a missing depth format → `Unsupported` naming it; an encoder failure → `Encode(wgpu's message)`; a pick readback not ready → `NotReady`, which callers treat as "ask again", not as a failure.
- **Tests:** `every_variant_renders_its_numbers`. `a_depth_format_is_named`. `an_encode_failure_is_reported_not_swallowed`. `not_ready_is_not_an_error`.
- **Acceptance:** the type is the single error of the GPU path and every fallible function returns it.
- **Risk:** `RenError` overlapping `SceneError`. Fallback: `RenError::BadCamera` carries the same `&'static str` and a `From<SceneError>` conversion exists so a caller need not match both.

### REN-GPU-022 — Timestamp queries, or an honest wall clock
- **State:** BACKLOG
- **Depends on:** REN-GPU-002
- **Files:** `crates/ui-egui/src/gpu3.rs::timing` (owner U)
- **Contract:** `pub fn timings(device: &wgpu::Device) -> Option<Timings>;` with `pub struct Timings { pub encode_ms: f64, pub gpu_ms: f64 }` — `None` when the adapter does not expose `TIMESTAMP_QUERY`.
- **Behaviour:** Where the query exists, the GPU's own elapsed time is read after a frame; where it does not, the number is `None` and **says so**. An estimate is never printed.
- **Refusals:** a query read that has not resolved → `None`, and the previous frame's value is kept with a note.
- **Tests:** `timings_are_none_without_the_query_feature`. `timings_are_present_with_it`. `a_pending_read_returns_none`.
- **Acceptance:** `REN-PERF-005` gets a real GPU number where the hardware allows one.
- **Risk:** the feature is optional on some backends. Fallback: the CPU-side encode number is always available and is what CI records.

### REN-GPU-023 — Caps, and what happens when one is hit
- **State:** READY
- **Depends on:** REN-GPU-003
- **Files:** `crates/render/src/scene3_gpu.rs::caps` (owner REN)
- **Contract:** `pub const MAX_VIEWPORT_TRIS: usize = 4_000_000;` `pub const MAX_VIEWPORT_PIXELS: usize = 8_388_608;` `pub const MAX_VIEWPORT_BODIES: usize = 65_535;` `pub const MAX_DRAW_CALLS: usize = 4_096;` and `pub fn budget_report(scene: &Scene) -> Result<BudgetReport, SceneError>;`
- **Behaviour:** Every cap is a **refusal or a degradation, never a truncation**: over the triangle cap → `SceneError::TooManyTriangles`; over the pixel cap → `RenError::Limit`; over the draw-call cap → the bodies are merged by material **and the merge is reported**, because a silent merge is a silent visual change.
- **Refusals:** each cap returns the count and the maximum in the message.
- **Tests:** `every_cap_names_its_count_and_maximum`. `over_the_draw_call_cap_merges_and_reports`. `nothing_is_truncated_silently` — a test asserting the drawn triangle count equals the requested one or an `Err`.
- **Acceptance:** the four constants are the only caps in the GPU path, asserted by a test that greps for others.
- **Risk:** a cap hit on a legitimate 5 M-triangle assembly. Fallback: the number is in the message, and raising it is a decision.

### REN-GPU-024 — The parity test: the CPU picture and the GPU picture
- **State:** BLOCKED
- **Depends on:** REN-GPU-006, REN-GPU-007, REN-GPU-008, REN-GPU-012
- **Files:** `crates/ui-egui/tests/gpu_parity.rs` (owner Q)
- **Contract:** `pub fn parity(cpu: &cadcraft_render::scene3::Image, gpu: &[u8], max_channel_delta: u8, max_edge_px: f64) -> Result<(), String>;` — on eight fixtures, with **max_channel_delta ≤ 2** and **max_edge_px ≤ 1.0**.
- **Behaviour:** Renders the same scene on both paths at the same size with `fs_flat` (not PBR, so the comparison is about geometry) and compares: a silhouette that matches within 1 px, and interior pixels within 2/255. Supersampling is off on both, because the CPU box filter averages in the encoded domain (`scene3.rs:66-70`) and a GPU resolve does not.
- **Refusals:** a size mismatch → `Err` naming both. A channel delta over the bound → `Err` with the worst pixel's coordinates and both values.
- **Tests:** `the_two_paths_agree_on_all_eight_fixtures`. `an_edge_shift_of_one_pixel_passes`. `a_channel_delta_of_three_fails`. `disabling_cull_changes_both_paths_the_same_way`.
- **Acceptance:** eight fixtures, both bounds met; the test writes both PNGs on failure so a human can look at them.
- **Risk:** the two paths differ in the last bit by libm. Fallback: the per-channel bound of 2 is the documented tolerance and `scene3.rs:77-80` is cited in the test's comment.

---

# REN-VIS — the viewport itself (22 tasks)

### REN-VIS-001 — Put the 3D modules in the build
- **State:** READY
- **Depends on:** —
- **Files:** `crates/render/src/lib.rs` (add `pub mod picking;` and the re-exports; owner REN — **`scene3.rs` is agent U's in wave 1, this file is not**), `crates/ui-egui/src/lib.rs` (add `pub mod viewport3; pub mod viewport_state; pub mod panels3;`; owner U)
- **Contract:** four module declarations and the re-exports `pub use picking::{Ray, Hit, pick_at, pick_scene, ray_through_pixel};` (`lib.rs:42`'s existing list gains them).
- **Behaviour:** **72 test functions of 3D code have never been compiled** (§1.1). This task declares them and does nothing else; the first compile will produce errors, which are the real content of the task, and they are listed here so nobody is surprised: `viewport_state.rs:525` calls `cadcraft_render::picking::ray_through_pixel` (fine once the module exists), `viewport3.rs:565` calls `picking::pick_at` (same), and `panels3/mod.rs` is a module tree whose parent does not exist.
- **Refusals:** if a declaration cannot compile without changing another owner's file, the task is **BLOCKED** and the compile error is the report — not a silent edit to `scene3.rs`.
- **Tests:** `every_3d_module_is_declared` — a test that the four module paths resolve. `the_3d_test_count_is_at_least_72` — asserting the count does not fall, so a module cannot be quietly un-declared. `the_2d_test_count_is_unchanged`.
- **Acceptance:** `cargo xtask ci` green; the test count for the workspace rises by **72**.
- **Risk:** the errors are in a 400-line camera file that has never seen the compiler. Fallback: this task's acceptance is "compiles and its 72 tests run", and if a fix needs `scene3.rs`, it is raised to agent U as a co-sequenced change.

### REN-VIS-002 — The host: a scene from the document, with a revision counter
- **State:** READY
- **Depends on:** REN-VIS-001
- **Files:** `crates/ui-egui/src/host3.rs` (new; owner U), `apps/cadcraft/src/workspace.rs` (owner C)
- **Contract:**
  ```rust
  pub struct Viewport3Host { pub state: ViewportState, pub frame: Frame, pub scene: Scene, pub revision: u64, pub dirty: bool }
  impl Viewport3Host {
      pub fn sync(&mut self, model: Option<&cadcraft_model::Model>) -> Result<(), ModelError>;  // bumps `revision` only when the geometry changed
      pub fn render(&mut self) -> Result<(), SceneError>;                                   // scene3 CPU path
      pub fn take_pick(&mut self) -> Option<PickResult>;
  }
  ```
- **Behaviour:** The piece `viewport3.rs` deliberately does not own ("It owns nothing", `viewport3.rs:4-11`): building the `Scene` from the document's solid, carrying the revision, rendering on a worker thread when there is one, and resolving `PickRequest` into a `PickResult`. `revision` is bumped **only** when the geometry changed, because the panel uploads the texture once per revision (`viewport3.rs:50-53`) and a camera move must not re-upload (`REN-GPU-015`).
- **Refusals:** a document with no solid → an empty scene and the panel's existing "No solid to show" hint (`viewport3.rs:147`), not an error. A solid the kernel refuses to tessellate → `SceneError::Solid` surfaced in the panel as a message (`REN-VIS-017`).
- **Tests:** `a_camera_move_does_not_bump_the_revision`. `a_parameter_change_bumps_it_once`. `a_document_with_no_solid_yields_an_empty_scene`. `a_tessellation_refusal_is_reported_not_hidden`.
- **Acceptance:** the host is the only thing that touches `Model`, and the panel still owns nothing.
- **Risk:** re-rendering on a worker thread needs a channel and a cancellation flag (`Options::cancel` is the 2D precedent, `lib.rs:233`). Fallback: render on the UI thread first, move off it when a budget is missed.

### REN-VIS-003 — The display modes
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/render/src/scene3_modes.rs` (owner REN), `crates/ui-egui/src/host3.rs` (owner U)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum DisplayMode { Wireframe, Shaded, ShadedWithEdges, HiddenLine, XRay, Monochrome }
  impl DisplayMode { pub fn needs_edges(self) -> bool; pub fn needs_alpha(self) -> bool; }
  ```
- **Behaviour:** Six modes, each a documented pipeline: `Wireframe` (edges only), `Shaded` (triangles only), `ShadedWithEdges`, `HiddenLine` (a CPU computation, see the refusal), `XRay` (alpha 0.25 with the transparency pass), `Monochrome` (one colour). **`scene3` today does shaded only, with an optional `cull_back_faces`** — the other five are new work.
- **Refusals:** `HiddenLine` is refused: a true hidden-line renderer needs silhouette and visibility computation over the projected edges, and a z-buffer edge test shows *silhouette* edges only, not interior occlusion edges. It is `RenError::Unsupported("hidden-line removal needs a visibility algorithm; use ShadedWithEdges")` naming the alternative, **not** a z-buffer approximation presented as hidden line.
- **Tests:** `each_mode_is_a_documented_pipeline`. `wireframe_draws_no_triangles`. `xray_uses_the_transparency_pass`. `hidden_line_is_a_named_refusal_with_its_alternative`. `monochrome_ignores_the_material`.
- **Acceptance:** five modes work, one is refused by name, and the refusal is in the UI's list with its reason.
- **Risk:** users want hidden line. Fallback: the refusal names what exists, which is the house rule.

### REN-VIS-004 — Edges: the model's edges, or the tessellation's
- **State:** READY
- **Depends on:** REN-VIS-001, KER-MSH-003
- **Files:** `crates/render/src/scene3_edges.rs` (owner REN)
- **Contract:** `pub fn model_edges(solid: &Solid, tol: f64) -> Result<EdgeSet, SceneError>;` with `pub struct EdgeSet { pub segments: Vec<Segment>, pub count: usize }`, plus `pub fn mesh_edges(mesh: &Mesh, dihedral_deg: f64) -> EdgeSet;`
- **Behaviour:** Two different things, and conflating them is the classic viewport bug. **Model edges** come from `Solid::edges()` — exact, the real B-rep edges — but it returns `Option<Vec<Edge>>` and **`None` when any edge is not used by exactly two faces** (`solid.rs:702`), i.e. `None` on an open shell, and a chamfer creates a new edge set. **Mesh edges** come from the triangulation and include every triangle diagonal unless a dihedral test removes them. `DisplayMode::Wireframe` wants the mesh edges; `ShadedWithEdges` wants the model edges.
- **Refusals:** a solid whose `edges()` is `None` → `SceneError::NonManifold` (**new variant**) naming the reason, and the wireframe falls back to the mesh edges with a note. Never a silent fallback.
- **Tests:** `a_cube_has_twelve_model_edges`. `the_mesh_edges_of_a_cube_include_its_diagonals`. `a_dihedral_test_removes_the_coplanar_diagonals`. `an_open_shell_names_itself`. `a_chamfer_adds_its_edges`.
- **Acceptance:** the two edge sources are separate types and the mode picks one.
- **Risk:** `None` from `edges()` is `BACKLOG`-level dependency on `KER-SRF-005` (sewing). Fallback: the mesh edges always exist, so the viewport never has nothing to draw; it says which it drew.

### REN-VIS-005 — Adaptive tessellation
- **State:** BLOCKED
- **Depends on:** KER-SRF-003, REN-VIS-002
- **Files:** `crates/render/src/scene3_tess.rs` (owner REN)
- **Contract:** `pub fn tessellate_at(solid: &Solid, chord_tol: f64) -> Result<Mesh, SolidError>;` and `pub fn chord_for_screen_error(px: f64, fov_degrees: f64, distance: f64, height_px: f64) -> f64;`
- **Behaviour:** `mesh_from_solid` takes **no density parameter** (`mesh.rs:54`) — it triangulates every face exactly, once. So the "tolerance" the rasteriser wants (`Options::tolerance`, `lib.rs:207`, about half a pixel at the current zoom) is not expressible today: the kernel's mesh is fixed and a far-away model is either too dense or too coarse with no dial. `chord_for_screen_error` computes the chord tolerance the current zoom wants; `tessellate_at` needs a kernel that can honour it, which is `KER-SRF-003` (analytic surfaces with a real tessellation control).
- **Refusals:** `chord_tol <= 0` → `SolidError::Invalid`. Until the kernel supports it, the viewport **uses the fixed mesh and says so in the status bar** rather than pretending to be adaptive.
- **Tests:** `the_screen_error_formula_matches_the_camera` — asserted against `world_per_pixel` (`viewport_state.rs:328`). `a_fixed_mesh_is_used_and_reported_until_the_kernel_can_refine`. `a_chord_tolerance_below_tolerance_for_is_refused`.
- **Acceptance:** the formula is tested; the capability is `BLOCKED` with an exact dependency.
- **Risk:** a fixed mesh at 1e6 triangles is refused by `scene3`'s own 1 M cap. Fallback: `REN-VIS-006`'s LOD and `REN-GPU-017`'s cache are the mitigations available without the kernel.

### REN-VIS-006 — The triangle budget and the LOD ladder
- **State:** READY
- **Depends on:** REN-VIS-002, REN-PERF-010
- **Files:** `crates/render/src/scene3_budget.rs` (owner REN)
- **Contract:** `pub struct TriBudget { pub max_tris: usize, pub levels: Vec<usize> }` with `pub fn budget_for(view_px: [u32; 2], triangles: usize, measured: &LodPoint) -> TriBudget;`
- **Behaviour:** Chooses a triangle ceiling from the **measured** frame budget (`REN-PERF-013`), not a constant: `max_tris` is the largest level whose measured frame time is inside the budget for this pixel count. When the scene exceeds it, the LOD ladder steps down and the status bar says "displayed at N/M triangles" — a visible degradation, never a silent one.
- **Refusals:** a scene whose **coarsest** level is still over budget → `SceneError::TooManyTriangles`, and the panel shows the refusal.
- **Tests:** `the_ceiling_follows_the_measured_ladder`. `an_over_budget_scene_steps_down_and_says_so`. `a_scene_over_budget_at_the_coarsest_level_is_refused`. `the_ladder_never_exceeds_max_viewport_tris`.
- **Acceptance:** the ceiling is a number read from `REN-PERF-013`'s table, and the test fails if it is hard-coded.
- **Risk:** the table is empty until `REN-PERF-013` runs. Fallback: a conservative constant with the table's absence stated in the code's doc.

### REN-VIS-007 — Visstyles, as named data
- **State:** READY
- **Depends on:** REN-VIS-003, REN-MAT-001
- **Files:** `crates/render/src/scene3_visstyle.rs` (owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct VisStyle { pub name: String, pub mode: DisplayMode, pub light: LightRig, pub material: Shading, pub background: Rgba, pub edges: EdgeStyle, pub xray_alpha: f64 }
  pub fn builtin_visstyles() -> Vec<VisStyle>;   // "Shaded with edges", "Hidden line" (refused, see REN-VIS-003), "X-Ray", "Monochrome", "Clay"
  ```
- **Behaviour:** A visstyle is a **named bundle**, so switching is one value and the user's choice persists in the document. Five built-in ones, our own definitions with their numbers in the source.
- **Refusals:** a `VisStyle` with a non-finite field → `SceneError::NonFinite` naming the field. A name longer than 120 characters → refused.
- **Tests:** `the_builtin_visstyles_validate`. `each_visstyle_names_its_numbers`. `a_visstyle_round_trips_through_the_document`. `an_invalid_visstyle_is_refused_by_name`.
- **Acceptance:** the list is data, asserted row by row.
- **Risk:** a visstyle that duplicates a visstyle. Fallback: `Clay` is the one that differs (a single material for every body) and the test asserts it does.

### REN-VIS-008 — Grid, axes and the world triad
- **State:** READY
- **Depends on:** REN-VIS-001
- **Files:** `crates/render/src/scene3_grid.rs` (owner REN)
- **Contract:** `pub fn adaptive_grid(spacing_mm: f64, world_per_pixel: f64) -> GridSpec;` with `pub struct GridSpec { pub spacing_mm: f64, pub extent_mm: f64, pub major_every: u32 }` — the grid switches spacing to the next power of ten when a line falls below `8 px`.
- **Behaviour:** Three workplanes by default, each a `Workplane` (`scene3.rs:450`) whose `grid_lines()` is already capped at `MAX_GRID_LINES = 512` per axis and **clips** the extent to what fits (`scene3.rs:512-518`). Adaptive spacing is what makes a grid legible at 10 mm and at 10 m with the same code. The axis triad at the origin is drawn as three short coloured segments.
- **Refusals:** a spacing below `tolerance_for(diagonal)` → `SceneError::BadWorkplane` (the existing validator). An extent past `MAX_GRID_LINES` → **clipped, and the clip is recorded** (`scene3.rs:117`), never a silent `half_lines()` clamp.
- **Tests:** `the_spacing_steps_by_powers_of_ten`. `a_line_never_falls_below_eight_pixels`. `the_grid_is_clipped_at_the_cap_and_says_so`. `the_triad_is_three_segments`. `the_three_workplanes_are_xy_xz_yz`.
- **Acceptance:** one test per grid fact, with the numbers.
- **Risk:** three workplanes × 2 × 512 lines = 3 072 segments drawn per frame by `Raster::line` (`scene3.rs:941`), each stepped per pixel. Fallback: the budget is in `REN-PERF-004`; if the grid dominates, it becomes an overlay drawn by egui instead of by the rasteriser.

### REN-VIS-009 — Transparency on the CPU path: what it does today, stated
- **State:** READY
- **Depends on:** REN-VIS-003
- **Files:** `crates/render/src/scene3.rs` (module doc; owner U in wave 1, REN after)
- **Contract:** a documentation change plus one named refusal: `pub fn blended_alpha() -> bool { false }` and a module-doc paragraph stating: alpha is written to the buffer and never blended, nothing is sorted, so a translucent body draws as its nearest triangle.
- **Behaviour:** A one-task honest statement. The `Material::with_alpha` doc already says "which this renderer writes but never blends" (`scene3.rs:190`), and the module doc says it twice (`scene3.rs:20-22`). This task makes it **reachable from the API**: a caller asking for blending gets a false, not a wrong picture.
- **Refusals:** `XRay` mode on the CPU path is refused with a message naming the GPU requirement — the mode exists (`REN-VIS-003`) and says it needs `REN-GPU-018`.
- **Tests:** `the_cpu_path_declares_no_blending`. `xray_on_the_cpu_path_is_refused_by_name`. `an_alpha_body_renders_with_its_last_triangle_winning` — pins today's behaviour so a future change is deliberate.
- **Acceptance:** a caller cannot ask for blending and get silence.
- **Risk:** pinning bad behaviour in a test. Fallback: the test's name says what it pins and the comment says it is a known limitation to be replaced by `REN-GPU-018`.

### REN-VIS-010 — Selection highlight in 3D
- **State:** BLOCKED
- **Depends on:** KER-MSH-003, REN-PICK-004
- **Files:** `crates/ui-egui/src/gpu3.rs::highlight` (owner U)
- **Contract:** `pub fn highlight_mesh(scene: &Scene, sel: &Selection3, width_px: f32) -> Result<Scene3Mesh, RenError>;` with `pub enum Selection3 { Faces(Vec<usize>), Edges(Vec<(usize, usize)>), Bodies(Vec<usize>), Vertices(Vec<usize>), None }`
- **Behaviour:** Reuses the 2D highlight's decision — `HIGHLIGHT_FILL_ALPHA = 0.5` and a dashed outline (`gpu.rs:246-292`, `:358`) — as a separate draw with the same numbers. A selected face is outlined **and** filled at 0.5 so the lines and text under it stay visible, which is what the 2D canvas does and why that constant exists.
- **Refusals:** a face index the solid does not have → `RenError::Invalid` naming the index and the count. A face index that no longer names the same face after a rebuild → `Ker`-level, refused until `KER-MSH-003`.
- **Tests:** `a_selected_face_is_outlined_and_filled_at_half_alpha`. `an_out_of_range_face_is_refused`. `a_body_selection_highlights_every_face`. `the_highlight_alpha_matches_the_2d_constant`.
- **Acceptance:** the alpha is the same constant as the 2D path, asserted.
- **Risk:** a stale index highlights the wrong face after an edit. Fallback: `KER-MSH-003` is what makes it correct; until then the panel shows the index and the user sees the wrong face, which is why it is BLOCKED.

### REN-VIS-011 — The view cube and the six standard views
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/ui-egui/src/viewcube.rs` (exists for 2D; **the 3D binding is new**, owner U), `crates/render/src/scene3_camera.rs` (owner REN)
- **Contract:** `pub fn standard_view(name: &str, bounds: &Bounds3, fov_degrees: f64) -> Result<OrbitView, SceneError>;` with `pub struct OrbitView { pub azimuth: f64, pub elevation: f64, pub distance: f64 }` and the six names `top, bottom, front, back, left, right`.
- **Behaviour:** Generated from the scene bounds and the document's unit, never stored (the DAT-066/067 rule). `OrbitCamera::home()` (`viewport_state.rs:313`) already exists and `frame(bounds)` (`:298`) already frames with `FRAME_MARGIN = 1.25`; this task supplies the six named orientations and the orthographic flags.
- **Refusals:** an unknown name → `SceneError::BadCamera` listing the six. A `top` view on a camera with `up = Z` is **degenerate** (the view direction is parallel to up) and is refused by name — the classic top-view gimbal problem, which `OrbitCamera::POLE_LIMIT` (`viewport_state.rs:322`) already limits.
- **Tests:** `each_of_the_six_views_faces_its_axis`. `the_top_view_is_at_the_pole_and_does_not_singular`. `an_unknown_view_name_lists_the_six`. `framing_uses_the_margin`.
- **Acceptance:** six tests, one per view.
- **Risk:** the top view singular at exactly 90°. Fallback: `POLE_LIMIT` is `π/2 − 1°`, and the test asserts the camera is still valid there.

### REN-VIS-012 — The 3D cursor at the work origin
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/render/src/scene3_cursor.rs` (owner REN)
- **Contract:** `pub struct Cursor3 { pub at: Vec3, pub size_mm: f64, pub axes: bool }` and `pub fn cursor_lines(c: &Cursor3, world_per_pixel: f64) -> Result<Vec<Segment>, SceneError>;`
- **Behaviour:** Three axis segments whose **world size is fixed in pixels** — `size_mm = world_per_pixel · 40` — so the cursor stays the same on-screen size at any zoom. `world_per_pixel(distance, fov, height)` already exists (`viewport_state.rs:328`), so the constant is the only thing being defined.
- **Refusals:** a non-finite position → `SceneError::NonFinite("cursor")`. A `size_mm <= 0` → refused. A `world_per_pixel` of 0 (camera on the target) → refused.
- **Tests:** `the_cursor_is_forty_pixels_at_any_zoom`. `the_axes_are_three_coloured_segments`. `a_non_finite_position_is_refused`. `a_zero_size_is_refused`.
- **Acceptance:** the pixel size is asserted at three zoom levels.
- **Risk:** a cursor at the exact camera target has no world size. Fallback: the panel shows the numeric coordinate, which needs no size.

### REN-VIS-013 — Measure in the viewport
- **State:** BLOCKED
- **Depends on:** DRW-001, REN-PICK-013
- **Files:** `crates/ui-egui/src/host3.rs::measure` (owner U + DRW)
- **Contract:** `pub fn measure(from: PickResult, to: PickResult) -> MeasureResult;` with `pub struct MeasureResult { pub distance: f64, pub delta: [f64; 3], pub text: String }` — the text formatted by `cadcraft_render::units::format_linear` (DAT-045).
- **Behaviour:** A two-click measurement between picked points, displayed in the document's unit. The measurement is **not** a drawing entity: it lives in the viewport and is not saved, because CADCraft has no persistent 3D annotation model.
- **Refusals:** two points that are not finite → refused. A measurement stored in the document → refused by name ("3D annotations are not saved; use a drawing sheet").
- **Tests:** `the_distance_is_euclidean`. `the_text_uses_the_document_unit`. `a_non_finite_pick_is_refused`. `a_measurement_is_not_persisted`.
- **Acceptance:** a measurement reads back the number the user typed, per DAT-045.
- **Risk:** this becomes a drawing feature by accident. Fallback: the non-persistence refusal is a test.

### REN-VIS-014 — The clipping box (visual half of `REN-SECT`)
- **State:** BACKLOG
- **Depends on:** REN-SECT-001
- **Files:** `crates/render/src/scene3_clip.rs` (owner REN)
- **Contract:** `pub struct ClipBox { pub min: Vec3, pub max: Vec3, pub enabled: bool }` and `pub fn clip_solid(solid: &Solid, box_: &ClipBox) -> Result<Solid, SolidError>;`
- **Behaviour:** A real boolean: `solid ∩ box_`. This is the honest implementation — the clipped result is a **solid**, so picking and sectioning work on it, and the "solid" the user is looking at is genuinely a different solid. It depends on `boolean::intersect` (`boolean.rs:61`), which is subject to the three open defects.
- **Refusals:** a box that does not intersect the solid → an **empty** result, and the panel says "nothing is inside the box" rather than showing a hole. A boolean failure → `CamError`/`SolidError` propagated with the kernel's message.
- **Tests:** `a_clip_box_removes_the_outside_material`. `an_empty_clip_box_is_a_message_not_a_hole`. `the_clipped_solid_picks_correctly`. `a_boolean_failure_is_propagated`.
- **Acceptance:** the clipped result is a `Solid`, not a display trick.
- **Risk:** the boolean defects bite here. Fallback: BLOCKED behind `KER-BOOL-001..003` in practice; a display-only clip that does not produce a solid is explicitly *not* this task.

### REN-VIS-015 — Frame pacing
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/ui-egui/src/host3.rs::pacing` (owner U)
- **Contract:** reuse `FrameCap` (`gpu.rs:65-92`): `pub fn pace(cap: &mut FrameCap, now: Instant) -> Duration;` — the viewport only re-renders when the camera, the model or the panel size changed, and never faster than the cap.
- **Behaviour:** The 2D canvas already has a frame cap with a test (`gpu.rs:817-829`: 250 → 4 ms). The 3D path needs the same: a viewport that re-rasterises on every egui repaint burns the frame budget on a picture that did not change. The rule is "redraw when `revision` changed **or** the camera moved", which is what `ViewportDelta::camera_changed` already reports (`viewport3.rs:84-88`).
- **Refusals:** a frame requested while one is in flight → the request is coalesced, not queued (a queue of camera moves is a frame-time disaster).
- **Tests:** `an_idle_frame_does_not_re_render`. `a_camera_move_does`. `a_coalesced_request_produces_one_frame`. `the_cap_matches_the_2d_cap_test`.
- **Acceptance:** idle costs zero renders, asserted by a counter.
- **Risk:** coalescing drops the last camera position. Fallback: the coalesced request keeps the **latest** camera, not the first.

### REN-VIS-016 — The one-frame lag, and camera interpolation
- **State:** READY
- **Depends on:** REN-VIS-015
- **Files:** `crates/ui-egui/src/host3.rs::interp` (owner U)
- **Contract:** `pub fn interpolate(prev: &Camera, next: &Camera, alpha: f64) -> Camera;` with `alpha ∈ 0..=1`, and the host renders at `prev → next` interpolated so the picture does not trail the cursor.
- **Behaviour:** `viewport3.rs:17-20` states the lag plainly: the picture is one frame behind, which is what every incremental renderer does. At 60 Hz that is 16.7 ms of visual lag on an orbit, which is perceptible. Interpolating the *rendered* camera between the last two input states makes the picture lead the cursor's smoothness without changing the input path.
- **Refusals:** an `alpha` outside `0..=1` or non-finite → refused. Interpolation **across a wrap** (azimuth past ±π) must take the short way round; the test asserts that.
- **Tests:** `alpha_zero_is_the_previous_camera`. `alpha_one_is_the_next`. `a_wrap_takes_the_short_way`. `a_non_finite_alpha_is_refused`. `the_rendered_camera_leads_the_cursor`.
- **Acceptance:** the lag is halved at most, measured, and never worse.
- **Risk:** interpolation can look wrong on a fast flick. Fallback: it is disabled above a stated camera angular velocity.

### REN-VIS-017 — The empty and error states
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/ui-egui/src/host3.rs::status` (owner U)
- **Contract:** `pub enum ViewStatus { Empty, Ok, Refused { reason: String }, Degraded { shown: usize, total: usize } }` and `pub fn status(model: Option<&Model>, last: Result<(), SceneError>, budget: &TriBudget) -> ViewStatus;`
- **Behaviour:** Four states, each with a message the user can act on: no document, fine, refused (with the kernel's own words), degraded (showing N of M triangles). The existing hint ("No solid to show", `viewport3.rs:147`) is the `Empty` case and stays.
- **Refusals:** a `SolidError` or `SceneError` becomes `Refused { reason }` and the panel shows the reason — never a blank rectangle and never a panic.
- **Tests:** `no_document_is_empty`. `a_tessellation_refusal_shows_the_kernel_message`. `a_degraded_scene_reports_both_counts`. `a_refused_state_is_not_an_empty_picture`.
- **Acceptance:** four tests, one per state.
- **Risk:** an error message in a panel is easy to miss. Fallback: it replaces the picture area, so it cannot be missed.

### REN-VIS-018 — Hover feedback
- **State:** READY
- **Depends on:** REN-PICK-002, REN-VIS-002
- **Files:** `crates/ui-egui/src/host3.rs::hover` (owner U)
- **Contract:** `pub fn hover(cam: &Camera, scene: &Scene, px: f64, py: f64, w: f64, h: f64) -> Option<Hit>;` with a **budget of ≤ 1 ms** per mouse-move, reusing `picking::pick_at` (`picking.rs:323`).
- **Behaviour:** A hover highlight on every mouse move is a pick per event, which is why the budget is half the click's. The 2D canvas has the same feature and the same cost discipline.
- **Refusals:** a pick over the face cap → `None`, not a slow answer.
- **Tests:** `a_hover_is_within_1_ms`. `the_hover_uses_the_same_answer_as_the_click`. `a_miss_returns_none_quickly`.
- **Acceptance:** measured and recorded; the value is a hover cost.
- **Risk:** mouse moves arrive faster than the budget. Fallback: one pick per frame, coalesced, like `REN-VIS-015`.

### REN-VIS-019 — Drawing the toolpath preview (cross-domain)
- **State:** BLOCKED
- **Depends on:** REN-EXT-002, CAM-PATH-001
- **Files:** `crates/render/src/scene3_overlay.rs` (owner REN)
- **Contract:** `pub fn overlay_lines(moves: &[[f64; 6]], kinds: &[u8], width_px: f32) -> Overlay3;` — 3D polylines coloured by cut kind (rapid, cut, lead-in), drawn with the same overlay pipeline as the cursor and the measure annotations.
- **Behaviour:** The renderer needs **3D line drawing**; today `scene3` draws only the `Workplane` grid segments (`Raster::line`, `scene3.rs:941`), one pixel wide and uncoloured per segment. This task generalises it: a coloured, width-carrying 3D polyline batch.
- **Refusals:** more than `MAX_OVERLAY_SEGMENTS = 2_000_000` → `SceneError::Limit` naming both. A non-finite endpoint → refused.
- **Tests:** `the_overlay_draws_in_the_camera_projection`. `each_cut_kind_has_its_own_colour`. `a_toolpath_of_a_million_segments_is_refused_or_paged`. `the_overlay_does_not_write_depth` — so it never occludes the model wrongly.
- **Acceptance:** a CAM toolpath is visible in the viewport, coloured by kind.
- **Risk:** a million-segment toolpath is a million line draws on the CPU path. Fallback: paged drawing with the count in the status bar, and the GPU path's triangle pipeline for wide overlays.

### REN-VIS-020 — Every new label in the translation table
- **State:** READY
- **Depends on:** REN-VIS-002
- **Files:** `crates/ui-egui/src/i18n/uk.tsv` (owner U), a test in `crates/ui-egui/tests/`
- **Contract:** a test asserting every string passed to `crate::tl!` in the 3D files appears as a key in `i18n/uk.tsv`.
- **Behaviour:** `master-plan.md` §5's `UI-005` requires every label in `uk.tsv`, and `viewport3.rs` already uses `tl!("No solid to show")`, `tl!("Home")`, `tl!("Fit")` (`:147`, `:222`, `:224`). Every new task in this file adds labels and this task is what keeps them honest — one test, run in CI, over a source scan of the 3D files.
- **Refusals:** a missing key is a test failure naming the string and the file.
- **Tests:** `every_3d_label_is_in_the_translation_table`. `no_3d_label_is_a_literal_string`. `the_table_has_no_unused_3d_keys`.
- **Acceptance:** green; a new literal in a 3D file fails CI.
- **Risk:** a source scan is brittle to formatting. Fallback: the scan looks for `tl!("…")` and reports what it found, so a false negative is visible.

### REN-VIS-021 — Releasing the viewport's memory
- **State:** READY
- **Depends on:** REN-GPU-002, REN-VIS-002
- **Files:** `crates/ui-egui/src/host3.rs::release` (owner U)
- **Contract:** `pub fn release(&mut self) { … }` dropping the CPU `Image`, the `Scene3Mesh` CPU arrays and the GPU textures when the panel closes, and `pub fn live_bytes(&self) -> usize;`
- **Behaviour:** egui keeps a texture in its memory until it is freed (`viewport3.rs:243-260`), and wgpu textures live until dropped. A user who closes the 3D dock and leaves it closed should not hold 40 MiB of colour and depth.
- **Refusals:** releasing while a frame is in flight → deferred to the next frame boundary, never freed mid-encode.
- **Tests:** `releasing_frees_the_cpu_image`. `releasing_frees_the_gpu_textures`. `a_release_during_a_frame_is_deferred`. `reopening_rebuilds_without_a_leak` — the counter is back where it started.
- **Acceptance:** `live_bytes()` returns to its pre-open value after close.
- **Risk:** a use-after-free by dropping a texture a submitted command still reads. Fallback: wgpu's refcounting handles it; the deferral is belt and braces.

### REN-VIS-022 — Viewport property tests and hostile scenes
- **State:** READY
- **Depends on:** REN-VIS-017
- **Files:** `crates/ui-egui/tests/viewport3_properties.rs` (owner Q)
- **Contract:** `no_scene_ever_panics_the_host`; `every_refusal_becomes_a_status_not_a_blank`; `a_solid_at_1e6_renders`; `an_empty_solid_renders_a_status`; `a_degenerate_camera_is_refused`; `a_hostile_scene_never_panics`.
- **Behaviour:** Over random solids and random cameras: every outcome is a `ViewStatus`, never a panic and never a hang. Includes the cases the kernel can produce: an empty `Solid::default()` (picking.rs:519 already pins its refusal), a solid with a NaN vertex, a camera on the target, a panel of zero size.
- **Refusals:** all of them, each to a status.
- **Tests:** the six above.
- **Acceptance:** green; this is the `never crash` standard applied to the 3D surface.
- **Risk:** a random solid the kernel refuses to tessellate. Fallback: that is a `Refused` status, which the test asserts.

---

# REN-PICK — picking, selection, snapping (16 tasks)

### REN-PICK-001 — Declare `picking` and run its 18 tests
- **State:** READY
- **Depends on:** —
- **Files:** `crates/render/src/lib.rs` (`pub mod picking;` + re-exports; owner REN)
- **Contract:** `pub mod picking;` plus `pub use picking::{Hit, Ray, pick_at, pick_scene, pick_ray, ray_faces_intersection, ray_mesh_intersection, ray_tessellated_intersection, ray_through_pixel, view_tangent, bounds_diagonal, PARALLEL_EPS};`
- **Behaviour:** The module exists (`crates/render/src/picking.rs`, 647 lines, 18 tests) and **is not in the build**. This task declares it and runs its tests for the first time. Part of `REN-VIS-001`, separated because it is a different owner's file (`crates/render`, not `crates/ui-egui`).
- **Refusals:** a compile error in a file that has never been compiled is a report, not an edit to another module.
- **Tests:** `every_pick_entry_point_is_reachable`. `the_pick_module_has_its_documented_refusals`.
- **Acceptance:** `cargo xtask ci` green with 18 more tests running.
- **Risk:** the module was written against a `scene3` API that has since changed. Fallback: the task's output is the list of compile errors, which is the most useful thing it can produce.

### REN-PICK-002 — The acceleration structure, and why
- **State:** BLOCKED
- **Depends on:** KER-MSH-001, REN-PICK-001
- **Files:** `crates/render/src/scene3_bvh.rs` (owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default)]
  pub struct Bvh { pub nodes: Vec<BvhNode>, pub prims: Vec<u32> }
  #[derive(Clone, Copy, Debug)]
  pub struct BvhNode { pub aabb: Bounds3, pub left: u32, pub count: u32 }  // count == 0 → interior
  pub fn build_bvh(prims: &[(Vec3, u32)], leaf_size: usize) -> Result<Bvh, SceneError>;
  pub fn nearest_hit(bvh: &Bvh, prims: &[(Vec3, u32)], ray: &Ray) -> Option<(f64, u32)>;
  ```
- **Behaviour:** `ray_faces_intersection` tests every face (`picking.rs:142`) — at `limits::MAX_FACES = 60 000` that is 60 000 plane tests per click, which is why `REN-PERF-008` sets a 2 ms budget. A BVH over **faces** (each face's `Bounds3` plus its index) prunes by AABB, and it is **exact**: the traversal only skips faces whose AABB the ray cannot meet, and the surviving faces go through the *same* plane-crossing test, so the answer is bit-identical to the brute-force one. Triangles get their own BVH for the mesh path.
- **Refusals:** a BVH over more than `MAX_BVH_PRIMS = 4_000_000` → `SceneError::Limit`. A primitive with a non-finite AABB → `SceneError::NonFinite` naming the index. A `leaf_size == 0` → refused.
- **Tests:** `the_accelerated_pick_equals_the_brute_force_pick` — over 200 random rays and 50 random solids, asserting the **same** face and distance to `1e-12`. `a_bvh_over_an_empty_primitive_list_is_empty`. `a_non_finite_aabb_is_refused`. `the_build_is_deterministic`.
- **Acceptance:** the equality test is the acceptance; acceleration that changes an answer is not acceleration.
- **Risk:** a face whose AABB is enormous (a face spanning the whole model) prunes nothing. Fallback: split a primitive whose AABB exceeds a stated fraction of the scene's diagonal — or accept that one face costs one test.

### REN-PICK-003 — Keeping the tie rule
- **State:** READY
- **Depends on:** REN-PICK-002
- **Files:** `crates/render/src/scene3_bvh.rs::order` (owner REN)
- **Contract:** the same `Hit` and the same rule: a tie keeps the **lower face index** (`picking.rs:167`), and `pick_scene` keeps the **earlier body** in `Scene::solids` on a strict `<` (`picking.rs:284`).
- **Behaviour:** A BVH traversal visits nodes in an order that has nothing to do with face indices, so the "first one wins" rule must be re-established explicitly: collect the nearest distance, then among all hits within `tolerance_for(diagonal)` of it take the lowest index. The property is not cosmetic — `the_nearest_face_wins_from_outside` (`picking.rs:417`) is a test today, and it must keep passing.
- **Refusals:** two hits within tolerance but not within `1e-12` → the lower index wins; the test asserts it on a ray down a shared edge.
- **Tests:** `a_ray_down_a_shared_edge_picks_the_lower_face_index`. `the_accelerated_tie_rule_matches_the_brute_force_one`. `a_body_tie_picks_the_earlier_body`.
- **Acceptance:** the existing three tie tests pass unchanged with the BVH in place.
- **Risk:** changing traversal order changes answers. Fallback: the acceptance is "the old tests pass", which is exactly the guard.

### REN-PICK-004 — `Selection3`, and what a selection can be
- **State:** READY
- **Depends on:** REN-VIS-001
- **Files:** `crates/ui-egui/src/viewport_state.rs::Selection` (owner U)
- **Contract:** the existing `Selection` (`viewport_state.rs:459`) grows from its current shape to `pub enum Selection { None, Body(usize), Face(usize), Edge(usize), Vertex(usize), Box { faces: Vec<usize> } }`.
- **Behaviour:** The state lives in `viewport_state.rs` because it has no egui in it and is unit-testable; the panel renders it. A box selection is a **face index list**, not an AABB, so it survives until the topology changes (which `REN-VIS-010`'s `KER-MSH-003` dependency acknowledges).
- **Refusals:** clearing a selection of a body that no longer exists → `None`, not a stale index.
- **Tests:** `a_selection_round_trips`. `a_box_selection_is_a_face_list`. `selecting_a_missing_body_clears_it`. `a_selection_does_not_own_a_model`.
- **Acceptance:** the type is in `viewport_state` and has no egui dependency.
- **Risk:** growing a public enum breaks matches elsewhere. Fallback: it is currently constructed only in tests (`viewport_state.rs:510`).

### REN-PICK-005 — Edge and vertex snapping
- **State:** BACKLOG
- **Depends on:** KER-MSH-003, REN-PICK-002
- **Files:** `crates/render/src/scene3_snap.rs` (owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Snap { None, Vertex(Vec3), Edge(Vec3, Vec3), Face(usize), Intersection(Vec3) }
  pub fn snap(solid: &Solid, cam: &Camera, px: f64, py: f64, w: f64, h: f64, radius_px: f64) -> Result<Option<Snap>, SceneError>;
  ```
- **Behaviour:** Finds the nearest **vertex, then edge, then face** within `radius_px` **in screen space**, using the same projection `picking.rs:299` uses so the snap marker lands on the cursor. The world tolerance is derived from the pixel radius: `radius_world = radius_px · world_per_pixel` (`viewport_state.rs:328`), so a snap is as sticky at 10 m as at 100 mm. The preference order is the CAD convention and is in the doc.
- **Refusals:** a non-finite pixel → `Ok(None)`, never a snap at a NaN. A solid with no vertices → `Ok(None)`. A `radius_px <= 0` → `SceneError::BadCamera`.
- **Tests:** `a_pixel_near_a_vertex_snaps_to_it`. `a_pixel_near_an_edge_snaps_to_the_edge`. `the_vertex_preference_beats_the_edge`. `a_pixel_far_from_everything_returns_none`. `the_world_radius_scales_with_the_zoom`.
- **Acceptance:** four snaps at four zoom levels, with the same pixel radius snapping to the same feature.
- **Risk:** an edge snap needs the edge set, and `Solid::edges()` returns `None` on an open shell (`REN-VIS-004`). Fallback: the mesh edges are used when the model edges are unavailable, and the snap says which it snapped to.

### REN-PICK-006 — Pixel tolerance: a click near an edge picks the edge
- **State:** READY
- **Depends on:** REN-PICK-002
- **Files:** `crates/render/src/scene3_snap.rs::nearest` (owner REN)
- **Contract:** `pub fn pick_within(ray: &Ray, solid: &Solid, bvh: &Bvh, tol_world: f64) -> Result<Option<Hit>, SceneError>;`
- **Behaviour:** The standard CAD behaviour and the reason clicking a 1 px edge is possible at all: if the exact ray misses but comes within `tol_world` of an edge, return that edge's nearest point. `tol_world = 4 px · world_per_pixel`, which is a **derived** number, not a constant.
- **Refusals:** a `tol_world <= 0` → `SceneError::BadCamera`. Two edges within tolerance → the lower index (the tie rule again).
- **Tests:** `a_click_one_pixel_from_an_edge_picks_the_edge`. `a_click_far_from_everything_misses`. `the_tolerance_is_four_pixels_in_world_units`. `the_tie_rule_still_holds`.
- **Acceptance:** a 1 px edge is selectable at any zoom.
- **Risk:** a pick with a tolerance picks something the user did not click on. Fallback: the tolerance is small (4 px) and the returned `Hit::distance` is the exact distance to the edge, not to the ray.

### REN-PICK-007 — Picking a screen-space rectangle
- **State:** READY
- **Depends on:** REN-PICK-002, REN-PICK-006
- **Files:** `crates/render/src/scene3_pickbox.rs` (owner REN)
- **Contract:** `pub fn pick_box(solid: &Solid, cam: &Camera, a: [f64; 2], b: [f64; 2], crossing: bool) -> Result<Vec<usize>, SceneError>;`
- **Behaviour:** Window and crossing selection, decided in **screen space**: window = a face whose projected bounding box is entirely inside the rectangle; crossing = any overlap. Both are the 2D CAD rules applied to projected faces, which is why the projection is `scene3`'s (one projection, invariant 3).
- **Refusals:** a rectangle with zero area → `Err` naming both corners. A non-finite corner → refused.
- **Tests:** `a_window_selection_takes_the_fully_contained_faces`. `a_crossing_selection_takes_the_overlapping_ones`. `a_back_facing_face_is_excluded_from_both`. `a_zero_area_rectangle_is_refused`.
- **Acceptance:** both rules, tested on a box from two angles.
- **Risk:** a face's projected AABB over-selects for a strongly perspective view. Fallback: documented; the crossing rule is the standard approximation every CAD uses.

### REN-PICK-008 — GPU picking, and its agreement with the CPU
- **State:** BLOCKED
- **Depends on:** REN-GPU-020, REN-PICK-002
- **Files:** `crates/ui-egui/src/gpu3.rs::pick` (owner U)
- **Contract:** `pub fn pick_gpu(target: &IdTarget, px: u32, py: u32) -> Option<usize>;`
- **Behaviour:** One pixel of the id texture gives the body; the face still comes from `picking::face_at` (`picking.rs:261`, O(faces)) or the tracked mesh (`KER-MSH-001`). The id pass runs **only on a click**, never per frame.
- **Refusals:** a readback not ready → `None`. An id of 0 → `None`. A body index outside `Scene::solids` → `None` and a fault recorded.
- **Tests:** `the_gpu_pick_agrees_with_the_cpu_pick` — 100 random clicks over 5 scenes. `a_click_on_the_background_picks_nothing`. `an_instanced_body_picks_its_instance`.
- **Acceptance:** the agreement test, which is the whole point.
- **Risk:** the two disagree where the CPU is exact and the GPU is rasterised. Fallback: the CPU path stays the default and the GPU path is opt-in behind a setting, with the disagreement counted.

### REN-PICK-009 — Picking on a sectioned model
- **State:** BLOCKED
- **Depends on:** REN-SECT-002, REN-PICK-002
- **Files:** `crates/render/src/scene3_pick.rs::sectioned` (owner REN)
- **Contract:** `pub fn pick_sectioned(ray: &Ray, solid: &Solid, sec: &SectionPlane) -> Result<Option<Hit>, SceneError>;`
- **Behaviour:** When a section plane is enabled, a pick must not hit geometry the user cannot see. Two rules, both stated: a hit on the far side of the clip plane is **not** a hit, and a hit on the **capping** face is a hit on the model face whose cross-section produced it.
- **Refusals:** a hit whose distance is beyond the clip plane → `None`, not a shortened hit. A cap pick with no source face → `Hit { tessellated: true }` with the face index 0 and a warning, never a fabricated face.
- **Tests:** `a_ray_past_the_clip_plane_picks_nothing`. `a_ray_through_the_cap_picks_the_source_face`. `a_ray_through_the_void_picks_nothing`. `sectioned_picking_agrees_with_the_unsectioned_one_away_from_the_plane`.
- **Acceptance:** the clip and the pick agree, tested on a box and a holed plate.
- **Risk:** the cap is a drawn polygon, not a face, so the pick's `face` index is a fiction. Fallback: the `Hit` carries `tessellated: true` and the UI says "cross-section", not "face 7".

### REN-PICK-010 — The hole rule, pinned on both paths
- **State:** READY
- **Depends on:** REN-PICK-001
- **Files:** `crates/render/tests/picking_holes.rs` (owner Q)
- **Contract:** `a_ray_down_a_hole_hits_nothing_on_the_exact_path`; `the_mesh_path_reports_the_hole_as_surface_and_that_is_documented`; `the_gpu_path_agrees_with_the_exact_path`; `a_ray_beside_the_hole_hits`.
- **Behaviour:** `picking.rs:18-25` documents this precisely: a region with a hole is triangulated as its outer loop plus **reversed** triangles over the hole, so a mesh hit test that ignores winding reports the hole as surface. `a_hole_is_not_a_surface_and_a_face_around_it_is` (`picking.rs:525`) pins the exact path. This task extends the pin to the mesh and GPU paths and **turns the documented difference into a named test**.
- **Refusals:** none — both behaviours are correct for their own path, and the tests say which is which.
- **Tests:** the four above.
- **Acceptance:** the difference is a test, so a change to either path is deliberate.
- **Risk:** someone "fixes" the mesh path and breaks the parity with the rasteriser (which draws the reversed triangles as surface too, since it culls by winding... which it does *not* for a hole). Fallback: the comment in `mesh.rs:78-85` explains the winding subtraction is for area, and the test names both truths.

### REN-PICK-011 — A selection that survives a rebuild
- **State:** BLOCKED
- **Depends on:** KER-MSH-003
- **Files:** `crates/ui-egui/src/host3.rs::restore` (owner U)
- **Contract:** `pub fn restore_selection(sel: &Selection3, solid: &Solid, before: &Solid) -> Result<Selection3, RenError>;`
- **Behaviour:** After a rebuild, a selection stored as a face **index** points at whatever is now at that index. With `KER-MSH-003` the selection is stored as a `FaceId` and re-resolved; without it, the best that can be done is to **refuse**: "the model changed; the selection was cleared", with the count of what was lost.
- **Refusals:** an unresolvable selection after a rebuild → `Selection3::None` plus a message naming how many were dropped. Never a silently re-targeted selection — that is `master-plan.md` §4's "None" costing real work.
- **Tests:** `a_selection_survives_an_edit_that_keeps_the_topology`. `a_selection_is_cleared_and_reported_when_the_topology_changes`. `nothing_is_silently_retargeted`.
- **Acceptance:** green with the refusal, and the task is marked as waiting on `KER-MSH-003` for the good behaviour.
- **Risk:** this is the single most valuable UX fix in the file and it is not ours to write. Fallback: the honest refusal, which is already better than a wrong highlight.

### REN-PICK-012 — The pick result the UI receives
- **State:** READY
- **Depends on:** REN-PICK-004
- **Files:** `crates/ui-egui/src/viewport_state.rs::PickResult` (owner U)
- **Contract:** `pub struct PickResult { pub body: usize, pub hit: Hit, pub screen: [f64; 2], pub miss: bool }` — carrying the screen position so the UI can draw the snap marker at the right place, and `miss` so "nothing" is an answer rather than an absence.
- **Behaviour:** `viewport3.rs` queues a `PickRequest` and the host resolves it (`viewport3.rs:26-28`); this is the resolved form. It is in `viewport_state` so the whole pick conversation is testable without a window.
- **Refusals:** a `PickResult` with a non-finite `hit` → refused (the existing `Hit::is_finite`, `picking.rs:129`, is the check).
- **Tests:** `a_miss_is_a_result_with_miss_true`. `a_hit_carries_its_screen_position`. `a_non_finite_hit_is_refused`. `the_pick_result_has_no_egui_type`.
- **Acceptance:** a test in `viewport_state` with no harness proves the whole path from request to result.
- **Risk:** none; this is a data type.

### REN-PICK-013 — Snap to the surface under the cursor
- **State:** BLOCKED
- **Depends on:** KER-MSH-006
- **Files:** `crates/render/src/scene3_snap.rs::surface` (owner REN)
- **Contract:** `pub fn snap_to_surface(solid: &Solid, p: Vec3, radius: f64) -> Result<Option<Vec3>, SolidError>;`
- **Behaviour:** Given a point, the nearest point on the shell. Needs `KER-MSH-006`, because `Plane::distance` is per-face and there is no distance-to-shell query; a brute-force nearest-face loop is `O(faces)` and would run on every mouse move.
- **Refusals:** a non-finite point → `SolidError::NonFinite`. A radius ≤ 0 → refused. A point farther than the radius from the shell → `Ok(None)`.
- **Tests:** `a_point_above_a_face_snaps_to_it`. `a_point_far_from_the_shell_returns_none`. `the_radius_is_in_world_units`. `a_non_finite_point_is_refused`.
- **Acceptance:** the four tests, once the kernel contract exists.
- **Risk:** a brute-force version is possible but is O(faces) per mouse move. Fallback: with the BVH from `REN-PICK-002` the brute force becomes a nearest-AABB descent, which is a legitimate interim — and is named as one.

### REN-PICK-014 — The pick budget, enforced
- **State:** BLOCKED
- **Depends on:** REN-PERF-008, REN-PICK-002
- **Files:** `crates/render/tests/perf_pick.rs` (owner Q)
- **Contract:** **≤ 2 ms** at 60 000 faces; **≤ 0.5 ms** at 1 000 faces; **≤ 8 ms** for a 200-face box selection.
- **Behaviour:** The three cells are measured and recorded; a regression over 10 % fails (`REN-PERF-002`). The box selection is slower and that is expected and written down rather than discovered.
- **Refusals:** a pick over the face cap is refused, not run slowly.
- **Tests:** `the_three_pick_cells_meet_their_budgets`. `a_pick_over_the_cap_is_refused`.
- **Acceptance:** three numbers in the baseline file.
- **Risk:** a shared CI machine cannot assert times. Fallback: `REN-PERF-018` keeps them `#[ignore]`d except on the reference machine.

### REN-PICK-015 — A hidden body is not pickable
- **State:** BLOCKED
- **Depends on:** REN-EXT-001, REN-VIS-002
- **Files:** `crates/ui-egui/src/host3.rs::visible` (owner U)
- **Contract:** `pub fn pickable(body: &str, vis: &BodyVisibility) -> bool;`
- **Behaviour:** A body the user hid is not in the pick set at all. **CADCraft 3D has no visibility concept**: `Model { parameters, features, records }` (`master-plan.md` §4) has no per-body flag, which is why this is `BLOCKED` on `REN-EXT-001`'s exact signature.
- **Refusals:** a body with no visibility entry → **visible** (the default, so nothing disappears on an upgrade), and the panel says which default was used.
- **Tests:** `a_hidden_body_is_not_pickable`. `a_body_with_no_entry_is_visible_by_default`. `a_ghosted_body_is_pickable_but_rendered_translucent`.
- **Acceptance:** once `REN-EXT-001` lands.
- **Risk:** the default hides nothing, so a missing concept cannot lose a click. Fallback: none needed; the default is the safe direction.

### REN-PICK-016 — Picking an annotation is not picking the model
- **State:** READY
- **Depends on:** REN-PICK-012
- **Files:** `crates/ui-egui/src/host3.rs::annotations` (owner U)
- **Contract:** `pub enum PickKind { Model(Hit), Annotation(usize), SnapMarker, Nothing }` and `pub fn classify_pick(hit: Option<Hit>, annotations: &[Annotation3], req: &PickRequest) -> PickKind;`
- **Behaviour:** The cursor, the measure annotations and the snap markers are **drawn in front of** the model, so a click on one must not select the face behind it. Classification is explicit, in that order, and `Model` is last.
- **Refusals:** an annotation with a non-finite anchor → `PickKind::Nothing` and a warning, never a model pick through it.
- **Tests:** `a_click_on_a_measure_annotation_picks_the_annotation`. `a_click_on_the_model_picks_the_model`. `a_click_on_a_snap_marker_picks_nothing_but_the_marker`. `a_broken_annotation_does_not_fall_through_to_the_model`.
- **Acceptance:** three tests, one per kind, plus the malformed case.
- **Risk:** an invisible annotation swallowing clicks. Fallback: an annotation only claims a click within 4 px of its anchor, the same rule as `REN-PICK-006`.

---

# REN-SECT — section planes and the cut view (14 tasks)

### REN-SECT-001 — The section plane, as data
- **State:** READY
- **Depends on:** —
- **Files:** `crates/render/src/scene3_section.rs` (owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct SectionPlane { pub plane: Plane, pub enabled: bool, pub cap: bool, pub keep: Half }
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Half { Negative, Positive, Both }
  ```
- **Behaviour:** Pure data in `render`: a `scene3::Plane` (`scene3.rs:34-42` has the plane algebra; `solid::Plane` is the kernel's), a flag, whether to cap, and which half to keep. No geometry, no GPU type.
- **Refusals:** a plane whose normal is not unit after `Plane::new` → `SceneError::BadWorkplane` (the existing validator, `scene3.rs:476-503`).
- **Tests:** `a_section_plane_validates`. `a_non_unit_normal_is_refused`. `keep_both_is_the_default`. `the_section_round_trips_through_the_document`.
- **Acceptance:** the type exists in `render` and is testable without a GPU.
- **Risk:** two plane types (the kernel's and `scene3`'s). Fallback: `scene3::Plane` is the *same struct* re-exported (`lib.rs:42` exports `Workplane`, and the crate uses `cadcraft_geom` types), so there is one plane, not two.

### REN-SECT-002 — Clipping the triangles for display
- **State:** READY
- **Depends on:** REN-SECT-001
- **Files:** `crates/render/src/scene3_section.rs::clip` (owner REN)
- **Contract:** `pub fn clip_triangles(mesh: &Mesh, sec: &SectionPlane, tol: f64) -> Result<ClippedMesh, SceneError>;` with `pub struct ClippedMesh { pub verts: Vec<Vec3>, pub tris: Vec<[usize; 3]>, pub cut_edges: Vec<(u32, u32)> }`
- **Behaviour:** Clips the **tessellation** against the plane (each triangle against one plane gives a triangle or a quad, fanned back into two triangles — the same shape as `scene3`'s near clip, `scene3.rs:1002-1027`) and reports the `cut_edges`, which is the loop the cap is built from.
- **Refusals:** `tol <= 0` → `SceneError::BadCamera`. More triangles than `MAX_VIEWPORT_TRIS` after clipping → `SceneError::TooManyTriangles`.
- **Tests:** `a_triangle_crossing_the_plane_becomes_two`. `a_triangle_beside_the_plane_is_kept_or_dropped_per_keep`. `the_cut_edges_form_a_closed_loop_on_a_box`. `clipping_is_deterministic`.
- **Acceptance:** the cut-edge loop is closed for a convex body, asserted.
- **Risk:** a tessellation's diagonal across the hole means the cut loop may have a jog. Fallback: the cap is triangulated by the kernel's own `triangulate_region` from the loop, which handles the jog.

### REN-SECT-003 — Capping: a drawing, and it says so
- **State:** READY
- **Depends on:** REN-SECT-002
- **Files:** `crates/render/src/scene3_section.rs::cap` (owner REN)
- **Contract:** `pub fn cap_polygon(cut_edges: &[(u32, u32)], plane: &Plane, outline: &[(f64; 4)]) -> Result<Vec<[f64; 3]>, SceneError>;` with `outline` = (r, g, b, hatch spacing).
- **Behaviour:** Triangulates the cut loop with `cadcraft_solid::triangulate_region` (the kernel's ear clipper, `mesh.rs:86`) and fills it with a **flat** colour, optionally with a hatch at a stated spacing in world units. **The cap is a drawing, not geometry:** it has no face index, no material and no picking identity beyond "cross-section". The doc says this in its first paragraph.
- **Refusals:** a cut loop that does not close → `SceneError::BadWorkplane("the cross-section is not a closed loop")`. A hatch spacing ≤ 0 → refused. More cap triangles than `MAX_VIEWPORT_TRIS / 4` → `SceneError::Limit`.
- **Tests:** `a_capped_section_of_a_box_is_a_square`. `the_cap_is_flat_shaded`. `a_non_closed_cut_loop_is_refused`. `the_hatch_spacing_is_in_world_units`. `the_cap_has_no_face_identity` — asserted by the type.
- **Acceptance:** the cap draws and says it is a cap.
- **Risk:** a user reads the cap as a face and tries to select it. Fallback: `REN-PICK-009` answers with `tessellated: true` and the UI says "cross-section".

### REN-SECT-004 — Moving the section plane in the viewport
- **State:** READY
- **Depends on:** REN-SECT-001, REN-VIS-012
- **Files:** `crates/ui-egui/src/host3.rs::section_drag` (owner U)
- **Contract:** `pub fn drag_section(sec: &mut SectionPlane, drag: DragState, cam: &Camera) -> Result<(), RenError>;` with the plane drawn as a translucent square outline plus a normal arrow.
- **Behaviour:** Three gestures: drag along the normal (the offset, clamped to the scene bounds), rotate (about the plane's own `u` axis), and pick a face to make the plane normal to it. The offset is clamped so the plane cannot leave the bounds, because a plane outside the body draws nothing and looks broken.
- **Refusals:** a drag on a disabled plane → refused. A rotation that makes the normal non-finite → refused. A picked face whose plane is not finite → refused.
- **Tests:** `dragging_along_the_normal_moves_the_plane_only_along_it`. `a_plane_outside_the_bounds_is_clamped`. `picking_a_face_makes_the_plane_normal_to_it`. `a_drag_on_a_disabled_plane_is_refused`.
- **Acceptance:** three gestures, four tests.
- **Risk:** the drag plane must be perpendicular to the camera to feel right. Fallback: the offset uses the normal's screen projection; a normal nearly parallel to the view is refused, which is the standard CAD behaviour.

### REN-SECT-005 — Picking a sectioned model
- **State:** BLOCKED
- **Depends on:** REN-PICK-009
- **Files:** `crates/render/src/scene3_pick.rs` (owner REN)
- **Contract:** as `REN-PICK-009`.
- **Behaviour:** The task is the pick side of sectioning; the contract lives with the pick that consumes it, and this entry exists so the section epic's dependencies are visible in one place.
- **Refusals:** as `REN-PICK-009`.
- **Tests:** `a_ray_past_the_clip_picks_nothing`. `a_cap_pick_says_cross_section`.
- **Acceptance:** `REN-PICK-009`'s tests.
- **Risk:** duplicated in two epics. Fallback: this entry is a pointer, and only one file changes.

### REN-SECT-006 — Sectioning never modifies the model
- **State:** READY
- **Depends on:** REN-SECT-002
- **Files:** `crates/render/src/scene3_section.rs` (doc + a test; owner REN)
- **Contract:** `pub fn section_is_view_only(sec: &SectionPlane, solid: &Solid) -> Result<Solid, SolidError>;` — returns the **original** solid, always, and exists so the claim is a function.
- **Behaviour:** A section is a **view**. `REN-VIS-014`'s clip box is the *other* thing, and it does produce a different solid. The distinction is the confusing part, so it is made into a function that returns the input.
- **Refusals:** any code path that mutates the model from a section → refused by the type: `SectionPlane` holds no model reference.
- **Tests:** `a_section_never_changes_the_model`. `the_volume_is_unchanged_after_enabling_a_section`. `a_clip_box_does_change_the_solid` — the contrast, asserted.
- **Acceptance:** the volume is identical before and after, asserted.
- **Risk:** a user cuts the model expecting it to be modified. Fallback: the UI's section control is in the *view* menu, and the doc says so.

### REN-SECT-007 — Multiple clipping planes, and the six-plane limit
- **State:** READY
- **Depends on:** REN-SECT-002
- **Files:** `crates/render/src/scene3_clipplanes.rs` (owner REN)
- **Contract:** `pub const MAX_CLIP_PLANES: usize = 6;` and `pub fn clip_all(mesh: &Mesh, planes: &[SectionPlane]) -> Result<ClippedMesh, SceneError>;`
- **Behaviour:** Up to **six** planes, which is the WebGL and WebGL2 guaranteed minimum and therefore the number that works everywhere; the constant says so. Sequential clipping, one plane at a time, each re-triangulating what the last left.
- **Refusals:** a seventh plane → `SceneError::Limit { what: "clip planes", count: 7, max: 6 }`, not a silent ignore.
- **Tests:** `six_planes_clip`. `a_seventh_plane_is_refused_with_both_numbers`. `clipping_by_two_planes_is_the_same_as_clipping_twice`. `clipping_is_deterministic`.
- **Acceptance:** the limit is named and its reason is in the doc.
- **Risk:** sequential clipping accumulates error. Fallback: each clip is exact (a plane against a triangle), and the test compares two orders of the same two planes on a box.

### REN-SECT-008 — The section curve from the kernel, not from the mesh
- **State:** BLOCKED
- **Depends on:** KER-MSH-005
- **Files:** `crates/render/src/scene3_section.rs::kernel_curve` (owner REN)
- **Contract:** `pub fn kernel_section(solid: &Solid, sec: &SectionPlane, tol: f64) -> Result<Regions, SolidError>;` — a thin wrapper over `cadcraft_solid::section`, which **is not re-exported** (`lib.rs:93`).
- **Behaviour:** The cut loop drawn on the section plane comes from the **model's own** section (`section.rs:50`), not from the tessellation's cut edges, so the curve is exact where the mesh's is faceted. This is the one place in REN that is `BLOCKED` on a **one-line** kernel change, and it is the cheapest unblock in the whole programme (it also unblocks five SIM and six CAM tasks).
- **Refusals:** a section that cannot be closed → `SolidError` propagated, never an empty cap.
- **Tests:** `the_kernel_curve_is_exact_where_the_mesh_is_faceted`. `a_plane_through_a_vertex_gives_a_closed_loop`. `an_open_section_is_the_kernels_refusal`.
- **Acceptance:** the curve is drawn from the kernel, and the test proves the difference on a cylinder.
- **Risk:** the re-export is agent A's file. Fallback: the mesh path stays and says which it drew.

### REN-SECT-009 — A section box from a saved view
- **State:** BACKLOG
- **Depends on:** DAT-066, REN-SECT-007
- **Files:** `crates/render/src/scene3_clipplanes.rs::from_view` (owner REN + D)
- **Contract:** `pub fn planes_from_view(v: &cadcraft_model::SavedView, margin: f64) -> [SectionPlane; 6];` — the six faces of a view's bounding box, each enabled, which is the "cut away everything in front of the standard views" trick.
- **Behaviour:** Turns a saved view (`DAT-066`'s `SavedView`, whose fields are `OrbitCamera`'s own) into six clipping planes inflated by `margin`. `DAT-066` stores views; this consumes them.
- **Refusals:** a `SavedView` with a non-finite field → refused (DAT-066's rule). A `margin < 0` → refused.
- **Tests:** `a_front_view_clips_to_its_front_quadrant`. `a_named_view_clips_the_same_way_twice`. `a_non_finite_view_is_refused`.
- **Acceptance:** the same view produces the same six planes, asserted.
- **Risk:** `SavedView` is not written yet (`crates/model/src/view.rs` does not exist). Fallback: the function takes the six floats it needs, and the `SavedView` version is a one-line constructor when it lands.

### REN-SECT-010 — Sections on a drawing sheet
- **State:** BLOCKED
- **Depends on:** DRW-001, REN-SECT-008
- **Files:** `crates/drawing/src/section.rs` (new; owner DRW — **not REN's file**)
- **Contract:** `pub fn section_viewport(solid: &Solid, plane: &Plane, tol: f64) -> Result<SectionView, SolidError>;` with `SectionView { pub outline: Vec<Vec3>, pub hatch: Vec<Vec3>, pub label: String }`
- **Behaviour:** The 3D section becomes a **2D drawing view** — the outline plus hatching on the cut face. The renderer supplies the clipped geometry; the drawing crate lays it out. Cross-domain and blocked on `DRW-001`.
- **Refusals:** a section that cannot be closed → refused. A view with no scale → refused.
- **Tests:** cross-ref: the outline is closed and the hatch spacing is a drawing standard (1.5 mm at 1:1, per `crates/doc`'s hatch rules) — **written in DRW's acceptance, not here**.
- **Acceptance:** a section in the viewport appears on a sheet at the same scale.
- **Risk:** two hatch implementations. Fallback: the viewport's hatch spacing is in world units and the sheet's is in paper mm, and the conversion is the sheet's.

### REN-SECT-011 — The "open the solid" mode
- **State:** READY
- **Depends on:** REN-VIS-014, REN-SECT-006
- **Files:** `crates/render/src/scene3_clipbox.rs` (owner REN)
- **Contract:** `pub fn open_solid(solid: &Solid, box_: &ClipBox) -> Result<Solid, SolidError>;` — `solid − box_` (a difference), so the removed part of the body is **visible as an open shell** rather than clipped away.
- **Behaviour:** The x-ray inspection mode: everything outside the box is removed *and* the resulting open shell is drawn, so the user sees into the part. This is a different solid from the clip box and is honestly a different solid (`solid − box_`).
- **Refusals:** a difference that fails (the three open boolean defects) → `SolidError` propagated with the kernel's message. A result with no faces → refused, not drawn as a point.
- **Tests:** `open_solid_removes_the_outside_material`. `the_result_is_a_different_solid_with_fewer_faces`. `an_open_shell_is_rendered_not_hidden`. `a_difference_failure_is_propagated`.
- **Acceptance:** the face count drops and the picture shows the interior.
- **Risk:** an open shell fails `Solid::validate` (invariant I2), so `mesh_from_solid` still works (it does not validate) but picking sees through the hole. Fallback: the mode says the shell is open, and `REN-PICK-009`'s rule (a hit behind the clip is not a hit) is what keeps picking honest.

### REN-SECT-012 — Sectioning and transparency
- **State:** BLOCKED
- **Depends on:** REN-SECT-002, REN-GPU-018
- **Files:** `crates/render/src/scene3_section.rs::alpha` (owner REN)
- **Contract:** `pub fn sectioned_materials(m: &Material, sec: &SectionPlane, at: Vec3) -> Material;`
- **Behaviour:** The clipped-away material is **not drawn** (it is geometry, not an alpha trick), and the cap is drawn opaque while the model behind it keeps its own transparency. On the CPU path a transparent body behind a cap is drawn by the depth test and the cap wins — which is correct, and is why this task only matters on the GPU path.
- **Refusals:** a material with a non-finite alpha → `SceneError::NonFinite` naming it.
- **Tests:** `the_cap_is_opaque_whatever_the_material_is`. `the_model_behind_a_cap_keeps_its_own_alpha`. `a_non_finite_alpha_is_refused`.
- **Acceptance:** the cap never inherits a transparent alpha.
- **Risk:** a translucent body clipped open shows its interior surfaces with the wrong back-face culling. Fallback: `double_sided` (`REN-MAT-011`) is set on clipped bodies, and the setting is on the visstyle.

### REN-SECT-013 — Hostile planes
- **State:** READY
- **Depends on:** REN-SECT-001, REN-SECT-002
- **Files:** `crates/render/tests/section_hostile.rs` (owner Q)
- **Contract:** `a_nan_plane_is_refused`; `a_zero_length_normal_is_refused`; `a_plane_far_outside_the_scene_clips_everything_and_says_so`; `a_plane_at_the_origin_of_a_scene_at_1e6_clips_nothing`; `hostile_planes_never_panic` (200 mutations).
- **Behaviour:** The existing `Workplane::validate` (`scene3.rs:476-503`) already refuses non-finite fields and non-perpendicular axes; this task applies the same discipline to `SectionPlane` and to the clipping code that consumes it, with the 200-mutation sweep as the proof.
- **Refusals:** each case returns `Err` or an empty clip with a stated reason.
- **Tests:** the five above.
- **Acceptance:** green; a NaN plane never reaches the clipper.
- **Risk:** a clip loop that divides by a zero normal. Fallback: the plane is validated before use, and the test sweeps.

### REN-SECT-014 — Section determinism and properties
- **State:** READY
- **Depends on:** REN-SECT-002, REN-SECT-003
- **Files:** `crates/render/tests/section_properties.rs` (owner Q)
- **Contract:** `sectioning_is_deterministic`; `the_clipped_volume_never_exceeds_the_original`; `the_cut_loop_is_closed_for_a_convex_solid`; `a_plane_outside_the_solid_clips_to_nothing`; `sectioning_never_panics_on_random_planes`.
- **Behaviour:** Over random planes and random solids: the clipped mesh's volume (by signed tetrahedra, the same method `mesh.rs:361` uses) never exceeds the original's; the cut loop closes for a convex body; and two runs are byte-identical.
- **Refusals:** any violation is a test failure with both volumes.
- **Tests:** the five above, over 20 random solids × 10 random planes.
- **Acceptance:** green.
- **Risk:** the clipped mesh has unwelded vertices (three per triangle, `mesh.rs:60-62`), so a volume needs the signed-tetrahedron method, not a region. Fallback: that method is what the test uses.

---

# REN-MAT — materials, colour, alpha (18 tasks)

### REN-MAT-001 — The decision: the renderer's material is a light-model input
- **State:** READY
- **Depends on:** —
- **Files:** `crates/render/src/scene3_material.rs` (new; owner REN — **`scene3.rs` is agent U's in wave 1; the new file is not**), `docs/plan/REN-adr-0002-material-is-not-a-colour-store.md` (owner PLT)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Shading { pub base: Rgba, pub metallic: f64, pub roughness: f64, pub emissive: Rgba, pub alpha_mode: AlphaMode, pub double_sided: bool }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum AlphaMode { Opaque, Mask { cutoff: f32 }, Blend }
  ```
- **Behaviour:** The ADR and the type that resolves the collision `DAT-068` names: `scene3::Material { base, roughness, metallic }` exists, its docs say the renderer **ignores `roughness`** (`scene3.rs:244-246`), and `DAT-068` is about to add a `DocMaterial` with six more fields. Two options: grow the renderer's struct, or introduce a **render-side `Shading`** that the document's `DocMaterial` converts into. This task takes the second, records why, and adds `emissive`, `alpha_mode` and `double_sided` — the three a document needs and a light model does not.
- **Refusals:** `Shading::validate()` — a non-finite component or a `metallic`/`roughness` outside `0..=1` → `SceneError::NonFinite` naming the field. `Material` (the old type) stays and is re-implemented as `Shading { base, metallic, roughness, ..default }`, so every existing caller compiles.
- **Tests:** `the_old_material_converts_to_a_shading`. `the_shading_carries_three_fields_the_old_one_did_not`. `a_non_finite_shading_is_refused_by_field`. `the_adr_names_the_collision`.
- **Acceptance:** `scene3::Material` is a thin alias and `DAT-068`'s `DocMaterial` has exactly one conversion.
- **Risk:** two material types in flight. Fallback: `Material` becomes a constructor for `Shading` and is deprecated in its doc, with the removal named.

### REN-MAT-002 — The document-to-renderer bridge
- **State:** READY
- **Depends on:** REN-MAT-001, DAT-068
- **Files:** `crates/render/src/scene3_material.rs::from_doc` (owner REN + D)
- **Contract:** `pub fn shading_for(m: &cadcraft_model::DocMaterial, appearance: &cadcraft_model::Appearance) -> Result<Shading, ModelError>;`
- **Behaviour:** The single function `DAT-009` (STEP colours), `DAT-029` (3MF materials) and `DAT-032`/`DAT-033` (glTF `pbrMetallicRoughness`) all call, which is why `DAT.md` marks those tasks "BLOCKY READY — depends REN-MAT-002". It resolves **appearance → override → body default → library default** and converts to the render-side `Shading`.
- **Refusals:** a material name not in the library → `ModelError` naming the material and the library's names (the `DAT-068` rule). A stale face override → reported and dropped (`DAT-069`'s rule), never silently re-targeted.
- **Tests:** `an_override_beats_the_body_material`. `the_body_material_beats_the_library_default`. `an_unknown_name_is_refused_with_the_library`. `a_stale_override_is_dropped_with_a_report`.
- **Acceptance:** `DAT-029`, `DAT-032`, `DAT-033` compile against this one function.
- **Risk:** `DocMaterial` does not exist yet. Fallback: the function is written against the struct `DAT-068` specifies, and its test is a hand-built `DocMaterial` literal.

### REN-MAT-003 — One sRGB ↔ linear transfer function, in the colour crate
- **State:** READY
- **Depends on:** —
- **Files:** `crates/color/src/transfer.rs` (new; owner D), `crates/render/src/scene3.rs::encode_channel` (delegates; owner U in wave 1)
- **Contract:** `pub fn srgb_to_linear(c: u8) -> f64;` and `pub fn linear_to_srgb(c: f64) -> u8;` in `cadcraft-color` (L0, so `model`, `render` and `io` may all use it), with `scene3::encode_channel` (`scene3.rs:227`) and `Rgba::from_srgb8` (`scene3.rs:199`) **calling** it rather than restating the piecewise form.
- **Behaviour:** The transfer function currently exists twice — as the sRGB piecewise curve in `scene3.rs:227-240` and as its inverse in `scene3.rs:199-205` — and `DAT-033` needs the same curve on the export side. One implementation in the colour crate; `DAT-068` stores linear and decodes **once at document load** through this function.
- **Refusals:** `linear_to_srgb(NaN)` → `0`, as `encode_channel` already does (`scene3.rs:228-230`) — a hostile colour must not write a NaN into a buffer. `srgb_to_linear` takes a byte, so it cannot be given a non-finite value.
- **Tests:** `the_transfer_functions_round_trip_through_every_byte` — all 256 values, `linear_to_srgb(srgb_to_linear(b)) == b`. `encode_channel_delegates_to_the_colour_crate` — asserted by a test comparing the two functions over 1024 samples to exact equality. `a_non_finite_linear_encodes_as_zero`. `the_curve_is_the_published_one` — asserted at the two breakpoints `0.0031308` and `12.92`.
- **Acceptance:** `grep -c 'powf(2.4)' crates` finds the exponent in exactly one file.
- **Risk:** a change to the curve changes every rendered pixel. Fallback: the round-trip test over all 256 bytes pins the behaviour exactly.

### REN-MAT-004 — The document's material library
- **State:** BACKLOG
- **Depends on:** REN-MAT-002, DAT-055
- **Files:** `crates/model/src/material.rs` (owner D), `crates/render/src/scene3_material.rs` (owner REN)
- **Contract:** `pub fn library_from_doc(lib: &DocMaterialLibrary) -> Result<Vec<(String, Shading)>, ModelError>;` and `pub fn material_names(lib: &DocMaterialLibrary) -> Vec<&str>;`
- **Behaviour:** The library the document stores becomes the renderer's lookup table, and the names are what `REN-MAT-008`'s assign command offers. Round trip: a library written and re-read produces the same `Shading` values, byte for byte, including the linear colours.
- **Refusals:** a library with two materials of one name → refused naming both. A material with a non-finite colour → refused with the material's name (the `DAT-068` rule).
- **Tests:** `the_library_round_trips_with_its_linear_colours`. `a_duplicate_name_is_refused_with_both`. `a_non_finite_colour_is_refused_with_the_material_name`.
- **Acceptance:** `DAT-069`'s appearance resolves against it.
- **Risk:** a library stored in the document grows the file. Fallback: it is a small table and `DAT-054` records the field.

### REN-MAT-005 — PBR, and the exact BRDF
- **State:** BACKLOG
- **Depends on:** REN-GPU-005, REN-MAT-001
- **Files:** `crates/ui-egui/src/gpu3.rs::SHADER3` (`fs_pbr`; owner U), `crates/render/src/scene3_material.rs::brdf` (owner REN)
- **Contract:** `pub fn cook_torrance(n: Vec3, v: Vec3, l: Vec3, base: Rgba, metallic: f64, roughness: f64) -> Rgba;` — GGX NDF, Smith height-correlated visibility, Schlick Fresnel, `F0 = mix(0.04, base.rgb, metallic)`, diffuse = `(1 − metallic) · base / π`.
- **Behaviour:** The model, **written down with its formula**, so a reader can check it and a test can pin its limits: a perfect mirror (`metallic = 1`, `roughness → 0`) reflects the light direction; a perfect diffuser (`metallic = 0`, `roughness = 1`) returns `base · max(dot(n,l),0) · albedo/π`, which reduces to the Lambert term in `Material::shade` (`scene3.rs:292`) scaled by `1/π`. That reduction is the test that says the two models agree in the limit.
- **Refusals:** `roughness` clamped to `[0.045, 1.0]` (a GGX alpha below 0.045 is undefined and a mirror with `α = 0` divides by zero) and the clamp is **reported**, not hidden. A non-finite input → opaque black, as `Material::shade` already does (`scene3.rs:282-284`).
- **Tests:** `the_diffuse_limit_matches_lambert` — `metallic = 0, roughness = 1` equals `base · max(dot(n,l),0) / π` to `1e-9`. `the_metal_limit_reflects_the_light`. `a_roughness_below_the_floor_is_clamped_and_reported`. `a_non_finite_input_is_black`. `energy_is_not_gained` — a white surface under a single light never exceeds 1.0 before the encode.
- **Acceptance:** the four limits are tested and the formula is in the source.
- **Risk:** an environment term makes it look like PBR but is not measured. Fallback: with no environment map, the ambient term stays the constant it is today (`DEFAULT_AMBIENT 0.15`) and the doc says so.

### REN-MAT-006 — The material preview
- **State:** BACKLOG
- **Depends on:** REN-MAT-005
- **Files:** `crates/render/src/scene3_material.rs::preview` (owner REN)
- **Contract:** `pub fn preview_image(s: &Shading, size: u32) -> Result<Image, SceneError>;` — a sphere or a torus at `size × size`, rendered with the same shader as the viewport.
- **Behaviour:** The swatch next to the material list. It uses the **same** code path as the model, so a preview that looks right means the model looks right — that is the point, and a separate "preview renderer" is exactly the drift this avoids.
- **Refusals:** `size == 0` → `SceneError::EmptyImage`. `size` over `MAX_IMAGE_PIXELS` → `SceneError::ImageTooLarge` naming both.
- **Tests:** `the_preview_uses_the_same_shading`. `a_material_change_changes_the_preview`. `a_zero_size_is_refused`. `two_materials_differ_visibly`.
- **Acceptance:** the preview is a `scene3::render` call, not a second renderer.
- **Risk:** the preview's camera differs from the viewport's, so two materials could differ under the viewport. Fallback: the preview uses the viewport's current light rig.

### REN-MAT-007 — Per-face appearance overrides
- **State:** BACKLOG
- **Depends on:** REN-MAT-002, DAT-069, KER-MSH-003
- **Files:** `crates/render/src/scene3_material.rs::resolve` (owner REN + D)
- **Contract:** `pub fn resolve_face(app: &Appearance, body: &str, face: usize) -> Result<Shading, ModelError>;` — the single resolution chain.
- **Behaviour:** One function answers "which material is this face", in the order: face override → body material → library default. `DAT-069` already sets the contract and the honest limitation (a face **index** is stable only for a fixed geometry).
- **Refusals:** an override for a face index past the face count → **dropped with a report** (`DAT-069`), not fatal, so a document with one stale override still opens. An override naming an unknown material → refused at load, naming the body.
- **Tests:** `the_resolution_order_is_face_body_default`. `a_stale_index_is_dropped_with_a_report`. `an_unknown_material_is_refused_at_load`.
- **Acceptance:** one function, one order, three tests.
- **Risk:** the index changes after a rebuild. Fallback: `KER-MSH-003`; until then, `REN-VIS-010`'s refusal and this report.

### REN-MAT-008 — The assign command
- **State:** BACKLOG
- **Depends on:** REN-MAT-004, REN-PICK-004
- **Files:** `crates/engine/src/cmd/solid.rs` (owner C — **not REN's file**)
- **Contract:** `CommandSpec` ids `solid.material.assign {body?, faces[], material}` → `{assigned, dropped}` and `solid.material.library {action: list|add|remove, …}`.
- **Behaviour:** The command surface, because `AGENTS.md`: "Everything is a command." Assignment goes through the appearance resolver, so a bad index is dropped with a count in the JSON, not silently applied.
- **Refusals:** a material name not in the library → `Err` naming it and the list. An empty `faces` array → `Err`. A face index past the face count → dropped with a count in `{dropped}`.
- **Tests:** `assign_reaches_the_panel`. `an_unknown_material_is_a_named_error`. `a_stale_index_is_dropped_and_counted`. `the_command_appears_in_the_parity_list`.
- **Acceptance:** the command runs under `Session::execute`'s `catch_unwind` guard, like every other (`AGENTS.md`).
- **Risk:** engine files are agent C's. Fallback: `ren.material.assign` lands with the type and the engine command is requested through the orchestrator.

### REN-MAT-009 — Textures: the honest limitation, then a planar projection
- **State:** BACKLOG
- **Depends on:** REN-MAT-002, KER-SRF-002
- **Files:** `crates/render/src/scene3_texture.rs` (owner REN)
- **Contract:**
  ```rust
  pub fn uv_for(face: &cadcraft_solid::Face, p: Vec3, scale: [f64; 2], origin: Vec2) -> Option<Vec2>;   // planar projection only
  pub fn texture_refuse(face: &cadcraft_solid::Face) -> RenError;                                     // always: no UV parameterisation
  ```
- **Behaviour:** **A B-rep has no UV parameterisation.** CADCraft's faces are planes with a 2D region (`crates/solid/src/lib.rs:5-8`), so there is no surface parameter to map a texture onto. A real texture therefore needs either a trim-surface parameterisation (a NURBS surface with its own UV — `KER-SRF-002`) or an explicit planar projection. This task ships the **refusal** plus the planar projection as the honest interim, named as such.
- **Refusals:** asking for a per-face UV parameterisation → `RenError::Unsupported("CADCraft faces are planar with no UV parameterisation; use a planar projection with a stated scale")`. A texture whose path leaves the document's folder → refused naming the path (`DAT-029`'s rule, reused).
- **Tests:** `asking_for_a_uv_parameterisation_is_refused_with_the_alternative`. `a_planar_projection_maps_a_face_to_its_plane`. `a_texture_path_outside_the_folder_is_refused`. `the_projection_scale_is_in_world_units`.
- **Acceptance:** the refusal names the alternative, and the projection's scale is a stated world length.
- **Risk:** a planar projection on a cylinder shears. Fallback: it refuses above a stated face aspect ratio (`4:1`) and says why.

### REN-MAT-010 — The inverse direction, for glTF
- **State:** BACKLOG
- **Depends on:** REN-MAT-002, REN-MAT-003
- **Files:** `crates/render/src/scene3_material.rs::to_pbr` (owner REN + D)
- **Contract:** `pub fn to_pbr(s: &Shading) -> PbrFactors;` with `pub struct PbrFactors { pub base_color_linear: [f64; 3], pub metallic: f64, pub roughness: f64, pub emissive_linear: [f64; 3], pub alpha_mode: AlphaMode, pub double_sided: bool }`
- **Behaviour:** The direction `DAT-033` needs when it checks that what it wrote as glTF equals what the renderer used. It is a *check*, not an export: the writer (`DAT-033`) and this function must agree to `1e-9`, which is the round-trip test.
- **Refusals:** a factor outside `0..=1` → clamped and **reported** (`DAT-033`'s rule: glTF's schema requires the range and a clamp is visible while an error is confusing).
- **Tests:** `the_round_trip_is_exact` — `to_pbr(shading_for(doc))` equals the document's own factors to `1e-9`. `out_of_range_factors_are_clamped_and_reported`. `the_colour_stays_linear`.
- **Acceptance:** `DAT-033`'s round-trip test uses this function.
- **Risk:** two material representations drift. Fallback: the round-trip test runs in both domains' test suites.

### REN-MAT-011 — Alpha modes and double-sided rendering
- **State:** READY
- **Depends on:** REN-MAT-001, REN-GPU-018
- **Files:** `crates/render/src/scene3_material.rs::alpha` (owner REN), `crates/ui-egui/src/gpu3.rs::blend` (owner U)
- **Contract:**
  ```rust
  impl AlphaMode {
      pub fn writes_depth(self) -> bool;    // Opaque → true, Mask → true (discard), Blend → false
      pub fn discards(self, alpha: f32) -> bool;   // Mask → alpha < cutoff
  }
  ```
- **Behaviour:** The three modes, with their exact meaning, which is what `DAT-033`'s `alphaMode` needs to round trip: **`Opaque`** writes depth and ignores alpha. **`Mask { cutoff }`** is alpha-tested: fragments below the cutoff are discarded, the rest write depth — it is *not* transparency. **`Blend`** writes no depth and is composited back-to-front. Plus `double_sided: bool`, which disables the back-face cull (`REN-GPU-006`'s pipeline has the flag) and is what a clipped-open shell needs (`REN-SECT-012`).
- **Refusals:** a `Mask` cutoff outside `0..=1` → refused. A `Blend` material in a scene with no transparency pass → `RenError::Unsupported("blending needs the GPU transparency pass")` naming the task, not silently drawn opaque.
- **Tests:** `opaque_writes_depth_and_ignores_alpha`. `mask_discards_below_the_cutoff_and_writes_depth`. `blend_writes_no_depth`. `double_sided_disables_the_cull`. `a_blend_material_without_the_transparency_pass_is_refused`.
- **Acceptance:** the three modes are distinguished by their **depth behaviour**, tested, not by their appearance.
- **Risk:** `Mask` on a CPU path has no discard. Fallback: on the CPU path `Mask` is treated as `Opaque` and the doc says so; the distinction needs the GPU pass.
- **Note:** this is the `REN-MAT-011` that `DAT-033` names as a dependency for its `alphaMode` and `doubleSided` handling.

### REN-MAT-012 — The emissive channel
- **State:** BACKLOG
- **Depends on:** REN-MAT-001
- **Files:** `crates/render/src/scene3_material.rs::emissive` (owner REN)
- **Contract:** `pub fn emissive_contribution(e: Rgba) -> Rgba;` and the shader term `rgb += emissive.rgb` **after** shading and **before** the encode.
- **Behaviour:** Emissive is added, not modulated by the light: a face with `emissive = 1` is full white whatever its normal. On the CPU path it is a second `Material::shade`-like term; on the GPU it is in `fs_flat` and `fs_pbr` alike.
- **Refusals:** an emissive component outside `0..=1` → clamped and reported. A non-finite emissive → the material renders as if it had none, and the report says so (`scene3.rs:282` treats a non-finite material as black; emissive follows that rule rather than inventing one).
- **Tests:** `an_emissive_face_is_bright_regardless_of_its_normal`. `emissive_is_added_not_multiplied`. `an_out_of_range_emissive_is_clamped`. `a_non_finite_emissive_is_black_and_reported`.
- **Acceptance:** a face lit from behind still glows.
- **Risk:** emissive is not energy-conserving, so it is a **material**, not a light. Fallback: it is a channel in the material and never counted in `REN-LIGHT`'s light rig.

### REN-MAT-013 — Clearcoat, sheen, anisotropy: refused by name
- **State:** READY
- **Depends on:** REN-MAT-001
- **Files:** `crates/render/src/scene3_material.rs` (doc + `RenError`); owner REN
- **Contract:** `pub fn unsupported_layer(name: &str) -> RenError;` — returns `RenError::Unsupported("clearcoat is not modelled; CADCraft's material has base colour, metallic, roughness, emissive, alpha mode and double-sided")` naming the fields that **do** exist.
- **Behaviour:** The refusal, and the **field list**, so a user importing a glTF with a clearcoat extension is told what is lost and what is kept.
- **Refusals:** n/a (the function is the refusal).
- **Tests:** `clearcoat_is_a_named_refusal`. `sheen_is_a_named_refusal`. `anisotropy_is_a_named_refusal`. `each_refusal_lists_the_fields_that_exist`.
- **Acceptance:** three refusals, each naming its replacement state.
- **Risk:** a glTF import dropping a clearcoat silently. Fallback: `DAT-034`'s `ConversionReport::lost` is where the drop is recorded, and it names the extension.

### REN-MAT-014 — Colour management: one transfer function, no tone mapping
- **State:** READY
- **Depends on:** REN-MAT-003
- **Files:** `docs/plan/REN-adr-0003-no-tone-mapping.md` (new; owner PLT)
- **Contract:** the ADR states: CADCraft applies **exactly one** transfer function, the sRGB one, and **no tone mapping**. ACES, Filmic and Reinhard are refused by name because a tone curve changes every colour CADCraft has ever shown and none of them is in the product.
- **Behaviour:** A decision record plus a refusal: `pub fn tone_mapping(name: &str) -> RenError;`. The 2D side has the same property today (the plot/canvas path encodes through the same curve), so this is a **consistency** decision as much as a 3D one.
- **Refusals:** `RenError::Unsupported("tone mapping is not applied; CADCraft's colour pipeline is linear → sRGB")` naming the pipeline.
- **Tests:** `the_pipeline_is_linear_then_srgb`. `aces_is_a_named_refusal`. `the_2d_and_3d_paths_use_the_same_transfer` — asserted by comparing `raster.rs`'s encode with `scene3::encode_channel` over samples.
- **Acceptance:** the ADR exists and the 2D/3D equality test is green.
- **Risk:** a bright scene clips to white without tone mapping. Fallback: that is what the sRGB curve does and it is documented; the user lowers the light intensity.

### REN-MAT-015 — Material property tests
- **State:** READY
- **Depends on:** REN-MAT-005
- **Files:** `crates/render/tests/material_properties.rs` (owner Q)
- **Contract:** `no_material_produces_a_nan`; `a_white_diffuser_never_exceeds_one`; `a_metal_reflects_more_than_a_dielectric`; `roughness_monotonically_widens_the_highlight`; `the_double_sided_flag_does_not_change_the_colour`.
- **Behaviour:** Four physical sanity properties over 10⁴ random materials: no NaN reaches the buffer; a white Lambertian under a unit light is ≤ 1.0; a metal at the same base colour is brighter than a dielectric (F0 = 0.04 vs base); and increasing roughness never narrows the specular lobe.
- **Refusals:** a material that violates one is `SceneError::NonFinite` naming the field, not a rendered pixel.
- **Tests:** the five above.
- **Acceptance:** green, and the `no_nan` test is the one that matters most.
- **Risk:** "brighter than a dielectric" fails for a very dark metal. Fallback: the test states its precondition (base colour above 0.1).

### REN-MAT-016 — The material comparison view
- **State:** BACKLOG
- **Depends on:** REN-MAT-005, REN-VIS-007
- **Files:** `crates/render/src/scene3_modes.rs::split` (owner REN)
- **Contract:** `pub fn split_scene(scene: &Scene, axis: SplitAxis, at: f64, a: &Shading, b: &Shading) -> Result<Scene, SceneError>;`
- **Behaviour:** Two halves of the same body, shaded with two materials, split by a screen-space line. The cost is one draw with two material ranges and a scissor, not two renders.
- **Refusals:** an `at` outside `0..=1` → refused. A non-finite shading → refused.
- **Tests:** `the_split_is_a_screen_space_line`. `each_half_has_its_own_material`. `an_out_of_range_split_is_refused`. `the_cost_is_two_material_ranges_not_two_renders`.
- **Acceptance:** the cost is measured in `REN-PERF-005`.
- **Risk:** a niche feature. Fallback: it is one task and the visstyle mechanism carries the rest.

### REN-MAT-017 — The materials chapter in the parity doc
- **State:** BACKLOG
- **Depends on:** REN-MAT-005, REN-PERF-017
- **Files:** `docs/ui-parity.md` (owner PLT)
- **Contract:** a table: material property → modelled (`yes`/`no`) → where (`Shading` field or refusal) → the test that pins it.
- **Behaviour:** Eleven rows today (base colour, alpha, metallic, roughness, emissive, alpha mode, double-sided, clearcoat, sheen, anisotropy, transmission), each with a link to its task and its test. The "no" rows are as load-bearing as the "yes" rows.
- **Refusals:** an unmodelled property without a row → the test fails.
- **Tests:** `every_material_property_has_a_row`. `every_row_links_its_test`.
- **Acceptance:** eleven rows, each checkable.
- **Risk:** the table rots. Fallback: the test reads `Shading`'s fields and the `unsupported_layer` list and fails on a mismatch.

### REN-MAT-018 — Material import and export through DAT
- **State:** BLOCKED
- **Depends on:** DAT-029, DAT-033, REN-MAT-010
- **Files:** `crates/render/src/scene3_material.rs::io` (owner REN + D)
- **Contract:** `pub fn shading_to_json(s: &Shading) -> serde_json::Value;` and `pub fn shading_from_json(v: &serde_json::Value) -> Result<Shading, ModelError>;`
- **Behaviour:** The renderer-side halves of the two interchange tasks: 3MF (`DAT-029`) and glTF (`DAT-033`) both need a material in and out, and both already have the JSON shapes. This task is the converter between `Shading` and those values, so neither adapter writes its own.
- **Refusals:** a factor outside `0..=1` → clamped and reported (`DAT-029`/`DAT-033`'s shared rule). An unknown alpha mode name → `ModelError` naming it and the three that exist. A non-finite colour → refused.
- **Tests:** `a_material_round_trips_through_glTF_json`. `a_material_round_trips_through_3mf_json`. `an_unknown_alpha_mode_names_the_three_that_exist`. `a_glTF_alpha_mode_maps_to_our_three_modes` — `OPAQUE`/`MASK`/`BLEND`, asserted one by one.
- **Acceptance:** `DAT-029` and `DAT-033` call these two functions.
- **Risk:** three representations of a material in flight (`DocMaterial`, `Shading`, glTF JSON). Fallback: the round-trip test in each direction is the guard, and `REN-MAT-010` is the check.

---

# REN-LIGHT — lighting and shadows (16 tasks)

### REN-LIGHT-001 — The light rig, as data
- **State:** READY
- **Depends on:** —
- **Files:** `crates/render/src/scene3_light.rs` (owner REN)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
  pub struct LightRig { pub key: Directional, pub fill: Option<Directional>, pub rim: Option<Directional>, pub ambient: f64, pub key_intensity: f64 }
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
  pub struct Directional { pub dir: Vec3, pub colour: Rgba }
  ```
- **Behaviour:** Today there is exactly **one** directional light plus a constant ambient (`scene3.rs:579-583`, and the whole shading equation at `:57-60`). The rig is the general form with three slots, and `Material::shade` is generalised from one light to three by summing their contributions. `DEFAULT_AMBIENT 0.15` and `DEFAULT_DIFFUSE 0.85` are the single-light defaults and are preserved.
- **Refusals:** a light direction of zero length → refused naming the slot. A non-finite intensity → refused. `ambient` outside `0..=1` → clamped and reported.
- **Tests:** `the_default_rig_matches_the_current_defaults`. `three_lights_sum_their_contributions`. `a_zero_length_direction_is_refused`. `a_non_finite_intensity_is_refused`.
- **Acceptance:** the default rig renders **identically** to today, asserted by a pixel comparison.
- **Risk:** adding lights changes every picture. Fallback: the default is exactly today's rig and the pixel test proves it.

### REN-LIGHT-002 — Headlight versus world light
- **State:** READY
- **Depends on:** REN-LIGHT-001, REN-VIS-002
- **Files:** `crates/render/src/scene3_light.rs::headlight` (owner REN)
- **Contract:** `pub fn headlight(cam: &Camera, rig: &LightRig, mode: Headlight) -> LightRig;` with `pub enum Headlight { Off, On }`
- **Behaviour:** A headlight's direction is the camera's view direction, recomputed per frame; a world light's is fixed. **`scene3`'s default is a world light** (`RenderOptions::light` is a fixed `Vec3(-0.4, -0.75, 0.9)`, `scene3.rs:600`), so the honest default is `Off` and the picture does not change when it lands.
- **Refusals:** a headlight with a camera that fails `validate` → refused.
- **Tests:** `the_default_is_a_world_light`. `a_headlight_follows_the_camera`. `the_default_rig_renders_identically_before_and_after`. `an_invalid_camera_is_refused`.
- **Acceptance:** the pixel-identity test is the acceptance.
- **Risk:** a headlight flattens a model (every visible face gets the same light). Fallback: the default is off and the doc says why.

### REN-LIGHT-003 — The three-point rig with stated numbers
- **State:** BACKLOG
- **Depends on:** REN-LIGHT-001
- **Files:** `crates/render/src/scene3_light.rs::presets` (owner REN)
- **Contract:** `pub fn studio_rig() -> LightRig;` with **written** numbers: key at direction `normalize(-0.4, -0.75, 0.9)` intensity 1.0, fill at `normalize(0.6, 0.3, 0.4)` intensity 0.35, rim at `normalize(0.2, 0.5, -0.8)` intensity 0.45, ambient 0.15.
- **Behaviour:** One preset, our own, with every number in the source and a test asserting each. The intensity ratio (1 : 0.35 : 0.45) is a judgement; it is written down so it can be argued with rather than rediscovered.
- **Refusals:** n/a.
- **Tests:** `the_studio_rig_has_the_documented_numbers`. `the_fill_is_never_brighter_than_the_key`. `the_rim_is_behind_the_default_camera`.
- **Acceptance:** the numbers are in the source and in the test.
- **Risk:** "taste". Fallback: the user edits the rig; it is data in the visstyle.

### REN-LIGHT-004 — A code-drawn environment, and no asset
- **State:** READY
- **Depends on:** REN-LIGHT-001
- **Files:** `crates/render/src/scene3_env.rs` (owner REN)
- **Contract:** `pub fn gradient_environment(top: Rgba, horizon: Rgba, bottom: Rgba) -> EnvProbe;` with `pub struct EnvProbe { pub top: Rgba, pub horizon: Rgba, pub bottom: Rgba }` — an analytic environment, sampled as a function of the direction, with **no texture and no file**.
- **Behaviour:** `AGENTS.md` forbids any non-original asset, so an environment map is a **function**: a three-stop vertical gradient in direction space. It gives the ambient term a direction, so a surface facing up is brighter than one facing down — which a constant `ambient` cannot do (`scene3.rs:581`: "Constant light every surface gets, whatever its normal").
- **Refusals:** a non-finite colour → refused naming the field. An HDRI path → `RenError::Unsupported("environment maps are not loaded; the environment is a code-drawn gradient")` naming the alternative.
- **Tests:** `an_upward_normal_is_brighter_than_a_downward_one`. `the_environment_has_no_file_dependency`. `an_hdri_request_is_refused_with_the_alternative`. `a_non_finite_colour_is_refused`.
- **Acceptance:** `grep -rn "\.hdr\|\.exr" crates/render` is empty; the ambient term has a direction.
- **Risk:** a gradient is not an HDR environment. Fallback: it is called a gradient and is used as a gradient; the intensity range is stated.

### REN-LIGHT-005 — Shadow mapping on the offscreen pass
- **State:** BLOCKED
- **Depends on:** REN-GPU-002, REN-LIGHT-001
- **Files:** `crates/ui-egui/src/gpu3.rs::shadow` (owner U)
- **Contract:** `pub const SHADOW_SIZE: u32 = 1024;` and `pub fn encode_shadow(encoder: &mut wgpu::CommandEncoder, depth: &wgpu::Texture, m: &Scene3Mesh, light: &Directional) -> Result<(), RenError>;` — a depth-only pass from the light, then a 3 × 3 PCF lookup in `fs_*`.
- **Behaviour:** `scene3` has **no shadows at all**: "One directional light, no shadow map, no occlusion, no ambient term that knows about geometry" (`scene3.rs:14-17`). The shadow pass is a second render pass in the same encoder as `REN-GPU-002`, so it costs one depth render of the same geometry: a measurable `+1 draw` and a measurable ms.
- **Refusals:** no `Depth32Float` support → `RenError::Unsupported` naming the format. A light direction of zero length → refused. A scene over `MAX_VIEWPORT_TRIS` → refused before the pass.
- **Tests:** `the_shadow_pass_renders_to_its_own_depth`. `a_cube_casts_a_shadow_on_its_floor`. `turning_shadows_off_changes_only_the_shadowed_pixels`. `the_shadow_pass_costs_its_own_draw` — measured.
- **Acceptance:** the shadow map is a real pass and its cost is in `REN-PERF-005`.
- **Risk:** 1024² over a large scene is blocky. Fallback: the map covers the scene's bounding sphere, and the texel size is reported as the shadow's sharpness.

### REN-LIGHT-006 — Shadow bias and acne
- **State:** BLOCKED
- **Depends on:** REN-LIGHT-005
- **Files:** `crates/ui-egui/src/gpu3.rs::bias` (owner U)
- **Contract:** `pub struct ShadowBias { pub constant: f32, pub slope_scale: f32 }` with `pub const DEFAULT_BIAS: ShadowBias = ShadowBias { constant: 0.002, slope_scale: 2.0 };`
- **Behaviour:** Two numbers, both stated, and a test that **names the failure they fix**: acne (a surface shadowing itself) and peter-panning (a shadow detaching from its caster). The bias is scaled by the light's texel world size, so it is not a magic constant at a different zoom.
- **Refusals:** a bias so large the shadow detaches (`> 5 %` of the caster's size) → reported as a warning in the visstyle, not refused.
- **Tests:** `a_flat_face_does_not_shadow_itself` (no acne). `the_shadow_stays_attached_to_its_caster` (no peter-panning). `the_bias_scales_with_the_light_texel_size`. `an_oversized_bias_is_reported`.
- **Acceptance:** no acne and no detachment on the eight parity fixtures.
- **Risk:** a bias that works at one zoom fails at another. Fallback: the texel-scaled formula, and the test at two scene scales.

### REN-LIGHT-007 — Ambient occlusion: refused, with the measured reason
- **State:** READY
- **Depends on:** REN-PERF-001
- **Files:** `docs/plan/REN-adr-0004-no-cpu-sao.md` (new; owner PLT), `crates/render/src/scene3_light.rs::ao` (refusal; owner REN)
- **Contract:** `pub fn screen_space_ao(_: &SceneError) -> RenError { RenError::Unsupported("screen-space ambient occlusion is not implemented on the CPU path; its cost is a per-pixel depth neighbourhood search and the frame budget is {n} ms") }` — with the budget number substituted from `REN-PERF-004`'s table, not from memory.
- **Behaviour:** The ADR records the decision: CPU SAO is a per-pixel neighbourhood search over the depth buffer, which at 1920 × 1080 with a 16-sample kernel is ~33 M depth reads per frame — more than the whole CPU raster's budget (`REN-PERF-004`'s last row). A GPU implementation is a separate epic; until then the ambient term is the constant it is today.
- **Refusals:** `screen_space_ao` always refuses, naming the cost with the measured number.
- **Tests:** `ao_is_a_named_refusal_with_a_measured_number`. `the_adr_states_the_cost`.
- **Acceptance:** the ADR names a measured number, not an adjective.
- **Risk:** "no AO" looks flat. Fallback: `REN-LIGHT-004`'s directional environment gives a directional ambient, which is the honest cheap answer.

### REN-LIGHT-008 — Picking a light in the viewport
- **State:** BACKLOG
- **Depends on:** REN-LIGHT-002, REN-PICK-004
- **Files:** `crates/ui-egui/src/host3.rs::light_pick` (owner U)
- **Contract:** `pub fn light_at(rig: &LightRig, ray: &Ray) -> Option<LightSlot>;` with `pub enum LightSlot { Key, Fill, Rim }`
- **Behaviour:** A click on a light's gizmo selects that slot. Lights are drawn as **egui overlays**, not geometry, so the pick is screen-space against the gizmo's rectangle — the same shape as `REN-PICK-016`'s annotation pick.
- **Refusals:** a light with `fill: None` has no gizmo and is not pickable. A non-finite gizmo position → refused.
- **Tests:** `a_click_on_the_key_gizmo_selects_key`. `an_absent_light_has_no_gizmo`. `a_click_through_an_absent_light_picks_nothing`.
- **Acceptance:** three tests, one per slot's behaviour.
- **Risk:** a gizmo overlapping the model. Fallback: gizmos draw on top and are picked first (`REN-PICK-016`'s order).

### REN-LIGHT-009 — Lighting per saved view
- **State:** BACKLOG
- **Depends on:** REN-LIGHT-001, DAT-066
- **Files:** `crates/model/src/view.rs` (owner D), `crates/render/src/scene3_light.rs` (owner REN)
- **Contract:** `SavedView { …, pub light: Option<LightRig> }` — an optional field, `#[serde(default)]`, so a v1 view still loads (`DAT-054`'s rule).
- **Behaviour:** A view remembers its light, so "the front view" is not just a camera. `DAT-066`'s `SavedView` is the camera plus the six orthographic flags; this adds one optional field.
- **Refusals:** a `LightRig` with a non-finite intensity → refused at load naming the field. A view with no light → the document's default.
- **Tests:** `a_view_round_trips_its_light`. `a_view_without_a_light_uses_the_default`. `a_non_finite_light_is_refused_by_name`.
- **Acceptance:** the field is in `DAT-054`'s v2 field table.
- **Risk:** `crates/model` is agent D's file. Fallback: the field is `Option` with a default, so the two files can land apart.

### REN-LIGHT-010 — Per-face lighting modes
- **State:** READY
- **Depends on:** REN-LIGHT-001
- **Files:** `crates/render/src/scene3_light.rs::modes` (owner REN)
- **Contract:** `pub enum LightMode { Flat, Gouraud, Phong }` and `pub fn vertex_normals(mesh: &Mesh) -> Result<Vec<Vec3>, SceneError>;`
- **Behaviour:** `scene3` shades **flat**: one normal per triangle, computed as `(b−a)×(c−a)` (`scene3.rs:752`), and the module doc says so. Gouraud and Phong need per-vertex normals, which `mesh.rs` does not produce (it stores no normals — the 3MF writer notes normals are "written only when they disagree with the winding", `DAT-028`).
- **Refusals:** a mode the vertex normals cannot support (a mesh with unwelded vertices — and `mesh.rs:60-62` pushes three per triangle, so **every** kernel mesh is unwelded) → `SceneError::Unsupported("smooth shading needs welded vertices; the kernel's tessellation is unwelded")` naming the kernel's reason.
- **Tests:** `flat_is_the_default_and_matches_today`. `vertex_normals_average_the_adjacent_face_normals`. `a_smooth_mode_on_an_unwelded_mesh_is_refused_with_the_kernel_reason`. `welding_changes_the_picture_and_that_is_why_it_is_refused`.
- **Acceptance:** flat is exact, smooth is refused with its cause named.
- **Risk:** smooth shading is a visible feature users expect. Fallback: the refusal names the kernel property; `SIM-MSH-011` has the weld, and the renderer can borrow it later.

### REN-LIGHT-011 — The GPU path reproduces the CPU equation
- **State:** BLOCKED
- **Depends on:** REN-GPU-005, REN-LIGHT-001
- **Files:** `crates/ui-egui/tests/light_parity.rs` (owner Q)
- **Contract:** `pub fn light_parity(cpu: &Image, gpu: &[u8], light: &LightRig, max_delta: u8) -> Result<(), String>;` with **max_delta = 2** per channel, on eight fixtures and four rigs.
- **Behaviour:** The decisive test for the whole epic: `Material::shade`'s equation (`scene3.rs:281-294`) evaluated on the CPU and in `fs_flat` on the GPU, for every rig. Any disagreement above 2/255 is a bug in one of them.
- **Refusals:** a mismatch → the failure names the fixture, the rig and the worst pixel.
- **Tests:** `the_two_paths_agree_on_eight_fixtures_and_four_rigs`. `a_three_light_rig_agrees`. `a_non_finite_light_is_refused_on_both_paths`.
- **Acceptance:** 32 comparisons, all inside the bound.
- **Risk:** the `powf` of the sRGB curve differs in the last bit. Fallback: the per-channel bound of 2, cited from `scene3.rs:77-80`.

### REN-LIGHT-012 — Shadows do not change the silhouette
- **State:** BLOCKED
- **Depends on:** REN-LIGHT-005
- **Files:** `crates/ui-egui/tests/shadow_silhouette.rs` (owner Q)
- **Contract:** `pub fn silhouette_is_invariant(shadows_on: &Image, shadows_off: &Image) -> Result<(), String>;` — the set of pixels that differ from the background is **identical** with shadows on and off.
- **Behaviour:** A property test, not a picture test: a shadow map cannot change what is **drawn**, only how it is lit. A shadow that removes a silhouette pixel is a depth bug.
- **Refusals:** a silhouette difference → the failure names the differing pixels, which is the depth test's bug.
- **Tests:** `the_silhouette_is_invariant_with_shadows_on_and_off`. `only_the_shadowed_pixels_differ`. `the_difference_is_within_the_lit_area`.
- **Acceptance:** the two silhouettes are byte-equal.
- **Risk:** a shadow darkening a pixel to the background colour. Fallback: the test compares against the background *before* the difference, so a dark pixel still counts as drawn.

### REN-LIGHT-013 — Intensity units and exposure
- **State:** READY
- **Depends on:** REN-LIGHT-001
- **Files:** `crates/render/src/scene3_light.rs::exposure` (owner REN)
- **Contract:** `pub fn apply_exposure(c: Rgba, stops: f64) -> Rgba;` with `stops` in photographic stops, converted as `f64::exp2(stops)` and clamped to `[-8, 8]`.
- **Behaviour:** One exposure control on the whole scene, applied **before** the sRGB encode. The unit is stops, not a 0..1 slider, because a slider cannot express "+1 stop" and 1 stop is what a photographer means.
- **Refusals:** `stops` outside `[-8, 8]` → clamped and reported. A non-finite value → refused. A non-finite result after the multiply → encoded as 0 (`encode_channel`'s rule, `scene3.rs:228`).
- **Tests:** `zero_stops_is_identity`. `one_stop_doubles`. `minus_one_stop_halves`. `the_clamp_is_reported`. `a_non_finite_exposure_is_refused`.
- **Acceptance:** the five tests, and the value persists in the visstyle.
- **Risk:** exposure that clips. Fallback: it is applied before the encode and the doc says clipping is possible without tone mapping (`REN-MAT-014`).

### REN-LIGHT-014 — Light sanity tests
- **State:** READY
- **Depends on:** REN-LIGHT-001
- **Files:** `crates/render/tests/light_properties.rs` (owner Q)
- **Contract:** `the_brightest_surface_faces_the_key`; `no_surface_exceeds_one_before_the_encode`; `a_zero_ambient_gives_a_fully_black_unlit_surface`; `three_lights_never_exceed_one_light_for_the_same_base`; `no_light_produces_a_nan`.
- **Behaviour:** Four physical properties: a surface facing the key directly is the brightest; nothing exceeds 1.0 before the encode; with `ambient = 0` an unlit surface is black; and adding fill and rim to a rig never exceeds what the key alone gives for the same base colour (because the intensities are additive and bounded).
- **Refusals:** a rig that would exceed 1.0 → the value is clamped by `encode_channel` (`scene3.rs:231`), and the test asserts the clamp rather than pretending it cannot happen.
- **Tests:** the five above over 10³ random rigs and normals.
- **Acceptance:** green.
- **Risk:** the fourth property needs the intensities bounded. Fallback: `REN-LIGHT-001` refuses a non-finite intensity, and the sum is clamped by the encode.

### REN-LIGHT-015 — The lighting cost
- **State:** BACKLOG
- **Depends on:** REN-LIGHT-005, REN-PERF-005
- **Files:** `crates/ui-egui/tests/perf_shadow.rs` (owner Q)
- **Contract:** `shadow_cost_ms`: the measured cost of the shadow pass and the PCF lookup at 1920 × 1080 with 2 000 000 triangles, recorded as a **separate number** from the main pass.
- **Behaviour:** Two numbers — the shadow pass's draw cost and the per-pixel lookup's fill cost — because they scale differently: the first with triangles, the second with pixels.
- **Refusals:** a rig with shadows that cannot be measured (no timestamp query) → the CPU-side encode number is recorded and the GPU number is `unmeasured`.
- **Tests:** `the_shadow_cost_is_two_numbers`. `shadows_off_costs_less_than_shadows_on`. `the_measured_cost_is_in_the_baseline`.
- **Acceptance:** the number is in the baseline file, or explicitly unmeasured.
- **Risk:** shadows are the classic "free until you turn them on" feature. Fallback: the measurement is in the plan and the budget in `REN-PERF-006` includes it.

### REN-LIGHT-016 — Lighting presets, with their numbers
- **State:** BACKLOG
- **Depends on:** REN-LIGHT-003, REN-LIGHT-004, REN-VIS-007
- **Files:** `crates/render/src/scene3_light.rs::presets` (owner REN)
- **Contract:** `pub fn lighting_presets() -> Vec<(&'static str, LightRig, EnvProbe)>;` — three presets, ours: `"Studio"` (the `studio_rig`), `"Flat"` (ambient 1.0, key 0.0 — an unlit look for reading silhouettes) and `"Gradient"` (one key plus the directional environment).
- **Behaviour:** Three presets with every number in the source, each stored in a `VisStyle` (`REN-VIS-007`). `Flat` is the one people actually use for reading a silhouette, and it is **unlit** — an honest name.
- **Refusals:** n/a.
- **Tests:** `the_three_presets_are_distinct`. `the_flat_preset_has_no_key`. `each_preset_matches_its_documented_numbers`. `every_preset_validates`.
- **Acceptance:** three presets, three tests, numbers in the source.
- **Risk:** a preset that is just a different number. Fallback: each preset's numbers are asserted individually.

---

## 3. Task count

| Epic | Tasks | Ready now | Blocked (KER / cross-domain) | Backlog |
|---|---|---|---|---|
| `REN-PERF` | 18 | 10 | 5 | 3 |
| `REN-GPU` | 24 | 10 | 13 | 1 |
| `REN-VIS` | 22 | 17 | 4 | 1 |
| `REN-PICK` | 16 | 8 | 7 | 1 |
| `REN-SECT` | 14 | 9 | 4 | 1 |
| `REN-MAT` | 18 | 7 | 1 | 10 |
| `REN-LIGHT` | 16 | 7 | 4 | 5 |
| **total** | **128** | **68** | **38** | **22** |

(The state column of each task above is the authoritative count; this table is its sum.)

### The blockers, in the order they should be cleared

1. **`REN-VIS-001` — declare the four modules.** `crates/render/src/picking.rs`, `crates/ui-egui/src/{viewport3,viewport_state}.rs` and `crates/ui-egui/src/panels3/**` are **not in the build**: 72 test functions have never been compiled or run, and `viewport3::show` has no caller, so there is no viewport in the product. This is the cheapest task in the file and it is first for that reason.
2. **`KER-MSH-005` — re-export `section`.** One line in `crates/solid/src/lib.rs:93`. It unblocks `REN-SECT-008` here, five SIM tasks and six CAM tasks.
3. **`KER-SRF-003` — analytic surfaces.** Blocks `REN-VIS-005`'s adaptive tessellation, which is the viewport's one real quality ceiling: until it lands, the tessellation has no density dial and the triangle budget is the only control.
4. **`KER-MSH-003` — stable face/edge ids.** Blocks `REN-PICK-011` and `REN-VIS-010`: a selection that survives a rebuild.
5. **`REN-EXT-001` — a per-body visibility concept.** CADCraft 3D has no layers and no visibility, so `REN-PICK-015` cannot be written.
6. **A measured baseline.** Every millisecond in this file is a requirement until `REN-PERF-001..008` runs. Until then no performance claim in this document is a fact, and `docs/roadmap.md` must say so.

### What this file does not claim

- **`scene3` is not a viewport.** It is a validation renderer (`scene3.rs:1-33`): no shadows, no PBR, no
  transparency blending, no culling acceleration, single-threaded, O(triangles + covered pixels). The
  GPU path here is what makes a viewport possible, and it is not written.
- **No hidden-line removal.** `REN-VIS-003` refuses it by name rather than approximating it with a
  z-buffer edge test.
- **No tone mapping, no HDRI, no clearcoat, no sheen, no anisotropy.** Each is a named refusal that
  lists what CADCraft does model instead.
- **No smooth shading.** The kernel's tessellation is unwelded (`mesh.rs:60-62`), so a per-vertex
  normal would be an average of nothing; `REN-LIGHT-010` refuses it with that reason.
- **No performance numbers.** Every budget is a requirement with a named measurement. The
  implementation has not been run by the author of this file.