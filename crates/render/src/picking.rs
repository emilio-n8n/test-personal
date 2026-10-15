//! Ray picking against the solid kernel — the geometry behind "what did the user click on".
//!
//! Nothing here knows what a GUI is. A [`Ray`] and a [`cadcraft_solid::Solid`] go in, the nearest
//! [`Hit`] comes out, and there is no window, no widget and no egui in this file (nor anywhere in
//! the crate below the UI layer). The viewport panel in `cadcraft-ui-egui` turns a pointer
//! position into a ray with [`ray_through_pixel`] and reads the answer with [`pick_scene`].
//!
//! # Two ways to answer the same question
//!
//! The kernel's representation is a B-rep of planar faces with exact regions, so
//! [`ray_faces_intersection`] answers *exactly*: it intersects the ray with each face's plane and
//! asks the region whether the crossing is on that face. A chamfer or a round is therefore hit on
//! its true surface, and so is the *hole* of a face with one.
//!
//! [`ray_mesh_intersection`] answers from a tessellation, through Möller–Trumbore, which is what a
//! GPU depth buffer would do. It exists because the exact answer is not always the one on screen:
//! a meshed view is what a person is looking at, so picking has to agree with the picture. The two
//! are tested against each other on a body without holes.
//!
//! **They are not interchangeable.** A region with a hole is triangulated as its outer loop plus
//! *reversed* triangles laid over the hole — the kernel subtracts a hole by winding, not by
//! clipping the outer triangulation (see `mesh::triangulate_region`). A mesh hit test ignores
//! winding, so down the axis of a hole it reports the outer loop's triangles as surface.
//! [`ray_faces_intersection`] is the authoritative answer; the mesh path exists for flat bodies,
//! where the two provably agree.
//!
//! [`ray_solid_intersection`] is the default entry point and uses the exact faces.
//!
//! # What counts as no answer
//!
//! [`None`] is a real answer, and it is returned — never a NaN, never an infinity, never a point
//! that is not on the surface — for every case that has no single right answer:
//!
//! - A ray with no usable direction (zero length, or a non-finite component).
//! - A ray **parallel** to a face's plane: the crossing point is not defined, so the face is
//!   skipped rather than resolved at some arbitrary point. See [`PARALLEL_EPS`].
//! - A ray whose origin **lies in** a face's plane. It grazes that face: there is no crossing to
//!   find, so the face is refused and the ray goes on to whatever else it meets. This is
//!   deliberately the same for a ray that starts exactly on a surface and points into the solid —
//!   it is reported where it *leaves*, not where it started.
//! - A crossing point outside the face's region: the ray passes beside the face, not through it.
//! - A crossing behind the origin, beyond the solid's own tolerance.
//! - A tessellation the kernel refuses, which is reported as an `Err` by
//!   [`ray_tessellated_intersection`] and never silently turned into "no hit".
//!
//! # Ties and direction
//!
//! Two crossings at the same distance (a ray straight down a shared edge) keep the **lower face
//! index**, so a pick is reproducible rather than dependent on iteration luck. Back faces are not
//! culled: the entry face of a body seen from outside is the answer, and the exit face is only
//! reached by a ray that started inside.

use cadcraft_geom::{Bounds3, Vec3};
use cadcraft_solid::{Mesh, Solid, SolidError, Triangle, mesh_from_solid};

use crate::scene3::{Camera, Scene};

/// Smallest `|direction · normal|` treated as parallel, and therefore as not crossing.
///
/// A pure sine of the angle between the ray and the face normal, so it is dimensionless and does
/// not have to be rescaled with the model. One degree of graze (`sin 1° ≈ 0.017`) is far above it,
/// so this refuses only the degenerate band where the crossing point is numerically meaningless: a
/// ray parallel to a face to within floating-point noise.
pub const PARALLEL_EPS: f64 = 1e-9;

