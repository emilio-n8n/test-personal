//! Independent verification of what `step_faceted_brep` writes.
//!
//! # Why a second parser
//!
//! The tests inside `step.rs` read the writer's output back with a parser that sits beside the
//! writer: same author's idea of what a part 21 file looks like, same shortcuts, same blind spots.
//! A bug that both share is invisible to both. `DAT-003` asked for validation against an
//! independent STEP reader and no third-party reader (OCCT, FreeCAD, stepcode) is installed here,
//! so this file is the next best thing, and it is worth being precise about what it is and is not:
//!
//! - It **is** a second implementation. Different data model (a tokeniser producing a typed
//!   `Arg` tree, not a map of substrings), different statement splitting (it walks the file looking
//!   for `#`, an id and a terminator, rather than cutting on `;` and hoping), and — the part that
//!   actually catches things — a **table of the expected attribute list of every keyword the writer
//!   can emit**. A keyword the writer invents, an argument dropped or added, or a `FACE_BOUND`
//!   written with a surface's arguments fails here and nowhere else. A second table says what each
//!   *referenced* attribute must name — the schema table establishes that an attribute is a
//!   reference, the typing table says to which entity — so an `AXIS2_PLACEMENT_3D` whose `location`
//!   is a `VERTEX_POINT`, where the schema says `CARTESIAN_POINT`, fails here as well.
//! - **It was wrong twice before it was right, and the first version failed for the wrong reason.**
//!   As first written it consumed the opening `.` of an enumeration and left the closing one behind,
//!   so it lost the thread on the second entity of every file — the `SI_UNIT` — stopped reading, and
//!   reported all eleven tests as "the file holds 0 solids". It also had no opinion at all about
//!   what a reference names, which is how it read a file carrying a `VERTEX_POINT` in a placement's
//!   `location` without complaint. Both are corrected here, and both are exactly what a reader that
//!   had never been run against a file a real CAD system wrote gets wrong. That is the argument for
//!   getting one, not against this one.
//! - It also checks the *geometry*, not just the syntax: the shell is walked from the coordinates
//!   that were written, so a reversed loop, a dropped face, a vertex off its own plane or an edge
//!   used twice the same way round all show up as numbers.
//! - It **is not** a conforming STEP implementation. It knows nothing about EXPRESS: no subtypes, no
//!   inherited attributes, no WHERE rules, no `SELECT`s, no units, no product structure semantics.
//!   A file can pass everything here and still be rejected by a real reader. The list of what a
//!   real reader has to be asked is in the `step.rs` module docs, section E.
//!
//! # The tolerances, and where each one comes from
//!
//! - The STEP side writes `f64` coordinates through Rust's shortest round-trip form, which reads
//!   back bit-identically, so the volume check is a *relative* `1e-9` against `Solid::volume`.
//! - The file's declared uncertainty is the kernel's welding tolerance, `diagonal × 1e-9`. Two
//!   geometric checks need a little more than that, and the extra is derived rather than guessed:
//!   `DIRECTION`s are interned on a `1e-9` grid (deviation 14 in `step.rs`), so a point `R` away
//!   from a placement can sit about `R × 1e-9` off the plane the placement names, and a `VECTOR`
//!   of length `L` can reach its end vertex about `L × 1e-9` short. Both bounds below say so
//!   instead of hiding a fudge factor in a constant.
//! - The ASCII STL writes six decimals (`{:.6}`), so its vertices carry up to `5e-7` of absolute
//!   error each. `stl_rounding_band` turns that into a computed bound rather than a magic number:
//!   the triangulation itself is exact, so the whole band is formatting, and a volume
//!   disagreement larger than the band is a real one.
//!
//! Nothing here has been executed. See the note at the bottom of this file.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_solid::{EdgeRef, Plane, Solid, Vec3, extrude, fillet, mesh_from_solid, step_faceted_brep, step_schema, stl_ascii, support};

fn must<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e:?}"),
    }
}

/// A 2 x 4 x 6 box, off the origin so that a sign error cannot hide.
fn box_solid() -> Solid {
    must(support::box_at(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0)), "box")
}

/// A washer: two faces carry a hole, so the file has to carry `FACE_BOUND`s.
fn washer() -> Solid {
    let profile = support::rect_with_hole(0.0, 0.0, 10.0, 10.0, 3.0, 3.0, 7.0, 7.0);
    must(extrude(&profile, &Plane::xy(0.0), 4.0), "washer")
}

/// A box with one edge rounded: many faces, loops of many vertices, non-orthogonal adjacency.
///
/// The reference is corrected, not weakened. `EdgeRef` is a point that must lie **on** an edge —
/// `(5, 0, 5)` is the middle of this box's *bottom face*, five units clear of all four of its
/// edges, and `resolve` answers "no edge passes through" for it, correctly: the old reference named
/// a face, not an edge. The edge from `(10, 0, 0)` to `(10, 0, 10)` is the one `(10, 0, 5)` lies
/// on, five units from the two edges that meet it at its ends, so the reference is unambiguous —
/// which matters, because `resolve` refuses an ambiguous one as a guess rather than a choice.
fn filleted_box() -> Solid {
    let s = must(support::box_at(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)), "box");
    must(fillet(&s, &[EdgeRef::new(Vec3::new(10.0, 0.0, 5.0), 1e-6)], 1.0, 6), "fillet")
}

// ---------------------------------------------------------------------------------------------
// The second reader: a tokeniser, a typed argument tree, and a schema table.
// ---------------------------------------------------------------------------------------------

/// One argument of an entity instance.
#[derive(Clone, Debug, PartialEq)]
enum Arg {
    /// `#n`.
    Ref(usize),
    /// `'text'`.
    Text(String),
    /// A part 21 real.
    Number(f64),
    /// `.NAME.` — an enumeration, with the name kept whole and the two dots stripped.
    ///
    /// The name is the point of the variant. `.T.` and `.MILLI.` have the same shape to a parser
    /// that only wants a boolean, and a reader that cannot tell them apart is precisely the reader
    /// this file exists to be harder than — so `.MILLI.` must not be able to pass for `.F.`.
    Enum(String),
    /// `*`, the derived-attribute placeholder.
    Derived,
    /// `$`, an unset optional attribute.
    Unset,
    /// `(a, b, c)`.
    List(Vec<Arg>),
    /// `LENGTH_MEASURE(1.0)`: a typed parameter.
    Typed(String, Vec<Arg>),
    /// A bare identifier, which the writer only emits as a last resort.
    Keyword(String),
}

/// One entity instance. A complex instance keeps its parts as well.
#[derive(Clone, Debug)]
struct Entity {
    id: usize,
    /// The keyword, or `A+B+C` for a complex instance.
    keyword: String,
    /// The arguments of a simple instance; empty for a complex one.
    args: Vec<Arg>,
    /// `(keyword, arguments)` for each part of a complex instance.
    parts: Vec<(String, Vec<Arg>)>,
}

impl Entity {
    fn names(&self, keyword: &str) -> bool {
        self.keyword == keyword || self.parts.iter().any(|(n, _)| n.as_str() == keyword)
    }
}

