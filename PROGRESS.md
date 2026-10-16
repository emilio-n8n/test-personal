# PROGRESS — 3D parametric CAD for CADCraft

**Branch:** `feature/3d-parametric` (forked from `main` at `fc4e983` "Release 0.7.0")
**Goal:** add a working 3D parametric CAD area (sketch → dimension → extrude → cut → recompute →
save/reopen → STL/STEP) to CADCraft **without degrading anything 2D**.

> This file is the durable handoff record. Update it at every milestone, before committing.
> Last updated: Phase 3 done — the kernel and the parametric layer are working and tested. See "Status".

## Storage and persistence (verified, not assumed)

Verified via the GCE metadata server (`/instance/disks/?recursive=true`): the VM has exactly two
attached disks, **both persistent**, no scratch disks:

| Mount | Device | GCE device | Type | Use |
|---|---|---|---|---|
| `/` | `overlay` (container) | — | container overlay | **ephemeral** — caches only |
| `/home` | `/dev/disk/by-id/google-home-part1` | `home` (index 1) | `PERSISTENT` | **repo — durable** |

- The checkout `/home/emilio_developpeur/cadcraft` (20 MB, full git history) is on the persistent
  `home` PD. **No relocation was needed.**
- `/tmp/opencode` holds only re-downloadable data: `rustup/`, `cargo/`, `target/`, `logs/`.
  Source this before any cargo work: `. /tmp/opencode/env.sh`
- `/home` has ~1.3 GB free; keep build output off it.
- Nothing is pushed to any remote. All work is local commits on the feature branch.

## Environment

- 2 CPU cores, 7 GB RAM, no GPU, no Vulkan ICD, no X11/Wayland, no Xvfb.
- rustc/cargo under `/tmp/opencode/rustup` + `/tmp/opencode/cargo`
  (`rustup toolchain install stable --profile minimal --component rustfmt,clippy`), plus the
  `wasm32-unknown-unknown` target. Re-installable from `logs/rustup.log`.
- **The VM restarted between sessions and `/tmp/opencode` was empty**, exactly as predicted: the
  toolchain was reinstalled from scratch (rustc/cargo **1.99.1** now, was 1.99.0) and `/tmp/opencode/env.sh`
  was recreated. The repository on the persistent `home` disk was untouched, all six commits intact.
  This is the expected cost of keeping only reproducible data off the persistent disk.
- **The desktop app cannot be launched in this VM.** Any UI work can only be validated through
  egui_kittest harnesses and CPU-rendered PNGs, never through a running window. Claims about the
  GUI must say so explicitly.

## Baseline (before any 3D change)

`cargo test -p cadcraft-geom -p cadcraft-constraints` → **120 passed** (geom 84, constraints 27+9),
0 failed, 178 s cold on 2 cores.

## Status

| Phase | Scope | State |
|---|---|---|
| 0 | Toolchain, branch, persistence, test baseline | **done** |
| 1 | `geom`: robust 2D boolean engine | **done** (`crates/geom/src/bool2d.rs`) |
| 2 | `cadcraft-solid`: planar-faced B-rep kernel | **done** — blends work, **0 ignored tests** |
| 3 | `cadcraft-model`: feature tree, recompute, save/load | **done** (extrude, cut, blends; revolve refused) |
| 4 | Engine commands, CLI, MCP | not started |
| 5 | 3D viewport + tree/parameter panels | not started |
| 6 | STEP/OBJ export, docs, roadmap rows | not started |

**The user's milestone is met end to end, headlessly:** sketch → dimension → extrude → cut →
change the upstream dimension → recompute → save → reopen → the geometry checks out, and a broken
edit keeps the last good solid and says why (`crates/model/tests/parametric.rs`).

## Phase 1 — 2D boolean engine (`crates/geom/src/bool2d.rs`)

**What it does.** `union / intersect / difference / xor` on `Regions` (a set of regions; a region
is an outer loop plus hole loops). Algorithm: split every edge at every crossing (including
collinear overlaps) → merge fragments both operands contribute → classify each fragment by
evaluating the fill rule at a point just to *either* side against *both* operands → chain survivors
into loops → sort loops into regions by nesting depth.

**Design decisions worth keeping:**
- **Even-odd fill**, so self-intersecting input stays well defined and operand orientation is
  irrelevant.
