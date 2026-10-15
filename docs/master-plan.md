# CADCRAFT master plan — building a Fusion 360-class system

**Status:** authoritative development reference. Every subagent's task is defined here.
**Owner:** the orchestrator (session holding this file). No agent edits this file except through
an explicit coordinator request.
**Branch policy:** `feature/3d-parametric` until the wave-1 contracts land, then a per-wave branch.

---

## 0. Non-negotiable constraints

These come from `AGENTS.md` and are not negotiable at any level of the plan.

| Constraint | Consequence for this plan |
|---|---|
| `unsafe_code = "forbid"` in every crate | No FFI. No OCCT binding. No C++ anything. |
| LGPL/GPL/AGPL code is forbidden (`AGENTS.md`) | **OpenCASCADE cannot be linked, vendored or read.** It is a *conceptual* reference only: its algorithms are published literature (boolean arrangement, B-rep topology, NURBS evaluation, sew/knit). Porting a published algorithm is not copying code. Reading its source is out of bounds. |
| No `unwrap`/`expect`/`panic!`/`todo!`/`unreachable!` in non-test code | Enforced per crate by `deny` lints. Not a style preference; it is the crash-safety contract. |
| Pure Rust, no handwritten JS/TS | Web build stays Rust→wasm. |
| The 2D application must not regress | Every wave re-runs the 2D baseline: `geom`, `constraints`, `doc`, `engine`, `io`. |
| No new assets that are not original/CC0/OSI | Every icon and pattern is drawn in code. |

---

## 1. Scale, stated honestly

The aim is a system that competes with Fusion 360: parametric modelling, surface modelling,
assemblies, engineering drawing, simulation, manufacturing, on desktop and web.

Reference points for what that actually costs, from the outside:

- FreeCAD ≈ 3M lines of C++/Python developed over ~25 years by hundreds of contributors, and it is
  still hard to use.
- Fusion 360 is a commercial product of Autodesk built by a large team over a decade.
- CADCraft's existing 2D application is ~150 000 lines of Rust and is honestly described by its own
  `ROADMAP.md` as **pre-alpha**, ~38 % "ready for real work".

So the honest number is not "150 000 hours" and it is not "one year". It is: **a multi-engineer,
multi-year program**, of which we are building the first foundation layers. The plan is therefore
written to make *ordering* and *parallelism* correct, not to promise a date.

**This document will not carry a global percentage.** See §7 for why, and what is measured instead.

---

## 2. Task identity, ownership and state

```
DOMAIN-EPIC-NNN        e.g. KER-BOOL-004
```

State of every task, recorded in this file's companion index (`docs/master-plan-index.md`):

| State | Meaning |
|---|---|
| `BACKLOG` | specified, not started |
| `READY` | all prerequisites validated, files unowned by anyone else, can be handed to an agent |
| `IN PROGRESS` | an agent owns it |
| `CODE WRITTEN` | code exists, **not compiled, not tested** — this is a real state and it is not success |
| `INTEGRATED` | compiles, unit tests pass, in the main branch |
| `VALIDATED` | compiles, unit tests pass, property tests pass, and an independent check confirms the behaviour |
| `BLOCKED` | a named prerequisite is missing |

**`CODE WRITTEN` is not `INTEGRATED`, and `INTEGRATED` is not `VALIDATED`.** With terminal-less
agents, most work lands in `CODE WRITTEN` first. Counting tasks without distinguishing these states
is exactly how a program like this lies about itself.

---

## 3. Domains

| ID | Domain | Weight | Owns |
|---|---|---|---|
| `KER` | Geometry kernel: solids, surfaces, booleans, tolerances | 20 % | `crates/solid/**` |
| `SKT` | Sketches and constraints | 10 % | `crates/model/src/sketch/**` |
| `MOD` | Parametric model, history, recompute | 12 % | `crates/model/**` (except sketch) |
| `ASM` | Assemblies, joints, interference | 12 % | `crates/asm/**` (new) |
| `DRW` | Engineering drawing, GD&T, dimensions | 8 % | `crates/drawing/**` (new) |
| `SIM` | Simulation: mesh, FEA, thermal | 10 % | `crates/sim/**` (new) |
| `CAM` | Manufacturing: toolpaths, post, CAM | 8 % | `crates/cam/**` (new) |
| `DAT` | Documents, save/load, interchange, units | 8 % | `crates/model/src/file.rs`, `crates/io/**` |
| `REN` | Rendering and the 3D viewport | 6 % | `crates/render/**` |
| `UI` | Application shell, panels, interaction | 6 % | `crates/ui-egui/**` |
| `QAT` | Quality: property tests, corpus, benchmarks, CI | 6 % | `crates/*/tests/**`, `xtask/**` |
| `PLT` | Platform: build, wasm, docs, i18n, licensing | 4 % | `docs/**`, `xtask/**`, root `Cargo.toml` |

