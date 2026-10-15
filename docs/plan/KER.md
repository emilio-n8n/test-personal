# `KER` — geometry kernel task index

**Owner:** the kernel agent(s) working under `crates/solid/**`.
**Status:** authoritative work breakdown for the `KER` domain of `docs/master-plan.md`.
**Compiled / tested:** nothing here has been compiled or run. This is a specification document.

This file expands the ~20 seed tasks of `master-plan.md` §5 (`KER-BOOL`, `KER-SRF`, `KER-FEAT`,
`KER-TOL`) plus the two the kernel needs for scale (`KER-PERF`) and stability (`KER-API`) into
**222 individually assignable tasks**. Every task carries its state, its dependencies, the exact
files it may own, the exact Rust signature it must produce or consume, the inputs it must refuse,
the tests it must write, the condition that ends it, and the one thing most likely to go wrong.

Nothing else edits this file. The orchestrator owns it. An agent handed one task from here edits
`crates/solid/**` and nothing else.

---

## 0. Conventions and invariants — inherited by every task below

These are not repeated per task except where a task overrides them. A task that contradicts this
section is wrong; a task that cannot satisfy it is `BLOCKED` and says so in its **Risk** field.

### 0.1 The error contract

> An operation that cannot produce valid geometry returns `Err` naming the reason. It never returns
> a silently invalid result and it never panics.

- Every public entry point returns `Result<T, SolidError>` (or `Option`/`Vec` for genuinely
  optional queries). No public entry point may panic, index out of bounds, or produce NaN.
- `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!` are forbidden in
  non-test code. Every crate root keeps
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
  and `#![forbid(unsafe_code)]`.
- **Every task in this file has a Refusals field.** It is not optional and not a formality: it is
  the specification of the task's error surface. A task whose Refusals read "none" is incomplete
  and must not be handed out.
- An error message must name *what* and *where* in numbers a human can act on
  (`"edge (0.125, -0.281, 0.097) -> ... is used by 1 face"`, not `"boolean failed"`).
- The error *variant* must be stable and matchable (`matches!(e, SolidError::Empty)`), because
  callers (`MOD`, `ASM`, the engine) branch on it. See `KER-API-003`.

### 0.2 Tolerance contract

- No geometric predicate uses exact floating-point equality. Every coincidence, tangency and
  coplanarity test takes an explicit tolerance.
- Tolerance scales with the geometry. The current rule is
  `tolerance_for(diagonal) = (diagonal * 1e-9).clamp(1e-12, 1e-3)` (`crates/solid/src/lib.rs`).
  A task that needs a different rule replaces the rule in one place (`KER-TOL-001`), never inline.
- Angular comparisons use their own tolerance from the same policy (`dir_tol`, currently `1e-9`).
  Mixing a length tolerance into a direction comparison is a defect.
- A feature thinner than the working tolerance is not resolved. That is a *refusal with a named
  reason*, not a silently wrong solid. See `KER-TOL-047`.

### 0.3 Orientation and winding — every task that touches topology

These are stated in the task bodies as well, because a task handed to an agent with no other
context must carry them:

- **Loops in a face's own frame**: outer loop **counter-clockwise**, every hole **clockwise**.
- **Material on the left** of every directed boundary segment, when walking a loop.
- **Face normal points outward**, away from the material. For a planar face the normal is the
  plane's normal and the loop is CCW *in the plane's own `u,v` frame* (`u × v = normal`), which is
  the statement that makes the two agree.
- **Positive signed volume** means "outward". A shell that is closed and consistently oriented but
  reports a negative volume is inside-out and is an error (`SolidError::NonPositiveVolume`), never
  a body.
- A face carried from one frame into another via a *reflection* must have its loops reversed to
  restore the convention; a face carried by a rotation must not. See the existing
  `Face::retargeted` / `Face::flipped` / `reverse_regions` for the established form.
- For a general surface, the normal is `∂S/∂u × ∂S/∂v` normalised, and **the UV frame is chosen so
  that this points outward**. The kernel therefore owns the UV frame of every surface it builds
  (`KER-SRF-018`, `KER-SRF-019`): a caller may not choose a UV frame that inverts the normal.
- Every operation ends in validation. A result that fails `validate()` or `validate_vertices()` is
  an `Err`, not a solid (`KER-BOOL-041` gate is the reference implementation).

### 0.4 Scale-free shape definition

A shape is defined by ratios and an anchor, then built at any size, so that "the same shape at
1e-6, 1 and 1e6" is a well-defined statement. The reference generator is
`crates/solid/tests/property.rs` (`struct Shape`, `struct Case`, `enum Regime`). New tests reuse
that vocabulary rather than inventing a third one.

### 0.5 File ownership (anti-collision, from `master-plan.md` §6)

| Path | Owner | Note |
|---|---|---|
| `crates/solid/src/**` | `KER` | the only thing a `KER` task may create or edit |
| `crates/solid/src/step.rs` | `D` (data/interchange) | `KER` never touches it |
| `crates/solid/tests/**` | `Q` (quality) | see below |
| `crates/solid/benches/**` | `KER-PERF` | created by `KER-PERF-001` |
| `xtask/**`, `crates/*/tests/**` | `Q` | `KER` never touches it |
| every other crate | another domain | `KER` never touches it |

**Consequence that applies to every task in this file:** a kernel task's tests live in the
`#[cfg(test)] mod tests` block of the file it owns. That is the existing convention in this crate
and it is what keeps wave-1 ownership clean. A task that wants a *property* or *integration* test in
`crates/solid/tests/**` is `BLOCKED` on a `QAT` assignment and must, until then, put a
deterministic named regression test in its own module instead. Tasks that need this say so in
their **Files** field.

### 0.6 Task state (as used in this file)

| State | Meaning here |
|---|---|
| `READY` | every prerequisite task is `INTEGRATED`, the files are unowned by anyone else, the contract is written. Hand it to an agent. |
| `BACKLOG` | specified, prerequisites not yet satisfied, but nothing is missing at the *contract* level. Ordered work. |
| `BLOCKED` | a named prerequisite is missing. The task names which one. Do not hand it out. |

`IN PROGRESS`, `CODE WRITTEN`, `INTEGRATED` and `VALIDATED` (master-plan §2) are orchestrator-tracked
and deliberately do not appear in the **State** field below; an agent never writes them.

### 0.7 Cross-domain rules — no invented shared types

- A type that crosses a domain boundary is **invented here, once, in the kernel**, and every other
  domain consumes it. The kernel does not adopt another domain's type and does not define a second
  version of it. The three exceptions, where the type already exists and must be reused as-is:
  `Vec2`, `Vec3`, `Bounds2`, `Bounds3`, `Mat3`, `Mat4`, `Region`, `Regions`, `Spline`
  (all from `cadcraft_geom`, crate L0) and the existing `Plane`, `Solid`, `Mesh`, `SolidError`.
- Where another domain needs something the kernel does not yet export, the task says
  **CONTRACT: needs `KER-…-nnn`** and states the required signature. It does not define the type
  in place. §4 lists every such contract.
- **The kernel does not wrap `geom::Spline`.** That type is 2D, DXF-spline-shaped and serde-
  serialised for the document model. The kernel's 3D rational curve is a distinct type
  (`KER-SRF-001`) and must convert to and from it explicitly, never alias it.
- The kernel emits no JSON, opens no dialog, and knows nothing about commands, files or sessions.
  Those are `UI` / `DAT` / `engine`.

### 0.8 Determinism

Two runs of the same operation on the same inputs produce the same output *byte for byte*:
same face order, same vertex order, same loop order. Every collection that reaches an output is
sorted by an explicit total order derived from the geometry (`f64::total_cmp`, never `HashMap`
iteration order). `KER-BOOL-036` and `KER-API-014` are the tests that pin this; every task that
introduces an unordered collection is `BLOCKED` on reading them.

### 0.9 What a task report must contain

Task ID · outcome · files created or modified · implemented behaviour · tests added with
`NOT EXECUTED` stated · unverified assumptions · interface changes requested · blockers · next
tasks. (`docs/agents/SYSTEM_PROMPT.md` §7.) A report that says "verified" for code nobody compiled
is worse than no report.

---

## 1. Task counts

| Epic | Tasks | IDs | Entry point |
|---|---|---|---|
| `KER-BOOL` | 40 | `KER-BOOL-001..040` | boolean algebra, robust arrangement, degenerate matrix, scale |
| `KER-SRF` | 50 | `KER-SRF-001..050` | NURBS curves/surfaces, analytic surfaces, intersection, trimming, sew/knit/heal, tolerance model |
| `KER-FEAT` | 42 | `KER-FEAT-001..042` | sweep, loft, shell, draft, thicken, face ops, variable fillet, corner blends, patterns, threads |
| `KER-TOL` | 30 | `KER-TOL-001..030` | distance fields, section curves, curvature, interference |
| `KER-PERF` | 32 | `KER-PERF-001..032` | spatial indexing, large-model budget, incremental rebuild support |
| `KER-API` | 28 | `KER-API-001..028` | the public surface: stable, versioned, internal |
| **Total** | **222** | | |

States at the time of writing: 41 `READY`, 158 `BACKLOG`, 23 `BLOCKED`. The `BLOCKED` ones each
name their prerequisite in **Depends on**; the count is reproduced in §3.

---

## 2. The dependency graph and the critical path

### 2.1 Critical path

The longest chain in the kernel. Nothing on it can be parallelised away; every arrow is a hard
interface dependency, not a preference.

```
KER-API-002  (kernel API versioning — decides what a stable public item means)
  └─ KER-SRF-002  (knot-vector validation and normalisation)
      └─ KER-SRF-003  (de Boor evaluation, 3D rational)
          └─ KER-SRF-005  (rational weights; the zero-denominator refusal)
              └─ KER-SRF-015  (Curve3 enum + dispatch + trim-by-parameter)
                  └─ KER-SRF-018  (Surface enum — the UV frame rule)
                      └─ KER-SRF-023  (NurbsSurface evaluation + normal)
                          └─ KER-SRF-033  (BFace: surface + pcurve-trimmed boundaries)
                              └─ KER-SRF-035  (plane × surface)
                                  └─ KER-SRF-039  (NURBS × NURBS refinement)
                                      └─ KER-SRF-042  (pcurve on the second face)
                                          └─ KER-SRF-043  (sew)
                                              └─ KER-SRF-044  (knit: orientation propagation)
                                                  └─ KER-SRF-046  (per-vertex/edge/face tolerance model)
                                                      └─ KER-SRF-048  (Brep container)
                                                          └─ KER-SRF-049  (Face migrates to Surface)
                                                              └─ KER-FEAT-019  (shell on a general Brep)
                                                                  └─ KER-FEAT-042  (every feature validates)
```

17 hops. `KER-SRF-018` and `KER-SRF-046` are the two most expensive nodes on it: the `Surface`
enum fixes the UV-frame rule that every later task obeys, and the tolerance model fixes the
representation that sew, loft, shell and thickness all read. Neither can be deferred without
re-opening work behind it.

### 2.2 Second path — the three open defects (wave 1)

```
KER-BOOL-001  (revolve closes its shell)
  └─ KER-BOOL-002  (union across a shared plane welds)
      └─ KER-BOOL-003  (union is order-independent)
          └─ KER-BOOL-004  (vertex welding on the canonical grid, cell-straddle documented)
              └─ KER-BOOL-011  (named error variants for the degenerate matrix)
                  └─ KER-BOOL-040  (close-out: all three defects un-ignored, matrix green)
```

