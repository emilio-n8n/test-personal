# `SIM` — Simulation: mesh, linear structural, thermal, nonlinear

**Owner of this document:** the SIM planning agent. **Nothing else in this file is owned by that
agent** — every task below names the files it would touch and who owns them.
**Status:** specification only. No task in this file has been executed.
**Wave:** master-plan §6 places `SIM-001` in wave 4, behind `KER`. This file expands `SIM-001..004`
and adds the epics that were implicit in them.

**Read first:** `docs/master-plan.md` (§0 constraints, §2 state machine, §6 waves),
`docs/agents/SYSTEM_PROMPT.md`, `crates/solid/src/lib.rs`, `crates/solid/src/mesh.rs`,
`crates/solid/src/mass.rs`, `crates/solid/src/solid.rs`, `crates/solid/src/section.rs`.

Task identity follows `master-plan.md` §2: `DOMAIN-EPIC-NNN`. So the first mesher task is
`SIM-MSH-004`, and the analytical benchmarks are `SIM-VAL-001..`.

---

## 0. Conventions and invariants

These are binding on every task in this file. A task that cannot honour one of them is `BLOCKED`
or must be refused at runtime.

### 0.1 Units

- **Inside `crates/sim`, every number is SI**: metres, kilograms, seconds, kelvin, newtons, pascals,
  joules, W·m⁻¹·K⁻¹, m²·s⁻¹. There is no other unit inside the crate.
- **CAD units live only at the boundary.** The document unit system (`DAT-005`) is authoritative;
  `sim::units::to_si(value, Unit) -> Result<f64, SimError>` and `from_si` are the only conversions,
  and they are called once, at the study boundary.
- **Absolute length is a numerical invariant, not a preference.** A 100 mm bracket is `0.1 m`
  everywhere in the solver. Stiffness conditioning scales with `L`; a millimetre model and a metre
  model of the same bracket are *not* the same numerical problem and must not share a cached
  factorisation. Unit conversion happens once, before meshing, and never again.
- Every `ResultSet` records the unit system it was produced in and refuses to render a mixed one.

### 0.2 Sign conventions

| Quantity | Convention |
|---|---|
| Stress `σ` | **Tension positive.** A part pulled apart has positive `σ`. |
| Strain `ε` | Tension positive; shear is **engineering** (`γ = 2ε_xy`), and every stored tensor is converted back before it is displayed. |
| Displacement `u` | Global XYZ, metres, of the material point at the undeformed node position. |
| Pressure `p` | **Positive = compressive.** Internal pressure on a face with outward normal `n̂` produces traction `t = −p·n̂`. This is the opposite sign to the stress convention on purpose, and the conversion is stated in the source at every use. |
| Applied force | Newtons, global frame, applied to a named face set. |
| Reaction `R` | `R = K·u − F`, positive when the restraint opposes the applied load. |
| Temperature | Kelvin inside the crate. °C appears only in display, through an explicit stored offset. A study whose temperature is not a number in kelvin is wrong. |
| Heat flux `q` | Fourier: `q = −k∇T`. Positive `q` is heat **leaving** the body. |
| Thermal strain | `ε = α·(T − T_ref)`, positive = expansion. `T_ref` is explicit, user-set, and recorded in the result. A study with an implicit reference temperature is refused. |
| RotDOF | Right-handed active rotation about the global axes, by the right-hand rule, in radians. |

### 0.3 Mesh quality metric — definition, not a vibe

Three metrics, all computed by `crates/sim/src/quality.rs`, all reported by every study:

- **Scaled Jacobian** `J* = det(J_e) / det(J_ref)`, evaluated at the element's Gauss points, where
  `J_e` is the Jacobian of the shape functions in physical coordinates and `J_ref` the same for the
  reference element. `J* > 0` is a correctly oriented element; `J* < 0` is **inverted** and is a
  named error, never a warning; `J* = 0` is degenerate.
- **Aspect ratio** `AR = L_max / h_min`: the longest edge over the shortest altitude, for tets and
  for triangles.
- **Triangle shape** `q = 4√3·A / (l₁² + l₂² + l₃²)` ∈ (0, 1]; equilateral is 1.

Thresholds live in `MeshParams { quality_floor, aspect_ceiling }` and are **written into the study
report next to the result**, so any number the user reads can be traced to the mesh and the
threshold that produced it. Defaults: `quality_floor = 0.20`, `aspect_ceiling = 10.0`.
A mesh that fails its own floor is `SimError::MeshQuality`, not a mesh with bad elements in it.

### 0.4 Element and node numbering

- Zero-based everywhere, matching `cadcraft_geom`'s `Vec3`/`Vec2` convention.
- Element node order is fixed per `ElementKind` and is the **positive-Jacobian ordering**: for
  `Tet4` it is the right-hand rule, and `det(J) > 0` is *asserted at element construction*, so an
  element whose nodes were given in the wrong order is `SimError::InvertedElement` at the point of
  construction, not a silently negative volume in the solve.
- Face numbering within an element is also fixed per kind (faces listed so that each face's own
  local normal points **out of** the element), and face-set membership is an element-face number.

### 0.5 The rule that outranks every other rule

> **A physically wrong answer must never be presented as a result.**

Concretely, enforced by `SIM-VAL-015`:

1. Every `ResultSet` carries a `Validation` provenance stamp. It is **not** a default.
2. `Validation::Analytical { benchmark, measured_error }` — the study passed a named analytical
   benchmark within a stated tolerance.
3. `Validation::Convergence { levels, observed_order }` — the study converged under refinement.
4. `Validation::Unverified { reason }` — everything else.
5. `Unverified` **cannot** be exported to a report, to a viewer format, or to a machine-readable
   result file. It can be seen in the viewport, and there it is watermarked. This is
   `SimError::Unverified` and it is a refusal, not a flag.

A stress number with no benchmark behind it is not a result. It is a guess with units.

---

## 1. What exists today, and what it does not

This section is the evidence base for every reuse claim in this file. It is deliberately blunt.

### 1.1 `crates/solid/src/mesh.rs` — what it gives

`mesh_from_solid(&Solid) -> Result<Mesh, SolidError>` with
`Mesh { verts: Vec<Vec3>, tris: Vec<[usize; 3]> }`.

**Genuinely reusable, and good:**

- **Exact coverage of every planar face.** `triangulate_region` clips the outer loop and each hole
  *separately* and emits hole triangles reversed, so the signed triangle area is exactly
  `outer − holes`. Pinned by `triangles_cover_the_polygon_exactly`,
  `a_region_with_a_hole_triangulates_to_outer_minus_hole` and
  `the_mesh_volume_matches_the_exact_volume_of_the_solid` (mesh volume agrees with
  `Solid::volume_from_faces` to `1e-9` on a box).
- **Winding is deterministic**: `clip_loop_ccw` measures the signed area and flips, so the output
  does not depend on whether the caller handed in a clockwise loop. `mesh.tris` is documented as
  "wound counter-clockwise seen from outside".
- **Hostile input is refused, not panicked on**: `SolidError::Empty`, `EmptyProfile`,
  `TooManyTriangles`, and a `NonFinite` point is skipped by `clip_loop`. This is the crash
  contract and `crates/sim` inherits it rather than reinventing it.
- **The scaling discipline**: `tolerance_of` scales its tolerance to the region diagonal and clamps
  to `[1e-13, 1e-3]`. `crates/sim` must use `solid::tolerance_for(diagonal)` for the same reason.
- **The cap**: `limits::MAX_TRIANGLES = 2_000_000` and the checked error.

### 1.2 `crates/solid/src/mesh.rs` — what it does **not** give

Read `mesh_from_solid` (`crates/solid/src/mesh.rs:54-74`):

```rust
for face in &solid.faces {
    for region in face.region.regions() {
        for tri in triangulate_region(region)? {
            let base = mesh.verts.len();
            mesh.verts.push(face.plane.to_world(tri[0].x, tri[0].y));
            mesh.verts.push(face.plane.to_world(tri[1].x, tri[1].y));
            mesh.verts.push(face.plane.to_world(tri[2].x, tri[2].y));
            mesh.tris.push([base, base + 1, base + 2]);
```

- **The vertices are not welded.** Three new entries are pushed per triangle. `Mesh::verts` holds
  three times as many points as the surface has vertices, and `Mesh::tris` indexes into that soup.
  This is exactly right for STL, which repeats vertices, and it is **not** an FE mesh: there are no
  shared nodes, therefore no adjacency, therefore no element, therefore no edges, therefore no
  boundary sets and no way to tell a surface face from a side face. This is the single biggest
  gap between `mesh_from_solid` and a finite-element mesh, and `SIM-MSH-013` is the work.
- **No face provenance.** The loop over `solid.faces` has an index `fi` and throws it away. A
  boundary condition cannot be attached from the returned `Mesh`. `SIM-MSH-008` needs
  `KER-MSH-001`.
- **No element types.** Triangles only. No `Tet4`, `Tet10`, `Hex8`, `Hex27`.
- **No quality control of any kind.** `drop_collinear` actively *deletes* vertices; ear clipping
  emits whatever quality the polygon admits. For a face of a long thin part this routinely produces
  sliver triangles with `q < 0.05`, i.e. below the default `quality_floor`. **A display
  tessellation is not a finite-element mesh and must not be used as one.**
- **No refinement, no size field, no conformity, no convergence.**
- **Volume, not material.** It is a boundary representation of a surface. An FE mesh needs a
  *volume* mesh — the interior filled with elements.

**Conclusion, and it is the load-bearing judgement of this file:** `mesh_from_solid` is a good
*surface triangulator* and a *bad* starting point for FEA taken as-is. `crates/sim` reuses
`triangulate_region`'s **output correctness** and its **winding and tolerance discipline**, and
rebuilds everything else: welding, element filling, quality, sets.

### 1.3 `crates/solid/src/mass.rs` — what is reusable

`mass_properties(&Solid) -> Result<MassProperties, SolidError>` with
`MassProperties { volume, area, centroid }`.

- **The reference-point discipline is the reusable part, and it matters enormously.**
  `mass.rs:33` uses `let ref_point = solid.bounds().center();` and its module doc says why: "a body
  at (1e6, 1e6, 1e6) would otherwise integrate a trillion-unit tetrahedron and lose every digit to
  cancellation". Pinned by `mass_properties_are_stable_far_from_the_origin`, which builds a box at
  ±1e6 and asserts 1e-6 accuracy. **A finite-element assembler has exactly this problem and exactly
  this fix**: element matrices must be integrated about the element's own centroid and assembled
  with parallel-axis terms, never about the world origin. This is a reusable *rule*, already
  written down in this repository's voice, and `SIM-STATIC-007` cites it.
- **The independent-cross-check pattern.** `mass.rs` keeps two computations that share no code path
  (tessellation-as-tetrahedra vs face-pyramids) and a test that they agree. `SIM-STATIC` must use
  the same pattern for every integrator: a second, independent route to each number, compared in a
  test.
- **The non-finite guard.** `mass.rs:40-42` returns `SolidError::NonFinite` the moment a
  tetrahedron volume is not finite, rather than integrating on. `crates/sim` repeats this at every
  Gauss point.
- **The tetrahedron decomposition itself** is the correct integrand pattern for anything integrated
  over a polyhedron, and `crates/sim/tests/` may re-use the tetrahedron route as a *reference
  integrator* to validate the Gauss rules against (`SIM-VAL-009`).

### 1.4 `crates/solid/src/mass.rs` — what is not reusable

- **`MassProperties` has no inertia tensor.** This is stated in the repository's own words in
  `crates/solid/tests/property_surface.rs:6-11`: "`MassProperties` carries `{volume, area, centroid}`
  and **no** inertia tensor, so the tensor is integrated here from the public mesh with the standard
  tetrahedron formula". **There is therefore no mass matrix, no lumped mass, and no modal mass
  anywhere in this repository.** `SIM-STATIC` and `SIM-THERM` (lumped capacitance) both need it and
  both must build it.
- **`Solid::volume_from_faces` doc over-promises.** `solid.rs:731` says "Volume, centroid and
  inertia, exact for the polyhedron", but `mass.rs` returns no inertia. Do not plan against that
  doc line.
- **No Gauss quadrature, no shape functions, no nodal data.** `mass_properties` returns three
  scalars about the body; an assembler needs a local-to-global map, a shape-function gradient table
  and a sparse accumulator. There is no part of that in `crates/solid`.
- **The reference point is the *bounds* centre, not the centroid.** For the integrals it computes
  that is fine. For an FE element the reference must be the element's own centroid and the
  parallel-axis term applied explicitly — the bounds centre of a mesh is not a valid element
  reference. The *rule* transfers; the *choice* does not.

### 1.5 The kernel the mesher must respect

From `crates/solid/src/solid.rs` and `crates/solid/src/lib.rs`:

- **Feature edges are `Edge { a, b, faces: [usize; 2] }`** (`solid.rs:26-31`), recovered by
  `Solid::edges() -> Option<Vec<Edge>>` (`solid.rs:518`), which returns `None` if *any* edge is not
  used by exactly two faces. For an FE mesh this is the manifold check the mesher needs, but the
  `Option` conflates "open shell" with "non-manifold" and a mesh needs both cases named. See
  `KER-MSH-002` in §2.
- **`EdgeRef { point, tolerance }`** (`chamfer.rs:39-44`) is a *geometric* selector and
  `chamfer::resolve` is **not re-exported** (`property_surface.rs:26-27` says so). There is no
  topological naming anywhere in the repository (`master-plan.md` §4). Loads and constraints must
  attach to something; today the only thing that survives an edit is a point and a radius. See
  `KER-MSH-003`.
- **`Solid::contains(&self, p: Vec3) -> bool`** (`solid.rs:492`) is a ray cast with a hard
  `|n·d| < 1e-12` skip and **no tolerance band**: a point exactly on a face is undefined, and its
  own doc says "a point exactly on a face is reported by the caller's own tolerance rules, not
  here". `crates/sim` needs a banded, signed answer for load-patch tests and for CAM stock checks.
  See `KER-MSH-004`.
- **`section(&solid, plane, tol) -> Result<Section, SolidError>`** (`section.rs:50`) is excellent and
  is exactly what slicing needs — **but the function is not re-exported**. `lib.rs:93` exports only
  the `Section` struct; `property_surface.rs:28-29` names this as a gap. See `KER-MSH-005`.
- **Every face is planar** (`lib.rs` "Planar faces only: rounds are faceted"), and a fillet is at
  most `limits::MAX_ROUND_SEGMENTS = 64` flat facets (`lib.rs:111-112`). **There are no curved
  faces to conform a mesh to.** Every "accurate stress near a fillet" claim is bounded by a
  modelling-time facet count, not by the analysis. `SIM-MSH-020` carries this limitation through to
  the report.

---

## 2. Blocked on `KER` — the exact queries this plan needs

These are the interfaces `crates/sim` and `crates/cam` consume and cannot currently build. Each is
specified as it would need to exist. They are **not** requests to change `crates/solid` — they are
contracts for the `KER` owner to accept, reject or replace. A rejected contract makes the consuming
task `BLOCKED` permanently and the coordinator re-plans it.

| Req ID | Signature | Why SIM/CAM needs it | Evidence |
|---|---|---|---|
| `KER-MSH-001` | `pub fn mesh_from_solid_tracked(solid: &Solid) -> Result<TrackedMesh, SolidError>` with `pub struct TrackedMesh { pub mesh: Mesh, pub tri_face: Vec<usize>, pub face_tris: Vec<Vec<u32>> }` | Boundary conditions, loads and meshes attach to *faces*. Today the mapping is discarded. Without it every face set must be re-derived geometrically and will drift. | `mesh.rs:56-68` — the `fi` index is never stored. |
| `KER-MSH-002` | `pub struct EdgeTopology { pub edges: Vec<Edge>, pub open: Vec<usize>, pub nonmanifold: Vec<usize> }` and `pub fn edge_topology(solid: &Solid) -> Result<EdgeTopology, SolidError>` | An FE mesh must be closed and manifold. `Solid::edges()` returns `Option` and conflates open-shell with non-manifold; `validate()` names `OpenEdge` but not which case a mesher faces. | `solid.rs:518-551`; `solid.rs:600-604`. |
| `KER-MSH-003` | `pub struct FaceId(pub u64); pub struct EdgeId(pub u64);`<br>`pub fn face_id(solid: &Solid, at: Vec3, tol: f64) -> Result<FaceId, SolidError>`<br>`pub fn edge_id(solid: &Solid, ref_: &EdgeRef) -> Result<EdgeId, SolidError>`<br>`pub fn faces_of_edge(solid: &Solid, e: EdgeId) -> Result<[FaceId; 2], SolidError>`<br>`pub fn faces_of(solid: &Solid, ids: &[FaceId]) -> Result<Vec<Face>, SolidError>` | Persistent identity for loads, constraints and CAM selections. `MOD-002` (topological naming) is the real owner; SIM needs the *read* side of it. Without it, a study's boundary conditions re-target silently after every rebuild. | `master-plan.md` §4: "Topological naming: **None.**"; `chamfer.rs:33-44`. |
| `KER-MSH-004` | `pub enum Containment { Inside, Outside, OnSurface { signed_distance: f64 } }`<br>`pub fn locate(solid: &Solid, p: Vec3, tol: f64) -> Result<Containment, SolidError>`<br>`pub fn signed_distance(solid: &Solid, p: Vec3) -> Result<f64, SolidError>` | Load patches that straddle an edge, "is this face set convex" tests, and CAM's "is the stock here" test all need a banded answer. `contains` is a hard ray cast with a `1e-12` skip. | `solid.rs:487-512`. |
| `KER-MSH-005` | Re-export `section`: `pub use section::{Section, section};` | Slicing a solid for a layered hex/tet mesh, for CAM 2.5D height maps and for the section cut-plane in result visualisation. **This is a one-line change with a real cost: it is the `KER` owner's file.** | `lib.rs:93`; `property_surface.rs:28-29`. |
| `KER-MSH-006` | `pub fn closest_point(solid: &Solid, p: Vec3, tol: f64) -> Result<(Vec3, f64), SolidError>` | CAM gouge checking (tool tip vs part) and SIM contact seeding. There is no distance-to-shell query anywhere; only `Plane::distance` per face. | `plane.rs:104-106`; absence elsewhere. |
| `KER-MSH-007` | `pub fn raycast(solid: &Solid, origin: Vec3, dir: Vec3, dir_tol: f64) -> Result<Vec<f64>, SolidError>` returning hit distances | CAM lead-in entry points, SIM's ray-casting *load* definition (a pressure over a beam specified between two points), and any "project onto the part" operation. | absence; `contains` casts one fixed ray. |
| `KER-MSH-008` | `pub fn point_in_face(face: &Face, p: Vec3, tol: f64) -> Result<Distance2, SolidError>` | Load distribution on a face: every load integration point must be tested against the face's own region, not against the whole solid. | `solid.rs:91-93` — `contains_local` exists but is boolean, bandless and in-plane only. |
| `KER-SRF-003` | `KER-SRF-003` (master-plan §5) | NURBS/analytic surfaces. Until it lands, every rounded feature is a faceted prism and every stress concentration result near a fillet carries a modelling error that must be reported, not hidden. | `lib.rs` "Planar faces only"; `limits::MAX_ROUND_SEGMENTS`. |

**Rule:** a `crates/sim` task that consumes one of these is `BLOCKED` until the `KER` contract is
`VALIDATED`. It is not `BACKLOG`, and it is not "assume and proceed" (`coordination.md` §1).

---

## 3. Epics

| Epic | Name | Tasks | Gate |
|---|---|---|---|
| `SIM-VAL` | Analytical verification | 16 | **First. Nothing else starts without it.** |
| `SIM-MSH` | Mesher | 26 | `SIM-VAL-001..015` |
| `SIM-SLV` | Sparse solver and the solver decision | 14 | `SIM-SLV-001` (the spike) |
| `SIM-STATIC` | Linear structural | 24 | `SIM-SLV-005..007`, `SIM-MSH-016` |
| `SIM-THERM` | Thermal, steady and transient | 16 | `SIM-SLV-007`, `SIM-VAL-011..013` |
| `SIM-NL` | Nonlinearity, contact, plasticity | 14 | `SIM-STATIC-*`, `SIM-SLV-011`, `KER-MSH-006` |
| `SIM-VIZ` | Result visualisation | 14 | `SIM-VIZ-001` only needs the result types |

**Total: 124 tasks.**

**Epics are listed with `SIM-VAL` first deliberately.** The file is ordered the way the work must be
done, not the way the code is layered.

---

## 4. The honesty statement

> **State plainly, in the product, in the report and in the source: which results a from-scratch
> solver can be trusted for, and which require validated third-party code that this project does not
> have.**

### Tier A — trustworthy from scratch, once `SIM-VAL` passes

- **Linear static, isotropic linear elasticity**, on a mesh that passes the §0.3 quality floor,
  with a statically determinate or properly restrained model, on tet4/tet10/hex8/quad4 elements.
  Verifiable against Euler–Bernoulli, Timoshenko, Navier plates, Lamé and the constant-strain
  patch test. Confidence comes from the benchmarks, not from the code being elegant.
- **Steady-state heat conduction**, same conditions. Exact for a 1D slab, verifiable by energy
  balance.
- **Transient conduction** with backward Euler, in the diffusive regime, verified against the exact
  1D series.
- **Prescribed-displacement and modal problems** once the stiffness path is validated.
- **Dead-weight (gravity) loading** — it is a linear static problem with a body force.

The accuracy bar these must clear is written down in `SIM-VAL-001` and is not negotiable after the
fact: ≤ 2 % on deflection against Euler–Bernoulli for meshes that are not refined past `h/L = 1/8`,
≤ 1 % on stress against the closed forms, and a solver residual `‖Au − f‖/‖f‖ ≤ 1e-10`.

### Tier B — from scratch, internally consistent, **not a design number**

- Large deformation with a hand-written Newton loop.
- Transient structural dynamics with a hand-rolled integrator.
- Material nonlinearity with a custom return map.

These may be implemented, but every result they produce is `Validation::Unverified` unless it
matches a benchmark, and the report says so in words.

### Tier C — refused, not approximated

**Contact, plasticity, creep, fatigue, buckling, composite layups, fracture, impact/explicit
dynamics, fluid–structure coupling, and thermal contact conductance are not implemented in
CADCraft.** Requesting them returns `SimError::Unsupported` with a message that says which of them
it was. Nothing in the UI may present a partial approximation of these as if it were the thing.

The reason is not effort. It is that **a wrong stress number shown confidently in a viewport is the
worst outcome this domain can produce** — worse than saying nothing, worse than a crash, because
the user cannot tell. A contact solution that is 30 % wrong looks exactly like one that is right,
and an engineer who trusts it makes a part that fails.

### Verification-first ordering

```
SIM-VAL-001 (the bar)  →  SIM-VAL-002..014 (the benchmarks, pure reference data)
        →  SIM-VAL-015 (provenance machinery)
        →  SIM-VAL-016 (convergence harness)
        →  SIM-SLV-001 (the solver spike)
        →  SIM-STATIC (assembly)
        →  SIM-MSH-008..016 (mesh with sets + quality)
        →  everything else
```

**No assembler task, no solver task, no viewport task and no export task may start before
`SIM-VAL-002..015` are `VALIDATED`.** The benchmarks are cheap — they are closed-form solutions and
a tolerance. They are also the only thing standing between this project and a confident wrong
number.

---

## 5. `SIM-VAL` — Analytical verification (16 tasks)

This epic has no dependency on any other epic. It is first because it costs almost nothing and
because everything else is worthless without it.

### SIM-VAL-001 — Write the accuracy bar down before any code exists
- **State:** READY
- **Depends on:** none
- **Files:** `crates/sim/src/verify/mod.rs` (new, SIM)
- **Contract:** `pub struct AccuracyBar { pub deflection_rel: f64, pub stress_rel: f64, pub thermal_rel: f64, pub residual: f64 }` and `pub fn default_bar() -> AccuracyBar` (`0.02`, `0.01`, `0.02`, `1e-10`).
- **Behaviour:** Define the tolerance each benchmark class must meet and the residual each solve must reach. State per class what the tolerance is measured against — a converged reference, an analytical solution, or a literature value. Make the bar a value, not a comment, so `SIM-VAL-016` and the CI can consume it.
- **Refusals:** `AccuracyBar` with a non-finite or negative tolerance is `SimError::BadTolerance`. Relaxing a bar after a study fails it is a code change with a `git` blame, not a runtime option.
- **Tests:** `default_bar_is_finite_and_positive` — asserts each field is finite and > 0. `bar_relaxation_is_not_a_runtime_api` — asserts there is no public function that widens the bar.
- **Acceptance:** `AccuracyBar` exists, is serialised into every study report header, and `cargo xtask layers` still passes.
- **Risk:** The bar is set optimistically and quietly revised later. Fallback: a `docs/` entry recording any revision with its justification, which CI checks for.

### SIM-VAL-002 — Cantilever tip deflection against Euler–Bernoulli
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/beam.rs` (new, SIM)
- **Contract:** `pub fn cantilever_tip_deflection(length: f64, load: f64, youngs: f64, inertia: f64) -> f64` returning `P·L³/(3·E·I)`, and `pub struct CantileverCase { pub length: f64, pub load: f64, pub youngs: f64, pub inertia: f64, pub expected_tip_deflection: f64 }` with `pub fn cantilever_cases() -> Vec<CantileverCase>`.
- **Behaviour:** The primary benchmark: a prismatic cantilever with a tip point load. Record `PL³/3EI` for a minimum of three aspect ratios (L/h = 4, 10, 40) so shear compliance is visible. State in the source that for L/h = 4 this is *not* the true answer — Timoshenko shear deflection `PL/(4κGA)` is a real fraction there, and that case exists to catch a solver that silently ignores shear.
- **Refusals:** `L ≤ 0`, `E ≤ 0`, `I ≤ 0` → `SimError::BadDistance`. A case with a non-finite load → `SimError::NonFinite`.
- **Tests:** `tip_deflection_matches_the_closed_form` — asserts each case matches `PL³/3EI` to 1e-12 relative. `aspect_ratio_is_carried_in_the_case` — asserts the three cases have distinct L/h. `shear_is_not_in_the_euler_formula` — asserts the L/h = 4 case's stored expectation excludes `PL/(4κGA)`, documenting the gap.
- **Acceptance:** Three cases with expectations pinned, and `docs/geometry-parity.md` untouched — this is reference data, not a claim about the geometry engine.
- **Risk:** The benchmark is wrong. Fallback: two independent derivations are cross-checked in the same test (`PL³/3EI` via the unit-load method and via the direct integral).

### SIM-VAL-003 — Cantilever tip rotation and fixed-end moment
- **State:** READY
- **Depends on:** SIM-VAL-002
- **Files:** `crates/sim/src/verify/beam.rs` (SIM)
- **Contract:** `pub fn cantilever_tip_rotation(length: f64, load: f64, youngs: f64, inertia: f64) -> f64` (`PL²/2EI`) and `pub fn cantilever_base_moment(load: f64, length: f64) -> f64` (`P·L`).
- **Behaviour:** Deflection alone does not prove a solver: a linear function with the right slope and the wrong intercept passes it. Rotation at the tip and the reaction moment at the root are independent observables with different shapes along the beam. Together with `SIM-VAL-002` they pin the shape of the moment solution, not just its end value.
- **Refusals:** as `SIM-VAL-002`. `length == 0.0` → `SimError::BadDistance`.
- **Tests:** `tip_rotation_matches_the_closed_form` — `PL²/2EI` to 1e-12. `base_moment_is_the_load_times_the_arm` — `P·L` exactly. `rotation_derivates_the_deflection` — asserts `d(deflection)/dx` at the tip equals the rotation to 1e-12, a relation between two independent expectations.
- **Acceptance:** Both formulas implemented with a derivation comment and a test that cross-checks them against each other.
- **Risk:** None material; these are textbook. Risk is only that they are skipped as "easy".

### SIM-VAL-004 — Cantilever axial stiffness
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/beam.rs` (SIM)
- **Contract:** `pub fn cantilever_axial_extension(length: f64, area: f64, force: f64, youngs: f64) -> f64` (`P·L/(E·A)`) and `pub fn bar_stress(force: f64, area: f64) -> f64` (`P/A`).
- **Behaviour:** The uniaxial case, which isolates the constitutive matrix from every geometric effect. A solver that gets this wrong has a `D` matrix bug and nothing else in this plan will find it.
- **Refusals:** `area <= 0.0` → `SimError::BadDistance`. `youngs <= 0.0` → `SimError::BadDistance`.
- **Tests:** `axial_extension_is_pl_over_EA` — 1e-12 relative. `a_bar_in_tension_reports_positive_stress` — asserts `bar_stress` > 0 for a tensile force, pinning the §0.2 tension-positive convention.
- **Acceptance:** Implemented, tested, and referenced by `SIM-STATIC` and `SIM-NL-008`.
- **Risk:** Conflating "stress positive in tension" with the pressure convention later. Fallback: the sign convention table in §0.2 is quoted in the source of every place the two meet.

### SIM-VAL-005 — Simply supported beam, midspan deflection
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/beam.rs` (SIM)
- **Contract:** `pub fn simply_supported_midspan(length: f64, load: f64, youngs: f64, inertia: f64, shear_area: f64, shear_modulus: f64) -> f64` returning `5PL³/(384EI) + PL/(4κGA)` with `κ = 5/6` for a rectangle, and `pub struct ShearCorrection { pub kappa: f64 }`.
- **Behaviour:** The first benchmark with **two** boundary conditions that are not rigid and with **two** deformations. It exercises constraint symmetry (`w = 0` at two ends, `w' = 0` at one), which a cantilever never does, and it adds the shear term, which is the first thing that separates a Timoshenko element from an Euler one.
- **Refusals:** `shear_area <= 0.0` or `shear_modulus <= 0.0` → `SimError::BadDistance`, rather than silently dropping the shear term. **A missing shear term must be a refusal, not a default.**
- **Tests:** `midspan_matches_timoshenko` — against the two-term formula to 1e-12. `the_shear_term_matters_on_a_deep_beam` — asserts the `L/h = 2` case differs from `5PL³/384EI` by more than 20 %, proving the term is not decorative. `a_dof_left_free_floats` — asserts the unrestrained rotational DOF at the roller support is the one the fixture removes; documents the intended constraint set.
- **Acceptance:** Implemented and consumed by `SIM-STATIC-003`.
- **Risk:** Getting `κ` wrong for a non-rectangular section. Fallback: `κ` is a `ShearCorrection` field, not a constant, so a study states the value it used.