/// A cursor over the file text. ASCII outside a string literal, which is all part 21 requires and
/// all `step_faceted_brep` writes: it sanitises every name it puts in a string literal.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Reader<'a> {
        Reader { text, at: 0 }
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.text.as_bytes().get(at).copied()
    }

    fn skip_space(&mut self) {
        while self.byte(self.at).is_some_and(|b| b.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn take(&mut self, c: u8) -> bool {
        self.skip_space();
        if self.byte(self.at) == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn identifier(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.at;
        while let Some(c) = self.byte(self.at)
            && (c == b'_' || c.is_ascii_alphanumeric())
        {
            self.at += 1;
        }
        if self.at == start { None } else { Some(self.text[start..self.at].to_string()) }
    }

    fn digits(&mut self) -> usize {
        self.skip_space();
        let start = self.at;
        while self.byte(self.at).is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        self.text[start..self.at].parse::<usize>().unwrap_or(0)
    }

    fn number(&mut self) -> f64 {
        self.skip_space();
        let start = self.at;
        if matches!(self.byte(self.at), Some(b'+') | Some(b'-')) {
            self.at += 1;
        }
        while self.byte(self.at).is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.at += 1;
        }
        if matches!(self.byte(self.at), Some(b'e') | Some(b'E')) {
            self.at += 1;
            if matches!(self.byte(self.at), Some(b'+') | Some(b'-')) {
                self.at += 1;
            }
            while self.byte(self.at).is_some_and(|b| b.is_ascii_digit()) {
                self.at += 1;
            }
        }
        // A literal that will not parse is a failure, not a NaN. A NaN would sail through the
        // schema table as a number and surface two hundred lines later as "the file says NaN",
        // which is a worse answer than the one this gives.
        let literal = self.text.get(start..self.at).unwrap_or("<end of file>").trim();
        match literal.parse::<f64>() {
            Ok(v) => v,
            Err(_) => panic!("byte {start} of the DATA section holds {literal:?}, which is not a part 21 real"),
        }
    }

    fn string(&mut self) -> String {
        // Stepping over the opening quote is the caller's business.
        self.at += 1;
        let mut out = String::new();
        while let Some(c) = self.byte(self.at) {
            self.at += 1;
            if c != b'\'' {
                out.push(c as char);
            } else if self.byte(self.at) == Some(b'\'') {
                // Part 21 doubles a quote inside a string.
                out.push('\'');
                self.at += 1;
            } else {
                break;
            }
        }
        out
    }

    /// Parse arguments up to and including the closing `)`.
    fn arguments(&mut self) -> Vec<Arg> {
        let mut out: Vec<Arg> = Vec::new();
        loop {
            self.skip_space();
            if self.at >= self.text.len() || self.take(b')') {
                break;
            }
            let before = self.at;
            out.push(self.argument());
            self.skip_space();
            if self.take(b',') {
                continue;
            }
            if self.take(b')') {
                break;
            }
            if self.at == before {
                // A character this reader cannot read: step over it rather than spin on it.
                self.at += 1;
            }
            break;
        }
        out
    }

    fn argument(&mut self) -> Arg {
        self.skip_space();
        match self.byte(self.at) {
            None => Arg::Keyword(String::new()),
            Some(b'\'') => Arg::Text(self.string()),
            Some(b'#') => {
                self.at += 1;
                Arg::Ref(self.digits())
            }
            Some(b'(') => {
                self.at += 1;
                Arg::List(self.arguments())
            }
            Some(b'.') => {
                // An enumeration is written `.NAME.` and **both dots belong to the token**. Taking
                // the opening one and leaving the closing one behind is what made this reader lose
                // the thread on `#2` and call every file empty: the stray `.` derails the rest of
                // the statement, and the failure then surfaces a hundred entities away as a
                // missing solid. Nothing here tolerates a grammar it does not expect.
                let start = self.at;
                self.at += 1;
                let name = self.identifier().unwrap_or_default();
                assert!(self.take(b'.'), "byte {start} holds the enumeration `.{name}` with no closing dot, and part 21 writes one as `.NAME.`");
                Arg::Enum(name)
            }
            Some(b'*') => {
                self.at += 1;
                Arg::Derived
            }
            Some(b'$') => {
                self.at += 1;
                Arg::Unset
            }
            Some(c) if c.is_ascii_digit() || c == b'+' || c == b'-' => Arg::Number(self.number()),
            Some(c) if c == b'_' || c.is_ascii_alphabetic() => {
                let name = self.identifier().unwrap_or_default();
                if self.take(b'(') { Arg::Typed(name, self.arguments()) } else { Arg::Keyword(name) }
            }
            Some(c) => {
                self.at += 1;
                Arg::Keyword(char::from(c).to_string())
            }
        }
    }

    fn named(&mut self) -> Option<(String, Vec<Arg>)> {
        let name = self.identifier()?;
        if !self.take(b'(') {
            return None;
        }
        let args = self.arguments();
        Some((name, args))
    }

    /// The body of one statement: a simple instance, or the parts of a complex one.
    fn body(&mut self) -> Option<(String, Vec<Arg>, Vec<(String, Vec<Arg>)>)> {
        if self.take(b'(') {
            let mut parts: Vec<(String, Vec<Arg>)> = Vec::new();
            loop {
                self.skip_space();
                if self.at >= self.text.len() || self.take(b')') {
                    break;
                }
                match self.named() {
                    Some(part) => parts.push(part),
                    None => break,
                }
            }
            let names: Vec<String> = parts.iter().map(|(n, _)| n.clone()).collect();
            return Some((names.join("+"), Vec::new(), parts));
        }
        let (name, args) = self.named()?;
        Some((name, args, Vec::new()))
    }
}

/// Read the DATA section. Statements are found by structure rather than by cutting on `;`, so a
/// `;` inside a string literal cannot cut the file in half.
///
/// Every way the read can fail is a panic that says what it expected and where, and **none of them
/// is a silent `break`**. A reader that gives up quietly and returns what it managed to read turns a
/// one-token defect in the writer into "the file holds 0 solids" eleven tests later, which is what
/// this function did before its enumeration handling was fixed. A truncated read is a truncated
/// read: it is named here, at the byte where it happened.
fn parse_data(text: &str) -> Vec<Entity> {
    let Some(mark) = text.find("DATA;") else { panic!("the file has no DATA section") };
    let mut r = Reader::new(text);
    r.at = mark + "DATA;".len();
    let mut out: Vec<Entity> = Vec::new();
    loop {
        r.skip_space();
        if r.at >= r.text.len() || r.text[r.at..].starts_with("ENDSEC") {
            break;
        }
        assert!(r.take(b'#'), "byte {} of the DATA section starts {}, not `#`", r.at, here(&r));
        let id = r.digits();
        assert!(r.take(b'='), "#{id} is followed by {} rather than `=`", here(&r));
        let Some((keyword, args, parts)) = r.body() else {
            panic!("#{id} = is followed by {}, which is neither a keyword nor a complex instance", here(&r))
        };
        assert!(r.take(b';'), "#{id} = {keyword} is followed by {} rather than `;`", here(&r));
        out.push(Entity { id, keyword, args, parts });
    }
    out
}

/// The next dozen bytes of the file, for a failure message that has to stand in for the file.
fn here(r: &Reader<'_>) -> String {
    r.text.get(r.at..r.at + 12).unwrap_or("<the end of the file>").to_string()
}

/// Every `#n` an entity names, including the ones inside a complex instance.
fn refs_of(entity: &Entity) -> Vec<usize> {
    fn walk(args: &[Arg], out: &mut Vec<usize>) {
        for arg in args {
            match arg {
                Arg::Ref(id) => out.push(*id),
                Arg::List(inner) | Arg::Typed(_, inner) => walk(inner, out),
                Arg::Text(_) | Arg::Number(_) | Arg::Enum(_) | Arg::Derived | Arg::Unset | Arg::Keyword(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&entity.args, &mut out);
    for (_, args) in &entity.parts {
        walk(args, &mut out);
    }
    out
}

/// What one argument of an entity is expected to be.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    Text,
    Num,
    Ref,
    Enum,
    /// `.T.` / `.F.`, and the `$` of an optional SI-unit prefix.
    Flag,
    Derived,
    RefList,
    Num3,
    Measure,
}

/// The attribute list of every keyword this writer can emit, in the order part 21 wants it.
///
/// A keyword that is not in this table is one the writer produced and the table has never seen,
/// and the test fails on it rather than skipping: an unexamined keyword is exactly the thing this
/// file exists to catch. `FACE_SURFACE` is the documented one-word alternative to `ADVANCED_FACE`
/// and is listed so the table survives that swap.
const SCHEMA: &[(&str, &[Want])] = &[
    ("APPLICATION_CONTEXT", &[Want::Text]),
    ("LENGTH_UNIT", &[]),
    ("NAMED_UNIT", &[Want::Derived]),
    ("PLANE_ANGLE_UNIT", &[]),
    ("SOLID_ANGLE_UNIT", &[]),
    ("SI_UNIT", &[Want::Flag, Want::Flag]),
    ("UNCERTAINTY_MEASURE_WITH_UNIT", &[Want::Measure, Want::Ref, Want::Text, Want::Text]),
    ("GEOMETRIC_REPRESENTATION_CONTEXT", &[Want::Num]),
    ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", &[Want::RefList]),
    ("GLOBAL_UNIT_ASSIGNED_CONTEXT", &[Want::RefList]),
    ("REPRESENTATION_CONTEXT", &[Want::Text, Want::Text]),
    ("CARTESIAN_POINT", &[Want::Text, Want::Num3]),
    ("VERTEX_POINT", &[Want::Text, Want::Ref]),
    ("DIRECTION", &[Want::Text, Want::Num3]),
    ("VECTOR", &[Want::Text, Want::Ref, Want::Num]),
    ("LINE", &[Want::Text, Want::Ref, Want::Ref]),
    ("EDGE_CURVE", &[Want::Text, Want::Ref, Want::Ref, Want::Ref, Want::Enum]),
    ("POLY_LOOP", &[Want::Text, Want::RefList]),
    ("FACE_OUTER_BOUND", &[Want::Text, Want::Ref, Want::Enum]),
    ("FACE_BOUND", &[Want::Text, Want::Ref, Want::Enum]),
    ("FACE_SURFACE", &[Want::Text, Want::RefList, Want::Ref, Want::Enum]),
    ("ADVANCED_FACE", &[Want::Text, Want::RefList, Want::Ref, Want::Enum]),
    ("AXIS2_PLACEMENT_3D", &[Want::Text, Want::Ref, Want::Ref, Want::Ref]),
    ("PLANE", &[Want::Text, Want::Ref]),
    ("CLOSED_SHELL", &[Want::Text, Want::RefList]),
    ("FACETED_BREP", &[Want::Text, Want::Ref]),
    ("COMPOSITE_CURVE_SEGMENT", &[Want::Enum, Want::Enum, Want::Ref]),
    ("COMPOSITE_CURVE", &[Want::Text, Want::RefList, Want::Enum]),
    ("GEOMETRIC_CURVE_SET", &[Want::Text, Want::RefList]),
    ("GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION", &[Want::Text, Want::RefList, Want::Ref]),
    ("ADVANCED_BREP_SHAPE_REPRESENTATION", &[Want::Text, Want::RefList, Want::Ref]),
    ("PRODUCT_CONTEXT", &[Want::Text, Want::Ref, Want::Text]),
    ("PRODUCT", &[Want::Text, Want::Text, Want::Text, Want::RefList]),
    ("PRODUCT_DEFINITION_FORMATION", &[Want::Text, Want::Text, Want::Ref]),
    ("PRODUCT_DEFINITION_CONTEXT", &[Want::Text, Want::Ref, Want::Text]),
    ("PRODUCT_DEFINITION", &[Want::Text, Want::Text, Want::Ref, Want::Ref]),
    ("PRODUCT_DEFINITION_SHAPE", &[Want::Text, Want::Text, Want::Ref]),
    ("SHAPE_DEFINITION_REPRESENTATION", &[Want::Ref, Want::Ref]),
];

/// What each *referenced* attribute of an entity must name, for the keywords that hold references.
///
/// [`SCHEMA`] establishes that an attribute is a reference. This table says to which entity, which
/// is the half of the check a graph-walking reader cannot do: `#17=AXIS2_PLACEMENT_3D('',#12,#13,
/// #14)` is a perfectly well-formed graph no matter what `#12` is, and every id in it resolves, and
/// yet the schema says `location` is a `cartesian_point` and a `vertex_point` is a topological
/// vertex rather than a point in space. That is exactly the defect this writer had, and neither
/// parser here noticed it until this table existed.
///
/// A keyword absent from this table is one that carries no references — `CARTESIAN_POINT`,
/// `DIRECTION`, the three unit keywords, `SI_UNIT` — or one this reader has not been given an
/// opinion about, and those are not checked. Unlike [`SCHEMA`] this table is deliberately partial:
/// it is a list of what must be true, not a claim to be complete. `A|B` accepts either, and a
/// complex instance matches if any of its parts does, which is how every unit in the file is
/// written.
const REFERENCED: &[(&str, &[&str])] = &[
    ("VERTEX_POINT", &["CARTESIAN_POINT"]),
    ("VECTOR", &["DIRECTION"]),
    ("LINE", &["CARTESIAN_POINT", "VECTOR"]),
    ("EDGE_CURVE", &["VERTEX_POINT", "VERTEX_POINT", "LINE", "enum"]),
    ("POLY_LOOP", &["list:VERTEX_POINT"]),
    ("FACE_OUTER_BOUND", &["POLY_LOOP", "enum"]),
    ("FACE_BOUND", &["POLY_LOOP", "enum"]),
    ("ADVANCED_FACE", &["list:FACE_OUTER_BOUND|FACE_BOUND", "PLANE", "enum"]),
    ("FACE_SURFACE", &["list:FACE_OUTER_BOUND|FACE_BOUND", "PLANE", "enum"]),
    // The one this table exists for.
    ("AXIS2_PLACEMENT_3D", &["CARTESIAN_POINT", "DIRECTION", "DIRECTION"]),
    ("PLANE", &["AXIS2_PLACEMENT_3D"]),
    ("CLOSED_SHELL", &["list:ADVANCED_FACE|FACE_SURFACE"]),
    ("FACETED_BREP", &["CLOSED_SHELL"]),
    ("COMPOSITE_CURVE_SEGMENT", &["enum", "enum", "EDGE_CURVE|LINE"]),
    ("COMPOSITE_CURVE", &["list:COMPOSITE_CURVE|COMPOSITE_CURVE_SEGMENT", "enum"]),
    ("GEOMETRIC_CURVE_SET", &["list:COMPOSITE_CURVE|EDGE_CURVE|LINE"]),
    ("UNCERTAINTY_MEASURE_WITH_UNIT", &["any", "LENGTH_UNIT|PLANE_ANGLE_UNIT|SOLID_ANGLE_UNIT", "any", "any"]),
    ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", &["list:UNCERTAINTY_MEASURE_WITH_UNIT"]),
    ("GLOBAL_UNIT_ASSIGNED_CONTEXT", &["list:LENGTH_UNIT|PLANE_ANGLE_UNIT|SOLID_ANGLE_UNIT"]),
    ("PRODUCT_CONTEXT", &["APPLICATION_CONTEXT", "any"]),
    ("PRODUCT", &["any", "any", "any", "list:PRODUCT_CONTEXT"]),
    ("PRODUCT_DEFINITION_FORMATION", &["PRODUCT"]),
    ("PRODUCT_DEFINITION_CONTEXT", &["APPLICATION_CONTEXT", "any"]),
    ("PRODUCT_DEFINITION", &["any", "any", "PRODUCT_DEFINITION_FORMATION", "PRODUCT_DEFINITION_CONTEXT"]),
    ("PRODUCT_DEFINITION_SHAPE", &["any", "any", "PRODUCT_DEFINITION"]),
    (
        "SHAPE_DEFINITION_REPRESENTATION",
        &["PRODUCT_DEFINITION_SHAPE", "ADVANCED_BREP_SHAPE_REPRESENTATION|GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION"],
    ),
];

/// Every keyword `#id` answers to: one for a simple instance, one per part for a complex one.
fn keywords_of<'a>(entities: &'a [Entity], id: usize, what: &str) -> Vec<&'a str> {
    match entities.iter().find(|e| e.id == id) {
        Some(e) if e.parts.is_empty() => vec![e.keyword.as_str()],
        Some(e) => e.parts.iter().map(|(n, _)| n.as_str()).collect(),
        None => panic!("{what} names #{id}, which no entity defines"),
    }
}

/// Does any of the keywords an entity answers to satisfy `want`, which may be `A|B`?
fn accepts(want: &str, got: &[&str]) -> bool {
    want.split('|').any(|w| got.contains(&w))
}

/// Check one instance's references against [`REFERENCED`]. A keyword with no row is not checked.
fn check_typed(id: usize, keyword: &str, args: &[Arg], entities: &[Entity]) {
    let Some((_, wants)) = REFERENCED.iter().find(|(name, _)| *name == keyword) else {
        return;
    };
    for (i, want) in wants.iter().enumerate() {
        let Some(arg) = args.get(i) else {
            panic!("#{id} = {keyword} has {} attributes and the typing table wants one at {i}", args.len());
        };
        let what = format!("#{id} = {keyword}, attribute {i}");
        if let Some(item) = want.strip_prefix("list:") {
            let Arg::List(list) = arg else {
                panic!("{what} is {arg:?}, and a reader would want a list of {item} there");
            };
            for (n, entry) in list.iter().enumerate() {
                let Arg::Ref(target) = entry else {
                    panic!("{what}, item {n} of the list is {entry:?}, and a reader would want a reference there");
                };
                let got = keywords_of(entities, *target, &format!("{what}, item {n}"));
                assert!(accepts(item, &got), "{what}, item {n} names #{target}, which is a {got:?}, and a reader wants {item}");
            }
        } else if *want == "enum" {
            assert!(matches!(arg, Arg::Enum(_)), "{what} is {arg:?}, and a reader would want an enumeration there");
        } else if *want != "any" {
            let Arg::Ref(target) = arg else {
                panic!("{what} is {arg:?}, and a reader would want a reference to {want} there");
            };
            let got = keywords_of(entities, *target, &what);
            assert!(accepts(want, &got), "{what} names #{target}, which is a {got:?}, and a reader wants {want}");
        }
    }
}

/// Every reference in the file, against the table above, including each part of a complex instance.
fn check_references(entities: &[Entity]) {
    for e in entities {
        if e.parts.is_empty() {
            check_typed(e.id, &e.keyword, &e.args, entities);
        } else {
            for (name, args) in &e.parts {
                check_typed(e.id, name, args, entities);
            }
        }
    }
}

fn want_matches(want: Want, arg: &Arg) -> bool {
    match (want, arg) {
        (Want::Text, Arg::Text(_)) => true,
        (Want::Num, Arg::Number(_)) => true,
        (Want::Ref, Arg::Ref(_)) => true,
        (Want::Enum, Arg::Enum(_)) | (Want::Flag, Arg::Enum(_)) | (Want::Flag, Arg::Unset) => true,
        (Want::Derived, Arg::Derived) => true,
        (Want::RefList, Arg::List(v)) => !v.is_empty() && v.iter().all(|a| matches!(a, Arg::Ref(_))),
        (Want::Num3, Arg::List(v)) => v.len() == 3 && v.iter().all(|a| matches!(a, Arg::Number(_))),
        (Want::Measure, Arg::Typed(name, v)) => name == "LENGTH_MEASURE" && v.len() == 1 && matches!(v.first(), Some(Arg::Number(_))),
        _ => false,
    }
}

fn check_one(keyword: &str, args: &[Arg], id: usize) {
    let Some((_, wants)) = SCHEMA.iter().find(|(name, _)| *name == keyword) else {
        panic!("#{id} is a {keyword}, which this file is not supposed to write and this table has never seen");
    };
    assert_eq!(args.len(), wants.len(), "#{id} = {keyword} has {} attributes, its schema gives {}", args.len(), wants.len());
    for (i, (want, arg)) in wants.iter().zip(args.iter()).enumerate() {
        assert!(want_matches(*want, arg), "#{id} = {keyword}, attribute {i} is {arg:?}, its schema wants {want:?}");
    }
}

/// Every entity in the file against both tables above, each part of a complex instance on its own.
///
/// `check_schema` says an entity has the attributes its EXPRESS entity declares;
/// `check_references` says those attributes name the entities the schema says they name. A file can
/// pass the first and fail the second, and the defect that motivated it did.
fn check_entities(entities: &[Entity]) {
    check_schema(entities);
    check_references(entities);
}

// ---------------------------------------------------------------------------------------------
// Reading the parsed file.
// ---------------------------------------------------------------------------------------------

/// A parsed part 21 file, with the typed accessors the checks below need.
struct Part21 {
    text: String,
    entities: Vec<Entity>,
}

impl Part21 {
    fn read(text: &str) -> Part21 {
        Part21 { text: text.to_string(), entities: parse_data(text) }
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn get(&self, id: usize) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    fn need(&self, id: usize) -> &Entity {
        match self.get(id) {
            Some(e) => e,
            None => panic!("#{id} is named by something, and the file does not define it"),
        }
    }

    /// Every entity of one keyword, in file order. A complex instance matches on any of its parts.
    fn of(&self, keyword: &str) -> Vec<&Entity> {
        self.entities.iter().filter(|e| e.names(keyword)).collect()
    }

    fn keyword(&self, id: usize) -> String {
        self.need(id).keyword.clone()
    }

    /// The argument list of one entity, whether it is simple or a part of a complex instance.
    fn args_of(&self, id: usize, keyword: &str) -> &[Arg] {
        let e = self.need(id);
        if let Some((_, args)) = e.parts.iter().find(|(name, _)| name.as_str() == keyword) {
            return args.as_slice();
        }
        if e.keyword == keyword {
            return e.args.as_slice();
        }
        panic!("#{id} is a {}, and was asked for as a {keyword}", e.keyword);
    }

    fn arg<'a>(&self, args: &'a [Arg], i: usize) -> &'a Arg {
        match args.get(i) {
            Some(a) => a,
            None => panic!("attribute {i} is missing from a {}-attribute entity", args.len()),
        }
    }

    fn as_ref(&self, arg: &Arg, what: &str) -> usize {
        match arg {
            Arg::Ref(id) => *id,
            other => panic!("{what} is {other:?}, and a reader would want a reference there"),
        }
    }

    fn as_list<'a>(&self, arg: &'a Arg, what: &str) -> &'a [Arg] {
        match arg {
            Arg::List(v) => v.as_slice(),
            other => panic!("{what} is {other:?}, and a reader would want a list there"),
        }
    }

    fn as_number(&self, arg: &Arg, what: &str) -> f64 {
        match arg {
            Arg::Number(n) => *n,
            other => panic!("{what} is {other:?}, and a reader would want a number there"),
        }
    }

    fn triple(&self, list: &[Arg], what: &str) -> Vec3 {
        assert_eq!(list.len(), 3, "{what} has {} coordinates, not three", list.len());
        let x = self.as_number(self.arg(list, 0), what);
        let y = self.as_number(self.arg(list, 1), what);
        let z = self.as_number(self.arg(list, 2), what);
        Vec3::new(x, y, z)
    }

    fn point_of(&self, id: usize) -> Vec3 {
        let args = self.args_of(id, "CARTESIAN_POINT");
        self.triple(self.as_list(self.arg(args, 1), "a CARTESIAN_POINT's coordinates"), "a coordinate")
    }

    fn direction_of(&self, id: usize) -> Vec3 {
        let args = self.args_of(id, "DIRECTION");
        self.triple(self.as_list(self.arg(args, 1), "a DIRECTION's ratios"), "a ratio")
    }

    fn vertex_point(&self, id: usize) -> Vec3 {
        let args = self.args_of(id, "VERTEX_POINT");
        let point = self.as_ref(self.arg(args, 1), "a VERTEX_POINT's point");
        self.point_of(point)
    }

    /// The `SI_UNIT` behind the file's length unit: the one every coordinate is read through.
    fn length_unit(&self) -> usize {
        match self.entities.iter().find(|e| e.names("LENGTH_UNIT")) {
            Some(e) => e.id,
            None => panic!("the file declares no LENGTH_UNIT, so it never says what its coordinates are measured in"),
        }
    }

    /// The distance within which the file itself says two points are the same point.
    fn uncertainty(&self) -> f64 {
        let unc = self.of("UNCERTAINTY_MEASURE_WITH_UNIT");
        let e = match unc.first() {
            Some(e) => *e,
            None => panic!("the file declares no uncertainty, so it never says what 'the same point' means"),
        };
        let args = self.args_of(e.id, "UNCERTAINTY_MEASURE_WITH_UNIT");
        // A tolerance quoted in the wrong unit is not a tolerance at all.
        let unit = self.as_ref(self.arg(args, 1), "an uncertainty's unit");
        assert_eq!(unit, self.length_unit(), "the uncertainty is measured in #{unit}, not in the file's length unit");
        match self.arg(args, 0) {
            Arg::Typed(name, inner) => {
                assert_eq!(name, "LENGTH_MEASURE", "the uncertainty is a {name:?}, not a length");
                self.as_number(self.arg(inner, 0), "the declared uncertainty")
            }
            other => panic!("the uncertainty is {other:?}"),
        }
    }
}