Weights are effort shares, not priorities. **Dependency order overrides weight**: `KER` before
`MOD` before `ASM` before `SIM`.

---

## 4. What exists today (audit, 2026-10-15)

Evidence, not opinion. Every claim below is checkable.

| Area | State | Evidence |
|---|---|---|
| 2D application | Substantial, working, pre-alpha | 408 `CommandSpec`, 72 781 LOC in `crates/engine`, ~1 400 tests |
| 2D boolean | Working, property-tested | `crates/geom/src/bool2d.rs`, `crates/geom/src/property_tests.rs` |
| Solid kernel | **Working for planar-faced solids; three open defects** | `crates/solid`, 74 tests |
| Property tests 3D | 12 tests, **4 failing on purpose**, each naming a kernel defect | `crates/solid/tests/property.rs` |
| Parametric model | Feature tree, parameters, dirty propagation, atomic rebuild, `.ccm` | `crates/model`, 18 tests |
| STEP AP214 | Writer exists, **never validated by a third-party reader** | `crates/solid/src/step.rs` |
| Renderer | CPU rasterizer for geometry validation, **not a viewport** | `crates/render/src/scene3.rs`, 17 tests |
| Engine commands 3D | In progress | `crates/engine/src/cmd/solid.rs` |
| Assemblies / drawing / simulation / CAM | **Nothing** | — |
| Topological naming | **None.** Edge references are geometric points | `EdgeRef` in `crates/solid` |
| Sketches reading parameters | **No.** A sketch's profile is static geometry | `crates/model/src/feature.rs` |
| NURBS / analytic surfaces | **None.** Planar faces only | crate docs, `crates/solid/src/lib.rs` |

**Three kernel defects are open right now** (`crates/solid/tests/property.rs`):
1. `revolve` does not close its shell (caps do not meet the swept walls).
2. Union across a shared face plane leaves the junction with the neighbouring side face unwelded.
3. `union(a,b)` can succeed where `union(b,a)` fails (`seed 0xc0ffee42`).

These are `KER-BOOL-001..003` below and they gate several `MOD` tasks.

---

## 5. Epics and seed tasks

Full task lists are expanded in `docs/master-plan-index.md` as waves are unlocked. This section is
the authoritative *structure*; the index carries the *count*.

### `KER` — Geometry kernel

- **`KER-BOOL` Boolean algebra and topology** — prerequisite: none. **CONTRACT OWNER: agent A.**
  - `KER-BOOL-001` Close `revolve`'s shell: match the end caps to the swept walls at the cap seam.
  - `KER-BOOL-002` Weld the union-across-a-shared-plane junction (`crates/solid/tests/property.rs::union_of_two_bodies_touching_across_a_plane`).
  - `KER-BOOL-003` Make union order-independent (`::union_is_commutative`, seed `0xc0ffee42`).
  - `KER-BOOL-004` Vertex welding: snap to a canonical grid *and* to the kernel's tolerance grid; document the cell-straddle case that can still duplicate a vertex.
  - `KER-BOOL-005` Robust boolean at scale: 10⁵–10⁶ triangles, spatial indexing (BSP/grid/octree), no O(n²) plane loop.
  - `KER-BOOL-006` Degenerate cases matrix: coincident faces, tangent edges, zero-thickness features, self-intersecting input. Each must be a named `Err`, never a solid.
