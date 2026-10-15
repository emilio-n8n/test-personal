//! Volume, surface area and centroid of a planar-faced solid.
//!
//! Two independent computations live here, and the tests compare them on every solid the kernel
//! builds:
//!
//! - [`mass_properties`] integrates the **tessellation** as tetrahedra from a reference point.
//! - [`Solid::volume_from_faces`] integrates the **faces** as pyramids: `Σ A·d/3` over planar
//!   faces, with no triangulation involved.
//!
//! They share no code path, so a tessellation that loses a hole, a face that was dropped, or a
//! loop wound the wrong way shows up as a disagreement instead of a plausible number. The
//! reference point is the bounds centre, never the world origin: a body at (1e6, 1e6, 1e6) would
//! otherwise integrate a trillion-unit tetrahedron and lose every digit to cancellation.

use cadcraft_geom::Vec3;

use crate::{Solid, SolidError, mesh::mesh_from_solid};

/// Volume, area and centroid of a solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    /// Enclosed volume. Positive for outward-facing faces.
    pub volume: f64,
    /// Total surface area.
    pub area: f64,
    /// Centre of mass.
    pub centroid: Vec3,
}

/// Volume, area and centroid, integrated over the tessellation.
pub fn mass_properties(solid: &Solid) -> Result<MassProperties, SolidError> {
    let mesh = mesh_from_solid(solid)?;
    let ref_point = solid.bounds().center();
    let mut volume = 0.0;
    let mut weighted = Vec3::ZERO;
    for t in mesh.triangles() {
        // Tetrahedra from the reference point; one face at a time.
        let (a, b, c) = (t.a - ref_point, t.b - ref_point, t.c - ref_point);
        let v = a.dot(b.cross(c)) / 6.0;
        if !v.is_finite() {
            return Err(SolidError::NonFinite);
        }
        volume += v;
        // The tetrahedron's centroid is the mean of its four vertices, one of which is `ref_point`.
        let centroid = (a + b + c) / 4.0;
        weighted = weighted + centroid * v;
    }
    let area = solid.area();
    let centroid = if volume.abs() > 0.0 {
        let c = weighted / volume;
        if c.is_finite() { ref_point + c } else { ref_point }
    } else {
        ref_point
    };
    Ok(MassProperties { volume, area, centroid })
}

impl Solid {
    /// Volume as a sum of face pyramids about `apex`: `Σ A·d/3`.
    ///
    /// Needs no triangulation, so it is the independent check on [`Solid::volume`]. `apex` should
    /// be inside the solid; any point works mathematically, but an interior one keeps the terms
    /// from cancelling.
    pub fn volume_from_faces(&self, apex: Vec3) -> Result<f64, SolidError> {
        if !apex.is_finite() {
            return Err(SolidError::NonFinite);
        }
        let mut v = 0.0;
        for f in &self.faces {
            // Outward normals put the material on the negative side, so the height is -distance.
            let d = -f.plane.distance(apex);
            let term = f.area() * d / 3.0;
            if !term.is_finite() {
                return Err(SolidError::NonFinite);
            }
            v += term;
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{box_at, must};

    #[test]
    fn a_box_has_the_volume_of_its_own_dimensions() {
        let s = box_at(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        let m = must(s.mass_properties(), "mass properties");
        assert!((m.volume - 24.0).abs() < 1e-9, "volume {}", m.volume);
        assert!((m.area - 2.0 * (2.0 * 3.0 + 2.0 * 4.0 + 3.0 * 4.0)).abs() < 1e-9, "area {}", m.area);
        assert!(m.centroid.near(Vec3::new(1.0, 1.5, 2.0), 1e-9), "centroid {:?}", m.centroid);
    }

    #[test]
    fn the_two_independent_volumes_agree() {
        let s = box_at(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        let a = s.volume();
        let b = must(s.volume_from_faces(s.bounds().center()), "volume from faces");
        assert!((a - b).abs() < 1e-9, "mesh {a} vs faces {b}");
    }

    #[test]
    fn mass_properties_are_stable_far_from_the_origin() {
        let base = Vec3::new(1e6, -1e6, 1e6);
        let s = box_at(base, base + Vec3::new(3.0, 4.0, 5.0));
        let m = must(s.mass_properties(), "mass properties");
        assert!((m.volume - 60.0).abs() < 1e-6, "volume {}", m.volume);
        assert!(m.centroid.near(base + Vec3::new(1.5, 2.0, 2.5), 1e-6), "centroid {:?}", m.centroid);
        let f = must(s.volume_from_faces(base + Vec3::new(1.5, 2.0, 2.5)), "volume from faces");
        assert!((m.volume - f).abs() < 1e-6, "mesh {} vs faces {}", m.volume, f);
    }
}