/// The name inside an enumeration token: `.T.` is `T`, and the two dots are part of the token
/// rather than part of the name.
///
/// Compared by name rather than by `matches!`, so that a failure says what the file actually wrote
/// — which is the whole reason [`Arg::Enum`] keeps the name.
fn enum_name<'a>(arg: &'a Arg, what: &str) -> &'a str {
    match arg {
        Arg::Enum(name) => name.as_str(),
        other => panic!("{what} is {other:?}, and a reader would want an enumeration there"),
    }
}

// ---------------------------------------------------------------------------------------------
// The geometry, read back out of the file.
// ---------------------------------------------------------------------------------------------

fn key_of(a: usize, b: usize) -> (usize, usize) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Signed volume of one loop, as the tetrahedra from `origin`.
fn loop_volume(points: &[Vec3], origin: Vec3) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 1..n.saturating_sub(1) {
        let (Some(first), Some(b), Some(c)) = (points.first(), points.get(i), points.get(i + 1)) else { continue };
        total += (*first - origin).dot((*b - origin).cross(*c - origin)) / 6.0;
    }
    total
}

/// Signed volume of a closed triangle mesh, by the same rule.
fn triangle_volume(tris: &[[Vec3; 3]], origin: Vec3) -> f64 {
    tris.iter().map(|t| (t[0] - origin).dot((t[1] - origin).cross(t[2] - origin)) / 6.0).sum()
}