### 2.3 Third path — scale

```
KER-PERF-003  (uniform grid index)
  └─ KER-PERF-008  (index kind heuristic)
      └─ KER-PERF-010  (index prunes the boolean's plane loop)
          └─ KER-BOOL-005  (no quadratic coplanarity scan)
              └─ KER-BOOL-039  (boolean at 10^5 faces within budget)
```

### 2.4 Where the three paths meet

`KER-BOOL-040` (boolean validated) is the gate for `KER-FEAT-001..018` in master-plan wave 2.
`KER-SRF-049` (Face migrated) is the gate for `KER-FEAT-019`, `KER-FEAT-022`, `KER-TOL-027` and for
every other domain that consumes a `Brep`. Until `KER-SRF-049` lands, the whole `KR-FEAT` tail,
`KER-TOL-015`, `KER-TOL-027` and all of `§4`'s cross-domain contracts are `BLOCKED`.

---

## 3. Epic `KER-BOOL` — boolean algebra, arrangement, degenerate cases, scale

Covers `master-plan.md` `KER-BOOL-001..006` at 001–006 verbatim, then expands the degenerate-case
matrix into 35 individually assignable pieces.

**Orientation inherited for this epic:** outer loop CCW, holes CW in the face's own plane frame;
material on the left; face normals outward; positive signed volume. Every result ends in
`Solid::validate()`.

### KER-BOOL-001 — Close `revolve`'s shell at the cap seam

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/extrude.rs` (tests in its `#[cfg(test)] mod tests`). Must not touch
  `crates/solid/tests/**` (QAT-owned).
- **Contract:** keep the existing signature unchanged.
  `pub fn revolve(profile: &Regions, plane: &Plane, axis: Vec3, angle: f64, segments: usize) -> Result<Solid, SolidError>`
  Fix the private helper to emit all three vertices:
  `fn triangle_face(a: &Vec3, b: &Vec3, c: &Vec3) -> Option<Face>`
- **Behaviour:** `triangle_face` currently builds `Region::new(vec![plane.to_local(*b), plane.to_local(*c)])`
  — a two-point region, which `Region::is_empty` reports as empty, so `Face::new` rejects it and the
  `.ok()` swallows the error: every swept patch is dropped and `revolve` never reaches a solid.
  Build the region from all three vertices, `vec![Vec2::ZERO, plane.to_local(*b), plane.to_local(*c)]`,
  which places `a` at local `(0,0)` because the plane's origin is `a`. Then make the partial-turn
  caps meet the last ring of swept patches at the seam: the final patch runs from
  `theta(segments-1)` to `theta(segments)`, and `theta(segments)` is exactly the far cap's plane,
  so the loop must stop there and never run one step further. A full turn has no caps and must
  close on its own.
  **Orientation:** the start cap's normal is the turned `plane.normal × sense`, the far cap's is
  the other way round, where `sense = sign(plane.v() · axis)`; a cap's loop is CCW in the cap's own
  frame, so the caps bound the material and the swept walls carry the material on their left.
  Finish with `weld_edges(faces, tol)` before `orient_faces()` and `validate()`, so the cap seam is
  welded rather than merely coincident.
- **Refusals:** profile with no area → `SolidError::EmptyProfile`; `angle <= 0` or non-finite →
  `BadDistance`; `segments == 0` or `> MAX_ROUND_SEGMENTS` → `Invalid` naming both numbers; axis
  zero or non-finite → `BadDirection`; a profile plane that does not contain the axis
  (`plane.v().cross(axis).len() > 1e-9`) → `Invalid("the revolve plane must contain the axis")`;
  a result that does not close after welding → `OpenEdge(a, b, n)`; a result with negative signed
  volume → `NonPositiveVolume(v)`.
- **Tests:** in `extrude.rs`, un-`#[ignore]` the existing `revolve` cases and add:
  `revolve_full_turn_builds_a_closed_tube` (volume equals the n-gon annulus formula to 1e-9,
  face count `4 * segments * 2`, `validate()` ok); `revolve_partial_turn_caps_meet_the_swept_walls`
  (quarter turn of a 1×1 rectangle 1 off the axis: 66 faces, volume one quarter of the full turn,
  no `OpenEdge`); `revolve_about_z_uses_the_plane_frame_not_an_axis_letter` (the same solid from
  `Plane::yz(0.0)`/`+Z`); `revolve_a_profile_touching_the_axis_makes_a_cone` (volume
  `ngon_area(8,1)/3`, `validate_vertices()` ok — a cone apex is a vertex, not a pinch);
  `revolve_refuses_bad_input` (all six refusals above, each `matches!` on the variant).
- **Acceptance:** `revolve_builds_no_solid` in `crates/solid/tests/property.rs` passes with no
  `#[ignore]`, the defect-3 comment block in that file is updated to say the defect is closed, and
  the `revolve` entry in the file's module doc no longer says "currently refuses every input".
- **Risk:** the seam may still leave a vertex unwelded where the cap plane and the last patch meet
  at a non-axis-aligned angle, because `weld_edges` only splits an edge at a vertex that is
  *strictly inside* it. **Fallback:** if welding is insufficient, compute the cap loop directly
  from the swept ring at `theta(segments)` rather than from the unrotated profile, so the two are
  built from the same numbers by construction.

### KER-BOOL-002 — Weld the union junction across a shared plane

- **State:** READY
- **Depends on:** KER-BOOL-001 (same module sweep, must not regress `revolve`), KER-BOOL-004
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/src/solid.rs`
- **Contract:** unchanged public signature
  `pub fn boolean(a: &Solid, b: &Solid, op: Bool3) -> Result<Solid, SolidError>`.
  Add `pub(crate) fn slot_regions(slot: &Slot, sa: &Regions, sb: &Regions, op: Bool3) -> Option<(Regions, Option<(Regions, Plane)>)>`
  if the case table is extracted; the extraction is optional and internal.
- **Behaviour:** two solids touching across a shared plane on **opposite** sides, whose footprints
  do not coincide, currently drop the whole plane and return an open shell: `(Union, true, true,
  false)` answers `Regions::default()` on the grounds that the plane is interior, but it is interior
  only where the two overlap. The face on that plane is exactly the union of each operand's
  exposed part, and the two parts face **opposite** directions, so they cannot be one planar face.
  Emit two faces: `bool2d::union(&sa, &sb)` carried into `slot.plane`, and `bool2d::union(&sb, &sa)`
  — no: `difference` — carried into `slot.plane.flipped()`, because that half's material faces the
  other way. The mirrored frame requires `x → -x` remap, exactly as the existing block does.
  **Orientation:** each half is wound CCW in *its own* plane frame, so its normal points away from
  the material; the union shell therefore has the shared plane appearing twice with opposite
  normals, one bounding each lobe, and the exposed faces around the seam line up.
- **Refusals:** the two halves' union regions both empty (nothing exposed) → the plane is not a
  face at all and no face is emitted (not an error); the resulting shell still reports
  `OpenEdge(a, b, 1)` → propagate; the un-mirrored-region mistake surfacing as a face whose signed
  area is negative → must not be emitted, caught by `orient_faces()`/`validate()` and reported as
  `Invalid`.
- **Tests:** in `boolean.rs`, extend the case table test with
  `union_of_two_blocks_stepped_over_a_shared_plane` — `box_at(0,0,0)-(2,2,1)` and
  `box_at(1,0,1)-(3,2,2)` union to a 10-face L-prism of volume 12 to 1e-9, `validate()` and
  `validate_vertices()` ok; `union_of_a_partial_overhang_leaves_both_lobes_exposed` (a box
  `(0,0,0)-(4,4,1)` and a smaller box `(1,1,1)-(3,3,2)`: the larger box's top face keeps an
  L-shaped ring of area `16 - 4 = 12`); `subtracting_a_tool_that_touches_only_leaves_the_face`
  (regression: the opposite-sides difference case must still return the original face, not a
  merged one).
- **Acceptance:** `union_of_two_bodies_touching_across_a_plane` in `crates/solid/tests/property.rs`
  passes with no `#[ignore]`, including its five-row table, and the misleading comment in
  `boolean.rs`'s module doc ("Coplanar faces fall out for free") is corrected.
- **Risk:** the two halves may share a vertex that neither half's own region contains, so welding
  alone cannot join them. **Fallback:** run `weld_edges` twice — once before assembling the halves,
  once after — and if the shell still reports a one-use edge, emit the shared plane's face pair as
  a single face with an internal slit only if `validate()` accepts it; otherwise return
  `SolidError::Invalid` naming the seam endpoints rather than shipping an open shell.

### KER-BOOL-003 — Make the union independent of the operand order

- **State:** READY
- **Depends on:** KER-BOOL-002, KER-BOOL-004, KER-BOOL-031
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged public signature
  `pub fn boolean(a: &Solid, b: &Solid, op: Bool3) -> Result<Solid, SolidError>`;
  internal `fn plane_slots(a: &Solid, b: &Solid, tol: f64, dir_tol: f64) -> Result<Vec<Slot>, SolidError>`
  whose output order is a function of the geometry only, never of the argument order.
- **Behaviour:** on seed `0xc0ffee42`, case 66, `union(a,b)` returns a valid 13-face solid while
  `union(b,a)` refuses the same pair with `InconsistentFaces`. The offending edge is the first step
  down `b`'s bottom cap at the corner `b.min.x, b.max.y, b.min.z`, which the union splits but does
  not weld. `Difference` is *not* commutative and must not be made so; `Union` and `Intersect`
  must both be. The fix has two halves: build the plane-slot list in a total order derived from
  the plane geometry itself (see `KER-BOOL-031`), and run `weld_edges` over the finished face list
  rather than only over each face's own neighbourhood.
  **Orientation:** unchanged — each face keeps outer CCW / holes CW in its own plane frame, and the
  result's outward direction is settled by `fix_orientation()` on the measured volume, never by
  tracking which operand a plane came from.
- **Refusals:** operands that overlap but produce an empty intersection → `Empty`; a slot list
  above `MAX_FACES` → `TooManyFaces(n, MAX_FACES)`; a result that still fails `validate()` →
  the specific `OpenEdge` / `InconsistentFaces` / `NonPositiveVolume` variant, propagated verbatim;
  if one order produces a solid and the other an error, that is a **test failure**, not an error
  to report — the property is `union_is_commutative`.
- **Tests:** in `boolean.rs`, `union_is_independent_of_operand_order_for_boxes_and_prisms` — for
  every pair drawn from the existing `Case`/`Regime` vocabulary with `Regime::Overlap`,
  `boolean(&a,&b,Union)` and `boolean(&b,&a,Union)` either both refuse with the **same** error
  variant or both succeed with equal face count and the same vertex multiset to
  `1e-12 * diagonal`; `intersect_is_independent_of_operand_order` (same, plus volumes equal to
  1e-12 relative); `difference_is_not_commutative_and_does_not_need_to_be` (asserts
  `vol(a\b) != vol(b\a)` for an asymmetric pair, to stop a future agent "fixing" it into a bug);
  `a_boolean_is_deterministic_given_the_same_operands` (run twice, assert identical face count and
  identical vertex multisets).