- Split points are stored as *points*, not parameters, so both edges of a crossing get
  bit-identical endpoints.
- Edges are sorted by geometry (stable) before splitting, and the split grid is a `BTreeMap`.
  Without both, `union(a,b)` and `union(b,a)` differ in the last bit — HashMap iteration order
  decided which of two near-equal crossing points survived de-duplication.
- Nesting depth is decided by "does loop X enclose a *vertex* of loop Y", never by interior
  points: nested loops can share an interior point, which silently produced depth 1 for everything.
- Parity queries use a horizontal band (active-edge) index. Indexing edges by their own bounding
  box is wrong for ray casting: a query must see every edge the ray crosses.
- Only a crossing of both *segments* splits them, not of the infinite lines.

**Verified limits (documented in the module header):** tolerance scales with the bounds
(`diag × 1e-11`, clamped); features thinner than the tolerance are not resolved; degenerate
geometry is dropped rather than returned as slivers; oversized input returns `None`.

## Phase 2 — solid kernel (`crates/solid`)

New crate at L0 (registered in `xtask/src/layers.rs`), building on `geom` for vectors and the 2D
boolean. Representation: a `Solid` is a set of planar `Face`s, each an infinite `Plane` plus a
`Regions` (outer loop + holes) in that plane's own frame. Faces carry no vertices — the shared
boundary *is* the topology.

**Working and tested:** `extrude`, `revolve`, 3D `boolean` (union / intersect / difference),
plane `section`, tessellation + STL (ASCII and binary), exact volume / area / centroid.

**Invariants** (documented in the crate header, enforced by `Solid::validate`, with a second,
off-the-default `validate_vertices`): closed (every edge exactly twice), consistently oriented (once
in each direction), outward (positive volume), and manifold at vertices (one fan, not a pinch). A
solid that violates any of them is an error, never a body handed to the renderer.

**Honest limits (as of commit `4e5a947`):**
- Chamfer and fillet **work**: a chamfer of 2 on a 10-cube removes exactly 20, a fillet's removed
  cross-section is within a whisker of `r²(1 − π/4)`, and a round on a *slanted* edge works too.
  A blend whose tool runs off the end of its edge **across** a face it was not asked to cut is
  refused with a reason (`tool_stays_within`), because welding three faces at that corner is not
  finished. Touching such a face is normal and works.
- A plane section through an exact edge or vertex now works (the old failure was a wrong
  expectation in the test: the chord across a 4×4 face cut by `x+y=2` is 2√2, not 4√2).
- Planar faces only, so rounds are faceted. Edges are referenced geometrically (a point on the
  edge), not by persistent topological names.
- **Revolve does not build a solid.** A partial revolve meets its own end caps at a degenerate
  corner and a full turn has nothing to close with. The feature is wired and refused with the
  reason; the tests state the rule (a solid that validates, or a named failure).

**The blend bugs that were fixed (all real, all caught by tests):**
1. The blend tool was built **inside out**: its cross-section ran the wrong way round relative to
   the sweep, so every wall and cap faced inward and the tool was not a valid solid — which is why
   *every* chamfer and fillet returned an open shell. The wedge is now the profile extruded through
   the ordinary `extrude`, wound counter-clockwise about the sweep.
2. The fillet's arc swept an angle measured in the **XY plane**, which is only right when the edge
   lies in XY, and put its tangent points `r` along each face, which is only right at a right-angle
   corner. It now rotates about the edge direction and uses `r/tan(half)`.
3. **The boolean's coplanar welding** (`weld_edges`): a face a tool merely *touches* keeps its whole
   loop while the faces around the touch get split, so the two stop sharing an edge. Two details make
   the pass correct rather than plausible: vertices are snapped onto a shared grid *first* (the same
   corner arrives by two routes a ten-millionth apart, and splitting one face and not the other
   creates the mismatch), and only a vertex **in the face's plane** may split one of its edges (a
   vertex off the plane projects exactly onto the edges of an axis-aligned solid).
4. Validation reported a *different* broken edge on each run: the report is the first offender in
   iteration order, and that order was `HashMap` order. Now a `BTreeMap`.

### Bugs the tests caught (each was real, not a test artefact)

1. `difference(a, empty)` returned **empty** instead of `a` — this silently deleted whole solids,
   because an empty section is the normal case for a cutting plane that misses.