/// A ray: a point and a **unit** direction. Distances along it are in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    /// A ray along `dir`, normalised.
    ///
    /// `None` when `origin` is not finite or `dir` has no usable length: a ray that cannot be
    /// built is not a ray that silently goes nowhere.
    pub fn new(origin: Vec3, dir: Vec3) -> Option<Ray> {
        if !origin.is_finite() {
            return None;
        }
        let len = dir.len();
        if !len.is_finite() || len < PARALLEL_EPS {
            return None;
        }
        let unit = dir * (1.0 / len);
        unit.is_finite().then_some(Ray { origin, dir: unit })
    }

    /// This ray with a unit direction, or `None` when it has no usable one. Lets a [`Ray`] whose
    /// fields were written by hand still be intersected safely.
    pub fn unit(self) -> Option<Ray> {
        Ray::new(self.origin, self.dir)
    }

    /// The same line, pointing the other way.
    pub fn reversed(self) -> Option<Ray> {
        Ray::new(self.origin, self.dir * -1.0)
    }

    /// The point `t` along the ray. Only meaningful for finite `t`.
    pub fn at(self, t: f64) -> Vec3 {
        self.origin + self.dir * t
    }

    /// Every number usable.
    pub fn is_finite(self) -> bool {
        self.origin.is_finite() && self.dir.is_finite()
    }
}

/// Where a ray met a solid, and what it met.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Distance from [`Ray::origin`] along the unit direction. Never negative: a crossing that
    /// landed a hair behind the origin — within the solid's own tolerance — is reported as `0`.
    pub distance: f64,
    /// The crossing point, in world coordinates and on the face's surface.
    pub point: Vec3,
    /// The face's unit normal, pointing out of the material (the kernel's invariant I4).
    pub normal: Vec3,
    /// Index into [`Solid::faces`].
    pub face: usize,
    /// `true` when the answer came from the tessellation rather than from the exact face.
    pub tessellated: bool,
}

impl Hit {
    /// Every number usable — the check a caller makes before trusting a pick.
    pub fn is_finite(&self) -> bool {
        self.distance.is_finite() && self.point.is_finite() && self.normal.is_finite()
    }
}

/// The nearest exact hit of `ray` on `solid`, with the index of the face it met.
pub fn ray_faces_intersection(ray: &Ray, solid: &Solid) -> Option<Hit> {
    let ray = ray.unit()?;
    // A tolerance in world units that scales with the solid, not a magic constant: a millimetre
    // feature in a metre part and a millimetre feature in a micron part are different problems.
    let eps = solid.tolerance();
    let mut best: Option<Hit> = None;
    for (i, face) in solid.faces.iter().enumerate() {
        let n = face.plane.normal;
        let n_len = n.len();
        if !n_len.is_finite() || n_len < 0.5 || !face.plane.origin.is_finite() {
            continue;
        }
        let denom = ray.dir.dot(n);
        // Parallel: no crossing point exists. `<=`, not `<`, so a direction within noise of
        // parallel is refused too rather than landing a kilometre away.
        if denom.abs() <= PARALLEL_EPS * n_len {
            continue;
        }
        let signed = (face.plane.origin - ray.origin).dot(n);
        if !signed.is_finite() {
            continue;
        }
        // A graze: the origin already lies in the plane, so there is nothing to cross.
        if signed.abs() <= eps * n_len {
            continue;
        }
        let t = signed / denom;
        if !t.is_finite() || t < -eps {
            continue;
        }
        // Nearest wins; a tie keeps the lower face index, which makes a pick reproducible.
        if best.is_some_and(|b| b.distance <= t) {
            continue;
        }
        let point = ray.at(t);
        if !point.is_finite() || !face.contains_local(face.plane.to_local(point)) {
            continue;
        }
        best = Some(Hit { distance: t.max(0.0), point, normal: n * (1.0 / n_len), face: i, tessellated: false });
    }
    best
}

/// The nearest hit of `ray` on `solid`: the exact faces, which is what a pick should use.
pub fn ray_solid_intersection(ray: &Ray, solid: &Solid) -> Option<Hit> {
    ray_faces_intersection(ray, solid)
}

