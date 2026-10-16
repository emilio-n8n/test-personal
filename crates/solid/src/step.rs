//! STEP AP214 writing: a whole solid as one `FACETED_BREP`.
//!
//! # Why a `FACETED_BREP` at all
//!
//! STEP has two ways to say "a polyhedron", and the kernel is already a polyhedron: its faces are
//! planar and its boundaries are exact straight loops, so there is nothing to approximate and no
//! reason to write a NURBS surface and a pile of trim curves. [`FACETED_BREP`] is the honest
//! representation of what this kernel holds — a shell of planar faces over poly loops.
//!
//! # How far this writer has been checked
//!
//! **No third-party STEP reader has ever opened a file this writer produced.** No OCCT, FreeCAD,
//! SolidWorks, CAD Exchanger or `stepcode` was available where it was written, and a file no
//! independent parser has accepted is a claim, not a guarantee. Nothing in this repository can
//! change that, so it is stated at the top of this file, in the crate's own docs, and here in full:
//!
//! - The tests in this module read the output back with a parser written beside the writer. They
//!   catch a graph that does not hang together — a dangling `#n`, an entity nothing refers to, a
//!   reversed loop, a dropped face.
//! - `tests/step_interop.rs` reads the same files with a **second**, independently written parser
//!   and checks the entity graph against a table of the expected attribute list of every keyword
//!   this file can emit, the closure and orientation of the shell read back from the written
//!   coordinates, and the volume recomputed from those coordinates. It exists so that a bug in
//!   the first parser cannot hide, and it agrees with the same geometry the mesh writer produces.
//! - **The second parser had to earn its place, and its first version was itself broken.** Written
//!   before this revision, it read the opening `.` of an enumeration and not its closing one, so it
//!   lost the thread on `#2` — the `SI_UNIT` — and reported every file it was handed as holding no
//!   solids; and it had no opinion whatever about *what* a reference names, so it could not see the
//!   `VERTEX_POINT` this writer had put where `axis2_placement_3d` names a `cartesian_point`.
//!   Both are corrected, and the second is now pinned by a reference-typing table in the interop
//!   test. **Neither correction is evidence about a real reader.**
//! - Both parsers were written by the same author, from the same reading of the same specification.
//!   Neither is a conforming STEP implementation: they do not check EXPRESS semantics, they do not
//!   know about subtypes, inherited attributes or WHERE rules, and they do not build a topology.
//!   **A file can pass every test here and still be rejected by a real reader.** What is missing is
//!   one round trip through one independent implementation, and `DAT-003` in the master plan exists
//!   to get it. **That is as true of this revision as of any other: no OCCT, FreeCAD, SolidWorks,
//!   CAD Exchanger or `stepcode` has opened a file this writer produced. Finding and fixing two
//!   defects in our own code is not a round trip.**
//!
//! The list below is therefore not a summary of the tests. It is the list of things this file does
//! that a careful reader has to be told, because nothing about them is obvious from the schema and
//! several of them are places a strict validator can object.
//!
//! # Every departure, shortcut and approximation, in one place
//!
//! ## A. Deliberate departures from AP214's `faceted_brep` rule
//!
//! 1. **`ADVANCED_FACE`, where the rule names `FACE_SURFACE`.** `ADVANCED_FACE` is a `FACE_SURFACE`
//!    subtype that adds obligations stated over *edges*: every loop vertex must lie on the surface,
//!    and every edge must carry a curve. Both hold here — every face is planar and every edge is a
//!    `LINE` in that same plane — and `tests/step_interop.rs` checks both against the coordinates
//!    actually written. What does **not** hold is the shape those obligations are phrased in: a
//!    `POLY_LOOP` (item 3) is a `LOOP` but is not an `EDGE_LOOP`, so a validator that reads
//!    `ADVANCED_FACE` as demanding `EDGE_LOOP` bounds has nothing in this file it will accept, and
//!    there is no `ORIENTED_EDGE` anywhere for it to check one with. **This is the largest
//!    remaining risk in the file and it is an open judgement, not a settled one.** Swapping the
//!    keyword back to `FACE_SURFACE` removes the question, is a one-line change, and `FACE_SURFACE`
//!    is in the interop test's schema and reference tables so the file stays checkable either way.
//!    It has deliberately *not* been swapped: no reader has ever accepted either spelling, so
//!    choosing blind would be pretending to knowledge nobody in this repository has.
//! 2. **A second outer loop becomes a `FACE_BOUND`.** A face holding *several disjoint* regions
//!    writes its extra outer loops as `FACE_BOUND`, because a STEP face's bound list has no way to
//!    say "a second outer loop". A reader that assumes every `FACE_BOUND` is a hole reads such a
//!    face as one region with a hole punched through it, and imports the wrong shape.
//! 3. **`POLY_LOOP` instead of `EDGE_LOOP`, so there are no `ORIENTED_EDGE`s anywhere.** A
//!    `POLY_LOOP` lists `VERTEX_POINT`s and nothing else, so the file carries no per-use edge
//!    orientation flag: each face's sense along a shared edge is implied only by the order of the
//!    vertices in that face's `POLY_LOOP`. That is correct and is what every faceted B-rep does,
//!    but a reader that looks for oriented edges to build its topology map will find none, and a
//!    reader that assumes the two faces of an edge traverse it the *same* way will be wrong here.
//!
//! ## B. Entities a reader has to tolerate
//!
//! 4. **The `EDGE_CURVE`s are referenced by nothing at all.** A `POLY_LOOP` names vertices, never
//!    edges, so an edge curve written for a faceted shell is an orphan, and strict readers reject
//!    orphans. The writer therefore hangs them off a second shape representation —
//!    `GEOMETRIC_CURVE_SET` of `COMPOSITE_CURVE`s of `COMPOSITE_CURVE_SEGMENT`s — attached to the
//!    same `PRODUCT_DEFINITION_SHAPE` as the solid. That is an edge-based *wireframe*
//!    representation, not a real B-rep: the composite curves carry the edges and nothing else, and
//!    no reader can rebuild the shell from them.
//! 5. **The product has two shape representations.** `ADVANCED_BREP_SHAPE_REPRESENTATION` (the
//!    solid) and `GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION` (the edges), both named by
//!    a `SHAPE_DEFINITION_REPRESENTATION` from the same `PRODUCT_DEFINITION_SHAPE`. That is legal —
//!    a shape may have several representations — but a reader that expects exactly one, or that
//!    picks the first and warns about the second, is behaving reasonably.
//! 6. **Exactly two entities in the file are named by nothing**, and they are those two
//!    representations: a part 21 file is a forest, not a single tree, and the shape representations
//!    are where a reader starts.
//!
//! ## C. What the file does not contain
//!
//! 7. **No `APPLICATION_PROTOCOL_DEFINITION`, no `MECHANICAL_CONTEXT`, no
//!    `PRODUCT_RELATED_PRODUCT_CATEGORY`.** The product structure is the AP203-shaped minimum
//!    (`PRODUCT_CONTEXT` + `PRODUCT_DEFINITION_CONTEXT`). An AP214 reader that classifies a part by
//!    walking to `MECHANICAL_CONTEXT('...',#app,'mechanical')` will not find the branch it expects
//!    and may report the file as "not an AP214 part" or import it as a generic shape with no
//!    product metadata.
//! 8. **No colours, styles or units of presentation.** The shape is unstyled. A reader that shows
//!    what it finds will use its own defaults; a reader that insists on a
//!    `PRESENTATION_STYLE_ASSIGNMENT` has nothing to find.
//! 9. **No assembly structure, no `SHAPE_REPRESENTATION_RELATIONSHIP`, no product shape
//!    identity.** One solid, one product. Correct for this writer's job, and a reader looking for
//!    a component tree will find none.
//! 10. **The units are millimetres and nothing says so in words.** The `LENGTH_UNIT` is
//!    `SI_UNIT(.MILLI.,.METRE.)` with an **empty name**, as are the angle and solid-angle units. A
//!    reader has to read the prefix to know the file is in millimetres; a tool that trusts the
//!    unit *name* sees a blank. The conversion is not performed anywhere — coordinates go out in
//!    kernel units, and the kernel works in millimetres.
//! 11. **No `DATE_AND_TIME`/`TIME_STAMP`, and empty author, organisation and authorisation fields
//!    in `FILE_NAME`.** Deliberate: a clock would make two exports of one solid differ, and an
//!    export that reproduces byte for byte is worth more than a timestamp nobody reads. A reader
//!    that requires a timestamp to display provenance gets an empty string.
//!
//! ## D. Numeric and lexical choices that are legal but unusual
//!
//! 12. **Reals are written in Rust's shortest round-trip form, which is never exponential.** `1.0`
//!     becomes `1.0` and `48.0` becomes `48.0`, but a coordinate of `1e-300` becomes three hundred
//!     zeroes and a `0`. Part 21 accepts both (`1.E-300` is legal and is what mainstream writers
//!     emit), so this is a legibility defect rather than an invalid file — but a part-21 reader
//!     that parses reals with a fixed-size buffer has a real chance of tripping over a 300-digit
//!     literal. The cure is one line in `real`: format with an exponent when the magnitude leaves
//!     a sane band.
//! 13. **Names are sanitised, not escaped.** `clean_name` keeps ASCII alphanumerics, space and
//!     `_-.:`, caps the result at 120 characters and substitutes `cadcraft` for anything empty. A
//!     name in a non-Latin script or carrying an accent is therefore silently truncated — part 21
//!     has an `\X2\...\X0\` escape for exactly this and it is not used. A name carrying a quote
//!     would end the literal and corrupt every statement after it, so dropping it is deliberate;
//!     the cost is that the product name in the file is not always the product name asked for.
//! 14. **`DIRECTION`s are interned on a `1e-9` grid.** A cube has twelve edges and three
//!     directions, so sharing the few directions is most of the file's size. Two directions closer
//!     than `1e-9` share one `DIRECTION` and the stored vector is whichever was written first — a
//!     reader measuring directions below `1e-9` sees a difference the writer considers noise.
//! 15. **The file's declared uncertainty is the kernel's working tolerance**, not a round number
//!     (`diagonal × 1e-9`, clamped). That is the honest value — it is the distance the shell was
//!     welded at — but it means the uncertainty changes with the size of the part, and a reader
//!     that assumes a conventional `1e-7` will see something else.
//!
//! ## E. Ten minutes with a real CAD system: what to ask it
//!
//! This is the checklist. In order, because the first failures make the rest unreadable.
//!
//! - [ ] Does the reader accept the file at all (no schema error, no "unknown entity")?
//! - [ ] Does it find **exactly one** solid, and one `CLOSED_SHELL` under it?
//! - [ ] Does the solid's **volume** equal the kernel's, to a relative `1e-9`?
//! - [ ] Do the faces point **outward** — is the solid selectable and is its volume positive?
//! - [ ] Is the second shape representation (the edge wireframe) ignored, warned about, or fatal?
//! - [ ] Does the reader accept **`ADVANCED_FACE`** where `faceted_brep` names `FACE_SURFACE`,
//!       and does it accept a `POLY_LOOP` inside one at all? A `POLY_LOOP` is a `LOOP`, not an
//!       `EDGE_LOOP`, and this file has no `ORIENTED_EDGE` for a validator to check one with. If
//!       either is refused, `FACE_SURFACE` is a one-line swap and this is the reason to make it.
//! - [ ] Does it reject an `AXIS2_PLACEMENT_3D` whose `location` is not a `CARTESIAN_POINT`, or
//!       accept one? This file writes `CARTESIAN_POINT`s there and both test files insist on it, so
//!       a reader that objects is reading the attribute more strictly than we are.
//! - [ ] Does it need `APPLICATION_PROTOCOL_DEFINITION` / `MECHANICAL_CONTEXT`?
//! - [ ] Does it report the unit as **millimetre**, given an empty `LENGTH_UNIT` name?
//! - [ ] For a face with a hole (a washer): are the **`FACE_BOUND`s read as holes**?
//! - [ ] For a face split into two disjoint regions: is the second region read, or punched through?
//! - [ ] Does the reader reject two `VERTEX_POINT`s a hair apart as coincident? (`Topology::build`
//!       welds on a tolerance grid, and a corner a whisker either side of a cell boundary can be
//!       written twice; `weld_edges` is the cure, and it is the caller's job.)
//! - [ ] Does anything object to a `POLY_LOOP` whose points are not strictly distinct, or to a
//!       self-intersecting loop? **Neither is checked by the writer**: it trusts the kernel's
//!       loops, and [`Solid::validate`] proves closure and orientation, not simplicity.
//!
//! # The parts that are *not* on that list, because they are checked here
//!
//! Every other property of the output is asserted by this module's tests and by
//! `tests/step_interop.rs`: the section layout, sequential ids from `#1`, no dangling or forward
//! reference, exactly two entry points, the entity table of every keyword, **what every referenced
//! attribute actually names** (an `axis2_placement_3d.location` is a `cartesian_point`, because a
//! `vertex_point` is a topological vertex and not a point in space — this file was wrong about that
//! and both test files now pin it), the `CLOSED_SHELL` face count against the solid's own, each
//! face's loop count and vertex count, every boundary edge used once in each direction, every loop
//! vertex on its own face's plane within the declared uncertainty, every `LINE` passing through
//! both of its vertices, every edge curve referenced by exactly one composite segment, the winding
//! of every bound against its own face normal, and the volume recomputed from the written
//! coordinates. Orientation is preserved rather than re-derived:
//! the loops go into the file as the kernel wound them and each face's `PLANE` axis is its own
//! normal, so a shell that is open or inside-out is written faithfully and uselessly — run
//! [`Solid::validate`] first.
//!
//! [`FACETED_BREP`]: https://www.step.org/

