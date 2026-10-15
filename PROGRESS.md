# PROGRESS — 3D parametric CAD for CADCraft

**Branch:** `feature/3d-parametric` (forked from `main` at `fc4e983` "Release 0.7.0")
**Goal:** add a working 3D parametric CAD area (sketch → dimension → extrude → cut → recompute →
save/reopen → STL/STEP) to CADCraft **without degrading anything 2D**.

> This file is the durable handoff record. Update it at every milestone, before committing.
> Last updated: Phase 0 (baseline) — see "Status" below.

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
- rustc/cargo **1.99.0** installed under `/tmp/opencode/rustup` + `/tmp/opencode/cargo`
  (`rustup toolchain install stable --profile minimal --component rustfmt,clippy`), plus the
  `wasm32-unknown-unknown` target. Re-installable from `logs/rustup.log`.
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
| 2 | `cadcraft-solid`: planar-faced B-rep kernel | next |
| 3 | `cadcraft-model`: feature tree, recompute, projects | not started |
| 4 | Engine commands, CLI, MCP | not started |
| 5 | 3D viewport + tree/parameter panels | not started |
| 6 | STEP/OBJ export, revolve, docs | not started |

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

## Test evidence

| Command | Result |
|---|---|
| `cargo test -p cadcraft-geom` | **108 passed**, 0 failed (was 84) |
| `cargo test -p cadcraft-constraints` | **36 passed**, 0 failed |
| `cargo clippy -p cadcraft-geom --all-targets` | clean |
| `cargo fmt -p cadcraft-geom` | applied |

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

## Known failures / limitations

- OpenCASCADE: absent from the machine, unbuildable here on 2 cores, LGPL (forbidden by
  `AGENTS.md`), and its bindings require `unsafe` (workspace forbids). Not used.
- The only 2D boolean in the repo (`engine/src/cmd/inquiry/regions.rs`) fails when region
  boundaries cross or touch — the Phase 1 engine in `geom` is meant to replace it.

## Next actions

1. Phase 0: run the baseline test suites, record timings, write `PROGRESS.md`, commit.
2. Phase 1: `crates/geom/src/bool2d.rs`.

## Resume instructions

```bash
cd /home/emilio_developpeur/cadcraft
git switch feature/3d-parametric          # durable: persistent home PD
. /tmp/opencode/env.sh                    # re-create if the VM restarted (re-downloadable)
git log --oneline -5                      # last committed milestone
cargo test -p cadcraft-geom               # then resume at "Next actions" above
```