2. `Face::flipped` kept the loop in a **mirrored** frame, so flipping a face moved its boundary.
3. The plane section rebuilt crossing points from the line parameter, dropping the offset **across**
   the line — segments came out in the wrong place.
4. Ear clipping tested containment against *all* loop vertices instead of the remaining ones, and a
   hole bridged into one weakly simple loop produced triangles that overlapped instead of failing.
5. Binary STL wrote `f64` and a 20-byte header, so no reader would accept the file.
6. The containment ray solved the plane equation with the wrong dot product.
7. A blend tool's end caps were read as world points instead of offsets from the edge, so every tool
   was built off its own edge.

## Test evidence

| Command | Result |
|---|---|
| `cargo test -p cadcraft-geom` | **116 passed**, 0 failed (was 84) |
| `cargo test -p cadcraft-constraints` | **36 passed**, 0 failed |
| `cargo test -p cadcraft-solid` | **68 passed**, 0 failed, **0 ignored** (was 47 + 7 ignored) |
| `cargo test -p cadcraft-model` | **18 passed**, 0 failed |
| `cargo test -p cadcraft-doc` | **64 passed**, 0 failed (2D app untouched) |
| `cargo clippy -p cadcraft-geom --all-targets` | clean |
| `cargo fmt -p cadcraft-geom` | applied |
| `cargo check --workspace --all-targets` | **passes**, 14m07s (before phases 2–3) — rerun before the next commit |
| `cargo run -p xtask -- layers` | **18 crates, no layering violations** (`solid` L0 → `geom`; `model` L3) |