/// Does a loop wind counter-clockwise about `normal`, seen from outside?
fn winds_with(points: &[Vec3], normal: Vec3) -> bool {
    let n = points.len();
    let Some(anchor) = points.first().copied() else { return false };
    let mut acc = Vec3::ZERO;
    for i in 0..n {
        let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % n)) else { continue };
        acc = acc + (*a - anchor).cross(*b - anchor);
    }
    acc.dot(normal) > 0.0
}

/// What the file said, once every check in [`audit`] has passed.
#[derive(Clone, Copy, Debug)]
struct Audit {
    faces: usize,
    loops: usize,
    vertices: usize,
    edges: usize,
    volume: f64,
}

/// Read a file back and check *everything* this file knows how to check.
///
/// One implementation, called by every test below, so a test that is nominally about the volume is
/// not a test that quietly stopped checking the orientations.
fn audit(solid: &Solid, text: &str) -> Audit {
    let file = Part21::read(text);

    // -- the file is a part 21 file ----------------------------------------------------------
    assert!(file.text().starts_with("ISO-10303-21;"), "part 21 opens with its own marker");
    assert!(file.text().trim_end().ends_with("END-ISO-10303-21;"), "and closes with it");
    assert_eq!(file.text().matches("ENDSEC;").count(), 2, "one section closes after the header, one after the data");

    // -- every entity against the schema and reference tables -------------------------------------
    check_entities(&file.entities);

    // -- ids are 1..n, each defined once ------------------------------------------------------
    let ids: Vec<usize> = file.entities.iter().map(|e| e.id).collect();
    let unique: BTreeSet<usize> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "an id is defined twice");
    let mut sorted_ids = ids.clone();
    sorted_ids.sort_unstable();
    for (i, id) in sorted_ids.iter().enumerate() {
        assert_eq!(*id, i + 1, "the ids are not 1..n: #{id} sits where #{} should be", i + 1);
    }

    // -- every reference resolves, and points backwards ----------------------------------------
    for e in &file.entities {
        for r in refs_of(e) {
            assert!(file.get(r).is_some(), "#{} names #{}, which no entity defines", e.id, r);
            assert!(r < e.id, "#{} names #{}, which is written after it: references run backwards", e.id, r);
        }
    }

    // -- exactly one solid, one shell ---------------------------------------------------------
    let breps = file.of("FACETED_BREP");
    assert_eq!(breps.len(), 1, "the file holds {} solids, and a reader expects one", breps.len());
    let shells = file.of("CLOSED_SHELL");
    assert_eq!(shells.len(), 1, "the file holds {} closed shells", shells.len());
    let brep = breps[0];
    let brep_args = file.args_of(brep.id, "FACETED_BREP");
    let shell = file.as_ref(file.arg(brep_args, 1), "a FACETED_BREP's outer shell");
    assert_eq!(shell, shells[0].id, "the solid's shell is the only shell in the file");
    let shell_args = file.args_of(shell, "CLOSED_SHELL");
    let face_ids: Vec<usize> =
        file.as_list(file.arg(shell_args, 1), "a CLOSED_SHELL's faces").iter().map(|a| file.as_ref(a, "a face of the shell")).collect();
    assert_eq!(
        face_ids.len(),
        solid.faces.len(),
        "the CLOSED_SHELL holds {} faces, the solid it came from has {}",
        face_ids.len(),
        solid.faces.len()
    );

    // -- the unit every coordinate in the file is read through ----------------------------------
    // An enumeration a reader cannot tell from `.F.` is an enumeration it cannot check, and this is
    // the one the first version of this parser could not see: `.MILLI.` and `.F.` both parsed as
    // "false". The unit is the single thing in the file that every measurement passes through.
    let si = file.args_of(file.length_unit(), "SI_UNIT");
    assert_eq!(enum_name(file.arg(si, 0), "the length unit's prefix"), "MILLI", "the kernel works in millimetres and the file has to say so");
    assert_eq!(enum_name(file.arg(si, 1), "the length unit's name"), "METRE", "a milli-prefixed unit has to be a metre");

    // -- faces, bounds, loops and vertices ---------------------------------------------------
    let uncertainty = file.uncertainty();
    assert!(uncertainty > 0.0, "the file declares an uncertainty of {uncertainty}, which is no distance at all");
    // `DIRECTION`s are interned on a 1e-9 grid, so an axis can be that far from unit-normal and a
    // point `R` from the placement can sit about `R * 1e-9` off the plane the placement names.
    let direction_slop = 1e-8;
    let origin = solid.bounds().min;
    let mut volume = 0.0;
    let mut loops = 0usize;
    let mut segments: Vec<(usize, usize, usize)> = Vec::new();
    for (fi, face_id) in face_ids.iter().enumerate() {
        let face_id = *face_id;
        let Some(kernel_face) = solid.faces.get(fi) else { continue };
        let face_args = file.args_of(face_id, "ADVANCED_FACE");
        assert_eq!(enum_name(file.arg(face_args, 3), "a face's same_sense"), "T", "#{face_id} is not written with same_sense true");

        let surface = file.as_ref(file.arg(face_args, 2), "a face's surface");
        let plane_args = file.args_of(surface, "PLANE");
        let placement = file.as_ref(file.arg(plane_args, 1), "a PLANE's placement");
        let place_args = file.args_of(placement, "AXIS2_PLACEMENT_3D");
        let place_point = file.point_of(file.as_ref(file.arg(place_args, 1), "a placement's point"));
        let axis = file.direction_of(file.as_ref(file.arg(place_args, 2), "a placement's axis"));
        let ref_dir = file.direction_of(file.as_ref(file.arg(place_args, 3), "a placement's reference direction"));
        assert!((axis.len() - 1.0).abs() <= direction_slop, "face {fi}'s axis is {axis:?}, which is not a unit vector");
        assert!((ref_dir.len() - 1.0).abs() <= direction_slop, "face {fi}'s reference direction is {ref_dir:?}, not a unit vector");
        assert!(axis.dot(ref_dir).abs() <= direction_slop, "face {fi}'s placement is not a right-handed frame");

        let bounds: Vec<usize> =
            file.as_list(file.arg(face_args, 1), "a face's bounds").iter().map(|a| file.as_ref(a, "a bound of a face")).collect();
        let kernel_loops: Vec<usize> = kernel_face.region.loops().map(|l| l.len()).collect();
        assert_eq!(
            bounds.len(),
            kernel_loops.len(),
            "face {fi} was written with {} bounds, the kernel wound {} loops",
            bounds.len(),
            kernel_loops.len()
        );
        for (bi, bound) in bounds.iter().enumerate() {
            let bound = *bound;
            let expected = if bi == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
            assert_eq!(
                file.keyword(bound),
                expected,
                "face {fi}'s bound {bi} is a {}, and the first bound is an outer one while the rest are holes",
                file.keyword(bound)
            );
            let bound_args = file.args_of(bound, expected);
            assert_eq!(enum_name(file.arg(bound_args, 2), "a bound's orientation"), "T", "#{bound} is not written with orientation true");

            let poly = file.as_ref(file.arg(bound_args, 1), "a bound's loop");
            let poly_args = file.args_of(poly, "POLY_LOOP");
            let verts: Vec<usize> =
                file.as_list(file.arg(poly_args, 1), "a POLY_LOOP's polygon").iter().map(|a| file.as_ref(a, "a point of a polygon")).collect();
            let expected_len = kernel_loops.get(bi).copied().unwrap_or(0);
            assert_eq!(verts.len(), expected_len, "#{poly} has {} vertices, the kernel's loop {bi} of face {fi} has {expected_len}", verts.len());
            assert!(verts.len() >= 3, "#{poly} has {} vertices, and a polygon needs three", verts.len());

            let points: Vec<Vec3> = verts.iter().map(|v| file.vertex_point(*v)).collect();
            for (i, a) in points.iter().enumerate() {
                for b in points.iter().skip(i + 1) {
                    assert!(a.dist(*b) > uncertainty, "#{poly} names a vertex twice, {} apart", a.dist(*b));
                }
            }
            for p in &points {
                let reach = p.dist(place_point);
                let off = ((*p - place_point).dot(axis)).abs();
                assert!(off <= uncertainty + reach * 1e-9, "face {fi} has a loop vertex {off} off its own plane, against a declared {uncertainty}");
            }
            assert_eq!(winds_with(&points, axis), bi == 0, "face {fi}'s bound {bi} winds against its own normal {axis:?}");
            volume += loop_volume(&points, origin);
            loops += 1;
            for i in 0..points.len() {
                let a = verts.get(i).copied().unwrap_or(0);
                let b = verts.get((i + 1) % points.len()).copied().unwrap_or(0);
                segments.push((face_id, a, b));
            }
        }
    }
    let kernel_loop_count: usize = solid.faces.iter().map(|f| f.region.loops().count()).sum();
    assert_eq!(loops, kernel_loop_count, "the file holds {loops} loops, the solid has {kernel_loop_count}");

    // -- every boundary edge is a fold, used once in each direction ---------------------------
    let mut uses: BTreeMap<(usize, usize), Vec<(usize, bool)>> = BTreeMap::new();
    for (face_id, a, b) in &segments {
        uses.entry(key_of(*a, *b)).or_default().push((*face_id, a < b));
    }
    for ((a, b), list) in &uses {
        assert_eq!(list.len(), 2, "the edge #{a}..#{b} is used by {} faces, and a shell uses each twice", list.len());
        assert_ne!(list[0].1, list[1].1, "the two faces at #{a}..#{b} run along it the same way");
        assert_ne!(list[0].0, list[1].0, "one face at #{a}..#{b} uses that edge twice");
    }

    // -- one edge curve per boundary edge, each a line through its two vertices ----------------
    let curves = file.of("EDGE_CURVE");
    assert_eq!(curves.len(), uses.len(), "the file holds {} edge curves for {} boundary edges", curves.len(), uses.len());
    let mut curve_keys: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for c in &curves {
        let args = file.args_of(c.id, "EDGE_CURVE");
        let v0 = file.as_ref(file.arg(args, 1), "an EDGE_CURVE's start vertex");
        let v1 = file.as_ref(file.arg(args, 2), "an EDGE_CURVE's end vertex");
        let key = key_of(v0, v1);
        assert!(uses.contains_key(&key), "the edge curve #{} joins #{v0} and #{v1}, which no face loop names", c.id);
        assert!(curve_keys.insert(key, c.id).is_none(), "two edge curves join the same pair of vertices");
        assert_eq!(enum_name(file.arg(args, 4), "an edge curve's same_sense"), "T", "#{} is not written with same_sense true", c.id);

        let line = file.as_ref(file.arg(args, 3), "an EDGE_CURVE's curve");
        let line_args = file.args_of(line, "LINE");
        let start_point = file.point_of(file.as_ref(file.arg(line_args, 1), "a LINE's point"));
        let vector = file.as_ref(file.arg(line_args, 2), "a LINE's vector");
        let vector_args = file.args_of(vector, "VECTOR");
        let direction = file.direction_of(file.as_ref(file.arg(vector_args, 1), "a VECTOR's direction"));
        let magnitude = file.as_number(file.arg(vector_args, 2), "a VECTOR's magnitude");
        let (start, end) = (file.vertex_point(v0), file.vertex_point(v1));
        assert!((direction.len() - 1.0).abs() <= direction_slop, "#{vector} names a direction of length {}", direction.len());
        assert!(magnitude > 0.0, "#{vector} has magnitude {magnitude}, so #{line} is not a line");
        let reach = start_point.dist(start);
        assert!(reach <= uncertainty, "#{line} starts {reach} from its own start vertex");
        let span = start.dist(end);
        let overshoot = (start_point + direction * magnitude).dist(end);
        assert!(overshoot <= uncertainty + span * 1e-9, "#{line} ends {overshoot} from its own end vertex");
        let error = (magnitude - span).abs();
        assert!(error <= 1e-9 * span.max(1.0), "#{vector} is {magnitude} long and its own edge is {span}");
    }

    // -- and every edge curve is carried exactly once by the wireframe half ---------------------
    let carried: Vec<usize> = file
        .of("COMPOSITE_CURVE_SEGMENT")
        .iter()
        .map(|s| {
            let args = file.args_of(s.id, "COMPOSITE_CURVE_SEGMENT");
            file.as_ref(file.arg(args, 2), "a composite segment's parent curve")
        })
        .collect();
    let unique_carried: BTreeSet<usize> = carried.iter().copied().collect();
    assert_eq!(unique_carried.len(), carried.len(), "one edge curve is carried by two composite segments");
    let mut curve_ids: Vec<usize> = curves.iter().map(|c| c.id).collect();
    curve_ids.sort_unstable();
    let mut sorted_carried = carried;
    sorted_carried.sort_unstable();
    assert_eq!(sorted_carried, curve_ids, "the faces and the edge wireframe disagree about which edges exist");
    for c in &file.of("COMPOSITE_CURVE") {
        let args = file.args_of(c.id, "COMPOSITE_CURVE");
        let segs = file.as_list(file.arg(args, 1), "a COMPOSITE_CURVE's segments");
        assert_eq!(segs.len(), 1, "#{} carries {} segments, and there is one per edge", c.id, segs.len());
        assert_eq!(enum_name(file.arg(args, 2), "a composite curve's self_intersect"), "F", "#{} is written as self-intersecting", c.id);
    }

    // -- one vertex per place -----------------------------------------------------------------
    let vertex_entities = file.of("VERTEX_POINT");
    let coordinates: Vec<Vec3> = vertex_entities.iter().map(|v| file.vertex_point(v.id)).collect();
    for (i, a) in coordinates.iter().enumerate() {
        for b in coordinates.iter().skip(i + 1) {
            assert!(a.dist(*b) > uncertainty, "two vertices sit {} apart, and a reader rejects that as coincident", a.dist(*b));
        }
    }
    let declared: BTreeSet<usize> = vertex_entities.iter().map(|v| v.id).collect();
    let used: BTreeSet<usize> = segments.iter().flat_map(|(_, a, b)| [*a, *b]).collect();
    assert_eq!(used, declared, "the vertices the faces name and the vertices the file declares are not the same set");

    // -- the volume, measured from the numbers that were written ------------------------------
    let exact = solid.volume();
    assert!(exact.is_finite() && exact != 0.0, "the solid's own volume is {exact}, so there is nothing to compare against");
    let band = 1e-9 * exact.abs().max(1.0);
    assert!((volume - exact).abs() <= band, "the file says {volume}, the solid says {exact}, and the tolerance is {band}");

    Audit { faces: face_ids.len(), loops, vertices: vertex_entities.len(), edges: uses.len(), volume }
}

