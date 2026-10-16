//! Joints: the relationships between two components in an assembly.
//!
//! Every joint names the two components it connects and carries its own
//! parameters (axis, origin, limits). A joint that connects a component to
//! itself, or that names a component that does not exist in the assembly, is
//! refused with a named error when it is added.

use cadcraft_geom::Vec3;

use crate::{AsmError, Result};

/// The number of relative degrees of freedom a joint allows between its two
/// components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joint {
    /// No relative motion at all.
    Rigid { a: String, b: String },
    /// One rotary degree of freedom about `axis`.
    Revolute { a: String, b: String, axis: Vec3, origin: Vec3, limits: (f64, f64) },
    /// One translational degree of freedom along `axis`.
    Slider { a: String, b: String, axis: Vec3, origin: Vec3, limits: (f64, f64) },
    /// Two relative degrees: rotation and translation about and along `axis`.
    Cylindrical { a: String, b: String, axis: Vec3, origin: Vec3, limits: (f64, f64) },
    /// Three rotational degrees of freedom.
    Spherical { a: String, b: String, origin: Vec3 },
    /// Three relative degrees: translation in the plane, none perpendicular to
    /// it.
    Planar { a: String, b: String, normal: Vec3, origin: Vec3 },
    /// Keeps the two origins at a fixed `distance`: five relative degrees.
    Distance { a: String, b: String, origin: Vec3, distance: f64 },
}

impl Joint {
    /// The name of the first component this joint connects.
    pub fn component_a(&self) -> &str {
        match self {
            Joint::Rigid { a, .. }
            | Joint::Revolute { a, .. }
            | Joint::Slider { a, .. }
            | Joint::Cylindrical { a, .. }
            | Joint::Spherical { a, .. }
            | Joint::Planar { a, .. }
            | Joint::Distance { a, .. } => a.as_str(),
        }
    }

    /// The name of the second component this joint connects.
    pub fn component_b(&self) -> &str {
        match self {
            Joint::Rigid { b, .. }
            | Joint::Revolute { b, .. }
            | Joint::Slider { b, .. }
            | Joint::Cylindrical { b, .. }
            | Joint::Spherical { b, .. }
            | Joint::Planar { b, .. }
            | Joint::Distance { b, .. } => b.as_str(),
        }
    }

    /// The two component names this joint connects, as `(a, b)`.
    pub fn component_names(&self) -> (&str, &str) {
        (self.component_a(), self.component_b())
    }

    /// Shorter aliases for [`component_a`] and [`component_b`].
    pub fn a(&self) -> &str {
        self.component_a()
    }

    pub fn b(&self) -> &str {
        self.component_b()
    }

    /// The relative degrees of freedom this joint allows.
    pub fn degrees_of_freedom(&self) -> usize {
        match self {
            Joint::Rigid { .. } => 0,
            Joint::Revolute { .. } => 1,
            Joint::Slider { .. } => 1,
            Joint::Cylindrical { .. } => 2,
            Joint::Spherical { .. } => 3,
            Joint::Planar { .. } => 3,
            Joint::Distance { .. } => 5,
        }
    }

    /// Validate the joint's own parameters, ignoring whether the components
    /// exist. Returns `None` when everything is finite and well formed.
    pub fn check(&self) -> Option<AsmError> {
        let a = self.component_a().to_string();
        let b = self.component_b().to_string();
        if a == b {
            return Some(AsmError::SameComponent(a));
        }
        match self {
            Joint::Rigid { .. } => None,
            Joint::Revolute { axis, origin, limits, .. }
            | Joint::Slider { axis, origin, limits, .. }
            | Joint::Cylindrical { axis, origin, limits, .. } => {
                if !axis.is_finite() || !origin.is_finite() {
                    return Some(AsmError::NonFinite);
                }
                if axis.len() < 1e-12 {
                    return Some(AsmError::BadDirection("axis".to_string()));
                }
                if !limits.0.is_finite() || !limits.1.is_finite() {
                    return Some(AsmError::NonFinite);
                }
                if limits.0 > limits.1 {
                    return Some(AsmError::Refused("joint limits are inverted".to_string()));
                }
                None
            }
            Joint::Spherical { origin, .. } => {
                if !origin.is_finite() {
                    Some(AsmError::NonFinite)
                } else {
                    None
                }
            }
            Joint::Planar { normal, origin, .. } => {
                if !normal.is_finite() || !origin.is_finite() {
                    return Some(AsmError::NonFinite);
                }
                if normal.len() < 1e-12 {
                    return Some(AsmError::BadDirection("normal".to_string()));
                }
                None
            }
            Joint::Distance { origin, distance, .. } => {
                if !origin.is_finite() || !distance.is_finite() {
                    Some(AsmError::NonFinite)
                } else {
                    None
                }
            }
        }
    }

