# DAT — documents, save/load, interchange, units

Task index for the `DAT` domain (8 % of the programme). Owned crates:
`crates/model/src/file.rs`, `crates/io/**`, and (for the kernel-owned writer)
`crates/solid/src/step.rs`.

**Owner letters** are the wave-1 agents of `docs/master-plan.md` §6: **A** kernel, **Q** quality,
**D** interchange, **U** ui/viewport, **C** orchestrator. Later waves reuse the domain letter
(`REN`, `UI`, `PLT`) or are marked "co-sequenced".

---

## Invariants of this domain

1. **One document model.** `crates/model` owns the document (`Model { parameters, features,
   records }`, `Document`, `FORMAT_VERSION`). `crates/io` owns *file adapters*. A 3D adapter
   takes `&cadcraft_model::Model` and a `&cadcraft_solid::Solid`; it never builds a second tree.
   Every task below says which it uses.
2. **A format task is not a text-file task.** `crates/solid/src/step.rs` states in its own module
   docs that **no third-party STEP reader has ever opened a file it produced**. The claim "CADCraft
   exports STEP" is therefore *unproven* today. In this index, an interchange feature is finished
   only when an **independent implementation has read the file and agreed on the geometry**; that
   is an acceptance criterion on every format task, never an aspiration. The report of that check
   lives in the repo as text (reader name, version, exact command, fixture list, pass count) so
   the claim is checkable by a human without the tool installed.
3. **Every writer states what it does not support.** A refused feature must be a named `Err`,
   never a silently dropped entity and never a panic.
4. **There is exactly one internal length unit: the millimetre.** `crates/solid/src/step.rs`
   documents that the kernel works in millimetres and that the file's `LENGTH_UNIT` is
   `SI_UNIT(.MILLI.,.METRE.)` with an empty name. Conversion happens at boundaries only.
5. **One dispatch point.** Format selection is the extension table in
   `crates/io/src/lib.rs::{read, write, write_with, write_framed}`. No adapter may open a dialog,
   and no adapter may choose its own format by sniffing.
6. **A converter reports what it lost.** The DXF layer already does this (`d.read_notes`, and the
   `crates/io/tests/roundtrip_audit.rs` test). Every 3D adapter returns the same thing, so "we
   imported it" never hides "we dropped the assemblies".

### What is reusable from `crates/io` (evidence)