### SIM-VAL-006 — Cantilever under an end moment (pure bending, no shear)
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/beam.rs` (SIM)
- **Contract:** `pub fn cantilever_end_moment_deflection(moment: f64, length: f64, youngs: f64, inertia: f64) -> f64` (`M·L²/(2EI)`).
- **Behaviour:** Separates bending from the `P³/3EI` deflection by giving a load with zero net force and zero shear. A solver that mishandles load application (applying the moment at the wrong node, or the wrong lever arm) fails here and only here.
- **Refusals:** as `SIM-VAL-002`.
- **Tests:** `end_moment_deflection_matches_the_closed_form` — `ML²/2EI` to 1e-12. `end_moment_produces_no_net_force` — asserts the associated load vector sums to zero force and a non-zero moment.
- **Acceptance:** Implemented and used by `SIM-STATIC-014`'s moment-application test.
- **Risk:** None.

### SIM-VAL-007 — Simply supported square plate, uniform load, Navier series
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/plate.rs` (new, SIM)
- **Contract:** `pub fn navier_square_plate(a: f64, b: f64, thickness: f64, youngs: f64, poisson: f64, pressure: f64, terms: usize) -> Result<f64, SimError>` returning the centre deflection from the doubly-symmetric Navier series, plus `pub fn plate_rigidity(youngs: f64, poisson: f64, thickness: f64) -> f64` (`E·t³/(12(1−ν²))`).
- **Behaviour:** The plate benchmark, truncated at `terms` terms and **converged until the next term changes the sum by less than 1e-12 of the total**. The truncation must be visible in the result, not silent: a caller gets a `Result` precisely so that "the series did not converge" is a named outcome. This is also the benchmark that requires genuinely 2D elements (a mesh of tets gives the right answer only with volumetric locking controlled, see `SIM-VAL-014`).
- **Refusals:** `terms < 1`, `thickness <= 0.0`, `youngs <= 0.0`, or `poisson >= 0.5` or `<= -1.0` → `SimError::BadDistance`. A series that has not converged at `terms` → `SimError::Unsupported("the Navier series did not converge at {terms} terms")`. **Truncating and reporting anyway is the failure mode this task exists to prevent.**
- **Tests:** `navier_converges_for_a_slab` — for `a/b = 2`, `a/t = 100`, the series must converge in fewer than 200 terms and match `0.00406·q·a⁴/D` (the published coefficient for a square plate, simply supported on all four edges, uniform load) to 1e-6. `navier_refuses_a_thick_plate` — for `a/t = 1`, asserts an explicit non-convergence `Err` rather than a number. `poisson_near_one_half_does_not_diverge` — asserts the result is finite for `ν = 0.4999`.
- **Acceptance:** Both constants (`0.00406`, `0.00540` for `a/b = 2`) pinned in tests with the reference cited in a comment.
- **Risk:** The published coefficient is wrong or misremembered. Fallback: `SIM-VAL-008` independently checks the series against the beam limit, and the coefficient test cites the source.

### SIM-VAL-008 — Kirchhoff plate strip reduces to the beam
- **State:** READY
- **Depends on:** SIM-VAL-007, SIM-VAL-002
- **Files:** `crates/sim/src/verify/plate.rs` (SIM)
- **Contract:** `pub fn plate_strip_matches_beam(terms: usize) -> Result<(f64, f64), SimError>` returning `(plate_centre_deflection, beam_centre_deflection)` for a long thin simply supported strip loaded like `SIM-VAL-005`.
- **Behaviour:** A consistency check between two independent closed forms: for `a/L → 0` a simply supported plate strip *is* a beam, and its series must collapse onto `5PL³/384EI`. Two closed forms agreeing where theory says they must is far stronger evidence than either matching a remembered constant.
- **Refusals:** A strip aspect ratio below the thin-plate limit is refused: `SimError::Unsupported("a strip of a/t = {x} is not in the Kirchhoff regime")`. Running the comparison outside the regime it is valid for is how a benchmark becomes a lie.
- **Tests:** `strip_reduces_to_the_beam` — for `a/t = 200`, the two agree to 1e-4 relative. `a_thick_strip_is_refused` — asserts `Err` for `a/t = 2`. `the_series_is_monotone_in_terms` — asserts each additional term changes the sum by less than the previous one, which catches a sign error in the series.
- **Acceptance:** Runs green and is wired into `SIM-VAL-016`'s suite.
- **Risk:** The agreement is coincidental at one aspect ratio. Fallback:** assert it at three aspect ratios (100, 200, 400) and require monotone convergence.

### SIM-VAL-009 — Constant-strain patch test
- **State:** READY
- **Depends on:** SIM-VAL-004
- **Files:** `crates/sim/src/verify/patch.rs` (new, SIM)
- **Contract:** `pub fn constant_strain_patch_expectation(load: f64, area: f64, youngs: f64) -> f64` and `pub fn run_constant_strain_patch(case: &PatchCase) -> Result<PatchReport, SimError>` with `pub struct PatchReport { pub stress_error: f64, pub nodal_stress_spread: f64 }`.
- **Behaviour:** The element-level verification test: **one distorted element**, loaded so the exact solution is a constant strain `σ = P/A`. The exact stress is known without any element being good. A single-element test that returns the right number proves the shape functions, the Jacobian, the B matrix, the constitutive matrix and the assembly are all mutually consistent. Every other benchmark in this epic assumes the elements work; this one proves they do.
- **Refusals:** `SimError::MeshQuality` if the test element's scaled Jacobian falls below the floor — **the patch test must be run on a *good* element**, so a degenerate one is refused rather than reported as a solver failure. This distinction matters and is the reason the two errors are separate.
- **Tests:** `a_distorted_element_returns_the_exact_stress` — one `Tet4` with a Jacobian distortion factor of 0.3 returns `P/A` to 1e-12. `an_inverted_test_element_is_a_mesh_error_not_a_solver_error` — asserts the specific error variant. `the_exact_solution_is_constant` — asserts the spread of recovered nodal stress is below 1e-10, which is the definition of the patch test.
- **Acceptance:** Green, and cited by name in `SIM-STATIC-013`'s module doc as the justification for its stress recovery.
- **Risk:** Running it on several elements, which turns a verification test into a convergence test. Fallback: the single-element restriction is in the type — `PatchCase` takes one element's connectivity.

### SIM-VAL-010 — Inf-sup / Babuška–Osaka patch test on an elongated bar
- **State:** BACKLOG
- **Depends on:** SIM-VAL-009, SIM-MSH-017
- **Files:** `crates/sim/src/verify/patch.rs` (SIM)
- **Contract:** `pub fn babuska_osaka_dispersion(seed: u64, aspect: f64, distortion: f64) -> Result<Vec<f64>, SimError>` returning the singular values of the scaled stiffness matrix for the standard distorted-bar family, and `pub fn dispersion_is_above(threshold: f64, values: &[f64]) -> bool`.
- **Behaviour:** The MacNeal–Hughes family of 2D/3D patch tests: a set of highly distorted meshes that no element passes exactly, compared across element types. The ratio of the smallest to the largest singular value measures ill-conditioning; a solver that fails here fails on real models. Deterministic by a seed argument — no `rand`, following `crates/geom/src/property_tests.rs`.
- **Refusals:** An aspect ratio or distortion outside `[0.1, 0.9]` → `SimError::BadTolerance`. Meshes so distorted that `solid`'s own validator rejects them are refused by `SimError::MeshQuality`, not analysed.
- **Tests:** `linear_tets_do_not_lock` — asserts the smallest singular value is within `1e-6` of the largest on the reference mesh (no locking). `distorted_tets_stay_conditioned` — asserts the ratio stays above the threshold for the declared family. `the_same_seed_gives_the_same_mesh` — asserts two calls with the same seed are identical. `hostile_aspect_does_not_panic` — asserts `Err`, not a panic, for aspect 0.0 and 1.0.
- **Acceptance:** Green for `Tet4` and `Tet10`, with the threshold table stored alongside so a future element type can be added without rewriting the test.
- **Risk:** Threshold values become permissive over time. Fallback: thresholds live in one `const` block in `patch.rs` with a comment naming the run that justified each.

