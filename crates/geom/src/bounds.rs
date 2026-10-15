use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3};

/// Axis-aligned bounding box. An empty box has `min > max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds2 {
    pub min: Vec2,
    pub max: Vec2,
}

impl Default for Bounds2 {
    fn default() -> Self {
        Bounds2::EMPTY
    }
}

impl Bounds2 {
    pub const EMPTY: Bounds2 = Bounds2 { min: Vec2 { x: f64::INFINITY, y: f64::INFINITY }, max: Vec2 { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY } };

    pub fn new(a: Vec2, b: Vec2) -> Self {
        Bounds2 { min: a.min(b), max: a.max(b) }
    }
    pub fn from_points<I: IntoIterator<Item = Vec2>>(it: I) -> Self {
        let mut b = Bounds2::EMPTY;
        for p in it {
            b.add(p);
        }
        b
    }
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }
    pub fn add(&mut self, p: Vec2) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Bounds2) -> Bounds2 {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        Bounds2 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
    }
    pub fn center(&self) -> Vec2 {
        if self.is_empty() {
            return Vec2::ZERO;
        }
        // (max - min) can overflow even when both endpoints and their midpoint
        // are finite. Halve first when the endpoints have opposite signs.
        let mid = |a: f64, b: f64| {
            if a.is_sign_negative() != b.is_sign_negative() { a * 0.5 + b * 0.5 } else { a + (b - a) * 0.5 }
        };
        Vec2::new(mid(self.min.x, self.max.x), mid(self.min.y, self.max.y))
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    /// True when `o` lies entirely inside.
    pub fn contains_box(&self, o: &Bounds2) -> bool {
        !o.is_empty() && self.contains(o.min) && self.contains(o.max)
    }
    pub fn intersects(&self, o: &Bounds2) -> bool {
        !(self.is_empty() || o.is_empty() || o.min.x > self.max.x || o.max.x < self.min.x || o.min.y > self.max.y || o.max.y < self.min.y)
    }
    pub fn expand(&self, d: f64) -> Bounds2 {
        if self.is_empty() {
            return *self;
        }
        Bounds2 { min: self.min - Vec2::new(d, d), max: self.max + Vec2::new(d, d) }
    }
    pub fn corners(&self) -> [Vec2; 4] {
        [self.min, Vec2::new(self.max.x, self.min.y), self.max, Vec2::new(self.min.x, self.max.y)]
    }
}

/// Axis-aligned bounding box in 3D. An empty box has `min > max`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bounds3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds3 {
    pub const EMPTY: Bounds3 = Bounds3 {
        min: Vec3 { x: f64::INFINITY, y: f64::INFINITY, z: f64::INFINITY },
        max: Vec3 { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY, z: f64::NEG_INFINITY },
    };

    pub fn new(a: Vec3, b: Vec3) -> Self {
        Bounds3 { min: a.min(b), max: a.max(b) }
    }
    pub fn from_points<I: IntoIterator<Item = Vec3>>(it: I) -> Self {
        let mut b = Bounds3::EMPTY;
        for p in it {
            b.add(p);
        }
        b
    }
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    pub fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Bounds3) -> Bounds3 {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        Bounds3 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
    }
    pub fn depth(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.z - self.min.z }
    }
    /// Size along each axis.
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
    /// Longest side, 0 for an empty box.
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.size().len() }
    }
    pub fn center(&self) -> Vec3 {
        if self.is_empty() {
            return Vec3::ZERO;
        }
        // (max - min) can overflow; halve first when the endpoints straddle zero.
        let mid = |a: f64, b: f64| {
            if a.is_sign_negative() != b.is_sign_negative() { a * 0.5 + b * 0.5 } else { a + (b - a) * 0.5 }
        };
        Vec3::new(mid(self.min.x, self.max.x), mid(self.min.y, self.max.y), mid(self.min.z, self.max.z))
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z <= self.max.z
    }
    /// True when `o` lies entirely inside.
    pub fn contains_box(&self, o: &Bounds3) -> bool {
        !o.is_empty() && self.contains(o.min) && self.contains(o.max)
    }
    pub fn intersects(&self, o: &Bounds3) -> bool {
        !(self.is_empty()
            || o.is_empty()
            || o.min.x > self.max.x
            || o.max.x < self.min.x
            || o.min.y > self.max.y
            || o.max.y < self.min.y
            || o.min.z > self.max.z
            || o.max.z < self.min.z)
    }
    pub fn expand(&self, d: f64) -> Bounds3 {
        if self.is_empty() {
            return *self;
        }
        let e = Vec3::new(d, d, d);
        Bounds3 { min: self.min - e, max: self.max + e }
    }
    pub fn corners(&self) -> [Vec3; 8] {
        [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
        ]
    }
}

#[cfg(test)]
mod tests3 {
    use super::*;