// ---------------------------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_file_carries_the_sections_and_the_schema_a_reader_compares_against() {
    let solid = box_solid();
    let text = must(step_faceted_brep(&solid, "box"), "export");
    let file = Part21::read(&text);
    assert!(file.text().contains("FILE_DESCRIPTION("), "the header says what the file holds");
    assert!(file.text().contains("FILE_NAME("), "the header names the file");
    assert!(file.text().contains(&format!("FILE_SCHEMA(('{}'))", step_schema())), "the schema a reader checks is the one we promise");
    // Everything below is the schema table's own run over the box.
    assert_eq!(audit(&solid, &text).faces, 6, "a box has six faces");
}

#[test]
fn every_entity_matches_the_attribute_list_its_schema_names() {
    for (name, solid) in [("box", box_solid()), ("washer", washer()), ("rounded", filleted_box())] {
        let text = must(step_faceted_brep(&solid, name), "export");
        let file = Part21::read(&text);
        check_schema(&file.entities);
        assert!(file.entities.len() > 40, "{name} holds {} entities, which is too few to be a shape", file.entities.len());
        let _ = audit(&solid, &text);
    }
}

#[test]
fn the_closed_shell_holds_exactly_the_faces_the_solid_has() {
    for (name, solid) in [("box", box_solid()), ("washer", washer()), ("rounded", filleted_box())] {
        let text = must(step_faceted_brep(&solid, name), "export");
        let a = audit(&solid, &text);
        assert_eq!(a.faces, solid.faces.len(), "{name}");
        // Euler's count over the whole file: V - E + F of a closed shell is at least a sphere's 2,
        // and the three numbers come from three different parts of the file.
        let euler = a.vertices as i64 - a.edges as i64 + a.faces as i64;
        assert!(euler >= 2, "{name}: V {} - E {} + F {} = {euler}, which is below a sphere's", a.vertices, a.edges, a.faces);
    }
}

