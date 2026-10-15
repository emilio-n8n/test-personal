use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// A 2D point or vector in drawing units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const X: Vec2 = Vec2 { x: 1.0, y: 0.0 };
    pub const Y: Vec2 = Vec2 { x: 0.0, y: 1.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
    /// Unit vector at `angle` radians.
    pub fn from_angle(angle: f64) -> Self {
        Vec2::new(angle.cos(), angle.sin())
    }
    pub fn polar(center: Vec2, radius: f64, angle: f64) -> Self {
        center + Vec2::from_angle(angle) * radius
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    pub fn dist(self, o: Vec2) -> f64 {
        (self - o).len()
    }
    pub fn angle(self) -> f64 {
        crate::norm_angle(self.y.atan2(self.x))
    }
    pub fn angle_to(self, o: Vec2) -> f64 {
        (o - self).angle()
    }
    /// Unit vector, or zero for a zero-length vector.
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l > crate::EPS { self / l } else { Vec2::ZERO }
    }
    /// Rotated 90° counter-clockwise.
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    pub fn rotate(self, a: f64) -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    pub fn rotate_about(self, center: Vec2, a: f64) -> Vec2 {
        center + (self - center).rotate(a)
    }
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
    pub fn mid(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.midpoint(o.x), self.y.midpoint(o.y))
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn near(self, o: Vec2, tol: f64) -> bool {
        // Squaring very large finite values can overflow on both sides and make
        // unrelated points compare equal (infinity <= infinity).
        tol.is_finite() && tol >= 0.0 && self.dist(o) <= tol
    }
    pub fn to3(self, z: f64) -> Vec3 {
        Vec3::new(self.x, self.y, z)
    }
    pub fn min(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.min(o.x), self.y.min(o.y))
    }
    pub fn max(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.max(o.x), self.y.max(o.y))
    }
    /// Mirror across the infinite line through `a` and `b`.
    pub fn mirror(self, a: Vec2, b: Vec2) -> Vec2 {
        let d = (b - a).normalized();
        if d == Vec2::ZERO {
            return self;
        }
        let v = self - a;
        let proj = d * v.dot(d);
        a + proj * 2.0 - v
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Div<f64> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f64) -> Vec2 {
        if s == 0.0 { Vec2::ZERO } else { Vec2::new(self.x / s, self.y / s) }
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// A 3D point or vector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const X: Vec3 = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    /// The three unit axes in order.
    pub fn axes() -> [Vec3; 3] {
        [Vec3::X, Vec3::Y, Vec3::Z]
    }
    /// Component `i` (`0`, `1`, `2`), or `0.0` out of range.
    pub fn get(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => 0.0,
        }
    }
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
    /// Unit vector at `angle` radians.
    pub fn from_angle(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Vec3::new(c, s, 0.0)
    }
    /// Angle of the direction from `self` to `o`, in `[0, 2π)`.
    pub fn angle_to(self, o: Vec3) -> f64 {
        crate::norm_angle((o - self).y.atan2((o - self).x))
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn len(self) -> f64 {
        // Avoid overflowing the squared components of large finite vectors.
        self.x.hypot(self.y).hypot(self.z)
    }
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    pub fn dist(self, o: Vec3) -> f64 {
        (self - o).len()
    }
    /// Unit vector, or zero for a zero-length vector.
    pub fn normalized(self) -> Vec3 {
        let l = self.len();
        if l > crate::EPS { self * (1.0 / l) } else { Vec3::ZERO }
    }
    /// Component-wise absolute value.
    pub fn abs(self) -> Vec3 {
        Vec3::new(self.x.abs(), self.y.abs(), self.z.abs())
    }
    /// Component-wise minimum.
    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    /// Component-wise maximum.
    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    pub fn mid(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.midpoint(o.x), self.y.midpoint(o.y), self.z.midpoint(o.z))
    }
    /// A vector of unit length perpendicular to `self` (zero for a non-finite or tiny input).
    ///
    /// Picks the world axis least aligned with `self` so the result never degenerates, which is
    /// what makes frame construction reproducible.
    pub fn any_perp(self) -> Vec3 {
        let a = self.abs();
        let least = if a.x <= a.y && a.x <= a.z {
            Vec3::X
        } else if a.y <= a.z {
            Vec3::Y
        } else {
            Vec3::Z
        };
        self.cross(least).normalized()
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// Distance within `tol`, refusing to square (which overflows for large finite vectors).
    pub fn near(self, o: Vec3, tol: f64) -> bool {
        tol.is_finite() && tol >= 0.0 && self.dist(o) <= tol
    }
    /// Rotated about `axis` by `angle` radians (Rodrigues' formula).
    ///
    /// `axis` need not be normalized. A zero axis or a non-finite angle leaves the vector alone, so
    /// a caller that has not checked its input gets a point that still means something.
    pub fn rotate_about(self, axis: Vec3, angle: f64) -> Vec3 {
        let k = axis.normalized();
        if k.len() < 0.5 || !angle.is_finite() {
            return self;
        }
        let (s, c) = angle.sin_cos();
        self * c + k.cross(self) * s + k * (k.dot(self) * (1.0 - c))
    }
    /// Signed angle from `self` to `o` about `axis`, in `(-π, π]`.
    ///
    /// The three-dimensional counterpart of [`Vec3::angle_to`], which only ever sweeps in the XY
    /// plane and so silently gives the wrong answer for any other orientation.
    pub fn signed_angle_about(self, o: Vec3, axis: Vec3) -> f64 {
        let k = axis.normalized();
        if k.len() < 0.5 {
            return 0.0;
        }
        (k.dot(self.cross(o))).atan2(self.dot(o))
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f64) -> Vec3 {
        if s == 0.0 { Vec3::ZERO } else { Vec3::new(self.x / s, self.y / s, self.z / s) }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midpoint_handles_finite_coordinate_extremes() {
        for (a, b, expected) in [
            (Vec2::new(-1e308, 1e308), Vec2::new(1e308, -1e308), Vec2::ZERO),
            (Vec2::new(1e308, -1e308), Vec2::new(1.6e308, -1.6e308), Vec2::new(1.3e308, -1.3e308)),
            (Vec2::new(f64::from_bits(1), 0.0), Vec2::new(f64::from_bits(1), 0.0), Vec2::new(f64::from_bits(1), 0.0)),
        ] {
            assert_eq!(a.mid(b), expected);
            assert_eq!(b.mid(a), expected);
        }
    }

    #[test]
    fn near_uses_finite_nonnegative_tolerance_without_squaring() {
        let origin = Vec2::ZERO;
        let distant = Vec2::new(1e200, 0.0);
        assert!(!distant.near(origin, 1e100), "overflowed squares must not compare equal");
        assert!(distant.near(origin, 1.1e200));
        assert!(origin.near(origin, 0.0));
        assert!(!origin.near(origin, -1.0));
        assert!(!origin.near(origin, f64::INFINITY));
    }

    #[test]
    fn large_finite_vec3_has_finite_length_and_unit_direction() {
        let v = Vec3::new(3e200, 4e200, 0.0);
        assert!((v.len() / 1e200 - 5.0).abs() < 1e-12);
        let unit = v.normalized();
        assert!((unit.x - 0.6).abs() < 1e-12);
        assert!((unit.y - 0.8).abs() < 1e-12);
        assert!((unit.len() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rotating_about_an_axis_keeps_length_and_sweeps_the_right_way() {
        let v = Vec3::new(1.0, 0.0, 0.0);
        let quarter = v.rotate_about(Vec3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2);
        assert!((quarter.len() - 1.0).abs() < 1e-12);
        assert!(quarter.near(Vec3::new(0.0, 1.0, 0.0), 1e-12), "counter-clockwise about +Z");
        // The axis need not be normalized, and rotating about a point off the vector's own
        // perpendicular must swing it in the right plane.
        let same = v.rotate_about(Vec3::new(0.0, 0.0, 7.0), std::f64::consts::PI);
        assert!(same.near(Vec3::new(-1.0, 0.0, 0.0), 1e-12));
        let y = Vec3::new(0.0, 1.0, 0.0);
        assert!(y.rotate_about(Vec3::new(0.0, 1.0, 0.0), 1.0).near(y, 1e-12), "a vector on the axis does not move");
    }

    #[test]
    fn rotating_with_a_degenerate_axis_or_angle_leaves_the_vector_alone() {
        let v = Vec3::new(3.0, 4.0, 0.0);
        assert!(v.rotate_about(Vec3::ZERO, 1.0).near(v, 0.0));
        assert!(v.rotate_about(Vec3::new(0.0, 0.0, 1.0), f64::NAN).near(v, 0.0));
    }

    #[test]
    fn a_signed_angle_is_measured_about_the_axis_it_is_given() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let quarter = std::f64::consts::FRAC_PI_2;
        assert!((x.signed_angle_about(y, Vec3::new(0.0, 0.0, 1.0)) - quarter).abs() < 1e-12);
        assert!((y.signed_angle_about(x, Vec3::new(0.0, 0.0, 1.0)) + quarter).abs() < 1e-12);
        // The same right angle in the XZ plane: a 2D-only angle reads this pair as `π`, which is the
        // wrong answer, and the reason the fillet cross-section needed a three-dimensional arc.
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 0.0, -1.0);
        assert!((a.signed_angle_about(b, Vec3::new(0.0, 1.0, 0.0)) - quarter).abs() < 1e-12);
        assert!((a.angle_to(b) - std::f64::consts::PI).abs() < 1e-12, "the XY-plane angle is the one that is wrong");
        assert!(a.signed_angle_about(b, Vec3::ZERO).abs() < f64::EPSILON, "no axis means no angle");
    }
}
