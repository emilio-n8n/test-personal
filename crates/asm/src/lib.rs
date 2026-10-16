//! CADCraft assemblies: components, placements and the joints that relate them.
//!
//! # Invariants
//!
//! Assembly geometry obeys the same last-good rule as `cadcraft_model`:
//!
//! 1. **A failed rebuild keeps its last good placement.** A component that no
//!    longer builds retains its previous `origin` and `body`; it is reported
//!    `State::Failed`, never silently emptied or removed.
//! 2. **A blocked component is not a broken one.** When an earlier component
//!    fails, every component after it is reported `State::Blocked`, each keeping
//!    its own last good placement, so "this could not be built" and "this was
//!    not built because something under it failed" stay distinct.
//!
//! Components are named: empty names and duplicate names are refused by named
//! errors. Joints connect two distinct components that must actually exist in the
//! assembly.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use cadcraft_geom::Vec3;
use cadcraft_solid::Solid;
use serde::{Deserialize, Serialize};

mod joint;
pub use joint::Joint;

// ---------------------------------------------------------------- Quat

/// A unit quaternion in Hamilton (scalar-first) convention: `q = w + x i + y j + z k`.
///
/// Right-handed rotation convention: rotating a vector `v` by `q` gives
/// `q * (0, v) * q^{-1}`, computed in [`Quat::rotate_vec3`] / [`Quat::rotate`].
/// A non-unit quaternion is normalised before a vector is rotated, so a
/// quaternion always acts as a rotation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(w: f64, x: f64, y: f64, z: f64) -> Self {
        Quat { w, x, y, z }
    }

    /// Alternative constructor, `(x, y, z, w)` argument order.
    pub fn from_xyzw(x: f64, y: f64, z: f64, w: f64) -> Self {
        Quat { w, x, y, z }
    }

    pub fn normalized(self) -> Self {
        self.normalize()
    }

    pub fn length(self) -> f64 {
        self.w
            .hypot(self.x)
            .hypot(self.y)
            .hypot(self.z)
    }

    /// Quaternion from an axis and an angle (radians, right-handed).
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Self {
        if !axis.is_finite() || !angle.is_finite() {
            return Quat {
                w: f64::NAN,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
        }
        let len = axis.len();
        if len <= 0.0 {
            return Quat::IDENTITY;
        }
        let k = axis * (1.0 / len);
        let (s, c) = (angle * 0.5).sin_cos();
        Quat {
            w: c,
            x: k.x * s,
            y: k.y * s,
            z: k.z * s,
        }
    }

    pub fn conjugate(self) -> Self {
        Quat {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    pub fn multiply(self, rhs: Self) -> Self {
        Quat {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }

    pub fn mul(self, rhs: Self) -> Self {
        self.multiply(rhs)
    }

    /// Rotate `v` by this quaternion (the quaternion is normalised first).
    pub fn rotate_vec3(self, v: Vec3) -> Vec3 {
        let q = self.normalize();
        let p = Quat::new(0.0, v.x, v.y, v.z);
        let r = q.multiply(p).multiply(q.conjugate());
        Vec3::new(r.x, r.y, r.z)
    }

    pub fn rotate(self, v: Vec3) -> Vec3 {
        self.rotate_vec3(v)
    }

    pub fn rotate_vector(self, v: Vec3) -> Vec3 {
        self.rotate_vec3(v)
    }

    pub fn apply(&self, v: Vec3) -> Vec3 {
        (*self).rotate_vec3(v)
    }

    pub fn normalize(self) -> Self {
        let n = self
            .w
            .hypot(self.x)
            .hypot(self.y)
            .hypot(self.z);
        if n > 0.0 && n.is_finite() {
            Quat {
                w: self.w / n,
                x: self.x / n,
                y: self.y / n,
                z: self.z / n,
            }
        } else {
            Quat::IDENTITY
        }
    }

    pub fn is_finite(&self) -> bool {
        self.w.is_finite() && self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl std::ops::Mul for Quat {
    type Output = Quat;
    fn mul(self, rhs: Quat) -> Quat {
        self.multiply(rhs)
    }
}

// ---------------------------------------------------------------- Transform

/// A rigid placement: a translation and a rotation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    pub fn new(translation: Vec3, rotation: Quat) -> Self {
        Transform {
            translation,
            rotation,
        }
    }

    pub fn apply(&self, p: Vec3) -> Vec3 {
        self.rotation.rotate_vec3(p) + self.translation
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

// ---------------------------------------------------------------- Component / state

/// Build state of a component or joint reference, mirrors `cadcraft_model::State`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Built,
    Pending,
    Failed(String),
    Blocked(String),
    Suppressed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub body: Solid,
    pub origin: Transform,
}

impl Component {
    pub fn new(name: impl Into<String>, body: Solid, origin: Transform) -> Self {
        Component {
            name: name.into(),
            body,
            origin,
        }
    }
}

// ---------------------------------------------------------------- AsmError

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AsmError {
    #[error("empty name")]
    EmptyName,
    #[error("duplicate `{0}`")]
    Duplicate(String),
    #[error("no component named `{0}`")]
    NotFound(String),
    #[error("component `{0}` cannot be connected to itself")]
    SameComponent(String),
    #[error("non-finite value")]
    NonFinite,
    #[error("bad direction `{0}`")]
    BadDirection(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("failed: {0}")]
    Failed(String),
    #[error("blocked: {0}")]
    Blocked(String),
}

pub type Result<T> = std::result::Result<T, AsmError>;

// ---------------------------------------------------------------- Assembly

/// An assembly: its components and the joints between them.
#[derive(Clone, Debug, Default)]
pub struct Assembly {
    pub components: Vec<Component>,
    pub joints: Vec<Joint>,
}

impl Assembly {
    pub fn new() -> Self {
        Assembly::default()
    }

    pub fn add_component(&mut self, component: Component) -> Result<()> {
        if component.name.is_empty() {
            return Err(AsmError::EmptyName);
        }
        if self.components.iter().any(|c| c.name == component.name) {
            return Err(AsmError::Duplicate(component.name.clone()));
        }
        self.components.push(component);
        Ok(())
    }

    pub fn add(&mut self, component: Component) -> Result<()> {
        self.add_component(component)
    }

    pub fn push_component(&mut self, component: Component) -> Result<()> {
        self.add_component(component)
    }

    pub fn push(&mut self, component: Component) -> Result<()> {
        self.add_component(component)
    }

    pub fn get_component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }

    pub fn component(&self, name: &str) -> Result<&Component, AsmError> {
        self.get_component(name)
            .ok_or_else(|| AsmError::NotFound(name.to_string()))
    }

    pub fn find_component(&self, name: &str) -> Option<&Component> {
        self.get_component(name)
    }

    pub fn lookup_component(&self, name: &str) -> Option<&Component> {
        self.get_component(name)
    }

    pub fn component_mut(&mut self, name: &str) -> Result<&mut Component, AsmError> {
        let pos = self
            .components
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| AsmError::NotFound(name.to_string()))?;
        Ok(&mut self.components[pos])
    }

    pub fn remove_component(&mut self, name: &str) -> Result<Component, AsmError> {
        if name.is_empty() {
            return Err(AsmError::EmptyName);
        }
        let pos = self
            .components
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| AsmError::NotFound(name.to_string()))?;
        let removed = self.components.remove(pos);
        // A joint that pointed at a removed component has no meaning.
        self.joints.retain(|j| {
            let (a, b) = j.component_names();
            a != name && b != name
        });
        Ok(removed)
    }

    pub fn remove(&mut self, name: &str) -> Result<Component, AsmError> {
        self.remove_component(name)
    }

    pub fn remove_at(&mut self, index: usize) -> Result<Component, AsmError> {
        if index >= self.components.len() {
            return Err(AsmError::Refused("component index out of range".to_string()));
        }
        let removed = self.components.remove(index);
        let name = removed.name.clone();
        self.joints.retain(|j| {
            let (a, b) = j.component_names();
            a != name.as_str() && b != name.as_str()
        });
        Ok(removed)
    }

    pub fn add_joint(&mut self, joint: Joint) -> Result<()> {
        let a = joint.component_a().to_string();
        let b = joint.component_b().to_string();

        if a == b {
            return Err(AsmError::SameComponent(a));
        }
        if !self.components.iter().any(|c| c.name == a) {
            return Err(AsmError::NotFound(a));
        }
        if !self.components.iter().any(|c| c.name == b) {
            return Err(AsmError::NotFound(b));
        }
        if let Some(err) = joint.check() {
            return Err(err);
        }
        self.joints.push(joint);
        Ok(())
    }

    pub fn push_joint(&mut self, joint: Joint) -> Result<()> {
        self.add_joint(joint)
    }

    pub fn joint(&self, index: usize) -> Option<&Joint> {
        self.joints.get(index)
    }

    pub fn remove_joint(&mut self, index: usize) -> Result<Joint, AsmError> {
        if index >= self.joints.len() {
            return Err(AsmError::Refused("joint index out of range".to_string()));
        }
        Ok(self.joints.remove(index))
    }

    /// Apply `build` to each component in order. A failed component keeps its
    /// last good placement and body; components after a failure are `Blocked`.
    pub fn rebuild<F>(&mut self, mut build: F) -> Vec<State>
    where
        F: FnMut(&Component) -> std::result::Result<Solid, String>,
    {
        let mut states = Vec::with_capacity(self.components.len());
        let mut failed = false;
        let count = self.components.len();
        for i in 0..count {
            if failed {
                states.push(State::Blocked(
                    "not rebuilt: an earlier component failed".to_string(),
                ));
                continue;
            }
            let result = build(&self.components[i]);
            match result {
                Ok(solid) => {
                    self.components[i].body = solid;
                    states.push(State::Built);
                }
                Err(msg) => {
                    failed = true;
                    states.push(State::Failed(msg));
                }
            }
        }
        states
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_solid::support;

    #[test]
    fn quat_rotate_90_degrees() {
        let q = Quat::from_axis_angle(Vec3::Z, std::f64::consts::FRAC_PI_2);
        let v = q.rotate_vec3(Vec3::X);
        assert!((v - Vec3::Y).len() < 1e-12, "got {v:?}");
    }

    #[test]
    fn two_rotations_compose() {
        let q1 = Quat::from_axis_angle(Vec3::Z, std::f64::consts::FRAC_PI_2);
        let q2 = Quat::from_axis_angle(Vec3::Z, std::f64::consts::FRAC_PI_2);
        let composed = q1 * q2;
        let v = composed.rotate_vec3(Vec3::X);
        // Two 90-degree rotations about Z give 180 degrees: X -> -X.
        assert!((v - (-Vec3::X)).len() < 1e-12, "got {v:?}");
        // And direct composition of two half-turn-free paths agrees:
        let a = q1.rotate_vec3(Vec3::X);
        let b = q2.rotate_vec3(a);
        let c = composed.rotate_vec3(Vec3::X);
        assert!((b - c).len() < 1e-12);
    }

    #[test]
    fn non_unit_quaternion_normalises() {
        let q = Quat::new(2.0, 0.0, 0.0, 0.0);
        let n = q.normalize();
        assert!((n.w - 1.0).abs() < 1e-12);
        assert!(n.is_finite());
        // A non-unit quaternion acts as the same rotation once normalised.
        let q2 = Quat::new(0.0, 2.0, 0.0, 0.0).normalize();
        let v = q2.rotate_vec3(Vec3::new(0.0, 1.0, 0.0));
        assert!((v - Vec3::new(0.0, -1.0, 0.0)).len() < 1e-12, "got {v:?}");
    }

    #[test]
    fn assembly_refuses_empty_and_duplicate_names() {
        let mut a = Assembly::new();
        let box_solid = support::box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).expect("box");
        let c = Component::new("part", box_solid.clone(), Transform::IDENTITY);
        assert!(matches!(a.add_component(c.clone()), Err(AsmError::EmptyName) | Ok(())));
        // empty name refused
        let empty = Component::new("", box_solid.clone(), Transform::IDENTITY);
        assert!(matches!(a.add_component(empty), Err(AsmError::EmptyName)));
        assert!(a.add_component(Component::new("part", box_solid.clone(), Transform::IDENTITY)).is_ok());
        // duplicate refused
        assert!(matches!(
            a.add_component(Component::new("part", box_solid, Transform::IDENTITY)),
            Err(AsmError::Duplicate(_))
        ));
        // lookup missing
        assert!(matches!(a.component("nope"), Err(AsmError::NotFound(_))));
        // remove missing
        assert!(matches!(a.remove_component("nope"), Err(AsmError::NotFound(_))));
    }

    #[test]
    fn placement_discipline_failed_keeps_origin_blocked_is_not_failed() {
        let mut a = Assembly::new();
        let box_solid = support::box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).expect("box");
        a.add_component(Component::new("a", box_solid.clone(), Transform::IDENTITY))
            .expect("add a");
        a.add_component(Component::new(
            "b",
            box_solid.clone(),
            Transform::new(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY),
        ))
        .expect("add b");

        let before_a = a.component("a").expect("a").origin;
        let before_b = a.component("b").expect("b").origin;

        let states = a.rebuild(|c| {
            if c.name == "a" {
                Err("boom".to_string())
            } else {
                Ok(c.body.clone())
            }
        });

        assert_eq!(states.len(), 2);
        assert!(matches!(states[0], State::Failed(_)));
        assert!(matches!(states[1], State::Blocked(_)));
        // Failed component keeps its last good placement.
        assert_eq!(a.component("a").expect("a").origin, before_a);
        // Blocked component also keeps its own last good placement.
        assert_eq!(a.component("b").expect("b").origin, before_b);
    }
}