- **Acceptance:** `union_does_not_depend_on_the_operand_order` and `union_is_commutative` in
  `crates/solid/tests/property.rs` pass with no `#[ignore]`, and `union_is_commutative`'s doc
  comment no longer says "this test currently fails".
- **Risk:** making the slot order total may expose a *different* latent asymmetry that was masked by
  the plane list's incidental order. **Fallback:** if the property still fails after the ordering fix,
  do not chase the seed; reduce the failing pair by hand to the smallest two solids that
  reproduce it, add that pair as a named unit test, and report the reduction as the deliverable.

### KER-BOOL-004 — Vertex welding on the canonical and tolerance grids

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/solid.rs`
- **Contract:** keep
  `pub fn weld_edges(faces: &mut [Face], tol: f64) -> usize`
  and add the documented failure form as a second entry point:
  `pub fn weld_edges_checked(faces: &mut [Face], tol: f64) -> Result<usize, SolidError>`.
  `KER-BOOL-008` re-exports both.
- **Behaviour:** the same corner arrives at two faces by different routes and lands ~1e-7 apart;
  snapping to a `tol`-sized grid and replacing each vertex by the canonical point of its cell is
  what makes the edges match. Phase 2 splits each face edge at any vertex that lies **strictly
  inside** it *and in the face's own plane* (`plane.distance(v).abs() <= tol`), which is what stops a
  box sprouting midpoints along every edge when an off-plane vertex projects onto it. The task is to
  make this the documented, tested, reusable contract and to **write down the cell-straddle case**:
  two points farther apart than `tol` but landing in the same cell are welded together, and two
  points within `tol` but landing in different cells are not. Document both in the function's doc
  comment with a worked example, and make the choice deliberate — snap to the *canonical grid* with
  the cell's minimum as the canonical point, not "first vertex wins", so the result does not depend
  on face order.
- **Refusals:** `tol <= 0` or non-finite → the function treats it as `1e-12` and says so in the
  doc, but `weld_edges_checked` returns `BadTolerance(tol)`; a vertex count above
  `MAX_WELD_VERTICES` → `weld_edges_checked` returns
  `Invalid("{n} vertices to weld, the limit is {MAX_WELD_VERTICES}")` instead of silently skipping,
  which is what `weld_edges` does today; a cell-straddle detected with `weld_edges_checked` →
  `Invalid("vertex {p:?} welds to {q:?}, {d} apart across a grid cell")`.
- **Tests:** in `solid.rs`, keep the three existing `weld_tests` and add
  `welding_is_independent_of_face_order` (weld the same face set in two permutations, assert the
  same canonical vertex set); `welding_changes_no_area_or_volume` (already present — keep it, it is
  the guarantee that matters); `a_point_just_across_a_cell_boundary_does_not_straddle_when_it_can_be_avoided`
  (two points `tol * 1.01` apart whose midpoint lands on a cell edge: assert they weld into two
  cells, and document why); `welding_above_the_cap_is_refused_not_skipped`.
- **Acceptance:** the doc comment on `weld_edges` contains the cell-straddle paragraph with a
  numeric example; `KER-BOOL-002`, `KER-BOOL-003` and `KER-BOOL-008` are unblocked; the property
  test `boolean_welding_never_moves_geometry_and_is_idempotent` in `property_surface.rs` (QAT-owned,
  already written) compiles and passes unmodified.
- **Risk:** choosing "cell minimum as canonical" changes every existing welded result by up to `tol`,
  which may move a volume past a 1e-9 assertion in `property.rs`. **Fallback:** keep the current
  first-writer-wins rule but sort faces by a geometry-derived key before welding so the writer is
  deterministic; document the tie-break rule rather than changing the canonical point.

### KER-BOOL-005 — Boolean without a quadratic coplanarity scan

- **State:** BLOCKED
- **Depends on:** KER-PERF-003, KER-PERF-008, KER-BOOL-031
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/src/index.rs` (new, created by
  `KER-PERF-003`)
- **Contract:**
  `pub fn plane_slots(a: &Solid, b: &Solid, tol: f64, dir_tol: f64) -> Result<Vec<Slot>, SolidError>`
  (moved from a private function to module-private `pub(crate)`; `Slot` stays private).
  Consumes `cadcraft_solid::index::SpatialIndex`:
  `pub fn planes(&self) -> Vec<Plane>`,
  `pub fn plane_pairs_near(&self, pad: f64) -> Vec<(usize, usize)>`.
- **Behaviour:** the slot loop today is `O(F_a × F_b)` on `Slot::coplanar`, which at 10⁵ faces is
  10¹⁰ comparisons. Build the spatial index over both operands' face planes once, query candidate
  pairs whose planes come within `pad = 4 * tol` of each other, and only run the exact coplanarity
  test on candidates. A plane with no candidate neighbour forms its own slot without being compared
  against every other plane. **Orientation:** slot construction must not change any face's winding;
  the merge rule for two coplanar faces in the same slot is unchanged from `KER-BOOL-002`.
- **Refusals:** a plane pair that is within `pad` but not actually coplanar → not merged, no error;
  an index that would hold more than `limits::MAX_INDEX_ITEMS` planes → `TooManyFaces(n, MAX_FACES)`;
  a plane with a non-finite origin or normal → `NonFinite`; a face count above `MAX_FACES` on either
  side → `TooManyFaces`.
- **Tests:** in `boolean.rs`, `plane_slot_count_is_independent_of_operand_order` (the same two
  solids produce byte-identical `plane_slots` output in either order — this is the unit test that
  `KER-BOOL-003` depends on); `indexed_plane_search_matches_the_quadratic_one` (for every pair drawn
  from the `Case` vocabulary, the indexed slot list equals the naive one element-for-element);
  `a_union_of_two_large_coplanar_faces_merges_them` (regression for the behaviour being preserved).
- **Acceptance:** `KER-BOOL-039` is unblocked and the slot-construction cost on a 10⁴-face operand
  pair is below the budget recorded in `KER-PERF-001`.
- **Risk:** an index that only buckets by plane *position* will miss planes that are coincident but
  far from any sample point. **Fallback:** fall back to the quadratic scan when the index reports
  more than `MAX_FACES / 4` candidate pairs for a single plane — a bounded, measured fallback beats a
  silent wrong answer, and the fallback must be counted and reported, not hidden.

### KER-BOOL-006 — The degenerate-case matrix: harness and table

- **State:** READY
- **Depends on:** KER-BOOL-001, KER-BOOL-002, KER-BOOL-003
- **Files:** `crates/solid/src/boolean.rs` — a new `#[cfg(test)] mod degenerate_matrix` with one
  named test per row of §3.1 below. This is the matrix's only home until `QAT` takes
  `crates/solid/tests/**`.
- **Contract:** no new public API. The matrix consumes
  `pub fn boolean(a: &Solid, b: &Solid, op: Bool3) -> Result<Solid, SolidError>`,
  `pub fn Solid::validate(&self) -> Result<(), SolidError>`,
  `pub fn Solid::validate_vertices(&self) -> Result<(), SolidError>`,
  `pub fn Solid::validate_intersections(&self) -> Result<(), SolidError>` (from `KER-BOOL-007`).
- **Behaviour:** the matrix is the kernel's statement that *invalid geometry is a named `Err`,
  never a solid*. Every row is (operands, operation, expected outcome, expected error variant).
  Every row asserts two things: the call returns the stated outcome, **and** any `Ok` value passes
  `validate()` + `validate_vertices()`. A row that returns `Ok` with a solid failing validation is
  the failure mode the whole matrix exists to prevent. No row may be satisfied by "it returned
  something"; a refusal must name the reason and a success must be geometrically correct (the volume
  is asserted wherever the answer is known in closed form).
- **Refusals:** the matrix *is* the refusal specification; a row that cannot produce its expected
  outcome is itself the defect the row names. Every row's expected `Err` variant is part of the
  public contract and may not be weakened to `is_err()`.
- **Tests:** the 23 rows of §3.1, one test each, named by row. `crates/solid/tests/**` counterpart
  is BLOCKED on QAT.
- **Acceptance:** every row in §3.1 has a named test; the module doc in `boolean.rs` links the
  matrix; a new row added later has a test before it is considered specified.
- **Risk:** a matrix that only exercises `Vec3::ZERO`-scale boxes will not find the cell-straddle
  or the coplanarity-boundary defects. **Fallback:** every row that is not a clean construction case
  is additionally run at `1e-6` and `1e6` scale, using the `Case` generator's draw-once-build-twice
  discipline, so a row that only fails at one scale is still found.

#### 3.1 The degenerate-case matrix

| Row | Case | Expected outcome | Error variant |
|---|---|---|---|
| D1 | coplanar faces, same side, identical footprints | one merged face, volume unchanged | — |
| D2 | coplanar faces, opposite sides, identical footprints | two faces, shell closed | — |
| D3 | coplanar faces, opposite sides, partial overlap | L-prism (the `KER-BOOL-002` case) | — |
| D4 | coplanar faces offset by exactly `tol` | treated as coplanar, not as two slabs | — |
| D5 | coplanar faces offset by `tol * 10` | treated as distinct planes, boolean succeeds | — |
| D6 | solids sharing only one edge | refusal | `Invalid` naming the shared edge |
| D7 | solids sharing only one vertex | refusal | `Invalid` naming the vertex |
| D8 | one operand strictly inside the other, union | the larger operand, unchanged volume | — |
| D9 | one operand strictly inside the other, difference | a shell with an internal void, refused | `Invalid` naming the void |
| D10 | operands equal up to `tol` | union/intersect return that operand | — |
| D11 | `a - a` | refusal | `Empty` |
| D12 | cut leaving a zero-thickness sliver (`width <= tol`) | refusal | `Invalid` naming the sliver width |
| D13 | tool's faces exactly tangent to the base's faces | refusal | `Invalid` naming the tangent plane |
| D14 | self-intersecting operand (faces overlap away from a shared boundary) | refusal before any work | `FaceOverlap(i, j)` |
| D15 | open (non-closed) operand | refusal | `OpenEdge(a, b, n)` |
| D16 | inside-out operand | refusal | `NonPositiveVolume(v)` |
| D17 | self-touching operand at a vertex | refusal | `Invalid` naming the vertex and the fan size |
| D18 | plane through an exact vertex of a face | empty section, no panic | — |
| D19 | plane tangent to a face | empty section | — |
| D20 | plane containing a face and grazing elsewhere | refusal | `Section` |
| D21 | operands at `1e-300` coordinates | refusal or correct result, never a panic | `NonFinite` |
| D22 | operands at `1e300` coordinates | refusal or correct result, never a panic | `NonFinite` |
| D23 | operand with 2 vertices in one loop | refusal | `Empty` |

### KER-BOOL-007 — Implement `Solid::validate_intersections`

- **State:** READY
- **Depends on:** KER-BOOL-004, KER-PERF-003
- **Files:** `crates/solid/src/solid.rs`
- **Contract:** add exactly the signature the crate's own module doc already promises:
  `pub fn validate_intersections(&self) -> Result<(), SolidError>`