Property tests added (deterministic seeded RNG, repo's existing xorshift):
- `boolean_of_rectangles_matches_the_analytic_overlap` — 400 random rectangle pairs, union /
  intersect / difference / xor areas compared against an **independently computed** overlap area
  (not the engine's own), plus a simplicity validator on every output loop.
- `boolean_area_identities_hold_for_random_polygons` — 300 random convex polygon pairs, the
  inclusion–exclusion identities.
- `boolean_is_idempotent_commutative_and_reflexive` — 200 cases; commutativity is checked
  **bit-for-bit**.
- `boolean_of_a_contained_region_follows_set_rules`.
- `boolean_never_panics_on_hostile_geometry` — 400 cases of NaN/±inf/subnormal/1e300/1e18
  coordinates and self-crossing polygons.

Known-answer tests cover shared edges, collinear overlaps, nested regions, islands inside holes,
a notch on an edge, a square lying in a hole, tangency, determinism, scale invariance
(1e-6 … 1e6) and a bow-tie under even-odd.

## Phase 3 — parametric model (`crates/model`, L3)

New crate above the kernel, registered in `xtask/src/layers.rs` as L3. `Model` = parameters +
ordered named features. `FeatureKind` = `Extrude { sketch, depth, symmetric }`,
`Revolve { sketch, axis, angle, segments }`, `Cut { tool }`, `Blend { edges, size, segments, round }`.
`Expr`/`parse`/`evaluate` give parameters that are expressions over other parameters
(`twice = height * 2`), with a DFS and a cycle check.

**The two rules, and why they are the design:**
1. **A failure never destroys the last good shape.** Each feature keeps the last solid that worked;
   a failure keeps the feature's *own* last solid (not the running one — that bug once replaced a
   cut's result with the untouched body).
2. **Blocked ≠ failed.** A feature above a failure is `State::Blocked` with the reason, and its own
   last good solid. "This could not be built" and "this was not built because the thing under it
   could not" are different facts.

**Dirty propagation** (`Model::dirty_from`) is a transitive walk: naming a feature dirties it, and a
parameter defined in terms of another carries the change to everything that reads it.

**Save/load**: `.ccm` is JSON holding the *history* (`Document`), never the geometry — so a file can
never disagree with the features in it, and a document whose features no longer build still opens
with the tree, the parameters and the error. `FORMAT_VERSION = 1`; a newer file is refused with both
numbers. serde derives for `Vec2`/`Vec3` were added to `geom` (which already depends on serde).

**Four bugs the tests found** (each documented in the code): a cut's tool replaced the body it was
meant to cut; a partial rebuild handed each skipped feature the running solid instead of its own; a
failure/blocked record kept the running solid; `Plane::flipped` reverses `u` as well as the normal
and therefore *mirrors* a profile (a symmetric extrude moves its plane instead).

**Honest limits:**
- **Sketches do not read parameters.** A dimension drives depths, angles and blend sizes — and
  everything that reads those — but cannot move a profile's corners. Constraint-driven sketches are
  the next piece of work on this layer.
- **Revolve is wired and refused**, not building (see Phase 2).
- Edge references are geometric (`EdgeRef`: a point + tolerance). No topological naming. A reference
  that stops resolving is an error naming the edge that is gone, never a silent re-target — tested.

## Known failures / limitations

- OpenCASCADE: absent from the machine, unbuildable here on 2 cores, LGPL (forbidden by
  `AGENTS.md`), and its bindings require `unsafe` (workspace forbids). Not used.
- The only 2D boolean in the repo (`engine/src/cmd/inquiry/regions.rs`) fails when region
  boundaries cross or touch — the Phase 1 engine in `geom` is meant to replace it.

## Next actions

The kernel and the parametric path are done and tested. What is left is reaching them from outside:
commands, then rendering, then export.

1. **Engine commands** (`crates/engine`): `solid.body.new`, `solid.feature.extrude|cut|fillet|chamfer`,
   `solid.param.set|list`, `solid.export.stl|obj`, `solid.save|open`. JSON-only `CommandSpec`s with
   **no `interactive` factory**, so the `Vec2` prompt protocol and the two global registry sweeps
   stay untouched. A Ukrainian row per label in `crates/ui-egui/src/i18n/uk.tsv`. Then the CLI
   subcommand and the MCP tools in both `Remote` and `Headless` backends.
2. **Constraint-driven sketches** (`crates/model`): a sketch whose corners read parameters, so a
   dimension moves geometry and not only depths. This is what makes the user's sketch → dimension →
   extrude → *change the dimension* loop complete in the shape people expect.
3. **Fix revolve in the kernel** (`crates/solid/src/extrude.rs`): a partial revolve meets its end
   caps at a degenerate corner, and a full turn produces no closing faces. The model-side tests are
   already written in the form that says what a correct answer looks like.
4. **Rendering + viewport**: `render::scene3` and a CPU raster first, then `ui-egui` orbit/pan/zoom,
   a feature-tree panel and a parameter panel, with ray picking. **No UI work before the commands
   work headlessly** — this VM has no display, so a GUI claim could not be verified.
5. **STEP AP214** export (`FACETED_BREP`). Only claim it once an **independent** STEP parser accepts
   the file, as the user required; OBJ is a quick check that the mesh path is honest.
6. **Docs**: `docs/solid-parity.md`, the `docs/gaps.md` F24 row (3D, ~300–450 h), ROADMAP.
7. **`cargo xtask ci`** at the next phase boundary (20–40 min on 2 cores); `cargo check` plus
   targeted tests in the loop.

Do **not** start the UI before the feature tree and commands work headlessly: this VM has no
display, so every UI claim would be unverifiable.

## Resume instructions

```bash
cd /home/emilio_developpeur/cadcraft
git switch feature/3d-parametric          # durable: persistent home PD
. /tmp/opencode/env.sh                    # re-create if the VM restarted (re-downloadable)
git log --oneline -5                      # last committed milestone
cargo test -p cadcraft-solid -p cadcraft-model -p cadcraft-geom   # then "Next actions" above
cargo run -p xtask -- layers              # 18 crates, no violations
```
---

## 2026-10-15 — parallel-agent pass (post-audit)

Four subagents on strictly disjoint file partitions; the orchestrator verified every result itself.

**Landed and verified:**
- `crates/solid/src/step.rs` — STEP AP214 `FACETED_BREP` writer, 6 tests. Real part 21 output with a
  shared vertex/edge table; the tests check the *entity graph* (every reference resolves, every
  reference points backwards, only the two entry points are unreferenced). **No third-party STEP
  reader has ever opened its output** — the module docs say so, and the file must be validated in the
  CAD system it is meant for before anyone trusts it. Uses `ADVANCED_FACE` where AP214's
  `faceted_brep` rule names `FACE_SURFACE` (a subtype relationship, not a verified import).
- `crates/solid/tests/property.rs` — 12 randomized property tests, 11 s, fixed seeds. **4 fail on
  purpose** and are not `#[ignore]`d: a green suite here would be a lie. They found three real
  kernel defects, each with exact reproducing inputs in the failure message:
  1. `revolve` closed no shell — `triangle_face` built a 2-point region, so every swept patch was
     silently dropped (`Region::is_empty` is `outer.len() < 3`). Fixed; the caps still do not meet
     the swept walls.
  2. Union across a shared face plane left an open shell — `boolean.rs` dropped the plane whenever
     the operands sat on opposite sides, which is right only when their sections coincide, so
     stacking a box on *part* of another failed. Now two faces with opposite normals (a planar face
     carries one normal); their junction with the neighbouring side face is still unwelded.
  3. `union(a,b)` can succeed where `union(b,a)` fails (`seed 0xc0ffee42`). The existing
     commutativity test compares volumes and never checks that both orders *succeed*.
- `crates/render/src/scene3.rs` — camera / material / scene / workplane plus a CPU rasterizer with
  back-face culling, depth buffer and flat shading. 17 tests, `cadcraft-render` now depends on
  `cadcraft-solid` (L2 → L0, `xtask layers` accepts). Explicitly a geometry validator, not an
  interactive viewport: no shadows, no PBR, no transparency, no spatial acceleration.

**Corrections to my own audit.** I wrote that the kernel was "the most advanced and least risky
block". That was wrong, and wrong for an instructive reason: it was the only block with no
randomized tests, so 68 hand-written tests written alongside the code gave false confidence. Three
fundamental defects survived them. The remaining risk is concentrated there, not in the UI.

**In flight:** the engine-commands agent (`crates/engine/src/cmd/solid.rs`).

---

## 2026-10-16 — Wave 1 validation, real numbers

Toolchain restored a second time (rustc 1.99.1, repo needs >= 1.90, edition 2024).
`cargo test -p cadcraft-solid`, every suite, nothing ignored:

| Suite | Passed | Failed |
|---|---:|---:|
| `--lib` | 82 | 4 |
| `property` | 9 | 3 |
| `property_surface` | 7 | 10 |
| `fuzz` | 8 | 2 |
| `corpus` | 10 | 3 |
| `step_interop` | **0** | **11** |

Eleven compile errors had to be fixed first: all in agent-written test files that had never
been run (inline format args with arithmetic, a moved value, a shadowed helper, an `Option` given
to a `Result` helper, a method called from a free function, `u8::is_ascii_whitespace` where a
by-value closure is required, a slice borrow not tied to its argument).

**The headline finding: `step_interop` fails 11 of 11.** The independent second parser finds zero
solids in the file the writer produces. So the STEP writer does not survive a reader written by
anyone else — including one written to be lenient. That is exactly what `DAT-003` existed to find,
and it means the STEP export is **not** an interchange feature today. It is a text file.

Other findings:
- `fuzz`: flipping a solid twice does not reproduce the faces exactly — a real `Face::flipped`
  idempotence defect, with a seed (`0xf1a0000a`) and a full face dump.
- `corpus`: a chamfer at size 100 on a 100-cube fails `used 2 times forward`. The blend path is
  not scale-robust.
- One corpus expectation is itself wrong (derives 600, asserts 936). Left visible rather than
  corrected to match the kernel.
- A hypothesis of mine about `revolve`'s on-axis guard was **falsified** by the suite in one run
  (82/4 -> 80/6) and reverted; the reasoning is recorded in the source so it is not retried.

Wave 1 is `CODE WRITTEN`. **0 `VALIDATED`.** Wave 2 stays closed. Nothing was deleted, weakened or
ignored to improve any number above.

Backup: `/home/emilio_developpeur/cadcraft-backup-3d.bundle`, complete history, verified. Push is
impossible: `403`, `emilio-n8n` has no write access to `storytold/cadcraft`.

### Blend scale defect — pinned, not guessed at

`corpus::a_chamfer_removes_its_triangle_at_every_size` fails at exactly one cell of its grid:
`side = 100, d = 0.25` — a chamfer whose width is 1/400 of the solid. Every other cell passes,
including `side = 4, d = 0.25` (ratio 1/16). So the failure is a *relative* one, which points at the
tolerance model rather than at the blend geometry: `tolerance_for(100*sqrt(3))` is 1.73e-7, and the
weld grid in `weld_edges` is sized from it, so at this ratio the chamfer's own features sit only
1.4e6 cells from their neighbours while the grid quantises at ~5.8e6 cells per unit.

Failure mode: the vertical edge `(99.75, 0, 0) -> (99.75, 0, 100)` — the seam between the y=0 face
and the bevel face — is traversed twice in the same direction, so the two faces disagree about which
side is out.

This is **not** fixed here, and deliberately so. It is a kernel defect on the critical path for
`KER-FEAT-*`, a plausible but unproven cause, and no budget to verify a speculative fix against 82
passing tests. The failing corpus test is itself the pinned regression: the next attempt starts by
bisecting `side` to find where the ratio starts to matter, then measures the weld grid against the
chamfer's feature size at that point.

The same defect family is already visible in `KER-BOOL-002` (union across a shared plane) — both are
cases where two faces must agree along a seam and the welding does not make them.

### Blend scale defect — root cause identified by sweep

Swept `side` × `d` on a box chamfer at its vertical edge. Results:

```
side      4:  ok  ok   -   -
side     10:  ok  ok  ok  ok
side     25:  ok  ok  ok  ok
side     50: ERR ok  ok  ok
side    100: ERR ok  ok  ok        (d = 0.25)
side    200:  ok ERR ok  ok        (d = 1)
side    400:  ok ERR ERR ERR      (d = 1, 2, 3)
ratio 1/400 swept over scale: ERR at every scale from 100 to 1600
side 100, d swept: 0.005..0.2 ok, 0.25 ERR, 0.3 ok, 0.5 ERR, 1.0 ok
```

**It is not a ratio threshold.** At side 100, d=0.2 passes, d=0.25 fails, d=0.3 *passes again*,
d=0.5 fails, d=1.0 passes. A monotone tolerance explanation is already excluded.

What separates the failures is that **d is exactly representable in binary**: 0.25 = 2^-2 and
0.5 = 2^-1 fail; 0.2, 0.3, 0.05, 1.0 (which is exact too, but large relative to the box) do not.
When d is a power of two every coordinate in the boolean is exact, and `weld_edges`'s snapping
grid — `((v - origin) * (1/tol)).round()` with `tol = diagonal * 1e-9` — lands points exactly on a
cell boundary, so a vertex is rounded into a different cell in one face than in its neighbour and
the two faces never share the welded edge.

That is the cell-straddle weakness `KER-BOOL-004` names, and this sweep is its minimal
reproducer: `side = 100, d = 0.25` (and `side = 50, d = 0.25`, `side = 200, d = 1`). The fix belongs
in the weld: snap to a coarser lattice and de-duplicate by proximity, or search neighbouring cells,
rather than trusting one rounded cell index.

### Task index complete — 10 of 10 files, 952 tasks

`docs/plan/`: KER, SKT, MOD, ASM, SIM, CAM, DAT, REN/UI, UI, DRW, PLT — **19 150 lines**,
**952 tasks**, of which **632 are `READY`**.

| Index | Lines | Index | Lines |
|---|---:|---|---:|
| KER | 1362 | SIM | 1857 |
| SKT | 1009 | CAM | 1876 |
| MOD | 1781 | DAT | 1803 |
| ASM | 656 | UI | 1870 |
| DRW | 4091 | PLT | 2845 |

Two contradictions between indexes were found by the writing agents and deliberately left for the
coordinator rather than resolved silently:

1. **`PLT-LAYER-009` is a blocked *decision*, not a task.** `ASM.md` puts `cadcraft-asm` at L3 and
   allows it to use `cadcraft-model`, also L3 — which `check()` at `xtask/src/layers.rs:180`
   flags as a violation. Two sibling documents disagree. Three ways out are listed; none chosen.
2. **`cadcraft-drawing`'s layer (L2) is a judgement**, not something any document states, and
   `cadcraft-drawing` may not use `render` (L2 sideways) — so the viewport's hidden-line style has
   to be served from the engine layer.

Also recorded rather than smoothed over: **hidden-line removal on a B-rep is surface-surface
intersection** (`DRW-XRF-018`), which makes it the single hardest dependency in the whole plan and
puts it squarely behind the NURBS work in `KER-SRF`.

None of this is code. None of it is validated. It is a work breakdown with 952 entries, each with
state, dependencies, owned files, a contract, refusals, tests, acceptance and risk.

### 72 3D tests had never been compiled — my omission, now fixed

`REN.md`'s writer found it: `crates/render/src/picking.rs`, `crates/ui-egui/src/{viewport3,
viewport_state}.rs` and `crates/ui-egui/src/panels3/**` existed on disk but were **never declared
as modules**, because I had forbidden those agents from touching `lib.rs` — correctly, they did not
own it — and then never added the lines myself. 72 test functions had never been compiled.

`picking` is now declared and compiling: **render 100 → 116 passed, 1 failed** after fixing two
compile errors (a `&Ray`/`Ray` mismatch and `Hit::is_finite` used where a closure is required).

Its one remaining failure is a **test-math bug, not a kernel bug**: `a_pixel_round_trips_through_the_projection`
unprojects with `pr[0][0] * v[0] / depth`, which is not an inverse projection, and gets
`(-2.9e-17, -3.9e-17)` where it expects `(320, 240)`. Left failing rather than papered over with a
looser tolerance; whoever writes `REN-GPU` needs a real inverse matrix and this test becomes its
specification.

`viewport3` / `viewport_state` / `panels3` are still undeclared and still uncompiled — that is the
next piece of work, and it needs `ui-egui`'s module list edited, which is mine to do.

### UI 3D modules: declared, found broken, reverted

`crates/ui-egui/src/{viewport3.rs, viewport_state.rs, panels3/**}` were declared in `lib.rs` so they
would finally compile. They do not. Errors found on first compilation, none of which is a typo I
could fix inside the budget:

- `viewport3.rs`: `egui::UV_TOP_RIGHT` does not exist in this egui version.
- `viewport_state.rs`: `std::f64::consts::SQRT_3` is behind the unstable `more_float_constants`
  feature.
- `panels3/parameters.rs`: two conflicting `Default` impls for `ParameterPanel`.
- `panels3/feature_tree.rs`: `.clicked()` / `.double_clicked()` called on `InnerResponse`, which
  does not have them; a `Color32: From<Pos2>` conversion that cannot exist.

**The declarations were reverted.** Declaring them breaks the compilation of `cadcraft-ui-egui`,
which is the 2D application — 157 tests that pass. Trading that for three modules that do not
compile is not a trade worth making. The files stay on disk as `CODE WRITTEN` and undeclared, which
is exactly what "not integrated" means.

Separately, and this one is fixed: the engine agent added four commands
(`New Solid Model`, `Set Parameter`, `List Parameters`, `Model Info`) whose Ukrainian rows were
missing, which the crate's own i18n test caught. `uk.tsv` now has them and **ui-egui is back to
157 passed, 0 failed**.

A warning that matters more than the errors: **the i18n test only failed because I ran the suite.**
Nothing in the pipeline would have told me those four commands shipped untranslated.

---

## 2026-10-16 — HANDOFF: the CI era begins

**Read this section first if you are resuming.** The working method changed completely and the
old sections below describe how it used to work.

### What changed

The repository is now also pushed to **https://github.com/emilio-n8n/test-personal** (remote `mine`),
which the owner controls and can push to. `storytold/cadcraft` (remote `origin`) is somebody else's
project and returns **403** for this account — the work cannot be pushed there.

**GitHub Actions is the compiler now, not this VM.** `.github/workflows/agent-check.yml` runs on
every push: fmt, clippy `-D warnings`, `cargo check --workspace`, one runner per crate for the 12
crates with tests, the application shell on its own runner, and the three non-cargo gates
(`xtask layers`, `xtask assets`, `xtask wasm`).

### The rule that follows from it

**Do not compile locally to verify work.** Push, then read the Actions result. The local terminal is
for file work and git only. This was the whole point of getting a writable remote: the previous
bottleneck was that verification was serial on two cores.

### The agent flow, and its one gap

Agents have **no terminal**, so they cannot push. The cycle is:

```
agents write files  ->  orchestrator commits and pushes  ->  GitHub Actions judges  ->  fix or accept
```

That means the orchestrator must keep pushing in batches. Nothing an agent writes is durable until
it is committed and pushed, and nothing is known to compile until CI has run on it.

### Agents currently in flight (all on FREE models, per the owner's budget)

| Agent | Task | Owns |
|---|---|---|
| weld | `KER-BOOL-004` cell-straddle in `weld_edges` | `crates/solid/src/solid.rs` |
| union | `KER-BOOL-002/003` shared plane + order | `crates/solid/src/boolean.rs` |
| step | `DAT-003` independent-reader failure | `crates/solid/src/step.rs`, `tests/step_interop.rs` |
| ui | compile the 3 UI modules | `crates/ui-egui/src/{viewport3,viewport_state,panels3/**}` |
| nurbs | `KER-SRF-001` NURBS curves | new `crates/geom/src/nurbs.rs` |
| sketch | `SKT-001` constraint-derived sketch | new `crates/model/src/sketch.rs` |
| asm | `ASM-001/002` components + joints | new `crates/asm/**` |

**Pending orchestrator work when they return:** add the `mod` lines they must not add themselves
(`crates/geom/src/lib.rs`, `crates/model/src/lib.rs`, `crates/ui-egui/src/lib.rs`) and the crate
registration for `crates/asm` (root `Cargo.toml` + `xtask/src/layers.rs`), then push and read CI.

### Models

Free models only: `opencode-go/cat-jam-free` and `opencode-go/longcat-2.5-preview-free`. The owner
has almost no credits; `opencode-go/deepseek-v4.1-flash` is a genuine last resort, not a default.
`opencode/cat-jam-free` does not exist — the free ones are on the `opencode-go` provider.

### State of the tree at this commit

- `crates/solid`: 82 lib tests passing, 4 failing; property 9/3, property_surface 7/10, fuzz 8/2,
  corpus 10/3, **step_interop 0/11**
- `crates/render`: 116 passing, 1 failing (a bug in the test's own unprojection maths)
- `crates/ui-egui`: 157 passing, 3 modules undeclared because they do not compile
- Wave 1 is `CODE WRITTEN`, **0 `VALIDATED`**
- 952 tasks specified in `docs/plan/`, 632 `READY`

### Honest expectation, recorded so nobody is misled later

A Fusion 360-class system is a multi-year, multi-engineer programme. Seven free-model agents and a
CI pipeline do not change that. What they change is that work can now be *judged* at a rate this VM
could never achieve. The plan's own weights stand: the kernel is 20 %, and NURBS — the gate on
assemblies, drawing, simulation and CAM — is not started.

### First CI map, per crate — run #2 (commit 2a78b01)

The pipeline works and it judges in parallel. **Green:**

```
tests (cadcraft-geom)        tests (cadcraft-constraints)   tests (cadcraft-doc)
tests (cadcraft-io)          tests (cadcraft-dxf)           tests (cadcraft-dwg)
tests (cadcraft-fonts)       tests (cadcraft-color)         tests (cadcraft-mcp)
gates (layers)               gates (assets)                 wasm
```

**Red:** `fmt`, `clippy`, `check`, `tests (cadcraft-ui-egui)` (`app`), `tests (cadcraft-render)`,
`tests (cadcraft-solid)`, `tests (cadcraft-model)`.

So the entire 2D application and its supporting crates are green on a clean machine, the layering and
asset gates pass, and the wasm build passes. **Every red job is either the 3D work in progress or a
workspace-wide format/lint pass that the 3D work breaks.**

That is the map to work from. It also proves the thing this session was for: 12 crates judged in
parallel, on GitHub's runners, without touching this VM's two cores.

### CI found a real defect in the new NURBS code (run #8)

`geom` went from green to red the moment `nurbs.rs` was declared public, which is the pipeline
doing its job on brand-new code. Locally reproduced:

```
cargo test -p cadcraft-geom
  nurbs::nurbs3d_tests::derivative_matches_finite_difference FAILED
  crates/geom/src/nurbs.rs:1182: x 1.1875 vs 1.812500000042405
  test result: FAILED. 123 passed; 1 failed
```

The analytic `derivative(u, 1)` disagrees with a central difference of `eval` by ~0.6 on a cubic —
that is not a tolerance, it is a wrong derivative for a degree-3 curve. The degree-1 and degree-2
cases pass, so the fault is in the de Boor derivative path for degree >= 3 (or in what the second
argument means: `derivative(u, order)` is being called with `1`, and if the implementation reads it
as an order rather than a repeating count the result is a different quantity entirely).

**Not fixed here** — the fix needs the derivative rewritten against a known-good curve and checked
against finite differences at several degrees, which is a task for a fresh agent with budget.
Everything else in `geom` passes: 123 of 124.

Also worth noting: the GitHub logs API returns **403 for this account** (`Must have admin rights`),
so job logs cannot be read from here. Only the job-level pass/fail list is visible. To see an error
message, reproduce it locally on the crate in question — which is exactly what the local terminal is
still for when CI cannot be interrogated.