#[test]
fn a_face_with_a_hole_writes_the_hole_as_an_inner_bound() {
    let solid = washer();
    let text = must(step_faceted_brep(&solid, "washer"), "export");
    let a = audit(&solid, &text);
    let file = Part21::read(&text);
    assert_eq!(file.of("FACE_OUTER_BOUND").len(), solid.faces.len(), "every face has an outer bound");
    assert_eq!(file.of("FACE_BOUND").len(), 2, "the washer's two holed faces each write one inner bound");
    // A square with a square hole, extruded: two caps of two loops, four outer walls, four walls
    // around the hole. That is ten faces and twelve loops, and the file has to say the same.
    assert_eq!(a.faces, 10, "the washer has ten faces");
    assert_eq!(a.loops, 12, "the washer has twelve loops");
    assert!((a.volume - 336.0).abs() <= 1e-9, "10 * 10 less 4 * 4 across, 4 thick: 336, the file says {}", a.volume);
}

#[test]
fn a_face_winds_the_way_its_own_normal_says() {
    // The orientation convention surviving into the file rather than being re-derived by it. A
    // solid that came out inside-out is written faithfully and uselessly, which is what the next
    // test measures, and `Solid::validate` is the gate that keeps such a solid from being exported
    // through a command that runs it first.
    let solid = filleted_box();
    solid.validate().unwrap_or_else(|e| panic!("the rounded box is not valid, so there is nothing to check: {e}"));
    let text = must(step_faceted_brep(&solid, "rounded"), "export");
    let a = audit(&solid, &text);
    assert!(a.volume > 0.0, "a file that reads as positive volume from its own coordinates is outward-facing");
}

