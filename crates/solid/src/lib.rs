//! The CADCraft solid kernel: solids whose faces are all planar.
//!
//! # Representation
//!
//! A [`Solid`] is a set of [`Face`]s. A face is an infinite [`Plane`] plus a planar [`Regions`] —
//! an outer loop and its holes — stored in that plane's own 2D frame. Faces carry no vertices of
//! their own: the boundary between two faces *is* the shared loop, which is what makes this a
//! B-rep and not a mesh.
//!
//! Because every face is planar, a 3D boolean reduces to a 2D boolean in each face's plane, and
//! an exact planar chamfer is just another planar face. The price is that curved surfaces are
//! approximated by facets: a cylinder is an n-sided prism. That is a documented limit, not a
//! hidden approximation — [`extrude_faceted`] and [`revolve`] say so in their own docs.
//!
//! # Invariants
//!
//! Every operation returns a solid that satisfies these, and [`Solid::validate`] checks them.
//! An operation that cannot is an error, never a silently broken solid:
//!
//! - **I1 — finite and non-empty.** Every face has a finite plane, a non-empty region and a unit
//!   normal.
//! - **I2 — closed.** Every boundary edge is used by exactly two faces. A dangling edge means the
//!   shell is open.
//! - **I3 — consistently oriented.** Every boundary edge is used once in each direction, and the
//!   solid lies to the left of each face's loop.
//! - **I4 — outward.** Face normals point away from the material, so the signed volume is
//!   positive.
//! - **I5 — simple.** Two distinct faces touch only along shared boundary (checked by
//!   [`Solid::validate_intersections`], which is quadratic and off the default path).
//!
//! I2 and I3 together are what "manifold" means here: each edge is a fold between exactly two
//! faces, never a crack and never a three-way junction. I4 is verified by volume, so a solid
//! that is closed but inside-out fails validation instead of rendering as a hole.
//!
//! # Limits
//!
//! - Planar faces only: rounds are faceted. `fillet` makes this explicit by taking a segment count.
//! - Tolerance scales with the solid's size (`diagonal × 1e-9`, clamped to `[1e-12, 1e-3]`).
//!   Features thinner than the tolerance are not resolved.
//! - **Chamfer and fillet are implemented but not yet correct**: they are built as a tool and
//!   subtracted with the ordinary boolean, and every result goes through validation, so a blend
//!   that cannot be done is an error, never a broken body. The boolean's handling of a tool face
//!   that lies in the plane of a target face is unfinished, so blends are currently refused.
//! - Face, loop and triangle counts are capped ([`limits`]); oversized input is an error.
//! - Nothing here can panic on hostile input: every entry point returns [`Result`] or [`Option`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod boolean;
mod chamfer;
mod error;
mod extrude;
mod mass;
mod mesh;
mod plane;
mod section;
mod solid;

// The geometry types that appear in this crate's own API are re-exported so a caller needs one
// dependency, not two.
pub use boolean::{Bool3, boolean};
pub use cadcraft_geom::{Bounds3, Region, Regions, Vec2, Vec3};
pub use chamfer::{EdgeRef, chamfer, fillet};
pub use error::SolidError;
pub use extrude::{extrude, revolve};
pub use mass::{MassProperties, mass_properties};
pub use mesh::{Mesh, Triangle, mesh_from_solid, stl_ascii, stl_binary};
pub use plane::Plane;
pub use section::Section;
pub use solid::{Edge, Face, Solid, reverse_regions};

/// Caps that keep every operation bounded on hostile input.
pub mod limits {
    /// Faces of one solid.
    pub const MAX_FACES: usize = 60_000;
    /// Vertices in one face loop.
    pub const MAX_LOOP_VERTS: usize = 60_000;
    /// Triangles in a tessellation.
    pub const MAX_TRIANGLES: usize = 2_000_000;
    /// Segments used to approximate a round edge.
    pub const MAX_ROUND_SEGMENTS: usize = 64;
    /// Edges a single chamfer or fillet operation may touch.
    pub const MAX_BLENDS: usize = 2_000;
}

/// Geometry builders, public so integration tests build the same shapes the unit tests do.
///
/// They live in the library rather than in a test module on purpose: a profile's winding
/// convention (outer counter-clockwise, holes clockwise) is easy to get backwards, and every test
/// that derives its own helper re-derives that bug. One definition, used everywhere.
pub mod support {
    use super::*;
    use cadcraft_geom::{Region, Vec2, Vec3};

    /// A box spanning `min` to `max`, as an extrusion.
    ///
    /// Returns a `Result` rather than panicking on a degenerate extent: the crate denies
    /// `clippy::panic`, and a public builder that panics would be the wrong shape for it anyway.
    pub fn box_at(min: Vec3, max: Vec3) -> Result<Solid, SolidError> {
        let profile =
            Region::new(vec![Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), Vec2::new(min.x, max.y)]).to_regions();
        extrude(&profile, &Plane::xy(min.z), max.z - min.z)
    }

    /// A rectangular profile in a plane's frame.
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Regions {
        Region::new(vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]).to_regions()
    }

    /// A rectangular profile with a rectangular hole in it.
    pub fn rect_with_hole(x0: f64, y0: f64, x1: f64, y1: f64, hx0: f64, hy0: f64, hx1: f64, hy1: f64) -> Regions {
        Region {
            outer: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
            // Clockwise, as the kernel's convention requires: loops are wound so the material
            // is on their left.
            holes: vec![vec![Vec2::new(hx0, hy0), Vec2::new(hx0, hy1), Vec2::new(hx1, hy1), Vec2::new(hx1, hy0)]],
        }
        .to_regions()
    }
}

/// Working tolerance for a solid of the given diagonal.
pub fn tolerance_for(diagonal: f64) -> f64 {
    if !diagonal.is_finite() || diagonal <= 0.0 {
        return 1e-9;
    }
    (diagonal * 1e-9).clamp(1e-12, 1e-3)
}

/// Test-only wrappers that panic with the operation name, so a failing test says what it was
/// doing rather than printing a bare `Err`.
#[cfg(test)]
pub(crate) mod tests_support {
    pub use super::support::*;
    use super::{Solid, SolidError, Vec3};

    /// Unwrap a `Result` in a test, with the operation named in the panic.
    pub fn must<T>(r: Result<T, SolidError>, what: &str) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{what}: {e}"),
        }
    }

    /// [`support::box_at`], panicking with the extent on failure.
    pub fn box_at(min: Vec3, max: Vec3) -> Solid {
        super::support::box_at(min, max).unwrap_or_else(|e| panic!("box_at({min:?}, {max:?}): {e}"))
    }
}