    // Convenience constructors so a caller never has to name every field.
    pub fn rigid(a: impl Into<String>, b: impl Into<String>) -> Self {
        Joint::Rigid { a: a.into(), b: b.into() }
    }

    pub fn revolute(a: impl Into<String>, b: impl Into<String>, axis: Vec3, origin: Vec3, limits: (f64, f64)) -> Self {
        Joint::Revolute { a: a.into(), b: b.into(), axis, origin, limits }
    }

    pub fn slider(a: impl Into<String>, b: impl Into<String>, axis: Vec3, origin: Vec3, limits: (f64, f64)) -> Self {
        Joint::Slider { a: a.into(), b: b.into(), axis, origin, limits }
    }

    pub fn cylindrical(a: impl Into<String>, b: impl Into<String>, axis: Vec3, origin: Vec3, limits: (f64, f64)) -> Self {
        Joint::Cylindrical { a: a.into(), b: b.into(), axis, origin, limits }
    }

    pub fn spherical(a: impl Into<String>, b: impl Into<String>, origin: Vec3) -> Self {
        Joint::Spherical { a: a.into(), b: b.into(), origin }
    }

    pub fn planar(a: impl Into<String>, b: impl Into<String>, normal: Vec3, origin: Vec3) -> Self {
        Joint::Planar { a: a.into(), b: b.into(), normal, origin }
    }

    pub fn distance(a: impl Into<String>, b: impl Into<String>, origin: Vec3, distance: f64) -> Self {
        Joint::Distance { a: a.into(), b: b.into(), origin, distance }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;

    #[test]
    fn dof_counts_per_joint_type() {
        assert_eq!(Joint::rigid("a", "b").degrees_of_freedom(), 0);
        assert_eq!(Joint::revolute("a", "b", Vec3::Z, Vec3::ZERO, (0.0, 1.0)).degrees_of_freedom(), 1);
        assert_eq!(Joint::slider("a", "b", Vec3::Z, Vec3::ZERO, (0.0, 1.0)).degrees_of_freedom(), 1);
        assert_eq!(Joint::cylindrical("a", "b", Vec3::Z, Vec3::ZERO, (0.0, 1.0)).degrees_of_freedom(), 2);
        assert_eq!(Joint::spherical("a", "b", Vec3::ZERO).degrees_of_freedom(), 3);
        assert_eq!(Joint::planar("a", "b", Vec3::Z, Vec3::ZERO).degrees_of_freedom(), 3);
        assert_eq!(Joint::distance("a", "b", Vec3::ZERO, 1.0).degrees_of_freedom(), 5);
    }

    #[test]
    fn every_refusal_fires() {
        let same = Joint::rigid("a", "a");
        assert!(matches!(same.check(), Some(AsmError::SameComponent(_))));

        let zero_axis = Joint::revolute("a", "b", Vec3::ZERO, Vec3::ZERO, (0.0, 1.0));
        assert!(matches!(zero_axis.check(), Some(AsmError::BadDirection(_))));

        let non_finite = Joint::slider("a", "b", Vec3::Z, Vec3::new(f64::NAN, 0.0, 0.0), (0.0, 1.0));
        assert!(matches!(non_finite.check(), Some(AsmError::NonFinite)));

        let inverted = Joint::cylindrical("a", "b", Vec3::Z, Vec3::ZERO, (1.0, 0.0));
        assert!(matches!(inverted.check(), Some(AsmError::Refused(_))));

        let zero_normal = Joint::planar("a", "b", Vec3::ZERO, Vec3::ZERO);
        assert!(matches!(zero_normal.check(), Some(AsmError::BadDirection(_))));

        assert!(Joint::rigid("a", "b").check().is_none());
    }
}