#[test]
fn an_inside_out_shell_is_written_inside_out() {
    let solid = box_solid();
    let inside_out = must(solid.flipped(), "flip");
    let text = must(step_faceted_brep(&inside_out, "inside out"), "export");
    let a = audit(&inside_out, &text);
    assert!((a.volume + solid.volume()).abs() <= 1e-9, "the file says {}, the upright solid says {}", a.volume, solid.volume());
    assert!(a.volume < 0.0, "an inside-out shell has a negative volume and this one does not");
    assert!(matches!(solid.validate(), Ok(())), "and the upright one is still a solid");
}

#[test]
fn the_same_solid_writes_the_same_bytes_and_the_same_graph() {
    let solid = washer();
    let a = must(step_faceted_brep(&solid, "washer"), "first export");
    let b = must(step_faceted_brep(&solid, "washer"), "second export");
    assert_eq!(a, b, "two exports of one solid differ: no clock, no hash order, no address in the output");
    assert_eq!(audit(&solid, &a).volume, audit(&solid, &b).volume);
}

#[test]
fn a_name_that_would_break_the_file_cannot() {
    // Part 21 has no escape for a quote, so a hostile product name would end the literal and turn
    // every statement after it into rubbish. The second parser is the check: were the file broken,
    // `audit` would never reach the volume.
    let solid = box_solid();
    let text = must(step_faceted_brep(&solid, "it's a \"box\"; DROP TABLE #7 = ('; garbage"), "export");
    assert!(!text.contains("= (';"), "a name must not be able to end the string literal it sits in");
    assert_eq!(audit(&solid, &text).faces, 6, "the file survives a hostile name");
    let empty = must(step_faceted_brep(&solid, ""), "export");
    assert!(empty.contains("'cadcraft'"), "an empty name still names the product");
    audit(&solid, &empty);
}