/// The nearest hit of `ray` on a tessellation, by Möller–Trumbore.
///
/// `solid` is the body `mesh` came from: it is what names the face. A crossing that is on no face
/// (a mesh that does not belong to the solid, or a point beyond the solid's tolerance) is reported
/// as no hit, because it is not on the surface.
///
/// See the module docs for why this is not the same answer as [`ray_faces_intersection`] on a body
/// with holes.
pub fn ray_mesh_intersection(ray: &Ray, mesh: &Mesh, solid: &Solid) -> Option<Hit> {
    let ray = ray.unit()?;
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for tri in mesh.triangles() {
        let Some((t, point, normal)) = ray_triangle(ray, &tri) else { continue };
        if best.is_some_and(|(bt, _, _)| bt <= t) {
            continue;
        }
        best = Some((t, point, normal));
    }
    let (distance, point, normal) = best?;
    if !distance.is_finite() || distance < 0.0 || !point.is_finite() || !normal.is_finite() {
        return None;
    }
    Some(Hit { distance, point, normal, face: face_at(solid, point)?, tessellated: true })
}

/// [`ray_mesh_intersection`] on a freshly tessellated solid.
///
/// `Err` when the kernel refuses the tessellation (a cap, or an empty shell): that is a real
/// failure of the *body*, and reporting it as "nothing was hit" would hide a broken model behind
/// a missing click.
pub fn ray_tessellated_intersection(ray: &Ray, solid: &Solid) -> Result<Option<Hit>, SolidError> {
    let mesh = mesh_from_solid(solid)?;
    Ok(ray_mesh_intersection(ray, &mesh, solid))
}