| Reusable | Where | Why it is the right reuse |
|---|---|---|
| `IoError { Format, Unsupported }` | `crates/io/src/lib.rs:46` | `Unsupported(String)` is already *"unsupported file type `x`"* — exactly the error a "we do not do X" interchange refusal needs. |
| Extension-keyed dispatch + `ext()` | `lib.rs:56,62,80,86,113` | One `match` on the lowercased extension selects the format. Every 3D format is a new arm here, not a new entry point. |
| `.dwt` → `.dwg` name rewrite | `lib.rs:114` | The established pattern for "a container whose content is another's": rewrite the name, re-dispatch. |
| `check_window` / `MAX_WINDOW_COORD` | `lib.rs:137` | The numeric-hostility template: `is_finite`, a magnitude cap of ±1e12, a minimum side derived from the magnitude. Every 3D adapter copies this for coordinate ranges. |
| `SaveOptions { version, binary }` | `lib.rs:33` | The shape of an export-knob struct: a `Option` that defaults to the document's own setting. |
| `png_framed` / `png_view` | `lib.rs:168,176` | The offline render-and-encode path. Visual diffs of an export reuse it, or `cadcraft_render::scene3::Image::encode_png`. |
| `miniz_oxide`, `quick-xml` | `crates/io/Cargo.toml:22,23` | 3MF, `.glb` and glTF are zip/XML. No new dependency for them. |
| `read_notes` + `roundtrip_audit.rs` | `lib.rs:5,66`; `crates/io/tests/roundtrip_audit.rs` | The "kept what we do not model and wrote it back" convention (#528). It becomes `ConversionReport`. |
| 58 integration test files, `crates/io/src/tests.rs` | `crates/io/tests/` | The adapter test layout to copy: one file per format concern, fixtures under `tests/data/`. |

### The state of the existing STEP writer, quoted from its own docs

`crates/solid/src/step.rs` lists fifteen departures. The tasks below exist to close them, one by
one, and each names the numbered item it closes. Do not re-derive them; read the file.

---

# DAT-STEP — AP242 with a real B-rep, assemblies, colours, and a reader that agreed

### DAT-001 — Extend `IoError` with the variants a 3D adapter needs
- **State:** READY
- **Depends on:** —
- **Files:** `crates/io/src/lib.rs` (owner D)
- **Contract:**
  ```rust
  pub enum IoError {
      Format(String),
      Unsupported(String),
      Truncated { format: &'static str, at: usize },          // DAT-001
      Limit { what: &'static str, count: usize, max: usize },   // DAT-001
      Validation(Vec<String>),                                 // DAT-001, never swallowed
  }
  ```
- **Behaviour:** Add the three variants without removing or renaming the existing two, so the 58
  2D test files and every `IoError::Format(_)` match keep compiling unchanged. `Truncated` is for
  a file that ends mid-record; `Limit` for an input that passed its cap; `Validation` carries a
  validator's findings as a list, because one message cannot say "ids 41 and 812 are dangling".
  All five render with `thiserror` and name the format.
- **Refusals:** A validator with findings returns `Err(Validation(..))` — never `Ok` with warnings
  dropped. A file past a cap returns `Err(Limit(..))`, never a truncated write.
- **Tests:** `io_error_names_its_format` — `Truncated{format:"step", at: 41}` renders a string
  containing `step` and `41`. `validation_carries_every_finding` — a three-finding list renders
  three lines. `existing_two_variants_are_unchanged` — a compile-time construction of both plus
  the two existing tests in `lib.rs` still pass.
- **Acceptance:** `cargo xtask ci` green; no 2D test edited; `grep -c 'IoError::Format' crates/io`
  is unchanged.
- **Risk:** An existing `match` on `IoError` without a wildcard somewhere outside `crates/io`.
  Fallback: keep the variants `#[non_exhaustive]`-safe by fixing the compile errors that CI shows.

### DAT-002 — Put every 3D extension into the one dispatch table
- **State:** READY
- **Depends on:** DAT-001
- **Files:** `crates/io/src/lib.rs::{read, write, write_with, write_framed, is_image, ext}` (owner D)
- **Contract:**
  ```rust
  /// Extensions this build reads and writes, in one table, checked by a test against the file system.
  pub const EXCHANGE_3D: &[(&str, Format3)] = &[
      ("step", Format3::StepAp242), ("stp", Format3::StepAp242),
      ("iges", Format3::Iges), ("igs", Format3::Iges),
      ("3mf", Format3::ThreeMf),
      ("gltf", Format3::Gltf), ("glb", Format3::Gltf),
      ("jt", Format3::Jt),
  ];
  pub enum Format3 { StepAp242, Iges, ThreeMf, Gltf, Jt }
  impl Format3 {
      pub fn from_extension(name: &str) -> Option<Format3>;
      pub fn is_readable(self) -> bool;   // Jt is false until DAT-JT-002 lands
      pub fn is_writable(self) -> bool;
  }
  ```
- **Behaviour:** `read(bytes, name)` and `write_with(model, name, opts)` gain one arm per format and
  each arm calls the adapter module named by the table. `.step` and `.stp` are the same format, as
  `.iges`/`.igs` are and as `.gltf`/`.glb` are (JSON+buffers vs one container — the *same* content,
  the `.dwt`→`.dwg` pattern at `lib.rs:114`). `is_readable`/`is_writable` are separate so a
  read-only format is a table fact and not an arm that panics.
- **Refusals:** An extension in `EXCHANGE_3D` whose format is neither readable nor writable returns
  `Err(IoError::Unsupported(name))`. An unknown extension keeps today's behaviour exactly.
- **Tests:** `every_exchange_extension_dispatches` — for each `(ext, fmt)` pair, a one-byte body
  plus the name returns either `Ok` or `Err(Unsupported)` naming that extension, never an unknown
  extension error. `stp_and_step_are_the_same_format` — `Format3::from_extension("a.stp") ==
  Format3::from_extension("a.step")`. `case_is_ignored` — `.STEP` is `.step`.
- **Acceptance:** `cargo xtask ci` green; adding a format is one table row plus one adapter arm.
- **Risk:** A hostile extension of 10 000 characters used to allocate a path; `ext()` already
  allocates a `String` for it. Fallback: cap the extension length at 16 before matching.

### DAT-003 — Fix the `real()` formatter so a 1e-300 coordinate is not 300 zeroes
- **State:** READY
- **Depends on:** —
- **Files:** `crates/solid/src/step.rs::real` (owner D)
- **Contract:** `fn real(x: f64) -> String` — unchanged signature; behaviour: format with
  `{:.*E}`-style exponent when `x != 0 && (|x| >= 1e16 || |x| < 1e-4)`, else the current shortest
  round-trip form with a trailing `.` forced. Never emits more than 24 significant characters.
- **Behaviour:** Closes item **D.12** of `step.rs`. Part 21 accepts `1.E-300` and it is what
  mainstream writers emit; the shortest-round-trip form is kept inside the band so the existing
  byte-for-byte reproducibility test (`the_same_solid_writes_the_same_bytes_twice`) still passes on
  ordinary models. `0.0` is `0.0`, never `0`; `-0.0` is `-0.0` and never `-0`.
- **Refusals:** A non-finite `x` is unreachable (`Topology::build` refuses it with
  `SolidError::NonFinite` first); `real` must still not be able to write `NaN` if called directly,
  so it returns `"0.0"` for it and a test says so.
- **Tests:** `small_magnitudes_use_an_exponent` — `real(1e-300)` has length < 16 and contains `E`.
  `ordinary_magnitudes_keep_the_short_form` — `real(48.0) == "48.0"`, `real(1.0) == "1.0"`.
  `no_exponent_beyond_300_digits` — for `1e-300`, `1e300`, `f64::MIN_POSITIVE`, `f64::MAX`, the
  output is under 32 characters. `a_cube_export_is_still_byte_identical_twice`.
- **Acceptance:** the existing module tests pass unchanged **and** the new length test passes.
- **Risk:** A reader that parses only decimal points. Fallback: make the exponent band a
  `step_writer` option and keep the old form available while a reader is being surveyed.

### DAT-004 — Replace the non-Latin-name truncation with part 21 escaping
- **State:** READY
- **Depends on:** —
- **Files:** `crates/solid/src/step.rs::{clean_name, Writer}` (owner D)
- **Contract:**
  ```rust
  fn escape_p21(s: &str) -> String;          // ' -> '', \ -> \\
  fn encode_p21(s: &str) -> String;          // ASCII passthrough, non-ASCII as \X2\....\X0\
  fn clean_name(name: &str) -> String;       // unchanged signature; now escape, not delete
  ```
- **Behaviour:** Closes item **D.13**. A name in Cyrillic or with an accent is written as its
  `\X2\...\X0\` UTF-16BE form instead of being dropped character by character. The 120-character cap
  applies to the *decoded* name. ASCII is unchanged, so existing exports are byte-identical for
  ASCII product names. A name that is empty after decoding still becomes `cadcraft`.
- **Refusals:** A name holding a NUL is replaced with a space (part 21 has no NUL and a reader
  truncating at one would see a different name than we wrote).
- **Tests:** `a_cyrillic_name_survives_as_an_escape` — `clean_name("Кубик")` contains `\X2\` and
  `\X0\` and its decoded length is 5. `a_quote_is_doubled_not_dropped` — `clean_name("a\"b")`
  is `a''b`. `an_ascii_name_is_unchanged` — `clean_name("plate-01_a") == "plate-01_a"`.
  `the_cap_applies_to_the_decoded_name`.
- **Acceptance:** `grep -c 'take(120)' step.rs` is 0; the three existing `clean_name` tests pass.
- **Risk:** A reader that does not implement `\X2\` shows the escape literally. Fallback: keep the
  old truncation behind the writer option until DAT-STEP-019 has a reader verdict.

### DAT-005 — Give the file a named `LENGTH_UNIT` and honour the document's unit
- **State:** READY
- **Depends on:** DAT-UNIT-006
- **Files:** `crates/solid/src/step.rs::{step_schema, Writer}` (owner D)
- **Contract:**
  ```rust
  pub fn step_length_unit(unit: UnitSystem) -> &'static str;
  // "SI_UNIT(.MILLI.,.METRE.)" | "CONVERSION_BASED_UNIT('INCH',#n)" | "SI_UNIT($,.METRE.)" …
  pub fn step_faceted_brep_with(solid: &Solid, name: &str, unit: UnitSystem) -> Result<String, SolidError>;
  pub fn step_faceted_brep(solid: &Solid, name: &str) -> Result<String, SolidError>; // = …with(MILLIMETRE)
  ```
- **Behaviour:** Closes item **C.10**. The `LENGTH_UNIT` carries its own name (`'MILLIMETRE'`,
  `'INCH'`, `'METRE'`, `'FOOT'`) instead of being empty, and the `GEOMETRIC_REPRESENTATION_CONTEXT`
  is unchanged. Coordinates are multiplied by the conversion factor on the way out and the
  `UNCERTAINTY_MEASURE_WITH_UNIT` is converted with them, so the declared uncertainty is still the
  distance two points of this shell were welded at, expressed in the file's unit.
- **Refusals:** A unit whose factor is not finite, or whose factor times the solid's diagonal
  overflows `f64`, returns `Err(SolidError::Invalid(..))` naming the unit. A solid whose bounding
  diagonal after conversion is below `tolerance_for` is refused rather than written as a point.
- **Tests:** `a_millimetre_file_names_its_unit` — the `LENGTH_UNIT` line contains `'MILLIMETRE'`.
  `an_inch_file_scales_the_coordinates` — a 25.4 mm cube in inches has volume 1.0 within 1e-9,
  read back with the same `volume_of_file` helper the module already has. `the_declared_uncertainty
 _is_converted_too` — inch file, uncertainty `tol/25.4`. `a_scale_that_overflows_is_an_err`.
- **Acceptance:** the 131-entity cube assertion at `step.rs:840` still holds for the default
  (millimetre) path.
- **Risk:** An inch file read by a reader that trusts the *prefix* rather than the name.
  Fallback: write `SI_UNIT` only for mm and a `CONVERSION_BASED_UNIT` + `MEASURE_WITH_UNIT` for
  everything else, which is the only schema-legal spelling.

### DAT-006 — Emit `MANIFOLD_SOLID_BREP` with real `FACE_SURFACE`s and `EDGE_LOOP`s
- **State:** BLOCKED
- **Depends on:** KER-SRF-002, KER-SRF-003, DAT-003, DAT-004, DAT-005
- **Files:** `crates/solid/src/step/` (new module tree; owner D) — `crates/solid/src/lib.rs` gains
  `pub mod step;` re-exports and **must be registered in `xtask/src/layers.rs` only if a new crate
  appears — it does not; the writer stays in `cadcraft-solid` (L0)**
- **Contract:**
  ```rust
  pub struct StepWriterOptions {
      pub unit: UnitSystem,
      pub faces: FaceKeyword,          // FaceSurface | AdvancedFace
      pub loops: LoopKeyword,          // PolyLoop | EdgeLoop
      pub provenance: bool,            // DATE_AND_TIME
  }
  pub fn step_ap242(solid: &Solid, name: &str, opts: &StepWriterOptions) -> Result<String, SolidError>;
  ```
- **Behaviour:** Replaces `FACETED_BREP` with `MANIFOLD_SOLID_BREP` over a `CLOSED_SHELL` of
  `FACE_SURFACE`s (item **A.1**: `ADVANCED_FACE` is only emitted when `opts.faces` says so, and the
  default is `FaceSurface` because the AP rule names it). Each face's bounds are `FACE_OUTER_BOUND`
  or `FACE_BOUND` wrapping an `EDGE_LOOP` of `ORIENTED_EDGE`s (item **A.3**), so a per-use
  orientation flag exists and a reader no longer has to infer one from vertex order. Surfaces are
  the kernel's own: `PLANE` for a planar face, and after KER-SRF-003 a `CYLINDRICAL_SURFACE`,
  `CONICAL_SURFACE`, `SPHERICAL_SURFACE`, `TOROIDAL_SURFACE` or
  `B_SPLINE_SURFACE_WITH_KNOTS` for a curved one, with `B_SPLINE_CURVE_WITH_KNOTS` edges.
- **Refusals:** `Err(SolidError::Invalid(..))` for: a face whose loop vertex does not lie on its own
  surface within `tolerance_for(diagonal)` (the `ADVANCED_FACE` obligation this keyword imposes); a
  boundary edge not used exactly twice; an edge whose curve does not pass through both its
  vertices within tolerance; a face with no area; a non-finite coordinate; a `B_SPLINE_SURFACE`
  whose knots are not non-decreasing; an entity count over `MAX_STEP_ENTITIES`.
- **Tests:** `a_cube_is_a_manifold_solid_brep` — exactly one `MANIFOLD_SOLID_BREP`, one
  `CLOSED_SHELL`, six `FACE_SURFACE`, eight `VERTEX_POINT`, twelve `EDGE_CURVE`, twelve
  `ORIENTED_EDGE`, no `POLY_LOOP`. `every_loop_vertex_lies_on_its_face` — for each face, each
  `EDGE_LOOP` vertex is on that face's surface within the declared uncertainty. `each_boundary_edge
  _is_used_once_in_each_direction` — counts `ORIENTED_EDGE.orientation` per shared `EDGE_CURVE`.
  `a_washer_writes_two_bounds_per_holed_face`. `the_volume_read_back_from_the_file_matches_the_solid`.
  `hostile_solids_are_errors_not_panics` — the module's existing hostile test, extended with a
  hand-built face whose loop vertex is off its plane.
- **Acceptance:** DAT-STEP-019 reports an independent reader accepting every fixture **and**
  agreeing on volume to a relative `1e-9`. A writer that only passes its own parser is `CODE WRITTEN`.
- **Risk:** `EDGE_LOOP` makes vertex welding visible to the reader: the cell-straddle case in
  `Topology::build` (`step.rs:344`) can still write two `VERTEX_POINT`s a whisker apart and a
  strict reader rejects them. Fallback: DAT-STEP-022 welds on export and reports it.

### DAT-007 — Kill the orphan `EDGE_CURVE`s and the second shape representation
- **State:** BLOCKED
- **Depends on:** DAT-006
- **Files:** `crates/solid/src/step/` (owner D)
- **Contract:** Same as DAT-006; the writer emits **exactly one** `SHAPE_DEFINITION_REPRESENTATION`.
- **Behaviour:** Closes items **B.4** and **B.5**. With `EDGE_LOOP` the `EDGE_CURVE`s are referenced
  by `ORIENTED_EDGE`s, so the `GEOMETRIC_CURVE_SET` of `COMPOSITE_CURVE`s and the
  `GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION` exist only to give them a parent — they go
  away, and with them the second entry point. A part 21 file is a forest whose roots are the shape
  representations; there is then exactly one root.
- **Refusals:** A writer run that would leave any entity unreferenced is a bug the module's
  `roots()` helper must catch; it is asserted, not assumed.
- **Tests:** `exactly_one_shape_definition_representation` — the count is 1, and its kind is
  `SHAPE_DEFINITION_REPRESENTATION`. `every_entity_is_reachable` — `roots()` returns exactly one id.
  `no_composite_curve_survives`. `the_entity_graph_has_no_dangling_ids` — the existing test,
  re-pinned to the new count for a cube.
- **Acceptance:** the module's root assertion says one root; the reader report says the reader did
  not warn about a second representation.
- **Risk:** Some readers *want* a wireframe companion for edge display. Fallback: an
  `opts.also_wireframe: bool` that re-adds it deliberately, off by default, documented as a
  compatibility choice rather than a necessity.

### DAT-008 — Write AP242 product structure, not AP203
- **State:** READY
- **Depends on:** DAT-006
- **Files:** `crates/solid/src/step/` (owner D)
- **Contract:**
  ```rust
  pub fn step_schema() -> &'static str;   // now "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"
  ```
- **Behaviour:** Closes item **C.7**. The header carries `APPLICATION_PROTOCOL_DEFINITION`, the
  product carries `PRODUCT_DEFINITION_CONTEXT` naming `MECHANICAL_CONTEXT`, and a
  `PRODUCT_RELATED_PRODUCT_CATEGORY('part','',(#product))` is written so a reader that classifies
  by walking to the mechanical context finds the branch it expects. The schema string changes from
  `AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }` to the AP242 one; `step_schema()` stays the single
  source so a test can assert the file against it rather than repeating the literal.
- **Refusals:** A product name that sanitizes to empty is `cadcraft`, as today. A name over 120
  decoded characters is truncated with no error (documented), never silently dropped whole.
- **Tests:** `the_schema_is_ap242` — the `FILE_SCHEMA` line equals `format!("FILE_SCHEMA(('{}'))", step_schema())`.
  `an_application_protocol_definition_is_present`. `the_product_has_a_mechanical_context`.
  `a_reader_that_classifies_the_part_finds_it` — only meaningful via DAT-STEP-019; here, assert the
  entities exist and are referenced by the expected parents.
- **Acceptance:** DAT-STEP-019's report records the reader reporting an AP242 part, not "generic
  shape with no product metadata".
- **Risk:** A reader whose STEP build has no AP242 MIM support. Fallback: keep
  `StepWriterOptions::schema` so AP214 is one option away, and record which reader chose which.

### DAT-009 — Write colours and presentation styles
- **State:** BLOCKED
- **Depends on:** DAT-006, DAT-CAD-004, REN-MAT-002
- **Files:** `crates/solid/src/step/`, `crates/io/src/d3/step.rs` (owner D)
- **Contract:**
  ```rust
  pub struct StepStyle { pub body: String, pub face: Option<usize>, pub rgb: [f64; 3], pub name: String }
  pub fn step_ap242_styled(solid: &Solid, styles: &[StepStyle], opts: &StepWriterOptions) -> Result<String, SolidError>;
  ```
- **Behaviour:** Closes item **C.8** for colours. Emits `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`
  alongside the shape representation, `SURFACE_STYLE_USAGE` / `SURFACE_SIDE_STYLE_USAGE` per styled
  face, and `PRESENTATION_STYLE_ASSIGNMENT` carrying `SURFACE_STYLE_FILL_AREA` with an sRGB colour
  from `RGB_COLOR`. Colour comes from the document's appearance, not from the renderer's
  `scene3::Material` (which is a light model, not a colour store) — see DAT-CAD-004.
- **Refusals:** An sRGB triple with a component outside `0..=1` or non-finite is
  `Err(SolidError::Invalid("style colour .. out of range"))`. A `face` index the solid does not
  have is the same error naming the index.
- **Tests:** `a_styled_solid_carries_presentation_entities`. `an_unstyled_solid_writes_none`.
  `a_colour_out_of_range_is_an_err`. `a_face_index_outside_the_solid_is_an_err`.
  `the_styled_file_still_parses_with_the_modules_own_parser`.
- **Acceptance:** the reader report shows the body opening in the exported colour, or the reader's
  own log naming the colour entity; "we wrote a colour" is not the claim.
- **Risk:** AP242 presentation styles are the least portable corner of the schema.
  Fallback: also write a `COLOUR_RGB` in the legacy `DRAUGHTING_PRE_PRESENTATION_...` style some
  readers still read, and record which reader used which.

### DAT-010 — Map a CADCraft layer onto the STEP product structure
- **State:** BLOCKED
- **Depends on:** DAT-008, DAT-013, DAT-CAD-006
- **Files:** `crates/solid/src/step/`, `crates/io/src/d3/step.rs` (owner D)
- **Contract:**
  ```rust
  pub struct StepLayerMap { pub layer: String, pub product: String, pub occurrence: Option<String> }
  pub fn step_ap242_layers(solid: &Solid, map: &[StepLayerMap], opts: &StepWriterOptions) -> Result<String, SolidError>;
  ```
- **Behaviour:** STEP has no layer entity. The honest mapping is one layer per *product*, so a
  layered document exports as a product structure: `PRODUCT` per layer, one
  `NEXT_ASSEMBLY_USAGE_OCCURRENCE` per body, and each body its own
  `ADVANCED_BREP_SHAPE_REPRESENTATION`. A reader that shows a component tree then shows the layers.
  A layer with no body produces no product and is listed in the `ConversionReport`, not written as
  an empty one.
- **Refusals:** Two layers mapping to the same product name is
  `Err(SolidError::Invalid("two layers map to one product"))`. A layer name that sanitizes to
  `cadcraft` is reported, because it collides with the default.
- **Tests:** `two_layers_export_as_two_products_with_two_occurrences`.
  `an_empty_layer_is_reported_not_written`. `a_colliding_layer_name_is_an_err`.
  `the_component_tree_a_reader_sees_matches_the_layer_table` — asserted structurally here, and
  visually via DAT-STEP-019's report.
- **Acceptance:** the reader's import dialog shows one component per CADCraft layer.
- **Risk:** A reader that flattens a single-occurrence assembly back to one part.
  Fallback: a `preserve_layers: bool` that also writes a `PROPERTY_DEFINITION` per layer carrying
  the name, which costs a schema entity and buys a second chance at being read.

### DAT-011 — Write the assembly structure (blocked on `ASM-001`)
- **State:** BLOCKED
- **Depends on:** ASM-001, DAT-006, DAT-008
- **Files:** `crates/solid/src/step/`, `crates/io/src/d3/step.rs` (owner D)
- **Contract:**
  ```rust
  /// Consumes `cadcraft_asm::Assembly`; today that type does not exist, which is the block.
  pub fn step_ap242_assembly(asm: &cadcraft_asm::Assembly, opts: &StepWriterOptions) -> Result<String, SolidError>;
  ```
- **Behaviour:** Closes item **C.9**. One `PRODUCT` per component definition, one
  `PRODUCT_DEFINITION` per occurrence, `NEXT_ASSEMBLY_USAGE_OCCURRENCE` per parent-child link, and
  `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` + `REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION`
  for each placement transform. An occurrence whose transform is not a rigid motion (a scale other
  than 1, or a shear) is refused: STEP placements are rigid.
- **Refusals:** `Err` for a non-rigid transform naming the occurrence and the offending axis scale;
  for a cycle in the occurrence graph; for an occurrence with no component definition; for a
  nesting depth over `MAX_BLOCK_DEPTH`-equivalent (the engine's own bound, reused, not re-invented).
- **Tests:** `a_two_component_assembly_writes_two_products_and_one_occurrence`.
  `a_non_rigid_placement_is_refused`. `a_cyclic_occurrence_graph_is_refused`.
  `a_deeply_nested_assembly_is_bounded` — `MAX_BLOCK_DEPTH` levels then a named `Err`.
  `the_placement_transform_round_trips` — read back, every occurrence's matrix equals its input to
  1e-12.
- **Acceptance:** DAT-STEP-019's reader imports the assembly and shows the same tree with the same
  placements.
- **Risk:** `CADCraft` has no assembly type and `cadcraft-asm` is not in `xtask/src/layers.rs::TABLE`
  yet — the block is real. Fallback: none needed; this is sequencing, not design.

### DAT-012 — Keep `provenance` opt-in so an export is reproducible
- **State:** READY
- **Depends on:** —
- **Files:** `crates/solid/src/step/` (owner D)
- **Contract:** `StepWriterOptions::provenance: bool` — `false` writes the empty `FILE_NAME` time
  stamp and the empty author/organisation/authorisation fields of `step.rs` today; `true` writes a
  `DATE_AND_TIME`, the document's author and organisation (DAT-CAD-005) and the build SHA.
- **Behaviour:** Closes item **C.11** and makes the choice explicit rather than accidental. With
  `provenance: false` two exports of one solid are byte-identical — the existing test
  `the_same_solid_writes_the_same_bytes_twice` becomes the default-path guarantee. With `true` the
  file carries `DATE_AND_TIME` as a `DATE_AND_TIME` entity in the header section's `FILE_NAME`
  fifth field, formatted `YYYY-MM-DDTHH:MM:SS`.
- **Refusals:** A timestamp built from a clock that went backwards (a non-monotonic reading) is
  refused with `Err(SolidError::Invalid("the clock went backwards"))` rather than written as a
  1970 date.
- **Tests:** `two_exports_are_byte_identical_without_provenance`.
  `provenance_writes_a_date_and_time_and_the_build_sha`.
  `a_backwards_clock_is_an_err`.
  `the_header_section_order_is_unchanged` — `HEADER;`, `FILE_DESCRIPTION`, `FILE_NAME`,
  `FILE_SCHEMA`, `ENDSEC;` in that order.
- **Acceptance:** the reproducibility test holds by default; a reader shows a timestamp when asked.
- **Risk:** A build without `CADCRAFT_BUILD_SHA` writes an empty string where a reader wants a
  token. Fallback: write `unknown` and record it in the report.

### DAT-013 — A part-21 validator that runs on every export
- **State:** READY
- **Depends on:** DAT-006
- **Files:** `crates/io/src/d3/step/validate.rs` (owner D)
- **Contract:**
  ```rust
  pub fn validate_part21(text: &str) -> Vec<String>;
  pub struct StepKeywords { /* owned keyword table: every keyword this writer emits, with its attribute list */ }
  pub fn validate_against_schema(text: &str, kw: &StepKeywords) -> Vec<String>;
  ```
- **Behaviour:** The checks are structural, not semantic: a `#n` nothing defines; an id defined
  twice; a forward reference; an entity nothing names (more roots than the file's own entry
  points); a keyword outside the owned table; an attribute list whose arity or order differs from
  the table's. This is the `tests/step_interop.rs` table, promoted into the build so every export
  is checked by the thing that knows our keywords.
- **Refusals:** A validator that finds anything returns `Err(IoError::Validation(findings))` from
  the writer path. Validation never repairs: a file is written or it is not written.
- **Tests:** `a_clean_cube_has_no_findings`. `a_dangling_reference_is_found` — one `#n` deleted
  from the text yields a finding naming `n`. `a_duplicate_id_is_found`. `a_forward_reference_is_found`.
  `a_foreign_keyword_is_found`. `an_unknown_attribute_is_found` — `ADVANCED_FACE('x',(#1),#2,.T.,.T.)`
  yields a finding naming the fifth argument.
- **Acceptance:** the `solid.export.step` command runs it and returns findings in the `Err` string;
  CI is green with the fixture corpus.
- **Risk:** The keyword table is our own reading of the schema and could itself be wrong — the same
  limitation `step.rs:25-28` names for the two parsers. Fallback: the table is a plain data file
  (`step_keywords.tsv`) that can be diffed against the published schema by a human.

### DAT-014 — The external-reader harness (`DAT-003` in the master plan)
- **State:** READY
- **Depends on:** DAT-013, DAT-006, DAT-007, DAT-008
- **Files:** `crates/io/tests/step_external.rs`, `crates/io/tests/data/step/` (owner D),
  `crates/io/tests/data/step-reader-report.md`
- **Contract:**
  ```rust
  /// Ignored by default. Run: CADCRAFT_STEP_READER="python3 tools/step_probe.py" \
  ///   cargo test -p cadcraft-io --test step_external -- --ignored --nocapture
  pub fn probe(bytes: &[u8], name: &str, reader: &dyn Fn(&[u8], &str) -> Result<ProbeReport, String>) -> Result<(), String>;
  #[derive(Default, Debug, PartialEq)]
  pub struct ProbeReport {
      pub accepted: bool,
      pub schema_seen: Option<String>,
      pub solids: usize,
      pub shells: usize,
      pub faces: usize,
      pub volume: Option<f64>,
      pub area: Option<f64>,
      pub warnings: Vec<String>,
      pub errors: Vec<String>,
  }
  ```
- **Behaviour:** For each fixture, the harness writes the file to a temporary directory, shells out
  to the reader named by `CADCRAFT_STEP_READER`, and compares the reader's answer to ours: accepted,
  solid and shell counts, face count, and volume to a relative `1e-9`. The harness **fails** on any
  disagreement and prints the reader's own warnings so they are not lost. The default run writes a
  human-readable summary to stdout; the checked-in report records reader name, version, exact
  command, fixture list with per-fixture verdicts, and the date.
- **Refusals:** No reader configured → the test is `#[ignore]`d and says so; it does **not** pass
  silently. A reader that exits non-zero → the fixture fails with the reader's stderr in the message.
  A reader whose volume differs by more than the relative tolerance → fails with both numbers.
- **Tests:** `the_probe_compares_volume_to_a_relative_tolerance` — a report whose volume is off by
  1e-8 relative fails, 1e-12 passes. `the_probe_requires_a_reader` — with no environment variable the
  harness returns a message naming the variable. `a_report_records_the_reader_version`.
- **Acceptance:** **the checked-in report exists and names at least one real independent reader, its
  version, and a pass count equal to the fixture count.** Until then every STEP task in this epic is
  `CODE WRITTEN`, and `docs/file-format-parity.md`'s STEP row stays blank.
- **Risk:** the dev machine has no reader installed (this is exactly the state `step.rs:12-16`
  describes). Fallback: the harness is written, documented and ignored; the first agent who has a
  reader runs it and writes the report. **Do not write a report that was not produced by a run.**
- **Risk:** a generated STEP fixture is a `.step` file; `xtask/src/assets.rs::ASSET_EXT` has no
  `step` entry, so committing fixtures raises no attribution question today. See PLT-ASSET-008,
  which decides the rule.

### DAT-015 — Turn `step.rs`'s ten-minute checklist into the fixture list
- **State:** READY
- **Depends on:** DAT-014
- **Files:** `crates/io/tests/data/step/`, `crates/io/tests/step_external.rs` (owner D)
- **Contract:** `pub const FIXTURES: &[&str]` — one entry per line of the checklist at
  `step.rs:126-143`: plain cube; washer (hole in a face); face split into two disjoint regions
  (unwelded); two vertices a whisker apart; a solid with an unwelded junction (`KER-BOOL-002`);
  a non-manifold edge; an inside-out shell; a 0-face solid; a face with a NaN vertex; a solid
  with 10⁵ faces.
- **Behaviour:** Each fixture is built by our own writer from a named model, exported, and probed.
  The checklist's questions become the probe's assertions: exactly one solid and one `CLOSED_SHELL`;
  volume equal to the kernel's; faces outward (positive volume and the solid selectable); a second
  representation tolerated; the face keyword accepted; unit reported as millimetre; `FACE_BOUND`s
  read as holes; the second region read rather than punched through; a whisker-apart vertex pair
  rejected or welded.
- **Refusals:** A fixture the kernel refuses to build is not exported and is listed in the report as
  *not produced*, with the `SolidError` — it is never skipped silently.
- **Tests:** `the_fixture_list_covers_every_question_on_the_checklist` — asserts `FIXTURES` has one
  entry per checklist bullet, so removing a question without adding a fixture fails.
  `an_unweldable_fixture_is_reported_not_produced`. `a_disjoint_region_fixture_exports_two_face_bounds`.
- **Acceptance:** the report lists every fixture with a verdict.
- **Risk:** some checklist questions need a reader that reports face-level detail (which reader can
  say "the second region was punched through"?). Fallback: record the question as *unanswerable by
  the reader in use* in the report, rather than marking it passed.

### DAT-016 — Round-trip: our writer's file, our reader
- **State:** BLOCKED
- **Depends on:** DAT-006, DAT-013
- **Files:** `crates/io/src/d3/step.rs` (owner D)
- **Contract:**
  ```rust
  pub fn step_read(bytes: &[u8], name: &str) -> Result<Import, IoError>;
  // IoError::Truncated / Validation / Limit; never a partial Solid::default().
  ```
- **Behaviour:** Read the product structure, rebuild the shell, and return an `Import` (DAT-018).
  Orientation is taken from the file, not re-derived; a file whose shell is closed but inside-out
  is returned with `ConversionReport.warnings` naming it and the solid reversed, because refusing
  would lose the user's geometry while reversing is a stated, tested transformation.
- **Refusals:** `Err(IoError::Truncated{..})` for a file that ends mid-entity;
  `Err(IoError::Validation(..))` for a dangling id, a loop vertex off its face, an edge used once,
  or a `B_SPLINE_SURFACE` with non-monotone knots; `Err(IoError::Limit{..})` past the entity cap;
  `Err(IoError::Unsupported)` for an entity type the reader does not implement (named in the message).
- **Tests:** `a_cube_round_trips_to_the_same_volume`. `an_inside_out_shell_is_reversed_and_reported`.
  `a_truncated_file_is_a_named_error`. `a_dangling_id_is_a_named_error`. `an_unsupported_entity_names_itself`.
  `hostile_step_never_panics` — 200 mutations of a valid file (bit flips in the DATA section) all
  return `Ok` or `Err`.
- **Acceptance:** the fixture corpus round-trips; the hostile file is `hostile_step_never_panics`.
- **Risk:** reading a file that an independent reader accepted and we reject is the worst outcome —
  it means our reader is behind our writer. Fallback: report the disagreement with the external
  probe's verdict next to ours in the report rather than choosing silently.

### DAT-017 — Export the kernel's tolerance, not a round number, and say what it means
- **State:** READY
- **Depends on:** —
- **Files:** `crates/solid/src/step/` (owner D)
- **Contract:** `pub fn step_uncertainty(solid: &Solid) -> f64` — `tolerance_for(diagonal)`, the
  same value `Topology::build` welds on, clamped to `[1e-12, 1e-3]`.
- **Behaviour:** Closes item **D.15**. The `UNCERTAINTY_MEASURE_WITH_UNIT` carries this value and
  the reader is told what it is ('the solid kernel working tolerance'). A round number would be a
  lie: a reader that uses the declared uncertainty to merge vertices needs the *real* welding
  distance, not a conventional `1e-7`.
- **Refusals:** A solid whose diagonal is not finite or not positive → `Err(SolidError::Invalid)`.
- **Tests:** `the_uncertainty_is_the_welding_distance` — export, read the value back, compare with
  `solid.tolerance()`. `a_huge_part_has_a_larger_uncertainty_than_a_small_one`. `an_invalid_diagonal_is_an_err`.
- **Acceptance:** the value in the file equals `tolerance_for(bounds.diagonal())` for all fixtures.
- **Risk:** a reader rejecting an uncertainty outside its expected range. Fallback: the value is a
  writer option with the honest default; the report records any reader's complaint.

### DAT-018 — Define the adapter seam every 3D format implements
- **State:** READY
- **Depends on:** DAT-001, DAT-002
- **Files:** `crates/io/src/d3/mod.rs` (owner D)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ConversionReport { pub lost: Vec<String>, pub kept_unknown: usize, pub warnings: Vec<String> }

  #[derive(Clone, Debug, PartialEq)]
  pub struct Import {
      pub bodies: Vec<(String, cadcraft_solid::Solid)>,  // name -> body, in file order
      pub unit: UnitSystem,
      pub report: ConversionReport,
  }

  pub trait Exchange3 {
      fn format() -> Format3;
      fn read(bytes: &[u8], name: &str) -> Result<Import, IoError>;
      /// Consumes `cadcraft_model::Model`, never a parallel document type.
      fn write(model: &cadcraft_model::Model, opts: &ExportOptions) -> Result<Vec<u8>, IoError>;
  }

  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct ExportOptions {
      pub unit: Option<UnitSystem>,   // None = the document's own unit
      pub name: String,
      pub provenance: bool,
      pub assembly: bool,
      pub colors: bool,
      pub schema: Option<String>,
  }
  ```
- **Behaviour:** One seam, so `write_with` dispatches by `Format3` and a format does not grow its
  own entry point. `Import.bodies` is a flat list because `cadcraft_model::Model` has one `solid`
  today (`Model::solid()`); multi-body is `MOD-004` and the field is the seam it will widen.
  `ConversionReport` is the 3D form of `d.read_notes` (`lib.rs:5,66`).
- **Refusals:** An adapter whose `Format3::is_readable()` is false must still implement `read` and
  return `Err(IoError::Unsupported(..))` naming the format — no `todo!`, no panic.
- **Tests:** `every_registered_format_implements_the_seam` — a table walk over `EXCHANGE_3D`
  asserting `Format3::is_readable/is_writable` matches the hand-written capability table.
  `an_export_report_lists_what_was_lost` — a model with a feature that did not build lists it.
  `a_read_only_format_returns_unsupported`.
- **Acceptance:** the table test passes; adding a format is one module implementing four items.
- **Risk:** an over-general seam that every adapter bends. Fallback: the seam is deliberately small
  (two functions); anything a format needs beyond it is passed in `ExportOptions`, not in the trait.

### DAT-019 — Export the model, not the last good solid
- **State:** READY
- **Depends on:** DAT-018, DAT-002
- **Files:** `crates/io/src/d3/step.rs`, `crates/engine/src/cmd/solid.rs` (owner D)
- **Contract:**
  ```rust
  pub fn export_solid(model: &cadcraft_model::Model, opts: &ExportOptions) -> Result<Vec<u8>, IoError>;
  // CommandSpec "solid.export.step" {path?, unit?, provenance?, colors?, schema?} → {path, bytes, faces, lost}
  ```
- **Behaviour:** The adapter reads `model.solid()`. When the rebuild produced nothing
  (`ModelError::NoSolid`) the export is refused with a message naming the first feature in
  `rebuild.problems`, not the last good shape — exporting stale geometry silently is the failure
  mode that loses a design. A body whose last feature failed is still exported, with the failed
  feature listed in `ConversionReport.lost`, because refusing would be worse than exporting with
  the caveat stated.
- **Refusals:** `Err(IoError::Format("the model has no solid: <feature>: <message>"))` for a model
  with nothing built. `Err(Unsupported)` for `.stl` through this path (STL is `solid.export.stl`,
  which already exists, and it is a mesh format, not a B-rep format).
- **Tests:** `a_built_model_exports`. `a_model_with_no_solid_is_a_named_error`.
  `a_model_whose_last_feature_failed_lists_it_in_the_report`.
  `the_export_command_returns_bytes_and_lost` — the JSON has `bytes` and a `lost` array.
- **Acceptance:** `solid.export.step` reaches `app.open`-equivalent coverage through the control
  channel; `cargo xtask ci` green.
- **Risk:** `crates/engine/src/cmd/solid.rs` is engine-owned; the wave-1 table gives agent D
  `crates/model/src/file.rs` and `crates/solid/src/step.rs`, and engine to C. Fallback: land the
  adapter first, then request the command as a co-sequenced task.

### DAT-020 — Write a `README` in every exported format's directory saying what it does not support
- **State:** READY
- **Depends on:** DAT-018
- **Files:** `crates/io/src/d3/{step,iges,threemf,gltf,jt}.rs` module docs (owner D)
- **Behaviour:** Each adapter's module doc opens with a **does not support** list, in the house
  style of `step.rs:36-158` and `scene3.rs:10-32`. This is not documentation taste: it is the
  only place a user can learn that an assembly was dropped. The list is generated from the same
  strings the code actually uses in its `ConversionReport::lost` pushes, so it cannot drift from the
  behaviour — a test asserts every literal in `lost` appears in the module doc.
- **Refusals:** n/a (documentation).
- **Tests:** `every_lost_message_appears_in_the_module_docs` — collect the string literals passed to
  `report.lost.push(..)` per adapter and assert each is in the module's `//!` text.
  `every_adapter_module_doc_names_its_format_schema`.
- **Acceptance:** the test passes; `docs/file-format-parity.md` links the module docs.
- **Risk:** someone pushes a `lost` string in a helper far from the module. Fallback: the test
  searches the whole crate's sources, not just the module, and names the file it found it in.

### DAT-021 — Refuse an unwelded shell on export instead of writing it faithfully
- **State:** READY
- **Depends on:** —
- **Files:** `crates/solid/src/step/` (owner D)
- **Contract:**
  ```rust
  pub enum ExportCheck { Validate, WeldThenWrite, WeldAndRefuseIfChanged }
  // StepWriterOptions::check: default WeldThenWrite
  ```
- **Behaviour:** `step.rs`'s docs tell the caller that `weld_edges` is the caller's job and that a
  corner a whisker either side of a weld cell boundary can be written twice. Make the writer do it:
  weld, then write, then compare face and vertex counts; `WeldAndRefuseIfChanged` returns
  `Err(SolidError::Invalid("welding changed the solid; fix the model"))` naming the counts. Welding
  is done on a `BTreeMap` grid keyed exactly as `Topology::build` keys it, so the two cannot disagree.
- **Refusals:** A solid that still fails `Solid::validate` after welding is `Err`, never written.
  A solid whose welding exceeds `limits::MAX_WELD_VERTICES` is `Err(TooManyFaces/Invalid)` naming
  the cap — welding is quadratic and an unbounded weld is a denial of service on a hostile model.
- **Tests:** `a_welded_solid_exports_identically`. `an_unwelded_solid_is_welded_on_export`.
  `refusing_a_changed_solid_names_the_counts`. `a_solid_too_large_to_weld_is_an_err`.
  `the_cell_straddle_case_is_reported` — the fixture from `step.rs:344`.
- **Acceptance:** DAT-015's fixture list has no unwelded export.
- **Risk:** welding silently changes a model the user did not ask to change. Fallback: default
  `WeldThenWrite` with the weld reported in `ConversionReport`, and the refusing mode available.

### DAT-022 — A named entity budget per format, and where it is enforced
- **State:** READY
- **Depends on:** DAT-018
- **Files:** `crates/io/src/d3/mod.rs` (owner D)
- **Contract:**
  ```rust
  pub const MAX_STEP_ENTITIES: usize = 2_000_000;      // already in step.rs
  pub const MAX_IGES_PARAM_LINES: usize = 4_000_000;
  pub const MAX_3MF_UNCOMPRESSED: usize = 512 * 1024 * 1024;
  pub const MAX_GLTF_ACCESSORS: usize = 4_000_000;
  pub const MAX_JT_TESSELLATION_TRIS: usize = 5_000_000;
  pub fn budget(format: Format3) -> usize;
  ```
- **Behaviour:** One place to look for "how much input is too much". Each is a backstop against a
  hostile file, not a tuning knob, and each is stated in the adapter's module doc. `miniz_oxide`
  decompression is bounded by `MAX_3MF_UNCOMPRESSED`, which is the zip-bomb case `crates/io`
  already guards for SVGZ.
- **Refusals:** `Err(IoError::Limit { what, count, max })` at every boundary, checked with checked
  arithmetic before any allocation sized from the input.
- **Tests:** `every_format_has_a_budget`. `a_file_past_the_budget_is_a_limit_error` per format.
  `a_zip_bomb_is_refused` — a `.3mf` that expands to 1 GB is refused before expansion completes.
- **Acceptance:** the hostile-file tests pass for every adapter.
- **Risk:** a legitimate 50 M-face assembly exceeds the STEP budget and cannot be exported.
  Fallback: the budget is reported as a clear limit in the error, so raising it is a decision, not a mystery.

---

# DAT-IGES — the oldest CAD interchange format still in use

IGES is NURBS-based and 40 years old. CADCraft's kernel is planar-faced, so **an honest IGES writer
is a tessellation writer that claims to be a B-rep writer, or it is not written.** That sentence is
the epics constraint.

### DAT-023 — Write the ADR that decides IGES is tessellation-only
- **State:** READY
- **Depends on:** —
- **Files:** `docs/file-format-parity.md`, new `crates/io/src/d3/iges.rs` module doc (owner D)
- **Contract:** `pub fn iges_facets(solid: &Solid, opts: &IgesOptions) -> Result<String, SolidError>`
  — deliberately *facets*, named as facets. Rational B-spline output (`entities 120/126/128`) is
  only legal once KER-SRF-001/002 exist; until then a rational B-spline approximation of a cylinder
  would be a curve the kernel cannot itself evaluate, and exporting something the product cannot
  read back is the worst of both.
- **Behaviour:** Emits `TRIMMED_CURVE` / trimmed surfaces over the tessellation with the same
  section structure (`G` global, `D` directory, `P` parameter, `T` terminate; 80-column fixed-width
  records; DE/PD pointer chains). The module doc says, in its first paragraph, that the output is a
  faceted approximation of a planar-faced B-rep, and that an IGES consumer editing it will be
  editing a polyhedron.
- **Refusals:** A solid with a face whose area is below `tolerance_for` is `Err`. A curve the
  tessellator cannot order within tolerance is `Err`. A tessellation over `limits::MAX_TRIANGLES`
  is `Err(TooManyTriangles)`.
- **Tests:** `a_cube_is_four_direction_entities_and_eight_points`. `every_de_pointer_chain_resolves`.
  `the_terminate_section_is_written`. `a_hostile_solid_is_an_err`.
- **Acceptance:** `docs/file-format-parity.md` gains an IGES row whose "honest scope" cell reads
  *faceted only*.
- **Risk:** users expect IGES for surface exchange and will be disappointed. Fallback: none, and
  the disappointment is the point — the alternative is a file that lies.

### DAT-024 — Set IGES's unit flag and the `TIME_STAMP`
- **State:** BLOCKY READY — depends DAT-UNIT-006
- **Depends on:** DAT-023, DAT-UNIT-006
- **Files:** `crates/io/src/d3/iges.rs` (owner D)
- **Contract:** `pub fn iges_time_stamp(opts: &IgesOptions) -> &'static str` — IGES encodes the
  stamp as two 8-character fields, `YYYYMMDD.HHMMSS.00` and `0000.00.HHMMSS.00`, taken from the
  *second* field for a "user entered on" and the first for "last update".
- **Behaviour:** The unit flag is global entity 1 parameter 15: `2` millimetre (default), `1`
  inch, `4` metre. With `provenance: false` both fields read `0000.00.0000.00.00`, which IGES
  defines as "no date supplied" and which keeps an export byte-identical. With `true` the second
  field carries the real time.
- **Refusals:** An unknown unit → `Err(IoError::Format("IGES has no unit flag for ..."))`. A
  timestamp string that is not exactly 16 characters is refused by the validator.
- **Tests:** `the_unit_flag_follows_the_document_unit` — mm gives `2`, inch gives `1`, metre `4`.
  `no_provenance_means_the_defined_empty_stamp`. `provenance_fills_both_fields`.
  `a_stamp_of_the_wrong_length_is_rejected`.
- **Acceptance:** an IGES consumer reports the correct unit.
- **Risk:** a reader that treats `0000.00.0000.00.00` as malformed. Fallback: `provenance: true`
  is the answer; the default is chosen for reproducibility and documented.

### DAT-025 — IGES reader: trimmed surfaces back to a body
- **State:** BLOCKED
- **Depends on:** KER-SRF-005, DAT-023
- **Files:** `crates/io/src/d3/iges.rs` (owner D)
- **Contract:**
  ```rust
  pub fn iges_read(bytes: &[u8], name: &str) -> Result<Import, IoError>;
  ```
- **Behaviour:** Parses the G/D/P/T sections, resolves the DE/PD pointer chains, and builds a
  `Solid` from planar trimmed faces. Requires `KER-SRF-005` (sew/knit/heal) to join the faces into a
  closed shell: a soup of trimmed faces is not a body and must not be presented as one. The unit
  from global entity 1 parameter 15 scales every coordinate before the body is built.
- **Refusals:** `Err(Unsupported(..))` naming the entity type for anything unimplemented (rational
  B-spline surfaces until KER-SRF-002 lands); `Err(Truncated)` for a pointer chain off the end;
  `Err(Validation)` for a section that is not 80 columns; `Err(Limit)` past
  `MAX_IGES_PARAM_LINES`; `Err(Validation("the faces do not sew into a closed shell"))` naming how
  many edges are open when sewing fails.
- **Tests:** `a_cube_round_trips_through_iges` — our export, our read, same volume to 1e-9.
  `an_inch_file_is_read_in_millimetres`. `an_unimplemented_entity_names_itself`.
  `a_truncated_file_is_a_named_error`. `a_section_that_is_not_80_columns_is_refused`.
  `an_open_shell_is_refused_with_the_open_edge_count`. `hostile_iges_never_panics` — 200 mutations.
- **Acceptance:** external-reader validation as in DAT-014, with an IGES reader named in the report.
- **Risk:** sewing a real IGES file is where this will fail. Fallback: read and report the face soup
  with every open edge named, and mark the file "read, not sewn" in the report rather than failing.

### DAT-026 — IGES: what the file held that we did not take
- **State:** BLOCKY READY — depends DAT-023
- **Depends on:** DAT-023
- **Files:** `crates/io/src/d3/iges.rs` (owner D)
- **Contract:** every unsupported entity pushes one line into `ConversionReport.lost`, in the form
  `"entity 206 (COPARASOLID) at DE 42: not implemented"`.
- **Behaviour:** Includes the directory attributes themselves: `P` entries in group 33 and 34
  (transformation matrices), `STATUS`, `LINE_WEIGHT`, `COLOUR`, `SUBORDINATE` are kept in a private
  passthrough table and re-emitted on export so a round trip does not lose them — the `#528`
  convention.
- **Refusals:** n/a.
- **Tests:** `an_unsupported_entity_is_reported_with_its_pointer`. `transformations_are_kept_and_reemitted`.
  `colours_are_kept_and_reemitted`. `the_report_is_empty_for_a_file_of_only_supported_entities`.
- **Acceptance:** `roundtrip_audit.rs`-style audit exists for IGES: export→read→export, and the
  directory section of the second file carries the same entity numbers.
- **Risk:** passthrough tables grow without bound. Fallback: cap the table at the entity budget and
  report what was dropped past it.

---

# DAT-3MF — the mesh format made for manufacturing

### DAT-027 — `.3mf` container: zip of XML, no new dependency
- **State:** READY
- **Depends on:** DAT-018
- **Files:** `crates/io/src/d3/threemf.rs` (owner D)
- **Contract:**
  ```rust
  pub fn threemf_write(model: &cadcraft_model::Model, opts: &ExportOptions) -> Result<Vec<u8>, IoError>;
  ```
- **Behaviour:** A `.3mf` is an OPC package: `[Content_Types].xml`, `_rels/.rels`, and
  `3D/3dmodel.model`, zipped with `miniz_oxide` (already a `cadcraft-io` dependency,
  `Cargo.toml:22`). The model part is UTF-8 XML with the `<model unit="millimeter">` root — the
  3MF core specification admits exactly one unit value, `millimeter`, which is why `unit` is not an
  option here and the conversion happens before the writer runs.
- **Refusals:** `Err(Limit)` if the uncompressed total passes `MAX_3MF_UNCOMPRESSED`; `Err(Format)`
  if the model has no body; `Err(Unsupported)` if asked for binary output (3MF is not a binary
  format, and `SaveOptions.binary` says so in the message).
- **Tests:** `a_cube_is_a_valid_opc_package` — the three required parts exist with the right content
  types and relationships. `the_root_declares_millimetres`. `a_zip_bomb_is_refused`.
  `two_exports_are_byte_identical` — OPC zip entries carry timestamps, so the entry timestamps must
  be fixed at the ZIP epoch for the reproducibility property to hold.
- **Acceptance:** the file opens in lib3mf (DAT-030).
- **Risk:** a zip writer that stamps entries with the clock breaks reproducibility. Fallback:
  fixed timestamps are the default and tested.

### DAT-028 — 3MF meshes, objects, items and build items
- **State:** READY
- **Depends on:** DAT-027
- **Files:** `crates/io/src/d3/threemf.rs` (owner D)
- **Contract:** `pub struct M3mfMesh { vertices: Vec<[f64;3]>, triangles: Vec<[u32;3]>, normals: Option<Vec<[f64;3]>> }`
- **Behaviour:** One `<mesh>` and one `<object>` per body, one `<item>` per body in the build, and
  `<components>` for an assembly once ASM-001 lands. Triangle winding must match the kernel's
  convention (counter-clockwise seen from outside, invariant I4) or every slicer will treat the
  solid as inverted — the single most damaging possible defect in this format, and the one a slicer
  reports only as "the part is inside out".
- **Refusals:** A body with a mesh whose winding is inconsistent (some faces inward) is
  `Err(Format("body {name} has inward-facing triangles at indices {i}"))` — refused, not written.
- **Tests:** `every_triangle_winds_counter_clockwise_seen_from_outside` — checked with the signed
  volume, which must be positive and equal the kernel's. `the_build_lists_every_object`.
  `an_inward_wound_mesh_is_refused`. `normals_are_written_only_when_they_disagree_with_the_winding`.
- **Acceptance:** a slicer opens it and reports the right volume.
- **Risk:** degenerate triangles (zero area) make a slicer emit a zero-length contour. Fallback:
  skip zero-area triangles and list the count in `ConversionReport.warnings`.

### DAT-029 — 3MF materials and colours
- **State:** BLOCKY READY — depends REN-MAT-002
- **Depends on:** DAT-028, REN-MAT-002
- **Files:** `crates/io/src/d3/threemf.rs` (owner D)
- **Contract:** `pub enum M3mfAppearance { BaseMaterial { name, display_color: [u8;4] }, Texture2D { path, uv: ... } }`
- **Behaviour:** `<basematerials>` with `m:colorgroup displaycolor="rgba(r,g,b,a)"`, referenced by
  `<triangle p1 p2 p3 pid="..."/>`. Textures go in `/3D/Textures/` with a content type part and a
  relationship; a texture we cannot ship (a path outside the document) is reported as lost, never
  embedded from the disk behind the user's back.
- **Refusals:** A texture path that escapes the export directory is `Err(Format("texture path {p}
  leaves the export directory"))`. A display colour with an alpha that 3MF forbids in a
  `basematerials` context is clamped and reported.
- **Tests:** `materials_reference_real_colours`. `a_texture_part_and_relationship_are_written`.
  `a_path_outside_the_directory_is_refused`. `alpha_is_written_the_way_3mf_expects`.
- **Acceptance:** the colours round-trip through lib3mf.
- **Risk:** 3MF's colour model is not PBR. Fallback: the doc says so and maps
  `pbrMetallicRoughness.baseColorFactor` to `displaycolor`, losing metallic/roughness into the
  report.

### DAT-030 — Validate `.3mf` against lib3mf
- **State:** READY
- **Depends on:** DAT-027, DAT-028
- **Files:** `crates/io/tests/3mf_external.rs`, `crates/io/tests/data/threemf-reader-report.md` (owner D)
- **Contract:** as DAT-014's `ProbeReport`, with `pub fn probe(bytes: &[u8], reader: &dyn Fn(&[u8]) -> Result<ProbeReport, String>) -> Result<(), String>;`
- **Behaviour:** Shells out to an independent 3MF reader (`CADCRAFT_3MF_READER`), checks accepted,
  object count, triangle count, volume against the kernel's, and that no validation error is
  reported. The report file names the library, version, command, fixtures and verdicts.
- **Refusals:** No reader configured → `#[ignore]`, with the variable named. A non-zero exit → fail
  with stderr. A volume outside the relative tolerance → fail with both numbers.
- **Tests:** `the_probe_requires_a_reader`. `a_volume_mismatch_fails_with_both_numbers`.
- **Acceptance:** the checked-in report names lib3mf and passes on every fixture.
- **Risk:** lib3mf may not be installed. Fallback: as DAT-014 — write the harness, leave it
  ignored, and do not fabricate a report.

### DAT-031 — 3MF reader: a mesh back into the document
- **State:** BLOCKY READY — depends MOD-004
- **Depends on:** DAT-027, MOD-004
- **Files:** `crates/io/src/d3/threemf.rs` (owner D)
- **Contract:** `fn read(bytes: &[u8], name: &str) -> Result<Import, IoError>;` (the `Exchange3::read` signature)
- **Behaviour:** A 3MF file holds triangles, and `cadcraft_solid::Solid` holds planar faces over
  poly loops. Triangles are planar faces, so the mapping is exact and lossless: one `Face` per
  triangle, in its own plane, with the region wound as the kernel demands. What this **is not** is a
  B-rep: a boolean against an imported mesh will be a boolean against 10⁵ independent planar faces,
  and the seam is a non-manifold edge. The import therefore lands as an *imported body* flagged
  `is_mesh: true` and refused by the boolean commands with a message that says why, until
  `KER-SRF-005` sewing is available.
- **Refusals:** A non-`millimeter` `unit` attribute → `Err(IoError::UnitMismatch{..})`.
  A triangle index out of range → `Err(Validation(..))` naming the index. An object with no mesh →
  reported as lost. A mesh with a non-manifold edge → imported with the flag set and counted, not
  refused.
- **Tests:** `triangles_become_planar_faces`. `an_imported_mesh_is_flagged_is_mesh`.
  `a_wrong_unit_is_a_unit_mismatch_error`. `an_out_of_range_index_is_refused`.
  `hostile_3mf_never_panics` — 200 mutations plus a zip bomb.
- **Acceptance:** a `.3mf` from a slicer opens as a body with a stated limitation, and the boolean
  command's refusal message names it.
- **Risk:** users will want to boolean against imported meshes immediately. Fallback: sew the faces
  (KER-SRF-005) and clear the flag; until then, refuse clearly.

---

# DAT-GLTF — the format the web already speaks

### DAT-032 — glTF 2.0 writer: JSON, buffers, and the coordinate change
- **State:** BLOCKY READY — depends REN-MAT-002
- **Depends on:** DAT-018, DAT-027, REN-MAT-002
- **Files:** `crates/io/src/d3/gltf.rs` (owner D)
- **Contract:**
  ```rust
  pub fn gltf_write(model: &cadcraft_model::Model, opts: &ExportOptions) -> Result<Vec<u8>, IoError>; // .gltf, buffers inline as data URIs
  pub fn glb_write(model: &cadcraft_model::Model, opts: &ExportOptions) -> Result<Vec<u8>, IoError>; // .glb, one binary chunk
  ```
- **Behaviour:** `.gltf` is JSON with a `buffers` array; the default writes each buffer as a
  `data:` URI so the result is one file, and `.glb` writes the binary chunk (`JSON`, `BIN`) instead.
  glTF is right-handed with **+Y up** and **−Z forward**, and its convention is metres; CADCraft's
  world is right-handed with +Z up and millimetres. The conversion is `z → y` and `mm → m`, applied
  once in the accessor writer, and it flips the triangle winding unless the projection is applied to
  both axes — this is the classic glTF upside-down-model bug and it is tested, not eyeballed.
- **Refusals:** An accessor byte length that is not a multiple of its component size →
  `Err(Validation(..))`. A buffer over `MAX_GLTF_ACCESSORS` → `Err(Limit)`. A body with a coordinate
  that overflows `f32` after the metre conversion → `Err(Format("body {name} exceeds the f32 range
  glTF requires"))` — this is a real limit and glTF has no f64 positions.
- **Tests:** `a_cube_is_a_valid_gltf` — accessors, bufferViews, meshes, nodes, scenes present.
  `the_up_axis_conversion_is_applied` — a cube built on the kernel's XY plane comes out with its
  20 mm height on glTF's +Y.
  `the_winding_survives_the_axis_change`. `a_gltf_and_a_glb_carry_the_same_mesh`.
  `a_coordinate_beyond_f32_is_refused`. `the_position_buffer_is_4_byte_aligned`.
- **Acceptance:** the Khronos validator reports no error (DAT-035).
- **Risk:** the axis change flips handedness if done wrong, and every triangle renders inside-out.
  Fallback: the volume computed from the written positions must equal the kernel's volume —
  asserted, and the test fails loudly if it does not.

### DAT-033 — glTF materials map onto PBR exactly
- **State:** BLOCKY READY — depends REN-MAT-002
- **Depends on:** DAT-032, REN-MAT-002, REN-MAT-011
- **Files:** `crates/io/src/d3/gltf.rs` (owner D)
- **Contract:** `pub fn gltf_material_json(m: &DocMaterial) -> serde_json::Value;`
- **Behaviour:** `pbrMetallicRoughness` with `baseColorFactor` in **linear** space — glTF is
  linear-light throughout, which is exactly what `scene3::Rgba` already holds
  (`scene3.rs:171-176`), so no decode happens here; the sRGB→linear decode belongs at document load
  (REN-MAT-003). `metallicFactor`, `roughnessFactor`, `emissiveFactor`, `alphaMode`, `doubleSided`.
- **Refusals:** A metallic or roughness outside `0..=1` is clamped and reported, because glTF's
  schema requires the range and a clamp is visible while an error is only confusing.
- **Tests:** `a_material_round_trips_to_json`. `the_colour_is_written_linear_not_srgb` — a colour
  chosen as `0x808080` writes the linear value, and reading it back gives the byte.
  `out_of_range_factors_are_clamped_and_reported`. `alpha_mode_and_double_sided_survive`.
- **Acceptance:** a viewer shows the material.
- **Risk:** the sRGB/linear confusion. Fallback: the round-trip test is the guard; keep it.

### DAT-034 — glTF reader, with the honest limitation on the result
- **State:** READY
- **Depends on:** DAT-032
- **Files:** `crates/io/src/d3/gltf.rs` (owner D)
- **Contract:** `fn read(bytes: &[u8], name: &str) -> Result<Import, IoError>;`
- **Behaviour:** Parses the JSON (`serde_json`, already a dependency), resolves `data:` URIs and,
  for `.glb`, the `BIN` chunk. `POSITION` gives the faces; `NORMAL` is **discarded** because the
  kernel's normals come from its own winding (invariant I4) and a mesh normal that disagrees would
  be a contradiction, not a refinement — recorded in `ConversionReport.lost` when present and
  non-zero. Indices that are absent mean a non-indexed triangle soup, which is legal and read as
  consecutive triples. The +Y-up/metre conversion is inverted on read.
- **Refusals:** `Err(Validation)` for an accessor whose `count` disagrees with its buffer length, a
  component type glTF cannot express here, or a primitive mode other than `TRIANGLES`
  (`POINTS`, `LINES`, `LINE_STRIP` are named and refused); `Err(Limit)` past
  `MAX_GLTF_ACCESSORS`; `Err(Truncated)` for a `.glb` whose chunk lengths do not sum to the file
  length.
- **Tests:** `a_gltf_round_trips_to_the_same_volume`. `a_glb_round_trips`. `a_data_uri_buffer_is_read`.
  `normals_are_discarded_and_reported`. `a_lines_primitive_names_itself`. `a_short_bin_chunk_is_truncated`.
  `hostile_gltf_never_panics` — 200 mutations plus a 1 MB JSON with a 4 GB declared buffer.
- **Acceptance:** the hostile test passes; the read path never allocates from an unchecked number.
- **Risk:** glTF's `KHR_draco_mesh_compression` is common in the wild and unsupported here.
  Fallback: `Err(Unsupported("KHR_draco_mesh_compression"))` naming the extension — a clear
  refusal beats a wrong mesh.

### DAT-035 — Validate glTF with the Khronos validator
- **State:** READY
- **Depends on:** DAT-032, DAT-033
- **Files:** `crates/io/tests/gltf_external.rs`, `crates/io/tests/data/gltf-reader-report.md` (owner D)
- **Contract:** `pub fn probe(bytes: &[u8], reader: &dyn Fn(&[u8], &str) -> Result<ProbeReport, String>) -> Result<(), String>;`
- **Behaviour:** Shells out to the Khronos `gltf-validator` (a Node tool) or, failing that, a
  viewer the report names. The report records the validator version, the command, the number of
  `errors`, `warnings` and `infos`, and the fixture verdicts.
- **Refusals:** Warnings are recorded and **counted**, not swallowed: a task is not done while the
  report's warning count is above the number the writer's own test justifies. No validator → `#[ignore]`.
- **Tests:** `the_probe_requires_a_reader`. `a_warning_count_above_the_justified_number_fails`.
  `the_report_records_the_validator_version`.
- **Acceptance:** the report names the validator and shows zero errors on every fixture.
- **Risk:** Node may not be present. Fallback: the report may name a different independent reader,
  but it must name one, with a version.

---

# DAT-JT — proprietary, and the honest answer is narrow

### DAT-036 — Write the ADR: what JT means for CADCraft
- **State:** READY
- **Depends on:** —
- **Files:** `docs/file-format-parity.md`, new `docs/adr/0002-jt-scope.md` (owner D)
- **Contract:** The ADR states: JT is specified by ISO 14304; reading it is a specification
  implementation, not a reverse-engineering exercise, and is permitted; **writing** it requires
  the Segenesis/Parasolid-derived encoding of the precise B-rep, which no public specification
  describes in enough detail; therefore CADCraft writes no JT, and reads tessellated geometry only.
- **Behaviour:** Records the decision, the reason, the alternative (buy a commercial JT library —
  rejected: it is proprietary code in a clean-room project, and `AGENTS.md` forbids linking
  non-OSI code), and the revisit condition (a CADCraft-authored tessellation export becomes worth
  JT once an assembly exists).
- **Refusals:** n/a.
- **Tests:** `the_adr_is_linked_from_the_file_format_parity_doc` — a test that the path in the
  parity doc resolves to a file that exists.
- **Acceptance:** the JT row of `docs/file-format-parity.md` says "read tessellation only; no writer"
  and links the ADR.
- **Risk:** someone later adds a JT writer without the ADR being revisited. Fallback: the
  `Format3::is_writable(Jt) == false` fact is in the table and tested.

### DAT-037 — JT reader skeleton: structure detection, and a named refusal
- **State:** BLOCKY READY — depends DAT-036
- **Depends on:** DAT-036
- **Files:** `crates/io/src/d3/jt.rs` (owner D)
- **Contract:** `fn read(bytes: &[u8], name: &str) -> Result<Import, IoError>;`
- **Behaviour:** Recognises the ISO 14303-1 file signature and reports precisely what it can do:
  identify the segment structure, list the product hierarchy, and refuse geometry with a message
  naming the segment type it stopped at. A reader that imports nothing but says exactly why is
  more useful than one that guesses.
- **Refusals:** `Err(Unsupported("JT segment <type> at offset <n>: the tessellated-geometry reader
  is not written"))` for every geometry segment until DAT-038; `Err(Format)` for a bad signature;
  `Err(Limit)` past a declared segment count.
- **Tests:** `a_bad_signature_is_a_format_error`. `the_product_hierarchy_is_readable_without_geometry`.
  `a_geometry_segment_is_refused_by_type_and_offset`. `hostile_jt_never_panics`.
- **Acceptance:** the structure reader opens a file whose geometry it refuses, and the refusal says what.
- **Risk:** half a feature. Fallback: the structure reader is genuinely useful for the assembly
  browser, so it is worth landing on its own.

### DAT-038 — JT tessellated geometry into a body
- **State:** BLOCKED
- **Depends on:** DAT-037, MOD-004
- **Files:** `crates/io/src/d3/jt.rs` (owner D)
- **Contract:** `pub fn jt_read_tessellation(bytes: &[u8], name: &str) -> Result<Import, IoError>;`
- **Behaviour:** Reads the tessellated shape segments, and applies **level of detail** selection by
  taking the coarsest LOD that is above a stated triangle budget (`MAX_JT_TESSELLATION_TRIS`), which
  is what a viewer does and what a reader should do by default. Each triangle becomes a planar face,
  exactly as DAT-031 does, and the result carries the same `is_mesh` limitation.
- **Refusals:** `Err(Limit)` past the triangle budget; `Err(Validation)` for an index out of range or
  a vertex count of zero in a segment that claims vertices.
- **Tests:** `the_coarsest_usable_lod_is_chosen`. `a_file_whose_finest_lod_passes_the_budget_is_refused`.
  `triangles_become_planar_faces`. `hostile_jt_never_panics`.
- **Acceptance:** validated against an independent JT reader (a viewer) with the report naming it.
- **Risk:** the precise B-rep segments will hit the refusal path, which is correct.
  Fallback: none; the refusal is the product.

---

# DAT-UNIT — one authoritative unit system, converted at every boundary

The kernel's unit is the millimetre. That is a fact in the code, not a choice made here:
`step.rs:90-94` says the `LENGTH_UNIT` is `SI_UNIT(.MILLI.,.METRE.)` "and the conversion is not
performed anywhere — coordinates go out in kernel units, and the kernel works in millimetres", and
`tolerance_for(diagonal) = diagonal * 1e-9` is a millimetre-scale tolerance.

### DAT-039 — Decide the home of the unit types, and do not add a crate for them
- **State:** READY
- **Depends on:** —
- **Files:** new `crates/geom/src/units.rs`, `crates/geom/src/lib.rs` (owner D), `xtask/src/layers.rs`
  (**no change needed** — `geom` is already `Class::Layer(0)`)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub enum UnitSystem { Millimetre, Centimetre, Metre, Inch, Foot, Microinch, Mil }
  impl UnitSystem {
      pub const fn to_mm(self) -> f64;
      pub const fn from_mm_factor(self) -> f64;
      pub fn parse(s: &str) -> Option<UnitSystem>;    // "mm", "cm", "m", "in", "ft", "µin", "mil"
      pub fn insunits(self) -> u16;                   // INSUNITS: 1=in, 4=mm, 5=cm, 6=m, 2=ft, 50=µin, 9=mil
      pub fn si_prefix(self) -> (&'static str, &'static str);
      pub fn name(self) -> &'static str;
  }
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum AngleUnit { Degrees, Radians, Gradians }
  ```
- **Behaviour:** `geom` is L0 and depends on no workspace crate, so `render` (L2), `model` (L3) and
  `io` (L3) can all reach it without an upward dependency. Adding a `cadcraft-units` crate would mean
  a `xtask/src/layers.rs` entry and a release of the whole workspace for one enum. The factors are
  `const` so there is no float parsing at a hot path; `to_mm` is exact for inch (`25.4`) and a
  rational expression for foot and mil.
- **Refusals:** `parse` of an unknown code is `None`, never a default. A unit whose factor is not
  finite (impossible today, checked anyway) is a named error at the conversion site.
- **Tests:** `inch_is_exactly_25_4_mm` — `UnitSystem::Inch.to_mm() == 25.4` exactly in `f64`.
  `every_unit_round_trips_through_mm` — for all seven, `to_mm` then `/to_mm` is 1 to 1e-15.
  `insunits_codes_are_the_documented_ones`. `parse_accepts_the_codes_and_rejects_the_rest`.
- **Acceptance:** `cargo xtask layers` is unchanged and green; the types compile on wasm (L0 is in
  `wasm_set()`).
- **Risk:** `geom` is the wrong home for a document-level concept. Fallback: if the model crate
  needs its own richer unit (with a `Format` carrying precision and an offset), `model` may add a
  wrapper; it may not move the factors.

### DAT-040 — A scale that carries tolerance with it
- **State:** READY
- **Depends on:** DAT-039
- **Files:** `crates/geom/src/units.rs` (owner D)
- **Contract:**
  ```rust
  pub struct Scale { pub factor: f64 }
  impl Scale {
      pub fn new(from: UnitSystem, to: UnitSystem) -> Scale;
      pub fn length(self, v: f64) -> f64;
      pub fn tolerance(self, t: f64) -> f64;   // same factor; a tolerance is a length
      pub fn area(self, v: f64) -> f64;        // factor squared
      pub fn volume(self, v: f64) -> f64;      // factor cubed
  }
  ```
- **Behaviour:** Converting a *tolerance* with the linear factor while converting an *area* with the
  square is the class of bug that makes a scaled model fail validation in a way nobody can explain.
  One type that knows the exponents makes it impossible to apply the wrong one.
- **Refusals:** `Scale::new` with a non-finite factor (impossible from the `const` table; checked)
  is refused. A length whose conversion is not finite is refused by the caller as a named error.
- **Tests:** `a_tolerance_scales_like_a_length`. `an_area_scales_squared`. `a_volume_scales_cubed`.
  `an_inch_to_millimetre_scale_turns_25_4_into_25_4`.
- **Acceptance:** used by DAT-005, DAT-024, DAT-032 and DAT-042 without any of them computing a
  factor.
- **Risk:** n/a.

### DAT-041 — Angles: the kernel is in radians and the user is not
- **State:** READY
- **Depends on:** DAT-039
- **Files:** `crates/geom/src/units.rs`, `crates/render/src/units.rs` (owner D + REN)
- **Contract:**
  ```rust
  impl AngleUnit { pub fn to_rad(self, v: f64) -> f64; pub fn from_rad(self, v: f64) -> f64; pub fn insunits_suffix(self) -> u16; }
  ```
- **Behaviour:** The kernel's `revolve` takes `f64` radians; `Plane` angles are radians; the UI and
  every interchange format except IGES/STEP's angle unit speak degrees. The conversion happens at
  four boundaries and nowhere else: the command line, the parameter panel, the dimension text, and
  the format writers.
- **Refusals:** A non-finite angle is `Err`, not `0.0`.
- **Tests:** `a_revolve_of_90_degrees_sweeps_half_turn` — via the command, not the kernel.
  `degrees_and_radians_round_trip` to 1e-12. `a_non_finite_angle_is_refused`.
- **Acceptance:** `solid.feature.revolve` accepts `angle` in the document's angle unit and the
  geometry is right.
- **Risk:** a reader of the `.ccm` sees radians while the UI shows degrees. Fallback: DAT-042
  stores the unit.

### DAT-042 — The document owns its unit
- **State:** BLOCKY READY — depends DAT-FMT-002
- **Depends on:** DAT-039, DAT-FMT-002
- **Files:** `crates/model/src/file.rs`, `crates/model/src/lib.rs` (owner D, co-owned with MOD)
- **Contract:**
  ```rust
  // in Document, #[serde(default)]
  pub unit: UnitSystem,               // default Millimetre for a v1 file
  pub angle_unit: AngleUnit,          // default Radians
  ```
- **Behaviour:** Every number in a document, in every format, is in the document's unit. A `.ccm`
  opened in a build whose `Format3` supports the unit reads the unit from the file; one whose
  kernel cannot resolve a feature at that scale reports it.
- **Refusals:** A `.ccm` whose unit is finer than `tolerance_for`'s floor (`1e-12` mm = 1 pm) is
  refused at load with `Err(ModelError::Expression("unit … is below the resolvable precision"))`.
  A unit whose feature scale makes a body sub-tolerance is refused at rebuild, naming the feature.
- **Tests:** `a_v1_file_defaults_to_millimetres`. `the_unit_survives_a_save_and_load`.
  `a_unit_below_the_precision_floor_is_refused`. `a_feature_that_becomes_sub_tolerance_is_refused_with_its_name`.
- **Acceptance:** a document authored in inches saves in inches and reopens in inches.
- **Risk:** an inch document makes every kernel tolerance 25.4× coarser, and the open boolean
  defects may behave differently. Fallback: refuse inch authoring until `KER-BOOL-001..003` are
  validated; read-only inch support first.

### DAT-043 — Convert at the DXF/DWG boundary too
- **State:** READY
- **Depends on:** DAT-039, DAT-042
- **Files:** `crates/io/src/lib.rs::{read, write_with}` (owner D)
- **Contract:** `pub fn doc_unit(d: &Drawing) -> UnitSystem` — `$INSUNITS` from the DXF header,
  `Millimetre` when absent; `pub fn apply_unit(d: &mut Drawing, unit: UnitSystem) -> Result<(), IoError>;`
- **Behaviour:** The 2D side already has an authoritative unit concept (AutoCAD's `$INSUNITS` and
  `$MEASUREMENT`); it is read and written by the DXF layer. When a 3D document is exported to DXF
  (a 3D model has no 2D drawing, so this is for a projected view — cross-ref DRW-001) the conversion
  happens once, at the boundary, and `$INSUNITS` is set to match.
- **Refusals:** An `$INSUNITS` value CADCraft does not model is read as millimetres and reported in
  `d.read_notes` — the existing convention at `lib.rs:5,66`.
- **Tests:** `the_dxf_unit_is_read_from_insunits`. `an_unmodelled_insunits_is_reported`.
  `a_projection_is_written_in_the_documents_unit`. `the_2d_default_is_unchanged` — a drawing with
  no `$INSUNITS` still writes exactly what it wrote before.
- **Acceptance:** the 58 2D tests pass unchanged.
- **Risk:** changing `$INSUNITS` on an existing drawing changes its meaning. Fallback: only set it
  when the drawing had none.

### DAT-044 — Every adapter declares the unit it writes and the one it reads
- **State:** READY
- **Depends on:** DAT-039, DAT-040
- **Files:** `crates/io/src/d3/mod.rs` (owner D)
- **Contract:**
  ```rust
  pub trait Exchange3 {
      // … DAT-018 …
      fn native_unit() -> UnitSystem;                 // what the format means by a bare number
      fn unit_suffix(self, u: UnitSystem) -> Option<&'static str>;  // None = only the native unit exists
  }
  ```
- **Behaviour:** The table, one row per format, is the single answer to "in what unit is this file?":
  STEP → any, via `SI_UNIT`/`CONVERSION_BASED_UNIT`; IGES → a flag, seven choices; glTF → metres,
  fixed; 3MF → millimetres, fixed; JT → millimetres, fixed; DXF → `$INSUNITS`. A format that cannot
  express a unit the document uses must say so at export time, not after the file is opened.
- **Refusals:** `gltf_write` with `opts.unit == Some(Inch)` is `Err(Unsupported("glTF has no unit
  field: it is metres by definition"))`. `threemf_write` with a non-millimetre unit is the same.
- **Tests:** `every_format_declares_its_native_unit`. `a_format_without_a_unit_field_refuses_a_non_native_unit`.
  `the_table_matches_the_specification` — one test per row asserting the expected value, with the
  specification section named in the assertion message.
- **Acceptance:** the table exists, is tested, and each adapter's module doc repeats its row.
- **Risk:** a unit field that exists but is advisory. Fallback: the `unit_suffix` returns `Option`,
  and `None` means "conversion happens silently in the adapter", which is recorded in the doc.

### DAT-045 — Display numbers in the document's unit
- **State:** READY
- **Depends on:** DAT-042, DAT-041
- **Files:** `crates/render/src/units.rs`, `crates/ui-egui/src/parametric.rs` (owner D + REN + UI)
- **Contract:** `pub fn format_in(v_mm: f64, unit: UnitSystem, f: &NumFormat) -> (String, String);`
  — the existing `format_linear` is called on a converted value, and the caller supplies the unit.
- **Behaviour:** `cadcraft_render::units::format_linear` formats a *drawing unit* number; it knows
  nothing about units. The conversion is one line at the call site and a test per unit that the
  displayed string is the number the user typed, not the millimetre value.
- **Refusals:** A non-finite value formats as `0` (the existing behaviour, `units.rs:41`) — kept.
- **Tests:** `a_25_4_mm_value_displays_as_1_inch`. `the_displayed_value_round_trips_through_a_parse`.
  `fractional_inches_format_correctly` — 25.4 mm is `1"` exactly, and 6.35 mm is `1/4"`.
  `engineering_and_architectural_formats_still_work` — the existing tests pass unchanged.
- **Acceptance:** a parameter panel showing inches shows what the user typed.
- **Risk:** the existing `NumFormat` is a *dimension style*, not a unit; mixing the two would make
  `DIMLUNIT` override the document unit. Fallback: the unit conversion is applied *before*
  `format_linear`, so `DIMLUNIT` still governs only the number's shape.

### DAT-046 — The tolerance budget across units
- **State:** READY
- **Depends on:** DAT-040
- **Files:** `crates/geom/src/units.rs` (owner D)
- **Contract:** `pub fn min_resolvable_length(unit: UnitSystem) -> f64` — the smallest `f64` a
  coordinate in `unit` can represent at the largest plausible magnitude, documented as the
  `MAX_COORDINATE` of `check_window` (`lib.rs:138`) divided by the factor.
- **Behaviour:** `crates/io/src/lib.rs:143` already refuses a window whose side is below `1e-9` of
  the largest coordinate, so the pattern exists; this makes the same bound addressable from a unit.
  A document whose working scale is within one `f64` epsilon of its own tolerance is refused at
  load, with the number in the message.
- **Refusals:** `Err` at load, naming both the coordinate magnitude and the tolerance.
- **Tests:** `an_inch_document_at_a_metre_magnitude_is_refused`. `a_millimetre_document_at_the_same_magnitude_is_accepted`.
  `the_bound_matches_the_dxf_window_check` — the same threshold as `check_window` for millimetres.
- **Acceptance:** the refusal exists and its message contains both numbers.
- **Risk:** n/a.

---

# DAT-XREF — a topology cross-reference between CADCraft and another CAD

The 2D side already has external references: `crates/io/src/dxf_xref.rs` (XRECORD handles),
`crates/doc`'s xref table, the engine's `xref`/`-xref`/`xattach`/`xopen` commands and the palette in
`crates/ui-egui/src/xrefs.rs`. The 3D version must reuse those conventions, not invent a second one.

### DAT-047 — State what a 3D xref can and cannot be, today
- **State:** READY
- **Depends on:** —
- **Files:** `docs/file-format-parity.md`, `crates/io/src/d3/xref.rs` module doc (owner D)
- **Contract:** module doc stating: an edge is referred to by a geometric point
  (`cadcraft_model::EdgeRef { point: [f64;3], tolerance: f64 }`), **there is no topological naming**
  (`crates/model/src/lib.rs:35-41`), therefore a cross-reference map is only as stable as the
  geometry and will break on any edit that moves an edge. `MOD-002` is what makes it durable.
- **Behaviour:** Writes the limitation in the first paragraph, exactly as `step.rs` writes its own,
  so nobody builds on the assumption that it is robust. Ships a read-only *detector* (does the
  external file still contain this solid?) and a *verifier* (does every mapped edge still resolve?).
  It does not ship an *applier* (push a change back into the other CAD) — that is a different and
  larger product, and the file says so.
- **Refusals:** An entry whose edge no longer resolves is `Err(IoError::Validation("edge {i} no
  longer resolves in {body}"))`, never silently dropped.
- **Tests:** `the_module_doc_names_the_limitation`. `a_stale_map_entry_is_named_not_dropped`.
  `a_changed_external_file_is_detected`.
- **Acceptance:** the doc is linked from the parity doc and the verifier exists.
- **Risk:** shipping a map that silently mis-resolves is the failure this whole domain fears.
  Fallback: never resolve an entry without reporting the distance to the nearest edge, and refuse
  above `EdgeRef.tolerance`.

### DAT-048 — The manifest file format
- **State:** READY
- **Depends on:** DAT-047
- **Files:** `crates/io/src/d3/xref.rs` (owner D)
- **Contract:**
  ```rust
  #[derive(Serialize, Deserialize)]
  pub struct XrefManifest {
      pub magic: String,                    // "CADCraft 3D xref"
      pub version: u32,
      pub source: XrefSource,               // { path_relative, sha256, units }
      pub entries: Vec<XrefEntry>,
  }
  #[derive(Serialize, Deserialize)]
  pub struct XrefEntry {
      pub feature: String,                  // CADCraft feature name
      pub external_id: String,              // the other CAD's persistent id, verbatim
      pub edges: Vec<[f64; 3]>,             // geometric EdgeRef points, in millimetres
      pub tolerance: f64,
  }
  pub fn xref_write(m: &XrefManifest) -> Result<Vec<u8>, IoError>;
  pub fn xref_read(bytes: &[u8]) -> Result<XrefManifest, IoError>;
  ```
- **Behaviour:** JSON, `serde_json` already present, stored beside the host document as
  `<document>.xref.json` — **not** inside the `.ccm`, because the external file's identity changes
  when the host does not. `sha256` is over the external file's bytes, so a change is detectable even
  when the mtime lies. Paths are stored **relative to the host document's folder**, which is the
  existing `relative_to_host` convention in `crates/ui-egui/src/xrefs.rs:51`.
- **Refusals:** `Err(Format)` for a wrong magic or `version > FORMAT_VERSION`; `Err(Validation)` for a
  malformed hex digest, a path escaping the host folder (`../`), or an empty feature name.
- **Tests:** `a_manifest_round_trips`. `a_wrong_magic_is_a_format_error`. `a_newer_version_is_refused`.
  `a_path_escaping_the_host_folder_is_refused`. `paths_are_stored_relative`.
- **Acceptance:** a manifest survives a save/load and a folder move.
- **Risk:** a manifest is a new file kind in a user's folder. Fallback: the folder name is derived
  from the document and documented in the module doc.

### DAT-049 — Detect that the external file changed
- **State:** READY
- **Depends on:** DAT-048
- **Files:** `crates/io/src/d3/xref.rs` (owner D)
- **Contract:**
  ```rust
  pub enum XrefFreshness { Current, Changed, Missing, Unreadable(String) }
  pub fn xref_freshness(manifest: &XrefManifest, host_dir: &std::path::Path) -> XrefFreshness;
  ```
- **Behaviour:** Reads the external file and compares the digest. `Changed` and `Missing` are
  reported as state, never acted on automatically: re-binding without asking is how a user's edit
  disappears. The digest is computed streaming, not by reading the whole file into memory.
- **Refusals:** An unreadable file is `Unreadable(reason)` — never `Missing`, so "permission denied"
  and "deleted" stay distinguishable.
- **Tests:** `an_unchanged_file_is_current`. `a_changed_file_is_changed`. `a_deleted_file_is_missing`.
  `a_permission_error_is_unreadable_not_missing`. `a_large_file_is_hashed_streaming` — a 200 MB file
  does not become a 200 MB allocation.
- **Acceptance:** the four states are distinct in the tests.
- **Risk:** hashing a 2 GB assembly on every open. Fallback: hash the size and mtime first and only
  hash when they differ, and say which was used.

### DAT-050 — Verify every mapped edge still resolves
- **State:** READY
- **Depends on:** DAT-048, DAT-047
- **Files:** `crates/io/src/d3/xref.rs` (owner D)
- **Contract:**
  ```rust
  pub struct XrefVerdict { pub entry: usize, pub edge: usize, pub distance: f64, pub ok: bool }
  pub fn xref_verify(manifest: &XrefManifest, model: &cadcraft_model::Model) -> Result<Vec<XrefVerdict>, IoError>;
  ```
- **Behaviour:** For each mapped point, finds the nearest edge of the named feature's solid and
  reports the distance. `ok` means the distance is within the entry's `tolerance`. A feature that no
  longer exists is a verdict with `distance = f64::INFINITY` and `ok = false`, not a skip.
- **Refusals:** A model with no solid is `Err(IoError::Validation("the document has no solid to verify against"))`.
  A distance that is not finite is a failed verdict.
- **Tests:** `every_entry_of_an_unchanged_document_verifies`. `a_moved_edge_is_reported_with_its_distance`.
  `a_deleted_feature_is_a_failed_verdict`. `a_non_finite_distance_fails`. `verifying_never_mutates_the_model`.
- **Acceptance:** a rebuild that moves an edge shows up in the panel without a user action.
- **Risk:** nearest-edge search is O(edges) per point. Fallback: a grid over the feature's own edges,
  built once, in a later task; the budget is in DAT-051.

### DAT-051 — Verification performance
- **State:** READY
- **Depends on:** DAT-050
- **Files:** `crates/io/src/d3/xref.rs` (owner D)
- **Contract:** `pub fn xref_verify_bounded(manifest: &XrefManifest, model: &Model, max_points: usize) -> Result<Vec<XrefVerdict>, IoError>;`
- **Behaviour:** Caps the number of points checked in one pass (`max_points`), reports how many were
  skipped in a `warnings` entry, and returns on the next pass where it left off. A 10⁵-edge
  verification is a batch, not a frame.
- **Refusals:** `Err(Limit)` past the cap; the caller must page, not raise the cap silently.
- **Tests:** `the_cap_reports_what_it_skipped`. `paging_covers_every_point_exactly_once`.
  `a_paged_verification_gives_the_same_verdicts_as_a_single_pass`.
- **Acceptance:** the three tests pass.
- **Risk:** n/a.

### DAT-052 — Read an external CAD's identity out of its file
- **State:** BLOCKY READY — depends DAT-014
- **Depends on:** DAT-014, DAT-048
- **Files:** `crates/io/src/d3/step.rs`, `crates/io/src/d3/xref.rs` (owner D)
- **Contract:**
  ```rust
  /// The `PRODUCT` id and name a STEP file assigns to its single solid, if any.
  pub fn step_product_identity(bytes: &[u8]) -> Result<Option<(String, String)>, IoError>;
  ```
- **Behaviour:** After DAT-006/DAT-008 the product name is a stable, writer-chosen identifier; a map
  can key on it. For a file written by *another* CAD the name is whatever that CAD chose, which is
  why `XrefEntry.external_id` is stored verbatim and the module doc says it is only meaningful to
  the tool that wrote it.
- **Refusals:** A file with several products and no `NEXT_ASSEMBLY_USAGE_OCCURRENCE` returns
  `Ok(None)` — not an error, not a guess.
- **Tests:** `our_own_export_carries_the_product_name`. `a_foreign_product_name_is_stored_verbatim`.
  `several_unrelated_products_return_none`.
- **Acceptance:** a `.ccm` exported and re-imported finds its own identity.
- **Risk:** another CAD rewrites the product name on every save, so the id is not durable.
  Fallback: the digest check (DAT-049) is the real staleness signal; the id is a convenience.

### DAT-053 — The engine command surface for 3D xrefs
- **State:** BLOCKED
- **Depends on:** DAT-048, DAT-050
- **Files:** `crates/engine/src/cmd/` (owner C, co-sequenced)
- **Contract:** `CommandSpec` ids `solid.xref.attach`, `solid.xref.verify`, `solid.xref.detach`,
  mirroring the 2D `xref`/`-xref`/`xattach`/`xopen` naming and JSON shape.
- **Behaviour:** `solid.xref.attach {path}` → `{path, entries, freshness}`; `solid.xref.verify {}` →
  `{verdicts, failed}`; `solid.xref.detach {path?}`. All three run under `Session::execute`'s
  `catch_unwind` guard like every other command (`AGENTS.md`).
- **Refusals:** A path that is not an exchange file → `Err` naming the extension and what is
  supported. Every failure names the entry.
- **Tests:** `attach_verify_detach_round_trip_through_the_control_channel`.
  `a_hostile_manifest_returns_a_named_error_not_a_panic`.
  `the_commands_appear_in_engine_commands`.
- **Acceptance:** `cargo xtask ci` green; the commands reach the palette as 2D ones do.
- **Risk:** engine files are another owner's. Fallback: the adapter lands first; the commands are
  requested through the orchestrator.

---

# DAT-FMT — `.ccm` v2, migrations, atomic save, corrupt-file recovery

`crates/model/src/file.rs` is 86 lines and makes two commitments: the file holds the **history, not
the geometry**, and a file whose features do not build **still loads**. Both survive into v2.

### DAT-054 — Write down what v2 must add, before writing it
- **State:** READY
- **Depends on:** —
- **Files:** new `docs/plan/DAT-FMT-v2.md` (owner D), cross-linked from `crates/model/src/file.rs` (owner D)
- **Contract:** a table of every field v2 adds: `unit`, `angle_unit`, `metadata`, `bodies`,
  `constraints`, `history`, `views`, `materials`, `xrefs` — each with the task that adds it and the
  task that must land first.
- **Behaviour:** Written first, on purpose: `.ccm` is the one format a user's work lives in, and a
  field added without a migration is a field that loses work. Every field names whether it has a
  `#[serde(default)]`, which is what makes a v1 file loadable.
- **Refusals:** n/a.
- **Tests:** `every_v2_field_is_listed_with_its_task` — a test that parses the markdown and asserts
  every `pub ` field of `Document` appears in it. A field added to the struct without a line here
  fails the build.
- **Acceptance:** the test passes and the file is linked from `file.rs`'s module doc.
- **Risk:** n/a.

### DAT-055 — `FORMAT_VERSION = 2` and the version rule
- **State:** READY
- **Depends on:** DAT-054
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:** `pub const FORMAT_VERSION: u32 = 2;` — and the existing rule kept verbatim: a file
  whose `version` is **greater** than this build reads is `Err(ModelError::TooNew { found, supported })`,
  never a best-effort load.
- **Behaviour:** A v1 file loads in a v2 build (every v2 field has `#[serde(default)]`), so
  `FORMAT_VERSION` is the *newest this build reads*, exactly as the doc comment says today. A v1
  file's geometry comes back rebuilt by the current kernel, which is the point of storing history.
- **Refusals:** `version == 0` is `Err(NotAModel)` (a file with the right magic and no version is
  not a document). `version > FORMAT_VERSION` is `TooNew`.
- **Tests:** `a_v1_document_loads_in_a_v2_build` — a committed-as-a-string v1 JSON literal loads and
  rebuilds. `a_v3_document_is_too_new`. `the_magic_is_still_checked_first`.
- **Acceptance:** the v1 fixture suite still loads; no v1 test is deleted.
- **Risk:** a v2 field whose default is *not* what a v1 file meant (a unit defaulting to mm when
  the v1 file was authored in inches — impossible today, possible after DAT-042).
  Fallback: `DAT-042` makes the v1 default explicit and tested.

### DAT-056 — The migration is a function, not a `serde(default)` pile
- **State:** READY
- **Depends on:** DAT-055
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:**
  ```rust
  pub fn migrate(raw: serde_json::Value) -> Result<Document, ModelError>;
  // Applies a chain of versioned steps: 1 -> 2 -> 3 …
  pub struct Document { …, #[serde(skip_deserializing)] pub from_version: u32 }
  ```
- **Behaviour:** Parses into a `serde_json::Value`, reads `version`, then applies each step in
  order, each step a named function `migrate_v1_to_v2(v: Value) -> Result<Value, ModelError>`. A v2
  file takes the zero-length path and is byte-identical. `from_version` is recorded so a caller can
  tell a migrated file from a native one — needed by the "this file was upgraded" message.
- **Refusals:** An unknown *lower* version (0) is `NotAModel`. A step that fails on a field it needs
  is `Err(ModelError::CorruptHistory("migrating v{n}: …"))` naming the step.
- **Tests:** `a_v1_file_migrates_to_v2` — `from_version == 1`, and the result equals a natively
  authored v2 document. `a_v2_file_takes_no_steps`. `a_failed_step_names_itself`.
  `migrating_twice_is_the_same_as_migrating_once` — idempotence.
- **Acceptance:** migration is idempotent and every v1 fixture lands.
- **Risk:** a migration that changes geometry (because it must re-derive a default). Fallback: a
  migration never touches `FeatureKind`; if it must, it is refused and the file is not loaded.

### DAT-057 — Atomic save: the temp-then-rename contract
- **State:** READY
- **Depends on:** DAT-054
- **Files:** `crates/model/src/file.rs`, `crates/engine/src/cmd/solid.rs::run_save` (owner D)
- **Contract:**
  ```rust
  pub fn save_atomic(model: &Model, path: &std::path::Path) -> Result<u64, ModelError>; // bytes written
  ```
- **Behaviour:** The engine's `solid.save` already writes `path.cadcraft-tmp` and renames
  (`crates/engine/src/cmd/solid.rs:601`). Move that convention into one named function so every
  writer uses it: write to `<path>.cadcraft-tmp`, `flush`, `sync_all`, then `fs::rename`. The
  rename is the atomicity; nothing before it touches `path`.
- **Refusals:** A path whose parent does not exist → `Err` naming the parent. A rename across
  filesystems → `Err` naming the operation (not a silent copy, which is not atomic). A directory at
  the target path → `Err`.
- **Tests:** `a_failed_write_leaves_the_original_intact` — a full disk (or a read-only directory)
  leaves `path` byte-identical. `a_successful_save_leaves_no_temp_file`. `the_temp_name_is_the_one
  _the_engine_already_uses`. `a_save_is_byte_identical_twice` — JSON serialisation is deterministic
  because every field is a `BTreeMap` or a `Vec`.
- **Acceptance:** the temp file never survives a failed save.
- **Risk:** an antivirus or a second CADCraft holding the file open on Windows makes `rename` fail.
  Fallback: retry a bounded number of times with a short sleep, then `Err` naming the failure — a
  clear error, not a corrupted save.

### DAT-058 — Durability: fsync the file and the directory
- **State:** READY
- **Depends on:** DAT-057
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:** unchanged `save_atomic` signature; behaviour: `File::sync_all` before the rename and
  a directory `File::open(parent)?.sync_all()` after it, on platforms that support it.
- **Behaviour:** Without the directory sync, the rename can be lost in a power failure even though the
  data was written — the exact failure that loses a drawing. On a platform where opening a directory
  fails, the directory sync is skipped and **the skip is documented in the function's doc**, not
  hidden.
- **Refusals:** A directory sync that fails on a platform that supports it → `Err` naming it: the
  save is reported as not durable rather than as done.
- **Tests:** `the_file_is_synced_before_the_rename` — observable through a mock or a code read; on a
  platform without the capability the test asserts the documented skip instead of failing.
  `a_directory_sync_failure_is_reported`.
- **Acceptance:** the sequence is in the code and both paths are documented.
- **Risk:** `sync_all` costs milliseconds on a spinning disk. Fallback: none needed; a save is rare
  and this is the one place where milliseconds are worth it.

### DAT-059 — Bounded read: a 10 GB `.ccm` must not become a 10 GB allocation
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:**
  ```rust
  pub const MAX_DOCUMENT_BYTES: usize = 512 * 1024 * 1024;
  pub fn load_bytes(bytes: &[u8]) -> Result<(Model, Rebuild), ModelError>;
  ```
- **Behaviour:** `Model::load_json` today takes a `&str` and will allocate whatever it is given.
  `load_bytes` checks the length against `MAX_DOCUMENT_BYTES` first, and `save_atomic` checks the
  produced length. The 2D side already learned this the hard way — a 10 MB DWG that failed after two
  minutes is written down in `docs/roadmap.md:39`.
- **Refusals:** A file over the cap is `Err(ModelError::CorruptHistory("the document is {n} bytes; the
  limit is {max}"))` — never a partial parse.
- **Tests:** `an_oversized_document_is_refused_before_parsing`. `a_document_at_the_cap_loads`.
  `a_zero_byte_file_is_a_format_error`. `hostile_documents_never_panic` — 200 mutations of a valid
  `.ccm`.
- **Acceptance:** the cap is named in the module doc next to `FORMAT_VERSION`.
- **Risk:** a legitimate 600 MB assembly document. Fallback: raise the cap deliberately; never
  silently.

### DAT-060 — Corruption: refuse, salvage, or both
- **State:** READY
- **Depends on:** DAT-059
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:**
  ```rust
  pub enum Salvage {
      Clean,
      Recovered { features: usize, parameters: usize, dropped: Vec<String> },
      Unusable { reason: String },
  }
  pub fn salvage(bytes: &[u8]) -> Result<(Model, Rebuild, Salvage), ModelError>;
  ```
- **Behaviour:** A `.ccm` that does not parse returns `Err`, as today. `salvage` is the *separate*,
  explicitly-invoked path — used by the recovery flow, not by `load`. It parses what it can and
  names every dropped feature in `dropped`, so the user is told exactly what was lost rather than
  finding out later.
- **Refusals:** A file whose `magic` is wrong is `Unusable` — salvaging a `.dxf` as a model would
  produce nonsense. A file that parses but has no features is `Recovered { features: 0, .. }`, not
  `Clean`.
- **Tests:** `a_truncated_document_salvages_what_it_can_and_names_the_loss`.
  `a_wrong_magic_is_unusable`. `salvage_never_mutates_anything_on_disk`. `a_salvaged_model_rebuilds`.
- **Acceptance:** a truncated file yields a model with the first N features and a named list.
- **Risk:** a salvage that produces a *plausible but wrong* model. Fallback: `Salvage::Recovered`
  is never presented as a clean load; the shell must show the dropped list.

### DAT-061 — Corruption: a feature that will not deserialize
- **State:** READY
- **Depends on:** DAT-056
- **Files:** `crates/model/src/feature.rs` (owner D, co-owned with MOD)
- **Contract:** `FeatureKind` gains `#[serde(other)]`-equivalent handling via a
  `pub enum FeatureKind` with an `Unknown { kind: String, raw: serde_json::Value }` variant.
- **Behaviour:** A feature kind written by a *newer* build (or by a build that had an extra kind)
  survives a round trip through an older one: it loads as `Unknown`, is named in the tree as
  "unrecognised feature", does not build, and is written back unchanged. This is what makes a
  forward-compatible file possible without bumping `FORMAT_VERSION`.
- **Refusals:** An `Unknown` feature blocks the features below it and says so — the existing
  `State::Blocked` mechanism carries it. It is never silently dropped from the file.
- **Tests:** `an_unknown_feature_kind_round_trips_unchanged`. `an_unknown_feature_blocks_the_ones_below`.
  `an_unknown_feature_is_named_in_the_tree`. `a_known_feature_is_unaffected`.
- **Acceptance:** a file written by this build opens in the next one.
- **Risk:** `serde`'s `#[serde(other)]` only works on unit variants; a data-carrying variant needs a
  manual `Deserialize`. Fallback: a hand-written impl, which is the plan.

### DAT-062 — The content hash and the writer version
- **State:** READY
- **Depends on:** DAT-055
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:** `Document { magic, version, #[serde(default)] content_hash: String, #[serde(default)] written_by: String, … }`
- **Behaviour:** `content_hash` is over the serialised document with the field itself blanked, so it
  verifies integrity and not the writer's mood. `written_by` is `env!("CARGO_PKG_VERSION")` plus the
  build SHA when present. Both are informational — a mismatch is a warning, not a refusal, because a
  hash computed by a different field order would otherwise lock a user out of their own file.
- **Refusals:** A malformed hash string is a warning, never a refusal. The refusal cases are
  `magic` and `version` only.
- **Tests:** `the_hash_verifies_a_tampered_document`. `a_hash_from_a_different_writer_is_a_warning`.
  `a_malformed_hash_is_a_warning`. `the_hash_excludes_itself`.
- **Acceptance:** the hash is recomputable from the loaded document.
- **Risk:** a hash algorithm change between builds makes every old file warn.
  Fallback: prefix the field with the algorithm (`"fnv1a64:…"`), so a change is legible.

### DAT-063 — Compressed documents
- **State:** BACKLOG
- **Depends on:** DAT-057, DAT-059
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:** `Document.compression: Option<String>` — `"gzip"`, or `None` for the plain JSON of
  today.
- **Behaviour:** `miniz_oxide` is already a dependency of `cadcraft-io` (`Cargo.toml:22`) but
  **not** of `cadcraft-model`; adding it means editing a `Cargo.toml`, which is a co-sequenced
  change with the orchestrator. Why: a history that is mostly numbers compresses 10–20×, and a
  large assembly document is the case where that matters. JSON stays the default so the file stays
  inspectable, which is worth more than the bytes until a corpus says otherwise.
- **Refusals:** `Err` for an unknown compression name and for a decompressed size over
  `MAX_DOCUMENT_BYTES` (the zip-bomb case).
- **Tests:** `a_gzip_document_round_trips`. `a_compressed_document_with_a_bomb_decompressed_size_is_refused`.
  `the_plain_format_still_round_trips`.
- **Acceptance:** a 10 000-feature document compresses to under a quarter of its plain size.
- **Risk:** a new dependency in L3. Fallback: keep it BACKLOG until a corpus file justifies it.

### DAT-064 — A published schema and a published change log
- **State:** READY
- **Depends on:** DAT-055
- **Files:** `docs/model-file-format.md` (owner D)
- **Contract:** the document describes every field, its `#[serde(default)]`, the migration chain,
  and the compatibility promise: *a file written by build N opens in every later build; a file
  written by a later build may refuse to open in an earlier one, with a named error.*
- **Behaviour:** The house convention (`step.rs`, `scene3.rs`, `picking.rs`) is that a public item's
  doc says what it does **not** do. This document applies it to the format.
- **Refusals:** n/a.
- **Tests:** `every_document_field_is_documented` — a test that walks `Document`'s fields (via
  `serde_json`'s reflected schema or a manual list) and asserts each appears in the markdown.
- **Acceptance:** the test passes; the file is linked from `crates/model/src/file.rs`.
- **Risk:** n/a.

### DAT-065 — The corpus of format fixtures
- **State:** READY
- **Depends on:** DAT-055
- **Files:** `crates/model/tests/data/` (small, text), `storytold/cadcraft-corpus` (owner Q + D)
- **Contract:** `crates/model/tests/corpus.rs` walks `tests/data/*.ccm`, loads each, asserts it
  rebuilds and asserts `content_hash` matches.
- **Behaviour:** Small, hand-written `.ccm` files (a few kB each, our own output) are committed;
  large ones live in the corpus repo, per `AGENTS.md` ("never commit large binary fixtures here").
  A `.ccm` is JSON, so committing a small one commits nothing an attacker could use and nothing an
  `ATTRIBUTION.md` row cannot cover (see PLT-ASSET-008).
- **Refusals:** A fixture over 64 kB is refused by the corpus test with a message pointing at the
  corpus repo.
- **Tests:** `every_committed_fixture_loads_and_rebuilds`. `an_oversized_fixture_is_pointed_at_the_corpus`.
  `the_corpus_fixtures_load_when_present` — skipped, not failed, when the repo is not checked out.
- **Acceptance:** the corpus test is green with and without the corpus repo present.
- **Risk:** the corpus repo not existing yet. Fallback: the skip.

---

# DAT-CAD — camera setup, materials, document metadata, layer and colour mapping

### DAT-066 — Camera setup: reuse the viewport's own camera type
- **State:** READY
- **Depends on:** —
- **Files:** `crates/model/src/view.rs` (new; owner D), `crates/ui-egui/src/viewport_state.rs` (owner U — **not this agent's file; see the note**)
- **Contract:**
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
  pub struct SavedView {
      pub name: String,
      pub target: [f64; 3], pub distance: f64, pub azimuth: f64, pub elevation: f64,
      pub fov_degrees: f64, pub is_top: bool, pub is_bottom: bool, pub is_front: bool, pub is_back: bool, pub is_left: bool, pub is_right: bool,
  }
  ```
- **Behaviour:** The six fields are `cadcraft_ui_egui::viewport_state::OrbitCamera`'s own public
  fields, so a saved view is the camera the viewport already has, not a parallel type. The
  orthographic flags let a view be *an* axis view (AutoCAD's TOP/FRONT/… are orthographic; the
  orbit camera is perspective) — which is why the flags exist rather than an azimuth convention.
  `frames` in the engine's `3DORBIT`/`PLANESWITCH` vocabulary is the 2D precedent for the naming.
- **Refusals:** A view with a non-finite field is refused at save, naming the field.
- **Tests:** `a_view_round_trips`. `a_view_with_a_nan_is_refused`. `a_saved_view_converts_into_an_OrbitCamera`
  — in `crates/ui-egui`, since that type lives there.
- **Acceptance:** saving and restoring a view gives the same picture, to the pixel.
- **Risk:** the type lives in the UI crate and the model must not depend on L5. Fallback: the
  *serialised* form is this struct; the conversion to `OrbitCamera` is a one-line constructor call
  in the UI crate. The five `is_*` flags make the conversion lossless without the model importing
  anything from L5.

### DAT-067 — Standard and named views
- **State:** READY
- **Depends on:** DAT-066
- **Files:** `crates/model/src/view.rs`, `crates/engine/src/cmd/` (owner D + C)
- **Contract:** `CommandSpec` ids `solid.view.save {name}`, `solid.view.restore {name}`,
  `solid.view.standard {top|front|right|…}`, each returning the `SavedView` they landed on.
- **Behaviour:** The six standard views are *generated* from the model's bounds and the document's
  unit, never stored, so they follow the geometry. A named view is stored in `Document.views`.
  Restoring a view whose target is outside the document is allowed — a view of nothing is a view.
- **Refusals:** A duplicate view name → `Err` naming both. A name that is empty or over 120
  characters → `Err`. `restore` of an unknown name → `Err` listing the names.
- **Tests:** `the_six_standard_views_face_the_axes`. `a_named_view_round_trips_through_save_and_load`.
  `a_duplicate_name_is_refused`. `an_unknown_name_is_refused_and_lists_the_known_ones`.
- **Acceptance:** the commands appear in `cadcraft-cli commands` and in `cargo xtask parity`'s output.
- **Risk:** n/a.

### DAT-068 — Materials: name the collision before writing the type
- **State:** READY
- **Depends on**: —
- **Files:** `crates/model/src/material.rs` (new; owner D), `crates/render/src/scene3.rs` (**owner U
  in wave 1 — REN-MAT-001 asks for the change; this task is the decision only**)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct DocMaterial {
      pub name: String,
      pub base_color_linear: [f64; 3],   // LINEAR, like scene3::Rgba
      pub base_color_alpha: f64,
      pub metallic: f64, pub roughness: f64,
      pub emissive_linear: [f64; 3],
      pub alpha_mode: AlphaMode,          // Opaque | Mask{cutoff} | Blend
      pub double_sided: bool,
  }
  ```
- **Behaviour:** `cadcraft_render::scene3::Material { base, roughness, metallic }` exists but its
  own docs say the renderer **ignores `roughness`** and shades with one `max(dot(n,l),0)`
  (`scene3.rs:244-256, 281-294`). It is a light-model input, not a colour store. So v2 stores
  `DocMaterial` and the renderer converts it. Colours are stored **linear** because that is what
  `Rgba` holds (`scene3.rs:171-176`) and what glTF wants; the sRGB→linear decode happens once, at
  document load (REN-MAT-003).
- **Refusals:** A colour component that is non-finite is refused at load with the material name;
  `metallic`/`roughness` outside `0..=1` are clamped with a report, not refused, because they are
  sliders and a slider cannot produce an error the user can act on.
- **Tests:** `a_material_round_trips`. `a_non_finite_colour_is_refused_with_the_material_name`.
  `out_of_range_factors_are_clamped_and_reported`. `colours_are_linear_not_srgb` — a byte colour
  `0x808080` loads to the linear value and exports back to `0x808080`.
- **Acceptance:** the model does not depend on `cadcraft-render` (L3 may not depend on L2? — it may:
  L3 > L2 is downward-allowed, so it *may*. It still must not, or the model gains a GPU-shaped API).
  The test asserting `cargo xtask layers` is clean is the acceptance.
- **Risk:** `cadcraft-model` depending on `cadcraft-render` for `Rgba` would drag a rasteriser into
  the document. Fallback: `[f64; 3]` as above, with a conversion in the render crate.

### DAT-069 — Appearance: what a body looks like is not what the renderer shades
- **State:** READY
- **Depends on:** DAT-068
- **Files:** `crates/model/src/appearance.rs` (new; owner D)
- **Contract:**
  ```rust
  pub struct Appearance { pub body: String, pub material: String, pub overrides: Vec<FaceOverride> }
  pub struct FaceOverride { pub feature: String, pub face_index: usize, pub material: String }
  ```
- **Behaviour:** A document holds a material library and an appearance per body: one material, plus
  per-face overrides. Face indices are `usize` into `Solid::faces`, which is stable only for a fixed
  geometry — the same fragility as `EdgeRef` and stated the same way: an override that no longer
  names the face it was set for is dropped **with a report**, never silently re-targeted. (Note the
  contrast with `EdgeRef`, which *refuses*: a face index can be checked for exactness, a geometric
  edge reference cannot.)
- **Refusals:** A material name that is not in the library → `Err` at load naming the body. A face
  index past the face count → reported and dropped, not fatal, so a document with one stale override
  still opens.
- **Tests:** `an_appearance_round_trips`. `a_stale_face_index_is_dropped_with_a_report`.
  `an_unknown_material_name_is_refused`. `an_override_beats_the_body_material`.
- **Acceptance:** a document with a stale override loads and says what it dropped.
- **Risk:** an index-based override is exactly what `MOD-002` will make unnecessary.
  Fallback: carry the geometric fallback (`EdgeRef`-style) alongside the index from the start.

### DAT-070 — Document metadata
- **State:** READY
- **Depends on**: —
- **Files:** `crates/model/src/file.rs` (owner D)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  pub struct DocMeta { pub title: String, pub author: String, pub company: String,
                       pub part_number: String, pub revision: String,
                       #[serde(default)] pub created: String, #[serde(default)] pub modified: String }
  ```
- **Behaviour:** These are the strings STEP's `PRODUCT` and `FILE_NAME` carry and that the writer
  deliberately leaves empty today (`step.rs:95-99`, item **C.11**). Storing them in the document is
  what makes `provenance: true` meaningful. Dates are ISO-8601 `YYYY-MM-DDTHH:MM:SS`, **UTC**,
  stored as strings so no date crate and no local-time ambiguity enters the file.
- **Refusals:** A date string that is not ISO-8601 is a load **warning** and the field is cleared,
  not a refusal — metadata is not worth refusing a document over. A title over 200 bytes is
  truncated with a warning.
- **Tests:** `metadata_round_trips`. `a_bad_date_is_a_warning_not_a_refusal`. `a_long_title_is_truncated_with_a_warning`.
  `metadata_is_written_into_a_step_file_when_provenance_is_on` — cross-ref DAT-012.
- **Acceptance:** the About-style document-properties dialog can show all seven fields.
- **Risk:** n/a.

### DAT-071 — Layer and colour mapping to an external CAD
- **State:** BLOCKED
- **Depends on:** DAT-068, DAT-069, DAT-052
- **Files:** `crates/io/src/d3/mod.rs` (owner D)
- **Contract:**
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct ExportMap { pub layers: BTreeMap<String, String>, pub colors: BTreeMap<String, [u8; 4]> }
  pub fn default_export_map(model: &Model) -> ExportMap;
  ```
- **Behaviour:** CADCraft's 3D model has **no layers** — `Model { parameters, features, records }`
  has no layer table, and inventing one inside the interchange layer would be a second document
  model. So this task is BLOCKED on a decision (see the report), and its contract is written for
  the answer "a body plays the part of a layer". `default_export_map` derives one body → one
  layer → one colour, which is honest and is what DAT-010 exports.
- **Refusals:** Two bodies mapped to one layer with two different colours → the map is refused at
  export with both named, because a file cannot say two things at once.
- **Tests:** `the_default_map_is_one_layer_per_body`. `two_bodies_one_layer_two_colours_is_refused`.
  `the_map_round_trips_through_a_step_export`.
- **Acceptance:** a STEP file's product names match the map.
- **Risk:** n/a — the block is a missing concept, not a missing design.

### DAT-072 — Camera and metadata survive a round trip through STEP
- **State:** BLOCKED
- **Depends on:** DAT-070, DAT-012, DAT-066
- **Files:** `crates/io/src/d3/step.rs` (owner D)
- **Contract:** `StepWriterOptions` carries `views: &[SavedView]` and `meta: &DocMeta`; both are
  written as `DESCRIPTIVE_REPRESENTATION_ITEM` properties on the product definition.
- **Behaviour:** A camera in a STEP file is not a standard entity; it belongs in
  `PROPERTY_DEFINITION` + `REPRESENTATION` with a descriptive item, which is the legal way to carry
  non-geometric data on a product. On import the items are read back. A reader that ignores them
  shows a correct shape, which is the right failure.
- **Refusals:** A `SavedView` with a non-finite field is refused (DAT-066).
- **Tests:** `a_view_survives_a_step_round_trip`. `a_reader_that_ignores_properties_still_imports_the_shape`.
  `properties_do_not_disturb_the_entity_graph` — the root count stays 1.
- **Acceptance:** views round-trip; the reader report says no warning about the extra properties.
- **Risk:** a strict validator may warn about non-standard property names. Fallback: name them with
  a CADCraft prefix and record the warning in the report.

---

## Task count

| Epic | Tasks |
|---|---|
| `DAT-STEP` | DAT-001 … DAT-022 — 22 |
| `DAT-IGES` | DAT-023 … DAT-026 — 4 |
| `DAT-3MF` | DAT-027 … DAT-031 — 5 |
| `DAT-GLTF` | DAT-032 … DAT-035 — 4 |
| `DAT-JT` | DAT-036 … DAT-038 — 3 |
| `DAT-UNIT` | DAT-039 … DAT-046 — 8 |
| `DAT-XREF` | DAT-047 … DAT-053 — 7 |
| `DAT-FMT` | DAT-054 … DAT-065 — 12 |
| `DAT-CAD` | DAT-066 … DAT-072 — 7 |
| **total** | **72** |