- **Behaviour:** `crates/solid/src/lib.rs` documents invariant I5 — "two distinct faces touch only
  along shared boundary (checked by `Solid::validate_intersections`, which is quadratic and off the
  default path)" — and the function does not exist. Either implement it or delete the claim. The
  task is to implement it, because row D14 of the matrix needs it: two faces of one solid whose
  regions overlap in 2D **and** whose planes are not coplanar (or are coplanar but whose loops
  cross) are a self-intersecting shell that every other invariant passes. Test the pair's regions
  for non-empty intersection after removing their shared boundary segments; use the spatial index
  (row D14 is on the critical path to `KER-BOOL-005`) when it lands, and the quadratic scan before
  that, refusing above `MAX_WELD_VERTICES`.
  **Orientation:** unchanged; this function reads geometry and never mutates.
- **Refusals:** two faces overlapping away from a shared boundary → `FaceOverlap(i, j)`;
  non-finite plane → `NonFinite`; a face pair whose intersection cannot be decided (the planar
  boolean returns `None`) → `Invalid("faces {i} and {j} overlap and the overlap could not be
  decided")` — never a silent pass.
- **Tests:** in `solid.rs`, `two_faces_overlapping_away_from_a_shared_boundary_are_detected`
  (two squares on parallel planes offset by half a width); `touching_faces_are_not_overlapping`
  (the normal box: all 6 face pairs pass); `a_self_intersecting_shell_is_detected` (build the
  classic bowtie: two boxes unioned then one face translated inward — or directly construct the face
  set); `validate_intersections_agrees_with_validate_on_every_box_the_property_suite_builds`.
- **Acceptance:** the function exists with the documented signature, the `lib.rs` claim is no longer
  a lie, and matrix row D14 has a test that uses it.
- **Risk:** removing "shared boundary segments" robustly is itself a Boolean-2D problem and could
  consume the task. **Fallback:** implement the conservative version — report an overlap whenever
  the two regions intersect with area `> tol²`, *including* a legitimate shared boundary — and
  accept that `validate()` callers must only use it on shapes whose coplanar faces were already
  merged by the boolean. Document that restriction in the doc comment rather than pretending.

### KER-BOOL-008 — Re-export the welding entry points

- **State:** READY
- **Depends on:** KER-BOOL-004
- **Files:** `crates/solid/src/lib.rs`, `crates/solid/src/solid.rs`
- **Contract:** in `lib.rs`, extend the existing re-export line:
  `pub use solid::{Edge, Face, Solid, reverse_regions, weld_edges, weld_edges_checked};`