- **`KER-SRF` Surfaces** — the biggest single item. Planar-faced-only cannot reach a Fusion-class part.
  - `KER-SRF-001` NURBS curve: degree, knots, evaluation, derivative, curvature, arc length, robust at high degree.
  - `KER-SRF-002` NURBS surface: bilinear evaluation, normals, trimming curves in parameter space.
  - `KER-SRF-003` Analytic surfaces: plane, cylinder, cone, sphere, torus. Exact, not tessellated.
  - `KER-SRF-004` Surface-surface intersection and trimming (the hard one; the heart of sweep/loft).
  - `KER-SRF-005` Sew / knit / heal: join a soup of trimmed faces into a closed shell.
  - `KER-SRF-006` Surface tolerance model: per-vertex, per-edge, per-face; an honest answer when a shape cannot be made at the requested tolerance.
- **`KER-FEAT` Solid features** — depends on `KER-BOOL` validated.
  - `KER-FEAT-001` Sweep along a path with orientation (Frenet, corrected frame, guide curves).
  - `KER-FEAT-002` Loft between N sections (single and multiple, twist control).
  - `KER-FEAT-003` Shell (offset outward/inward, face selection, open faces preserved).
  - `KER-FEAT-004` Draft (neutral plane, split faces, self-intersection guard).
  - `KER-FEAT-005` Thicken / offset face / replace face / delete face / suppress face.
  - `KER-FEAT-006` Variable-radius fillet; corner blending; three-edge fillets (currently refused).
  - `KER-FEAT-007` 3D patterns: rectangular, circular, mirror, along path, on face.
  - `KER-FEAT-008` Helix, sweep-helix thread, thread modelling.
- **`KER-TOL` Tolerances and measurement**
  - `KER-TOL-001` Signed distance field queries, section curves, curvature/continuity reporting.
  - `KER-TOL-002` Interference detection between two B-reps at a stated tolerance.

### `SKT` — Sketches and constraints
- `SKT-001` Profile model: a sketch whose geometry is *derived from constraints*, not stored as points. **This is the blocker for parametric width/height.**
- `SKT-002` Constraint types: coincident, concentric, tangent, parallel, perpendicular, horizontal, vertical, equal, symmetric, fix.
- `SKT-003` Dimensional: distance, angle, radius, diameter, with expressions bound to parameters.
- `SKT-004` Solver: Newton on the constraint Jacobian; under-, over- and exactly-constrained detection.
- `SKT-005` Diagnostics: list which constraints are redundant and which conflict, by name.
- `SKT-006` 3D sketches on arbitrary planes.

### `MOD` — Parametric model and history
- `MOD-001` Sketch-backed extrude/revolve that reads parameters (blocked by `SKT-001`).
- `MOD-002` Topological naming: persistent references that survive topology-changing edits (this is the single hardest problem in the domain).
- `MOD-003` Undo/redo as a feature-tree operation graph.
- `MOD-004` Multi-body parts (multiple solids in one part, with boolean between them).
- `MOD-005` Robust rebuild: partial rebuild, on-demand, with a documented cost model.
- `MOD-006` File format v2: versioned, migration-tested (a v1 file must open in a future build).

### `ASM` — Assemblies
- `ASM-001` Component / occurrence model, hierarchical.
- `ASM-002` Joint types: rigid, revolute, slider, cylindrical, spherical, planar, distance.
- `ASM-003` Assembly constraint solver (degrees of freedom, conflicts, locked state).
- `ASM-004` Interference detection and reporting.
- `ASM-005` Exploded views, animation of motion studies.
- `ASM-006` Bill of materials export.

### `DAT` — Documents and interchange
- `DAT-001` `.ccm` v2 (see `MOD-006`).
- `DAT-002` STEP AP242 with **real B-rep** (current writer is faceted only).
- `DAT-003` **Validate `DAT-002` against an independent STEP reader before it is claimed.** Non-negotiable.
- `DAT-004` IGES, 3MF, glTF, JT read/write as far as they are honest.
- `DAT-005` Units: a single authoritative unit system with explicit conversion at every boundary.
- `DAT-006` Object naming cross-reference between CADCraft and an external CAD (the topology map, persisted).