use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt::{self, Write as _};

use cadcraft_geom::Vec3;

use crate::{Solid, SolidError, solid::vkey};

/// The schema written into `FILE_SCHEMA`, byte for byte what a reader compares it against.
///
/// A separate function rather than a private constant so a caller can assert on the file it just
/// wrote without repeating a string that has to match exactly.
pub fn step_schema() -> &'static str {
    "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }"
}

/// Entities one file may hold.
///
/// A solid's entity count is a small multiple of its edge count — a cube of twelve edges is about
/// a hundred lines — so this is far above anything that fits in memory anyway. It is a backstop
/// against a hostile loop count turning into gigabytes of text, not a tuning knob.
const MAX_STEP_ENTITIES: usize = 2_000_000;

/// Write `solid` as an ISO-10303-21 part 21 file holding one `FACETED_BREP`.
///
/// `name` becomes the product name and the file name; it is sanitized before it reaches a string
/// literal, because part 21 has no escape for a quote and one stray quote would end the literal and
/// corrupt every statement after it.
///
/// Orientation is **preserved, not re-derived**: the loops go into the file exactly as the kernel
/// wound them (outer counter-clockwise, holes clockwise, material on the left), and each face's
/// `PLANE` axis is its own normal. Run [`Solid::validate`] first if the solid came from outside
/// the kernel — an open or inside-out shell is written as it is, faithfully and uselessly.
///
/// Errors, never a panic: an empty solid, a non-finite coordinate, a face with no area, an edge
/// shorter than the tolerance, or a solid whose entity count passes the internal budget of
/// `MAX_STEP_ENTITIES`. Nothing here is time- or hash-dependent, so the same solid writes the same
/// bytes every time.
pub fn step_faceted_brep(solid: &Solid, name: &str) -> Result<String, SolidError> {
    if solid.faces.is_empty() {
        return Err(SolidError::Empty);
    }
    let topo = Topology::build(solid)?;
    let name = clean_name(name);
    let tol = solid.tolerance();
    let mut w = Writer::new();
    let mut dirs = Directions::default();

    // 0. Part 21's marker and the header, which is what a reader looks at before it reads any
    //    geometry. The time stamp is left empty on purpose: a clock would make two exports of one
    //    solid differ, and an export that reproduces byte for byte is worth more than a timestamp
    //    nobody reads.
    w.line(format_args!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('CADCraft faceted B-rep: {} faces, {} edges'),'2;1');\nFILE_NAME('{name}','',(),(),'CADCraft','CADCraft','');\nFILE_SCHEMA(('{}'));\nENDSEC;\nDATA;\n",
        topo.faces.len(),
        topo.edges.len(),
        step_schema()
    ))?;

    // 1. The context and units every later entity names. Written first, referred to last: every
    //    entity is written before anything names it, so no reference points forward and a reader
    //    never has to buffer the DATA section to resolve an id.
    let app = w.entity("APPLICATION_CONTEXT('core data for automotive mechanical design processes')")?;
    let length = w.entity("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))")?;
    let angle = w.entity("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))")?;
    let solid_angle = w.entity("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())")?;
    // The file's declared uncertainty is the kernel's working tolerance rather than a round number,
    // so a reader is told exactly the distance within which two points of this shell are the same
    // point — which is the distance they were welded at.
    let unc = w.entity(&format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),#{length},'distance_accuracy_value','the solid kernel working tolerance')",
        real(tol)
    ))?;
    let ctx = w.entity(&format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle}))REPRESENTATION_CONTEXT('','3D'))"
    ))?;

    // 2. One CARTESIAN_POINT and one VERTEX_POINT per corner of the shell. A reader rejects two
    //    vertices at the same place, so this table is the whole reason a solid with holes in its
    //    faces can be written at all: the corner belongs to the solid, not to a face.
    //
    //    The two ids are kept side by side rather than derived. `point_id[i]` is the
    //    `CARTESIAN_POINT` and `vertex[i]` is the `VERTEX_POINT` standing on it, and the face
    //    placements below need the first of those: `axis2_placement_3d.location` names a
    //    `cartesian_point`, and a `vertex_point` is a topological vertex, not a point in space. The
    //    two happen to differ by one because they are written in step, which is exactly the kind of
    //    coincidence a file should not rest on.
    let mut point_id = Vec::with_capacity(topo.points.len());
    let mut vertex = Vec::with_capacity(topo.points.len());
    for p in &topo.points {
        let c = w.entity(&format!("CARTESIAN_POINT('',({},{},{}))", real(p.x), real(p.y), real(p.z)))?;
        point_id.push(c);
        vertex.push(w.entity(&format!("VERTEX_POINT('',#{c})"))?);
    }

    // 3. One EDGE_CURVE per distinct edge, shared by the two faces that meet along it, each one a
    //    LINE from its start vertex's point along a VECTOR. `same_sense` is `.T.` because the
    //    vector points from the start vertex to the end one.
    let mut edge = Vec::with_capacity(topo.edges.len());
    for (a, b) in &topo.edges {
        // `start`/`end` are the topological `VERTEX_POINT`s that `EDGE_CURVE` names, and
        // `line_start`/`line_end` are the geometric `CARTESIAN_POINT`s that the `LINE` passing
        // through that edge actually sits on. `LINE`'s point attribute is a geometric point, not a
        // topological vertex — writing a `VERTEX_POINT` there is a type error of exactly the kind the
        // second interop parser's reference-typing table rejects.
        let (Some(&start), Some(&end), Some(&line_start), Some(p), Some(q)) =
            (vertex.get(*a), vertex.get(*b), point_id.get(*a), topo.points.get(*a), topo.points.get(*b))
        else {
            continue;
        };
        let delta = *q - *p;
        let len = delta.len();
        if !len.is_finite() || len <= tol {
            // A LINE with no length has no direction to name, and an edge shorter than the
            // tolerance is a welding artefact rather than geometry.
            return Err(SolidError::Invalid(format!("edge {p:?} -> {q:?} is {len} long, below the tolerance {tol}")));
        }
        let dir = dirs.id(&mut w, delta / len)?;
        let vector = w.entity(&format!("VECTOR('',#{dir},{})", real(len)))?;
        let line = w.entity(&format!("LINE('',#{line_start},#{vector})"))?;
        edge.push(w.entity(&format!("EDGE_CURVE('',#{start},#{end},#{line},.T.)"))?);
    }

    // 4. Loops, bounds, and the planar surface each face sits in.
    let mut faces = Vec::with_capacity(solid.faces.len());
    for (face, loops) in solid.faces.iter().zip(&topo.faces) {
        let mut bounds = Vec::with_capacity(loops.len());
        for (i, l) in loops.iter().enumerate() {
            let loop_refs: Vec<String> = l.iter().filter_map(|p| vertex.get(*p)).map(|id| format!("#{id}")).collect();
            if loop_refs.len() != l.len() {
                return Err(SolidError::Invalid("a face loop lost a vertex".into()));
            }
            let poly = w.entity(&format!("POLY_LOOP('',({}))", loop_refs.join(",")))?;
            bounds.push(if i == 0 {
                w.entity(&format!("FACE_OUTER_BOUND('',#{poly},.T.)"))?
            } else {
                w.entity(&format!("FACE_BOUND('',#{poly},.T.)"))?
            });
        }
        // The placement sits on the face's own first corner, so it reuses that corner's
        // CARTESIAN_POINT instead of adding a second point at the same place. It has to be the
        // CARTESIAN_POINT and not the VERTEX_POINT standing on it: `axis2_placement_3d.location` is
        // a `cartesian_point`, and pointing it at a topological vertex was a type error that a
        // graph-walking reader never sees and a schema-checking reader rejects on the spot.
        let anchor = loops.first().and_then(|l| l.first()).copied();
        let anchor_id = anchor.and_then(|p| point_id.get(p).copied());
        let Some(anchor_id) = anchor_id else {
            return Err(SolidError::Invalid("a face has no loop to place it by".into()));
        };
        let axis = dirs.id(&mut w, face.plane.normal)?;
        let ref_dir = dirs.id(&mut w, face.plane.u)?;
        let placement = w.entity(&format!("AXIS2_PLACEMENT_3D('',#{anchor_id},#{axis},#{ref_dir})"))?;
        let surface = w.entity(&format!("PLANE('',#{placement})"))?;
        faces.push(w.entity(&format!("ADVANCED_FACE('',({}),#{surface},.T.)", refs(&bounds)))?);
    }

    // 5. The shell and the solid.
    let shell = w.entity(&format!("CLOSED_SHELL('',({}))", refs(&faces)))?;
    let brep = w.entity(&format!("FACETED_BREP('{name}',#{shell})"))?;

    // 6. The edge curves need a home. A `POLY_LOOP` names its vertices and never its edges, so an
    //    `EDGE_CURVE` written for a faceted shell is referenced by nothing at all — which strict
    //    readers reject outright. An edge-based wireframe representation is what STEP has for this:
    //    one composite curve per edge, hung off the same product definition as the solid.
    let mut curves = Vec::with_capacity(edge.len());
    for e in &edge {
        let segment = w.entity(&format!("COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#{e})"))?;
        curves.push(w.entity(&format!("COMPOSITE_CURVE('',(#{segment}),.F.)"))?);
    }
    let curve_set = w.entity(&format!("GEOMETRIC_CURVE_SET('edges',({}))", refs(&curves)))?;
    let wire = w.entity(&format!("GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION('edges',(#{curve_set}),#{ctx})"))?;

    // 7. The solid's own representation, with the origin placement every shape representation
    //    carries so a reader knows where the world axes are.
    let origin_point = w.entity(&format!("CARTESIAN_POINT('',({},{},{}))", real(0.0), real(0.0), real(0.0)))?;
    let z_axis = dirs.id(&mut w, Vec3::Z)?;
    let x_axis = dirs.id(&mut w, Vec3::X)?;
    let origin = w.entity(&format!("AXIS2_PLACEMENT_3D('',#{origin_point},#{z_axis},#{x_axis})"))?;
    let rep = w.entity(&format!("ADVANCED_BREP_SHAPE_REPRESENTATION('{name}',(#{origin},#{brep}),#{ctx})"))?;

    // 8. The product structure, which is what a reader starts from: it walks down to the shape
    //    representations, and those are the only two entities in the file that nothing names.
    let product_ctx = w.entity(&format!("PRODUCT_CONTEXT('',#{app},'mechanical')"))?;
    let product = w.entity(&format!("PRODUCT('{name}','{name}','',(#{product_ctx}))"))?;
    let formation = w.entity(&format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"))?;
    let def_ctx = w.entity(&format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"))?;
    let definition = w.entity(&format!("PRODUCT_DEFINITION('design','',#{formation},#{def_ctx})"))?;
    let shape = w.entity(&format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"))?;
    w.entity(&format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"))?;
    w.entity(&format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{wire})"))?;

    w.line(format_args!("ENDSEC;\nEND-ISO-10303-21;\n"))?;
    Ok(w.out)
}

/// The shell as plain lists: corners, edges and faces, each shared and written once.
///
/// The kernel's faces share no vertices — the boundary between two faces simply *is* the shared
/// edge — so a file has to weld them itself. The key is [`vkey`], the same tolerance grid
/// [`Solid::validate`] counts edges on: if the shell closed for the kernel it closes for this
/// table, and if it did not, the two disagree loudly instead of writing a file whose corners
/// double back on themselves.
///
/// A vertex sitting within a whisker of a cell boundary can still land in two cells and be written
/// twice. That is inherent to welding on a grid, the kernel's own weld has it, and the cure is the
/// same one: `weld_edges` the faces before exporting a solid that came out of a boolean.
struct Topology {
    /// Distinct corners, in the order the loops first name them.
    points: Vec<Vec3>,
    /// Distinct directed edges as indices into `points`.
    edges: Vec<(usize, usize)>,
    /// One entry per face: its loops, each a list of indices into `points`.
    faces: Vec<Vec<Vec<usize>>>,
}

impl Topology {
    fn build(solid: &Solid) -> Result<Topology, SolidError> {
        let tol = solid.tolerance();
        let origin = solid.bounds().min;
        let mut topo = Topology { points: Vec::new(), edges: Vec::new(), faces: Vec::new() };
        // `BTreeMap` rather than `HashMap` for the same reason `Solid::validate` uses one: hash
        // order changes run to run, and a file whose bytes change run to run is not reproducible.
        let mut point_id: BTreeMap<(i64, i64, i64), usize> = BTreeMap::new();
        let mut edge_id: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for face in &solid.faces {
            let mut loops: Vec<Vec<usize>> = Vec::new();
            for loop_pts in face.region.loops() {
                if loop_pts.len() < 3 {
                    return Err(SolidError::EmptyProfile);
                }
                let mut here = Vec::with_capacity(loop_pts.len());
                for p in loop_pts {
                    let world = face.plane.to_world(p.x, p.y);
                    // Checked before the point is welded: a NaN would key to cell 0 and land in
                    // the file as `NaN`, which is not a number any reader accepts.
                    if !world.is_finite() {
                        return Err(SolidError::NonFinite);
                    }
                    let key = vkey(world, origin, tol);
                    let at = match point_id.get(&key) {
                        Some(i) => *i,
                        None => {
                            let i = topo.points.len();
                            topo.points.push(world);
                            point_id.insert(key, i);
                            i
                        }
                    };
                    here.push(at);
                }
                loops.push(here);
            }
            // A face thinner than the tolerance is not a face: the kernel cannot resolve it, so
            // writing it would claim a precision the model does not have.
            let area = face.area();
            if !area.is_finite() || area <= tol * tol {
                return Err(SolidError::EmptyProfile);
            }
            // The loops become edges as they stand, which is what keeps the winding: the file gets
            // the shell's own orientation, not one derived from a normal that may disagree.
            for l in &loops {
                let n = l.len();
                if n < 3 {
                    continue;
                }
                for i in 0..n {
                    let (Some(&a), Some(&b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
                    match edge_id.entry((a.min(b), a.max(b))) {
                        Entry::Occupied(_) => {}
                        Entry::Vacant(slot) => {
                            slot.insert(topo.edges.len());
                            topo.edges.push((a, b));
                        }
                    }
                }
            }
            topo.faces.push(loops);
        }
        if topo.faces.is_empty() {
            return Err(SolidError::Empty);
        }
        Ok(topo)
    }
}

/// Unit directions, interned: a cube has twelve edges but three directions, and every face's
/// placement wants the same few again.
#[derive(Default)]
struct Directions {
    id_by_key: BTreeMap<(i64, i64, i64), usize>,
}

impl Directions {
    /// The entity id for direction `d`, writing a `DIRECTION` the first time it is asked for.
    ///
    /// Keyed on a 1e-9 grid: two directions closer than that are the same direction as far as a
    /// reader measuring to the file's own uncertainty is concerned, and the stored vector is always
    /// a real one, never a blend of two.
    fn id(&mut self, w: &mut Writer, d: Vec3) -> Result<usize, SolidError> {
        if !d.is_finite() {
            return Err(SolidError::NonFinite);
        }
        let key = ((d.x * 1e9).round() as i64, (d.y * 1e9).round() as i64, (d.z * 1e9).round() as i64);
        if let Some(id) = self.id_by_key.get(&key) {
            return Ok(*id);
        }
        let id = w.entity(&format!("DIRECTION('',({},{},{}))", real(d.x), real(d.y), real(d.z)))?;
        self.id_by_key.insert(key, id);
        Ok(id)
    }
}

/// The file under construction: entity ids are handed out in emission order and never reused.
#[derive(Default)]
struct Writer {
    out: String,
    /// The next id. Ids start at 1: part 21 reserves `#0`, and a file that hands it out is read by
    /// some importers and rejected by others.
    next: usize,
}

impl Writer {
    /// A writer whose first entity is `#1`.
    fn new() -> Writer {
        Writer { out: String::new(), next: 1 }
    }

    /// Write one entity instance and hand back its id.
    fn entity(&mut self, body: &str) -> Result<usize, SolidError> {
        if self.next > MAX_STEP_ENTITIES {
            return Err(SolidError::Invalid(format!("the STEP file would need more than {MAX_STEP_ENTITIES} entities")));
        }
        let id = self.next;
        self.next += 1;
        self.line(format_args!("#{id}={body};\n"))?;
        Ok(id)
    }

    /// Append text. `String`'s formatting cannot fail, but the result is checked rather than
    /// dropped: a silently unwritten line in a file format is a corrupt file, not a smaller one.
    fn line(&mut self, args: fmt::Arguments<'_>) -> Result<(), SolidError> {
        self.out.write_fmt(args).map_err(|_| SolidError::Invalid("writing the STEP file failed".into()))
    }
}

/// `(a,b,c)` for a list of entity ids.
fn refs(ids: &[usize]) -> String {
    ids.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(",")
}

/// A part 21 real.
///
/// Rust writes `1` and `0` where the format needs `1.` and `0.`, and part 21 has no integer where a
/// real belongs. `{}` is also the shortest form that reads back as the same `f64`, so a coordinate
/// survives the round trip exactly — which is what lets a test measure the geometry it wrote.
fn real(x: f64) -> String {
    let s = format!("{x}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// A name safe to put inside a part 21 string literal.
///
/// The name is the caller's — a block name, a layer name — and part 21 cannot escape a quote, so a
/// name carrying one would end the literal early and turn every later statement into rubbish.
/// Everything outside a conservative set is dropped, and a name left empty becomes `cadcraft`.
fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || " _-.:".contains(*c)).take(120).collect();
    if cleaned.trim().is_empty() { "cadcraft".to_string() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Face;
    use crate::Plane;
    use crate::extrude;
    use crate::tests_support::{box_at, must, rect, rect_with_hole};

    /// Where an expected token is, so a test that fails on a missing one says so plainly.
    fn at(text: &str, needle: &str) -> usize {
        match text.find(needle) {
            Some(i) => i,
            None => panic!("{needle:?} is missing from the file"),
        }
    }

    // ---- the mini reader ----
    //
    // No third-party STEP parser was available when this writer was written, so these tests read
    // the file back themselves. A STEP file is a graph of numbered entities, and the classic way a
    // hand-written exporter fails is not bad syntax but a graph that does not hang together: a
    // `#n` nothing defines, or an entity nothing refers to. Both are silent — the file looks
    // right and imports as nothing at all — which is exactly why they are checked here.

    /// The DATA section read back as a graph.
    #[derive(Default)]
    struct Graph {
        /// id -> body, in file order.
        defs: BTreeMap<usize, String>,
        /// The ids as written, so a definition repeated under one id can be seen.
        order: Vec<usize>,
        /// id -> every id its body names.
        out: BTreeMap<usize, Vec<usize>>,
    }

    impl Graph {
        fn body(&self, id: usize) -> &str {
            match self.defs.get(&id) {
                Some(b) => b.as_str(),
                None => panic!("#{id} is referenced but never defined"),
            }
        }

        /// The ids of every entity of one type, in file order.
        fn of_type(&self, kind: &str) -> Vec<usize> {
            self.order.iter().copied().filter(|id| self.kind_of(*id) == kind).collect()
        }

        fn count(&self, kind: &str) -> usize {
            self.of_type(kind).len()
        }

        /// The type name of an entity: up to its first `(`, with a complex entity's leading `(` skipped.
        fn kind_of(&self, id: usize) -> &str {
            let body = self.body(id);
            let head = body.trim_start().trim_start_matches('(').trim_start();
            match head.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                Some(i) => &head[..i],
                None => head,
            }
        }

        /// The ids nothing in the file names: the entry points a reader starts from.
        fn roots(&self) -> Vec<usize> {
            let mut named = std::collections::BTreeSet::new();
            for ids in self.out.values() {
                named.extend(ids.iter().copied());
            }
            self.order.iter().copied().filter(|id| !named.contains(id)).collect()
        }

        /// The loops of a face, each with its bound's type (`FACE_OUTER_BOUND` or `FACE_BOUND`).
        fn bounds_of(&self, face: usize) -> Vec<(String, usize)> {
            self.out[&face]
                .iter()
                .filter(|id| self.kind_of(**id).starts_with("FACE_BOUND") || self.kind_of(**id) == "FACE_OUTER_BOUND")
                .map(|b| (self.kind_of(*b).to_string(), *b))
                .collect()
        }

        /// The points of a `POLY_LOOP`, followed through `VERTEX_POINT` to the numbers in the file.
        fn loop_points(&self, poly_loop: usize) -> Vec<Vec3> {
            self.out[&poly_loop]
                .iter()
                .map(|v| {
                    let point = match self.out.get(v).and_then(|p| p.first()) {
                        Some(p) => *p,
                        None => panic!("#{v} is not a vertex with a point"),
                    };
                    let [x, y, z] = self.coords_of(point);
                    Vec3::new(x, y, z)
                })
                .collect()
        }

        /// The coordinate triple of a `CARTESIAN_POINT`, read from the text.
        fn coords_of(&self, point: usize) -> [f64; 3] {
            match coordinates(self.body(point)) {
                Some(c) => c,
                None => panic!("#{point} has no coordinates: {}", self.body(point)),
            }
        }

        /// The outward normal a face was written with: face -> `PLANE` -> placement -> `DIRECTION`.
        fn normal_of(&self, face: usize) -> Vec3 {
            let plane = match self.out[&face].iter().find(|id| self.kind_of(**id) == "PLANE") {
                Some(p) => *p,
                None => panic!("#{face} has no PLANE"),
            };
            let placement = match self.out[&plane].first() {
                Some(p) => *p,
                None => panic!("#{plane} has no placement"),
            };
            let axis = match self.out.get(&placement).and_then(|a| a.get(1)) {
                Some(a) => *a,
                None => panic!("#{placement} has no axis"),
            };
            let [x, y, z] = self.coords_of(axis);
            Vec3::new(x, y, z)
        }
    }

    /// Split a part 21 file into `#N=body;` statements.
    ///
    /// `;` ends a statement but may also sit inside a string literal, so the scan tracks quotes:
    /// a product name carrying one would otherwise cut the file in half and every entity after it
    /// would look undefined.
    fn parse(text: &str) -> Graph {
        let mut graph = Graph::default();
        let data = match text.find("DATA;").map(|s| &text[s + 5..]) {
            Some(d) => d,
            None => return graph,
        };
        let data = data.split("ENDSEC;").next().unwrap_or(data);
        for stmt in split_statements(data) {
            let stmt = stmt.trim();
            let Some(rest) = stmt.strip_prefix('#') else { continue };
            let Some((id, body)) = rest.split_once('=') else { continue };
            let Ok(id) = id.trim().parse::<usize>() else { continue };
            graph.out.insert(id, references(body));
            graph.order.push(id);
            graph.defs.entry(id).or_insert_with(|| body.to_string());
        }
        graph
    }

    /// Split on `;`, never inside a part 21 string literal.
    fn split_statements(data: &str) -> Vec<&str> {
        let bytes = data.as_bytes();
        let mut out = Vec::new();
        let mut start = 0;
        let mut in_string = false;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\'' if in_string && bytes.get(i + 1) == Some(&b'\'') => i += 1,
                b'\'' => in_string = !in_string,
                b';' if !in_string => {
                    out.push(&data[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        if start < data.len() {
            out.push(&data[start..]);
        }
        out
    }

    /// Every `#n` in a body, in the order they appear.
    fn references(body: &str) -> Vec<usize> {
        let bytes = body.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let mut digits = String::new();
                while let Some(c) = bytes.get(i + 1 + digits.len()).copied()
                    && c.is_ascii_digit()
                {
                    digits.push(c as char);
                }
                if let Ok(n) = digits.parse::<usize>() {
                    out.push(n);
                    i += 1 + digits.len();
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// An entity's argument list, split on the commas outside nested parentheses and strings.
    fn arguments(body: &str) -> Vec<String> {
        let Some(open) = body.find('(') else { return Vec::new() };
        let inner = body.get(open + 1..body.rfind(')').unwrap_or(body.len())).unwrap_or("");
        let bytes = inner.as_bytes();
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut start = 0;
        for (i, c) in inner.char_indices() {
            match c {
                '\'' if in_string && bytes.get(i + 1) == Some(&b'\'') => continue,
                '\'' => in_string = !in_string,
                '(' if !in_string => depth += 1,
                ')' if !in_string => depth = depth.saturating_sub(1),
                ',' if !in_string && depth == 0 => {
                    out.push(inner[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(inner.get(start..).unwrap_or("").trim().to_string());
        out
    }

    /// The three numbers of a `CARTESIAN_POINT` body, parsed out of the written text.
    fn coordinates(body: &str) -> Option<[f64; 3]> {
        let list = arguments(body).into_iter().nth(1)?;
        let list = list.trim().trim_start_matches('(').trim_end_matches(')');
        let nums: Vec<f64> = list.split(',').map(|n| n.trim().parse::<f64>().ok()).collect::<Option<Vec<_>>>()?;
        match nums.as_slice() {
            [x, y, z] => Some([*x, *y, *z]),
            _ => None,
        }
    }

    /// Signed volume of a closed loop, by the tetrahedra from the world origin.
    ///
    /// A hole is wound the other way, so it subtracts without any special case — which is what
    /// makes this usable on a face with holes as well as one without.
    fn loop_volume(points: &[Vec3]) -> f64 {
        let n = points.len();
        if n < 3 {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 1..n.saturating_sub(1) {
            let (Some(a), Some(b), Some(c)) = (points.first(), points.get(i), points.get(i + 1)) else { continue };
            total += a.dot(b.cross(*c)) / 6.0;
        }
        total
    }

    /// The volume of the solid as the file states it: every loop of every face, in the file.
    fn volume_of_file(g: &Graph) -> f64 {
        let mut total = 0.0;
        for face in g.of_type("ADVANCED_FACE") {
            for (_, bound) in g.bounds_of(face) {
                for poly in &g.out[&bound] {
                    total += loop_volume(&g.loop_points(*poly));
                }
            }
        }
        total
    }

    /// Does a loop wind counter-clockwise about `normal`, as the kernel's convention demands?
    ///
    /// The kernel puts the material on a loop's left, so an outer loop reads counter-clockwise seen
    /// from outside and a hole clockwise. A writer that reversed either would produce a file that
    /// looks perfect and a solid pointing the wrong way.
    fn winds_with(points: &[Vec3], normal: Vec3) -> bool {
        let n = points.len();
        let Some(c) = points.first().copied() else { return false };
        let mut acc = Vec3::ZERO;
        for i in 0..n {
            let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % n)) else { continue };
            acc = acc + (*a - c).cross(*b - c);
        }
        acc.dot(normal) > 0.0
    }

    #[test]
    fn a_part_21_file_has_the_sections_the_format_asks_for() {
        let s = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let text = must(step_faceted_brep(&s, "cube"), "export");
        assert!(text.starts_with("ISO-10303-21;\n"), "part 21 opens with its own marker");
        assert!(text.ends_with("END-ISO-10303-21;\n"), "and closes with it");
        assert_eq!(text.matches("HEADER;").count(), 1);
        assert_eq!(text.matches("DATA;").count(), 1);
        // `ENDSEC;` closes a section, and this file has two of them, so it appears twice — once
        // after the header, once after the data. A file with one is missing a section; a file with
        // three has a stray one.
        assert_eq!(text.matches("ENDSEC;").count(), 2);
        assert!(at(&text, "HEADER;") < at(&text, "FILE_DESCRIPTION"), "the header names what the file holds");
        assert!(at(&text, "FILE_DESCRIPTION") < at(&text, "FILE_NAME"));
        assert!(at(&text, "FILE_NAME") < at(&text, "FILE_SCHEMA"));
        assert!(at(&text, "FILE_SCHEMA") < at(&text, "ENDSEC;"), "the schema comes last in the header");
        assert!(at(&text, "ENDSEC;") < at(&text, "DATA;"), "the data section follows the header");
        assert!(at(&text, "DATA;") < text.rfind("ENDSEC;").unwrap_or(0), "and the data section is closed");
        assert!(text.contains(&format!("FILE_SCHEMA(('{}'))", step_schema())), "the schema a reader checks is the one we promise");
    }

    #[test]
    fn the_entity_graph_has_no_dangling_ids() {
        let s = box_at(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0));
        let text = must(step_faceted_brep(&s, "box"), "export");
        let g = parse(&text);

        // Ids are sequential from 1 and defined once each: a repeated `#7=` would be silently
        // merged by the map, so the count of ids written is compared with the count of them read.
        assert!(!g.order.is_empty(), "the data section parsed to nothing");
        assert_eq!(g.order.len(), g.defs.len(), "some id is defined twice");
        assert_eq!(g.order[0], 1, "ids start at 1");
        assert!(g.order.windows(2).all(|w| w[0] < w[1]), "ids are written in order, so the file can be read top to bottom");

        // Every id named by a body exists, and was written before the body that names it.
        for (id, refs) in &g.out {
            for r in refs {
                assert!(g.defs.contains_key(r), "#{id} names #{r}, which no entity defines");
                assert!(*r < *id, "#{id} names #{r}, which is written after it: references run backwards");
            }
        }

        // And every entity is reachable: nothing is written that nothing names. A part 21 file is a
        // forest, not a single tree — the shape representations are its roots, because that is
        // where a reader starts — so those, and only those, may go unnamed.
        let roots = g.roots();
        assert_eq!(roots.len(), 2, "the solid's representation and its wireframe are the entry points, got {roots:?}");
        for r in &roots {
            assert_eq!(g.kind_of(*r), "SHAPE_DEFINITION_REPRESENTATION", "#{r} is an entry point nothing leads to");
        }
        let named = g.order.len() - roots.len();
        assert_eq!(named, 131, "the cube is 131 named entities; a stray edge curve or a point nothing refers to would move this");
    }

    #[test]
    fn a_cube_writes_one_solid_with_eight_vertices_and_twelve_edges() {
        let s = box_at(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0));
        let text = must(step_faceted_brep(&s, "cube"), "export");
        let g = parse(&text);
        assert_eq!(g.count("FACETED_BREP"), 1, "one solid");
        assert_eq!(g.count("CLOSED_SHELL"), 1, "one shell");
        assert_eq!(g.count("ADVANCED_FACE"), 6, "six faces");
        assert_eq!(g.count("FACE_OUTER_BOUND"), 6, "one outer bound each");
        assert_eq!(g.count("FACE_BOUND"), 0, "and no inner bounds: a cube has no holes");
        assert_eq!(g.count("VERTEX_POINT"), 8, "eight corners, each written once however many faces touch it");
        assert_eq!(
            g.count("CARTESIAN_POINT"),
            9,
            "the faces' placements reuse those eight; only the shape representation adds one, for the world origin"
        );
        assert_eq!(g.count("EDGE_CURVE"), 12, "twelve edges, shared by the two faces that meet along each");
        assert_eq!(g.count("POLY_LOOP"), 6, "one loop per face: a cube has no holes, so no face carries a second");
        for poly in g.of_type("POLY_LOOP") {
            assert_eq!(g.out[&poly].len(), 4, "a square face, four corners");
        }
        assert_eq!(g.count("PLANE"), 6, "every face is planar");

        // The volume, measured from the numbers that were written rather than from the solid: a
        // writer that dropped a face, reversed a loop or rounded a coordinate shows up here.
        let from_file = volume_of_file(&g);
        assert!((from_file - s.volume()).abs() < 1e-9, "the file says {from_file}, the solid says {}", s.volume());
        assert!((from_file - 48.0).abs() < 1e-9, "a 2x4x6 box is 48, got {from_file}");

        // And every face winds the way its own normal says it must, which is the orientation
        // convention surviving into the file rather than being re-derived by it.
        for face in g.of_type("ADVANCED_FACE") {
            let normal = g.normal_of(face);
            assert!((normal.len() - 1.0).abs() < 1e-12, "a face normal is a unit vector, got {normal:?}");
            for (kind, bound) in g.bounds_of(face) {
                for poly in &g.out[&bound] {
                    let winding = winds_with(&g.loop_points(*poly), normal);
                    assert_eq!(winding, kind == "FACE_OUTER_BOUND", "a {kind} of a face with normal {normal:?} winds the wrong way");
                }
            }
        }
    }

    #[test]
    fn a_face_placement_sits_on_a_cartesian_point_and_not_on_a_vertex() {
        // `axis2_placement_3d.location` is a `cartesian_point`. A `vertex_point` is a topological
        // vertex, not a point in space, so a reader that type-checks the attribute rejects the
        // entity — while a reader that only walks the graph sees nothing wrong at all, which is how
        // this shipped wrong in the first place.
        let s = box_at(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0));
        let text = must(step_faceted_brep(&s, "box"), "export");
        let g = parse(&text);

        let faces = g.of_type("ADVANCED_FACE");
        assert_eq!(faces.len(), 6, "a cube has six faces");
        for face in faces {
            let Some(plane) = g.out.get(&face).and_then(|refs| refs.iter().copied().find(|id| g.kind_of(*id) == "PLANE")) else {
                panic!("#{face} has no PLANE");
            };
            let Some(&placement) = g.out.get(&plane).and_then(|refs| refs.first()) else {
                panic!("#{plane} has no placement");
            };
            let Some(&location) = g.out.get(&placement).and_then(|refs| refs.first()) else {
                panic!("#{placement} has no location");
            };
            assert_eq!(
                g.kind_of(location),
                "CARTESIAN_POINT",
                "#{placement}'s location is a {}, and axis2_placement_3d.location is a cartesian_point",
                g.kind_of(location)
            );

            // And it is the face's own first corner, sharing the corner's point rather than adding
            // a second one at the same place. Exact equality is the right comparison here and not
            // merely a convenient one: both numbers came out of the same `f64` through the same
            // formatter, so any difference at all would be a real one.
            let Some(&(kind, outer)) = g.bounds_of(face).first() else {
                panic!("#{face} has no bound");
            };
            assert_eq!(kind, "FACE_OUTER_BOUND", "a cube's only bound is an outer one");
            let Some(&poly) = g.out.get(&outer).and_then(|refs| refs.first()) else {
                panic!("#{outer} has no loop");
            };
            let Some(corner) = g.loop_points(poly).first().copied() else {
                panic!("#{poly} has no first vertex");
            };
            let [x, y, z] = g.coords_of(location);
            assert_eq!(
                (x, y, z),
                (corner.x, corner.y, corner.z),
                "#{placement} sits at ({x}, {y}, {z}) rather than on the face's own first corner {corner:?}"
            );
        }

        // Sharing is the point: the placements add no points of their own.
        assert_eq!(
            g.count("CARTESIAN_POINT"),
            9,
            "the face placements reuse the eight corners; only the shape representation adds one, for the world origin"
        );
    }

    #[test]
    fn a_face_with_a_hole_writes_both_of_its_loops() {
        // An extruded washer: the top and the bottom each carry a hole, so both loops of those
        // faces have to reach the file or the solid imports as a slab.
        let profile = rect_with_hole(0.0, 0.0, 10.0, 10.0, 3.0, 3.0, 7.0, 7.0);
        let s = must(extrude(&profile, &Plane::xy(0.0), 4.0), "extrude");
        let text = must(step_faceted_brep(&s, "washer"), "export");
        let g = parse(&text);

        let holed: Vec<usize> = g.of_type("ADVANCED_FACE").into_iter().filter(|f| g.bounds_of(*f).len() == 2).collect();
        assert_eq!(holed.len(), 2, "the top and the bottom of a washer each have two loops, got {holed:?}");
        for face in holed {
            let bounds = g.bounds_of(face);
            assert_eq!(bounds[0].0, "FACE_OUTER_BOUND");
            assert_eq!(bounds[1].0, "FACE_BOUND", "a hole is an inner bound, not a second outer one");
            let outer = g.loop_points(*g.out[&bounds[0].1].first().unwrap_or(&usize::MAX));
            let inner = g.loop_points(*g.out[&bounds[1].1].first().unwrap_or(&usize::MAX));
            assert_eq!(outer.len(), 4);
            assert_eq!(inner.len(), 4);
            assert!(!winds_with(&inner, g.normal_of(face)), "a hole winds the other way from the loop around it");
            assert!(winds_with(&outer, g.normal_of(face)), "the outer loop is the one that winds with the normal");
        }
        assert_eq!(g.count("FACE_BOUND"), 2, "two inner bounds in the whole solid");
        // 10 * 10 - 4 * 4 = 84 across, 4 thick: 336 of material.
        assert!((volume_of_file(&g) - 336.0).abs() < 1e-9, "the washer is 336, the file says {}", volume_of_file(&g));
    }

    #[test]
    fn the_same_solid_writes_the_same_bytes_twice() {
        // A STEP file is text, so reproducibility is a property of the writer and not of any
        // hashing: no clock, no hash order, no allocation address anywhere in the output. The
        // header's time stamp is deliberately empty for this reason.
        let s = must(extrude(&rect_with_hole(0.0, 0.0, 6.0, 6.0, 1.0, 1.0, 3.0, 3.0), &Plane::xy(0.0), 2.0), "extrude");
        let a = must(step_faceted_brep(&s, "washer"), "first export");
        let b = must(step_faceted_brep(&s, "washer"), "second export");
        assert_eq!(a, b, "two exports of one solid differ");
        assert!(!a.contains('\n') || a.lines().count() > 10, "the file is more than a stub: {} lines", a.lines().count());
    }

    #[test]
    fn hostile_solids_are_errors_not_panics() {
        // An empty solid has nothing to describe.
        assert!(step_faceted_brep(&Solid::default(), "empty").is_err());
        assert!(step_faceted_brep(&Solid::default(), "empty").is_err());

        // A NaN or infinite coordinate cannot reach a solid through `Face::new`, so the face is
        // built by hand: what is under test is that the writer refuses it, not the constructor.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut region = rect(0.0, 0.0, 2.0, 2.0);
            region.0[0].outer[1].y = bad;
            let hostile = Solid { faces: vec![Face { plane: Plane::xy(0.0), region }] };
            assert!(step_faceted_brep(&hostile, "nan").is_err(), "a face holding {bad} was written out");
        }

        // A loop with fewer than three points cannot bound anything, and a face with no area is
        // not a face. Both are refused rather than written as a degenerate loop.
        let mut region = rect(0.0, 0.0, 2.0, 2.0);
        region.0[0].outer = vec![cadcraft_geom::Vec2::ZERO, cadcraft_geom::Vec2::new(1.0, 0.0)];
        let sliver = Solid { faces: vec![Face { plane: Plane::xy(0.0), region }] };
        assert!(step_faceted_brep(&sliver, "sliver").is_err());

        // A name full of quotes cannot be escaped — part 21 has no escape — so it is sanitized
        // rather than allowed to end the string literal and corrupt the file after it.
        let s = box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let nasty = must(step_faceted_brep(&s, "it's a \"box\"; DROP TABLE #7 = ('; garbage"), "export");
        let g = parse(&nasty);
        assert_eq!(g.count("FACETED_BREP"), 1, "the file survives a hostile name");
        assert!(!nasty.contains("= (';"), "a name must not be able to end the string literal it sits in");
        assert!(must(step_faceted_brep(&s, ""), "export").contains("'cadcraft'"), "an empty name still names the product");
    }
}