- **Behaviour:** `weld_edges` is `pub` inside a private module and is therefore unreachable; the
  `property_surface.rs` test says so in its own doc comment and drives welding indirectly through
  `boolean`. Once re-exported, the kernel's own contract is testable directly and `KER-PERF-014` has
  a public entry point to optimise. Add a doc line to `weld_edges` naming what it does *not* do
  (it never removes a vertex, never moves one off its edge, and never changes a region's shape).
- **Refusals:** none added; the re-export does not change behaviour. Any input refused before is
  refused identically after.
- **Tests:** in `solid.rs`, `welding_a_box_through_the_public_entry_point_is_a_no_op` (a box's face
  set welds to zero splits, same loops back); a compile-level use in the module doc example.
- **Acceptance:** the export exists; `crates/solid/tests/property_surface.rs` compiles unchanged
  (it does not yet use it — a later `QAT` task may).
- **Risk:** widening the public surface conflicts with `KER-API-007`'s stable-surface list. **Fallback:**
  if `KER-API-007` has already been done and does not list `weld_edges`, export it under
  `pub mod diagnostic` and mark it `#[doc(hidden)]`, keeping `KER-API-001`'s list accurate.

### KER-BOOL-009 — Re-export edge-reference resolution

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/lib.rs`, `crates/solid/src/chamfer.rs`
- **Contract:** `pub use chamfer::{EdgeRef, chamfer, fillet, resolve as resolve_edges};`
  keeping `pub fn resolve(solid: &Solid, refs: &[EdgeRef], tol: f64) -> Result<Vec<usize>, SolidError>`
  unchanged.
- **Behaviour:** `chamfer` and `fillet` both call `resolve` first, so an integration test can only
  reach edge selection by actually chamfering something. Exporting it makes "this selector matches
  exactly one edge" a directly testable contract, which is what `KER-FEAT-027` (`FaceRef`) will
  need as its model, and what `MOD-002` (topological naming) needs to know about.
- **Refusals:** unchanged: a selector that lands on no edge → `Invalid("no edge passes through
  {p:?}")`; one that lands on several → `Invalid("the reference {p:?} lies on {n} edges, which is
  ambiguous")`; a shell with no edge list (`edges()` returned `None`) → `Invalid("the shell has no
  edge list")`; a non-finite selector point or negative tolerance → `NonFinite`; more selectors than
  `MAX_BLENDS` → `Invalid` naming both numbers.
- **Tests:** in `chamfer.rs`, `resolve_finds_the_edge_a_blend_would_use` (a box: a selector at an
  edge midpoint resolves to exactly that edge's index); `resolve_on_a_corner_is_ambiguous`
  (already present — keep it and assert the message names the count); `resolve_returns_indices_sorted_ascending`.
- **Acceptance:** exported; `chamfer` and `fillet` still behave identically (their existing tests are
  unchanged and must pass).
- **Risk:** none material. **Fallback:** export as `pub mod diagnostic::resolve_edges` if
  `KER-API-007` objects.

### KER-BOOL-010 — Re-export `section`

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/lib.rs`, `crates/solid/src/section.rs`
- **Contract:** `pub use section::{Section, section};`
- **Behaviour:** `Section` is exported; the function that produces it is not, which is a strange
  split for a public type. Exporting it makes the planar section directly testable, which rows
  D18–D20 of the matrix and `KER-TOL-008` both need.
- **Refusals:** unchanged: segments that cannot be chained → `Section("segments could not be
  chained")`; loops that cannot be nested-sorted → `Section("section loops could not be sorted")`;
  a plane containing a face but grazing elsewhere → `Section` naming both areas.
- **Tests:** in `section.rs`, `section_is_callable_through_the_crate_root` (the existing
  `a_plane_through_the_middle_of_a_box_sections_it_as_a_rectangle` rewritten to use the root export).
- **Acceptance:** exported, nothing else changed.
- **Risk:** none material. **Fallback:** none needed.

### KER-BOOL-011 — Named error variants for the degenerate matrix

- **State:** READY
- **Depends on:** KER-BOOL-006, KER-BOOL-007
- **Files:** `crates/solid/src/error.rs`
- **Contract:** add to `pub enum SolidError` (thiserror), each with a `#[error("...")]` message that
  names its reason and its numbers:
  ```rust
  #[error("degenerate input: {0}")]
  Degenerate(String),
  #[error("a sliver of thickness {0} is below the working tolerance {1}")]
  SliverTooThin(f64, f64),
  #[error("the shells touch along {0:?} but share no volume; touching operands cannot be combined")]
  Touching(Vec3, Vec3),
  #[error("face {0} and face {1} are tangent, not crossing; a tangent cut removes nothing")]
  TangentFaces(usize, usize),
  ```
  Existing variants are never renamed, reordered or re-signed: `crates/solid/tests/property.rs`
  matches on `SolidError::Empty` and `SolidError::NonPositiveVolume` today.
- **Behaviour:** the matrix (rows D6, D9, D12, D13, D17) currently cannot say *why* it refused; every
  one of those refusals lands in `Invalid(String)`. The task is to give each of them a stable,
  matchable variant carrying the numbers a caller needs, and to update `boolean.rs`'s call sites to
  construct them. An error variant is part of the public contract from the moment it exists
  (`KER-API-003`).
  **Orientation:** unchanged — no error path may flip a face.
- **Refusals:** this task's own: changing an existing variant's discriminant is refused (it would
  break QAT's tests); a new variant whose message does not carry the offending numbers is refused
  in review.
- **Tests:** in `error.rs`, `every_variant_renders_a_message_that_names_its_reason` (format each
  variant and assert the string is non-empty and contains the interpolated value);
  `variant_discriminants_are_stable` (a test that lists the variants in order and asserts the
  ordering — it is the cheapest possible guard against a silent reorder).
- **Acceptance:** the five variants exist with the given signatures; the matrix rows D6, D9, D12,
  D13, D17 produce them; `master-plan.md` §4's "three kernel defects" count is reduced accordingly
  in the orchestrator's copy.
- **Risk:** widening the error enum is a public API change that `KER-API-003` may want to batch with
  the surface errors. **Fallback:** land these five now under `KER-BOOL`'s ownership and let
  `KER-API-003` append rather than reorganise; record the ordering as frozen.

### KER-BOOL-012 — Degenerate: coplanar faces, same side, identical footprints

- **State:** READY
- **Depends on:** KER-BOOL-006, KER-BOOL-011
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** no new API. Consumes `boolean`; relies on the `(Union, true, true, true)` arm.
- **Behaviour:** two solids whose faces share a plane *and* a normal direction must contribute one
  merged face, not two that then need welding, and no sliver may survive along the seam. The
  `bool2d::union(&sa, &sb)` result goes into one `Face` in the slot's plane.
  **Orientation:** the merged region is CCW in the slot plane's frame, so the single merged face's
  normal is the shared normal — outward for both operands, because they are on the same side.
- **Refusals:** a union of the two sections that is empty (the operands touch only from opposite
  sides despite reporting the same normal) → `Invalid`; both operands empty → `Empty`.
- **Tests:** `degenerate_D1_coplanar_same_side_identical_footprints_merge_into_one_face` — two boxes
  side by side sharing `x = 2` with both normals `+X`: the union has 10 faces, not 11, and no
  vertex at `(2, 2, 2)` that belongs to both; the shared face's area is exactly `4`.
- **Acceptance:** matrix row D1 has this test.
- **Risk:** low. **Fallback:** if the merged region arrives CW (because the two inputs were carried
  through a reflection), re-run `reverse_regions` and assert the signed area is positive in the test
  rather than in production code.

### KER-BOOL-013 — Degenerate: coplanar faces, opposite sides, identical footprints

- **State:** READY
- **Depends on:** KER-BOOL-002
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`.
- **Behaviour:** two solids on opposite sides of one shared plane whose sections coincide: the plane
  is interior everywhere and contributes **no** face. The result is the two lobes merged into one
  body with the shared plane absent. This is the case the module doc's "coplanar faces fall out for
  free" was written about, and it is the case that must *not* regress while `KER-BOOL-002` adds the
  two-face path.
  **Orientation:** with no face on the shared plane, the material on either side is bounded by the
  neighbouring planes; the result is validated by volume, so an inside-out build is caught.
- **Refusals:** a result that still reports the shared plane as an open edge → `OpenEdge`;
  a non-positive signed volume → `NonPositiveVolume`.
- **Tests:** `degenerate_D2_coplanar_opposite_sides_identical_footprints_emit_no_face` — two 2×2×1
  blocks stacked exactly: 6 faces, volume 8 (the existing control case in `property.rs`, promoted to
  a unit test here).
- **Acceptance:** matrix row D2 has this test, and it still passes after `KER-BOOL-002`.
- **Risk:** `KER-BOOL-002` changes this arm's behaviour and could break it. **Fallback:** gate the
  new two-face path on `sa != sb` (non-equal sections) so the coincident case keeps its old
  single-answer path, and assert both in the test.

### KER-BOOL-014 — Degenerate: coplanar faces, opposite sides, partial overlap

- **State:** READY
- **Depends on:** KER-BOOL-002
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-002`.
- **Behaviour:** the L-prism. The exposed part of A's face and the exposed part of B's face are each
  a boundary of the result and point in opposite directions, so two faces in the same plane with
  opposite normals are correct. Volume: `12` for `box(0,0,0)-(2,2,1) ∪ box(1,0,1)-(3,2,2)`.
  **Orientation:** each lobe's exposed part wound CCW in its own plane frame, so each normal points
  out of that lobe and away from the shared material.
- **Refusals:** if `bool2d::difference(&sa, &sb)` and its mirror both come back empty while the
  operands overlap, the sections are degenerate → `Invalid` naming both areas.
- **Tests:** `degenerate_D3_the_l_prism_union` (10 faces, volume 12 to 1e-9, `validate()` and
  `validate_vertices()` ok).
- **Acceptance:** matrix row D3 has this test; `KER-BOOL-002` and this task share it and only one of
  them writes it (the other adds a different scale).
- **Risk:** low once `KER-BOOL-002` lands. **Fallback:** none needed.

### KER-BOOL-015 — Degenerate: two solids sharing only an edge

- **State:** READY
- **Depends on:** KER-BOOL-011
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`.
- **Behaviour:** operands whose bounds intersect but whose closures meet only on a one-dimensional
  set. `Union` is mathematically legal (the result is two lobes) but the shell is pinched at the
  shared edge, which `validate_vertices()` rejects; `Difference` is a no-op; `Intersect` is empty.
  The kernel's contract is that the result is either a valid solid or a named `Err`, so the pinched
  union must be refused rather than returned.
  **Orientation:** the shared edge would be a four-way vertex fan; the pinching check
  (`fan_is_one_cycle`) is what detects it.
- **Refusals:** union touching only along an edge → `Touching(a, b)` naming the edge endpoints;
  difference with a tool touching only along an edge → `Ok(a)` unchanged (the tool removes nothing);
  intersect → `Empty`.
- **Tests:** `degenerate_D6_two_solids_sharing_only_an_edge` — two boxes meeting at `x = 2` in a
  line of zero width in `y` and `z`: union is `Touching`, difference returns A with A's exact
  volume, intersect is `Empty`.
- **Acceptance:** matrix row D6 has this test with the variant asserted, not `is_err()`.
- **Risk:** a *legal* union that happens to touch along an edge is refused, which a user may find
  surprising. **Fallback:** that is the documented trade; state it in the doc comment on `boolean`
  and in the `Touching` variant's error text so a caller can tell the user what happened.

### KER-BOOL-016 — Degenerate: two solids sharing only a vertex

- **State:** READY
- **Depends on:** KER-BOOL-015
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`.
- **Behaviour:** as `KER-BOOL-015` but the contact is a point. The pinching check catches it at the
  vertex; the difference and intersect answers are the same as row D6.
- **Refusals:** union touching at a vertex → `Invalid("vertex {v:?} joins {n} separate fans")`;
  intersect → `Empty`; difference → `Ok(a)`.
- **Tests:** `degenerate_D7_two_solids_sharing_only_a_vertex` (two boxes meeting at `(2,2,2)` and
  nowhere else: union is refused with a message naming the vertex, difference is A unchanged).
- **Acceptance:** matrix row D7 has this test.
- **Risk:** low. **Fallback:** none needed.

### KER-BOOL-017 — Degenerate: one operand strictly inside the other

- **State:** READY
- **Depends on:** KER-BOOL-006
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`.
- **Behaviour:** `a ∪ b = a` and `a ∩ b = b` when `b ⊂ a`. The plane-slot machinery handles this
  without a special case: every plane of `b` sections `a` to `b`'s own face, and the union of the
  two sections is `a`'s; the `b` planes are interior and contribute nothing.
  `a - b` is the interesting one: it produces a solid with an internal void, which is still closed
  and positively oriented but has two disconnected shells. The kernel has one shell per `Solid`, so
  this is refused rather than silently dropped.
  **Orientation:** a void would require the inner shell's normals to point **inward**, which would
  make the total signed volume `vol(a) - vol(b)`. Detecting this is a positive check: if the
  difference's volume comes out as `vol(a) - vol(b)` with a single shell, the inner faces were
  dropped; assert it.
- **Refusals:** `a - b` where `b` is strictly interior → `Invalid("the cut leaves an internal void
  at {c:?}; one shell per solid cannot represent it")`; union → `Ok(a)`; intersect → `Ok(b)`.
- **Tests:** `degenerate_D8_strictly_nested_union_and_intersect` (a 2×2×2 box and a 1×1×1 box at its
  centre: union volume 8 with 6 faces, intersect volume 1 with 6 faces);
  `degenerate_D9_subtracting_a_nested_solid_is_refused` (difference is refused naming the interior
  point).
- **Acceptance:** matrix rows D8 and D9 have these tests.
- **Risk:** refusing `a - b` for nested `b` may block a legitimate "pocket from a tool that sits
  wholly inside" case. **Fallback:** if the inner faces are *not* fully interior — the tool breaks
  the surface — the difference is an ordinary cut and must succeed; test that case explicitly so the
  refusal is narrow, and detect interiority with `KER-TOL-024`'s band test rather than by bounds
  alone.

### KER-BOOL-018 — Degenerate: operands equal up to tolerance

- **State:** READY
- **Depends on:** KER-BOOL-006
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`.
- **Behaviour:** `a` and `b` whose vertex multisets agree within `tol` but whose stored coordinates
  differ by up to `tol`. The boolean must treat them as equal: `a ∪ b` and `a ∩ b` return a solid
  whose volume equals `a`'s to within `1e-9` relative, and `a - b` refuses. The tolerance used is
  `max(a.tolerance(), b.tolerance())`, never either one alone.
  **Orientation:** unchanged; the merged faces come from the slot plane of whichever face was
  registered first, which must be deterministic — see `KER-BOOL-031`.
- **Refusals:** `a - b` → `Empty`; operands that are *close* but differ by more than `tol` (they are
  distinct) → the boolean proceeds normally, no error.
- **Tests:** `degenerate_D10_operands_equal_within_tolerance` (a box and the same box translated by
  `tol / 10`): union and intersect both succeed with volume equal to the box's to 1e-9, difference
  is `Empty`.
- **Acceptance:** matrix row D10 has this test.
- **Risk:** a translation of `tol/10` may still fall into a different weld cell and produce two
  distinct slots. **Fallback:** weld both operands before slot construction (a no-op for a clean
  box), which is already done at the end of the boolean — move it to the front.

### KER-BOOL-019 — Degenerate: a cut that leaves a zero-thickness sliver

- **State:** READY
- **Depends on:** KER-BOOL-011
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`, producing `SolidError::SliverTooThin(f64, f64)`.
- **Behaviour:** a cut whose result contains a wall thinner than the working tolerance. The planar
  boolean produces a region whose narrow dimension is below `tol` — the `bool2d` result is legal,
  and the sliver survives as a real face that no later stage can resolve. Detect it after the
  section: for every face region produced, measure the minimum width of the region
  (minimum over boundary segment pairs of the separation, or the inradius of the tightest local
  concavity) and refuse below `2 * tol`. The honest alternative — keeping the sliver — is what
  produces solids that render as hairlines and fail validation downstream.
  **Orientation:** unchanged.
- **Refusals:** a result face narrower than `2 * tol` → `SliverTooThin(width, tol)`;
  a result whose *whole* region is below `2 * tol` → `Empty` instead, because there is nothing left.
- **Tests:** `degenerate_D12_a_cut_leaving_a_sliver_is_refused` — a 10-unit plate cut by a tool
  leaving a `0.5 * tol`-wide rib: refused with `SliverTooThin`, and the test asserts the width
  reported is within 10 % of the constructed width; `degenerate_D12b_a_cut_removing_everything_is_Empty`.
- **Acceptance:** matrix rows D12 has this test with the numbers asserted.
- **Risk:** a minimum-width measure over an arbitrary polygon is itself expensive and error-prone.
  **Fallback:** measure the width only where the boolean *created* a boundary — i.e. compare the
  result region against the two input sections with `bool2d` and take the minimum edge-to-edge
  distance at vertices — which is O(loops) and catches the case the cut actually produces.

### KER-BOOL-020 — Degenerate: a tool tangent to the base's faces

- **State:** READY
- **Depends on:** KER-BOOL-011
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`, producing `SolidError::TangentFaces(usize, usize)`.
- **Behaviour:** the "fill the exact size of a hole" case: every wall of the tool lines up with a
  wall of the hole, so each cutting plane is *tangent* to the base along a whole face. This is
  legitimate and must work (`a_fill_the_exact_size_of_a_hole_changes_nothing` already pins it).
  What must be refused is a tool whose face is tangent to a base face **and whose other faces leave
  no material at all** — a cut that removes nothing while pretending to. Detect: for each tool face
  plane coplanar with a base face plane on the same side, if the sections' difference is empty and
  the intersection is non-empty, the tool merely touches → the difference is `Ok(a)` unchanged, not
  an error. The refusal is for the case where two tool faces are simultaneously tangent and their
  combined footprint is degenerate.
  **Orientation:** a tangent tool face has the same normal as the base face it touches, so merging
  them is correct; if they have opposite normals the tool is inside-out and is `TangentFaces`.
- **Refusals:** an inside-out tool that is everywhere tangent → `TangentFaces(i, j)`;
  a tool tangent along a face with the correct orientation → `Ok(a)`, no error.
- **Tests:** `degenerate_D13_a_tangent_tool_removes_nothing_and_says_so` (the exact-size fill:
  difference returns `a` with `a`'s exact volume — this already exists as
  `a_fill_the_exact_size_of_a_hole_changes_nothing`, promote it);
  `degenerate_D13b_an_inside_out_tool_is_refused`.
- **Acceptance:** matrix row D13 has both tests.
- **Risk:** `orient_faces()` on the *tool* is what distinguishes the two cases and it probes by ray
  cast, which is ambiguous for a tool thinner than its own tolerance. **Fallback:** orient the tool
  from its signed volume first (`fix_orientation`), which is exact and needs no interior point, and
  only then run the tangent analysis.

### KER-BOOL-021 — Degenerate: self-intersecting operand

- **State:** READY
- **Depends on:** KER-BOOL-007
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`, producing `SolidError::FaceOverlap(usize, usize)`.
- **Behaviour:** an operand that passes `validate()` (closed, consistently oriented, positive
  volume) but whose non-adjacent faces cross each other in space. Every current invariant misses
  it, which is why `KER-BOOL-007` exists. The boolean must run `validate_intersections()` on both
  operands **before** any work, because a self-intersecting operand makes every downstream section
  meaningless.
- **Refusals:** overlapping faces → `FaceOverlap(i, j)` with the operand named in the message
  (`Invalid` carrying it if `FaceOverlap` lacks the operand field — record which in the report);
  a shell whose intersections cannot be decided → `Invalid`.
- **Tests:** `degenerate_D14_a_self_intersecting_operand_is_refused_before_any_work` (a bowtie
  face set: assert the error arrives and that no partial result is returned);
  `degenerate_D14b_a_clean_operand_with_many_faces_is_not_refused` (a 12×12 grid of coplanar
  rectangles merged into one face, to prove the check is not simply refusing large inputs).
- **Acceptance:** matrix row D14 has these tests.
- **Risk:** the quadratic check on a legitimate 10⁴-face solid may refuse valid work. **Fallback:**
  gate the check on face count (`> MAX_WELD_VERTICES` skips, and says so in the returned
  `SolidError` note only when it then fails) — and prefer the indexed version from `KER-PERF-003`.

### KER-BOOL-022 — Degenerate: open (non-closed) operand

- **State:** READY
- **Depends on:** KER-BOOL-006
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`, propagating `SolidError::OpenEdge(Vec3, Vec3, usize)`.
- **Behaviour:** a shell with an edge used once (a box with a face removed). `Solid::edges()`
  already returns `None` in this case, and the boolean's section step will produce nonsense.
  Validate both operands up front — `validate()` is the cheap check and it already returns the
  right variant with the right endpoints.
  **Orientation:** n/a.
- **Refusals:** an open operand → `OpenEdge(a, b, 1)` naming which operand in the message;
  an operand whose `edges()` is `None` but which `validate()` accepts (an empty shell) → `Empty`.
- **Tests:** `degenerate_D15_an_open_operand_is_refused` (a five-face box: `OpenEdge` naming the
  missing face's edge and count 1); `degenerate_D15b_an_operand_with_no_edges_at_all` (`Empty`).
- **Acceptance:** matrix row D15 has this test.
- **Risk:** adding an up-front `validate()` costs a second traversal per operand. **Fallback:** keep
  the up-front check cheap by validating only I2 (edge use counts), which is the property that
  actually matters here, and let the end-of-function `validate()` do the rest.

### KER-BOOL-023 — Degenerate: inside-out operand

- **State:** READY
- **Depends on:** KER-BOOL-006
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** as `KER-BOOL-012`, propagating `SolidError::NonPositiveVolume(f64)`.
- **Behaviour:** an operand built with `Solid::flipped()` — closed, consistently oriented, but its
  normals point inward, so its signed volume is negative. The boolean must refuse it rather than
  produce a result whose material is on the wrong side.
- **Refusals:** negative signed volume on either operand → `NonPositiveVolume(v)` naming the
  operand; an operand with volume exactly 0 (a degenerate shell) → `Empty`.
- **Tests:** `degenerate_D16_an_inside_out_operand_is_refused` (a flipped box: the variant and a
  negative value are both asserted).
- **Acceptance:** matrix row D16 has this test.
- **Risk:** none material. **Fallback:** none needed.

### KER-BOOL-024 — Degenerate: self-touching operand at a vertex

- **State:** READY
- **Depends on:** KER-BOOL-011
- **Files:** `crates/solid/src/solid.rs`, `crates/solid/src/boolean.rs`
- **Contract:** `pub fn Solid::validate_vertices(&self) -> Result<(), SolidError>` (unchanged),
  producing `Invalid("vertex {key:?} has a pinched fan of {n} faces")`.
- **Behaviour:** every edge is used exactly twice and the orientation is consistent, but the shell
  pinches: two lumps meeting at one vertex. `validate_vertices()` exists and is not on the default
  `validate()` path because it is quadratic. The boolean must call it on both operands and on its
  own result (the result's pinch check is already missing from `boolean()`).
  **Orientation:** a pinched fan means the faces around the vertex form two cycles rather than one;
  each cycle is individually consistently wound, which is exactly why I2/I3 cannot see it.
- **Refusals:** an operand with a pinched fan → `Invalid` naming the vertex and the fan size; a
  result with a pinched fan → the same, propagated before the result is returned.
- **Tests:** in `solid.rs`, keep `two_boxes_touching_at_a_face_are_not_one_solid`;
  in `boolean.rs`, `degenerate_D17_the_boolean_does_not_return_a_pinched_result` (union two boxes
  that touch at a face: `Err`, never a 12-face pinched shell); `degenerate_D17b_a_cone_apex_is_not_a_pinch`
  (the apex of a revolved cone has many faces in one fan — must validate, which `KER-BOOL-001`
  depends on).
- **Acceptance:** matrix row D17 has both tests; `boolean()` calls `validate_vertices()` on its
  result before returning `Ok`.
- **Risk:** `validate_vertices` is quadratic in the fan and will dominate the boolean's cost at
  10⁴ faces. **Fallback:** run it only when the result's face count is below
  `MAX_WELD_VERTICES`, and say in the doc comment on `validate()` that I5 is then unverified rather
  than implying it was checked.

### KER-BOOL-025 — Section: a plane through an exact vertex

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged `pub fn section(solid: &Solid, plane: &Plane, tol: f64) -> Result<Section, SolidError>`
- **Behaviour:** `crossing_segments` walks each loop and pushes a crossing for an endpoint that is
  already on the line; a vertex lying exactly on the cutting plane is found twice (once per
  incident edge) and `dedup_by` collapses them. The task is to make the dedup provably order-
  independent — it currently sorts by the projection `(w - plane.origin) · dir` and dedups
  neighbours, so two distinct vertices whose projections coincide *along the line* but which are
  different points on it are safe, while two vertices at the same projection but different
  perpendicular offsets are not. Pin the behaviour with tests at the diagonal plane `x + y = 2`
  that passes exactly through two box vertices, and at a plane through a vertex that also clips
  another face's interior.
  **Orientation:** the section's region must be CCW-outer / CW-holes in the cutting plane's frame,
  because the boolean uses it as the face region. `Regions::from_loops` handles the nesting; this
  task pins the winding.
- **Refusals:** a plane through an exact vertex that yields no closed region → `Empty`-shaped
  section (an empty `Regions` is the correct answer, not an error); segments that cannot be chained
  → `Section("segments could not be chained")`.
- **Tests:** `a_plane_through_two_exact_vertices_sections_the_same_as_a_nudged_plane` (already
  present as `a_diagonal_plane_sections_the_box_as_a_rectangle` — keep it and add the nudge in the
  opposite direction); `a_plane_through_a_vertex_and_through_a_face_interior` (the two-crossings
  case); `a_plane_through_a_vertex_of_a_hole` (a tube's section must keep its hole).
- **Acceptance:** matrix row D18 and `a_plane_through_a_vertex_only_never_panics` both pass, and a
  nudged plane gives the same area to 1e-5.
- **Risk:** an edge exactly in the cutting plane contributes its two endpoints as crossings and can
  desynchronise the even-odd pairing. **Fallback:** detect a segment whose *both* endpoints lie on
  the line and treat it as a boundary segment directly (already what `crossing_segments` does for
  coplanar faces) rather than as two crossings.

### KER-BOOL-026 — Section: a plane tangent to a face

- **State:** READY
- **Depends on:** KER-BOOL-025
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged `section`.
- **Behaviour:** a plane that just misses the solid must produce an empty section, not the face it
  is near. `parallel(plane, 1e-9)` skips faces whose normal is within `dir_tol` of the cutting
  plane's; a face that is *almost* parallel is not skipped and contributes a crossing segment at a
  huge extrapolation, which is how "just touching" can pick up a face. Make the tangency test use
  the length tolerance as well as the direction one: a face counts as coplanar when both
  `parallel(&plane, dir_tol)` and `|face.plane.distance(plane.origin)| <= tol`, and is skipped when
  both hold and the plane is on the far side.
- **Refusals:** a tangent plane that also contains a face → `Section` (the existing
  "grazes the solid elsewhere" refusal, row D20).
- **Tests:** `a_plane_tangent_to_a_face_sections_nothing` (the existing
  `a_plane_just_touching_the_solid_sections_nothing` — keep it, and add the case where the tangent
  plane is at `+tol` and at `-tol` so both sides of the boundary are covered).
- **Acceptance:** matrix row D19 has this test.
- **Risk:** using `tol` for the coplanarity test where `1e-9` was used tightens or loosens the
  "contains a face" path, which row D20 depends on. **Fallback:** keep the two tests separate —
  `parallel` for the skip decision, `coplanar` (which already takes both tolerances) for the
  contains-a-face decision — and change only the skip.

### KER-BOOL-027 — Section: a plane containing a face and grazing elsewhere

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged `section`, producing `SolidError::Section(String)` with the message
  naming both areas.
- **Behaviour:** already implemented and already refused:
  `"the plane contains a face of area {a} but sections to {b}: it grazes the solid elsewhere"`. The
  task is to pin it with a named test at the tolerance boundary — a plane that contains a face and
  whose `xor` difference is within `slack = coplanar.area() * 1e-6 + tol * tol` must **not** be
  refused, and one just outside must be. Without this test the slack constant is untuned and will
  drift.
  **Orientation:** unaffected.
- **Refusals:** `xor` difference above the slack → `Section` naming both areas; `xor` returning
  `None` → `Invalid`, not a silent pass.
- **Tests:** `degenerate_D20_a_plane_containing_a_face_and_grazing_elsewhere_is_refused` (a pocket
  whose wall lines up exactly with an existing face plane, so the plane contains one face and cuts
  another); `degenerate_D20b_the_same_plane_nudged_by_one_tolerance_does_not_graze`.
- **Acceptance:** matrix row D20 has both tests; the slack constant has a justification comment.
- **Risk:** a legitimate boolean may hit the grazing path when a tool's face is tangent to a base's
  face, and then refuses where it used to succeed. **Fallback:** raise the slack to
  `coplanar.area() * 1e-4` and document that the check is a *diagnostic* rather than a correctness
  gate, keeping the refusal as the default when the difference exceeds it.

### KER-BOOL-028 — Deterministic merge and prune of duplicate and dangling segments

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged signatures for the two private helpers, promoted to
  `pub(crate) fn merge_segments(segments: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<(Vec2, Vec2)>` and
  `pub(crate) fn prune_dangling(segments: Vec<(Vec2, Vec2)>, tol: f64) -> Vec<(Vec2, Vec2)>`.
- **Behaviour:** `merge_segments` is O(n²) in the fragment count — it scans everything emitted so far
  for each new segment. It is also order-dependent in the sense that the *first* of a duplicate pair
  is the one kept, and "first" is the loop order. Make it deterministic by sorting the segments by
  `(min endpoint, max endpoint)` with `total_cmp` before deduplicating, so the survivor is a
  function of geometry alone. `prune_dangling` iterates up to 8 times and breaks when a pass removes
  nothing; it must also be order-independent, which it is once `merge_segments` is, because the
  degree map is keyed by grid cell.
- **Refusals:** a fragment with non-finite endpoints → dropped (documented, not an error — the
  coplanar-face loop can emit one when a face's plane origin is huge); a segment shorter than `tol` →
  dropped; more than `8` prune passes still finding degree-1 vertices → the remainder is returned as
  is and the caller's `chain_segments` refuses with `Section("segments could not be chained")`.
- **Tests:** `merge_segments_keeps_a_geometry_defined_survivor` (the same duplicate pair presented in
  both orders gives the same single segment); `merge_segments_is_equivalent_to_the_quadratic_scan`
  (for 500 random segment pairs from the `Case` vocabulary, output sets are equal);
  `prune_dangling_removes_only_degree_one` (a three-way junction keeps all its segments — the
  existing doc comment says this is a real tangency, so it must be a test).
- **Acceptance:** both helpers are `pub(crate)`, deterministic, and have the three tests.
- **Risk:** sorting changes which duplicate survives, which may move a section boundary by up to
  `tol` and break a 1e-9 volume assertion. **Fallback:** sort by `(endpoint, other endpoint)` and
  keep the first, and if an assertion breaks, widen the specific test's tolerance to the documented
  `diagonal * 1e-9` rather than loosening the dedup.

### KER-BOOL-029 — Section: a face whose loop crosses the cutting line four or more times

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged `section`; private
  `fn crossing_segments(face: &Face, plane: &Plane, tol: f64) -> Vec<(Vec2, Vec2)>`.
- **Behaviour:** `crossing_segments` pairs sorted crossings two-by-two (0-1, 2-3, …) on the theory
  that a simple polygon's crossings with a line alternate in/out. That is correct for a simple
  loop and the task is to *prove* it rather than assume it: construct a non-convex face whose
  boundary crosses the cutting line six times (a comb shape), section it, and assert the section is
  the union of the three inside intervals. If the theory holds for simple loops (it does — a Jordan
  curve crosses a line an even number of times and alternates), the task is to state it in the
  doc comment and pin it with a six-crossing test. If it fails for a *self-touching* loop (a loop
  that touches itself, which `Region` does not forbid), that is a refusal.
  **Orientation:** each paired segment runs along increasing projection `dir`; after
  `chain_segments` and `from_loops` the resulting region's outer loop must be CCW in the cutting
  plane's frame.
- **Refusals:** a loop that is not simple (touches itself, so crossings do not alternate) →
  `Section("loop {face} is not simple: the cutting line crosses it {n} times without alternating")`;
  odd crossing count (impossible for a closed loop, so a sign of an endpoint snapped onto the line
  incorrectly) → the same `Section`.
- **Tests:** `a_combed_face_sections_into_three_intervals` (a 6-tooth comb cut by a transverse
  plane: the section area equals the closed-form comb area to 1e-9);
  `a_self_touching_loop_is_refused` (a figure-eight loop: `Section`, named).
- **Acceptance:** the alternating-pairing claim is documented as a theorem about simple loops with
  a counterexample for non-simple ones, and both tests pass.
- **Risk:** the figure-eight case may not be constructible through `Region`'s constructor.
  **Fallback:** construct the loop directly as `Region { outer, holes }` with a hand-written
  `outer` that retraces, and if `Region::new` normalises it away, note that in the test and pin the
  behaviour that actually occurs.

### KER-BOOL-030 — Section: the loop-nesting sort failure path

- **State:** READY
- **Depends on:** KER-BOOL-029
- **Files:** `crates/solid/src/section.rs`
- **Contract:** unchanged `section`; consumes `cadcraft_geom::Regions::from_loops(loops, tol) -> Option<Regions>`
  (already used today, already returns `None` on failure).
- **Behaviour:** after chaining, `Regions::from_loops(traced, tol)` sorts loops into outer/holes by
  nesting depth. Its failure path is currently unreachable in tests — every section either nests
  cleanly or errors earlier. The task is to make it reachable: a section of a solid whose cutting
  plane crosses two *separate* lobes produces two disjoint outer loops with no hole, which must
  come back as **two** regions, not one. And a plane that grazes a lobe tangentially produces a
  loop that is inside another without being a hole — which *is* a hole, correctly. Pin both, plus
  a section of a torus-like prism where the loops nest three deep.
- **Refusals:** `from_loops` returning `None` → `Section("section loops could not be sorted")`
  (the message already exists); more than `MAX_LOOPS` traced loops → `Section` naming both numbers.
- **Tests:** `a_plane_through_two_separate_lobes_gives_two_regions` (a "U" shaped prism cut
  horizontally: two rectangles, both CCW, both non-empty); `a_section_three_loops_deep_nests_correctly`
  (a frame-shaped prism cut across: outer plus two holes).
- **Acceptance:** matrix rows D18/D20 unaffected; the two new tests pass; `MAX_LOOPS` is referenced
  (currently it is not, which is a latent unbounded loop).
- **Risk:** a disjoint multi-region section feeds the boolean's per-plane face construction, which
  builds **one** `Face` per plane. **Fallback:** build one `Face` per region of the section — that is
  legal (`Regions` holds several) and is what the code already does
  (`Face::new(*p, region.clone())` with a multi-region `Regions`), so no change is needed; add a test
  that proves it.

### KER-BOOL-031 — One total order for the plane-slot list

- **State:** READY
- **Depends on:** KER-BOOL-004
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** `fn slot_key(p: &Plane) -> ([u64; 4], [u64; 4])` returning the normal's bit pattern
  and the signed distance from the world origin along the normal, both as `total_cmp`-ordered
  `u64` keys. `Plane` has no such method today and this task must not add one (it is the orchestrator's
  to decide, via `KER-API-001`).
- **Behaviour:** the slot list is built by scanning A's faces then B's faces and appending. A plane
  in A that is coplanar with a plane in B joins A's existing slot; the same plane found in B first
  would create the slot with `b_owns` and then join it. The surviving difference is `slot.same_side`,
  which the current code overwrites on every match rather than accumulating — and a face that is
  coplanar with a slot's plane but has an *opposite* normal must set `same_side = false`, not leave
  the previous value. Make the key a pure function of the plane geometry so the list order is a pure
  function of the operand *set*, then sort the finished list by it.
- **Refusals:** two planes within `tol` of each other but not coplanar (they differ in direction) →
  they are **not** merged; the key includes the direction bits so they stay separate;
  a plane whose origin is not finite → `NonFinite`.
- **Tests:** `plane_slots_are_identical_for_either_operand_order` (for 40 pairs from the `Case`
  vocabulary, `plane_slots(a,b)` and `plane_slots(b,a)` produce byte-identical `Vec<Slot>` values
  including the flags); `same_side_is_accumulated_not_overwritten` (three faces on one plane, two
  with `+Z` and one with `-Z`: the slot records `same_side = false`);
  `two_nearly_but_not_quite_coplanar_planes_stay_separate` (planes at distance `tol/2` and
  `2 * tol`, same normal).
- **Acceptance:** this test is the precondition for `KER-BOOL-003`; it is written first.
- **Risk:** sorting by an absolute bit pattern makes the order depend on the world origin, so the
  same model translated by 1e6 produces a different face order. **Fallback:** key on the plane's
  `origin` relative to the *operand pair's* combined bounds min, which is translation-invariant up
  to the tolerance grid, and document that.

### KER-BOOL-032 — Per-operand tolerance in the boolean

- **State:** READY
- **Depends on:** KER-BOOL-031
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/src/solid.rs`
- **Contract:** unchanged `boolean`; the internal tolerance computation becomes explicit and
  documented: `fn operand_tolerances(a: &Solid, b: &Solid) -> (f64, f64, f64)` returning
  `(a_tol, b_tol, result_tol)` with `result_tol = a_tol.max(b_tol)`.
- **Behaviour:** `a` may be a 1e6-unit part and `b` a 1e-6-unit detail. `a.tolerance()` is
  `diagonal * 1e-9` ≈ 1e-3 and `b`'s is clamped to the 1e-12 floor. The boolean uses `max` for the
  section and the weld, which means `b`'s own features are below `a`'s tolerance and can be lost.
  The contract to state and test: the *section* tolerance is each operand's own when it decides
  whether a plane is coplanar with its own faces, and the *weld* tolerance is the max, because
  welding across operands must use one grid. Document this and test it: cutting a 1e6 box with a
  1e-6 tool either produces a solid with the tool's slot or refuses naming the tolerance — it must
  not silently produce the un-cut box.
- **Refusals:** a feature of operand `b` narrower than `result_tol` → `SliverTooThin(w, result_tol)`;
  an operand whose own tolerance is below `f64::MIN_POSITIVE * 2` → `NonFinite`;
  a non-finite tolerance argument → `BadTolerance(t)`.
- **Tests:** `two_operands_at_different_scales_use_each_own_tolerance_for_coplanarity` (a 1e6 box and
  a 1e6-scale box offset by `1e-7` are coplanar; offset by `1e-2` are not);
  `a_large_operand_and_a_tiny_tool_does_not_silently_keep_the_large_one`
  (assert the result is either correct or refused with a tolerance-named error, never the un-cut
  box).
- **Acceptance:** the tolerance rule is written in the function doc and both tests pass.
- **Risk:** changing the tolerance changes every existing boolean result. **Fallback:** keep
  `max` as the only tolerance the boolean uses, document the consequence honestly (features of the
  smaller operand below the larger's tolerance are not resolved), and turn the second test into a
  documented refusal rather than a behaviour change.

### KER-BOOL-033 — Difference with a tool that only touches

- **State:** READY
- **Depends on:** KER-BOOL-020, KER-BOOL-032
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged `boolean`.
- **Behaviour:** `a - b` where `b`'s closure meets `a`'s in a face, an edge or a vertex but encloses
  no volume must return `a` unchanged — bit-for-bit the same face list. This is the case the
  `(Difference, true, true, false)` arm handles today with `Some(sa.clone())` and
  `(Difference, false, true, false)` → empty. Both must hold when `b` touches but does not overlap,
  including when it touches along only part of a face.
  **Orientation:** the untouched face keeps its own plane and normal exactly.
- **Refusals:** a tool that touches *and* overlaps → a normal difference, not this case; a tool
  strictly interior → `Invalid` naming the void (row D9); an empty tool → `Ok(a)`.
- **Tests:** `degenerate_D33a_subtracting_a_face_touching_tool_changes_nothing` (the existing
  `subtracting_a_part_that_only_touches_gives_the_original` — keep it and assert the face list is
  identical, not just the volume); `degenerate_D33b_subtracting_a_tool_that_touches_at_a_vertex_changes_nothing`;
  `degenerate_D33c_subtracting_a_tangent_tool_leaves_every_face_outward`.
- **Acceptance:** all three pass; the "no material was deleted" guarantee is asserted by face
  count and face identity, not by volume alone.
- **Risk:** a face-touching tool that *also* pokes through elsewhere must not be classified as
  touching. **Fallback:** the classification comes from `bool2d::intersect(&sa, &sb)` being empty on
  every slot, which is exact, not from the bounds.

### KER-BOOL-034 — Boolean algebra identities on the empty and identical cases

- **State:** READY
- **Depends on:** KER-BOOL-006
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged `boolean`.
- **Behaviour:** pin the algebra `property.rs` relies on as unit tests in the crate:
  `a ∪ ∅ = a`, `a ∩ ∅ = ∅` (as `Err(Empty)`), `a - ∅ = a`, `∅ ∪ a = a`,
  `a ∩ a = a`, `a ∪ a = a`, `a - a = ∅`. The existing `booleans_with_an_empty_operand_follow_the_algebra`
  and `booleans_are_idempotent_and_reflexive` cover this; the task is to make the *faces* identical,
  not just the volumes, which is the stronger and currently untested claim.
- **Refusals:** `a ∩ ∅` and `∅ ∩ a` → `Empty`; `a - a` → `Empty`; `∅ - a` → `Empty` (nothing to
  remove, and no material to return); `∅ ∩ ∅` → `Empty`.
- **Tests:** `degenerate_D11_algebra_with_an_empty_operand_changes_no_face` (a box minus the default
  `Solid`: the returned face list equals the input's, element-wise);
  `degenerate_D11b_a_minus_a_is_empty` (asserts `Err`, and that no `Ok` of volume 0 ever appears).
- **Acceptance:** matrix row D11 has this test.
- **Risk:** none material. **Fallback:** none needed.

### KER-BOOL-035 — Caps enforced during construction, not only at the end

- **State:** READY
- **Depends on:** none
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/src/solid.rs`
- **Contract:** unchanged public API; add a private
  `fn check_cap(what: &str, n: usize, cap: usize) -> Result<(), SolidError>` used at every
  accumulation point.
- **Behaviour:** `Solid::from_faces` enforces `MAX_FACES`, and `boolean` enforces it on the slot
  list — but nothing bounds the *intermediate* region sizes, the fragment counts, or the number of
  faces produced by a single plane's section. A hostile pair of operands can therefore allocate far
  past the caps before anything refuses. Check at every place a collection grows: fragments before
  chaining, regions after `from_loops`, faces before `from_faces`, loop vertices before `Face::new`.
- **Refusals:** fragments above `MAX_TRIANGLES`-scaled bound → `TooManyFaces(n, cap)` (or a new
  variant — record which in the report; `TooManyFragments` would be cleaner and needs
  `KER-BOOL-011`); loop vertices above `MAX_LOOP_VERTS` → `TooManyFaces` (the existing
  `Face::new` already maps it there, which is a name that does not fit — note it for `KER-API-003`);
  faces above `MAX_FACES` → `TooManyFaces(n, MAX_FACES)`.
- **Tests:** `a_boolean_that_would_exceed_the_face_cap_refuses` (two operands whose combined face
  count is just above `MAX_FACES` built by tiling coplanar rectangles);
  `no_allocation_exceeds_its_cap` (a test that records peak allocation is not possible without a
  dependency — instead assert that a hostile operand pair returns within the test's own timeout, and
  state in the test doc that this is the property being approximated).
- **Acceptance:** every accumulation point is checked; the cap-exceeding case returns
  `TooManyFaces` naming both numbers.
- **Risk:** an over-eager cap refuses legitimate large work. **Fallback:** set the intermediate caps
  at `4 * MAX_FACES` and document that the cap exists to bound hostile input, not to bound models.

### KER-BOOL-036 — Determinism of the boolean result

- **State:** READY
- **Depends on:** KER-BOOL-031, KER-BOOL-028
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged `boolean`.
- **Behaviour:** two runs of the same boolean on the same operands must produce identical face
  order, identical loop order and identical vertex bit patterns. Sources of non-determinism in the
  current code: `HashMap` iteration in `Solid::edges()` and `validate()` (the latter is already a
  `BTreeMap`), `BTreeMap` iteration in `weld_edges` (already ordered), the order faces are pushed
  (derived from the slot list, which `KER-BOOL-031` fixes), and `Regions` ordering out of
  `bool2d`. Audit each, convert to `BTreeMap` or an explicit sort, and pin.
- **Refusals:** none — determinism is not a refusal, but a refusal's *variant* must also be
  deterministic: if one run of a boolean pair yields `Err(A)` and another `Err(B)`, that is a
  determinism failure and must be fixed, not documented.
- **Tests:** `a_boolean_result_is_bit_identical_across_runs` (run the same pair 5 times over 40
  drawn cases; compare face count, then a serialised vertex list, then `to_bits()` of every
  coordinate); `the_error_variant_is_deterministic` (same, for refused pairs);
  `weld_edges_is_deterministic` (already covered by `KER-BOOL-004`, referenced here).
- **Acceptance:** `KER-API-014` can cite this test; every `HashMap` that reaches an output is gone.
- **Risk:** `bool2d` lives in `cadcraft_geom` and is not this task's file. **Fallback:** sort the
  output `Regions` in the kernel after every `bool2d` call (a canonical ordering by outer loop's
  lowest vertex, then holes), documented as compensating for an unordered upstream.

### KER-BOOL-037 — Cross-check the result's volume independently

- **State:** READY
- **Depends on:** KER-BOOL-036
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged; the test consumes `pub fn Solid::volume_from_faces(&self, apex: Vec3) -> Result<f64, SolidError>`
- **Behaviour:** the boolean's own `validate()` uses `mass_properties`, which integrates the
  *tessellation*. A tessellation bug therefore fails to catch itself. Every boolean result must
  agree, to a relative 1e-9, between `mass_properties` (tessellated) and `volume_from_faces`
  (pyramid sum over the faces, no triangulation) — two code paths sharing nothing. This is the
  cheapest high-value check in the kernel and it belongs on the boolean's own path, not only in
  QAT's property file.
  **Orientation:** `volume_from_faces` assumes outward normals (height is `-distance`); a result
  with one inward face loses exactly that face's contribution, so a disagreement between the two
  numbers is a direct orientation-failure detector.
- **Refusals:** `volume_from_faces` on a non-finite apex → `NonFinite`; a non-finite term → `NonFinite`;
  a disagreement above the tolerance → the boolean must return `Invalid("the result's volume
  disagrees between two independent integrations: {a} vs {b}")` rather than shipping the solid.
- **Tests:** `every_boolean_result_agrees_between_two_volume_integrations` (over 60 drawn cases, all
  three operations; asserts relative agreement 1e-9 and that no case produced a disagreement above
  `1e-7`); `a_dropped_face_shows_up_as_a_volume_disagreement` (construct a face set with one face
  removed and assert the two integrations differ).
- **Acceptance:** both tests pass; the boolean's end-of-function gate includes this check.
- **Risk:** the extra `volume_from_faces` pass costs a third traversal. **Fallback:** run it only
  when the result is below `MAX_WELD_VERTICES` faces, and say so.

### KER-BOOL-038 — Volume identities at scale

- **State:** READY
- **Depends on:** KER-BOOL-037, KER-BOOL-032
- **Files:** `crates/solid/src/boolean.rs`
- **Contract:** unchanged `boolean`.
- **Behaviour:** the three identities `vol(a\b) + vol(a∩b) = vol(a)` and
  `vol(a∪b) + vol(a∩b) = vol(a) + vol(b)`, and the three monotonicity bounds
  (`vol(a\b) <= vol(a)`, `vol(a∪b) <= vol(a)+vol(b)`, `vol(a∩b) <= min(vol a, vol b)`), asserted
  over operands with thousands of faces rather than tens, and over operands drawn at a 1e6 scale
  span as well as a 0.6 scale span. The point is to find the scale at which the tolerance clamp
  (`1e-12` floor) starts losing features, and to record that scale in the kernel's docs.
- **Refusals:** an intersection that refuses with `Empty` contributes an exact 0 to the identities —
  a refusal, never a failure; a refusal for any *other* reason means there is nothing to measure and
  the case is counted and reported, as `property.rs` already does.
- **Tests:** `volume_identities_hold_for_four_thousand_face_operands` (a 40×40 grid extruded, unioned
  with a rotated copy of itself, intersected with it); `volume_identities_hold_at_spans_from_1e-3_to_1e9`
  (the same drawn pair at five scales, asserting the volume scales as the cube to 1e-9 relative).
- **Acceptance:** both tests pass; the smallest span at which an identity fails is written into
  `boolean.rs`'s module doc as a documented limit.
- **Risk:** a 10⁹-span test will overflow intermediate products. **Fallback:** the volume
  integrations already use the bounds centre as the reference point specifically to avoid
  cancellation; keep that and let the largest scale be the one that passes, recording the limit.

### KER-BOOL-039 — Boolean at 10⁵ faces within a stated budget

- **State:** BLOCKED
- **Depends on:** KER-BOOL-005, KER-BOOL-038, KER-PERF-024, KER-PERF-032
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/benches/boolean_scale.rs` (owned by
  `KER-PERF-001`)
- **Contract:** unchanged `boolean`.
- **Behaviour:** the master plan's `KER-BOOL-005` at its full size: two solids of ~10⁵ faces each,
  overlapping, must combine inside a stated wall-clock budget **and** produce a valid solid.
  "Stated budget" is whatever `KER-PERF-001` records as the baseline; this task's job is to reach
  it, not to invent it. The result's face count, volume and validity are asserted exactly as at
  small scale.
- **Refusals:** a result above `MAX_FACES` → `TooManyFaces`; a run above the budget → **not** a
  refusal; it is a failure of this task and must be reported with the measured number. (A kernel
  that refuses to be slow is a different design, and it is not this one.)
- **Tests:** in `benches/boolean_scale.rs`, `boolean_at_1e5_faces_validates_and_meets_budget`
  (a recorded baseline in `xtask` — owned by QAT — compares; this test asserts validity, volume
  within 1e-6 relative of the 10³-face run scaled, and that it completed);
  `boolean_at_1e5_faces_agrees_with_the_small_run` (the same *shape* at 10³ and 10⁵ faces: volumes
  within 1e-6).
- **Acceptance:** the run completes in the budget; `KER-PERF-031`'s gate would pass with this task
  as its subject.
- **Risk:** the fragment count in the planar booleans may explode before any of the 3D indexing
  helps, because the cost is dominated by `cadcraft_geom::bool2d`, which is not this task's file.
  **Fallback:** add a coarse pre-pass that rejects (with `TooManyFaces`) any operand pair whose
  *expected* section complexity exceeds the cap, computed from the indexed plane pair count; that
  is a refusal with a reason rather than an hour of CPU, and it is honest.

### KER-BOOL-040 — Close-out: the three open defects un-ignored and the matrix green

- **State:** BLOCKED
- **Depends on:** KER-BOOL-001, 002, 003, 004, 011, 019..030, 035, 036, 037
- **Files:** `crates/solid/src/boolean.rs`, `crates/solid/src/extrude.rs`,
  `crates/solid/src/lib.rs` (module docs only). `crates/solid/tests/**` edits are QAT's — this
  task *requests* them in its report.
- **Contract:** no new API. Consumes every contract above.
- **Behaviour:** the master plan's §4 "three kernel defects" become zero, or become named,
  reproducible refusals. Concretely: `revolve_builds_no_solid`,
  `union_of_two_bodies_touching_across_a_plane` and `union_does_not_depend_on_the_operand_order`
  in `crates/solid/tests/property.rs` must have no `#[ignore]`; the "Known failures" module doc in
  that file and the "Four defects" sentence in `crates/solid/src/lib.rs` must be updated. Every one
  of the 23 matrix rows has a test, and `boolean()` ends in validate + validate_vertices +
  double volume integration.
  **Orientation:** stated once in the module doc of `boolean.rs` and cited by every epic below.
- **Refusals:** if any matrix row cannot be satisfied as specified, the row is not deleted — it is
  reported to the orchestrator as a contract disagreement with a concrete counter-example, per
  `docs/agents/coordination.md` §5.
- **Tests:** `the_degenerate_matrix_is_complete` — a test that reflects over nothing (Rust has no
  reflection) but instead asserts, by name, that a fixed list of row names exists as `#[test]`
  functions via a generated macro list maintained alongside it; the honest alternative is the
  orchestrator's `grep` in `cargo xtask ci`. Document which is used.
- **Acceptance:** all 23 rows have tests; the three property tests pass; the orchestrator's defect
  count is 0 for `KER`; wave 1's gate ("kernel property suite green except documented refusals") is
  met.
- **Risk:** closing one defect may un-hide a fourth that was masked. **Fallback:** land each
  defect separately and in the order 001 → 002 → 003, running the full suite after each; if the
  fourth appears, report it as a new `KER-BOOL-04x` rather than folding it into this task.

---