// ---------------------------------------------------------------------------------------------
// The cross-check: the same solid through two formats, two readers, one number.
// ---------------------------------------------------------------------------------------------

/// A third reader, this one for ASCII STL, with nothing in common with either the STEP writer or
/// `mesh.rs`.
fn parse_ascii_stl(text: &str) -> Vec<[Vec3; 3]> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let number = |at: usize| -> f64 { tokens.get(at).and_then(|t| t.parse::<f64>().ok()).unwrap_or(f64::NAN) };
    let mut out: Vec<[Vec3; 3]> = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if tokens[i] != "facet" {
            i += 1;
            continue;
        }
        assert_eq!(tokens.get(i + 1).copied(), Some("normal"), "a facet has to name a normal");
        let normal = [number(i + 2), number(i + 3), number(i + 4)];
        assert!(normal.iter().all(|n| n.is_finite()), "a facet normal is not three numbers: {normal:?}");
        i += 5;
        assert_eq!(tokens.get(i).copied(), Some("outer"), "a facet has an outer loop");
        assert_eq!(tokens.get(i + 1).copied(), Some("loop"));
        i += 2;
        let mut points: Vec<Vec3> = Vec::new();
        while tokens.get(i).copied() == Some("vertex") {
            points.push(Vec3::new(number(i + 1), number(i + 2), number(i + 3)));
            i += 4;
        }
        assert_eq!(points.len(), 3, "a facet has three vertices, got {}", points.len());
        assert_eq!(tokens.get(i).copied(), Some("endloop"));
        assert_eq!(tokens.get(i + 1).copied(), Some("endfacet"));
        i += 2;
        out.push([points[0], points[1], points[2]]);
    }
    out
}

/// How far the ASCII STL's six decimals can move a volume, computed rather than guessed.
///
/// `stl_ascii` writes `{:.6}`, so every coordinate is off by at most `5e-7`. Each tetrahedron term
/// `a · (b × c)` moves by at most `3 δ R²` to first order and `3 δ² R` to second, and a triangle
/// volume divides that by six.
fn stl_rounding_band(tris: &[[Vec3; 3]], origin: Vec3) -> f64 {
    let d = 5e-7;
    let r = tris.iter().flat_map(|t| t.iter()).map(|p| p.dist(origin)).fold(0.0f64, f64::max);
    tris.len() as f64 * ((3.0 * d * r * r) / 6.0 + 3.0 * d * d * r)
}

#[test]
fn step_and_ascii_stl_agree_about_the_same_solid() {
    // The cheapest cross-check that needs no third party: two formats, two independent readers,
    // one number. The tessellation is exact for the faces it is given, so the whole disagreement
    // budget is the ASCII STL's six decimals, and anything outside that band is a real defect in
    // one of the two writers.
    let solid = washer();
    let step_text = must(step_faceted_brep(&solid, "washer"), "step export");
    let from_step = audit(&solid, &step_text).volume;

    let mesh = must(mesh_from_solid(&solid), "mesh");
    let tris = parse_ascii_stl(&stl_ascii(&mesh));
    assert_eq!(tris.len(), mesh.tris.len(), "the STL holds {} facets, the mesh has {} triangles", tris.len(), mesh.tris.len());

    let origin = solid.bounds().min;
    let from_stl = triangle_volume(&tris, origin);
    let band = stl_rounding_band(&tris, origin);
    let exact = solid.volume();
    assert!((from_stl - exact).abs() <= band, "the STL says {from_stl}, the solid says {exact}, and six-decimal formatting accounts for {band}");
    assert!(
        (from_stl - from_step).abs() <= band,
        "the two formats disagree about the same solid: STEP says {from_step}, ASCII STL says {from_stl}, and the formatting accounts for {band}"
    );
    // And the mesh's own numbers — full precision, no formatting at all — agree with the file.
    let from_mesh: f64 = mesh.triangles().map(|t| (t.a - origin).dot((t.b - origin).cross(t.c - origin)) / 6.0).sum();
    assert!((from_mesh - from_step).abs() <= 1e-9 * from_mesh.abs().max(1.0), "the mesh says {from_mesh} and the STEP file says {from_step}");
}

#[test]
fn the_two_formats_agree_at_another_scale() {
    // The writer's tolerance and every check above scale with the part, so the same cross-check
    // has to hold two orders of magnitude up. The band is relative, which is the only honest way
    // to write it.
    let profile = support::rect_with_hole(0.0, 0.0, 400.0, 400.0, 120.0, 120.0, 280.0, 280.0);
    let solid = must(extrude(&profile, &Plane::xy(0.0), 160.0), "big washer");
    let step_text = must(step_faceted_brep(&solid, "big"), "step export");
    let from_step = audit(&solid, &step_text).volume;
    // (400 * 400 less 160 * 160) deep by 160.
    assert!((from_step - (400.0 * 400.0 - 160.0 * 160.0) * 160.0).abs() <= 1e-9 * from_step.abs(), "the big washer is {}", from_step);

    let mesh = must(mesh_from_solid(&solid), "mesh");
    let tris = parse_ascii_stl(&stl_ascii(&mesh));
    let origin = solid.bounds().min;
    let from_stl = triangle_volume(&tris, origin);
    let band = stl_rounding_band(&tris, origin);
    assert!((from_stl - from_step).abs() <= band, "STEP says {from_step}, ASCII STL says {from_stl}, and the band is {band}");
}

#[test]
fn a_part_with_rounded_edges_agrees_through_both_formats() {
    // A fillet is where a kernel is most likely to drop or double a face, and where a mesh and a
    // B-rep writer are least likely to agree, so the cross-check is run on one as well as on the
    // axis-aligned solids.
    for (name, solid) in [("box", box_solid()), ("rounded", filleted_box())] {
        let step_text = must(step_faceted_brep(&solid, name), "step export");
        let from_step = audit(&solid, &step_text).volume;
        let mesh = must(mesh_from_solid(&solid), "mesh");
        let tris = parse_ascii_stl(&stl_ascii(&mesh));
        let origin = solid.bounds().min;
        let from_stl = triangle_volume(&tris, origin);
        let band = stl_rounding_band(&tris, origin);
        assert!((from_stl - from_step).abs() <= band, "{name}: STEP {from_step}, STL {from_stl}, band {band}");
    }
}

// ---------------------------------------------------------------------------------------------
// Execution status
// ---------------------------------------------------------------------------------------------
//
// Every test in this file is `NOT EXECUTED`: it was written by an agent with no terminal, and
// nothing here has been compiled or run. The orchestrator has to run `cargo test -p cadcraft-solid
// --test step_interop` before any of it is evidence, and `cargo fmt` before `xtask ci` will pass.
//
// The one thing this file cannot become, by construction, is a substitute for `DAT-003`: it is a
// second opinion written by the same hand. It says the two halves of the file agree with each
// other and with the mesh writer. It says nothing at all about whether OCCT accepts the file.