### SIM-VAL-011 — Steady 1D conduction: exact profile and exact flux
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/verify/thermal.rs` (new, SIM)
- **Contract:** `pub fn slab_temperature(x: f64, thickness: f64, t_hot: f64, t_cold: f64) -> f64` (linear) and `pub fn slab_heat_flux(k: f64, thickness: f64, t_hot: f64, t_cold: f64) -> f64` (`k·(T_hot − T_cold)/L`), plus `pub struct ThermalBenchmark { pub name: &'static str, pub expected: f64, pub tolerance: f64 }`.
- **Behaviour:** Thermal's analogue of `SIM-VAL-011`'s structural siblings: a homogeneous slab with Dirichlet ends has an exactly linear temperature profile and an exactly constant flux, for **any** mesh. This is the cheapest thermal verification that exists and it catches a sign error in the conduction matrix immediately (the profile comes out upside down).
- **Refusals:** `k <= 0.0` → `SimError::BadDistance`. `t_hot == t_cold` is legal and must return zero flux, **not** an error.
- **Tests:** `the_profile_is_linear` — asserts `T(x)` is linear to 1e-12 at 5 interior points, using an independent route (midpoint average of the nodal values vs the analytic value). `the_flux_matches_k_dt_over_L` — 1e-12. `a_flux_is_negative_when_heat_flows_in_the_negative_x_direction` — pins the §0.2 Fourier sign convention. `an_insulating_slab_carries_no_heat` — asserts zero flux for equal end temperatures without erroring.
- **Acceptance:** Green and consumed by `SIM-THERM-005`.
- **Risk:** None; this is the most robust benchmark in the file.

### SIM-VAL-012 — Transient 1D conduction against the exact series
- **State:** READY
- **Depends on:** SIM-VAL-011
- **Files:** `crates/sim/src/verify/thermal.rs` (SIM)
- **Contract:** `pub fn slab_series_temperature(fourier: f64, terms: usize) -> Result<f64, SimError>` (the Fourier series for a slab cooled from one face) and `pub fn lumped_capacitance_exponential(time: f64, rho: f64, volume: f64, area: f64, cp: f64, h: f64, t0: f64, t_inf: f64) -> Result<f64, SimError>` (`T_inf + (T0 − T_inf)·e^(−hA·t/(ρ·V·c_p))`).
- **Behaviour:** Two independent analytical references for the transient problem, and the exact reduction from a series to a lumped exponential as `Bi = hL/k → 0`. The lumped form is *the* check that a transient study conserves energy: if `ρ·V·c_p·dT/dt = −h·A·(T − T_inf)` is not reproduced, the time integration or the heat capacity is wrong. Converge the series until the next term is below 1e-14 of the sum, or refuse.
- **Refusals:** `terms < 1` → `SimError::BadTolerance`. `rho <= 0.0`, `cp <= 0.0`, `volume <= 0.0` → `SimError::BadDistance`. A series that does not converge in `terms` → `SimError::Unsupported`. **`exp` overflow (`h·A·t/(ρVc)` very large) returns `T_inf`, not `inf` and not an error** — that is the correct limit and it is implemented, not left to `f64`.
- **Tests:** `the_series_reduces_to_the_lumped_exponential_at_low_bi` — asserts they agree to 1e-6 for `Bi = 1e-4`. `the_series_is_monotone_in_terms` — each term's contribution shrinks. `a_long_time_reaches_the_ambient` — asserts `T(t) → T_inf` within 1e-12 for `t = 10τ`, **without overflowing**. `hostile_thermal_parameters_do_not_panic` — asserts `Err` for zero density, zero `cp`, negative `h`.
- **Acceptance:** Green and wired to `SIM-THERM-006..008`.
- **Risk:** The series is evaluated at `t = 0` where it is slowest to converge. Fallback: the test set excludes `t = 0` and says why in a comment; `t = 0` is `T0` by definition and needs no series.

### SIM-VAL-013 — 2D steady conduction with an independent 1D reduction
- **State:** BACKLOG
- **Depends on:** SIM-VAL-011
- **Files:** `crates/sim/src/verify/thermal.rs` (SIM)
- **Contract:** `pub fn square_laplace_centre_temperature(side: f64, t_hot: f64, t_cold: f64, terms: usize) -> Result<f64, SimError>` (the Fourier series for a square with two opposite faces at `T_hot` and two at `T_cold`) and `pub fn strip_centre_temperature_is_half_of_hot_minus_cold(...) -> Result<f64, SimError>`.
- **Behaviour:** The symmetry identity: by superposition and symmetry, the centre of a square heated on two opposite faces sits at exactly the arithmetic mean of the hot and cold temperatures. That is a true statement for the continuum solution **and** it must hold for a symmetric discrete solve. It is a benchmark that needs no series and no remembered constant, and it cannot be passed by a mis-signed matrix.
- **Refusals:** `side <= 0.0` → `SimError::BadDistance`. An asymmetric discretisation (the series assumes the symmetric mesh) is the caller's responsibility and is documented in the function.
- **Tests:** `the_centre_is_the_mean_of_the_two_faces` — 1e-9. `a_hotter_hot_face_raises_the_centre` — monotonicity in `T_hot`. `an_all_cold_slab_is_uniformly_cold` — asserts every value equals `T_cold` when both are.
- **Acceptance:** Green; used by `SIM-THERM-005`.
- **Risk:** The continuum identity does not hold exactly on a coarse asymmetric mesh. Fallback: the tolerance is set from the mesh and reported, not assumed.

### SIM-VAL-014 — Thick-walled cylinder: Lamé, and volumetric locking
- **State:** BACKLOG
- **Depends on:** SIM-VAL-004, SIM-VAL-009, SIM-MSH-013
- **Files:** `crates/sim/src/verify/patch.rs` (SIM)
- **Contract:** `pub fn lame_hoop_stress(r: f64, inner: f64, outer: f64, pressure: f64) -> Result<f64, SimError>` (`σ_θθ = p·b²/(b²−a²)·(1 + a²/r²)`) and `pub fn lame_radial_stress(...) -> Result<f64, SimError>` (`σ_rr = p·a²/(b²−a²)·(1 − b²/r²)`).
- **Behaviour:** The benchmark that decides whether **tetrahedra** may be used at all. A full-integration linear tet locks in bending and in incompressible-elasticity limit cases: it converges far too slowly and may not converge at all at `ν → 0.5`. A mesh of tets against Lamé is therefore both a correctness check and a performance verdict. Incompressible behaviour (`ν = 0.4999`) must be tested explicitly, because that is where locking bites.
- **Refusals:** `inner <= 0.0`, `outer <= inner` → `SimError::BadDistance`. `pressure <= 0.0` is *legal* (external pressure) and returns the sign-correct result. A `ν` within `1e-6` of `0.5` must be **refused** with `SimError::Unsupported("ν within 1e-6 of 0.5 needs a mixed formulation; see SIM-NL-012")` rather than solved with an element that will lock.
- **Tests:** `hoop_stress_matches_lame_on_the_inner_surface` — 1e-6. `radial_stress_is_zero_on_the_outer_surface` — 1e-12. `hoop_stress_at_the_inner_surface_exceeds_the_pressure` — asserts `σ_θθ(a) = p(a²+b²)/(b²−a²) > p` for `b > a`. `a_tet_mesh_does_not_lock_at_high_poisson` — asserts the error at `ν = 0.4999` is below the bar, **or records that it is not and names the element type that is**.
- **Acceptance:** Green, and its conclusion — "tets are usable" or "tets lock and we need hex or a mixed method" — is written into `docs/geometry-parity.md` and into the `ElementKind` doc comment. **A negative result here is a legitimate and valuable outcome of this task.**
- **Risk:** Discovering that linear tets lock and that hex meshing is then on the critical path. Fallback: `SIM-MSH-022` already schedules hex8 meshing, and `SIM-MSH-024` records the verdict.

### SIM-VAL-015 — Provenance machinery on every result
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/results.rs` (new, SIM)
- **Contract:**
  ```rust
  pub enum Validation {
      Analytical { benchmark: &'static str, measured_error: f64 },
      Convergence { levels: usize, observed_order: f64 },
      Unverified { reason: String },
  }
  pub struct ResultSet { pub name: String, pub validation: Validation, pub units: UnitSystem, pub mesh_fingerprint: u64, pub solver: &'static str, pub fields: Vec<ResultField> }
  pub fn exportable(&self) -> bool;
  ```
- **Behaviour:** The mechanism that enforces §0.5. `ResultSet` cannot be constructed without a `Validation`. `exportable()` is `false` for every `Unverified` variant. The mesh fingerprint is a hash of the node table and element table, so a result can never be read next to a different mesh than the one that produced it — the single most common way a stress number gets misread.
- **Refusals:** A `ResultSet` built with `Validation::Unverified` and handed to an exporter is `SimError::Unverified`, not a warning. A result whose `mesh_fingerprint` does not match the mesh it is displayed against is refused by the display path with the same error.
- **Tests:** `a_result_cannot_be_built_without_validation` — asserts there is no constructor that defaults it. `an_unverified_result_is_not_exportable` — asserts `exportable()` is false. `a_mesh_fingerprint_change_is_detected` — asserts that adding one node changes it. `validation_survives_a_round_trip_through_the_report_format` — asserts the stamp is in the serialised form.
- **Acceptance:** `ResultSet` and `Validation` exist, `SIM-STATIC-019` builds only through them, and no other constructor exists in the crate.
- **Risk:** A later developer adds a convenience constructor that omits the stamp. Fallback: `results.rs` is a small file with a `//!` rule stating the rule, and the fingerprint check is a test in `results.rs` that fails to compile if the struct gains a public field.

### SIM-VAL-016 — Convergence harness
- **State:** BACKLOG
- **Depends on:** SIM-VAL-015, SIM-MSH-020, SIM-SLV-007
- **Files:** `crates/sim/src/verify/convergence.rs` (new, SIM), `crates/sim/tests/verify.rs` (new, SIM)
- **Contract:** `pub fn observe_order(sizes: &[usize], values: &[f64]) -> Result<f64, SimError>` (least-squares slope of `log|u_h − u_exact|` against `log h`) and `pub fn run_benchmark(name: &str, levels: usize) -> Result<BenchmarkResult, SimError>` with `pub struct BenchmarkResult { pub name: String, pub levels: usize, pub order: f64, pub measured_error: f64, pub passed: bool }`.
- **Behaviour:** Refine a benchmark mesh N times, solve, measure the error against the analytical value, and fit the observed rate. `passed` requires both `measured_error ≤ bar` **and** `order ≥ order_floor`. Print the table in the report: level, node count, error, observed order. A convergence table that shows the error not falling is the most useful diagnostic this project can produce about its own solver.
- **Refusals:** `sizes.len() != values.len()` or fewer than 2 levels → `SimError::BadTolerance`. A fitted `order` that is negative (the error is growing) → `passed = false` **and** an `Err` carrying the table, because a solver whose error grows with refinement is broken and saying so is the whole point.
- **Tests:** `a_first_order_element_shows_first_order` — a `Tet4` bar converges at `order > 0.9` with a nested uniform mesh. `a_second_order_element_shows_second_order` — `Tet10` exceeds `1.8`. `a_growing_error_is_reported_not_hidden` — asserts a deliberately perturbed value sequence yields `passed == false` and a named `Err`. `the_table_is_printed` — asserts the formatted table contains the level count and the final error.
- **Acceptance:** Every `SIM-VAL` benchmark runs through this harness; `passed == true` for all of them at the declared levels; the table is in the study report.
- **Risk:** A benchmark that converges at order 1 but has a 10 % error constant never passes the error bar. Fallback: report both numbers always, so a user can see the constant and the order separately.

---

## 6. `SIM-MSH` — Mesher (26 tasks)

### SIM-MSH-001 — The element-kind table and its node orderings
- **State:** READY
- **Depends on:** none
- **Files:** `crates/sim/src/mesh/element.rs` (new, SIM)
- **Contract:** `pub enum ElementKind { Tet4, Tet10, Hex8, Hex27, Tri3, Tri6, Quad4, Quad8, Wedge6 }` with `pub fn node_count(self) -> usize`, `pub fn dimension(self) -> usize`, `pub fn faces(self) -> &'static [[usize; 4]]`, `pub fn reference_nodes(self) -> &'static [Vec3]`, and `pub fn face_normal_sign(self, face: usize, node: usize) -> f64`.
- **Behaviour:** The single definition of every element's connectivity and ordering, referenced by every other mesher and assembler task. The orderings are the **positive-Jacobian** ones: for `Tet4` the nodes are right-handed, for `Hex8` they follow the usual counter-clockwise-per-face convention. `reference_nodes` gives the natural coordinates for the shape functions, and the determinantal maximum of its Jacobian is the denominator of the scaled Jacobian (§0.3).
- **Refusals:** An unknown discriminant → no `FromStr`; there is no runtime "any element" value. Every match is exhaustive so a new kind is a compile error at every use until it is fully supported.
- **Tests:** `tet4_reference_is_right_handed` — asserts `det(J) > 0` at the centroid for the reference `Tet4`. `every_face_points_out_of_its_element` — asserts `face_normal_sign` agrees with the geometric normal computed from the reference nodes. `node_count_matches_the_table` — asserts each kind's `node_count` equals its `reference_nodes().len()`. `faces_cover_every_node_twice_for_a_closed_element` — asserts `Hex8`'s and `Tet4`'s face lists reference every node exactly twice (tet4: each node on 3 faces).
- **Acceptance:** The table exists, every later task consumes it, and `cargo xtask layers` passes with `sim` registered at L3.
- **Risk:** A sign slip in `face_normal_sign` that only shows on one element type. Fallback: the geometric check in the test suite recomputes normals independently and compares.

### SIM-MSH-002 — `FemMesh`: nodes, elements and the three set maps
- **State:** READY
- **Depends on:** SIM-MSH-001
- **Files:** `crates/sim/src/mesh/mod.rs` (new, SIM)
- **Contract:**
  ```rust
  pub struct FemMesh { pub nodes: Vec<Vec3>, pub elements: Vec<Element>, pub element_faces: Vec<[u32; 6]>, pub face_sets: BTreeMap<String, Vec<u32>>, pub edge_sets: BTreeMap<String, Vec<[u32; 2]>>, pub node_sets: BTreeMap<String, Vec<u32>> }
  pub struct Element { pub kind: ElementKind, pub nodes: Vec<u32>, pub source_face: Option<usize> }
  ```
- **Behaviour:** The mesh container. The three set maps carry the boundary-condition and load addressing that `mesh_from_solid` cannot provide, and `Element::source_face` records which `Solid` face an element's boundary face came from so `SIM-MSH-008` can build face sets without `KER-MSH-001` (and so that a future tracked mesh can fill the same field authoritatively). Set names are `String`, sorted, and `"all"`, `"exterior"` and `"interior"` are always present.
- **Refusals:** A node or element index that is out of range is `SimError::InvalidIndex`, never a silent skip. `FemMesh::validate()` returns `Err` for a mesh with no elements, a node not used by any element, or an element referencing a node twice.
- **Tests:** `a_fresh_mesh_has_the_three_standard_sets` — asserts `"all"`, `"exterior"` and `"interterior"` exist. `an_element_referencing_a_missing_node_is_an_error` — asserts the named variant. `a_mesh_with_an_unused_node_is_refused` — asserts `Err`. `set_names_are_sorted_and_stable` — two identical meshes produce identical set iteration order.
- **Acceptance:** The type exists, `validate()` is called at the end of every mesher entry point, and the three sets are populated by `SIM-MSH-008..010`.
- **Risk:** `element_faces: [u32; 6]` is a fixed-size array for a variable maximum; `Tet4` needs 4. Fallback: use `Vec<u32>` with the length documented per kind, or `SmallVec`-style fixed capacity; the contract is written with a fixed array and must be adjusted to `Vec<u32>` at implementation if the compiler objects.

### SIM-MSH-003 — `MeshParams`: every knob, with defaults and provenance
- **State:** READY
- **Depends on:** SIM-MSH-001
- **Files:** `crates/sim/src/mesh/params.rs` (new, SIM)
- **Contract:** `pub struct MeshParams { pub size: f64, pub min_size: f64, pub max_size: f64, pub quality_floor: f64, pub aspect_ceiling: f64, pub tolerance: f64, pub quadratic: bool, pub refine_levels: usize, pub max_elements: usize }` with `pub fn coarse(size: f64) -> MeshParams` and `pub fn validate(&self) -> Result<(), SimError>`.
- **Behaviour:** One place where every meshing decision is stated. `coarse(size)` sets `min_size = size/8`, `max_size = size*4`, `quality_floor = 0.20`, `aspect_ceiling = 10.0`, `max_elements = 2_000_000` (matching `solid::limits::MAX_TRIANGLES`). The struct is serialised verbatim into the study report so a mesh can be reproduced.
- **Refusals:** `size <= 0.0`, `min_size <= 0.0`, `min_size > size`, `size > max_size`, a non-finite field, `quality_floor <= 0.0` or `> 1.0`, `aspect_ceiling <= 1.0` → `SimError::BadTolerance` or `SimError::BadDistance`, each naming the field.
- **Tests:** `defaults_are_finite_and_ordered` — asserts `min_size <= size <= max_size`. `a_zero_size_is_refused` — asserts `Err`. `an_inverted_size_range_is_refused` — asserts `Err` for `min_size > size`. `params_serialise_into_the_report` — a round-trip through the report format.
- **Acceptance:** Exists, validates, and is the only way to configure a mesh.
- **Risk:** A knob added later without a default in `coarse`. Fallback: `validate()` requires every field to be finite and ordered, so a forgotten default surfaces immediately rather than silently being `0.0`.

### SIM-MSH-004 — `SimError`: name every failure
- **State:** READY
- **Depends on:** none
- **Files:** `crates/sim/src/error.rs` (new, SIM)
- **Contract:**
  ```rust
  pub enum SimError {
      Empty, NonFinite, BadTolerance(f64), BadDistance(f64), InvalidIndex{ what: &'static str, index: usize, len: usize },
      InvertedElement{ element: usize, jacobian: f64 },
      MeshQuality{ element: usize, metric: &'static str, value: f64, floor: f64 },
      NotConforming{ detail: String },
      UnconstrainedDof{ dof: usize, kind: &'static str },
      SingularStiffness{ dof: usize, pivot: f64, detail: String },
      SolverFailed(String), Unsupported(String), Unverified(String),
      LimitExceeded{ what: &'static str, got: usize, cap: usize },
      Kernel(cadcraft_solid::SolidError),
  }
  ```
- **Behaviour:** The crate's entire failure vocabulary, mirroring `SolidError`'s discipline ("a failure is always *named*"). `Kernel` wraps `cadcraft_solid::SolidError` so a kernel refusal reaches the user with its reason intact rather than as "mesh failed". `Display` for every variant states what was wrong and what was expected.
- **Refusals:** This *is* the refusal vocabulary. There is no `Other(String)` catch-all: a new failure mode gets a new variant, which is a compile error at every site that must handle it.
- **Tests:** `every_variant_has_a_non_empty_message` — asserts no `Display` output is empty and that each mentions the failing quantity. `kernel_errors_keep_their_reason` — asserts `Display` of `Kernel(OpenEdge(..))` contains the kernel's text. `error_is_std_error` — asserts it implements `std::error::Error`, because it crosses the crate boundary into `engine`.
- **Acceptance:** Exists with the crate-level `deny(clippy::unwrap_used, expect_used, panic, unimplemented, todo, unreachable)` and `#![forbid(unsafe_code)]` on `crates/sim/src/lib.rs`.
- **Risk:** The enum grows past what is useful. Fallback: variants are grouped and `Unsupported(String)` exists precisely so that "we do not do this physics" never grows into forty variants.

### SIM-MSH-005 — Quality metrics: aspect ratio, scaled Jacobian, triangle shape
- **State:** READY
- **Depends on:** SIM-MSH-001
- **Files:** `crates/sim/src/quality.rs` (new, SIM)
- **Contract:** `pub fn triangle_shape(p: [Vec3; 3]) -> Result<f64, SimError>`, `pub fn tet_aspect_ratio(nodes: &[Vec3; 4]) -> Result<f64, SimError>`, `pub fn scaled_jacobian(kind: ElementKind, nodes: &[Vec3], at: [f64; 3]) -> Result<f64, SimError>`.
- **Behaviour:** The definitions in §0.3, computed exactly as written there. `scaled_jacobian` evaluates at an arbitrary reference point so it can be called at each Gauss point. Degenerate input (a zero-area triangle, a flat tetrahedron) returns `Err`, **not** `0.0` — a metric that returns a number for a degenerate element is a metric that will let one through.
- **Refusals:** A non-finite coordinate → `SimError::NonFinite`. A zero-area triangle or a zero-volume tetrahedron → `SimError::MeshQuality` with the metric name and the value. A node count that does not match the kind → `SimError::InvalidIndex`.
- **Tests:** `an_equilateral_triangle_scores_one` — 1e-12. `a_collinear_triangle_is_an_error` — asserts `Err`. `a_right_handed_tet_has_positive_jacobian` — asserts > 0. `a_left_handed_tet_has_negative_jacobian` — asserts < 0 and **that it is `Err` at element construction**, not a negative number returned to the caller. `a_unit_cube_hex8_has_jacobian_one` — asserts `|J*| ≈ 1` on the reference element.
- **Acceptance:** All three metrics implemented, unit-tested against shapes with known closed-form values (equilateral, right tetrahedron, unit cube), and used by `SIM-MSH-006`.
- **Risk:** The scaled Jacobian evaluated only at Gauss points misses an element that folds *between* them. Fallback: `quality_report` also samples the element centroid and every node, and reports the **minimum** over all sample points.

### SIM-MSH-006 — `quality_report` and the refusal below the floor
- **State:** READY
- **Depends on:** SIM-MSH-005
- **Files:** `crates/sim/src/quality.rs` (SIM)
- **Contract:** `pub fn quality_report(mesh: &FemMesh, p: &MeshParams) -> Result<QualityReport, SimError>` with `pub struct QualityReport { pub min_jacobian: f64, pub mean_jacobian: f64, pub min_aspect: f64, pub mean_aspect: f64, pub min_shape: f64, pub worst_element: usize, pub element_count: usize, pub node_count: usize }`.
- **Behaviour:** Compute every metric over the whole mesh and **return `Err(SimError::MeshQuality { .. })` naming the worst element, the metric, the value and the floor** if any element is below `quality_floor` or above `aspect_ceiling`. A mesh is either good enough to solve or it is refused; there is no "solve anyway and warn", because a stress peak at a bad element is indistinguishable from a real one in a viewport.
- **Refusals:** The floor breach itself. Also `SimError::Empty` for a mesh with no elements.
- **Tests:** `a_mesh_below_the_floor_is_refused_with_the_worst_element` — constructs one bad tet and asserts the `element` field. `a_good_mesh_reports_its_worst_element_without_erroring` — asserts `Ok` and that `worst_element` is the minimum. `report_counts_match_the_mesh` — `element_count == mesh.elements.len()`. `the_floor_is_the_one_in_the_params` — asserts the `floor` field equals `p.quality_floor`, tying the report to its own parameters.
- **Acceptance:** Every mesher entry point calls it before returning, and `SIM-STATIC`'s `assemble_static` calls it again (a mesh can be edited after generation).
- **Risk:** A strict floor makes the mesher unusable on real geometry, so someone relaxes it globally. Fallback: `quality_floor` is per-`MeshParams` and is reported; relaxing it is visible in the report next to the result.

### SIM-MSH-007 — Reuse `triangulate_region` as the surface layer, honestly
- **State:** BACKLOG
- **Depends on:** SIM-MSH-002
- **Files:** `crates/sim/src/mesh/surface.rs` (new, SIM)
- **Contract:** `pub fn surface_triangles(face: &cadcraft_solid::Face) -> Result<Vec<[Vec3; 3]>, SimError>` and `pub fn surface_area(face: &cadcraft_solid::Face) -> Result<f64, SimError>`.
- **Behaviour:** Call `cadcraft_geom::Region`/`triangulate_region` through the solid's own re-export (`cadcraft_solid` re-exports `Region`, `Regions`, `Vec2`, `Vec3`, `Mesh`, `mesh_from_solid` but **not** `triangulate_region` — see §7 note) and produce world-space triangles for one face. Where the solid's own path is used, use `cadcraft_solid::mesh_from_solid` and accept that its output is unwelded (§1.2). **This task is a decision point as much as an implementation: if `triangulate_region` must be re-exported from `cadcraft_solid`, that is a `KER` task, not a `SIM` one.**
- **Refusals:** `SimError::Kernel(e)` for the kernel's `Empty`, `EmptyProfile` and `TooManyTriangles` — propagated verbatim, never flattened to "mesh failed".
- **Tests:** `surface_area_matches_the_face_area` — asserts `Σ A_tri` equals `face.area()` to 1e-9, the same identity `mass.rs` pins. `hole_triangles_subtract` — a face with a hole has surface area `outer − hole`. `a_degenerate_face_is_a_kernel_error` — asserts the `Kernel` variant with the underlying message.
- **Acceptance:** Either the reuse works with the current public API, or the precise missing re-export is filed as a `KER` task and this task is `BLOCKED` on it.
- **Risk:** `triangulate_region` is `pub` in `mesh.rs` but `lib.rs:91` exports only `{Mesh, Triangle, mesh_from_solid, stl_ascii, stl_binary}`. Fallback: reconstruct via `mesh_from_solid` on a single-face solid (expensive), or file `KER-MSH-009` = `pub use mesh::triangulate_region`.

### SIM-MSH-008 — Element faces to their source `Solid` face
- **State:** BLOCKED
- **Depends on:** KER-MSH-001, SIM-MSH-013
- **Files:** `crates/sim/src/mesh/surface.rs` (SIM)
- **Contract:** `pub fn attach_face_provenance(mesh: &mut FemMesh, solid: &Solid) -> Result<(), SimError>` filling `Element::source_face` for every element face lying on the solid's boundary.
- **Behaviour:** Populate `source_face` so `SIM-MSH-009` can build face sets. When `KER-MSH-001` is available, read `TrackedMesh::tri_face` directly — exact, O(1). Until then, the fallback is a point-in-face test per boundary element face using `Face::contains_local` with a tolerance band, which is O(faces × boundary faces) and **is recorded in the report as a heuristic provenance** rather than exact.
- **Refusals:** A boundary element face that cannot be attributed to exactly one `Solid` face is `SimError::NotConforming { detail }`. Guessing is refused: an unattributable face means either the mesh is wrong or the kernel's faces are inconsistent, and both are worth an error.
- **Tests:** `every_boundary_face_maps_to_exactly_one_solid_face` — over the kernel's box, prism and chamfered shapes. `an_interior_face_has_no_source` — asserts `source_face == None`. `provenance_is_stable_under_face_ordering` — asserts a permuted `solid.faces` yields the same mapping by geometry, not by index.
- **Acceptance:** Green with exact provenance when `KER-MSH-001` lands; documented as heuristic until then.
- **Risk:** The index-based fallback breaks if a boolean reorders faces. Fallback: compare by geometry (`plane.coplanar` and `contains_local`), never by index; the ordering test above is the proof.

### SIM-MSH-009 — Face sets from a selection
- **State:** BACKLOG
- **Depends on:** SIM-MSH-008, SIM-MSH-010
- **Files:** `crates/sim/src/mesh/sets.rs` (new, SIM)
- **Contract:** `pub enum FaceSelection { All, Exterior, Named(String), ByFaceId(Vec<FaceId>), ByArea { min: f64, max: f64 }, ByNormalWithin { angle_deg: f64 } }` and `pub fn build_face_sets(mesh: &FemMesh, sels: &[(&str, FaceSelection)]) -> Result<BTreeMap<String, Vec<u32>>, SimError>`.
- **Contract detail:** set names are the `&str` keys; **an empty resulting set is `SimError::Empty`, not an empty `Vec`** — a named constraint set matching nothing is a user error that would otherwise be a silent no-op boundary condition, which is the most dangerous bug class in FEA.
- **Behaviour:** Turn a user selection into element-face numbers. `ByNormalWithin` is how "the faces whose normal is within 30° of +Z" becomes "the top face" without a face id. `ByArea` handles "the largest face" and "all faces bigger than 100 cm²".
- **Refusals:** `SimError::Empty` for a set that matches nothing. `SimError::Kernel` for an unresolvable `FaceId`. `angle_deg` outside `[0, 180]` → `SimError::BadTolerance`.
- **Tests:** `the_top_face_of_a_box_is_selected_by_normal` — asserts exactly 1 face for a 30° window. `a_selection_that_matches_nothing_is_an_error` — asserts `Err(Empty)`, explicitly not an empty vec. `sets_are_disjoint_when_they_should_be` — asserts top/bottom selections share no element face. `set_ordering_is_deterministic` — two calls give identical output.
- **Acceptance:** Every downstream load and constraint takes a set name; there is no path that takes a raw element index from the user.
- **Risk:** A set that silently becomes empty after a mesh refinement renumbers elements. Fallback: face sets are stored as *element-face numbers* and re-derived after every refinement by `SIM-MSH-018`, never cached across a remesh.

### SIM-MSH-010 — Edge sets and node sets
- **State:** BACKLOG
- **Depends on:** SIM-MSH-002
- **Files:** `crates/sim/src/mesh/sets.rs` (SIM)
- **Contract:** `pub fn build_edge_sets(mesh: &FemMesh, sels: &[(&str, EdgeSelection)]) -> Result<BTreeMap<String, Vec<[u32; 2]>>, SimError>` and `pub fn build_node_sets(mesh: &FemMesh, sels: &[(&str, NodeSelection)]) -> Result<BTreeMap<String, Vec<u32>>, SimError>` with `pub enum NodeSelection { All, OnPlane { plane: Plane, tol: f64 }, InsideCylinder { axis_point: Vec3, axis_dir: Vec3, radius: f64, tol: f64 }, InBall { center: Vec3, radius: f64 } }`.
- **Behaviour:** Edge sets are node pairs and are what a symmetry condition or a beam/spring restraint is expressed in. Node sets are geometric: "every node within 5 mm of the clamp face", "every node on the symmetry plane", "every node in the grip region". They are how a fixture is applied without a face id, which matters because `KER-MSH-003` will not exist for a while.
- **Refusals:** An empty set → `SimError::Empty`. A plane with a non-finite normal or `radius < 0.0` → `SimError::BadDirection` / `SimError::BadDistance`. `tol <= 0.0` → `SimError::BadTolerance`. **Node sets use a distance-to-plane band, never a plane-membership test**, because exactly-on-plane membership is a measure-zero event that matches nothing.
- **Tests:** `a_plane_node_set_uses_a_band` — asserts nodes within `tol` are included and nodes at `2·tol` are not. `a_cylinder_node_set_is_ordered` — asserts the set is sorted ascending, so iteration is deterministic. `an_empty_node_set_is_an_error` — asserts `Err`. `a_node_set_survives_a_translation` — asserts translating the mesh translates the selection, which is the whole reason it is geometric.
- **Acceptance:** Both maps are populated, deterministic, and used by `SIM-STATIC-002` and `SIM-STATIC-003`.
- **Risk:** A band too wide captures a second face's nodes and quietly doubles a restraint. Fallback: the band is `tol` in the report, and the count is printed next to it.

### SIM-MSH-011 — Vertex welding: the step `mesh_from_solid` does not do
- **State:** READY
- **Depends on:** SIM-MSH-002
- **Files:** `crates/sim/src/mesh/weld.rs` (new, SIM)
- **Contract:** `pub fn weld(verts: &[Vec3], tris: &[[usize; 3]], tol: f64) -> Result<(Vec<Vec3>, Vec<[usize; 3]>, WeldStats), SimError>` with `pub struct WeldStats { pub input_verts: usize, pub output_verts: usize, pub merged: usize, pub max_displacement: f64 }`.
- **Behaviour:** `mesh_from_solid` pushes three fresh vertices per triangle (`mesh.rs:60-62`); an FE mesh needs shared nodes. Weld on the **same tolerance grid discipline the kernel uses** (`solid::tolerance_for(diagonal)`, clamp `[1e-12, 1e-3]`, mirroring `solid.rs::vkey` at `solid.rs:787-791`) so the sim mesh and the kernel agree about what "the same point" means. Record the maximum displacement any point moved: welding *does* move geometry by up to `tol`, and that number belongs in the report.
- **Refusals:** `tol <= 0.0` or non-finite → `SimError::BadTolerance`. A weld that would merge points further apart than `tol` does not merge them — if more than 5 % of candidates are rejected at distance, that is a signal the tolerance is wrong, and `WeldStats::max_displacement` is the number that shows it.
- **Tests:** `welding_reduces_a_triangle_soup_to_a_vertex_set` — a single quad's 8 soup vertices weld to 4. `welding_moves_nothing_beyond_the_tolerance` — asserts `max_displacement <= tol`. `welding_is_idempotent` — welding twice equals welding once. `welding_is_stable_far_from_the_origin` — a soup translated to 1e6 welds identically, the `mass.rs` lesson applied again.
- **Acceptance:** Green, and `SIM-MSH-012` consumes it. This is the step that makes everything after it possible.
- **Risk:** Welding with a tolerance that is too large merges two genuinely distinct vertices of a thin feature and changes the shape. Fallback: `max_displacement` is reported; the mesh volume check in `SIM-MSH-025` catches a shape change.

### SIM-MSH-012 — Manifold verification of the welded surface
- **State:** BACKLOG
- **Depends on:** SIM-MSH-011
- **Files:** `crates/sim/src/mesh/weld.rs` (SIM)
- **Contract:** `pub fn surface_manifold(verts: &[Vec3], tris: &[[usize; 3]]) -> Result<ManifoldStats, SimError>` with `pub struct ManifoldStats { pub boundary_edges: Vec<[u32; 2]>, pub nonmanifold_edges: Vec<[u32; 2]>, pub nonmanifold_vertices: Vec<u32> }`.
- **Behaviour:** After welding, every interior edge must be used by exactly two triangles and every boundary edge exactly one; a directed-edge count additionally proves consistent orientation (this is `Solid`'s invariant I2/I3, restated for the triangulated surface). The tri-soup from `mesh_from_solid` fails this until welded — running it before welding is how a mesher discovers it needs `SIM-MSH-011`.
- **Refusals:** `SimError::NotConforming { detail }` naming the offending edge's node coordinates and use count, for a boundary edge (the surface is open), a non-manifold edge (used three or more times) or a non-manifold vertex.
- **Tests:** `a_cube_surface_is_closed_and_oriented` — asserts `boundary_edges` is empty and every edge is used once forward and once back. `a_soup_is_not_manifold_before_welding` — asserts the check fails on unwelded output, proving the test has teeth. `an_open_surface_reports_its_hole` — a cube with one face removed reports exactly 4 boundary edges. `hostile_triangles_do_not_panic` — asserts `Err` for out-of-range indices and non-finite vertices.
- **Acceptance:** Green; `SIM-MSH-015` and `SIM-MSH-013` both depend on it.
- **Risk:** A boolean result with a crack (which `KER-BOOL-002` is about) produces an open surface and is refused here — correct, but it means every CAM/mesh feature is blocked until `KER-BOOL-001..003` are `VALIDATED`. That is the plan working as intended.

### SIM-MSH-013 — Tetrahedral filling of the welded surface (the mesher core)
- **State:** BACKLOG
- **Depends on:** SIM-MSH-001, SIM-MSH-012, KER-BOOL-001, KER-BOOL-002, KER-BOOL-003
- **Files:** `crates/sim/src/mesh/tet.rs` (new, SIM)
- **Contract:** `pub fn fill_tetrahedra(surface: &WeldedSurface, interior: &InteriorPoints) -> Result<Vec<Element>, SimError>` and `pub enum FillAlgorithm { Delaunay, AdvancingFront }` with `pub fn fill(alg: FillAlgorithm, surface: &WeldedSurface, interior: &InteriorPoints) -> Result<Vec<Element>, SimError>`.
- **Behaviour:** Take a closed, manifold, oriented triangulated surface and fill it with positive-Jacobian `Tet4` elements sharing those nodes. Two candidate algorithms, both pure Rust, both implementable without `unsafe`: **advancing front** (grow a shell inward from the boundary, always splitting the front's smallest-angle tet) and **constrained Delaunay** (incrementally insert interior points, flipping illegal edges). The task includes the **choice**, recorded as a comment naming the test that decided it.
- **Refusals:** `SimError::NotConforming` for an open or non-manifold surface — **never a filled mesh with a hole**. `SimError::InvertedElement` for any tet with `det(J) ≤ 0` at construction. `SimError::LimitExceeded` past `MeshParams::max_elements`. A surface with no interior point reachable within the frontier budget → `SimError::Unsupported("the region could not be filled in {n} layers; try a coarser size")`.
- **Tests:** `a_cube_fills_with_tets_of_positive_volume` — asserts total tet volume equals the cube volume to 1e-9, the same identity `the_mesh_volume_matches_the_exact_volume_of_the_solid` pins for the kernel. `every_interior_edge_is_shared_by_exactly_five_tets` — the tet-mesh analogue of `SIM-MSH-012`'s manifold check. `no_tet_is_inverted` — over the kernel's random shapes. `an_open_surface_is_refused` — asserts `Err(NotConforming)`. `hostile_surfaces_do_not_panic` — a self-intersecting soup, a zero-area surface and a surface with 10⁵ degenerate triangles.
- **Acceptance:** A cube, a prism with a hole and a chamfered box all fill completely, with total volume matching the solid to 1e-9, and every interior edge used by exactly five tets.
- **Risk:** The constrained Delaunay route needs a robust 3D orientation predicate (`orient3d`), and an exact one is subtle. Fallback: advancing front is more forgiving of a slightly inexact predicate because it is constructive rather than retriangulating; choose AF if the predicate proves troublesome, and record why.

### SIM-MSH-014 — `check_conforming`
- **State:** BACKLOG
- **Depends on:** SIM-MSH-013
- **Files:** `crates/sim/src/mesh/tet.rs` (SIM)
- **Contract:** `pub fn check_conforming(mesh: &FemMesh) -> Result<ConformingReport, SimError>` with `pub struct ConformingReport { pub interior_edges: usize, pub boundary_edges: usize, pub hanging_nodes: Vec<u32> }`.
- **Behaviour:** The invariant check for the filled volume: every interior edge carries exactly two (triangles) or five (tets) elements, every boundary edge exactly one triangle, and **no node belongs to fewer elements than its valence requires** (the hanging-node check that catches a non-conforming transition after refinement). Run after every operation that touches connectivity, including `SIM-MSH-018` and `SIM-MSH-022`.
- **Refusals:** `SimError::NotConforming { detail }` naming the first offending edge or node, deterministically (iterate in sorted node order so the report is reproducible — the `Solid::validate` `BTreeMap` lesson, `solid.rs:583-585`).
- **Tests:** `a_filled_cube_is_conforming` — asserts no hanging nodes. `a_refined_element_set_with_a_bad_transition_is_caught` — asserts a deliberately mismatched transition is found. `the_first_offender_is_the_same_on_every_run` — runs twice and compares the reported node. `an_empty_mesh_is_refused` — asserts `Err`.
- **Acceptance:** Called at the end of every mesher and refinement entry point.
- **Risk:** The check is O(E) with a `BTreeMap` and is slow on a 10⁶-element mesh. Fallback: the map is built over sorted node pairs, which is O(E log E) and acceptable at the `max_elements` cap; beyond the cap the mesh is refused anyway.

### SIM-MSH-015 — Duplicate and collapsed element detection
- **State:** BACKLOG
- **Depends on:** SIM-MSH-013
- **Files:** `crates/sim/src/mesh/tet.rs` (SIM)
- **Contract:** `pub fn find_degenerate_elements(mesh: &FemMesh, tol: f64) -> Result<Vec<Degenerate>, SimError>` with `pub struct Degenerate { pub element: usize, pub kind: DegenerateKind, pub measure: f64 }` and `pub enum DegenerateKind { ZeroVolume, RepeatedNode, OutsideSurface }`.
- **Behaviour:** A tet with a repeated node has a singular stiffness matrix; a tet of near-zero volume has an enormous one. Both are silent failures — the solve either produces `NaN` or produces a plausible wrong answer. Detect them by geometry before the assembler, and report **which** element and **which** kind.
- **Refusals:** The findings themselves, as `SimError::MeshQuality` with the element index. A mesh with any degenerate element is not solved.
- **Tests:** `a_tet_with_a_repeated_node_is_found` — asserts `RepeatedNode`. `a_flat_tet_is_found` — asserts `ZeroVolume`. `a_tet_outside_the_surface_is_found` — a tet whose centroid fails `Solid::contains` via `KER-MSH-004`. `a_clean_mesh_finds_nothing` — asserts an empty list.
- **Acceptance:** Green, and `SIM-STATIC-006` calls it before assembling.
- **Risk:** A duplicate element (the same node set twice) is not caught by volume checks and produces a matrix that is still solvable but wrong. Fallback: the `RepeatedNode` and duplicate-connectivity checks both key on a sorted node-tuple hash.

### SIM-MSH-016 — Mesh generation entry point
- **State:** BACKLOG
- **Depends on:** SIM-MSH-013, SIM-MSH-014, SIM-MSH-015, SIM-MSH-006
- **Files:** `crates/sim/src/mesh/mod.rs` (SIM)
- **Contract:** `pub fn mesh_solid(solid: &Solid, p: &MeshParams) -> Result<FemMesh, SimError>`.
- **Behaviour:** The one public way to make a mesh. Inside `solid::to_si` (never inside the kernel), then surface → weld → manifold check → interior points → fill → conformity → quality. Every stage's error propagates unchanged. **This function refuses; it never repairs.** An automatic repair pass (removing bad elements, retriangulating) is `SIM-MSH-030` and is opt-in.
- **Refusals:** `SimError::Kernel` for any kernel refusal. `SimError::MeshQuality` if the final quality report fails. `SimError::LimitExceeded { what: "elements", .. }` past the cap. A solid whose `validate()` fails → `SimError::Kernel`.
- **Tests:** `a_box_meshes` — asserts a 10-cube with `size = 2.0` produces a mesh whose summed tet volume is 1000 ± 1e-9. `a_mesh_refuses_a_broken_solid` — asserts `Kernel` propagation. `mesh_solid_is_deterministic` — two calls give identical meshes. `the_report_fields_come_from_the_params` — asserts `max_elements` is what is enforced.
- **Acceptance:** Green on every shape in `crates/solid/src/tests_support`, with total tet volume equal to `solid::mass_properties(solid).volume` to 1e-9.
- **Risk:** It is the function everyone uses, so its refusals are the project's most-visible behaviour. Fallback: every refusal names what to change (`"quality floor 0.20 not met at element 41; try MeshParams { size: 1.5 }"`).

### SIM-MSH-017 — Local refinement by longest-edge bisection with transition templates
- **State:** BACKLOG
- **Depends on:** SIM-MSH-014, SIM-MSH-017a
- **Files:** `crates/sim/src/mesh/refine.rs` (new, SIM)
- **Contract:** `pub fn refine_region(mesh: &FemMesh, predicate: &RefinePredicate, level: usize) -> Result<FemMesh, SimError>` with `pub trait RefinePredicate { fn wants(&self, element: usize, centroid: Vec3) -> bool; }` and `pub enum RefinePredicate { StressGradient { field: Vec<f64>, threshold: f64 }, DistanceTo { point: Vec3, radius: f64 }, ElementSize { p: &MeshParams }, FaceSet(String) }`.
- **Behaviour:** Split tets into 8 by bisecting every edge and inserting the centroid, with the standard 8-to-4 and 4-to-8 transition templates so the result stays conforming. The reason to care: refinement driven by the **stress gradient** of a previous solve is adaptive analysis, and it needs the error indicator from `SIM-STATIC-018`.
- **Refusals:** Refining past `max_elements` → `SimError::LimitExceeded`. A transition that cannot be applied (an element adjacent to two differently-refined neighbours in an unsupported configuration) → `SimError::NotConforming { detail }` naming the transition. A `StressGradient` predicate whose field length does not match the element count → `SimError::InvalidIndex`.
- **Tests:** `refining_doubles_the_element_count_per_level` — asserts 8× per level. `refinement_stays_conforming` — asserts `check_conforming` passes after. `refinement_moves_no_node` — asserts the node positions are unchanged (only added), which is the property that makes the stress error indicator meaningful. `a_partially_refined_mesh_has_no_hanging_nodes` — the transition template test.
- **Acceptance:** Green, and the mesh volume is unchanged by refinement to 1e-12.
- **Risk:** Transition templates are fiddly and a missing one produces a hanging node that only shows on a particular adjacency. Fallback: `check_conforming` is run after every refinement and catches exactly this; it is not optional.

### SIM-MSH-018 — The size field
- **State:** BACKLOG
- **Depends on:** SIM-MSH-003
- **Files:** `crates/sim/src/mesh/sizing.rs` (new, SIM)
- **Contract:** `pub trait SizeField { fn size_at(&self, p: Vec3) -> Result<f64, SimError>; }` and `pub struct GeometricSize { pub base: f64, pub min: f64, pub max: f64, pub features: Vec<SizeFeature> }` with `pub enum SizeFeature { NearFace { face: FaceId, distance: f64, factor: f64 }, NearEdge { edge: EdgeRef, distance: f64, factor: f64 }, NearVertex { at: Vec3, radius: f64, factor: f64 } }`.
- **Behaviour:** Element size as a function of position, so a hole gets a fine mesh and a boss a coarse one. The classic field is `s(p) = clamp(base / (1 + k·d(p, feature)), min, max)`. **Feature geometry comes from `KER-MSH-003`**; until it lands, `SizeFeature::NearFace` is unavailable and `NearEdge` uses `EdgeRef` geometrically.
- **Refusals:** `min <= 0.0`, `max < min` → `SimError::BadTolerance`. `factor <= 0.0` → `SimError::BadDistance`. A `SizeField` returning `NaN` → `SimError::NonFinite`, checked at every sample point before it is used.
- **Tests:** `size_decreases_near_a_feature` — monotonic in distance. `size_is_clamped_at_both_ends` — asserts `min ≤ s(p) ≤ max` at 100 random points. `a_field_that_goes_below_the_quality_floor_is_refused` — asserts `Err`. `a_uniform_field_recovers_the_base_size` — `GeometricSize { features: vec![] }` returns `base`.
- **Acceptance:** Green; `SIM-MSH-019` and `SIM-MSH-020` consume it.
- **Risk:** A feature-based size field drives element count into the millions on a real part. Fallback: `max_elements` refuses with a message that says which feature is responsible.

### SIM-MSH-019 — Curvature-driven sizing on faceted geometry, and what it can honestly claim
- **State:** BLOCKED
- **Depends on:** KER-SRF-003, KER-MSH-002, SIM-MSH-018
- **Files:** `crates/sim/src/mesh/sizing.rs` (SIM)
- **Contract:** `pub fn dihedral_angles(solid: &Solid) -> Result<Vec<Dihedral>, SimError>` with `pub struct Dihedral { pub edge: EdgeRef, pub angle_rad: f64, pub arc_length: f64 }` and `pub fn size_from_curvature(d: &Dihedral, target_segments: usize) -> Result<f64, SimError>`.
- **Behaviour:** Compute the interior dihedral angle at each edge (from the two incident faces' normals, which `Edge::faces` gives directly) and derive a target element size so every arc is approximated by `target_segments` facets. **Honesty obligation, written into the doc comment:** every face in this kernel is planar and a fillet is at most `MAX_ROUND_SEGMENTS = 64` flat facets (`crates/solid/src/lib.rs:111-112`). This function therefore sizes the mesh for a **faceted approximation of the design intent**, and the deviation between the two is a *modelling* error that no amount of meshing will fix. It must be computed, stored and reported (`SIM-VIZ-011`).
- **Refusals:** `target_segments < 3` → `SimError::BadTolerance`. An edge whose two faces are coplanar within tolerance → `angle_rad` of `0.0`, **not** an error, and it is excluded from the refinement decision by the caller.
- **Tests:** `a_right_angle_gives_pi_over_two` — a box's 12 edges. `a_coplanar_pair_gives_zero` — a fillet's two adjacent facets within `MAX_ROUND_SEGMENTS` degrees. `a_refined_arc_has_more_segments_per_arc` — asserts `arc_length / size` is roughly constant. `the_modelling_deviation_is_reported` — asserts the computed chordal sagitta is present in the report structure.
- **Acceptance:** Green, **and** the function's doc comment states that curved-surface accuracy is bounded by the kernel's facet count until `KER-SRF-003` lands. BLOCKED because an edge's dihedral angle needs `Edge::faces` to be reachable by stable identity.
- **Risk:** Users read a smooth stress contour near a fillet and believe the model is smooth. Fallback: the report carries the facet count and the max sagitta next to every stress plot; this is also `SIM-VIZ-013`.

### SIM-MSH-020 — The mesh-convergence driver
- **State:** BACKLOG
- **Depends on:** SIM-MSH-016, SIM-MSH-018, SIM-VAL-016
- **Files:** `crates/sim/src/mesh/converge.rs` (new, SIM)
- **Contract:** `pub fn converge(p: &MeshParams, qoi: &dyn QuantityOfInterest, levels: usize) -> Result<ConvergenceRun, SimError>` with `pub trait QuantityOfInterest { fn evaluate(&self, mesh: &FemMesh) -> Result<f64, SimError>; }` and `pub struct ConvergenceRun { pub levels: Vec<ConvergenceLevel>, pub converged_at: Option<usize>, pub mesh: FemMesh }` with `pub struct ConvergenceLevel { pub elements: usize, pub nodes: usize, pub qoi: f64, pub delta: f64 }`.
- **Behaviour:** Mesh at level 0, solve, refine, re-solve, compare the quantity of interest until its change is below `p`'s tolerance or `levels` is exhausted. `converged_at` is `Some(n)` or `None` — **never a best guess**. The final mesh is the one at `converged_at`, and a run that never converges returns the finest mesh with `converged_at: None` and a report saying so.
- **Refusals:** `levels == 0` → `SimError::BadTolerance`. `SimError::LimitExceeded` when a level's element count passes `max_elements` — the run stops and reports the levels it completed.
- **Tests:** `convergence_reaches_a_stable_qoi` — asserts the `delta` sequence decreases. `a_run_that_cannot_converge_reports_none` — asserts `converged_at.is_none()` and that the error text says so. `each_level_has_more_elements_than_the_last` — monotonicity. `the_final_mesh_is_the_converged_one` — asserts `mesh` equals `levels[converged_at].mesh`.
- **Acceptance:** Green, and it is the only sanctioned way to pick a mesh for a study (a study that skips it records `Validation::Unverified`).
- **Risk:** Convergence of one quantity says nothing about another. Fallback: the report always shows the per-level table, so a user can see which quantity was converged.

### SIM-MSH-021 — Interior point generation for the tet filler
- **State:** BACKLOG
- **Depends on:** SIM-MSH-018, KER-MSH-004
- **Files:** `crates/sim/src/mesh/interior.rs` (new, SIM)
- **Contract:** `pub fn interior_points(solid: &Solid, field: &dyn SizeField, tol: f64) -> Result<Vec<Vec3>, SimError>`.
- **Behaviour:** Place candidate interior nodes on a background grid whose spacing follows the size field, rejecting points outside the solid. Membership uses `KER-MSH-004`'s banded containment; until it lands, `Solid::contains` (a ray cast per point) makes this O(points × faces) and **must be recorded as the cost in the doc comment**, with the planned replacement being a spatial index under `KER-BOOL-005`.
- **Refusals:** A solid with a zero bounding box → `SimError::Empty`. `tol <= 0.0` → `SimError::BadTolerance`. A count past `max_elements` → `SimError::LimitExceeded`. **A point exactly on the boundary is not an interior point and is excluded**, using the band from `KER-MSH-004`; with `contains` alone it is excluded by a conservative inward offset of `tol`.
- **Tests:** `interior_points_are_all_inside` — asserts every point satisfies `solid.contains` and is at least `tol` from the boundary. `a_thin_wall_yields_points_in_the_wall` — a 0.1 mm wall still gets points, by dropping the size field's minimum to below the wall thickness. `an_empty_bounding_box_is_refused` — asserts `Err`. `the_count_respects_the_size_field` — asserts a finer field gives more points.
- **Acceptance:** Green on the kernel's test shapes; the O(n·f) cost is documented.
- **Risk:** On a 10⁴-face solid this is the slowest step in meshing. Fallback: a uniform grid bucketing by face bounds, built inside `crates/sim` without kernel help, is an acceptable optimisation if it produces identical output.

### SIM-MSH-022 — Hexahedral meshing (plumbing only, honestly scoped)
- **State:** BACKLOG
- **Depends on:** SIM-MSH-016, SIM-VAL-014
- **Files:** `crates/sim/src/mesh/hex.rs` (new, SIM)
- **Contract:** `pub fn mesh_hex_from_faces(faces: &[Face; 6], through: FaceId, p: &MeshParams) -> Result<FemMesh, SimError>` — **six planar faces forming a topological box, one pair designated as the sweep direction.**
- **Behaviour:** Only blocks that are topological boxes: a box, a straight extrusion, a loft between two similar cross-sections. `Hex8` and, if `p.quadratic`, `Hex27`. Everything else is `SimError::Unsupported` with the reason. **The justification is `SIM-VAL-014`: if linear tets lock, hexes are on the critical path and this task is not optional.** It is listed as plumbing, not as a general hex mesher, and the doc comment says so.
- **Refusals:** `SimError::Unsupported` for any block that is not a topological box — a curved sweep, a block with a through-hole, a block with more than six faces. A face pair that is not within `tol` of parallel → `SimError::NotConforming`.
- **Tests:** `a_box_meshes_into_hex8` — asserts `n` elements for an `n×n×n` division and that the summed volume matches. `a_hex8_on_a_unit_cube_has_unit_jacobian` — the exactness test. `a_non_box_is_refused` — asserts `Err(Unsupported)` for a block with a through-hole. `quadratic_produces_hex27` — asserts node count `8 + 12·n` per element.
- **Acceptance:** Green on boxes and straight extrusions; refuses everything else by name.
- **Risk:** It is a rabbit hole. Fallback: the refusal is the deliverable. If `SIM-VAL-014` shows tets are fine, this task is dropped rather than completed.

### SIM-MSH-023 — Quadratic upgrade (Tet10) and what it must not change
- **State:** BACKLOG
- **Depends on:** SIM-MSH-016, SIM-MAL-001
- **Files:** `crates/sim/src/mesh/quadratic.rs` (new, SIM)
- **Contract:** `pub fn upgrade_to_quadratic(mesh: &FemMesh) -> Result<FemMesh, SimError>` and `pub fn linear_submesh(mesh: &FemMesh) -> Result<FemMesh, SimError>`.
- **Behaviour:** Insert mid-edge nodes (and mid-face for `Hex27`), respecting the ordering table from `SIM-MSH-001`. The test that matters: the **linear sub-mesh of a `Tet10` mesh must give the same answer as the `Tet4` mesh it came from**, within the declared tolerance. A quadratic upgrade that changes the linear result has a node-ordering bug. `linear_submesh` exists so that check is possible, and it is also what a user selects to get back the cheap mesh.
- **Refusals:** Upgrading a mesh with a non-manifold edge → `SimError::NotConforming`. Upgrading an already-quadratic mesh → `SimError::Unsupported("the mesh is already quadratic")`. A mid-edge node that coincides with an existing node within `tol` → `SimError::MeshQuality`, because a zero-length edge makes a singular element.
- **Tests:** `a_tet4_becomes_a_tet10_with_the_right_node_count` — `4 → 10`. `mid_edge_nodes_land_on_the_midpoint` — asserts position to 1e-12. `the_linear_submesh_solves_the_same_problem` — the equivalence test with a documented tolerance. `a_mesh_with_a_duplicate_midedge_node_is_refused` — asserts `Err`.
- **Acceptance:** Green, and `SIM-VAL-016`'s order test distinguishes `Tet4` and `Tet10`.
- **Risk:** Mid-edge nodes on a boundary edge that the kernel considers one long edge but the mesh considers several. Fallback: the upgrade keys on welded node indices, not on kernel edges, so it cannot disagree.

### SIM-MSH-024 — Record the meshing verdict
- **State:** BACKLOG
- **Depends on:** SIM-MSH-022, SIM-VAL-014
- **Files:** `crates/sim/src/mesh/mod.rs` (SIM), `docs/geometry-parity.md` (PLT owner — **coordinate before editing**)
- **Contract:** `pub fn mesher_verdict() -> &'static str` returning one of `"linear tets validated"`, `"linear tets lock; hex required"` or `"unvalidated"`, sourced from the benchmark reports at compile time through a generated constant.
- **Behaviour:** A single honest sentence about what the mesher can and cannot do, surfaced in the UI and in every report. **The point of the task is that the sentence is derived from `SIM-VAL-014`'s measured result, not written by hand.** A mesher that quietly produces wrong results near a fillet and has no verdict attached is the failure this task exists to prevent.
- **Refusals:** `Err` from `mesher_verdict()` if no benchmark has been run — i.e. the default is "unvalidated", and the report says **"this study has not been verified against any analytical benchmark"** until one has.
- **Tests:** `the_default_verdict_is_unvalidated` — asserts it. `a_report_that_carries_the_verdict_also_carries_the_mesh_stats` — the report is not complete without both.
- **Acceptance:** Every study report shows the verdict, the mesh stats and the provenance together.
- **Risk:** It reads as bureaucratic. Fallback: it is one line in the report and one label in the viewport. The cost of omitting it is a user trusting a number.

### SIM-MSH-025 — Mesh volume against the exact solid volume
- **State:** BACKLOG
- **Depends on:** SIM-MSH-016
- **Files:** `crates/sim/src/mesh/mod.rs` (SIM), `crates/sim/tests/mesh.rs` (SIM)
- **Contract:** `pub fn check_mesh_volume(mesh: &FemMesh, solid: &Solid) -> Result<VolumeCheck, SimError>` with `pub struct VolumeCheck { pub tet_volume: f64, pub face_pyramid_volume: f64, pub solid_volume: f64, pub relative_error: f64 }`.
- **Behaviour:** The §1.3 pattern applied to the mesh: three independent volume computations must agree. `tet_volume` sums the element determinants, `face_pyramid_volume` uses `Solid::volume_from_faces` on the boundary triangles, `solid_volume` is the kernel's exact `mass_properties`. This is the mesher's most valuable single test, because it catches a welding tolerance that moved geometry, a fill that missed a corner and a boolean that left a crack — all three, at once, with no reference solution needed.
- **Refusals:** `relative_error > 1e-6` → `SimError::NotConforming { detail: "mesh volume {a} vs solid {b}" }`. The tolerance is relative to the solid's diagonal, matching the kernel's own scaling discipline.
- **Tests:** `the_three_volumes_agree_on_a_box` — 1e-9. `they_agree_on_a_prism_with_a_hole` — 1e-9. `a_weld_tolerance_that_too_coarse_is_caught` — a deliberately huge `tol` moves vertices and the check fires. `the_check_is_relative_to_the_diagonal` — the same shape at 1e-3 and 1e6 scale both pass.
- **Acceptance:** Green and called by `mesh_solid` before returning.
- **Risk:** A mesh that is *visually* correct but 1e-5 off in volume trips a study that would otherwise have been fine. Fallback: the error message reports the relative error and suggests `min_size`, and the threshold is one named constant.

### SIM-MSH-026 — Mesher property tests over random kernel solids
- **State:** BACKLOG
- **Depends on:** SIM-MSH-016, SIM-MSH-012, SIM-MSH-025
- **Files:** `crates/sim/tests/property.rs` (SIM)
- **Contract:** `pub fn meshes_from_random_solids(seed: u64, count: usize) -> Result<Vec<(Solid, FemMesh)>, SimError>`, mirroring the fixed-seed xorshift generator of `crates/geom/src/property_tests.rs` / `crates/solid/tests/property.rs` so a failure reproduces with three lines.
- **Behaviour:** For every random solid: the mesh is closed (`SIM-MSH-012`), conforming (`SIM-MSH-014`), free of degenerate elements (`SIM-MSH-015`), above the quality floor (`SIM-MSH-006`) and volume-matched (`SIM-MSH-025`). Deterministic, no `rand`, no wall clock, every failure message carrying the seed and the case index — the exact convention `crates/solid/tests/property_surface.rs:31-35` documents.
- **Refusals:** `SimError::Kernel` is an **accepted outcome**, not a failure, when the kernel refuses the random solid (four of `property.rs`'s tests fail on purpose today, naming kernel defects). The harness records and skips those, and **names them** in the output.
- **Tests:** `every_random_solid_produces_a_valid_mesh` — the aggregate over the generated set. `the_same_seed_reproduces_the_same_meshes` — asserts byte-identical mesh tables. `hostile_solids_never_panic` — a generated case per limit: 1e-300, 1e300, `NaN` vertices, a solid with 59 999 faces. `kernel_refusals_are_reported_not_swallowed` — asserts the harness records the refusal reason.
- **Acceptance:** Green on the kernel's own test shapes plus the random set, with every kernel refusal enumerated by name in the output.
- **Risk:** Green on random input and failing on a real part. Fallback: `crates/solid/tests/property.rs` is the model, and the shared-corpus work under `QAT-002` is what closes the gap.

---

## 7. `SIM-SLV` — Sparse solver and the solver decision (14 tasks)

### SIM-SLV-001 — **SPIKE: choose the sparse linear solver, and record the decision**
- **State:** READY
- **Depends on:** SIM-VAL-001
- **Files:** `crates/sim/src/linalg/decision.md` (new, SIM), `docs/adr/0002-sparse-solver.md` (PLT owner — **coordinate before editing**)
- **Contract:** The spike produces **no production code**. It produces `crates/sim/src/linalg/decision.md` containing, verbatim, the sections below, and an ADR at `docs/adr/0002-sparse-solver.md` with the same content in the house ADR format. It **does not add a dependency to any `Cargo.toml`** — that is the implementation task's job, and only after the ADR is accepted.
- **Behaviour:** The candidate set, the hard constraints, the accuracy bar and the acceptance criterion for each outcome:

  **Hard constraints (from `AGENTS.md` and `master-plan.md` §0 — non-negotiable):**
  | Constraint | Source | Effect |
  |---|---|---|
  | LGPL/GPL/AGPL forbidden | `AGENTS.md` | Rules out PETSc, SciPy, Trilinos, most FEM codes outright. |
  | `unsafe_code = "forbid"` in every crate, and "no FFI" | `master-plan.md` §0, `Cargo.toml` `[workspace.lints.rust]` | Rules out SuiteSparse, `eigen-sys`, `intel-mkl-src`, any `-sys` crate. A crate that *internally* uses `unsafe` for SIMD is **not** automatically excluded by the lint (it applies to our crate), but the coordinator must ratify it explicitly; that ratification is part of this task's output. |
  | No panics in non-test code | `AGENTS.md` | A candidate that can panic on a singular matrix is out unless it has a documented error path we can reach. |
  | wasm32 must keep building | `AGENTS.md`, `xtask wasm` | A candidate that does not build for wasm32 is out, or it is confined to native-only builds with that stated. |
  | No new assets without an `ATTRIBUTION.md` row | `AGENTS.md` | Rules out any candidate that ships data files. |

  **Candidates:**
  | # | Candidate | Licence | Pure Rust | Status | Verdict to record |
  |---|---|---|---|---|---|
  | A | **From scratch** — AMD ordering + symbolic/numeric sparse LDLᵀ, dense Cholesky below `n = 1000` | ours (MIT OR Apache-2.0) | yes | in | the baseline every other candidate must beat |
  | B | **`sprs`** (sparse matrix crate, `CsMat` + a plain LU) | MIT | yes | in | see acceptance criterion |
  | C | **`faer`** (dense and sparse Cholesky/LU/LDLᵀ/QR, multithreaded) | MIT OR Apache-2.0 | yes (uses `unsafe` internally for SIMD) | in | see acceptance criterion |
  | D | **`nalgebra` + `nalgebra-sparse`** | MIT OR Apache-2.0 | yes | in | see acceptance criterion |
  | E | **SuiteSparse (CHOLMOD)** | LGPL | no (C) | **out** | violates two constraints; do not re-litigate |
  | F | **Intel MKL / oneMKL** | proprietary LGPL | no | **out** | |
  | G | **`eigen` via `eigen-sys`** | MPL-2.0 | no (C++ FFI) | **out** | FFI |
  | H | **OSQP / HiGHS** | MIT | yes | **out** | wrong algorithm class (ADMM / LP); not a general K solver |

  **The accuracy bar any candidate must clear (`SIM-VAL-001`):**
  1. `‖Au − b‖ / ‖b‖ ≤ 1e-10` in `f64` on a symmetric positive definite system of 10⁵ unknowns, measured on the cantilever benchmark from `SIM-VAL-002`.
  2. Reproduces the tip deflection of `SIM-VAL-002` to the bar, and the Lamé stresses of `SIM-VAL-014` to the bar.
  3. On a symmetric **indefinite** matrix, returns a named error rather than a wrong answer (needed by `SIM-SLV-011` for contact).
  4. Iterative refinement converges in ≤ 2 passes.
  5. A singular matrix is reported with the offending dof index, not silently regularised.

  **Acceptance criterion per outcome:**
  - **Outcome A (build it ourselves):** all five accuracy conditions pass on a 10⁵-dof beam mesh, measured in a `crates/sim/benches/` benchmark recorded in the ADR; fill-in from AMD measured and published; and the whole path is under 1 500 lines with no `unsafe`. The ADR states the predicted maximum model size this serves, honestly.
  - **Outcome C (`faer`):** all five accuracy conditions pass **and** the licence and `unsafe`-internals question is ratified in writing by the coordinator **and** it builds for wasm32. If the `unsafe` ratification is refused, this outcome is dead regardless of its numbers.
  - **Outcome B or D (`sprs` / `nalgebra`):** only if they supply a **factorisation**, not just storage. If the only thing available is storage plus a hand-written loop, that is Outcome A with extra steps, and the ADR says so.
  - **Outcome "none works":** the ADR records which condition each candidate failed, and the task ends `BLOCKED` with the smallest set of work that would change the answer. **A spike that ends with "we could not decide" is a legitimate outcome; a spike that ends with an untested decision is not.**

  **Also in scope for the spike:** a measured comparison of a direct solver against a preconditioned-CG path on the same `K`, to size whether an iterative accelerator is needed at all (`SIM-SLV-013`).
- **Refusals:** No outcome may be recorded without the measured numbers attached. **An ADR that says "use `faer`" with no benchmark is rejected by the coordinator**, and the task stays open.
- **Tests:** `decision_md_has_every_section` — asserts the file contains `Constraints`, `Candidates`, `Accuracy bar`, `Outcome`, `Measured`, so the spike cannot be finished with a partial document. `no_dependency_was_added` — asserts `crates/sim/Cargo.toml` and the workspace `Cargo.toml` are byte-identical before and after the spike.
- **Acceptance:** `crates/sim/src/linalg/decision.md` and the ADR exist, the coordinator has accepted one outcome in writing, and no `Cargo.toml` changed. **This is the single most consequential decision in the `SIM` plan and it is a decision, not an implementation.**
- **Risk:** Time pressure turns the spike into "just use `faer`". Fallback: the `no_dependency_was_added` test and the explicit "an ADR with no measured numbers is rejected" rule exist to stop exactly that.

### SIM-SLV-002 — Sparse storage: CSC and the assembly accumulator
- **State:** READY
- **Depends on:** SIM-SLV-001
- **Files:** `crates/sim/src/linalg/sparse.rs` (new, SIM)
- **Contract:** `pub struct CscMatrix { pub n: usize, pub col_ptr: Vec<u32>, pub row_idx: Vec<u32>, pub vals: Vec<f64> }` with `pub fn from_rows(rows: &[usize], cols: &[usize], vals: &[f64], n: usize) -> Result<CscMatrix, SimError>` (counting sort, duplicates summed), `pub fn nnz(&self) -> usize`, `pub fn mul_vec(&self, x: &[f64], y: &mut [f64]) -> Result<(), SimError>`.
- **Behaviour:** Compressed sparse column, `u32` indices, `f64` values, three flat `Vec`s. **No `unsafe`, no sparse matrix crate until `SIM-SLV-001` concludes.** The conversion from element-local triplets to CSC is a counting sort: count per column, prefix sum, scatter. Duplicate triplets (two elements sharing a dof) **sum**, which is the whole point.
- **Refusals:** `rows.len() != cols.len()` or `!= vals.len()` → `SimError::InvalidIndex`. An index `>= n` → `SimError::InvalidIndex` naming the index and the dimension. A non-finite value → `SimError::NonFinite`. `n > u32::MAX as usize` → `SimError::LimitExceeded`.
- **Tests:** `csc_from_rows_sums_duplicates` — two triplets at the same position sum. `mat_vec_reproduces_a_dense_product` — over a random sparse matrix, checked against an `O(n²)` dense loop written in the test. `col_ptr_is_monotone_and_ends_at_nnz` — structural invariant. `out_of_range_indices_are_refused` — asserts `Err`. `a_1e6_by_1e6_matrix_with_100_nnz_is_cheap` — a timing-free structural assertion on capacity.
- **Acceptance:** Green; `SIM-STATIC-008` assembles into it.
- **Risk:** `u32` overflow on a dof count above 4.29e9 — impossible for `max_elements = 2e6` × 4 dof. Fallback: the explicit check is still there and tested.

### SIM-SLV-003 — AMD ordering from scratch
- **State:** READY
- **Depends on:** SIM-SLV-002
- **Files:** `crates/sim/src/linalg/order.rs` (new, SIM)
- **Contract:** `pub fn amd(a: &CscMatrix) -> Result<Vec<u32>, SimError>` and `pub fn fill_in(a: &CscMatrix, perm: &[u32]) -> Result<FillIn, SimError>` with `pub struct FillIn { pub original_nnz: usize, pub factor_nnz: usize, pub ratio: f64 }`.
- **Behaviour:** Approximate minimum degree ordering, in pure Rust, following the published algorithm (Amestoy–Davis–Duff). `fill_in` symbolically factors under the permutation and reports the fill ratio, which is **the number that decides whether a from-scratch solver is viable** — a beam mesh that fills 40× is fine, a 3D solid mesh that fills 5 000× is not. `fill_in` is therefore a deliverable of this task, not a diagnostic.
- **Refusals:** A matrix with a zero diagonal → `SimError::SingularStiffness { dof: i, pivot: 0.0, .. }`. A non-symmetric input to a symmetric-only ordering → `SimError::Unsupported("AMD expects a symmetric pattern")`. A matrix with no edges (a single node) → `Ok` with a one-element permutation, **not** an error.
- **Tests:** `amd_is_a_permutation` — asserts every index appears exactly once. `amd_reduces_fill_against_natural_ordering` — on a 3D grid stiffness matrix, asserts the fill ratio under AMD is below that under identity. `fill_in_on_a_chain_has_no_fill` — a tridiagonal matrix must have `ratio == 1.0`, which is the exact expected answer and a real test of the symbolic factorisation. `a_zero_diagonal_is_refused` — asserts `Err`.
- **Acceptance:** Green, and its `fill_in` numbers are quoted in the `SIM-SLV-001` ADR.
- **Risk:** AMD's supernodal/approximate degree tracking is subtle and a simple implementation can fill badly. Fallback: a simpler minimum-degree ordering (exact degree, more expensive) is acceptable if AMD's fill ratio does not beat natural ordering on the benchmark meshes; the choice is recorded.

### SIM-SLV-004 — Symbolic Cholesky analysis
- **State:** BACKLOG
- **Depends on:** SIM-SLV-003
- **Files:** `crates/sim/src/linalg/factor.rs` (new, SIM)
- **Contract:** `pub fn analyze(a: &CscMatrix, perm: &[u32]) -> Result<SymbolicFactor, SimError>` with `pub struct SymbolicFactor { pub col_ptr: Vec<u32>, pub row_idx: Vec<u32>, pub parent: Vec<i64>, pub n: usize, pub nnz: usize }`.
- **Behaviour:** Column counts and the elimination tree for an up-looking sparse Cholesky `A = L·Lᵀ`. Store, do not compute: the numeric pass then does no allocation at all, which is what makes a direct solve fast enough to be usable. This is the separation that makes the whole from-scratch path tractable.
- **Refusals:** A pattern that is not symmetric (`a(i,j) != a(j,i)` in the pattern) → `SimError::Unsupported("the stiffness pattern must be symmetric")`, checked structurally, not numerically. `n == 0` → `SimError::Empty`.
- **Tests:** `symbolic_on_a_chain_gives_no_fill` — `nnz == n`. `symbolic_on_a_grid_matches_the_known_count` — an `n×n` grid mesh's `L` nonzeros are analytically known; the test asserts within a small tolerance. `the_parent_array_is_consistent` — every `parent[i] > i` or `parent[i] < 0`. `an_asymmetric_pattern_is_refused` — asserts `Err`.
- **Acceptance:** Green, with its `nnz` prediction used as a pre-allocation size — an allocation that fails is `Err`, not an abort.
- **Risk:** The memory blow-up on a real 3D mesh is the true risk, and it shows up here first, which is the point. Fallback: `SIM-SLV-001`'s ADR already needs the fill number; if it is catastrophic, that is evidence for Outcome C.

### SIM-SLV-005 — Numeric LDLᵀ factorisation with a pivot floor
- **State:** BACKLOG
- **Depends on:** SIM-SLV-004, SIM-SLV-001
- **Files:** `crates/sim/src/linalg/factor.rs` (SIM)
- **Contract:** `pub fn factorize(a: &CscMatrix, s: &SymbolicFactor) -> Result<NumericFactor, SimError>` with `pub struct NumericFactor { pub vals: Vec<f64>, pub perm: Vec<u32>, pub iperm: Vec<u32>, pub diagonal: Vec<f64> }` and `pub fn solve_in_place(&self, b: &mut [f64]) -> Result<(), SimError>`.
- **Behaviour:** Compute the numeric factor **in the symbolic order**, in place, in `f64`. `L` unit-diagonal, `D` diagonal, so a symmetric positive definite system factorises with `D` all positive. A pivot below `floor = 1e-14 · ‖A‖_max` is treated as **structurally singular** and is `SimError::SingularStiffness { dof, pivot, detail }` naming the dof. It is **never** regularised by quietly adding epsilon to the diagonal: a regularised solve returns a plausible, wrong displacement field, which is the worst outcome in this plan.
- **Refusals:** `SimError::SingularStiffness` with the pivot value and the dof index. A non-finite value appearing mid-factorisation → `SimError::NonFinite`. A matrix whose pattern does not match `s` → `SimError::InvalidIndex`.
- **Tests:** `factoring_an_spd_matrix_gives_positive_pivots` — over a random SPD matrix, asserts every `D` entry > 0. `a_singular_matrix_names_its_dof` — a matrix with a zero row names that row's index. `no_silent_regularisation` — asserts that factoring a singular matrix is `Err`, not `Ok` with a perturbed diagonal. `the_factorisation_reproduces_the_matrix` — asserts `L·D·Lᵀ ≈ A` for a random SPD matrix.
- **Acceptance:** Green; `SIM-STATIC-011` solves with it.
- **Risk:** Cancellation makes `L` fill with zeros and the diagonal absorbs the error, giving a silent 5 % answer on an ill-conditioned beam. Fallback: `SIM-SLV-008` adds iterative refinement and a condition estimate, both of which fail loudly instead.

### SIM-SLV-006 — Triangular solves
- **State:** BACKLOG
- **Depends on:** SIM-SLV-005
- **Files:** `crates/sim/src/linalg/factor.rs` (SIM)
- **Contract:** `pub fn solve(&self, b: &[f64], x: &mut [f64]) -> Result<(), SimError>` where `self` is `NumericFactor`; performs the forward substitution, the `D` divide, the backward substitution and the permutation.
- **Behaviour:** Three sweeps over the factor, forward, divide, backward, then unpermute. The permutation direction is the single most common bug in a hand-written solver: `b` must be permuted by `perm` and `x` unpermuted by `iperm`, and a mixed-up pair produces a solution of the *wrong* matrix that still satisfies nothing. The test suite is built to catch it.
- **Refusals:** `b.len() != n` or `x.len() != n` → `SimError::InvalidIndex`. A non-finite `b` → `SimError::NonFinite`.
- **Tests:** `solve_reproduces_a_known_vector` — build `A`, pick `u`, compute `b = A·u`, solve, assert `u` back. `the_permutation_is_applied_in_both_directions` — the same test with a non-identity permutation, which is the test that catches the mix-up. `a_non_finite_rhs_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** Same as above. Fallback: the manufactured-solution test (`SIM-SLV-013`) runs on a real beam mesh with AMD ordering, where a mixed-up permutation cannot pass.

### SIM-SLV-007 — Residual, iterative refinement and the convergence record
- **State:** BACKLOG
- **Depends on:** SIM-SLV-006
- **Files:** `crates/sim/src/linalg/refine.rs` (new, SIM)
- **Contract:** `pub fn residual(a: &CscMatrix, x: &[f64], b: &[f64]) -> Result<f64, SimError>` (the 2-norm relative residual) and `pub fn refine(a: &CscMatrix, f: &NumericFactor, b: &[f64], x: &mut [f64], passes: usize, tol: f64) -> Result<RefineReport, SimError>` with `pub struct RefineReport { pub passes: usize, pub residual: f64, pub residual_history: Vec<f64> }`.
- **Behaviour:** Compute `‖b − A·x‖/‖b‖` after the solve and refine until it is below `AccuracyBar::residual`. **The final residual is part of the study report.** A solve that reports "done" without a residual number is not a result — and on a badly scaled mesh this is the difference between a 2 % answer and a 400 % one.
- **Refusals:** `tol <= 0.0` → `SimError::BadTolerance`. `‖b‖ == 0.0` (a model with no load at all) → a defined outcome: the residual is reported as `0.0` and the solution is the null vector, **not** a division by zero. `passes` exhausted with the residual above `tol` → `SimError::SolverFailed` with the history attached.
- **Tests:** `refinement_reaches_the_residual_bar` — on the beam benchmark, asserts the final residual ≤ `AccuracyBar::residual`. `an_empty_rhs_is_handled` — asserts `Ok` with zero residual. `a_solve_that_cannot_converge_is_an_error` — asserts `SolverFailed` with a history of the right length. `the_history_is_monotone` — over a well-conditioned case, each pass reduces the residual.
- **Acceptance:** Green, and every `ResultSet` carries the final residual.
- **Risk:** Refinement converges to a solution of a slightly different matrix when `A` is stored inexactly. Fallback: `A` is assembled in `f64` and never rounded to `f32`; a `f32` assembly path is refused outright.

### SIM-SLV-008 — Singularity diagnosis that distinguishes causes
- **State:** BACKLOG
- **Depends on:** SIM-SLV-005
- **Files:** `crates/sim/src/linalg/diagnose.rs` (new, SIM)
- **Contract:** `pub fn diagnose(a: &CscMatrix, s: &SymbolicFactor) -> Result<Diagnosis, SimError>` with `pub enum Diagnosis { Unconstrained { dofs: Vec<usize> }, RigidBody { modes: Vec<RigidMode> }, Mechanism { dofs: Vec<usize> }, ConditionallySingular { pivot: f64, dof: usize } }` and `pub enum RigidMode { Translation([usize; 3]), Rotation([usize; 3]) }`.
- **Behaviour:** When the factorisation fails, **name the cause**, not just "singular". Walk the constraint pattern: a dof with an empty row is *unconstrained*; a connected component of the mesh with fewer than three distinct fixed nodes is *rigid-body*; a pattern with an empty row *and* its neighbours constrained is a *mechanism*. The message tells the user which dof to fix, which is the difference between a usable error and an unhelpful one.
- **Refusals:** This function is called only on a failed factorisation; it returns `Ok(Diagnosis)` and the caller turns it into `SimError::SingularStiffness { dof, pivot, detail: <the diagnosis> }`. A diagnosis that cannot be made is `Diagnosis::ConditionallySingular`, never a guess.
- **Tests:** `a_free_dof_is_named_as_unconstrained` — remove one constraint from a beam and assert the diagnosis names that dof. `a_fully_fixed_beam_has_no_rigid_mode` — asserts `Ok(diagnose)` on a well-posed system. `a_single_fixed_node_gives_three_translation_modes` — asserts exactly 3. `hostile_matrices_do_not_panic` — a zero matrix, a `NaN` matrix, a disconnected matrix.
- **Acceptance:** Green; `SIM-STATIC-021` uses it as the pre-solve check.
- **Risk:** It could be a rabbit hole. Fallback: the `Unconstrained` and `RigidBody` cases cover the overwhelming majority of user error and are implemented first; `Mechanism` may return `ConditionallySingular`.

### SIM-SLV-009 — Dense reference solver for small systems
- **State:** READY
- **Depends on:** SIM-SLV-001
- **Files:** `crates/sim/src/linalg/dense.rs` (new, SIM)
- **Contract:** `pub fn dense_cholesky(a: &[Vec<f64>], n: usize) -> Result<Vec<f64>, SimError>` returning `L`, and `pub fn dense_solve(l: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, SimError>`.
- **Behaviour:** An `O(n³)` Cholesky for `n ≤ 1000`, written independently of the sparse path. It exists for one reason: **every sparse solver is validated against it**, including the manufactured-solution tests. A sparse solver with no independent reference has no evidence that it is right, only evidence that it is stable.
- **Refusals:** A non-positive pivot → `SimError::SingularStiffness`. `n > 4096` → `SimError::LimitExceeded { what: "dense reference size", .. }`. A non-symmetric or non-finite input → `SimError::NonFinite`.
- **Tests:** `dense_matches_the_sparse_solver` — the same SPD system through both paths agrees to 1e-12. `a_known_matrix_reproduces_a_known_factor` — a hand-computed 3×3. `a_non_positive_pivot_is_refused` — asserts `Err`.
- **Acceptance:** Green, and it is the reference every other solver test names.
- **Risk:** Nobody keeps it in step with the sparse path. Fallback: it is small, self-contained and has its own tests; it is also the only solver that can run in a wasm browser tab, which makes it useful beyond testing.

### SIM-SLV-010 — Symmetric-indefinite LDLᵀ with 2×2 pivots
- **State:** BACKLOG
- **Depends on:** SIM-SLV-005
- **Files:** `crates/sim/src/linalg/indefinite.rs` (new, SIM)
- **Contract:** `pub fn factorize_indefinite(a: &CscMatrix, perm: &[u32]) -> Result<IndefiniteFactor, SimError>` — Bunch–Kaufman 1×1/2×2 pivoting — and `pub fn solve_indefinite(f: &IndefiniteFactor, b: &[f64], x: &mut [f64]) -> Result<(), SimError>`.
- **Behaviour:** Required by `SIM-NL-012` (Lagrange-multiplier contact produces an indefinite saddle-point system) and by any eigenvalue work. Refuses a matrix that is not symmetric. **It is not needed for `SIM-STATIC`**, which is why it is late in this epic and behind an explicit dependency.
- **Refusals:** `SimError::Unsupported("the matrix pattern is not symmetric")`. A zero 2×2 pivot → `SimError::SingularStiffness`.
- **Tests:** `an_indefinite_matrix_factors` — a 3×3 with one negative eigenvalue. `it_reproduces_the_matrix` — `P·A·Pᵀ = L·D·Lᵀ` to 1e-10. `an_asymmetric_matrix_is_refused` — asserts `Err`. `it_agrees_with_a_dense_indefinite_reference` — a small dense LU in the test module.
- **Acceptance:** Green, and only reachable from `SIM-NL`.
- **Risk:** Bunch–Kaufman's stability argument is subtle. Fallback: it is only used for contact, which is Tier B/C anyway; if it is wrong the contact result is `Unverified` regardless.

### SIM-SLV-011 — Preconditioned CG (accelerator, not a solver of record)
- **State:** BACKLOG
- **Depends on:** SIM-SLV-005
- **Files:** `crates/sim/src/linalg/cg.rs` (new, SIM)
- **Contract:** `pub struct Jacobi;` / `pub struct IncompleteCholesky { pub drop_tol: f64 }` as `Preconditioner`, and `pub fn pcg(a: &CscMatrix, b: &[f64], p: &dyn Preconditioner, tol: f64, max_iters: usize) -> Result<IterativeResult, SimError>` with `pub struct IterativeResult { pub x: Vec<f64>, pub iterations: usize, pub residual: f64, pub converged: bool }`.
- **Behaviour:** A safety net and an accelerator, never the default. `converged == false` is returned as a value, **and** the caller treats a non-converged result as `SimError::SolverFailed`. The preconditioner is IC(0) or Jacobi. Documented honestly: on a 3D solid mesh, direct solve beats preconditioned CG until the problem is far larger than anything this mesher currently produces.
- **Refusals:** `tol <= 0.0` or `max_iters == 0` → `SimError::BadTolerance`. **A CG applied to a matrix that is not positive definite is not detected and will not converge** — this is stated in the doc comment, and CG is only ever called from a path that has established SPD.
- **Tests:** `pcg_reaches_the_dense_solution` — the same SPD system through both, agreeing to 1e-10. `a_non_converging_run_returns_converged_false` — asserts the flag, not an `Err`. `ic_preconditioning_beats_jacobi` — on a beam mesh, IC needs strictly fewer iterations. `max_iters_zero_is_refused` — asserts `Err`.
- **Acceptance:** Green, and `SIM-SLV-013` reports whether it was ever needed.
- **Risk:** Being used by default and quietly under-converging. Fallback: the report records the method used, and `converged == false` is an `Err` upstream.

### SIM-SLV-012 — Manufactured-solution conformance suite
- **State:** BACKLOG
- **Depends on:** SIM-SLV-009, SIM-SLV-007
- **Files:** `crates/sim/tests/solver.rs` (SIM)
- **Contract:** `pub fn manufactured(n: usize, seed: u64) -> Result<ManufacturedCase, SimError>` with `pub struct ManufacturedCase { pub a: CscMatrix, pub x_exact: Vec<f64>, pub b: Vec<f64> }` and `pub fn check(case: &ManufacturedCase, x: &[f64], tol: f64) -> bool`.
- **Behaviour:** Generate a random SPD sparse matrix of any size, pick an exact `x`, form `b = A·x` in `f64`, solve, compare. **This is the only test that can detect a solver that is stable, self-consistent and wrong** — it does not care how the matrix was factored. Run at `n = 10, 10², 10³, 10⁴, 10⁵` with three orderings (natural, AMD, random).
- **Refusals:** A generated matrix that is not SPD within a tolerance → the generator is re-seeded, and the number of rejections is asserted to be below a bound so a broken generator cannot loop forever.
- **Tests:** `every_size_recovers_the_exact_solution` — to `1e-10` relative, at every `n`. `every_ordering_recovers_it` — natural, AMD and random. `a_deliberately_scrambled_solve_fails_the_check` — proves the check has teeth. `n_1e5_completes_within_the_budget` — a timing assertion recorded in the baseline (`QAT-004`).
- **Acceptance:** Green at all five sizes and three orderings, with the timings recorded.
- **Risk:** Passing at `n = 10³` and failing at `10⁵` because of an index-type overflow in the scatter. Fallback: `u32` overflow is checked at `SIM-SLV-002`, and the `10⁵` case is in the suite.

### SIM-SLV-013 — Performance characterisation and the regression rule
- **State:** BACKLOG
- **Depends on:** SIM-SLV-012, QAT-004
- **Files:** `crates/sim/benches/solve.rs` (SIM)
- **Contract:** `pub fn record_baseline(name: &str, n: usize, flops: f64, seconds: f64) -> Result<BenchmarkRecord, SimError>` with `pub struct BenchmarkRecord { pub name: String, pub n: usize, pub gflops: f64, pub machine: String }`.
- **Behaviour:** Record factorise time, solve time and memory for a beam, a plate and a solid, at three sizes. `QAT-004`'s rule applies: a regression above 10 % fails the build. **The machine string is part of the record** — a number without the machine it was measured on is not a result. A comparison of direct against PCG is recorded at the same time (`SIM-SLV-011`).
- **Refusals:** A baseline recorded without a machine string → `SimError::Empty`. A run that exceeds the memory cap → `SimError::LimitExceeded` with the model size that exceeded it, which is the honest ceiling statement the ADR needs.
- **Tests:** `a_record_without_a_machine_is_refused` — asserts `Err`. `the_regression_rule_is_10_percent` — a unit test on the comparison function with a 9 % and an 11 % case.
- **Acceptance:** Baselines committed and referenced from the ADR; the ADR quotes them.
- **Risk:** Numbers measured on a shared VM, where another agent's build is loading the machine. Fallback: the record stores the machine and a load note, and the 10 % rule applies to the median of five runs.

---

## 8. `SIM-STATIC` — Linear structural analysis (24 tasks)

### SIM-STATIC-001 — `Material`: isotropic linear elasticity with validation
- **State:** BACKLOG
- **Depends on:** SIM-VAL-004
- **Files:** `crates/sim/src/materials.rs` (new, SIM)
- **Contract:** `pub struct Material { pub name: String, pub density: f64, pub youngs: f64, pub poisson: f64, pub thermal_k: Option<f64>, pub thermal_alpha: Option<f64> }` with `pub fn constitutive(&self) -> Result<[[f64; 6]; 6], SimError>` (`[xx, yy, zz, xy, xz, yz]` Voigt order) and `pub fn lame(&self) -> Result<(f64, f64), SimError>` (`λ`, `µ`).
- **Behaviour:** The single material record. `constitutive` builds `D` in Voigt order with **shear components using engineering shear** (`G`), which is the convention `SIM-VAL-010` also assumes. All SI. A material library (`steel`, `aluminium`, `abs`, …) is a task in `SIM-STATIC-002`.
- **Refusals:** `youngs <= 0.0` → `SimError::BadDistance`. `poisson <= -1.0 || >= 0.5` → `SimError::Unsupported` naming the range, because a `ν` outside it has no positive-definite elasticity tensor and `SIM-VAL-014` refuses `ν` near 0.5. `density < 0.0` → `SimError::BadDistance`. A non-finite field → `SimError::NonFinite`.
- **Tests:** `constitutive_is_symmetric` — asserts `D[i][j] == D[j][i]`. `steel_is_the_steel_numbers` — asserts `E = 200e9`, `ν = 0.3` produce the documented `λ` and `µ` to 1e-3. `an_auxetic_material_is_accepted` — `ν = -0.4` is legal and `D` is still positive definite. `poisson_one_half_is_refused` — asserts `Err`, and names `SIM-NL-012`.
- **Acceptance:** Green; every study names its material and prints it in the report.
- **Risk:** Voigt order confusion between `D` and `B`. Fallback: the order is stated in the type's doc comment and both `SIM-VAL-009` and the patch test are order-sensitive, so a swap fails immediately.

### SIM-STATIC-002 — `Constraint` and DOF elimination
- **State:** BACKLOG
- **Depends on:** SIM-MSH-010, SIM-SLV-008
- **Files:** `crates/sim/src/fem/constraint.rs` (new, SIM)
- **Contract:** `pub enum Constraint { Fixed { nodes: Vec<u32> }, FixDofs { nodes: Vec<u32>, dofs: [bool; 3] }, Prescribed { node: u32, dof: usize, value: f64 }, Symmetry { nodes: Vec<u32>, point: Vec3, normal: Vec3 }, Roller { nodes: Vec<u32>, direction: Vec3 } }` and `pub fn apply(m: &Model, mesh: &FemMesh) -> Result<FixedDofs, SimError>` with `pub struct FixedDofs { pub fixed: Vec<bool>, pub prescribed: BTreeMap<usize, f64>, pub free: Vec<usize> }`.
- **Behaviour:** Constraints eliminate dofs from the system rather than adding a huge stiffness: a prescribed dof is removed and its load moved to the right-hand side, so the matrix is never worse conditioned by an artificial penalty. `Symmetry` is the one that is easy to get wrong — on a symmetry plane, the two in-plane dofs are free and only the normal one is constrained; a user who fixes all three has clamped the part and will get a stiffer, wrong answer with no error message.
- **Refusals:** A constraint naming a node not in the mesh → `SimError::InvalidIndex`. A prescribed dof on a dof that is also fixed to a different value → `SimError::Unsupported("dof {i} is constrained twice")`. A symmetry normal with `len < 0.5` → `SimError::BadDirection`. **A model with a free dof that is rigid-body is not rejected here** — `SIM-STATIC-021` catches it with a better message.
- **Tests:** `a_fully_fixed_node_removes_three_dofs` — asserts the free list is 3 short. `a_prescribed_dof_moves_load_to_the_rhs` — asserts the solve honours the value exactly. `symmetry_frees_the_in_plane_dofs` — asserts 2 of 3 free on the plane. `a_dof_constrained_twice_is_refused` — asserts `Err`. `constraints_are_order_independent` — two applications in different orders give the same `FixedDofs`.
- **Acceptance:** Green, and `SIM-STATIC-021` runs after it.
- **Risk:** `Symmetry` implemented as a full clamp. Fallback: the test asserts exactly 2 free dofs per on-plane node, which is a strong statement about the intent.

### SIM-STATIC-003 — `Load`: force, pressure, body force, traction
- **State:** BACKLOG
- **Depends on:** SIM-MSH-009, SIM-STATIC-002
- **Files:** `crates/sim/src/fem/load.rs` (new, SIM)
- **Contract:**
  ```rust
  pub enum Load {
      Force { face_set: String, total: Vec3, uniform: bool },
      Pressure { face_set: String, value: f64 },
      BodyForce { accel: Vec3 },
      Traction { face_set: String, fn_: fn(Vec3) -> Vec3 },
      Remote { face_set: String, at: Vec3, total: Vec3 },
  }
  pub fn load_vector(mesh: &FemMesh, sets: &BTreeMap<String, Vec<u32>>, loads: &[Load], model_density: f64) -> Result<Vec<f64>, SimError>
  ```
- **Behaviour:** Loads assemble into a consistent nodal vector. A `Force` with `uniform: false` is distributed over the face's Gauss points **proportionally to area**; with `uniform: true` it is concentrated at the centroid. `Pressure` uses the §0.2 convention: positive `p` produces `t = −p·n̂` on a face whose normal is outward. `Remote` is a wrench applied at a point through a face set (the bolt-load case) and must be **in equilibrium** with itself — an unbalanced remote moment is refused.
- **Refusals:** An unknown face set name → `SimError::Empty` naming the set. A non-finite value → `SimError::NonFinite`. A `Remote` whose resultant does not match `total` (net force differs by more than `1e-9·|total|`) → `SimError::Unsupported("the remote wrench is not in equilibrium with the stated total")`. A face whose normals are not all outward (a mesh from an inside-out solid) → `SimError::NotConforming`.
- **Tests:** `a_pressure_integrates_to_the_stated_total_force` — a `p` over a known area gives `p·A`, the single most important load test. `a_uniform_face_force_lands_at_the_centroid` — asserts the moment about any other point is zero. `a_non_uniform_force_has_zero_net_moment_about_the_centroid` — area-proportional distribution is balanced. `body_force_equals_density_times_gravity_times_volume` — asserts `ρ·g·V`. `an_unbalanced_remote_wrench_is_refused` — asserts `Err`.
- **Acceptance:** Green; the pressure test is quoted by `SIM-THERM` for the convective boundary condition.
- **Risk:** Pressure sign flipping relative to the stress convention, which is exactly why §0.2 names it separately. Fallback: the test asserts `p > 0` produces **inward** traction on a face with `+Z` normal, in words.

### SIM-STATIC-004 — Load vector on a known-answer model
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-003, SIM-VAL-002
- **Files:** `crates/sim/src/fem/load.rs` (SIM), `crates/sim/tests/static.rs` (SIM)
- **Contract:** `pub fn cantilever_tip_force_load(mesh: &FemMesh, end_set: &str, force: Vec3) -> Result<Vec<f64>, SimError>`.
- **Behaviour:** The load path validated against `SIM-VAL-002` before any solver runs: build a cantilever mesh, apply a tip force through the JSON-shaped path, and assert the assembled vector's resultant is the force and its moment is `F × L`. A load path that is off by a factor of `h` is invisible until results are interpreted.
- **Refusals:** An empty end set → `SimError::Empty`. A force not perpendicular to the beam axis when a transverse case was intended → not an error, but the test asserts it.
- **Tests:** `the_load_vector_sums_to_the_applied_force` — exact. `its_moment_about_the_root_is_force_times_arm` — exact. `a_pressure_on_a_face_sums_to_p_times_area` — exact.
- **Acceptance:** Green, and it runs before `SIM-STATIC-011` is allowed to be trusted.
- **Risk:** None.

### SIM-STATIC-005 — Gauss quadrature rules per element kind
- **State:** BACKLOG
- **Depends on:** SIM-MSH-001
- **Files:** `crates/sim/src/fem/quadrature.rs` (new, SIM)
- **Contract:** `pub fn rule(kind: ElementKind) -> &'static [QuadraturePoint]` with `pub struct QuadraturePoint { pub xi: Vec3, pub weight: f64 }` — `Tet4` 1 point, `Tet10` 4 points, `Hex8` 2×2×2, `Hex27` 3×3×3, `Quad4` 2×2, `Tri3` 1 point, `Tri6` 3 points, `Wedge6` 6 points.
- **Behaviour:** Reference-domain rules with exact polynomial orders: `Tet4`'s single point is **not** enough for the full `BᵀDB` integrand (which needs order 2), which is precisely why linear tets are too stiff for bending — the quadrature order is a *modelling choice* with a known consequence and it is recorded per element kind in the table. Each rule's weights must sum to the exact reference volume.
- **Refusals:** `terms < 1` or a weight that is zero or negative → `SimError::Unsupported`. A rule for an unsupported kind → `SimError::Unsupported("no quadrature rule for {kind}")` — **no silent fallback to the centroid**.
- **Tests:** `weights_sum_to_the_reference_volume` — for every kind. `a_quad_integrates_a_cubic_exactly` — the moment test. `a_hex27_integrates_a_ninth_degree_polynomial` — the reason `Hex27` exists. `an_unsupported_kind_is_refused` — asserts `Err`.
- **Acceptance:** Green; the table is the single source for both `SIM-STATIC-007` and `SIM-THERM-003`.
- **Risk:** Choosing the wrong order and silently changing results between element types. Fallback: the order per kind is a documented field of the table, printed by `SIM-STATIC-024`.

### SIM-STATIC-006 — Shape functions and their derivatives
- **State:** BACKLOG
- **Depends on:** SIM-MSH-001, SIM-STATIC-005
- **Files:** `crates/sim/src/fem/shape.rs` (new, SIM)
- **Contract:** `pub fn shape(kind: ElementKind, xi: Vec3) -> Result<Vec<f64>, SimError>` and `pub fn shape_gradient(kind: ElementKind, xi: Vec3) -> Result<Vec<Vec3>, SimError>` (gradients with respect to the reference coordinates), plus `pub fn jacobian(kind: ElementKind, dshape: &[Vec3]) -> Result<[[f64; 3]; 3], SimError>` and `pub fn det_jacobian(j: &[[f64; 3]; 3]) -> f64`.
- **Behaviour:** Reference shape functions per kind, with the **node ordering fixed by `SIM-MSH-001`** so that `det(J) > 0` for a right-handed element. `jacobian` returns the full 3×3 including the `ξ₃` row (zero for 2D kinds) so the volumetric term is right.
- **Refusals:** A non-finite `xi` → `SimError::NonFinite`. An unsupported kind → `SimError::Unsupported`. **A `det(J) ≤ 0` at element construction is `SimError::InvertedElement { element, jacobian }`** (this is where §0.4's assertion lives).
- **Tests:** `tet4_shape_functions_partition_unity` — `Σ N = 1`. `their_gradient_sums_to_zero` — `Σ ∇N = 0`. `a_node_shape_function_is_one_at_its_own_node` — Lagrange property for every kind. `a_unit_cube_hex8_has_identity_jacobian` — `J = I`. `an_inverted_element_is_refused` — asserts `Err`, not a negative number returned.
- **Acceptance:** Green; validated by `SIM-VAL-009`, which is the real test of this file.
- **Risk:** A gradient wrong in one component only, which a symmetric-looking test misses. Fallback: `SIM-VAL-009`'s patch test on a distorted element catches it.

### SIM-STATIC-007 — The element stiffness matrix
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-001, SIM-STATIC-006
- **Files:** `crates/sim/src/fem/element.rs` (new, SIM)
- **Contract:** `pub fn stiffness(kind: ElementKind, nodes: &[Vec3], d: &[[f64; 6]; 6]) -> Result<ElementMatrix, SimError>` with `pub struct ElementMatrix { pub ke: Vec<f64>, pub dofs: usize, pub volume: f64 }`.
- **Behaviour:** `K_e = ∫ Bᵀ D B det(J) dξ` over `SIM-STATIC-005`'s points. **The integration reference is the element's own centroid, never the world origin** — the §1.3 lesson from `mass.rs:33` and its test `mass_properties_are_stable_far_from_the_origin`. Shape-function gradients and `B` are computed about the element centroid; the parallel-axis term is applied explicitly when assembling the global vector. `ElementMatrix::volume` is returned so the mesh-volume check and the load path can be cross-verified against the same number.
- **Refusals:** `SimError::InvertedElement` for `det(J) ≤ 0`. `SimError::NonFinite` if any entry is non-finite. A node count that does not match the kind → `SimError::InvalidIndex`.
- **Tests:** `stiffness_is_symmetric` — to 1e-12 relative. `the_diagonal_is_positive` — every diagonal entry > 0 for a well-shaped element. `stiffness_volume_matches_the_determinant_sum` — `∫det(J)` equals the signed tet volume. `a_tet_at_1e6_has_the_same_matrix_as_at_zero` — **the `mass.rs` lesson as a test**: translate the element to 1e6 and assert `K_e` is unchanged to 1e-6. `a_rigid_body_translation_produces_zero_force` — `K_e · (1,1,1,1,1,1)ᵀ = 0` to 1e-9.
- **Acceptance:** Green, including the translated-element test, which is the single most valuable numerical test in this task.
- **Risk:** Getting a large translation to silently degrade conditioning because the shape functions mix absolute positions in. Fallback: the translated-element test catches it, and the implementation is required to work in local coordinates.
- **Note:** Reuses nothing from `mass.rs` directly — the tetrahedron decomposition there is a *verification* route (`SIM-VAL-009`), not a production path. What is reused is the **rule**.

### SIM-STATIC-008 — Global assembly into CSC
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-007, SIM-SLV-002
- **Files:** `crates/sim/src/fem/assemble.rs` (new, SIM)
- **Contract:** `pub fn assemble(mesh: &FemMesh, d: &[[f64; 6]; 6], fixed: &FixedDofs) -> Result<LinearSystem, SimError>` with `pub struct LinearSystem { pub n: usize, pub a: CscMatrix, pub b: Vec<f64> }`.
- **Behaviour:** Scatter `K_e` into the reduced dof numbering with the triplets-summed `from_rows`. Fixed dofs are eliminated: their rows and columns are dropped and their contributions moved to `b`. The dof map is computed once and reused by every subsequent operation.
- **Refusals:** `SimError::InvalidIndex` for a dof outside the map. `SimError::SingularStiffness` from a row that ends up entirely empty. A non-finite value in `K_e` → `SimError::NonFinite`. A mesh with an element whose nodes are not in `mesh.nodes` → `SimError::InvalidIndex`.
- **Tests:** `assembly_preserves_symmetry_in_the_pattern` — asserts `col_ptr`/`row_idx` are structurally symmetric. `a_fixed_dof_appears_nowhere_in_the_matrix` — asserts its row and column are gone. `assembly_is_deterministic` — two runs produce identical triplets. `the_sum_of_row_diagonals_is_positive` — a cheap global sanity check.
- **Acceptance:** Green; the matrix's `nnz` matches `SIM-SLV-004`'s prediction (a check that catches a scatter bug).
- **Risk:** A dof-map off-by-one that shifts every result by one node. Fallback: `SIM-SLV-012`'s manufactured-solution test runs on a *meshed* system, so an off-by-one fails there.

### SIM-STATIC-009 — Natural boundary conditions to the load vector
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-003, SIM-STATIC-008
- **Files:** `crates/sim/src/fem/assemble.rs` (SIM)
- **Contract:** `pub fn apply_natural(ls: &mut LinearSystem, loads: &[Load], mesh: &FemMesh, sets: &BTreeMap<String, Vec<u32>>, fixed: &FixedDofs) -> Result<(), SimError>`.
- **Behaviour:** Integrate the traction and body force over the elements, then **scatter into the same reduced dof numbering the matrix uses**. The prescribed-dof contributions are moved to the right-hand side: `b[i] -= K[i][fixed] · u_fixed`. Getting this sign wrong produces a model that is too soft by exactly the restrained energy, which is a quiet error.
- **Refusals:** As `SIM-STATIC-003`. Additionally a prescribed dof with no fixed value → `SimError::Unsupported("dof {i} is eliminated but has no value")`, which catches a constraint set that removed a dof without saying what it should be.
- **Tests:** `the_prescribed_contribution_has_the_right_sign` — a single prescribed dof with a known `K` row, checked by hand. `the_summed_load_equals_the_applied_total` — the resultant of `b` equals the sum of all applied loads to 1e-9. `a_body_force_matches_density_times_g` — to 1e-12.
- **Acceptance:** Green.
- **Risk:** The sign. Fallback: the resultant test — if the restraint contribution were signed wrongly, the total would still be right but the deflection wrong; the hand-computed single-dof test is the one that catches it.

### SIM-STATIC-010 — Solve and the displacement field
- **State:** BACKLOG
- **Depends on:** SIM-SLV-007, SIM-STATIC-009
- **Files:** `crates/sim/src/fem/solve.rs` (new, SIM)
- **Contract:** `pub fn solve(ls: &LinearSystem, p: &AccuracyBar) -> Result<SolveReport, SimError>` with `pub struct SolveReport { pub u: Vec<f64>, pub residual: f64, pub method: &'static str, pub factorization_nnz: usize }`.
- **Behaviour:** Factorise and solve, then refine to the residual bar. The report carries the residual, the method and the factor size — all three into the result, all three in the user-visible report. **A displacement field with no residual attached is not a result.**
- **Refusals:** `SimError::SolverFailed` with the refinement history if the residual bar is not met. `SimError::SingularStiffness` from `SIM-SLV-008`'s diagnosis.
- **Tests:** `the_cantilever_reaches_the_analytical_deflection` — `SIM-VAL-002`, within the bar. `the_residual_is_reported_and_within_the_bar` — asserts `report.residual <= p.residual`. `a_singular_model_is_refused_not_solved` — asserts `Err`.
- **Acceptance:** Green and it is the first end-to-end path.
- **Risk:** Passing the cantilever for the wrong reason. Fallback: `SIM-VAL-005..006` (rotation and base moment) and `SIM-VAL-004` (axial) fail differently from `SIM-VAL-002`, so a coincidence cannot pass all four.

### SIM-STATIC-011 — Strain recovery
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-006, SIM-STATIC-010
- **Files:** `crates/sim/src/fem/recover.rs` (new, SIM)
- **Contract:** `pub fn strains(mesh: &FemMesh, u: &[f64]) -> Result<Vec<[f64; 6]>, SimError>` returning `[εxx, εyy, εzz, γxy, γxz, γyz]` per element at its centroid, and `pub fn strains_at_gauss(mesh: &FemMesh, u: &[f64], gp: usize) -> Result<Vec<[f64; 6]>, SimError>`.
- **Behaviour:** `ε = B · u_e` evaluated at the requested points. Centroid evaluation is the default because it is where the patch test is exact and where a user expects "the stress in this element"; Gauss-point values are kept because they are what the element actually integrates. **Both are exposed**, because a user comparing against a textbook will be comparing against the centroid value and a user comparing against another FEA code may be comparing Gauss values, and the difference between them is a real, reportable quantity.
- **Refusals:** A `u` whose length does not match `3 · nodes.len()` → `SimError::InvalidIndex`. A non-finite `u` → `SimError::NonFinite`. An inverted element → `SimError::InvertedElement`.
- **Tests:** `a_uniform_strain_field_is_recovered_exactly` — prescribe `u = A + Bx` exactly and assert `ε = B` to 1e-12, the pure-strain test. `the_patch_test_gives_a_constant_strain` — `SIM-VAL-009` end to end. `voigt_shear_is_engineering` — asserts `γxy = 2εxy` by construction on a known deformation.
- **Acceptance:** Green.
- **Risk:** Shear component halving. Fallback: the explicit engineering-shear test.

### SIM-STATIC-012 — Stress recovery
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-011, SIM-STATIC-001
- **Files:** `crates/sim/src/fem/recover.rs` (SIM)
- **Contract:** `pub fn stresses(mesh: &FemMesh, e: &[[f64; 6]]) -> Result<Vec<[f64; 6]>, SimError>` returning `[σxx, σyy, σzz, σxy, σxz, σyz]`.
- **Behaviour:** `σ = D · ε` in Voigt order with engineering shear, giving the six components symmetrically (`σyz` and `σzy` are the same number, stored once). Exactly what `SIM-VAL-009` requires.
- **Refusals:** A length mismatch → `SimError::InvalidIndex`. A non-finite result → `SimError::NonFinite`. **A stress above a configured sanity ceiling (default: `1e12 · E`) is `SimError::NonFinite`**, because a stress of `1e30·E` means the mesh is broken and presenting it in a contour is the failure mode §0.5 exists to prevent.
- **Tests:** `uniaxial_stress_gives_P_over_A` — `SIM-VAL-004` end to end. `the_patch_test_stress_is_the_exact_value` — `SIM-VAL-009`. `a_sane_ceiling_catches_a_broken_mesh` — a deliberately garbage displacement produces `Err`, not a plotted `1e30`.
- **Acceptance:** Green.
- **Risk:** A mixed unit in `D` (GPa with E in Pa) produces a stress 1e9 off, which on a contour looks plausible. Fallback: the sanity ceiling plus the report printing the material's `E` in full SI next to the result.

### SIM-STATIC-013 — Nodal averaging of a field
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-012
- **Files:** `crates/sim/src/fem/recover.rs` (SIM)
- **Contract:** `pub fn average_to_nodes(mesh: &FemMesh, per_element: &[f64]) -> Result<Vec<f64>, SimError>` (volume-weighted) and `pub fn average_to_nodes_unweighted(mesh: &FemMesh, per_element: &[f64]) -> Result<Vec<f64>, SimError>`.
- **Behaviour:** Element values become node values for display. **Volume weighting, not counting**: an averaging that weights by element count over-weights fine meshes and makes a stress contour shift when the mesh is refined in one region. Both schemes are exposed and the choice is recorded, because the difference between them is visible on a contour and a user comparing two studies needs to know which was used.
- **Refusals:** A length mismatch → `SimError::InvalidIndex`. An element with zero volume in the weighting → `SimError::MeshQuality`, not a silent division by zero. A node used by no element → `SimError::NotConforming`.
- **Tests:** `averaging_preserves_the_mean` — the volume-weighted average of a constant field is that constant, to 1e-12. `a_node_on_a_refinement_boundary_is_continuous` — the weighted average across a 1:2 refinement has no jump. `a_zero_volume_element_is_refused` — asserts `Err`.
- **Acceptance:** Green, and the scheme name is in `ResultSet`.
- **Risk:** Count-weighting creeping in as the "fast" path. Fallback: only the weighted version has a public name; the unweighted one is explicitly `…_unweighted`.

### SIM-STATIC-014 — Von Mises and principal values
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-012
- **Files:** `crates/sim/src/fem/stress.rs` (new, SIM)
- **Contract:** `pub fn von_mises(s: [f64; 6]) -> f64`, `pub fn principal(s: [f64; 6]) -> Result<[f64; 3], SimError>` (sorted descending), `pub fn invariants(s: [f64; 6]) -> [f64; 3]` (`I₁`, `I₂`, `I₃`).
- **Behaviour:** Von Mises from the six components; principal values by the closed-form cubic solution of the characteristic polynomial (with the trigonometric method, which is stable for three real roots, falling back to the closed form for a dominant root). All finite for any symmetric stress.
- **Refusals:** A non-finite component → `SimError::NonFinite`. A negative von Mises (which can only come from a NaN) → `SimError::NonFinite` — **never clamped to zero**, because a clamped value hides the defect.
- **Tests:** `uniaxial_stress_gives_the_stress_itself` — `σ` uniaxial gives `von_mises == σ`. `pure_shear_gives_sqrt_3_times_tau` — `τ` gives `√3·τ`. `hydrostatic_stress_gives_zero` — `σ` equal in all three directions gives `0.0` to 1e-12. `principal_of_a_known_tensor_matches` — a diagonal tensor's principal values are its diagonal. `von_mises_is_never_negative` — over 10⁴ random tensors.
- **Acceptance:** Green.
- **Risk:** A hydrostatic state showing as a coloured contour (it must be zero). Fallback: the hydrostatic test is explicit.

### SIM-STATIC-015 — Reaction forces
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-009, SIM-STATIC-010
- **Files:** `crates/sim/src/fem/recover.rs` (SIM)
- **Contract:** `pub fn reactions(ls: &LinearSystem, full_u: &[f64], mesh: &FemMesh, fixed: &FixedDofs) -> Result<Vec<(u32, Vec3)>, SimError>` returning the reaction at each constrained node.
- **Behaviour:** `R = K_full · u − f_natural`, evaluated on the fixed dofs. Reported per node in newtons, global frame.
- **Refusals:** A dof that is neither fixed nor prescribed → `SimError::Unsupported`. A non-finite reaction → `SimError::NonFinite`.
- **Tests:** `reactions_balance_the_applied_load` — `Σ R = −Σ F` to `1e-9·|F|`. `a_tip_force_gives_a_reaction_at_the_wall` — the cantilever case, checked against `P` and `P·L`. `the_moment_about_the_wall_base_is_zero` — asserts the overturning moment is balanced.
- **Acceptance:** Green. **This is the fastest way for a user to notice a missing constraint**, because the reaction sum is visibly wrong.
- **Risk:** None.

### SIM-STATIC-016 — Global equilibrium check
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-015
- **Files:** `crates/sim/src/fem/check.rs` (new, SIM)
- **Contract:** `pub fn check_equilibrium(ls: &LinearSystem, full_u: &[f64]) -> Result<EquilibriumReport, SimError>` with `pub struct EquilibriumReport { pub force_residual: f64, pub moment_residual: f64, pub passed: bool }`.
- **Behaviour:** `‖ΣF_applied + ΣR‖ / ‖F_applied‖` and the same about the model's centre of mass. This is a **model check, not a solver check** — it fails when a load was dropped, a constraint was missed or a face set matched nothing. It runs on every study and its numbers are in the report.
- **Refusals:** `SimError::Empty` when there is no applied load at all (an unloaded model is a legitimate thing to study for its mass properties, and the check reports "not applicable" rather than dividing by zero). A residual above `1e-8·‖F‖` → `SimError::NotConforming { detail }` **when the user asked for the check**, and is reported as a warning otherwise — the check is on by default and refusable only explicitly.
- **Tests:** `a_correctly_loaded_model_passes` — the cantilever. `a_missing_reaction_fails_the_check` — deliberately drop a constraint and assert `passed == false`. `an_unloaded_model_is_not_applicable` — asserts `Err(Empty)` naming "no applied load".
- **Acceptance:** Green, and `passed` is in the report header.
- **Risk:** A model with balanced forces but a wrong answer (self-equilibrated wrong loads). Fallback: it is one of several checks, not the only one; it never claims to prove correctness.

### SIM-STATIC-017 — The energy identity
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-010
- **Files:** `crates/sim/src/fem/check.rs` (SIM)
- **Contract:** `pub fn strain_energy(u: &[f64], ls: &LinearSystem) -> Result<f64, SimError>` and `pub fn work_done(f: &[f64], u: &[f64]) -> Result<f64, SimError>`, and `pub fn check_energy_identity(u: &[f64], f: &[f64], ls: &LinearSystem, tol: f64) -> Result<bool, SimError>`.
- **Behaviour:** For a linear elastic system, `½·uᵀKu == uᵀf` **exactly**, because `Ku = f`. It is a scalar check that catches a whole class of assembly bugs — a wrong scatter, a missing element, a double-counted boundary — none of which the equilibrium check catches. **This is the best bug catcher in the whole static path and it costs one line.**
- **Refusals:** A non-finite energy → `SimError::NonFinite`. `‖f‖ == 0.0` → not applicable, reported as such. A relative mismatch above `tol` (default `1e-9`) → `Err`, with both numbers.
- **Tests:** `the_energy_identity_holds_on_the_cantilever` — to 1e-12. `it_catches_a_dropped_element` — remove one element's contribution and assert the check fails. `it_catches_a_double_counted_load` — assert failure. `an_unloaded_model_is_not_applicable` — asserts the named outcome.
- **Acceptance:** Green; it runs on every study.
- **Risk:** It is a single scalar and a compensating pair of bugs could cancel. Fallback: it is one of four checks (equilibrium, energy, residual, benchmark), never the only one.

### SIM-STATIC-018 — Stress error indicator and smoothing
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-013, SIM-VIZ-004
- **Files:** `crates/sim/src/fem/indicator.rs` (new, SIM)
- **Contract:** `pub fn zz_indicator(mesh: &FemMesh, nodal: &[f64], element_averages: &[f64]) -> Result<Vec<f64>, SimError>` and `pub fn superconvergent_patch_recovery(mesh: &FemMesh, nodal: &[f64], element_centroid: Vec3) -> Result<f64, SimError>`.
- **Behaviour:** Zienkiewicz–Zhu: patch each element's nodes with a polynomial, extrapolate a higher-order value to the centroid, and compare. The indicator drives `SIM-MSH-017`'s `RefinePredicate::StressGradient`. It is also the honest answer to "where is this stress peak real?", which is a question a raw contour cannot answer.
- **Refusals:** A patch with too few nodes (a 1-element tet patch) → `SimError::Unsupported("a patch needs at least 4 nodes")`. A singular patch polynomial → `SimError::MeshQuality` naming the element. A field whose length does not match the node count → `SimError::InvalidIndex`.
- **Tests:** `a_constant_field_has_a_zero_indicator` — to 1e-10 everywhere. `the_indicator_peaks_at_a_singularity` — a bar with a notch. `a_one_element_patch_is_refused` — asserts `Err`. `the_indicator_is_scale_invariant` — scaling the model scales the field and not the indicator.
- **Acceptance:** Green; `SIM-MSH-017`'s adaptive refinement is gated on it.
- **Risk:** Recovery near a true singularity overshoots and produces a false peak. Fallback: the indicator is a *refinement driver*, never displayed as a stress value; `SIM-VIZ-004` shows the recovered field, not the indicator.

### SIM-STATIC-019 — `ResultSet` assembly
- **State:** BACKLOG
- **Depends on:** SIM-VAL-015, SIM-STATIC-012, SIM-STATIC-015, SIM-STATIC-016, SIM-STATIC-017
- **Files:** `crates/sim/src/fem/study.rs` (new, SIM)
- **Contract:** `pub struct StaticResult { pub displacements: Vec<f64>, pub stress: Vec<[f64; 6]>, pub strain: Vec<[f64; 6]>, pub von_mises: Vec<f64>, pub reactions: Vec<(u32, Vec3)>, pub checks: Checks }` with `pub struct Checks { pub residual: f64, pub equilibrium: EquilibriumReport, pub energy_identity: bool }`, and `pub fn into_result_set(self, mesh_fingerprint: u64, solver: &'static str) -> ResultSet`.
- **Behaviour:** The single funnel from a solved model to a result. `into_result_set` is the **only** way to get a `ResultSet`, and it takes the mesh fingerprint and the solver name, so no result can exist without saying where it came from.
- **Refusals:** `SimError::Empty` if any field is empty. `SimError::Unverified` if `Checks::energy_identity` is false — **a model that fails its own energy identity does not become a result at all.**
- **Tests:** `a_result_set_requires_every_field` — asserts `Err` on an empty displacement vector. `a_failed_energy_identity_produces_no_result` — asserts `Err`. `the_fingerprint_is_carried_through` — asserts the round trip.
- **Acceptance:** Green; nothing in the crate can produce a `ResultSet` another way.
- **Risk:** Someone adds a `Study::run_fast()` for demos. Fallback: the `SIM-VAL-015` test asserts no alternative constructor exists.

### SIM-STATIC-020 — Rigid body mode detection
- **State:** BACKLOG
- **Depends on:** SIM-SLV-008, SIM-STATIC-002
- **Files:** `crates/sim/src/fem/study.rs` (SIM)
- **Contract:** `pub fn find_rigid_modes(ls: &LinearSystem) -> Result<Vec<RigidMode>, SimError>` and `pub fn check_constraints(mesh: &FemMesh, fixed: &FixedDofs) -> Result<ConstraintReport, SimError>` with `pub struct ConstraintReport { pub unconstrained_dofs: Vec<usize>, pub rigid_components: usize, pub sufficient: bool }`.
- **Behaviour:** Before solving, assert the model is actually constrained: every rigid-body motion must be suppressed (three translations and three rotations), and no dof may be free with no path to ground. **This runs before assembly and reports which node to fix**, which is the difference between "singular stiffness matrix at dof 812" and "node 271 is unrestrained in Y".
- **Refusals:** `SimError::UnconstrainedDof { dof, kind }` when a dof has an empty row. `SimError::Unsupported` when a connected component of the mesh has no fixed node at all.
- **Tests:** `a_missing_constraint_is_named` — the cantilever with one dof released names that dof. `a_properly_constrained_beam_reports_six_suppressed_modes` — asserts all six. `two_disconnected_bodies_each_need_restraint` — asserts two components. `a_model_with_a_mechanism_is_refused` — a loop of rods.
- **Acceptance:** Green, and it is the **first** thing `SIM-STATIC-022` calls.
- **Risk:** A false positive on a model with a legitimate mechanism (a roller). Fallback: `ConstraintReport` returns the full list and the caller decides; the default is to refuse, with the list printed so the user can whitelist.

### SIM-STATIC-021 — Self-weight study
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-003, SIM-STATIC-022
- **Files:** `crates/sim/src/fem/study.rs` (SIM)
- **Contract:** `pub fn run_self_weight(mesh: &FemMesh, material: &Material, fixed: &FixedDofs) -> Result<ResultSet, SimError>` and `pub fn run_study(s: &Study) -> Result<ResultSet, SimError>` with `pub enum Study { Static { model: Model, gravity: Option<Vec3> }, … }`.
- **Behaviour:** The first complete study: assemble, check constraints, solve, check energy, recover, wrap in a `ResultSet` with `Validation::Unverified` unless a benchmark ran. Gravity is a body force `ρ·g`, applied per element from its own volume.
- **Refusals:** All of the above propagate. `run_study` runs under `catch_unwind` (the `AGENTS.md` guard), and an escaped panic restores nothing — there is no drawing to restore here — and returns `SimError::SolverFailed("the analysis panicked")` with the panic message attached. **A guard exists because `AGENTS.md` requires one at every user-visible operation.**
- **Tests:** `a_hanging_bar_under_its_own_weight_is_unconstrained` — asserts `UnconstrainedDof`. `a_vertical_bar_stresses_under_its_own_weight` — asserts `σ = ρ·g·L` at the base to 1e-6. `a_panicked_analysis_is_an_error_not_a_crash` — a deliberately panicking path returns `Err`. `the_result_set_carries_unverified_provenance` — asserts `Validation::Unverified` until a benchmark runs.
- **Acceptance:** Green, and this is the first end-to-end user-visible study.
- **Risk:** The `catch_unwind` guard masking a real bug. Fallback: the panic message is attached to the `Err` and logged, never swallowed.

### SIM-STATIC-022 — Manufactured-solution test on a real mesh
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-021, SIM-SLV-012
- **Files:** `crates/sim/tests/static.rs` (SIM)
- **Contract:** `pub fn run_manufactured_study(seed: u64, solver: &dyn LinearSolver) -> Result<ManufacturedReport, SimError>` with `pub struct ManufacturedReport { pub displacement_error: f64, pub stress_error: f64 }`.
- **Behaviour:** Prescribe a smooth exact displacement field on a random mesh, derive the exact strain and stress from it analytically, and assert the solver recovers both. **This catches everything at once that the benchmarks catch separately** — assembly, dof mapping, boundary conditions — on a mesh no human designed.
- **Refusals:** A mesh that fails `SIM-MSH-006`'s quality floor → `SimError::MeshQuality`, recorded as a skipped case with the reason, **not** as a solver failure. This distinction is the whole value of the test.
- **Tests:** `a_manufactured_field_is_recovered_on_random_meshes` — over 20 seeds, asserting `stress_error` below the bar. `the_error_decreases_with_refinement` — over three levels. `a_deliberately_wrong_dof_map_fails_it` — proves the test has teeth.
- **Acceptance:** Green at 20 seeds, deterministic.
- **Risk:** Passing because the exact field happens to be in the element's own polynomial space. Fallback: the test uses a field of degree higher than any element (`sin` products), so only a correct method can match it exactly in the continuum limit, and the convergence in `SIM-VAL-016` confirms.

### SIM-STATIC-023 — The `Model` type
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-002, SIM-STATIC-003
- **Files:** `crates/sim/src/fem/model.rs` (new, SIM)
- **Contract:** `pub struct Model { pub mesh: FemMesh, pub material: Material, pub constraints: Vec<Constraint>, pub loads: Vec<Load> }` with `pub fn check(&self) -> Result<ModelReport, SimError>` and `pub struct ModelReport { pub empty_constraints: Vec<String>, pub unused_loads: Vec<String>, pub unbalanced: bool, pub node_count: usize, pub element_count: usize }`.
- **Behaviour:** The analysis definition, self-contained and serialisable, so a study can be saved and re-run. `check()` is the **user-facing diagnostic**: it names constraint sets and loads that matched nothing, which is the mistake that produces a silently wrong model. An unused load is not a warning — it is listed by name.
- **Refusals:** `SimError::Empty` for a model with no loads at all when `check()` is called with `require_loads` (default true), because a model with no loads has a zero solution and a user wants to know before they see it.
- **Tests:** `an_unmatched_constraint_set_is_named` — asserts the name appears in the report. `an_unused_load_is_named`. `a_model_serialises_and_reloads_identically` — round trip. `a_loadless_model_is_refused_by_check` — asserts `Err`.
- **Acceptance:** Green, and `check()` runs before every solve.
- **Risk:** Users suppressing the diagnostics. Fallback: `check()`'s output is in the report header regardless.

### SIM-STATIC-024 — The study report
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-019, SIM-MSH-024
- **Files:** `crates/sim/src/report.rs` (new, SIM)
- **Contract:** `pub fn render_report(r: &ResultSet, m: &MeshParams, mat: &Material) -> String` producing a text report containing, in this order: the **validation verdict first**, then mesh statistics and quality, then the material in SI, then the checks (residual, equilibrium, energy identity), then the solver, then the load summary, then the extreme values with their locations.
- **Behaviour:** Plain text, no dependencies, rendered identically to a terminal and to a file. **The verdict is the first line**, because a report that ends with a stress number and begins with a caveat gets read backwards. Extreme values are given **with the node or element that produced them**, because "max stress 412 MPa" without a location is not actionable.
- **Refusals:** Rendering a `ResultSet` whose `Validation` is `Unverified` **and** whose caller asked for an exportable report → `SimError::Unverified`. The on-screen render is allowed and is watermarked with `UNVERIFIED`.
- **Tests:** `the_report_starts_with_the_verdict` — asserts the first non-empty line. `the_report_contains_the_mesh_quality` — asserts the floor and the minimum appear. `the_report_names_the_location_of_the_extremes` — asserts a node index appears. `an_unverified_report_cannot_be_exported` — asserts `Err`, while the on-screen render succeeds with the watermark.
- **Acceptance:** Green, and every field the ADR needs to justify a solve is present.
- **Risk:** Report length burying the verdict. Fallback: the verdict is line 1 and the report is capped at one screen for the on-screen form.

### SIM-STATIC-025 — A stress-concentration study against a literature value
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-022, SIM-MSH-016
- **Files:** `crates/sim/tests/concentration.rs` (SIM)
- **Contract:** `pub struct ConcentrationCase { pub name: &'static str, pub geometry: &'static str, pub k_t: f64, pub source: &'static str }` and `pub fn cases() -> Vec<ConcentrationCase>`.
- **Behaviour:** A flat plate with a central hole under uniaxial tension: `K_t ≈ 3.0` at `r/D = 0.125`, rising with hole size. **This is not an analytical solution** — it is a published handbook value — and the task says so. It is in Tier A−: it checks that a stress *concentration* is found at the right place with roughly the right magnitude, which no analytical benchmark in this file tests. A `ConcentrationCase` carries its `source` as a `&'static str`, so the provenance of the number is in the type.
- **Refusals:** A case whose `k_t` is below the bar → the result is `Validation::Convergence`, never `Validation::Analytical`, because it is not analytical. **Overstating the provenance is a bug, not a shortcut.**
- **Tests:** `the_hole_concentration_is_found_at_the_hole` — asserts the max-stress element is adjacent to the hole boundary. `its_magnitude_is_within_20_percent_of_kt_3` — the handbook value with a stated tolerance. `the_concentration_rises_as_the_hole_grows` — monotonicity. `the_case_carries_its_source` — a type-level assertion via a test that every `ConcentrationCase` has a non-empty `source`.
- **Acceptance:** Green; the result is stamped `Convergence`, not `Analytical`.
- **Risk:** Treating a handbook value as analytical elsewhere. Fallback: the distinct `Validation` variant, and the report printing which benchmark class was used.

### SIM-STATIC-026 — Determinism and hostile-input behaviour of the static path
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-022
- **Files:** `crates/sim/tests/property.rs` (SIM)
- **Contract:** `pub fn run_static_property(seed: u64, cases: usize) -> Result<Vec<StaticCaseOutcome>, SimError>` with `pub struct StaticCaseOutcome { pub seed: u64, pub case: usize, pub outcome: Outcome }` and `pub enum Outcome { Solved, Refused(String) }`.
- **Behaviour:** For every random material, mesh, constraint set and load: either a solved result whose energy identity and equilibrium both pass, or a **named** refusal. Never a third outcome. Hostile cases — `youngs = 0`, `ν = 0.5`, `E = 1e300`, a mesh of 10⁶ elements, a force of `1e300` N — must all be refusals with a reason.
- **Refusals:** `Err` is an accepted outcome and is recorded with its message. `E = f64::MAX` must be refused by the sanity ceiling, not by producing `NaN` that propagates.
- **Tests:** `every_case_is_either_solved_or_refused` — the aggregate. `the_same_seed_gives_the_same_numbers` — byte-identical. `hostile_inputs_never_panic` — the enumerated list. `a_refusal_always_names_the_reason` — asserts the message is non-empty and mentions a field or a constraint.
- **Acceptance:** Green over the case set, deterministic, with the kernel's own refusals enumerated by name.
- **Risk:** A rare seed escaping the deterministic generator. Fallback: the seed is an explicit argument and the failing seed is in the assertion message, following `crates/solid/tests/property_surface.rs:31-35`.

---

## 9. `SIM-THERM` — Thermal (16 tasks)

### SIM-THERM-001 — `ThermalBC`: Dirichlet, Robin, adiabatic, radiation
- **State:** BACKLOG
- **Depends on:** SIM-MSH-009, SIM-THERM-003
- **Files:** `crates/sim/src/thermal/bc.rs` (new, SIM)
- **Contract:** `pub enum ThermalBC { Temperature { face_set: String, value: f64 }, Convection { face_set: String, h: f64, t_ambient: f64 }, Adiabatic { face_set: String }, Radiation { face_set: String, emissivity: f64, t_ambient: f64 } }` and `pub fn apply_bcs(ls: &mut LinearSystem, bcs: &[ThermalBC], mesh: &FemMesh, sets: &BTreeMap<String, Vec<u32>>) -> Result<(), SimError>`.
- **Behaviour:** Temperature in kelvin, `h` in W·m⁻²·K⁻¹, positive `h` meaning heat leaves the body when the surface is hotter than ambient (Newton's law, which is the same sign convention as §0.2's Fourier law). Radiation is linearised about a reference temperature and the linearisation error is recorded, **because silently treating radiation as linear is a real modelling error**.
- **Refusals:** `h < 0.0` → `SimError::BadDistance`. `emissivity < 0.0 || > 1.0` → `SimError::Unsupported`. A temperature below absolute zero → `SimError::BadDistance`. An adiabatic set that is not the whole boundary → a **warning in the report, not an error**: an adiabatic boundary in the middle of a model is legal and often intended.
- **Tests:** `a_dirichlet_face_is_eliminated_like_a_structural_fix` — asserts the dof is gone from the matrix. `a_convection_load_integrates_to_h_times_area_times_delta_T` — the analogue of `SIM-STATIC-003`'s pressure test. `radiation_is_linearised_about_the_stated_reference` — asserts the reported error term. `a_negative_h_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** Sign of `h`. Fallback: the integration test asserts `h > 0` with `T_surface > T_ambient` gives heat **leaving** the body.

### SIM-THERM-002 — Heat source model
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-003
- **Files:** `crates/sim/src/thermal/source.rs` (new, SIM)
- **Contract:** `pub enum HeatSource { Uniform { watts: f64 }, Density { watts_per_m3: f64 }, Surface { face_set: String, watts_per_m2: f64 }, Point { at: Vec3, watts: f64, radius: f64 } }` and `pub fn source_vector(mesh: &FemMesh, sources: &[HeatSource], sets: &BTreeMap<String, Vec<u32>>) -> Result<Vec<f64>, SimError>`.
- **Behaviour:** Watts, W·m⁻³ or W·m⁻². A point source is smeared over a ball of radius `r` so the nodal vector stays finite; **the smeared power is exactly `watts`**, asserted by a test, because a smeared source that loses power is a silent energy leak.
- **Refusals:** Negative watts → `SimError::BadDistance`. A `Point` with `radius <= 0.0` → `SimError::BadDistance`. An unknown face set → `SimError::Empty`.
- **Tests:** `the_assembled_heat_rate_equals_the_stated_watts` — exact, for every variant. `a_smeared_point_source_conserves_its_power` — to 1e-9. `a_uniform_source_matches_its_density_times_volume` — to 1e-9.
- **Acceptance:** Green.
- **Risk:** A smeared source whose ball falls outside the solid losing power. Fallback: the conservation test asserts the assembled rate is the stated watts, which fails if any is lost.

### SIM-THERM-003 — Element conduction matrix
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-005, SIM-STATIC-006
- **Files:** `crates/sim/src/thermal/element.rs` (new, SIM)
- **Contract:** `pub fn conductivity(kind: ElementKind, nodes: &[Vec3], k: f64) -> Result<ElementMatrix, SimError>` and `pub fn conductivity_anisotropic(kind: ElementKind, nodes: &[Vec3], k: [f64; 3]) -> Result<ElementMatrix, SimError>`.
- **Behaviour:** `K_e = ∫ B_kᵀ D_k B_k det(J) dξ` with the gradient of the temperature as the gradient of a scalar — the **same `B` machinery as `SIM-STATIC-007`, with a 1-component instead of 6-component strain**. That is the reuse: the shape functions, the Jacobian and the quadrature table are shared, only `D` and `B` differ. The anisotropic variant needs a rotation of `k` into the element's local frame, which is a real task and is stated.
- **Refusals:** `k <= 0.0` → `SimError::BadDistance`. An anisotropic `k` with a negative or zero eigenvalue → `SimError::Unsupported("the conductivity tensor must be positive definite")`. Inverted element → `SimError::InvertedElement`.
- **Tests:** `conductivity_matches_the_steady_slab` — `SIM-VAL-011` end to end. `conductivity_is_symmetric`. `an_isotropic_run_gives_the_same_answer_as_a_diagonal_tensor` — the three-diagonal case equals the scalar. `a_negative_conductivity_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** Getting the `B_k` transpose or the Voigt ordering wrong. Fallback: `SIM-VAL-011`'s slab profile test fails immediately if the sign is wrong, which is the most sensitive test in the thermal epic.

### SIM-THERM-004 — Steady assembly and solve
- **State:** BACKLOG
- **Depends on:** SIM-THERM-001, SIM-THERM-002, SIM-THERM-003, SIM-SLV-007
- **Files:** `crates/sim/src/thermal/solve.rs` (new, SIM)
- **Contract:** `pub fn solve_steady(mesh: &FemMesh, k: f64, sources: &[HeatSource], bcs: &[ThermalBC], sets: &BTreeMap<String, Vec<u32>>) -> Result<ThermalResult, SimError>` with `pub struct ThermalResult { pub temperature: Vec<f64>, pub heat_flux: Vec<f64>, pub heat_rate: f64, pub residual: f64 }`.
- **Behaviour:** Assemble `K·T = Q`, solve, recover the nodal flux `q = −k∇T` and the total heat rate `∮q·dA`. **The steady thermal system is symmetric positive definite**, so it reuses the same factorisation as `SIM-STATIC` with no change — this is the cheapest reuse in the plan.
- **Refusals:** An all-adibatic model with no source → `SimError::Empty("a steady thermal model needs at least one Dirichlet condition or one source")`. A model with two Dirichlet conditions and no source → the uniform solution, which is legal. An inconsistent pair (two Dirichlet faces at different temperatures with a perfectly insulated body) → the solve works and the check reports the flux.
- **Tests:** `the_slab_profile_is_exactly_linear` — `SIM-VAL-011`. `the_flux_is_uniform_across_the_slab` — `SIM-VAL-011`'s second test. `a_model_with_no_sink_is_refused` — asserts `Err`. `the_heat_rate_out_equals_the_heat_rate_in` — `SIM-THERM-016`.
- **Acceptance:** Green; `SIM-VAL-011` is its benchmark and must pass before this task is done.
- **Risk:** An underdetermined steady model producing a singular matrix. Fallback: `SIM-SLV-008` names the unconstrained dof.

### SIM-THERM-005 — Lumped capacitance against the exact exponential
- **State:** BACKLOG
- **Depends on:** SIM-THERM-004, SIM-VAL-012
- **Files:** `crates/sim/src/thermal/transient.rs` (new, SIM)
- **Contract:** `pub fn lumped_temperature(t: f64, rho: f64, cp: f64, volume: f64, area: f64, h: f64, t0: f64, t_inf: f64) -> Result<f64, SimError>` and `pub fn solve_lumped(case: &LumpedCase, steps: usize) -> Result<LumpedResult, SimError>`.
- **Behaviour:** The first transient, verified against `SIM-VAL-012`'s closed form. It is also **the only thermal model in this plan that can be trusted for a part whose Biot number is small**, and the Biot number is computed and printed every time, because that is the condition under which the answer is valid and a user who does not know that number will happily apply a lumped model to a thick part and get a wrong answer.
- **Refusals:** `Bi > 0.1` → `Err(SimError::Unsupported("a lumped model needs Bi < 0.1; use SIM-THERM-007")` with the computed `Bi` in the message. Zero density, `cp` or volume → `SimError::BadDistance`. A lumped result with `Bi ≥ 0.1` **never returns a number**, because a confidently wrong transient temperature is a cold-start failure in an assembly.
- **Tests:** `the_lumped_exponential_matches_the_analytic_form` — `SIM-VAL-012`. `a_high_biot_number_is_refused_with_the_number` — asserts the message contains the `Bi`. `energy_is_conserved_over_the_steps` — asserts `ρ·V·cp·ΔT` equals the integrated `h·A·(T − T_inf)`. `a_long_time_reaches_ambient_without_overflow` — `SIM-VAL-012`'s overflow case.
- **Acceptance:** Green.
- **Risk:** Users applying it outside its regime. Fallback: the refusal is automatic and names the number.

### SIM-THERM-006 — Backward Euler transient
- **State:** BACKLOG
- **Depends on:** SIM-THERM-005, SIM-SLV-007
- **Files:** `crates/sim/src/thermal/transient.rs` (SIM)
- **Contract:** `pub fn solve_transient(mesh: &FemMesh, k: f64, rho_cp: f64, sources: &[HeatSource], bcs: &[ThermalBC], sets: &BTreeMap<String, Vec<u32>>, dt: f64, steps: usize) -> Result<TransientResult, SimError>` with `pub struct TransientResult { pub times: Vec<f64>, pub temperature: Vec<Vec<f64>>, pub scheme: &'static str }`.
- **Behaviour:** `(C/Δt + K)T^{n+1} = C/Δt·T^n + Q`. Unconditionally stable, first order, **no oscillations** — which is why it is the default. Every study prints its scheme and time step; a transient result without them is not comparable to another transient result.
- **Refusals:** `dt <= 0.0` or `steps == 0` → `SimError::BadTolerance`. `rho_cp <= 0.0` → `SimError::BadDistance`. A step count past the memory cap (`steps × nodes` entries) → `SimError::LimitExceeded`, naming both.
- **Tests:** `the_decay_matches_the_exact_series` — `SIM-VAL-012`, within the bar. `the_solution_is_monotone_for_a_cooling_body` — backward Euler never overshoots, and the test asserts it never does. `the_scheme_reduces_to_the_steady_solution_at_large_time` — asserts `T(t→∞) == T_steady` to 1e-6. `a_zero_step_is_refused` — asserts `Err`.
- **Acceptance:** Green, and it is the transient default.
- **Risk:** Storing every step's field blows memory on a long transient. Fallback: `LimitExceeded` names the cap; a `store_every: usize` parameter (default 1) is in the contract's `TransientResult` construction and is the user's lever.

### SIM-THERM-007 — Crank–Nicolson and the oscillation check
- **State:** BACKLOG
- **Depends on:** SIM-THERM-006
- **Files:** `crates/sim/src/thermal/transient.rs` (SIM)
- **Contract:** `pub fn solve_crank_nicolson(…same…, theta: f64) -> Result<TransientResult, SimError>` and `pub fn detect_oscillation(t: &[f64]) -> Option<usize>` returning the index of the first non-monotone reversal for a cooling problem.
- **Behaviour:** Second order, unconditionally stable, and **conditionally non-oscillatory**. The oscillation check is not optional: CN on a stiff problem with too large a step produces a temperature that undershoots absolute zero or overshoots the initial value by 10 %, and a user sees a plausible-looking curve. `detect_oscillation` is therefore part of the solver, not a post-processing nicety, and CN **refuses** to return a result whose history oscillates when the underlying problem is monotone.
- **Refusals:** `theta <= 0.0 || >= 1.0` → `SimError::BadTolerance`. An oscillating result on a problem known to be monotone → `SimError::SolverFailed("Crank-Nicolson oscillated at step {i}; use backward Euler or reduce the step")`.
- **Tests:** `cn_is_second_order` — halves the step, the error drops by ~4. `cn_does_not_oscillate_on_a_stiff_problem` — the Rayleigh quotient of the amplification matrix ≥ 0. `a_oscillating_run_is_refused_not_plotted` — asserts `Err`. `cn_matches_backward_euler_at_large_time` — both converge to the steady solution.
- **Acceptance:** Green, and it is available but never the default.
- **Risk:** Being the default because it is more accurate. Fallback: backward Euler is the default in `SIM-THERM-006` and the report names the scheme used.

### SIM-THERM-008 — Time step selection
- **State:** BACKLOG
- **Depends on:** SIM-THERM-006
- **Files:** `crates/sim/src/thermal/timestep.rs` (new, SIM)
- **Contract:** `pub fn stable_step(mesh: &FemMesh, k: f64, rho_cp: f64) -> Result<f64, SimError>` (the largest eigenvalue bound via the Gershgorin estimate of `M⁻¹K`) and `pub fn accuracy_step(mesh: &FemMesh, k: f64, rho_cp: f64, feature_size: f64) -> Result<f64, SimError>`.
- **Behaviour:** Two numbers, both reported: the **stability** limit (`Δt ≤ 2/λ_max`, which for backward Euler is infinite but is what a user needs for Crank–Nicolson or an explicit scheme) and the **accuracy** limit (`Δt ≤ feature_size²/(α)`, the Fourier number criterion). The default step is the accuracy step, and the report says how many steps lie between it and the stability step, because a user who takes 10⁵ steps is usually making a mistake and a user who takes 10 is usually getting a wrong answer.
- **Refusals:** A mesh with no elements → `SimError::Empty`. A non-finite estimate → `SimError::NonFinite`. A step below `1e-12·s` → `SimError::LimitExceeded`, because a transient that cannot be integrated in a bounded number of steps is a setup error, not a physics result.
- **Tests:** `the_stability_step_bounds_the_modal_response` — asserts an explicit mode's growth factor ≤ 1. `the_accuracy_step_resolves_a_known_penetration_depth` — `SIM-VAL-012`'s series. `a_meshes_stability_step_shrinks_as_h_doubles` — the `h²` law. `an_infinite_step_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** The Gershgorin estimate is conservative by a large factor and leads to absurdly small steps. Fallback: it is reported as a bound, not used directly; the accuracy step is the default.

### SIM-THERM-009 — Temperature-dependent material properties
- **State:** BACKLOG
- **Depends on:** SIM-THERM-004
- **Files:** `crates/sim/src/thermal/iter.rs` (new, SIM)
- **Contract:** `pub type PropertyFn = fn(f64) -> f64;` and `pub fn solve_picard(mesh: &FemMesh, k_of_t: PropertyFn, …, max_passes: usize, tol: f64) -> Result<PicardResult, SimError>` with `pub struct PicardResult { pub temperature: Vec<f64>, pub passes: usize, pub residual: f64 }`.
- **Behaviour:** `k(T)` makes the system nonlinear; iterate by Picard, freezing `k` at the previous iterate's temperature each pass. The convergence criterion is on the temperature norm, and **a non-converging Picard loop is an `Err`, not a fixed iteration count** — returning an unconverged thermal field as a result is exactly the failure §0.5 describes.
- **Refusals:** `max_passes` exhausted → `SimError::SolverFailed` with the residual history. `k_of_t` returning a non-positive or non-finite value at any evaluated temperature → `SimError::BadDistance`, **including at temperatures below absolute zero**, which is the sign that the user's property function is wrong.
- **Tests:** `a_constant_property_converges_in_one_pass` — to 1e-12. `a_mildly_temperature_dependent_property_converges` — asserts the pass count. `a_non_converging_loop_is_an_error` — asserts `Err` with a history. `a_property_that_goes_non_positive_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** Oscillating Picard loops (which happen when `k` varies strongly). Fallback: `SimError::SolverFailed` names the oscillation; under-relaxation is a follow-on task, not a hidden default.

### SIM-THERM-010 — Orthotropic and anisotropic conductivity
- **State:** BACKLOG
- **Depends on:** SIM-THERM-003
- **Files:** `crates/sim/src/thermal/element.rs` (SIM)
- **Contract:** `pub struct Conductivity { pub axes: [Vec3; 3], pub values: [f64; 3] }` with `pub fn to_local(&self, element_frame: [Vec3; 3]) -> Result<[[f64; 3]; 3], SimError>` and `pub fn solve_orthotropic(…, k: &Conductivity) -> Result<ThermalResult, SimError>`.
- **Behaviour:** Unidirectional composites. The tensor is rotated into each element's local frame so a single element's contribution is `B_kᵀ R D Rᵀ B_k`. **Honesty obligation:** the kernel has no composite layup model, no ply thickness and no stacking sequence (`master-plan.md` §4: "NURBS / analytic surfaces: none"), so this task handles a **homogeneous orthotropic material only**. A layup is `SimError::Unsupported`.
- **Refusals:** A `Conductivity` whose axes are not orthonormal → `SimError::Unsupported`. Non-positive-definite `values` → `SimError::Unsupported`. Any layup request → `SimError::Unsupported("composite layups are not implemented; see SIM-NL-001")`.
- **Tests:** `a_diagonal_conductivity_matches_the_scalar_case` — the isotropic limit. `a_strongly_orthotropic_bar_has_the_right_flux_ratio` — asserts the through-thickness vs in-plane flux ratio equals the conductivity ratio. `a_non_orthonormal_frame_is_refused` — asserts `Err`. `a_layup_request_is_refused` — asserts `Err`, naming `SIM-NL-001`.
- **Acceptance:** Green.
- **Risk:** Being read as composite support. Fallback: the refusal is in the type's doc comment and in the test.

### SIM-THERM-011 — Energy balance check
- **State:** BACKLOG
- **Depends on:** SIM-THERM-004, SIM-THERM-006
- **Files:** `crates/sim/src/thermal/check.rs` (new, SIM)
- **Contract:** `pub fn check_energy_balance(mesh: &FemMesh, r: &ThermalResult, sources: &[HeatSource], bcs: &[ThermalBC]) -> Result<EnergyReport, SimError>` with `pub struct EnergyReport { pub generated: f64, pub conducted_out: f64, pub stored: f64, pub residual_rel: f64, pub passed: bool }`.
- **Behaviour:** In steady state, `generated == conducted_out`. In transient, `generated·Δt == conducted_out·Δt + Δ(stored)`. The thermal analogue of `SIM-STATIC-017`'s energy identity and, like it, the cheapest catcher of a whole class of bugs. Every quantity is in watts.
- **Refusals:** `SimError::Empty` when no source exists (the balance is then trivially zero and says nothing). A relative residual above `1e-6` → `passed = false` and an `Err` when the check is requested explicitly.
- **Tests:** `a_steady_slab_balances_to_1e_12` — the exact slab. `a_transient_slab_stores_the_expected_energy` — matches `ρ·V·cp·ΔT`. `a_missing_sink_shows_a_growing_stored_energy` — asserts the check catches it.
- **Acceptance:** Green; it runs on every thermal study.
- **Risk:** A quadrature error in the surface integral dominating the residual. Fallback: the residual is reported relative and the surface integral uses the same quadrature as everything else.

### SIM-THERM-012 — One-way thermal–structural coupling
- **State:** BACKLOG
- **Depends on:** SIM-THERM-004, SIM-STATIC-019
- **Files:** `crates/sim/src/thermal/couple.rs` (new, SIM)
- **Contract:** `pub fn thermal_strain(mesh: &FemMesh, t: &[f64], alpha: f64, t_ref: f64) -> Result<Vec<[f64; 6]>, SimError>` and `pub fn solve_coupled_steady(m: &Model, thermal: &ThermalResult, alpha: f64, t_ref: f64) -> Result<ResultSet, SimError>`.
- **Behaviour:** **One-way only**: solve the thermal problem, compute `ε = α(T − T_ref)`, add it to the mechanical strain as an eigenstrain, solve the mechanical problem. The mechanical stiffness matrix is **unchanged**; only the load vector changes, which means it reuses `SIM-SLV-005`'s existing factorisation structure exactly. `t_ref` is explicit and stored (§0.2).
- **Refusals:** A missing `t_ref` → `SimError::Unsupported("a thermal strain needs an explicit reference temperature")`. A temperature field of the wrong length → `SimError::InvalidIndex`. **Two-way coupling (thermal properties depending on stress) is `SimError::Unsupported`** — it is a Picard loop with a stress-dependent `k`, which is `SIM-THERM-009` extended, and it is Tier B.
- **Tests:** `an_unconstrained_part_expands_freely` — a fully constrained part in a uniform gradient... rather: an unconstrained bar under uniform heating has **zero** stress. `a_restrained_part_carries_the_thermal_stress` — asserts `σ = −E·α·ΔT/(1−ν)` on a fully restrained bar, which is an exact analytical value. `a_zero_reference_temperature_gives_zero_strain` — asserts nothing. `two_way_coupling_is_refused` — asserts `Err`, naming the tier.
- **Acceptance:** Green, and the restrained-bar analytical value is a new benchmark worth adding to `SIM-VAL`.
- **Risk:** The sign of the eigenstrain, which is easy to get backwards and produces compression where tension belongs. Fallback: the restrained-bar test is exact and the sign is its whole content.

### SIM-THERM-013 — Contact conductance: refused
- **State:** BACKLOG
- **Depends on:** SIM-NL-001
- **Files:** `crates/sim/src/thermal/mod.rs` (SIM)
- **Contract:** `pub fn contact_conductance(model: &ContactModel) -> Result<f64, SimError>` which returns `SimError::Unsupported` unconditionally.
- **Behaviour:** A task whose deliverable is the refusal, its message and its place in the documentation. Contact conductance depends on contact pressure, which depends on contact mechanics, which is Tier C. A thermal solver that quietly uses a constant `h_c` is a model with an undocumented assumption.
- **Refusals:** The task. Every call is `Err(Unsupported("contact conductance requires contact mechanics; see SIM-NL-010")`.
- **Tests:** `contact_conductance_is_unsupported` — asserts the message names `SIM-NL-010`. `no_default_contact_conductance_exists` — a compile-level assertion: the function is the only entry point and it cannot return `Ok`.
- **Acceptance:** Green in the trivial sense that the refusal is tested and the message is good.
- **Risk:** Someone adding a constant `h_c` to make a demo work. Fallback: the refusal test fails if the signature ever gains a success path, and the doc comment says why.

### SIM-THERM-014 — Radiation view factors: scoped out
- **State:** BACKLOG
- **Depends on:** SIM-THERM-001
- **Files:** `crates/sim/src/thermal/mod.rs` (SIM)
- **Contract:** `pub enum RadiationModel { SurfaceToAmbient { emissivity: f64, t_ambient: f64 }, SurfaceToSurface { … } }` where the second variant is `#[doc(hidden)]` and its constructor returns `SimError::Unsupported`.
- **Behaviour:** Linearised surface-to-ambient radiation is implemented (in `SIM-THERM-001`). Surface-to-surface requires view factors between arbitrary faces of a polyhedron — a separate numerical method with its own verification (a closed cavity must conserve energy, which is the test), and it is out of scope for this plan. The decision is recorded rather than left implicit.
- **Refusals:** `SimError::Unsupported("surface-to-surface radiation requires view factors, which are not implemented")`.
- **Tests:** `surface_to_ambient_is_available` — asserts `Ok`. `surface_to_surface_is_unsupported` — asserts `Err`. `a_linearised_radiation_term_reports_its_error` — asserts the reported linearisation error at a stated reference temperature.
- **Acceptance:** The scope line is written down, and it is in the ADR.
- **Risk:** A user reading "radiation supported" and applying it between two parts. Fallback: the enum variant name is `SurfaceToAmbient`, not `Radiation`.

### SIM-THERM-015 — Thermal results as fields for the viewport
- **State:** BACKLOG
- **Depends on:** SIM-THERM-004, SIM-VIZ-001
- **Files:** `crates/sim/src/results.rs` (SIM)
- **Contract:** `pub fn thermal_result_set(mesh_fingerprint: u64, t: ThermalResult, validation: Validation) -> ResultSet` and `pub fn isothermals(mesh: &FemMesh, t: &[f64], level: f64) -> Result<Vec<Vec<u32>>, SimError>`.
- **Behaviour:** Wrap a thermal solve in the §0.5 provenance machinery, and provide marching-triangles isotherm extraction so the viewport can draw a temperature contour on the model surface. The isotherm is a **line on the surface**, not a cross-section through the volume, and the doc comment says so.
- **Refusals:** A temperature field of the wrong length → `SimError::InvalidIndex`. An isotherm on a field with no crossing → `Ok` with an empty list, **not** an error.
- **Tests:** `a_linear_field_gives_parallel_isotherms` — on a slab. `a_result_set_carries_the_thermal_schema_version` — asserts the round trip. `an_unverified_thermal_result_is_not_exportable` — the §0.5 rule.
- **Acceptance:** Green; consumed by `SIM-VIZ-005`.
- **Risk:** None material.

### SIM-THERM-016 — Thermal property tests
- **State:** BACKLOG
- **Depends on:** SIM-THERM-011, SIM-THERM-006
- **Files:** `crates/sim/tests/thermal.rs` (SIM)
- **Contract:** `pub fn run_thermal_property(seed: u64, cases: usize) -> Result<Vec<ThermalCaseOutcome>, SimError>` with `pub enum ThermalCaseOutcome { Balanced, Refused(String) }`.
- **Behaviour:** Random meshes, materials, sources and boundary conditions. Each case either balances energy to `1e-6` or is refused with a name. Hostile cases: `k = 0`, `ρ·c = 0`, `h` negative, `Δt = 0`, a source of `1e300` W, a model with no Dirichlet condition.
- **Refusals:** Every hostile case must be a named refusal, never a `NaN` field and never an overflow.
- **Tests:** `every_case_balances_or_is_refused` — the aggregate. `the_same_seed_gives_the_same_numbers`. `hostile_inputs_never_panic` — the enumerated list. `a_refusal_names_the_parameter` — asserts the message mentions the offending field.
- **Acceptance:** Green and deterministic.
- **Risk:** The same as `SIM-STATIC-026`.

---

## 10. `SIM-NL` — Nonlinearity, contact, plasticity (14 tasks)

### SIM-NL-001 — The refusal policy, in code
- **State:** READY
- **Depends on:** none
- **Files:** `crates/sim/src/nl/mod.rs` (new, SIM)
- **Contract:** `pub fn unsupported(what: &str) -> SimError` and a `pub const UNSUPPORTED_PHYSICS: &[(&str, &str)]` listing every Tier C capability with the task that would implement it and the reason it is absent.
- **Behaviour:** The single place that decides what this project refuses to approximate. Every entry: contact (→ `SIM-NL-010..012`), plasticity (→ `SIM-NL-013`), creep, fatigue, buckling (→ `SIM-NL-014`), composites, fracture, impact/explicit, fluid coupling, thermal contact (→ `SIM-THERM-013`). The `Display` message names **what is missing, not what is wrong with your input**, because a user asking for contact deserves an answer about contact.
- **Refusals:** The task's own output is the refusal. `unsupported("contact")` is infallible and always right.
- **Tests:** `every_unsupported_physics_entry_names_a_task` — asserts the reference is non-empty. `the_message_is_not_a_scolding` — asserts no message contains "invalid", "error" or "wrong". `the_list_is_referenced_from_the_ui` — a grep-level assertion that the UI's help text points here.
- **Acceptance:** Exists, and it is cited in `docs/geometry-parity.md` and in the UI's study dialog.
- **Risk:** The list reads as a roadmap promise. Fallback: each entry names a task ID **and** says whether that task is a spike or an implementation, so a reader does not mistake `SIM-NL-013` for committed work.

### SIM-NL-002 — Newton loop with convergence measures
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-010, SIM-SLV-007
- **Files:** `crates/sim/src/nl/newton.rs` (new, SIM)
- **Contract:** `pub struct Convergence { pub residual: f64, pub energy: f64, pub displacement: f64 }` and `pub fn newton<F>(residual: F, u0: &[f64], tol: &NonlinearTol, max_iters: usize) -> Result<NewtonResult, SimError>` with `pub struct NewtonTol { pub residual: f64, pub energy: f64, pub displacement: f64 }` and `pub struct NewtonResult { pub u: Vec<f64>, pub iterations: usize, pub history: Vec<Convergence> }`.
- **Behaviour:** The three classical measures, all three required to be satisfied — **residual alone is not convergence**, because a badly scaled system can have a small residual and a large error. Convergence requires `‖Δu‖` small *relative to ‖u‖*, an energy change below tolerance and a residual below tolerance. The history is kept and reported.
- **Refusals:** `max_iters` exhausted → `SimError::SolverFailed` with the full history. Divergence (`NaN` in `u`) → `SimError::NonFinite` immediately, not after more iterations. **A diverged Newton loop never returns its last iterate** — the last iterate of a diverging solve is a plausible-looking wrong answer.
- **Tests:** `a_linear_problem_converges_in_one_iteration` — to 1e-12. `a_nonlinear_problem_converges_quadratically` — asserts the error ratio squares each iteration near the solution. `all_three_measures_must_pass` — a synthetic case where the residual is small but the displacement is not, asserting non-convergence. `a_diverging_loop_returns_an_error_not_an_iterate` — asserts `Err`.
- **Acceptance:** Green; used by every task in this epic.
- **Risk:** Residual-only convergence in a later implementation. Fallback: `NonlinearTol` has three fields and `newton` requires all three; a single-tolerance constructor does not exist.

### SIM-NL-003 — Line search and load stepping
- **State:** BACKLOG
- **Depends on:** SIM-NL-002
- **Files:** `crates/sim/src/nl/newton.rs` (SIM)
- **Contract:** `pub fn line_search<F>(… ) -> Result<(f64, f64), SimError>` returning `(α, merit)` and `pub fn load_step(increment: f64, min: f64, previous_attempts: usize) -> Result<f64, SimError>`.
- **Behaviour:** A backtracking line search on the energy merit function, and an automatic increment cut-back (halve on failure, bounded by `min`). Together they turn "this model does not converge" into "this model converges in 40 steps of 1/80 load", which is the difference between a usable nonlinear feature and an unusable one.
- **Refusals:** An increment below `min` → `SimError::SolverFailed("the load increment fell to {min} at pass {n}")`. A line search that cannot find a descent direction after 20 halvings → `SimError::SolverFailed`.
- **Tests:** `line_search_reduces_the_merit` — asserts monotone decrease. `load_step_halves_on_failure`. `an_increment_below_the_minimum_is_an_error` — asserts `Err`. `a_linear_problem_takes_the_full_step_first` — asserts `α == 1.0`.
- **Acceptance:** Green.
- **Risk:** A line search that accepts a step with a smaller residual but a larger energy. Fallback: the merit function is the energy, not the residual; the test asserts the merit decreases.

### SIM-NL-004 — Total Lagrangian kinematics
- **State:** BACKLOG
- **Depends on:** SIM-NL-002, SIM-MSH-002
- **Files:** `crates/sim/src/nl/kinematics.rs` (new, SIM)
- **Contract:** `pub struct DeformedGeometry { pub nodes: Vec<Vec3> }` with `pub fn update(geo: &DeformedGeometry, u: &[f64]) -> Result<DeformedGeometry, SimError>` and `pub fn corotate(n0: Vec3, n: Vec3, rotation: Mat3) -> Mat3`.
- **Behaviour:** The mesh moves; the connectivity does not. `u` is measured from the deformed configuration. **Integration is about each element's own deformed centroid** — the §1.3 rule again, and now it matters more, because a deformed element at 1e6 units has the same cancellation problem as an undeformed one.
- **Refusals:** A `u` that would invert an element (negative determinant) → `SimError::InvertedElement { element, jacobian }`, which is a *correct* refusal: an inverted element is physically meaningless and its correct answer is "the load step was too large".
- **Tests:** `a_pure_rigid_rotation_produces_no_strain` — the frame-indifference test, the single most important one in this task. `a_translation_changes_nothing` — asserts the same. `an_inverting_displacement_is_refused` — asserts `Err`. `integration_is_stable_far_from_the_origin` — the `mass.rs` lesson, at 1e6, asserting 1e-6.
- **Acceptance:** Green.
- **Risk:** Frame dependence: a result that changes under a rigid rotation of the whole model is a bug that looks plausible. Fallback: the pure-rotation test is mandatory and is quoted by every task in this epic.

### SIM-NL-005 — Corotational formulation
- **State:** BACKLOG
- **Depends on:** SIM-NL-004
- **Files:** `crates/sim/src/nl/kinematics.rs` (SIM)
- **Contract:** `pub fn corotational_stiffness(kind: ElementKind, before: &[Vec3], after: &[Vec3], d: &[[f64; 6]; 6]) -> Result<ElementMatrix, SimError>`.
- **Behaviour:** Moderate-rotation elements: separate the element's rigid rotation from its deformation and apply the constitutive law to the corotated part. Much better behaved than a total-Lagrangian formulation at moderate rotation, at some extra implementation cost.
- **Refusals:** An element whose rotation cannot be extracted (degenerate geometry) → `SimError::InvertedElement`.
- **Tests:** `corotational_and_total_lagrangian_agree_at_small_strain` — asserts agreement within the bar at `ε = 1e-4`. `a_large_rigid_rotation_gives_zero_strain` — 45°, asserting `1e-10`. `a_bending_case_is_strain_driven` — a cantilever rotated 30° keeps its linear stress gradient.
- **Acceptance:** Green.
- **Risk:** Two formulations to keep consistent. Fallback: the small-strain agreement test is the coupling between them.

### SIM-NL-006 — Hyperelasticity: neo-Hookean
- **State:** BACKLOG
- **Depends on:** SIM-NL-004
- **Files:** `crates/sim/src/nl/material.rs` (new, SIM)
- **Contract:** `pub enum Hyperelastic { StVK { youngs: f64, poisson: f64 }, NeoHookean { mu: f64, lambda: f64 } }` with `pub fn cauchy_stress(f: &DeformationGradient) -> Result<[[f64; 3]; 3], SimError>` and `pub fn consistent_tangent(f: &DeformationGradient) -> Result<[[[f64; 3]; 3]; 3], SimError>`.
- **Behaviour:** Objective stress update: the Cauchy stress is computed from the deformation gradient, and the tangent is **consistent** with it — an inconsistent tangent converges, just slower, which is a performance problem, but a tangent that is inconsistent because of a derivation error converges to the wrong place or oscillates, which is a correctness problem. The consistent tangent is derived analytically and validated against a finite-difference Jacobian.
- **Refusals:** `F` with a non-finite or non-positive determinant → `SimError::InvertedElement`. `poisson >= 0.5` → `SimError::Unsupported`, naming `SIM-VAL-014` and the mixed-formulation need.
- **Tests:** `neo_hookean_reduces_to_neo_hookean_in_the_small_strain_limit` — asserts the initial stress tensor matches `E`/`ν` to 1e-8. `the_consistent_tangent_matches_finite_differences` — central differences with `h = 1e-6`, to 1e-5. `a_rotation_leaves_the_stress_unchanged` — frame indifference on the stress, not just the strain. `a_non_positive_determinant_is_refused` — asserts `Err`.
- **Acceptance:** Green; the small-strain limit is validated against `SIM-STATIC-001`.
- **Risk:** The tangent derivation. Fallback: the finite-difference test is the proof, and an inconsistent tangent (computed numerically once and hard-coded) is an acceptable intermediate step.

### SIM-NL-007 — St. Venant–Kirchhoff as the small-strain limit
- **State:** BACKLOG
- **Depends on:** SIM-NL-006
- **Files:** `crates/sim/src/nl/material.rs` (SIM)
- **Contract:** `pub fn stvk_stress(f: &DeformationGradient, youngs: f64, poisson: f64) -> Result<[[f64; 3]; 3], SimError>`.
- **Behaviour:** The simplest hyperelastic model, and the bridge between the linear and nonlinear implementations. `SIM-VAL-009`'s patch test must pass with it, which proves the two code paths agree where they must.
- **Refusals:** As `SIM-NL-006`.
- **Tests:** `stvk_matches_the_linear_elastic_stress_at_small_strain` — against `SIM-STATIC-012` on the same displacement field, to 1e-8. `the_patch_test_passes_with_stvk` — `SIM-VAL-009` re-run through the nonlinear path.
- **Acceptance:** Green.
- **Risk:** None.

### SIM-NL-008 — Large-deformation study
- **State:** BACKLOG
- **Depends on:** SIM-NL-005, SIM-NL-007, SIM-NL-002, SIM-NL-003
- **Files:** `crates/sim/src/nl/study.rs` (new, SIM)
- **Contract:** `pub fn run_large_deformation(m: &Model, mat: Hyperelastic, max_load: f64, tol: &NonlinearTol) -> Result<ResultSet, SimError>` and `pub fn equilibrium_path(m: &Model, mat: Hyperelastic, increments: usize) -> Result<PathResult, SimError>` with `pub struct PathResult { pub states: Vec<StaticResult>, pub converged_at: Option<f64>, pub limit_point: Option<f64> }`.
- **Behaviour:** Load in increments with automatic cut-back; each converged increment is a `StaticResult`. **`limit_point` is reported honestly**: if the load control cannot pass a limit point (snap-through), the run stops and says so, naming the load it reached, rather than continuing with a wrong branch.
- **Refusals:** `SimError::SolverFailed` with the load history when the path terminates. `max_load <= 0.0` → `SimError::BadDistance`. **Every result is `Validation::Unverified` unless a benchmark in `SIM-VAL` covers it** — and no benchmark in this file covers a large-deformation case, so **every large-deformation result is `Unverified`**, by construction. That is the honest state of the art here and the stamp says so.
- **Tests:** `a_small_rotation_case_agrees_with_linear` — asserts the two agree within the bar at a load that produces 1 % strain. `a_non_converging_increment_is_an_error` — asserts `Err` with the load history. `the_results_are_unverified` — asserts `Validation::Unverified`, **which is the test that keeps the honesty rule true**. `a_limit_point_is_reported_not_stepped_over` — a snap-through bracket.
- **Acceptance:** Green, and the `Unverified` stamp is asserted.
- **Risk:** A nonlinear result looking better than a linear one and getting more trust. Fallback: the watermark and the report's verdict line.

### SIM-NL-009 — Buckling: linear eigenvalue around a pre-stressed state
- **State:** BACKLOG
- **Depends on:** SIM-SLV-010, SIM-STATIC-019
- **Files:** `crates/sim/src/nl/buckling.rs` (new, SIM)
- **Contract:** `pub fn critical_load(ls: &LinearSystem, perturbation: &LinearSystem, modes: usize) -> Result<BucklingResult, SimError>` with `pub struct BucklingResult { pub eigenvalues: Vec<f64>, pub mode_shapes: Vec<Vec<f64>>, pub method: &'static str }`.
- **Behaviour:** The generalised eigenproblem `(K + λG)φ = 0` with `G` the stress-stiffness from the pre-stressed state. The cheapest serious use of an eigenvalue solver, and it has a clean analytical benchmark: a pinned-pinned Euler column has `P_cr = π²EI/L²`, which **is** on the critical path and should be added to `SIM-VAL`.
- **Refusals:** `SimError::Unsupported("buckling needs a consistent stress-stiffness; a linear analysis without one is not enough")`. A non-symmetric perturbation → `SimError::Unsupported`. Fewer than one converged mode → `SimError::SolverFailed`.
- **Tests:** `euler_column_gives_pi_squared_EI_over_L_squared` — to 1e-6, a new benchmark. `the_mode_shape_is_the_expected_half_sine` — asserts the shape to 1e-3. `a_compression_free_model_gives_no_positive_eigenvalue` — asserts all eigenvalues are positive-or-null.
- **Acceptance:** Green with the Euler benchmark.
- **Risk:** The stress-stiffness derivation is error-prone. Fallback: the Euler benchmark catches it immediately, and it is Tier B either way.

### SIM-NL-010 — Contact: penetration depth from the kernel
- **State:** BLOCKED
- **Depends on:** KER-MSH-006, SIM-NL-002
- **Files:** `crates/sim/src/nl/contact/mod.rs` (new, SIM)
- **Contract:** `pub fn penetration_depth(nodes: &[Vec3], target: &Solid, tol: f64) -> Result<Vec<Penetration>, SimError>` with `pub struct Penetration { pub node: u32, pub closest: Vec3, pub depth: f64 }`.
- **Behaviour:** Establish which mesh nodes are inside the opposing solid and by how much. This is the first contact algorithm and the one that is purely geometric; it is placed before the contact *formulations* so the geometry can be tested alone. `KER-MSH-006` (`closest_point`) is the dependency and it does not exist.
- **Refusals:** `SimError::Kernel` for any kernel refusal. A node within `tol` of the surface → `Penetration { depth: 0.0 }`, reported as touching, **not** as penetrating — the band matters.
- **Tests:** `a_node_inside_is_reported_with_a_positive_depth` — a box node inside a larger box. `a_node_outside_is_not_reported`. `a_node_exactly_on_the_surface_is_touching_not_penetrating` — asserts `depth == 0.0`. `hostile_geometry_does_not_panic` — `NaN` nodes, a zero-thickness target.
- **Acceptance:** Green once `KER-MSH-006` lands; BLOCKED until then.
- **Risk:** `closest_point` may be slow enough to make contact impractical on a real mesh. Fallback: batch it with a spatial grid built in `crates/sim`; the contract does not depend on the kernel's performance.

### SIM-NL-011 — Penalty contact
- **State:** BACKLOG
- **Depends on:** SIM-NL-010, SIM-NL-002
- **Files:** `crates/sim/src/nl/contact/penalty.rs` (new, SIM)
- **Contract:** `pub struct PenaltyContact { pub stiffness_scale: f64, pub frictionless: bool }` and `pub fn apply(…, c: &PenaltyContact) -> Result<ElementMatrix, SimError>`.
- **Behaviour:** Contact as a very stiff spring. Simple, always positive definite, and **unconditionally hard to tune**: too stiff and the conditioning destroys the whole solve; too soft and bodies interpenetrate visibly. The report states the stiffness chosen, the resulting penetration, and the conditioning it cost. That transparency is the deliverable.
- **Refusals:** A penalty stiffness below `1e3 × E` → `SimError::Unsupported("a penalty contact stiffness of {k} is too soft; penetration will exceed the mesh size")`. A penalty above `1e12 × E` → `SimError::Unsupported("a penalty stiffness of {k} will make the stiffness matrix numerically singular in f64")`. Both bounds are computed from `E` and the element size, not hard-coded.
- **Tests:** `a_penetration_produces_a_restoring_force` — asserts the force opposes the penetration. `the_reported_penetration_matches_the_analytic_spring` — `F = k·δ` to 1e-6. `a_stiff_penalty_is_refused` — asserts `Err`. `the_energy_identity_still_holds` — the check from `SIM-STATIC-017` still passes, which proves the contact did not break the assembly.
- **Acceptance:** Green, and the report carries the stiffness, the penetration and the residual.
- **Risk:** It looks like a solution and is not accurate enough for a real contact stress. Fallback: `Validation::Unverified` always, and the doc comment points to `SIM-NL-012`.

### SIM-NL-012 — Lagrange multiplier contact
- **State:** BACKLOG
- **Depends on:** SIM-NL-010, SIM-SLV-010
- **Files:** `crates/sim/src/nl/contact/multiplier.rs` (new, SIM)
- **Contract:** `pub fn assemble_saddle_point(mesh: &FemMesh, pairs: &[(u32, u32)], mu: Option<f64>) -> Result<LinearSystem, SimError>` — a saddle-point system with a non-penetration constraint per contact pair, assembled into an **indefinite** matrix.
- **Behaviour:** Contact as an exact constraint, which is what production contact actually is: no penetration, no penalty stiffness, no conditioning problem. Requires `SIM-SLV-010`'s indefinite factorisation and `SIM-VAL-014`'s finding on tets and locking. Friction is a Coulomb cone linearised about the current direction; the linearisation error is reported.
- **Refusals:** `SimError::Unsupported("frictional contact needs the Coulomb cone linearised about the previous increment")` unless a reference direction is supplied. A contact pair whose normal is degenerate → `SimError::BadDirection`. An incompatible system (an active set that makes the saddle point singular) → `SimError::SingularStiffness` with the active pair named.
- **Tests:** `a_rigid_block_on_a_rigid_floor_does_not_penetrate` — asserts `max depth == 0` to 1e-12. `the_reaction_equals_the_applied_weight` — to 1e-6. `an_active_set_that_is_infeasible_is_refused` — asserts `Err` naming the pair. `the_saddle_point_system_is_indefinite` — asserts a negative eigenvalue exists.
- **Acceptance:** Green, and **every result is `Validation::Unverified`** — contact is Tier B/C and the stamp says so until a contact benchmark exists.
- **Risk:** The active-set iteration is the hardest code in this epic. Fallback: `SIM-NL-011` is the simpler fallback path and the report names which was used.

### SIM-NL-013 — J2 plasticity
- **State:** BACKLOG
- **Depends on:** SIM-NL-006, SIM-NL-002
- **Files:** `crates/sim/src/nl/plasticity.rs` (new, SIM)
- **Contract:** `pub struct J2 { pub yield_stress: f64, pub hardening: f64 }` and `pub fn return_map(sigma_trial: [f64; 6], state: &HardeningState, m: &J2) -> Result<ReturnResult, SimError>` with `pub struct ReturnResult { pub sigma: [f64; 6], pub plastic_strain_increment: [f64; 6], pub yield_function: f64 }`.
- **Behaviour:** Radial return with linear isotropic hardening. It has the best verification story of any plasticity model available: an analytic solution exists for proportional loading, so `σ_equiv` versus plastic strain is a straight line for proportional loading and a curve for non-proportional loading, and both are checkable. **That verification is not yet written**, so this task is BACKLOG and its first act is to add a case to `SIM-VAL`.
- **Refusals:** `yield_stress <= 0.0` → `SimError::BadDistance`. An inconsistent state (`equivalent plastic strain < 0`) → `SimError::BadDistance`. `SimError::Unsupported` for any kinematic hardening, for Bauschinger, and for any rate dependence — none of which are implemented and all of which are asked for.
- **Tests:** `proportional_loading_gives_the_analytic_hardening_line` — the analytical check, to be added to `SIM-VAL`. `elastic_loading_below_yield_is_unchanged` — asserts exactly no plastic strain. `the_yield_surface_is_satisfied_after_return` — asserts `σ_eq == σ_y` to 1e-9. `kinematic_hardening_is_refused` — asserts `Err`.
- **Acceptance:** Green, **and a new `SIM-VAL` case exists and passes before this task is `VALIDATED`**.
- **Risk:** Delivering plasticity before its benchmark. Fallback: the acceptance criterion forbids it — the benchmark is a precondition, not a follow-up.

### SIM-NL-014 — Fatigue and creep: refused
- **State:** READY
- **Depends on:** SIM-NL-001
- **Files:** `crates/sim/src/nl/mod.rs` (SIM)
- **Contract:** `pub enum FatigueModel { SMiner { mean_stress_correction: MeanStressCorrection } }` whose constructor returns `SimError::Unsupported`, and `pub fn creep_rate(conditions: &CreepConditions) -> Result<f64, SimError>` which returns `SimError::Unsupported`.
- **Behaviour:** The refusals, with **good messages**. A fatigue request returns: that fatigue is not implemented, that a stress amplitude from `SIM-STATIC` is a valid input to a fatigue assessment done elsewhere, and which S-N curve the user should look for — **without** naming a specific curve we do not have data for. This is the honest help: the result is "we do not do this, here is what we can give you".
- **Refusals:** Both, unconditionally.
- **Tests:** `fatigue_is_unsupported` — asserts the message names the unsupported capability. `the_message_points_at_what_is_available` — asserts it mentions stress amplitude. `creep_is_unsupported` — asserts `Err`.
- **Acceptance:** Green; the messages are reviewed by a human before release.
- **Risk:** A user working around the refusal by treating `SIM-STATIC` output as a fatigue result. Fallback: the report's verdict line for every static study says it is a static stress result and names what it is not.

---

## 11. `SIM-VIZ` — Result visualisation (14 tasks)

Layering: `crates/sim` is L3 and must not depend on `ui-egui` (L5) or on egui/wgpu. `SIM-VIZ-001..006` are **pure numerics in `crates/sim`** — geometry and scalars, no colours, no widgets. `SIM-VIZ-007..014` are owned by REN/UI/engine and are marked with their owner.

### SIM-VIZ-001 — `ResultField`: a scalar on nodes or on elements
- **State:** READY
- **Depends on:** SIM-VAL-015
- **Files:** `crates/sim/src/results.rs` (SIM)
- **Contract:** `pub enum FieldLocation { Node, Element, GaussPoint }` and `pub struct ResultField { pub name: String, pub location: FieldLocation, pub values: Vec<f64>, pub unit: &'static str }` with `pub fn range(&self) -> Result<(f64, f64), SimError>` and `pub fn at_node(&self, mesh: &FemMesh, n: u32) -> Result<f64, SimError>`.
- **Behaviour:** The field is a named vector with a declared location and a unit string. `unit` is a `&'static str` so it cannot be a per-study typo, and it is written into the export. `at_node` averages element values onto a node on demand, so the viewport does not need to know the location.
- **Refusals:** A length mismatch with the mesh → `SimError::InvalidIndex`. A non-finite value → `SimError::NonFinite`. An empty field → `SimError::Empty`. A `GaussPoint` field with an inconsistent count → `SimError::InvalidIndex`.
- **Tests:** `range_of_a_constant_field_is_that_constant` — to 1e-12. `an_element_field_reads_at_a_node` — asserts the weighted average. `a_length_mismatch_is_refused` — asserts `Err`. `a_non_finite_value_is_refused` — asserts `Err`. `every_field_carries_a_unit` — asserts the string is non-empty.
- **Acceptance:** Green; every field the other epics produce is a `ResultField`.
- **Risk:** A unit string that does not match the values. Fallback: the values are SI and the unit string is fixed per field name in one table.

### SIM-VIZ-002 — Deformed positions
- **State:** READY
- **Depends on:** SIM-VIZ-001
- **Files:** `crates/sim/src/viz/deform.rs` (new, SIM)
- **Contract:** `pub fn deformed_positions(mesh: &FemMesh, u: &[f64], scale: f64) -> Result<Vec<Vec3>, SimError>` and `pub fn auto_scale(mesh: &FemMesh, u: &[f64], fraction: f64) -> Result<f64, SimError>`.
- **Behaviour:** `p' = p + scale·u`. `auto_scale` picks the scale so the maximum displacement is `fraction` of the model's diagonal, which is the only scaling that makes a result readable. **The scale factor is returned alongside and shown**, always — a deformed shape at 10× with no label is how a user concludes a part deflects a metre.
- **Refusals:** `scale` non-finite → `SimError::NonFinite`. A `u` of the wrong length → `SimError::InvalidIndex`. `fraction <= 0.0` → `SimError::BadTolerance`. A zero-displacement field → `auto_scale` returns `0.0`, **not** a division by zero.
- **Tests:** `a_zero_displacement_gives_the_original_positions` — exact. `auto_scale_makes_the_max_displacement_the_requested_fraction` — to 1e-9. `a_zero_displacement_field_gives_a_zero_scale` — asserts `0.0`, no panic. `the_scale_is_returned_with_the_positions` — a round-trip type that cannot lose it.
- **Acceptance:** Green; consumed by the viewport adapter.
- **Risk:** A large `scale` making an inverting element look fine. Fallback: the viewport draws the undeformed mesh as a ghost (`SIM-VIZ-003`), which shows the direction and magnitude relative to the real shape.

### SIM-VIZ-003 — The undeformed ghost
- **State:** BACKLOG
- **Depends on:** SIM-VIZ-002
- **Files:** `crates/sim/src/viz/deform.rs` (SIM)
- **Contract:** `pub struct GhostPair { pub undeformed: Vec<Vec3>, pub deformed: Vec<Vec3>, pub scale: f64 }` and `pub fn ghost(mesh: &FemMesh, u: &[f64], fraction: f64) -> Result<GhostPair, SimError>`.
- **Behaviour:** Both meshes in one value, with the scale, so the viewport cannot draw one without the other. Drawn as a wireframe over a shaded deformed surface, which is the standard presentation and is also the only way to see that a deformation is mostly rigid-body.
- **Refusals:** As `SIM-VIZ-002`.
- **Tests:** `the_ghost_pair_has_matching_positions` — same length. `a_zero_field_makes_the_two_identical`. `the_scale_is_carried` — asserts.
- **Acceptance:** Green.
- **Risk:** None.

### SIM-VIZ-004 — Contour bands
- **State:** READY
- **Depends on:** SIM-VIZ-001
- **Files:** `crates/sim/src/viz/contour.rs` (new, SIM)
- **Contract:** `pub fn bands(field: &ResultField, count: usize) -> Result<Vec<Band>, SimError>` with `pub struct Band { pub lo: f64, pub hi: f64, pub index: usize }`, and `pub fn assign(mesh: &FemMesh, field: &ResultField, bands: &[Band]) -> Result<Vec<usize>, SimError>` returning a band index per element.
- **Behaviour:** Split the field's range into `count` equal bands and assign each element to one. **Equal bands, not quantiles**: a quantile plot makes a nearly-constant field look dramatic, and a user reading a stress contour that hides its own scale has been misled. The exact extremes are always their own band (`count + 2` bands with the two extremes separate), because the maximum is the number the user is looking for.
- **Refusals:** `count < 2` → `SimError::BadTolerance`. A constant field (range zero) → `Ok` with **one** band, and the doc comment says the colour map must then be degenerate, **not** an arbitrary spread of noise. A non-finite range → `SimError::NonFinite`.
- **Tests:** `a_constant_field_gives_one_band` — asserts. `band_edges_are_contiguous_and_cover_the_range` — to 1e-12. `the_maximum_is_in_the_top_band` — asserts. `a_count_of_one_is_refused` — asserts `Err`. `every_element_gets_a_band` — asserts the assignment length.
- **Acceptance:** Green; consumed by the viewport's colour map.
- **Risk:** A colour map that maps a band index to a colour without the band **edges** visible. Fallback: the legend is built from the `Band` list, so it cannot drift from the data.

### SIM-VIZ-005 — Isosurface and isoplot extraction
- **State:** BACKLOG
- **Depends on:** SIM-VIZ-004, SIM-VIZ-001
- **Files:** `crates/sim/src/viz/contour.rs` (SIM)
- **Contract:** `pub fn isosurface(mesh: &FemMesh, field: &ResultField, level: f64) -> Result<Vec<Triangle>, SimError>` (marching tetrahedra) and `pub fn isoplot(nodes: &[Vec3], edges: &[[u32; 2]], field: &ResultField, level: f64) -> Result<Vec<(Vec3, Vec3)>, SimError>`.
- **Behaviour:** Marching tetrahedra on the element mesh, and marching segments on the surface wireframe for an isoplot. Marching tetrahedra is used rather than marching cubes **because it needs no case table and cannot produce the ambiguous cases that make marching cubes ambiguous** — a deliberate trade of memory for correctness.
- **Refusals:** A level outside the field's range → `Ok` with an empty list, **not** an error: "no isosurface at that level" is a true answer. A non-finite level → `SimError::NonFinite`.
- **Tests:** `a_linear_field_gives_a_plane_isosurface` — the surface area matches `A` within 1e-6. `a_level_outside_the_range_gives_nothing` — asserts an empty list, no error. `a_monotonic_field_gives_a_closed_surface` — asserts the triangle fan closes. `the_isosurface_is_inside_the_model_bounds` — every vertex lies within the mesh bounds.
- **Acceptance:** Green.
- **Risk:** A degenerate tet producing a zero-area triangle. Fallback: triangles below `1e-12 · bounds_diagonal²` in area are dropped and the count of drops is in the debug log.

### SIM-VIZ-006 — Principal direction glyphs
- **State:** BACKLOG
- **Depends on:** SIM-STATIC-014
- **Files:** `crates/sim/src/viz/glyphs.rs` (new, SIM)
- **Contract:** `pub fn principal_glyphs(mesh: &FemMesh, stress: &[[f64; 6]], at: GlyphWhere, scale: f64) -> Result<Vec<Glyph>, SimError>` with `pub enum GlyphWhere { OnSurface, OnSectionPlane(Plane), AtNode(u32) }` and `pub struct Glyph { pub at: Vec3, pub direction: Vec3, pub length: f64 }`.
- **Behaviour:** A line segment per node along the principal direction, length proportional to the principal value. `GlyphWhere::OnSectionPlane` reuses `KER`'s `section` (`KER-MSH-005`) — **BLOCKED until it is re-exported**, since glyphs inside a part are the most useful view of a stress field and there is currently no way to take that section from outside the kernel.
- **Refusals:** `scale <= 0.0` → `SimError::BadTolerance`. A stress array of the wrong length → `SimError::InvalidIndex`. A degenerate principal direction (a hydrostatic state, where all three are equal) → the glyph is omitted, **not** given an arbitrary direction.
- **Tests:** `a_uniaxial_state_gives_collinear_glyphs` — all along one axis. `a_hydrostatic_state_gives_no_glyphs` — asserts an empty list. `a_shear_state_gives_45_degrees` — to 1e-6. `a_section_plane_request_is_blocked_until_ker_exports_section` — a test asserting the current behaviour.
- **Acceptance:** Green for surface glyphs; section glyphs BLOCKED on `KER-MSH-005`.
- **Risk:** Glyph density hiding the model. Fallback: a decimation parameter, default 1 glyph per `k` nodes, with `k` in the UI.

### SIM-VIZ-007 — Viewport result layer
- **State:** BLOCKED
- **Depends on:** SIM-VIZ-004, SIM-VIZ-002, REN-001
- **Files:** `crates/render/src/simview.rs` (new, **REN owner**), `crates/render/src/lib.rs` (REN owner)
- **Contract:** `pub struct SimOverlay { pub geometry: Vec<(Vec3, Vec3, Vec3)>, pub colors: Vec<[f64; 3]>, pub legend: Vec<LegendEntry> }` and `pub fn push_overlay(scene: &mut Scene, overlay: &SimOverlay) -> Result<(), SceneError>`.
- **Behaviour:** **REN owns this file, not SIM.** The boundary is: `crates/sim` produces plain geometry and scalars (`Vec<Vec3>`, `Vec<f64>`, band edges); `crates/render` turns a band index into a colour using its own colormap and pushes triangles. `crates/render` (L2) therefore **must not** depend on `crates/sim` (L3) — the dependency runs the other way, through `ui-egui` (L5), which may depend on both. The adapter lives in `SIM-VIZ-008`.
- **Refusals:** `SceneError` for a non-finite colour or a geometry index out of range — matching `Scene::push`'s existing contract.
- **Tests:** `an_overlay_with_a_bad_colour_is_refused` — asserts `Err`. `an_overlay_with_mismatched_geometry_and_colors_is_refused` — asserts `Err`. `an_empty_overlay_is_a_no_op`.
- **Acceptance:** Green's owner is REN; SIM's acceptance is that the data it produces is sufficient and that no SIM type appears in `crates/render`'s signature.
- **Risk:** Someone types the overlay as `cadcraft_sim::ResultField` in `crates/render`, breaking the layer. Fallback: `cargo xtask layers` catches it, which is why the task is written with a plain-geometry signature.

### SIM-VIZ-008 — The sim↔viewport adapter
- **State:** BLOCKED
- **Depends on:** SIM-VIZ-007, UI-001
- **Files:** `crates/ui-egui/src/panels3/sim_view.rs` (new, **UI owner**)
- **Contract:** `pub fn to_overlay(r: &ResultSet, field: &str, mode: ResultMode) -> Result<SimOverlay, SceneError>` with `pub enum ResultMode { Undeformed, Deformed { scale: f64 }, DeformedGhost { scale: f64 }, Contour { field: String, bands: usize } }`.
- **Behaviour:** The one place `crates/sim` types become `crates/render` types. UI-owned. It selects the field, computes bands via `SIM-VIZ-004`, gets positions via `SIM-VIZ-002`, and builds a `SimOverlay`. **It must also refuse to render an `Unverified` result without the watermark** — the rule lives here because this is where the pixels are.
- **Refusals:** An unknown field name → `Err`, listing the available names. An `Unverified` result without `watermark: true` in the mode → refused.
- **Tests:** `an_unknown_field_is_refused_with_the_list` — asserts the message names the available fields. `an_unverified_result_cannot_be_rendered_without_the_watermark` — asserts `Err`. `a_deformed_mode_carries_the_scale_into_the_legend` — asserts.
- **Acceptance:** Green's owner is UI. SIM's acceptance: the adapter's inputs are exactly the types `SIM-VIZ-001..004` produce.
- **Risk:** The watermark being easy to omit. Fallback: it is a required parameter, not a default.

### SIM-VIZ-009 — Mesh display and quality colouring
- **State:** BACKED
- **Depends on:** SIM-MSH-002, SIM-MSH-006
- **Files:** `crates/ui-egui/src/panels3/sim_view.rs` (UI owner)
- **Contract:** `pub enum MeshOverlay { Off, Wireframe { every: usize }, ElementQuality, ElementType, NodeSet(String), EdgeSet(String), FaceSet(String) }`.
- **Behaviour:** Seeing the mesh is a diagnostic, not decoration: the user needs to see that the elements are long and thin exactly where the stress is odd. Quality colouring uses the same band machinery, inverted so red is bad.
- **Refusals:** A set name that does not exist → `Err` naming the available sets. `every == 0` → `SimError::BadTolerance`.
- **Tests:** `an_unknown_set_name_is_refused_with_the_list`. `quality_colouring_uses_the_same_bands_as_the_report` — asserts the legend matches, which is the consistency that matters.
- **Acceptance:** Green's owner is UI.
- **Risk:** None.

### SIM-VIZ-010 — Probe a result at a point
- **State:** BLOCKED
- **Depends on:** SIM-VIZ-008, REN-002
- **Files:** `crates/ui-egui/src/panels3/sim_probe.rs` (new, **UI owner**), `crates/render/src/pick.rs` (REN owner)
- **Contract:** `pub fn probe(mesh: &FemMesh, field: &ResultField, p: Vec3) -> Result<ProbeHit, SimError>` with `pub struct ProbeHit { pub element: usize, pub distance: f64, pub value: f64, pub coordinates: Vec3 }`, plus a viewport pick that produces the world point.
- **Behaviour:** Click a point on the model, get the number. **This is the most-used feature in every CAE product and it is where the "is this number real?" question gets answered.** The probe reports the element it hit, its coordinates, the distance to the element and the field value there — enough for the user to see whether they clicked the peak or a neighbour.
- **Refusals:** A point outside the mesh → `Err` with the distance. A pick that misses → `None`, not a default value.
- **Tests:** `probing_the_peak_reports_the_peak_element`. `probing_outside_the_mesh_is_refused`. `the_probe_reports_the_element_not_just_the_value` — asserts the element index is present, which is the anti-misleading part.
- **Acceptance:** Green's owner is UI; blocked on REN's picking.
- **Risk:** A probe value that is a smoothed nodal average rather than the element value. Fallback: the probe reports **both** the element value and the smoothed nodal value, labelled.

### SIM-VIZ-011 — The viewport cut-plane
- **State:** BLOCKED
- **Depends on:** KER-MSH-005, SIM-VIZ-005
- **Files:** `crates/ui-egui/src/panels3/sim_view.rs` (UI owner)
- **Contract:** `pub fn clip_by_plane(section: &Section, plane: &Plane) -> Result<ClippedMesh, SimError>` with `pub struct ClippedMesh { pub kept: Vec<u32>, pub cut_faces: Vec<[Vec3; 3]> }`.
- **Behaviour:** Clip the mesh against a plane and cap it with the section, so the interior stress field is visible. The cut cap is built from the element faces that straddle the plane, which requires the mesh — **not** the solid's section curve, because the mesh's internal faces are not in the B-rep at all.
- **Refusals:** `SimError::Kernel` if the section call fails. A plane that removes the whole mesh → `Ok` with an empty `kept`, and the viewport says "nothing on this side", **not** a crash.
- **Tests:** `clipping_removes_the_far_half` — asserts the element count roughly halves. `the_cut_cap_covers_the_cut` — asserts no hole. `a_plane_outside_the_mesh_clips_to_nothing` — asserts an empty result without error.
- **Acceptance:** Green's owner is UI. **BLOCKED on `KER-MSH-005`** — `section` is not exported.
- **Risk:** A cap that does not line up with the section curve, showing a gap. Fallback: the cap is built from element faces only, so it cannot disagree with the mesh by construction.

### SIM-VIZ-012 — Result animation for a transient
- **State:** BACKLOG
- **Depends on:** SIM-THERM-006, SIM-VIZ-002
- **Files:** `crates/sim/src/viz/animate.rs` (new, SIM)
- **Contract:** `pub struct Animation { pub frame_times: Vec<f64>, pub positions: Vec<Vec<Vec3>>, pub fields: Vec<Vec<f64>>, pub unit: &'static str, pub scheme: &'static str }` and `pub fn to_animation(r: &TransientResult, u: Option<&[Vec<f64>]>) -> Result<Animation, SimError>`.
- **Behaviour:** A frame list plus the **scheme and time step**, which the animation displays. A transient animation that does not say it is backward Euler at `Δt = 2 s` invites the reader to treat a smeared numerical history as a physical oscillation. The animation interpolates between stored frames and says so when it does.
- **Refusals:** A frame list of length 0 → `SimError::Empty`. Mismatched lengths → `SimError::InvalidIndex`. A field at a frame the positions do not have → `Err` naming the frame.
- **Tests:** `the_animation_carries_the_scheme_and_step` — asserts both strings are present. `interpolation_is_linear_and_labelled` — asserts. `a_zero_length_animation_is_refused` — asserts `Err`.
- **Acceptance:** Green.
- **Risk:** Memory for a long transient. Fallback: `store_every` from `SIM-THERM-006`, and the animation reports the stored interval.

### SIM-VIZ-013 — Export: VTK legacy ASCII and CSV
- **State:** BACKBACKLOG
- **Depends on:** SIM-VIZ-001, SIM-VAL-015
- **Files:** `crates/sim/src/export.rs` (new, SIM)
- **Contract:** `pub fn export_vtk(mesh: &FemMesh, fields: &[ResultField], deformed: Option<&[Vec3]>) -> Result<String, SimError>` and `pub fn export_csv(fields: &[ResultField]) -> Result<String, SimError>`.
- **Behaviour:** VTK legacy ASCII, written by hand — it is a text format we implement from its published specification, not an asset and not a dependency. **Both exports refuse an `Unverified` result** with `SimError::Unverified`, unless the caller passes `export_mode: ExportMode::UnverifiedWithWatermark`, which writes `UNVERIFIED` into the file header. **There is no path that exports an unverified result without the word in it.** The CSV carries the unit in the column header.
- **Refusals:** `SimError::Unverified` for an unverified export without the explicit mode. A field whose length does not match the mesh → `SimError::InvalidIndex`. A non-finite value → `SimError::NonFinite`.
- **Tests:** `an_unverified_result_is_refused` — asserts `Err`. `the_watermark_mode_writes_the_word` — asserts the header contains `UNVERIFIED`. `the_csv_header_carries_units` — asserts. `the_vtk_is_parseable_by_our_own_reader` — a round trip through a test reader, which is the only "third-party" validation available and is honestly labelled as such. `a_field_length_mismatch_is_refused` — asserts `Err`.
- **Acceptance:** Green, and the "validated by our own reader" caveat is in the doc comment, following the precedent `crates/solid/src/lib.rs:48-51` sets for STEP.
- **Risk:** A VTK file that ParaView rejects. Fallback: the doc comment says it has never been opened by ParaView and must be validated there first — **exactly the wording `lib.rs` uses for STEP**, because the same honesty applies.

### SIM-VIZ-014 — The unverified watermark
- **State:** READY
- **Depends on:** SIM-VAL-015
- **Files:** `crates/sim/src/results.rs` (SIM)
- **Contract:** `pub fn watermark(validation: &Validation) -> Option<&'static str>` returning `None` for `Analytical` and `Convergence`, and a sentence naming **what** is unverified for `Unverified { reason }`.
- **Behaviour:** The user-facing half of §0.5. The string is generated from the `reason`, so "not verified against any analytical benchmark" and "verified for deflection, not for stress" are different words on screen. It is not a generic "UNVERIFIED" badge.
- **Refusals:** None; this function is total.
- **Tests:** `an_analytical_result_has_no_watermark` — asserts `None`. `an_unverified_result_names_its_reason` — asserts the string contains the reason. `a_convergence_result_has_no_watermark`.
- **Acceptance:** Green; the viewport, the report and the exporter all call it, so the three cannot disagree.
- **Risk:** Users learn to ignore a badge. Fallback: the badge says *why*, so it is informative rather than a ritual; and it disappears the moment a benchmark is run.

---

## 12. Summary

| Epic | Tasks | Ready now | Blocked on `KER`/REN/UI | Backlog |
|---|---|---|---|---|
| `SIM-VAL` | 16 | 13 | 0 | 3 |
| `SIM-MSH` | 26 | 5 | 4 | 17 |
| `SIM-SLV` | 14 | 6 | 0 | 8 |
| `SIM-STATIC` | 26 | 0 | 0 | 26 |
| `SIM-THERM` | 16 | 0 | 1 | 15 |
| `SIM-NL` | 14 | 2 | 1 | 11 |
| `SIM-VIZ` | 14 | 5 | 4 | 5 |
| **Total** | **126** | **31** | **10** | **85** |

(`SIM-STATIC` is listed with 26 here because `SIM-STATIC-026` is included; the epic header above
says 24 and that figure is the one to correct — **the table is authoritative: 26 tasks**, total 126.)

### Immediate blockers, in the order they should be cleared

1. `KER-MSH-001` (tracked mesh) — unblocks `SIM-MSH-008`, and with it every face set.
2. `KER-MSH-003` (stable face/edge ids) — unblocks `SIM-MSH-009`, `SIM-MSH-019` and the whole
   selection story.
3. `KER-MSH-005` (`section` re-export) — a one-line change that unblocks `SIM-MSH-019`,
   `SIM-VIZ-006` and `SIM-VIZ-011`.
4. `KER-BOOL-001..003` — the three open kernel defects. They gate `SIM-MSH-013` (the mesher core)
   and every downstream analysis, because a mesh of a cracked solid is refused by
   `SIM-MSH-012`.
5. The `xtask/src/layers.rs` TABLE registration of `sim` at L3 — owned by PLT, one line.

### What is trusted, restated for the reader who skipped §4

- **Trusted (Tier A), after `SIM-VAL` passes:** linear static isotropic elasticity, steady thermal,
  diffusive transient thermal, self-weight.
- **Internal-consistency only (Tier B), always watermarked:** large deformation, hyperelasticity,
  penalty contact, buckling.
- **Refused (Tier C), by name:** contact with a validated model, plasticity, creep, fatigue,
  buckling as a design number, composites, fracture, impact, fluid coupling, thermal contact.