/// One triangle, no back-face culling: the entry face is the answer.
fn ray_triangle(ray: &Ray, tri: &Triangle) -> Option<(f64, Vec3, Vec3)> {
    let e1 = tri.b - tri.a;
    let e2 = tri.c - tri.a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    // Edge-on: `det` is the six times the triangle's projected area, so the scale-aware test is
    // the product of the two edge lengths rather than a bare number.
    let scale = e1.len() * p.len();
    if !det.is_finite() || !scale.is_finite() || det.abs() <= PARALLEL_EPS * scale {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - tri.a;
    let u = s.dot(p) * inv;
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if !u.is_finite() || !v.is_finite() || u < 0.0 || v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    if !t.is_finite() || t < 0.0 {
        return None;
    }
    let point = ray.at(t);
    if !point.is_finite() {
        return None;
    }
    let raw = e1.cross(e2);
    let len = raw.len();
    if !len.is_finite() || len <= 0.0 {
        return None;
    }
    Some((t, point, raw * (1.0 / len)))
}

/// Which face of `solid` the world point `p` lies on.
///
/// A tessellation of a planar face stays in that face's plane, so the point is on exactly one
/// face's surface. `None` is possible — a point past the kernel's tolerance, a shell whose faces
/// do not cover it, a mesh that did not come from this solid — and is reported as such rather than
/// guessed at.
pub fn face_at(solid: &Solid, p: Vec3) -> Option<usize> {
    if !p.is_finite() {
        return None;
    }
    let eps = solid.tolerance();
    for (i, face) in solid.faces.iter().enumerate() {
        let n = face.plane.normal;
        let n_len = n.len();
        if !n_len.is_finite() || n_len < 0.5 || face.plane.distance(p).abs() > eps * n_len {
            continue;
        }
        if face.contains_local(face.plane.to_local(p)) {
            return Some(i);
        }
    }
    None
}

/// The nearest solid of `scene` that `ray` meets, with its index in [`Scene::solids`].
pub fn pick_scene(ray: &Ray, scene: &Scene) -> Option<(usize, Hit)> {
    let mut best: Option<(usize, Hit)> = None;
    for (i, (solid, _)) in scene.solids.iter().enumerate() {
        let Some(hit) = ray_solid_intersection(ray, solid) else { continue };
        if best.as_ref().is_none_or(|(_, b)| hit.distance < b.distance) {
            best = Some((i, hit));
        }
    }
    best
}

/// The world ray through the pixel at (`px`, `py`) of a `width × height` image drawn by `camera`.
///
/// The pixel is the same one [`crate::scene3`] rasterises: origin at the **top left**, `x` right,
/// `y` down. The basis is read back out of [`Camera::view_matrix`] so this cannot drift from the
/// projection it has to agree with — there is a test that projects the answer back onto the pixel
/// it came from.
///
/// `None` for a zero-sized image, an unusable camera, or a pixel outside the image.
pub fn ray_through_pixel(camera: &Camera, px: f64, py: f64, width: f64, height: f64) -> Option<Ray> {
    if !(px.is_finite() && py.is_finite() && width.is_finite() && height.is_finite()) {
        return None;
    }
    if width <= 0.0 || height <= 0.0 || px < 0.0 || py < 0.0 || px > width || py > height {
        return None;
    }
    camera.validate().ok()?;
    let m = camera.view_matrix();
    let right = Vec3::new(m[0][0], m[0][1], m[0][2]);
    let up = Vec3::new(m[1][0], m[1][1], m[1][2]);
    let forward = Vec3::new(-m[2][0], -m[2][1], -m[2][2]);
    let tan_half = (camera.fov_degrees.to_radians() * 0.5).tan();
    if !(tan_half.is_finite() && tan_half > 0.0) {
        return None;
    }
    let ndc_x = 2.0 * (px / width) - 1.0;
    let ndc_y = 1.0 - 2.0 * (py / height);
    let dir = right * (ndc_x * tan_half * width / height) + up * (ndc_y * tan_half) + forward;
    Ray::new(camera.eye, dir)
}

/// [`pick_scene`] straight from a click: the pixel `(px, py)` of a `width × height` image drawn
/// by `camera`, and the solid and face it lands on.
pub fn pick_at(camera: &Camera, scene: &Scene, px: f64, py: f64, width: f64, height: f64) -> Option<(usize, Hit)> {
    let ray = ray_through_pixel(camera, px, py, width, height)?;
    pick_scene(&ray, scene)
}

/// Half the tangent of a camera's field of view: the slope of its view frustum at the target
/// distance, which is what a viewport needs to turn pixels into world units. `None` for a camera
/// that cannot see anything.
pub fn view_tangent(camera: &Camera) -> Option<f64> {
    camera.validate().ok()?;
    let tan_half = (camera.fov_degrees.to_radians() * 0.5).tan();
    (tan_half.is_finite() && tan_half > 0.0).then_some(tan_half)
}

/// The diagonal of `bounds`, or `None` for an empty or non-finite one.
pub fn bounds_diagonal(bounds: &Bounds3) -> Option<f64> {
    if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        return None;
    }
    let d = bounds.diagonal();
    (d.is_finite() && d > 0.0).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_solid::{Plane, support};

    /// Panic with a message rather than a bare `Err`, as the kernel's own tests do.
    fn must<T>(r: Result<T, SolidError>, what: &str) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{what}: {e}"),
        }
    }

    /// A box spanning `min` to `max`.
    fn box_between(min: Vec3, max: Vec3) -> Solid {
        must(support::box_at(min, max), "box_at")
    }

    /// The unit cube, centred on the origin.
    fn unit_box() -> Solid {
        box_between(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0))
    }

    fn mesh_of(solid: &Solid) -> Mesh {
        must(mesh_from_solid(solid), "mesh_from_solid")
    }

    /// A ray, refusing to build one that should not exist.
    fn ray(origin: Vec3, dir: Vec3) -> Ray {
        match Ray::new(origin, dir) {
            Some(r) => r,
            None => panic!("a ray along {dir:?} should exist"),
        }
    }

    /// The hit a test insists on, or a panic saying what was missing.
    fn hit(ray: &Ray, solid: &Solid) -> Hit {
        match ray_solid_intersection(ray, solid) {
            Some(h) => h,
            None => panic!("{ray:?} should meet the solid"),
        }
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// The material the scene tests push: a mid grey, spelled out so a change to the crate's
    /// default cannot quietly change what is being tested.
    fn grey() -> crate::scene3::Material {
        crate::scene3::Material::new(crate::scene3::Rgba::new(0.7, 0.7, 0.72))
    }

    #[test]
    fn a_ray_into_a_box_hits_the_expected_face_at_the_expected_distance() {
        let box_ = unit_box();
        // Straight at the -X face from 3 units away, so the crossing is exactly 2.
        let r = ray(Vec3::new(-3.0, 0.0, 0.0), Vec3::X);
        let h = hit(&r, &box_);
        assert!(near(h.distance, 2.0, 1e-12), "distance {}", h.distance);
        assert!(h.point.near(Vec3::new(-1.0, 0.0, 0.0), 1e-12), "point {:?}", h.point);
        assert!(h.normal.near(Vec3::new(-1.0, 0.0, 0.0), 1e-12), "normal {:?}", h.normal);
        assert!(!h.tessellated);
        assert!(h.is_finite());
        // The face index names a face that really is this one.
        let face = box_.faces.get(h.face).expect("the hit names a face of the solid");
        assert!(face.plane.normal.near(h.normal, 1e-12), "face {} has normal {:?}", h.face, face.plane.normal);
        assert!(near(face.plane.distance(h.point), 0.0, 1e-12));
        assert!(face.contains_local(face.plane.to_local(h.point)));
    }

    #[test]
    fn the_nearest_face_wins_from_outside() {
        let box_ = unit_box();
        // Aimed at a corner: both the -X and the -Y face are candidates at the same distance.
        let r = ray(Vec3::new(-5.0, -5.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        let h = hit(&r, &box_);
        assert!(near(h.distance, 4.0 * std::f64::consts::SQRT_2, 1e-12), "distance {}", h.distance);
        assert!(near(h.point.x, -1.0, 1e-12) && near(h.point.y, -1.0, 1e-12), "point {:?}", h.point);
    }

    #[test]
    fn a_ray_that_misses_returns_none() {
        let box_ = unit_box();
        // Above the top face and running past the side.
        assert!(ray_solid_intersection(&ray(Vec3::new(-5.0, 0.0, 5.0), Vec3::X), &box_).is_none(), "above the top face");
        assert!(ray_solid_intersection(&ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::Y), &box_).is_none(), "past the side");
        // Pointing away from it.
        assert!(ray_solid_intersection(&ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)), &box_).is_none());
        // In the +X face's plane, but beyond its edge: it crosses the plane beside the face.
        assert!(ray_solid_intersection(&ray(Vec3::new(-5.0, 3.0, 0.0), Vec3::X), &box_).is_none(), "beside the face");
    }

    #[test]
    fn a_ray_in_a_face_plane_is_grazing_and_refused() {
        let box_ = unit_box();
        // Travels in the plane of the top face (z = 1), straight over the box.
        let r = ray(Vec3::new(-5.0, 0.0, 1.0), Vec3::X);
        assert!(ray_solid_intersection(&r, &box_).is_none(), "a graze is not a hit");
        // The same line tilted a hair down crosses the side face, so the refusal above was the
        // grazing rule and not a plain miss.
        let tilted = ray(Vec3::new(-5.0, 0.0, 1.0), Vec3::new(1.0, 0.0, -1e-4));
        let h = hit(&tilted, &box_);
        assert!(near(h.distance, 4.0, 1e-3), "distance {}", h.distance);
        assert!(near(h.point.z, 1.0 - 4e-4, 1e-6), "point {:?}", h.point);
    }

    #[test]
    fn a_ray_that_starts_in_a_face_plane_does_not_meet_that_face() {
        let box_ = unit_box();
        // The origin sits exactly on the -X face and the ray crosses the solid diagonally.
        let r = ray(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        let h = hit(&r, &box_);
        assert!(h.normal.near(Vec3::Y, 1e-12), "normal {:?}: the face it started on is refused", h.normal);
        assert!(near(h.distance, std::f64::consts::SQRT_2, 1e-12), "distance {}", h.distance);
        assert!(h.point.near(Vec3::new(0.0, 1.0, 0.0), 1e-12), "point {:?}", h.point);
    }

    #[test]
    fn a_ray_starting_inside_returns_the_exit_point() {
        let box_ = unit_box();
        let r = ray(Vec3::new(0.25, -0.5, 0.0), Vec3::X);
        let h = hit(&r, &box_);
        // The entry face is behind the origin, so the answer is the way out.
        assert!(near(h.distance, 0.75, 1e-12), "distance {}", h.distance);
        assert!(h.point.near(Vec3::new(1.0, -0.5, 0.0), 1e-12), "point {:?}", h.point);
        assert!(h.normal.near(Vec3::X, 1e-12));
    }

    #[test]
    fn a_degenerate_ray_is_refused_rather_than_producing_a_nan() {
        let box_ = unit_box();
        let mesh = mesh_of(&box_);
        assert!(Ray::new(Vec3::ZERO, Vec3::ZERO).is_none(), "no direction");
        assert!(Ray::new(Vec3::ZERO, Vec3::new(1e-300, 0.0, 0.0)).is_none(), "no usable length");
        assert!(Ray::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X).is_none(), "non-finite origin");
        assert!(Ray::new(Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 0.0)).is_none(), "non-finite direction");
        // Hand-written fields are brought back to a unit direction or refused, never trusted.
        let raw = Ray { origin: Vec3::new(-5.0, 0.0, 0.0), dir: Vec3::new(3.0, 0.0, 0.0) };
        assert!(near(hit(&raw, &box_).distance, 4.0, 1e-12), "a non-unit ray still measures in world units");
        assert!(Ray { origin: Vec3::ZERO, dir: Vec3::ZERO }.unit().is_none());
        // And nothing non-finite ever escapes either path.
        for r in [Ray { origin: Vec3::ZERO, dir: Vec3::ZERO }, Ray { origin: Vec3::new(f64::NAN, 0.0, 0.0), dir: Vec3::X }] {
            assert!(ray_solid_intersection(&r, &box_).is_none_or(Hit::is_finite), "{r:?}");
            assert!(ray_mesh_intersection(&r, &mesh, &box_).is_none_or(Hit::is_finite), "{r:?}");
        }
    }

    #[test]
    fn the_exact_and_tessellated_answers_agree_on_a_box() {
        let box_ = unit_box();
        let mesh = mesh_of(&box_);
        let mut compared = 0usize;
        for dir in [Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.2, -0.7, 0.5), Vec3::new(-1.0, 0.0, 0.0), Vec3::new(-0.3, 0.9, 0.31)] {
            let origin = dir * -4.0 + Vec3::new(0.1, 0.2, -0.3);
            let r = ray(origin, dir);
            let Some(exact) = ray_solid_intersection(&r, &box_) else { continue };
            // The mesh path can legitimately answer nothing where the crossing lands exactly on
            // the triangulation's shared diagonal: that is a graze of two triangles at once.
            let Some(meshed) = ray_mesh_intersection(&r, &mesh, &box_) else { continue };
            compared += 1;
            assert!(meshed.tessellated);
            assert_eq!(meshed.face, exact.face, "face {} vs {} for {dir:?}", meshed.face, exact.face);
            assert!(near(meshed.distance, exact.distance, 1e-12), "distance {} vs {} for {dir:?}", meshed.distance, exact.distance);
            assert!(meshed.point.near(exact.point, 1e-9), "point {:?} vs {:?}", meshed.point, exact.point);
            assert!(meshed.normal.near(exact.normal, 1e-9));
        }
        assert!(compared >= 3, "only {compared} of the directions could be compared");
    }

    #[test]
    fn the_tessellated_path_reports_the_kernels_refusal_rather_than_no_hit() {
        // An empty shell has no tessellation; that is an `Err`, not "nothing was clicked".
        let empty = Solid::default();
        let r = ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::X);
        assert!(ray_solid_intersection(&r, &empty).is_none());
        assert!(ray_tessellated_intersection(&r, &empty).is_err(), "an untessellatable shell must not read as a miss");
    }

    #[test]
    fn a_hole_is_not_a_surface_and_a_face_around_it_is() {
        // A square ring with a square hole through it, from z = 0 to z = 2.
        let profile = support::rect_with_hole(-1.0, -1.0, 1.0, 1.0, -0.25, -0.25, 0.25, 0.25);
        let ring = must(cadcraft_solid::extrude(&profile, &Plane::xy(0.0), 2.0), "extrude");
        let down = ray(Vec3::new(0.0, 0.0, -5.0), Vec3::Z);
        assert!(ray_solid_intersection(&down, &ring).is_none(), "straight down the hole");
        assert!(face_at(&ring, Vec3::ZERO).is_none(), "the middle of the ring is not on a face");
        // Beside the hole it does hit.
        let h = hit(&ray(Vec3::new(0.5, 0.0, -5.0), Vec3::Z), &ring);
        assert!(near(h.distance, 5.0, 1e-12), "distance {}", h.distance);
        assert!(h.point.near(Vec3::new(0.5, 0.0, 0.0), 1e-12), "point {:?}", h.point);
    }

    #[test]
    fn the_nearest_of_several_solids_is_picked() {
        let camera = Camera { eye: Vec3::new(0.0, -10.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, ..Camera::default() };
        let mut scene = Scene::new(camera);
        scene.push(box_between(Vec3::new(-1.0, -2.0, -1.0), Vec3::new(1.0, -1.0, 1.0)), grey());
        scene.push(box_between(Vec3::new(-1.0, 2.0, -1.0), Vec3::new(1.0, 3.0, 1.0)), grey());
        let centre = ray(Vec3::new(0.0, -10.0, 0.0), Vec3::Y);
        let (i, h) = pick_scene(&centre, &scene).expect("the centre ray meets both");
        assert_eq!(i, 0, "the nearer body is picked");
        assert!(near(h.distance, 8.0, 1e-12), "distance {}", h.distance);
        // From the far side, the other body is nearest.
        let back = ray(Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(pick_scene(&back, &scene).map(|(i, _)| i), Some(1));
        // A ray that passes beside both meets neither.
        let gap = ray(Vec3::new(5.0, 0.0, 0.0), Vec3::X);
        assert!(pick_scene(&gap, &scene).is_none());
    }

    #[test]
    fn a_click_lands_on_the_body_the_picture_shows() {
        let camera = Camera { eye: Vec3::new(0.0, -10.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 45.0, near: 0.5, far: 200.0 };
        let mut scene = Scene::new(camera);
        scene.push(box_between(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)), grey());
        // 640 x 480: the centre pixel is the centre of the view, and the -Y face is what is there.
        let (i, h) = pick_at(&camera, &scene, 320.0, 240.0, 640.0, 480.0).expect("the centre pixel hits the box");
        assert_eq!(i, 0);
        assert!(h.normal.near(Vec3::new(0.0, -1.0, 0.0), 1e-9), "normal {:?}", h.normal);
        assert!(near(h.distance, 9.0, 1e-9), "distance {}", h.distance);
        // A corner of the image is empty space.
        assert!(pick_at(&camera, &scene, 2.0, 2.0, 640.0, 480.0).is_none());
        // And a degenerate request is refused.
        assert!(pick_at(&camera, &scene, 320.0, 240.0, 0.0, 480.0).is_none(), "a zero width");
        assert!(pick_at(&camera, &scene, f64::NAN, 240.0, 640.0, 480.0).is_none(), "a non-finite pixel");
        assert!(ray_through_pixel(&camera, -1.0, 240.0, 640.0, 480.0).is_none(), "left of the image");
        assert!(ray_through_pixel(&camera, 641.0, 240.0, 640.0, 480.0).is_none(), "right of the image");
        assert!(ray_through_pixel(&camera, 320.0, 481.0, 640.0, 480.0).is_none(), "below the image");
    }

    #[test]
    fn a_pixel_round_trips_through_the_projection() {
        // Whatever `scene3` draws, `ray_through_pixel` has to send the ray back at: projecting the
        // crossing point with the public matrices must land on the pixel that was clicked.
        let camera = Camera { eye: Vec3::new(4.0, -6.0, 4.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 45.0, near: 0.5, far: 200.0 };
        let (w, h) = (640.0f64, 480.0f64);
        let m = camera.view_matrix();
        let pr = camera.projection(w / h);
        let mut checked = 0usize;
        for (px, py) in [(320.0, 240.0), (120.0, 90.0), (500.0, 400.0), (10.5, 470.25), (200.25, 150.75)] {
            let Some(r) = ray_through_pixel(&camera, px, py, w, h) else { continue };
            let Some(crossing) = ray_solid_intersection(&r, &unit_box()) else { continue };
            let v = [
                m[0][0] * crossing.point.x + m[0][1] * crossing.point.y + m[0][2] * crossing.point.z + m[0][3],
                m[1][0] * crossing.point.x + m[1][1] * crossing.point.y + m[1][2] * crossing.point.z + m[1][3],
                m[2][0] * crossing.point.x + m[2][1] * crossing.point.y + m[2][2] * crossing.point.z + m[2][3],
            ];
            let depth = -v[2];
            let (bx, by) = (pr[0][0] * v[0] / depth, pr[1][1] * v[1] / depth);
            assert!(near(bx, px, 1e-9) && near(by, py, 1e-9), "({px}, {py}) came back at ({bx}, {by})");
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} pixels could be checked");
    }

    #[test]
    fn bounds_and_camera_helpers_refuse_nothing_sensible() {
        assert_eq!(bounds_diagonal(&Bounds3::EMPTY), None);
        assert_eq!(bounds_diagonal(&Bounds3::new(Vec3::ZERO, Vec3::ZERO)), None, "a point has no size");
        assert_eq!(bounds_diagonal(&Bounds3::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X)), None);
        assert!(near(bounds_diagonal(&Bounds3::new(Vec3::ZERO, Vec3::new(3.0, 4.0, 0.0))).expect("a diagonal"), 5.0, 1e-12));
        assert!(view_tangent(&Camera::default()).is_some());
        assert!(view_tangent(&Camera { fov_degrees: 0.0, ..Camera::default() }).is_none());
        assert!(view_tangent(&Camera { fov_degrees: 200.0, ..Camera::default() }).is_none());
        assert!(view_tangent(&Camera { eye: Vec3::ZERO, target: Vec3::ZERO, ..Camera::default() }).is_none(), "no view direction");
    }

    #[test]
    fn tolerance_scales_with_the_body() {
        // A part a kilometre across must not be judged by the tolerance of a part a micron across.
        let big = box_between(Vec3::new(-5e5, -5e5, -5e5), Vec3::new(5e5, 5e5, 5e5));
        let h = hit(&ray(Vec3::new(-5e5 - 100.0, 0.0, 0.0), Vec3::X), &big);
        assert!(near(h.distance, 100.0, 1e-6), "distance {}", h.distance);
        let small = box_between(Vec3::new(-1e-5, -1e-5, -1e-5), Vec3::new(1e-5, 1e-5, 1e-5));
        let h = hit(&ray(Vec3::new(-2e-5, 0.0, 0.0), Vec3::X), &small);
        assert!(near(h.distance, 1e-5, 1e-12), "distance {}", h.distance);
    }

    #[test]
    fn a_reversed_ray_meets_the_far_face() {
        let box_ = unit_box();
        let out = ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::X);
        let back = out.reversed().expect("a reversed ray");
        assert!(near(back.dir.dot(out.dir), -1.0, 1e-15));
        // From outside on the far side, the reversed ray starts behind the box and meets nothing.
        assert!(ray_solid_intersection(&back, &box_).is_none());
        // From inside, it leaves through the other face.
        let h = hit(&ray(Vec3::ZERO, Vec3::X).reversed().as_ref().expect("reversed"), &box_);
        assert!(h.point.near(Vec3::new(-1.0, 0.0, 0.0), 1e-12), "point {:?}", h.point);
    }

    #[test]
    fn face_at_names_the_face_a_point_is_on_and_nothing_else() {
        let box_ = unit_box();
        assert!(face_at(&box_, Vec3::new(-1.0, 0.0, 0.0)).is_some(), "on a face");
        assert_eq!(face_at(&box_, Vec3::ZERO), None, "in the material");
        assert_eq!(face_at(&box_, Vec3::new(2.0, 0.0, 0.0)), None, "outside the body");
        assert_eq!(face_at(&box_, Vec3::new(f64::NAN, 0.0, 0.0)), None, "not a point");
        assert_eq!(face_at(&Solid::default(), Vec3::ZERO), None, "an empty shell has no faces");
    }
}