//! CADCraft's parametric 3D model: a feature tree, the parameters it reads, and the rules for
//! rebuilding one when something upstream changes.
//!
//! This layer is L3, above the solid kernel (`cadcraft-solid`, L0) and below the engine (L4). It
//! knows nothing about windows, commands or files on disk — it is the model, and the things that
//! show it to somebody are elsewhere.
//!
//! # The shape of it
//!
//! A [`Model`] is an ordered list of named [`Feature`]s and a table of [`Param`]eters. Building it
//! walks the list: each extrude or revolve adds material, each cut takes it away, each blend
//! rounds what is there. What a feature produces is cached per feature, and a feature that depends
//! on another reads the *cached* result — so a cut always takes away the solid its tool actually
//! made, not a recomputed guess.
//!
//! # What happens when an edit breaks the model
//!
//! The last good solid of every feature survives a failure, and the features above a failure are
//! marked *blocked* rather than *failed*. That distinction is the point: "this could not be built"
//! and "this was not built because the thing under it could not" are different facts, and a person
//! fixing a model needs to know which one they are looking at. A failed rebuild is never a lost
//! drawing.
//!
//! # What is not built yet
//!
//! - **A sketch does not read parameters.** Its profile is stored as drawn. A dimension drives an
//!   extrusion's depth, a revolve's angle and a blend's size, and drives the features that read
//!   those — but changing a parameter cannot move a profile's corners yet. Making sketches
//!   constraint-driven is the next piece of work on this layer.
//! - **Revolve does not build a solid.** The kernel's partial revolve meets its own end caps at a
//!   degenerate corner and the full turn has no faces to close with. The feature is wired, refused
//!   with the reason, and tested in the form the rule demands: a solid that validates, or a named
//!   failure — never a broken shell.
//!
//! # Topological naming
//!
//! There is none. Edges are referred to by geometry ([`cadcraft_solid::EdgeRef`]): a point that
//! should lie on the edge. Change the model enough and the reference stops resolving, which is an
//! error naming the edge that is gone — never a reference that quietly lands on a different one.
//! Real topological naming is real work and is not here yet; see the crate's tests for what that
//! means in practice.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod error;
mod expr;
mod feature;
mod file;
mod rebuild;

pub use error::ModelError;
pub use expr::{BinOp, Expr, evaluate, parse};
pub use feature::{Dep, EdgeRef, Feature, FeatureKind, Model, Param, Record, Sketch, State, formula_names, sketch_bounds};
pub use file::{Document, FORMAT_VERSION};
pub use rebuild::{Problem, Rebuild, rebuild, rebuild_after};