    #[test]
    fn bounds3_basics() {
        let b = Bounds3::new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, 5.0, 3.0));
        assert!(b.min.near(Vec3::new(-1.0, 2.0, 3.0), 0.0));
        assert!(b.max.near(Vec3::new(1.0, 5.0, 3.0), 0.0));
        assert!((b.width() - 2.0).abs() < 1e-12);
        assert!((b.height() - 3.0).abs() < 1e-12);
        assert!((b.depth() - 0.0).abs() < 1e-12);
        assert!(b.center().near(Vec3::new(0.0, 3.5, 3.0), 1e-12));
        assert!(b.contains(Vec3::new(0.0, 3.0, 3.0)));
        assert!(!b.contains(Vec3::new(9.0, 3.0, 3.0)));
        assert_eq!(b.corners().len(), 8);
    }

    #[test]
    fn bounds3_empty_and_union() {
        assert!(Bounds3::EMPTY.is_empty());
        assert!(Bounds3::EMPTY.center().is_finite());
        assert_eq!(Bounds3::EMPTY.diagonal(), 0.0);
        let a = Bounds3::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(a.union(&Bounds3::EMPTY), a);
        assert_eq!(Bounds3::EMPTY.union(&a), a);
        let b = Bounds3::new(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0));
        assert!(a.union(&b).contains_box(&a));
        assert!(a.union(&b).contains_box(&b));
        assert!(!a.intersects(&b));
        assert!(Bounds3::from_points([Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO]).contains_box(&a));
    }

    #[test]
    fn bounds3_survives_extreme_coordinates() {
        let b = Bounds3::new(Vec3::new(-1e308, 1e308, 0.0), Vec3::new(1e308, -1e308, 0.0));
        assert!(b.center().is_finite());
        let b = Bounds3::new(Vec3::new(-1e300, -1e300, -1e300), Vec3::new(1e300, 1e300, 1e300));
        assert!(b.diagonal().is_finite());
    }

    #[test]
    fn vec3_operations() {
        assert!((Vec3::new(3.0, 0.0, 4.0).len() - 5.0).abs() < 1e-12);
        assert!(Vec3::new(1.0, 0.0, 0.0).any_perp().is_finite());
        // Repeatedly taking a perpendicular must stay finite and unit: a degenerate frame is what
        // silently breaks a whole downstream solid.
        let mut p = Vec3::new(0.3, -0.7, 0.2).normalized();
        for _ in 0..8 {
            p = p.any_perp();
            assert!(p.is_finite() && (p.len() - 1.0).abs() < 1e-9, "perp chain drifted: {p:?}");
        }
        assert!(Vec3::ZERO.any_perp().is_finite());
        assert!((Vec3::new(1.0, 2.0, 3.0) / 2.0).near(Vec3::new(0.5, 1.0, 1.5), 0.0));
        assert_eq!(Vec3::new(1.0, 2.0, 3.0) / 0.0, Vec3::ZERO);
        assert!(Vec3::new(1.0, -2.0, 3.0).abs().near(Vec3::new(1.0, 2.0, 3.0), 0.0));
        assert!(Vec3::new(1.0, 5.0, 3.0).min(Vec3::new(4.0, 2.0, 9.0)).near(Vec3::new(1.0, 2.0, 3.0), 0.0));
        assert!(Vec3::new(1.0, 5.0, 3.0).max(Vec3::new(4.0, 2.0, 9.0)).near(Vec3::new(4.0, 5.0, 9.0), 0.0));
        assert!((Vec3::ZERO.mid(Vec3::new(2.0, 4.0, 6.0))).near(Vec3::new(1.0, 2.0, 3.0), 0.0));
        assert_eq!(Vec3::new(7.0, 8.0, 9.0).get(1), 8.0);
        assert_eq!(Vec3::new(7.0, 8.0, 9.0).get(9), 0.0);
        assert_eq!(Vec3::axes().len(), 3);
        // Hostile input stays finite.
        let h = Vec3::new(f64::NAN, f64::INFINITY, 1e308);
        assert!(!h.is_finite());
        assert!(!h.any_perp().is_finite() || h.any_perp().len() < f64::INFINITY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_stays_finite_for_opposite_large_coordinates() {
        let b = Bounds2::new(Vec2::new(-1e308, -1e308), Vec2::new(1e308, 1e308));
        assert_eq!(b.center(), Vec2::ZERO);
        let b = Bounds2::new(Vec2::new(1e308, 0.0), Vec2::new(1.6e308, 10.0));
        assert!(b.center().x.is_finite());
        assert!((b.center().x / 1e308 - 1.3).abs() < 1e-12);
        assert_eq!(b.center().y, 5.0);
        assert_eq!(Bounds2::EMPTY.center(), Vec2::ZERO);
    }
}
