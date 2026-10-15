//! Why a solid operation could not produce a valid result.

use cadcraft_geom::Vec3;

/// Everything that can go wrong building or combining solids.
///
/// The point of the enum is that a failure is always *named*. The kernel never returns a solid
/// that quietly violates an invariant: if a cut would leave a non-manifold shell, that is
/// [`SolidError::Invalid`], not a broken body handed to the renderer.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SolidError {
    #[error("no geometry: the operation had nothing to work with")]
    Empty,

    #[error("non-finite coordinate or normal")]
    NonFinite,

    #[error("degenerate direction: cannot build a plane from it")]
    BadDirection,

    #[error("tolerance {0} is not positive and finite")]
    BadTolerance(f64),

    #[error("distance {0} is not positive and finite")]
    BadDistance(f64),

    #[error("profile has no area")]
    EmptyProfile,

    #[error("edge {0:?} -> {1:?} is used by {2} faces; a closed shell uses every edge exactly twice")]
    OpenEdge(Vec3, Vec3, usize),

    #[error("the shell does not enclose a positive volume ({0}); faces point inward or the shell is not closed")]
    NonPositiveVolume(f64),

    #[error("faces do not meet consistently along {0}")]
    InconsistentFaces(String),

    #[error("faces {0} and {1} overlap away from a shared edge")]
    FaceOverlap(usize, usize),

    #[error("the operation produced an invalid solid: {0}")]
    Invalid(String),

    #[error("the plane section failed: {0}")]
    Section(String),

    #[error("too many faces ({0}), the limit is {1}")]
    TooManyFaces(usize, usize),

    #[error("too many triangles ({0})")]
    TooManyTriangles(usize),
}
