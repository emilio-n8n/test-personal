//! Infinite planes with a reproducible in-plane frame.

use cadcraft_geom::Vec3;

use crate::SolidError;

/// An infinite plane: a point on it, a unit normal, and the first axis of its in-plane frame.
///
/// The frame matters as much as the plane: faces store their loops as 2D coordinates in it, so the
/// frame is part of the representation and has to be *stable*. `u` is stored rather than derived,
/// because a side wall of an extrusion wants its `u` along the extrusion edge, while an axis
/// aligned face wants `u` along a world axis so a profile sketched on the XY plane keeps its
/// coordinates. `v = normal × u`, so the frame is right-handed and `u × v = normal`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// A point on the plane.
    pub origin: Vec3,
    /// Unit normal.
    pub normal: Vec3,
    /// Unit in-plane axis, perpendicular to `normal`.
    pub u: Vec3,
}

impl Plane {
    /// A plane with the canonical frame for `normal`.
    ///
    /// Axis-aligned normals keep the world axes (the XY plane has `u = X`, `v = Y`), so local
    /// coordinates of a profile drawn there are its world coordinates. Everything else is built
    /// from the world axis least aligned with the normal, which is reproducible and never
    /// degenerate.
    pub fn new(origin: Vec3, normal: Vec3) -> Result<Plane, SolidError> {
        let n = normal.normalized();
        if !origin.is_finite() || !n.is_finite() || n.len() < 0.5 {
            return Err(if normal.is_finite() { SolidError::BadDirection } else { SolidError::NonFinite });
        }
        let u = Plane::canonical_u(n);
        Ok(Plane { origin, normal: n, u })
    }

    /// A plane with a caller-chosen in-plane axis. `u` is re-orthogonalized against the normal, so
    /// a slightly off axis is corrected rather than rejected.
    ///
    /// `u` is kept as given (orthogonalized), never swapped for the other axis: a wall face asks
    /// for `u` along its edge and expects the extrusion to run along `v`, and swapping them
    /// silently builds the face sideways.
    pub fn with_u(origin: Vec3, normal: Vec3, u: Vec3) -> Result<Plane, SolidError> {
        let p = Plane::new(origin, normal)?;
        let un = u.normalized();
        if !un.is_finite() || un.len() < 0.5 {
            return Err(SolidError::BadDirection);
        }
        let perp = un - p.normal * un.dot(p.normal);
        if perp.len() < 1e-9 || !perp.is_finite() {
            return Err(SolidError::BadDirection);
        }
        Ok(Plane { origin: p.origin, normal: p.normal, u: perp.normalized() })
    }

    fn canonical_u(n: Vec3) -> Vec3 {
        if n.z.abs() > 1.0 - 1e-12 {
            return Vec3::X;
        }
        if n.x.abs() > 1.0 - 1e-12 {
            return Vec3::Y;
        }
        if n.y.abs() > 1.0 - 1e-12 {
            return Vec3::Z;
        }
        n.any_perp()
    }

    /// The second in-plane axis.
    pub fn v(&self) -> Vec3 {
        self.normal.cross(self.u).normalized()
    }

    /// The XY plane at height `z`, normal +Z.
    pub fn xy(z: f64) -> Plane {
        Plane { origin: Vec3::new(0.0, 0.0, z), normal: Vec3::Z, u: Vec3::X }
    }

    /// The XZ plane at height `y`, normal +Y.
    pub fn xz(y: f64) -> Plane {
        Plane { origin: Vec3::new(0.0, y, 0.0), normal: Vec3::Y, u: Vec3::Z }
    }

    /// The YZ plane at `x`, normal +X.
    pub fn yz(x: f64) -> Plane {
        Plane { origin: Vec3::new(x, 0.0, 0.0), normal: Vec3::X, u: Vec3::Y }
    }

    /// World point from in-plane coordinates.
    pub fn to_world(&self, x: f64, y: f64) -> Vec3 {
        self.origin + self.u * x + self.v() * y
    }

    /// In-plane coordinates of a world point. The component along the normal is dropped.
    pub fn to_local(&self, p: Vec3) -> cadcraft_geom::Vec2 {
        let d = p - self.origin;
        cadcraft_geom::Vec2::new(d.dot(self.u), d.dot(self.v()))
    }

    /// Signed distance from the plane: positive on the side the normal points to.
    pub fn distance(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }

    /// Are the two planes parallel (normal in either direction)?
    pub fn parallel(&self, other: &Plane, dir_tol: f64) -> bool {
        self.normal.dot(other.normal).abs() >= 1.0 - dir_tol
    }

    /// Parallel and the same plane within `len_tol`.
    pub fn coplanar(&self, other: &Plane, len_tol: f64, dir_tol: f64) -> bool {
        self.parallel(other, dir_tol) && self.distance(other.origin).abs() <= len_tol
    }

    /// The same plane with the normal reversed, keeping the same point on the plane.
    pub fn flipped(&self) -> Plane {
        Plane { origin: self.origin, normal: self.normal * -1.0, u: self.u * -1.0 }
    }

    /// The same plane moved along its normal.
    pub fn offset(&self, distance: f64) -> Plane {
        Plane { origin: self.origin + self.normal * distance, normal: self.normal, u: self.u }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::{Vec2, Vec3};

    #[test]
    fn axis_aligned_frames_keep_world_axes() {
        let p = Plane::xy(3.0);
        assert!(p.u.near(Vec3::X, 0.0));
        assert!(p.v().near(Vec3::Y, 0.0));
        assert!((p.to_world(2.0, 5.0)).near(Vec3::new(2.0, 5.0, 3.0), 1e-15));
        assert_eq!(p.to_local(Vec3::new(2.0, 5.0, 9.0)), Vec2::new(2.0, 5.0));
    }

    #[test]
    fn frames_are_right_handed_for_any_normal() {
        for n in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.3, -0.7, 0.2),
            Vec3::new(-1.0, 1.0, -1.0),
        ] {
            let Ok(p) = Plane::new(Vec3::ZERO, n) else { panic!("plane") };
            assert!((p.u.len() - 1.0).abs() < 1e-12, "u not unit for {n:?}");
            assert!((p.v().len() - 1.0).abs() < 1e-12, "v not unit for {n:?}");
            assert!(p.u.cross(p.v()).near(p.normal, 1e-12), "frame not right handed for {n:?}");
            assert!(p.u.dot(p.normal).abs() < 1e-12);
        }
    }

    #[test]
    fn frames_are_reproducible() {
        let n = Vec3::new(0.3, -0.7, 0.2);
        let Ok(a) = Plane::new(Vec3::new(1.0, 2.0, 3.0), n) else { panic!("plane") };
        let Ok(b) = Plane::new(Vec3::new(9.0, 9.0, 9.0), n * 4.0) else { panic!("plane") };
        assert!(a.u.near(b.u, 0.0));
    }

    #[test]
    fn explicit_u_is_orthogonalized() {
        let Ok(p) = Plane::with_u(Vec3::ZERO, Vec3::Z, Vec3::new(1.0, 0.1, 0.0)) else { panic!("plane") };
        assert!(p.u.cross(p.v()).near(Vec3::Z, 1e-12));
        assert!(p.u.dot(Vec3::Z).abs() < 1e-12);
        // A u parallel to the normal is rejected, not silently kept.
        assert!(Plane::with_u(Vec3::ZERO, Vec3::Z, Vec3::Z).is_err());
    }

    #[test]
    fn hostile_input_is_refused() {
        assert!(Plane::new(Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(Plane::new(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
        assert!(Plane::new(Vec3::new(f64::INFINITY, 0.0, 0.0), Vec3::Z).is_err());
        assert!(Plane::new(Vec3::ZERO, Vec3::new(1e-300, 1e-300, 0.0)).is_err());
    }

    #[test]
    fn plane_relations() {
        let a = Plane::xy(0.0);
        let b = Plane::xy(1e-12);
        let c = Plane::xy(1.0);
        assert!(a.coplanar(&b, 1e-9, 1e-9));
        assert!(!a.coplanar(&c, 1e-9, 1e-9));
        assert!(a.parallel(&c, 1e-9));
        assert!(!a.parallel(&Plane::xz(0.0), 1e-9));
        assert!((a.distance(Vec3::new(5.0, 5.0, 2.0)) - 2.0).abs() < 1e-15);
        assert!((a.flipped().distance(Vec3::new(5.0, 5.0, 2.0)) + 2.0).abs() < 1e-15);
        assert!(a.offset(3.0).origin.near(Vec3::new(0.0, 0.0, 3.0), 1e-15));
    }
}