### `QAT` — Quality
- `QAT-001` Property tests for every kernel operation (the 2D engine's `property_tests.rs` is the model).
- `QAT-002` A model corpus: parametric models of known-good parts with expected volumes/areas, checked automatically.
- `QAT-003` Fuzzing at every public boundary (hostile coordinates, degenerate topology, truncated files).
- `QAT-004` Benchmarks with recorded baselines; a regression that costs >10 % fails the build.
- `QAT-005` Crash budget: `catch_unwind` guards at every command and every session operation.

### `REN`, `UI`, `DRW`, `SIM`, `CAM`, `PLT`
- `REN-001` GPU render path; `REN-002` picking and section box; `REN-003` visstyles/shading modes.
- `UI-001` 3D viewport (orbit/pan/zoom, picking); `UI-002` feature-tree panel with state per feature; `UI-003` parameter panel with live rebuild; `UI-004` sketch editor on the viewport; `UI-005` every label in `crates/ui-egui/src/i18n/uk.tsv`.
- `DRW-001` Orthographic views and sections from a solid; `DRW-002` Automatic dimensions; `DRW-003` GD&T callouts; `DRW-004` Title block, BOM, PDF export.
- `SIM-001` Mesher; `SIM-002` Linear static FEA (needs an external solver or a from-scratch one — decide); `SIM-003` Thermal; `SIM-004` Result visualisation in the viewport.
- `CAM-001` Toolpath generation (2.5D, 3D contour); `CAM-002` Tool/library model; `CAM-003` G-code post; `CAM-004` Machining simulation.
- `PLT-001` `cargo xtask ci` green at every wave boundary; `PLT-002` wasm build stays green; `PLT-003` docs.

---

## 6. Waves and who may work on what

A wave opens only when its prerequisites are `VALIDATED`.

| Wave | Contents | Gate |
|---|---|---|
| **0** | This plan, contracts, the agent system prompt, the audit in §4 | done |
| **1** | `KER-BOOL-001..004`, `QAT-001`, `DAT-003`, `UI-001` (parallel, disjoint files) | kernel property suite green except documented refusals |
| **2** | `KER-FEAT-*`, `SKT-001..003`, `MOD-001`, `REN-001`, `UI-002..003` | wave 1 `VALIDATED` |
| **3** | `KER-SRF-001..003`, `ASM-001..003`, `DRW-001`, `PLT-*` | wave 2 `VALIDATED` |
| **4** | `KER-SRF-004..006`, `SKT-004..005`, `MOD-002`, `SIM-001`, `CAM-001` | wave 3 `VALIDATED` |
| **5+** | Everything else, ordered by measured progress, not by ambition | — |

### File ownership in wave 1 (the anti-collision table)

| Agent | Owns (exclusive) | Must not touch |
|---|---|---|
| A — kernel | `crates/solid/src/{boolean,extrude,chamfer,section,solid,mesh,mass,plane,error}.rs` | `crates/solid/src/step.rs`, `crates/solid/tests/`, every other crate |
| Q — quality | `crates/solid/tests/**`, `crates/model/tests/**`, `xtask/**` | `crates/*/src/**` |
| D — data/interchange | `crates/solid/src/step.rs`, `crates/model/src/file.rs` | `crates/solid/src/{boolean,extrude,...}.rs`, `crates/engine/**` |
| U — ui/viewport | `crates/ui-egui/src/viewport3.rs`, `crates/ui-egui/src/panels3/**`, `crates/render/src/scene3.rs` | `crates/solid/**`, `crates/model/**`, every other `ui-egui` file |
| C — orchestrator only | everything else, plus integration and verification | — |

Two agents may never own two files in the same module. If a task needs a file another agent owns,
it is `BLOCKED` and says so; the orchestrator sequences it.

---

## 7. How progress is measured — and why there is no global percentage

Two independent numbers per domain, never one:

- **Coverage** — requirements in this plan that are `VALIDATED`, over requirements specified.
- **Maturity** — a separate scale, because coverage alone is a lie:
  `0 scaffold · 1 demo · 2 works on simple input · 3 works on realistic input · 4 property-tested ·
  5 hostile-input-tested · 6 performance-characterised · 7 interoperability-validated`.

Plus three standing counters:

- **Untested code share** — lines of `CODE WRITTEN` that have never been executed. Today this is
  high and it is the honest measure of how much of this program exists only on paper.
- **Open defect count** — currently 3 kernel defects, 4 failing property tests.
- **Blocked tasks** — waiting on a named prerequisite.

A percentage is reported only per domain, and only when both numbers are shown.

---

## 8. The rule that matters most

> A domain is not done because code was written. A domain is done when it survives randomized
> input, refuses the geometry it cannot make, and has been checked by something other than the
> author.

Every claim in this repository — progress, parity, capability — is measured against that rule.
