//! A 3D scene and a CPU rasteriser that draws it into an RGBA8 image.
//!
//! # What this is, and what it is not
//!
//! This is a **validation renderer**: you point a [`Camera`] at a handful of [`Solid`]s and get
//! back a picture to *look at* while checking geometry — a silhouette that should be closed, a
//! face normal that should point out and not in, a tessellation that came out wrong, a chamfer
//! that ate the wrong edge. It is the thing you attach to a test that says "the result of this
//! boolean is not obviously broken".
//!
//! It is **not** an interactive 3D viewport, and it does not pretend to be one. Specifically, it
//! does **not** do, and its docs never claim:
//!
//! - **Shadows.** One directional light, no shadow map, no occlusion, no ambient term that knows
//!   about geometry. A surface lit only by [`RenderOptions::ambient`] is the model being dark,
//!   not a shadow.
//! - **PBR.** [`Material`] carries a base colour and two knobs that are *not* a microfacet model:
//!   see [`Material::shade`] for the entire shading equation, which is one `max(dot(n, l), 0)`.
//! - **Transparency sorting.** Alpha is written, never blended. Nothing is sorted, so nothing
//!   translucent is drawn correctly.
//! - **Culling acceleration.** Every triangle is transformed, clipped and scanned. There is no
//!   bounding-volume hierarchy, no frustum culling of whole solids, no tiles, no threading. It is
//!   O(triangles + covered pixels) and it is slow.
//! - **Quality anti-aliasing.** [`RenderOptions::supersample`] is a whole-pixel box filter, off by
//!   default. Edges are hard-edged at factor 1.
//! - **Performance.** Nothing here is written for speed. The caps below are there so a hostile
//!   input cannot make it allocate without bound, not because it needs them.
//!
//! What it would take to be a viewport: a GPU path (or at least a tiled, SIMD, multi-threaded CPU
//! one), a bounding-volume hierarchy or at least per-solid frustum culling, a real BRDF, shadow
//! maps, a depth prepass, MSAA, a proper transparency pass, and incremental redraw on a moved
//! camera. That is a different crate.
//!
//! # Coordinates and conventions
//!
//! - **World** is right-handed, the solid kernel's own world, unmodified.
//! - **View space** is right-handed, the camera at the origin looking down **−Z**, `+X` right and
//!   `+Y` up. [`Camera::view_matrix`] builds it.
//! - **Clip space** is the OpenGL convention: `−w ≤ x ≤ w`, `−w ≤ y ≤ w`, `0 ≤ z ≤ w`, so
//!   **normalised device depth runs 0 (the near plane) to 1 (the far plane)**, not −1 to 1.
//!   [`Camera::projection`] is the matching matrix.
//! - **Image space** has `x` to the right and `y` **down**, row 0 at the top, which is what an
//!   RGBA8 buffer is.
//! - **Winding.** The kernel's convention (its invariant I4) is that outer loops run
//!   counter-clockwise seen from outside the material and face normals point out of it. Such a
//!   triangle therefore lands **clockwise in image space**, because the projection flips `y` and
//!   flips handedness with it. [`RenderOptions::cull_back_faces`] keeps the triangles whose signed
//!   image-space area is **negative**; see [`render`].
//! - **Matrices** are `[[f64; 4]; 4]` in row-major order and act on a column vector:
//!   `p' = Σ_i m[row][i] · p[i]`.
//!
//! # Shading and gamma, precisely
//!
//! For a triangle whose world normal is `n` (unit, from the kernel's winding) and light direction
//! `l` (the direction from the surface **towards** the light, unit):
//!
//! ```text
//! lambert = max(dot(n, l), 0)
//! rgb     = base.rgb × (ambient + diffuse × (1 − metallic) × lambert)
//! ```
//!
//! clamped to `0..=1`, gamma-encoded with the sRGB transfer function, and rounded to 8 bits —
//! so a colour picked as bytes comes back out as the bytes that picked it (see
//! [`encode_channel`] and [`Rgba::from_srgb8`]).
//!
//! [`Rgba`] components are **linear** light in `0..=1`, not the bytes you would type into a colour
//! picker; [`Rgba::from_srgb8`] converts one. When [`RenderOptions::supersample`] is on, each
//! sample is encoded as it is written and the block is then averaged in the encoded domain, which
//! is marginally darker than averaging linear light would be. Neither choice is approximate about
//! what it does; both are written down here.
//!
//! # Determinism
//!
//! Two renders of the same scene in the same process are byte-identical, and a test asserts it.
//! There is no `HashMap` iteration, no thread, no ray marching and no accumulation of a sum whose
//! order depends on anything but index order. Two libm calls happen: `tan` of half the field of
//! view, once per camera, and `powf` per encoded pixel of the sRGB curve. Both come from the
//! platform's libm, so the output is identical for a given build (that is the guarantee, and the
//! test) but the last bit of a pixel is not promised to agree across different libm
//! implementations.
//!
//! # Caps
//!
//! Hostile input is an [`Err`] or a skipped pixel, never a panic: the crate denies
//! `clippy::unwrap_used`, `clippy::expect_used` and `clippy::panic` and forbids `unsafe`.
//!
//! - [`MAX_IMAGE_PIXELS`] — the output image, `width × height`.
//! - [`MAX_RASTER_PIXELS`] — the supersampled buffer, which is `supersample²` times bigger.
//! - [`MAX_TRIANGLES`] — triangles from all the solids of one scene, counted as they are drawn;
//!   past the limit the render is refused rather than truncated to a partial picture.
//! - [`MAX_SUPERSAMPLE`] — the largest integer factor accepted.
//! - [`MAX_GRID_LINES`] — grid lines per axis of one [`Workplane`].
//!
//! [`MAX_IMAGE_PIXELS`] and [`MAX_RASTER_PIXELS`] are 2048 × 2048 each, so a full-size image is
//! the only one that can be drawn without supersampling. A raster pixel costs 5 bytes (RGBA8 plus
//! an `f32` depth).

use cadcraft_geom::{Bounds3, Vec2, Vec3};
use cadcraft_solid::{Solid, mesh_from_solid};

/// Largest output image, in pixels (2048 × 2048).
pub const MAX_IMAGE_PIXELS: usize = 4_194_304;

/// Largest supersampled raster buffer, in pixels (2048 × 2048, `supersample²` times the output).
pub const MAX_RASTER_PIXELS: usize = 4_194_304;

/// Triangles from every solid of one scene, over which a render is refused rather than truncated.
pub const MAX_TRIANGLES: usize = 1_000_000;

/// Largest integer supersampling factor per axis.
pub const MAX_SUPERSAMPLE: u32 = 3;

/// Grid lines per axis of one [`Workplane`], over which the grid is clipped to what fits.
pub const MAX_GRID_LINES: usize = 512;

/// Ambient term of the default [`RenderOptions`].
pub const DEFAULT_AMBIENT: f64 = 0.15;

/// Diffuse weight of the default [`RenderOptions`].
pub const DEFAULT_DIFFUSE: f64 = 0.85;

/// Why a scene could not be drawn.
///
/// Every variant is a refusal, never a panic and never a silently wrong image.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneError {
    /// A side of the image was zero.
    EmptyImage,
    /// The image is larger than [`MAX_IMAGE_PIXELS`].
    ImageTooLarge { pixels: usize, max: usize },
    /// The supersampled buffer is larger than [`MAX_RASTER_PIXELS`].
    RasterTooLarge { pixels: usize, max: usize },
    /// A field of the camera cannot be used; the message says which.
    BadCamera(&'static str),
    /// A field of the workplane cannot be used; the message says which.
    BadWorkplane(&'static str),
    /// A number that has to be finite was not; the message says which.
    NonFinite(&'static str),
    /// More triangles than [`MAX_TRIANGLES`].
    TooManyTriangles { count: usize, max: usize },
    /// The solid kernel refused to tessellate one of the solids.
    Solid(cadcraft_solid::SolidError),
    /// The image could not be encoded (only from [`Image::encode_png`]).
    Encode(String),
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::EmptyImage => write!(f, "the image has a zero side"),
            SceneError::ImageTooLarge { pixels, max } => write!(f, "the image is {pixels} pixels, the limit is {max}"),
            SceneError::RasterTooLarge { pixels, max } => write!(f, "the supersampled buffer is {pixels} pixels, the limit is {max}"),
            SceneError::BadCamera(w) => write!(f, "unusable camera: {w}"),
            SceneError::BadWorkplane(w) => write!(f, "unusable workplane: {w}"),
            SceneError::NonFinite(w) => write!(f, "not a finite number: {w}"),
            SceneError::TooManyTriangles { count, max } => write!(f, "{count} triangles, the limit is {max}"),
            SceneError::Solid(e) => write!(f, "the solid kernel refused: {e}"),
            SceneError::Encode(w) => write!(f, "the image could not be encoded: {w}"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<cadcraft_solid::SolidError> for SceneError {
    fn from(e: cadcraft_solid::SolidError) -> SceneError {
        SceneError::Solid(e)
    }
}

/// A linear RGBA colour, each component in `0..=1`.
///
/// **Linear, not the bytes you would type.** `Rgba { r: 0.5, .. }` is half the light, and the
/// rasteriser's gamma encode turns it into roughly `0.73` in the image — see the module docs. Use
/// [`Rgba::from_srgb8`] for a colour picked as bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    /// An opaque colour.
    pub const fn new(r: f64, g: f64, b: f64) -> Rgba {
        Rgba { r, g, b, a: 1.0 }
    }

    /// A colour with an alpha, which this renderer writes but never blends.
    pub const fn with_alpha(r: f64, g: f64, b: f64, a: f64) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// Opaque black.
    pub const BLACK: Rgba = Rgba::new(0.0, 0.0, 0.0);

    /// An sRGB byte triple decoded to linear light, alpha 1.
    pub fn from_srgb8(r: u8, g: u8, b: u8) -> Rgba {
        let d = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        Rgba::new(d(r), d(g), d(b))
    }

    /// Every component finite.
    pub fn is_finite(&self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite() && self.a.is_finite()
    }

    /// The eight bits this colour writes: gamma-encoded `rgb`, plain `a`.
    pub fn to_u8(self) -> [u8; 4] {
        [encode_channel(self.r), encode_channel(self.g), encode_channel(self.b), encode_byte(self.a)]
    }
}

/// Gamma-encode one linear channel: the sRGB transfer function (IEC 61966-2-1) — a `12.92·c`
/// linear segment below `0.0031308` and `1.055·c^(1/2.4) − 0.055` above it — rounded and clamped
/// to a byte.
///
/// The piecewise form, rather than a bare `c^(1/2.4)`, is what makes [`Rgba::from_srgb8`] a
/// lossless round trip, so a colour chosen as bytes comes out as the bytes that chose it.
///
/// Non-finite input encodes as 0 rather than propagating: a hostile colour must not be able to
/// write a NaN into the buffer.
pub fn encode_channel(linear: f64) -> u8 {
    if !linear.is_finite() {
        return 0;
    }
    let c = linear.clamp(0.0, 1.0);
    let e = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    encode_byte(e)
}

/// Round a `0..=1` value to a byte, without any transfer function.
fn encode_byte(v: f64) -> u8 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    u8::try_from((v * 255.0 + 0.5) as u32).unwrap_or(u8::MAX)
}

/// How a solid's surface is coloured.
///
/// This is not a physically based description and does not pretend to be: [`render`] reads
/// [`Material::base`] and [`Material::metallic`] and **ignores [`Material::roughness`]**, which is
/// carried so a caller can describe a material without the type changing under it. The whole
/// shading model is [`Material::shade`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// Linear base colour.
    pub base: Rgba,
    /// Carried, not read by this renderer.
    pub roughness: f64,
    /// 0 = dielectric (full diffuse), 1 = metal (diffuse switched off). Clamped to `0..=1`.
    pub metallic: f64,
}

impl Default for Material {
    fn default() -> Material {
        Material { base: Rgba::new(0.7, 0.7, 0.72), roughness: 0.6, metallic: 0.0 }
    }
}

impl Material {
    /// A dielectric material of base colour `base`.
    pub fn new(base: Rgba) -> Material {
        Material { base, ..Material::default() }
    }

    /// Every number usable (a non-finite colour renders black, it is not an error).
    pub fn is_finite(&self) -> bool {
        self.base.is_finite() && self.roughness.is_finite() && self.metallic.is_finite()
    }

    /// The shaded linear colour of a surface with unit normal `n` under a light in direction
    /// `l` (from the surface **towards** the light).
    ///
    /// `rgb = base.rgb × (ambient + diffuse × (1 − metallic) × max(dot(n, l), 0))`, clamped to
    /// `0..=1`; the alpha is the base alpha, unlit and unblended. A non-finite input anywhere
    /// gives opaque black rather than a NaN.
    pub fn shade(&self, n: Vec3, l: Vec3, ambient: f64, diffuse: f64) -> Rgba {
        if !self.is_finite() || !n.is_finite() || !l.is_finite() || !ambient.is_finite() || !diffuse.is_finite() {
            return Rgba::new(0.0, 0.0, 0.0);
        }
        let n = n.normalized();
        let l = l.normalized();
        if n.len() < 0.5 || l.len() < 0.5 {
            return Rgba::new(0.0, 0.0, 0.0);
        }
        let metallic = self.metallic.clamp(0.0, 1.0);
        let lambert = n.dot(l).max(0.0);
        let gain = (ambient + diffuse * (1.0 - metallic) * lambert).max(0.0);
        Rgba::with_alpha(self.base.r * gain, self.base.g * gain, self.base.b * gain, self.base.a)
    }
}

/// A pinhole camera.
///
/// Right-handed, looking down −Z in view space, OpenGL-style clip space (device depth 0 at
/// [`Camera::near`], 1 at [`Camera::far`]). See the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    /// What the camera looks at. The view direction is `target − eye`.
    pub target: Vec3,
    /// The up hint; orthogonalised against the view direction.
    pub up: Vec3,
    /// Vertical field of view, in degrees, strictly between 0 and 180.
    pub fov_degrees: f64,
    /// Distance to the near plane, positive.
    pub near: f64,
    /// Distance to the far plane, greater than `near`.
    pub far: f64,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera { eye: Vec3::new(4.0, -6.0, 4.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 45.0, near: 0.1, far: 1000.0 }
    }
}

impl Camera {
    /// The view matrix, row-major, for a column vector: `p' = Σ m[row][i] · p[i]`.
    ///
    /// Total by design: an unusable camera (non-finite, zero-length view direction, an `up`
    /// parallel to it) gives the **identity**, which draws nothing and never returns a NaN.
    /// [`render`] calls [`Camera::validate`] first and returns [`SceneError::BadCamera`] for such
    /// a camera, so this is a library convenience rather than a path anything relies on.
    pub fn view_matrix(&self) -> [[f64; 4]; 4] {
        let Some((r, u, b)) = self.basis() else { return identity() };
        [[r.x, r.y, r.z, -r.dot(self.eye)], [u.x, u.y, u.z, -u.dot(self.eye)], [b.x, b.y, b.z, -b.dot(self.eye)], [0.0, 0.0, 0.0, 1.0]]
    }

    /// The perspective projection for an image of `aspect` = width / height, row-major, for a
    /// column vector, with device depth 0 at [`Camera::near`] and 1 at [`Camera::far`].
    ///
    /// As with [`Camera::view_matrix`], an unusable camera or aspect gives all zeros, which makes
    /// every vertex project to `(0, 0, 0)` and draw nothing rather than panicking.
    pub fn projection(&self, aspect: f64) -> [[f64; 4]; 4] {
        let Some(f) = self.focal() else { return [[0.0; 4]; 4] };
        if !aspect.is_finite() || aspect <= 0.0 {
            return [[0.0; 4]; 4];
        }
        let (n, far) = (self.near, self.far);
        [
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            // The OpenGL depth map: 0 at the near plane, 1 at the far one. (The D3D/Vulkan
            // variant maps −1..1 instead, and would make every depth here land outside the buffer.)
            [0.0, 0.0, -far / (far - n), -(far * n) / (far - n)],
            [0.0, 0.0, -1.0, 0.0],
        ]
    }

    /// Whether [`render`] will accept this camera.
    pub fn validate(&self) -> Result<(), SceneError> {
        for (name, v) in [("eye", self.eye), ("target", self.target), ("up", self.up)] {
            if !v.is_finite() {
                return Err(SceneError::NonFinite(name));
            }
        }
        for (name, v) in [("fov_degrees", self.fov_degrees), ("near", self.near), ("far", self.far)] {
            if !v.is_finite() {
                return Err(SceneError::NonFinite(name));
            }
        }
        if self.fov_degrees <= 0.0 || self.fov_degrees >= 180.0 {
            return Err(SceneError::BadCamera("fov_degrees is not in 0..180"));
        }
        if self.near <= 0.0 {
            return Err(SceneError::BadCamera("near is not positive"));
        }
        if self.far <= self.near {
            return Err(SceneError::BadCamera("far is not beyond near"));
        }
        if self.basis().is_none() {
            return Err(SceneError::BadCamera("up is parallel to the view direction, or it has no length"));
        }
        Ok(())
    }

    /// [`Camera::validate`] as a bool.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// The view basis: right, up and back (−forward) axes, all unit and mutually orthogonal.
    fn basis(&self) -> Option<(Vec3, Vec3, Vec3)> {
        let f = self.target - self.eye;
        if !f.is_finite() {
            return None;
        }
        let f = f.normalized();
        if f.len() < 0.5 {
            return None;
        }
        let r = f.cross(self.up).normalized();
        if r.len() < 0.5 {
            return None;
        }
        let u = r.cross(f).normalized();
        if u.len() < 0.5 {
            return None;
        }
        Some((r, u, -f))
    }

    /// `1 / tan(fov / 2)`, the vertical focal length in clip units.
    fn focal(&self) -> Option<f64> {
        if !self.is_valid() {
            return None;
        }
        let f = (self.fov_degrees.to_radians() * 0.5).tan();
        if f.is_finite() && f > 0.0 { Some(1.0 / f) } else { None }
    }
}

fn identity() -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// A straight line segment in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: Vec3,
    pub b: Vec3,
}

impl Segment {
    pub fn new(a: Vec3, b: Vec3) -> Segment {
        Segment { a, b }
    }
    pub fn midpoint(&self) -> Vec3 {
        (self.a + self.b) * 0.5
    }
    pub fn length(&self) -> f64 {
        (self.b - self.a).len()
    }
    pub fn is_finite(&self) -> bool {
        self.a.is_finite() && self.b.is_finite()
    }
}

/// The drafting grid a sketch sits on: a plane, an in-plane frame, a spacing and an extent.
///
/// It draws the plane a profile was sketched in, so a person looking at a screenshot can tell
/// which way is up and where the sketch plane actually is. It is not a snapping aid and knows
/// nothing about the sketch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Workplane {
    pub origin: Vec3,
    /// Unit normal.
    pub normal: Vec3,
    /// Unit in-plane axis, orthogonal to `normal`.
    pub u: Vec3,
    /// Unit in-plane axis; `u × v = normal`.
    pub v: Vec3,
    /// Distance between neighbouring grid lines, positive.
    pub grid_spacing: f64,
    /// Half-width of the drawn grid: lines run from `−extent` to `+extent` along both axes.
    pub extent: f64,
}

impl Workplane {
    /// The drafting XY plane at height `z`: normal `+Z`, `u = X`, `v = Y`.
    pub fn xy_at(z: f64, grid_spacing: f64, extent: f64) -> Workplane {
        Workplane { origin: Vec3::new(0.0, 0.0, z), normal: Vec3::Z, u: Vec3::X, v: Vec3::Y, grid_spacing, extent }
    }

    /// A world point from in-plane coordinates.
    pub fn point(&self, a: f64, b: f64) -> Vec3 {
        self.origin + self.u * a + self.v * b
    }

    /// Whether [`render`] will draw this grid.
    pub fn validate(&self) -> Result<(), SceneError> {
        for (name, v) in [("origin", self.origin), ("normal", self.normal), ("u", self.u), ("v", self.v)] {
            if !v.is_finite() {
                return Err(SceneError::NonFinite(name));
            }
        }
        for (name, v) in [("grid_spacing", self.grid_spacing), ("extent", self.extent)] {
            if !v.is_finite() {
                return Err(SceneError::NonFinite(name));
            }
        }
        if self.normal.len() < 0.5 || self.u.len() < 0.5 || self.v.len() < 0.5 {
            return Err(SceneError::BadWorkplane("an axis has no length"));
        }
        if self.normal.dot(self.u).abs() > 1e-6 || self.normal.dot(self.v).abs() > 1e-6 {
            return Err(SceneError::BadWorkplane("an in-plane axis is not perpendicular to the normal"));
        }
        if self.u.dot(self.v).abs() > 1e-6 {
            return Err(SceneError::BadWorkplane("the in-plane axes are not perpendicular to each other"));
        }
        if self.grid_spacing <= 0.0 {
            return Err(SceneError::BadWorkplane("grid_spacing is not positive"));
        }
        if self.extent < 0.0 {
            return Err(SceneError::BadWorkplane("extent is negative"));
        }
        Ok(())
    }

    /// [`Workplane::validate`] as a bool.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// How many grid lines fit along each axis, per side: `ceil(extent / spacing)`, capped so one
    /// hostile extent cannot ask for a million segments.
    fn half_lines(&self) -> Option<i32> {
        self.validate().ok()?;
        let n = (self.extent / self.grid_spacing).ceil();
        if !n.is_finite() {
            return None;
        }
        Some(n.min((MAX_GRID_LINES.saturating_sub(1) / 2) as f64) as i32)
    }

    /// The grid as line segments in world space.
    ///
    /// Lines parallel to `v` first, at `u`-coordinates `−n·spacing … +n·spacing`, then lines
    /// parallel to `u` at the same `v`-coordinates, `n` from [`Workplane::half_lines`]. Every
    /// endpoint is exactly on the plane, because `u` and `v` are validated perpendicular to the
    /// normal. Empty for an unusable workplane — see [`Workplane::validate`], which is what
    /// reports why.
    pub fn grid_lines(&self) -> Vec<Segment> {
        let Some(n) = self.half_lines() else { return Vec::new() };
        let span = f64::from(n) * self.grid_spacing;
        let mut out = Vec::with_capacity(2 * (2 * n as usize + 1));
        for i in -n..=n {
            let at = f64::from(i) * self.grid_spacing;
            out.push(Segment::new(self.point(at, -span), self.point(at, span)));
        }
        for i in -n..=n {
            let at = f64::from(i) * self.grid_spacing;
            out.push(Segment::new(self.point(-span, at), self.point(span, at)));
        }
        out
    }
}

/// A camera, some solids, and the grid they were sketched against.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub camera: Camera,
    /// The bodies to draw, each with the material it is drawn in. Drawn in this order, which is
    /// the order a depth tie is broken in, so put the important body first.
    pub solids: Vec<(Solid, Material)>,
    pub workplane: Option<Workplane>,
}

impl Scene {
    /// An empty scene seen by `camera`.
    pub fn new(camera: Camera) -> Scene {
        Scene { camera, ..Scene::default() }
    }

    /// The bounds of every solid, ignoring the workplane.
    pub fn bounds(&self) -> Bounds3 {
        self.solids.iter().fold(Bounds3::EMPTY, |b, (s, _)| b.union(&s.bounds()))
    }

    /// Add a body.
    pub fn push(&mut self, solid: Solid, material: Material) -> &mut Scene {
        self.solids.push((solid, material));
        self
    }
}

/// How to draw a [`Scene`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    /// Filled in before anything is drawn; linear, encoded like any other colour.
    pub background: Rgba,
    /// Direction from a surface **towards** the light. Normalised internally.
    pub light: Vec3,
    /// Constant light every surface gets, whatever its normal (see [`Material::shade`]).
    pub ambient: f64,
    /// Weight of the `max(dot(n, l), 0)` term.
    pub diffuse: f64,
    /// Integer box-filter factor per axis, 1 (off) to [`MAX_SUPERSAMPLE`]. Values outside that
    /// range are clamped, never refused. Averaged in the encoded domain — see the module docs.
    pub supersample: u32,
    /// Drop triangles whose winding faces away from the camera.
    pub cull_back_faces: bool,
    /// The workplane grid's colour, unlit. Ignored when the scene has no workplane.
    pub grid_color: Rgba,
}

impl Default for RenderOptions {
    fn default() -> RenderOptions {
        RenderOptions {
            width: 320,
            height: 240,
            background: Rgba::new(0.055, 0.06, 0.07),
            light: Vec3::new(-0.4, -0.75, 0.9),
            ambient: DEFAULT_AMBIENT,
            diffuse: DEFAULT_DIFFUSE,
            supersample: 1,
            cull_back_faces: true,
            grid_color: Rgba::new(0.32, 0.34, 0.36),
        }
    }
}

impl RenderOptions {
    /// The image size, refusing a zero side, an overflowing product and anything over
    /// [`MAX_IMAGE_PIXELS`].
    pub fn size(&self) -> Result<(u32, u32), SceneError> {
        if self.width == 0 || self.height == 0 {
            return Err(SceneError::EmptyImage);
        }
        let pixels = usize::try_from(self.width).ok().and_then(|w| usize::try_from(self.height).ok().and_then(|h| w.checked_mul(h)));
        match pixels {
            Some(p) if p <= MAX_IMAGE_PIXELS => Ok((self.width, self.height)),
            Some(p) => Err(SceneError::ImageTooLarge { pixels: p, max: MAX_IMAGE_PIXELS }),
            None => Err(SceneError::ImageTooLarge { pixels: usize::MAX, max: MAX_IMAGE_PIXELS }),
        }
    }

    /// The supersampling factor actually used: clamped into `1..=MAX_SUPERSAMPLE`, so 0 draws
    /// rather than failing and a hostile 65535 does not ask for a 65535² buffer.
    pub fn factor(&self) -> u32 {
        self.supersample.clamp(1, MAX_SUPERSAMPLE)
    }

    /// The light as a unit vector, or zero when it has none (which shades everything ambient).
    pub fn light_dir(&self) -> Vec3 {
        if !self.light.is_finite() {
            return Vec3::ZERO;
        }
        let l = self.light.normalized();
        if l.len() < 0.5 { Vec3::ZERO } else { l }
    }
}

/// An RGBA8 image: 4 bytes per pixel, row-major, row 0 at the **top**, alpha never premultiplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Image {
    /// A background-filled image. `background` is encoded the way any other colour is.
    pub fn new(width: u32, height: u32, background: Rgba) -> Result<Image, SceneError> {
        let pixels = usize::try_from(width).ok().and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)));
        let Some(pixels) = pixels else { return Err(SceneError::ImageTooLarge { pixels: usize::MAX, max: MAX_IMAGE_PIXELS }) };
        if width == 0 || height == 0 {
            return Err(SceneError::EmptyImage);
        }
        if pixels > MAX_IMAGE_PIXELS {
            return Err(SceneError::ImageTooLarge { pixels, max: MAX_IMAGE_PIXELS });
        }
        let px = background.to_u8();
        let mut data = Vec::with_capacity(pixels * 4);
        for _ in 0..pixels {
            data.extend_from_slice(&px);
        }
        Ok(Image { width, height, data })
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let w = usize::try_from(self.width).ok()?;
        let at = usize::try_from(y).ok()?.checked_mul(w)?.checked_add(usize::try_from(x).ok()?)?;
        self.data.get(at.checked_mul(4)?..)?.get(..4).and_then(|p| <[u8; 4]>::try_from(p).ok())
    }

    /// Write a pixel; outside the image is a no-op.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let (Ok(w), Ok(x), Ok(y)) = (usize::try_from(self.width), usize::try_from(x), usize::try_from(y)) else { return };
        let Some(at) = y.checked_mul(w).and_then(|r| r.checked_add(x)).and_then(|i| i.checked_mul(4)) else { return };
        if let Some(slot) = self.data.get_mut(at..at + 4) {
            slot.copy_from_slice(&rgba);
        }
    }

    /// The PNG encoding of this image, via the `tiny-skia` the crate already draws 2D with — no
    /// image dependency is added for it.
    pub fn encode_png(&self) -> Result<Vec<u8>, SceneError> {
        let Some(size) = tiny_skia::IntSize::from_wh(self.width, self.height) else { return Err(SceneError::EmptyImage) };
        let pm = tiny_skia::Pixmap::from_vec(self.data.clone(), size)
            .ok_or_else(|| SceneError::Encode("the buffer is not the right size for the image".into()))?;
        pm.encode_png().map_err(|e| SceneError::Encode(e.to_string()))
    }
}

/// Draw `scene` into an RGBA8 image.
///
/// The order is: background, then the workplane grid, then the solids in [`Scene::solids`] order.
/// A pixel is written when its sample depth is **strictly** less than what is there, so a tie
/// keeps the earlier one — which is what makes two runs byte-identical.
///
/// **Culling rule.** With the image's `y` pointing down, a triangle the kernel wound
/// counter-clockwise seen from outside lands with a **negative** signed area
/// (`(p1 − p0) × (p2 − p0)`), because the projection flips `y` and flips handedness with it.
/// Front faces are therefore the negative-area ones; a zero-area (degenerate) triangle is dropped
/// either way. [`RenderOptions::cull_back_faces`] off keeps both, and the depth buffer then
/// decides — which is what a closed solid wants anyway.
///
/// Clipping is against the near plane only, in view space, before projection. The far plane is a
/// depth range, not a clip: a surface beyond it draws at the same depths as one just inside, which
/// no picture can tell apart.
pub fn render(scene: &Scene, opts: &RenderOptions) -> Result<Image, SceneError> {
    let (w, h) = opts.size()?;
    scene.camera.validate()?;
    if let Some(wp) = scene.workplane.as_ref() {
        wp.validate()?;
    }
    if !opts.background.is_finite() {
        return Err(SceneError::NonFinite("background"));
    }
    let f = opts.factor();
    let rw = w.checked_mul(f).ok_or(SceneError::RasterTooLarge { pixels: usize::MAX, max: MAX_RASTER_PIXELS })?;
    let rh = h.checked_mul(f).ok_or(SceneError::RasterTooLarge { pixels: usize::MAX, max: MAX_RASTER_PIXELS })?;
    let raster_pixels = usize::try_from(rw).ok().and_then(|a| usize::try_from(rh).ok().and_then(|b| a.checked_mul(b)));
    let Some(raster_pixels) = raster_pixels else {
        return Err(SceneError::RasterTooLarge { pixels: usize::MAX, max: MAX_RASTER_PIXELS });
    };
    if raster_pixels > MAX_RASTER_PIXELS {
        return Err(SceneError::RasterTooLarge { pixels: raster_pixels, max: MAX_RASTER_PIXELS });
    }

    let (rw, rh) = (usize::try_from(rw).unwrap_or(0), usize::try_from(rh).unwrap_or(0));
    let cam = Cam::new(&scene.camera, f64::from(w) / f64::from(h), rw as f64, rh as f64);
    let mut r = Raster::new(rw, rh, opts.background.to_u8());
    if let Some(wp) = scene.workplane.as_ref() {
        for s in wp.grid_lines() {
            r.line(&cam, s, opts.grid_color.to_u8());
        }
    }
    let light = opts.light_dir();
    let mut drawn = 0usize;
    for (solid, material) in &scene.solids {
        for t in mesh_from_solid(solid)?.triangles() {
            drawn += 1;
            if drawn > MAX_TRIANGLES {
                return Err(SceneError::TooManyTriangles { count: drawn, max: MAX_TRIANGLES });
            }
            let n = (t.b - t.a).cross(t.c - t.a);
            // A degenerate triangle has no normal and no pixels: skipped, not an error. The
            // test is exact rather than a tolerance so a small but real triangle is still drawn.
            if n.len() <= 0.0 || !n.len().is_finite() {
                continue;
            }
            let color = material.shade(n, light, opts.ambient, opts.diffuse).to_u8();
            r.triangle(&cam, t.a, t.b, t.c, color, opts.cull_back_faces);
        }
    }
    r.finish(w, h, f)
}

/// The view transform, the projection and the screen mapping, in one place.
struct Cam {
    view: [[f64; 4]; 4],
    proj: [[f64; 4]; 4],
    /// Raster size in pixels (`width × supersample`).
    width: f64,
    height: f64,
    near: f64,
    /// `z_clip = a · z_view + b`, the depth map the projection itself uses.
    a: f64,
    b: f64,
}

impl Cam {
    fn new(camera: &Camera, aspect: f64, width: f64, height: f64) -> Cam {
        let (n, far) = (camera.near, camera.far);
        Cam {
            view: camera.view_matrix(),
            proj: camera.projection(aspect),
            width,
            height,
            near: n,
            // The projection's own depth map, so the interpolated `z` and the stored buffer
            // value cannot drift apart.
            a: -far / (far - n),
            b: -(far * n) / (far - n),
        }
    }

    /// World to view space.
    fn to_view(&self, p: Vec3) -> Vec3 {
        let m = &self.view;
        Vec3::new(
            m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
        )
    }

    /// View space to image space: `x` right, `y` **down**, in pixels. `None` behind the camera.
    fn to_screen(&self, v: Vec3) -> Option<Vec2> {
        let w = -v.z;
        if !w.is_finite() || w <= 0.0 {
            return None;
        }
        let (x, y) = (self.proj[0][0] * v.x / w, self.proj[1][1] * v.y / w);
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some(Vec2::new((x * 0.5 + 0.5) * self.width, (0.5 - y * 0.5) * self.height))
    }

    /// Device depth of a view-space `z` at an interpolated `1/w`, the value kept in the depth
    /// buffer: the projection's own `z_clip` map, divided by `w` exactly as the hardware divide
    /// would.
    fn depth(&self, z_view: f64, inv_w: f64) -> f32 {
        if inv_w <= 0.0 || !inv_w.is_finite() {
            return 1.0;
        }
        let z = (self.a * z_view + self.b) * inv_w;
        if !z.is_finite() { 2.0 } else { z.clamp(0.0, 1.0) as f32 }
    }
}

/// A projected vertex: where it lands on the image, its view-space `z` and its `1/w`.
#[derive(Clone, Copy, Debug)]
struct Vtx {
    p: Vec2,
    z: f64,
    inv_w: f64,
}

impl Vtx {
    /// A view-space point that survived the near clip. `None` when it cannot be drawn.
    fn from_view(cam: &Cam, v: Vec3) -> Option<Vtx> {
        if !v.is_finite() || v.z > -cam.near {
            return None;
        }
        let inv_w = 1.0 / -v.z;
        Some(Vtx { p: cam.to_screen(v)?, z: v.z, inv_w })
    }
}

/// The colour samples and the depth buffer, at supersampled resolution.
struct Raster {
    w: usize,
    h: usize,
    color: Vec<u8>,
    depth: Vec<f32>,
}

impl Raster {
    fn new(w: usize, h: usize, background: [u8; 4]) -> Raster {
        let n = w.saturating_mul(h);
        let mut color = Vec::with_capacity(n.saturating_mul(4));
        for _ in 0..n {
            color.extend_from_slice(&background);
        }
        Raster { w, h, color, depth: vec![1.0; n] }
    }

    fn write(&mut self, x: usize, y: usize, z: f32, c: [u8; 4]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let Some(i) = y.checked_mul(self.w).and_then(|r| r.checked_add(x)) else { return };
        let Some(slot) = self.depth.get_mut(i) else { return };
        if z < *slot {
            *slot = z;
            if let Some(p) = self.color.get_mut(i * 4..i * 4 + 4) {
                p.copy_from_slice(&c);
            }
        }
    }

    /// Rasterize one world-space triangle.
    ///
    /// The triangle is clipped against the near plane in view space (which can leave a triangle, a
    /// quad or nothing), fanned back into triangles, projected, and scanned. Shading is flat, so
    /// only depth is interpolated — perspective-correctly: view-space `z` is the attribute and
    /// `1/w` the weight, which is what makes depth right on a slanted surface instead of right
    /// only when it faces the camera.
    fn triangle(&mut self, cam: &Cam, a: Vec3, b: Vec3, c: Vec3, color: [u8; 4], cull: bool) {
        let poly = clip_near(cam.near, [cam.to_view(a), cam.to_view(b), cam.to_view(c)]);
        let pts: Vec<Vtx> = poly.iter().filter_map(|v| Vtx::from_view(cam, *v)).collect();
        if pts.len() < 3 {
            return;
        }
        for i in 1..pts.len() - 1 {
            let (Some(p0), Some(p1), Some(p2)) = (pts.first().copied(), pts.get(i).copied(), pts.get(i + 1).copied()) else { return };
            self.scan(cam, p0, p1, p2, color, cull);
        }
    }

    /// Scan one projected, already-clipped triangle.
    fn scan(&mut self, cam: &Cam, p0: Vtx, p1: Vtx, p2: Vtx, color: [u8; 4], cull: bool) {
        let area = (p1.p - p0.p).cross(p2.p - p0.p);
        if !area.is_finite() || area == 0.0 {
            return;
        }
        // Front faces have negative image-space area (the module docs say why).
        if cull && area > 0.0 {
            return;
        }
        let (Some(min), Some(max)) = (min_of(p0.p, p1.p, p2.p), max_of(p0.p, p1.p, p2.p)) else { return };
        let x0 = scan_lo(min.x, self.w);
        let x1 = scan_hi(max.x, self.w);
        let y0 = scan_lo(min.y, self.h);
        let y1 = scan_hi(max.y, self.h);
        let inv_area = 1.0 / area;
        for y in y0..y1 {
            let py = y as f64 + 0.5;
            for x in x0..x1 {
                let q = Vec2::new(x as f64 + 0.5, py);
                let l0 = (p1.p - p0.p).cross(q - p0.p);
                let l1 = (p2.p - p1.p).cross(q - p1.p);
                let l2 = (p0.p - p2.p).cross(q - p2.p);
                let inside = if area < 0.0 { l0 <= 0.0 && l1 <= 0.0 && l2 <= 0.0 } else { l0 >= 0.0 && l1 >= 0.0 && l2 >= 0.0 };
                if !inside {
                    continue;
                }
                // Each edge function is the weight of the vertex *opposite* its edge, and the
                // three sum to the area: `l1` belongs to `p0`, `l2` to `p1`, `l0` to `p2`.
                let (b0, b1, b2) = (l1 * inv_area, l2 * inv_area, l0 * inv_area);
                let den = b0 * p0.inv_w + b1 * p1.inv_w + b2 * p2.inv_w;
                if !den.is_finite() || den <= 0.0 {
                    continue;
                }
                // Perspective-correct view-space z, then the same linear map the projection uses.
                let z = (b0 * p0.z * p0.inv_w + b1 * p1.z * p1.inv_w + b2 * p2.z * p2.inv_w) / den;
                self.write(x, y, cam.depth(z, den), color);
            }
        }
    }

    /// One world-space line segment, one raster pixel wide, depth-tested like a triangle.
    fn line(&mut self, cam: &Cam, s: Segment, color: [u8; 4]) {
        if !s.is_finite() {
            return;
        }
        let Some((a, b)) = clip_segment_near(cam.near, cam.to_view(s.a), cam.to_view(s.b)) else { return };
        let (Some(pa), Some(pb)) = (cam.to_screen(a), cam.to_screen(b)) else { return };
        let (dx, dy) = (pb.x - pa.x, pb.y - pa.y);
        let Some((ta, tb)) = clip_t_to_raster(pa.x, pa.y, dx, dy, self.w, self.h) else { return };
        let (span, len) = (tb - ta, dx.hypot(dy));
        let steps = (len * (tb - ta)).max(0.0);
        if !steps.is_finite() || len <= 0.0 {
            return;
        }
        let n = steps.min(f64::from(u32::MAX)).max(1.0) as u32;
        let (ia, ib) = (1.0 / -a.z, 1.0 / -b.z);
        for i in 0..=n {
            let t = ta + span * (f64::from(i) / f64::from(n));
            let den = (1.0 - t) * ia + t * ib;
            let z = ((1.0 - t) * a.z * ia + t * b.z * ib) / den;
            if !den.is_finite() || den <= 0.0 || !z.is_finite() {
                continue;
            }
            let (Some(xi), Some(yi)) = (pixel_at(pa.x + dx * t, self.w), pixel_at(pa.y + dy * t, self.h)) else { continue };
            self.write(xi, yi, cam.depth(z, den), color);
        }
    }

    /// Box-filter the supersampled buffer down to the output size.
    fn finish(self, w: u32, h: u32, f: u32) -> Result<Image, SceneError> {
        let f = f.max(1) as usize;
        let (ow, oh) = (usize::try_from(w).unwrap_or(0), usize::try_from(h).unwrap_or(0));
        let n = f * f;
        let half = (n / 2) as u32;
        let mut data = Vec::with_capacity(ow.saturating_mul(oh).saturating_mul(4));
        for y in 0..oh {
            for x in 0..ow {
                let mut sum = [0u32; 4];
                for dy in 0..f {
                    for dx in 0..f {
                        let sx = x * f + dx;
                        let sy = y * f + dy;
                        let Some(i) = sy.checked_mul(self.w).and_then(|r| r.checked_add(sx)) else { continue };
                        for (c, v) in sum.iter_mut().enumerate() {
                            *v += u32::from(self.color.get(i * 4 + c).copied().unwrap_or(0));
                        }
                    }
                }
                for v in sum {
                    data.push(u8::try_from((v + half) / n as u32).unwrap_or(0));
                }
            }
        }
        Ok(Image { width: w, height: h, data })
    }
}

/// Clip a view-space triangle against the near plane `z ≤ −near`, keeping the winding.
///
/// Sutherland–Hodgman on one plane: at most four vertices out, fanned back into triangles. A
/// triangle entirely beyond the near plane yields nothing at all, which is the point — that is the
/// camera-inside-a-face case, and it draws nothing instead of dividing by a zero `w`.
fn clip_near(near: f64, poly: [Vec3; 3]) -> Vec<Vec3> {
    if !near.is_finite() || near <= 0.0 || poly.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }
    let inside = |p: Vec3| p.z <= -near;
    let mut out: Vec<Vec3> = Vec::with_capacity(4);
    for i in 0..3 {
        let (cur, next) = (poly[i], poly[(i + 1) % 3]);
        let (ci, ni) = (inside(cur), inside(next));
        if ci {
            out.push(cur);
        }
        if ci != ni {
            let dz = cur.z - next.z;
            if dz == 0.0 || !dz.is_finite() {
                return Vec::new();
            }
            let t = (-near - cur.z) / dz;
            if !t.is_finite() {
                return Vec::new();
            }
            out.push(cur.lerp(next, t));
        }
    }
    out
}

/// Clip a view-space segment against the near plane; `None` when it is entirely behind it.
fn clip_segment_near(near: f64, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    if !near.is_finite() || near <= 0.0 || !a.is_finite() || !b.is_finite() {
        return None;
    }
    let (ia, ib) = (a.z <= -near, b.z <= -near);
    match (ia, ib) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) | (false, true) => {
            let dz = a.z - b.z;
            if dz == 0.0 || !dz.is_finite() {
                return None;
            }
            let t = (-near - a.z) / dz;
            if !t.is_finite() {
                return None;
            }
            let p = a.lerp(b, t);
            Some(if ia { (a, p) } else { (p, b) })
        }
    }
}

/// The elementwise minimum of three points.
fn min_of(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let m = Vec2::new(a.x.min(b.x).min(c.x), a.y.min(b.y).min(c.y));
    m.is_finite().then_some(m)
}

/// The elementwise maximum of three points.
fn max_of(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let m = Vec2::new(a.x.max(b.x).max(c.x), a.y.max(b.y).max(c.y));
    m.is_finite().then_some(m)
}

/// The smallest pixel index a scan of a span starting at image coordinate `v` can start at.
///
/// Deliberately one pixel wider than the tight bound rather than one pixel narrower: a triangle
/// that is skipped at the edge shows up as a hole, and a triangle scanned one pixel too wide shows
/// up as nothing at all (the coverage test throws those pixels away).
fn scan_lo(v: f64, limit: usize) -> usize {
    if !v.is_finite() || v <= 0.5 {
        return 0;
    }
    let i = v.floor() - 1.0;
    if !i.is_finite() || i <= 0.0 { 0 } else { (i as usize).min(limit) }
}

/// One past the largest pixel index a scan of a span ending at image coordinate `v` can reach.
fn scan_hi(v: f64, limit: usize) -> usize {
    if !v.is_finite() {
        return 0;
    }
    let i = v.ceil() + 1.0;
    if !i.is_finite() || i <= 0.0 { 0 } else { (i as usize).min(limit) }
}

/// The pixel an image point falls in, or `None` outside the raster.
fn pixel_at(v: f64, limit: usize) -> Option<usize> {
    if !v.is_finite() {
        return None;
    }
    let i = v.floor();
    if i < 0.0 || i >= limit as f64 {
        return None;
    }
    Some(i as usize)
}

/// The part of a segment, as `t` into `0..=1`, that lies inside the `0..w` × `0..h` raster.
///
/// Liang–Barsky. A grid line runs far past the frame — a 9-unit grid seen from 9 units away — and
/// stepping every one of its projected pixels to reach the middle of the image is work no picture
/// needs.
fn clip_t_to_raster(x0: f64, y0: f64, dx: f64, dy: f64, w: usize, h: usize) -> Option<(f64, f64)> {
    if !x0.is_finite() || !y0.is_finite() || !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (fw, fh) = (w as f64, h as f64);
    for (p, q) in [(-dx, x0), (dx, fw - x0), (-dy, y0), (dy, fh - y0)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            t0 = t0.max(r);
        } else {
            if r < t0 {
                return None;
            }
            t1 = t1.min(r);
        }
    }
    (t0 <= t1 && t0.is_finite() && t1.is_finite()).then_some((t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_solid::{Face, Plane, SolidError, support};

    /// Panic with a message rather than a bare `Err`, as the kernel's own tests do.
    fn must<T>(r: Result<T, SceneError>, what: &str) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{what}: {e}"),
        }
    }

    fn solid<T>(r: Result<T, SolidError>, what: &str) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{what}: {e}"),
        }
    }

    /// A box of half-size `half` centred on `c`.
    fn box_at(center: Vec3, half: f64) -> Solid {
        solid(support::box_at(center - Vec3::new(half, half, half), center + Vec3::new(half, half, half)), "box_at")
    }

    /// Options at a size, with the default light and background.
    fn opts(width: u32, height: u32) -> RenderOptions {
        RenderOptions { width, height, ..RenderOptions::default() }
    }

    /// The camera the silhouette and depth tests share: up on +X, −Y and +Z, looking at the
    /// origin, so three faces are visible.
    fn camera() -> Camera {
        Camera { eye: Vec3::new(4.0, -6.0, 4.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 45.0, near: 0.5, far: 200.0 }
    }

    /// A camera almost level with a cube: the two side faces fill the silhouette's middle row and
    /// the top face is only a sliver at the top.
    fn level_camera() -> Camera {
        Camera { eye: Vec3::new(5.0, -7.0, 2.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 45.0, near: 0.5, far: 200.0 }
    }

    fn luma(img: &Image, x: u32, y: u32) -> Option<f64> {
        img.pixel(x, y).map(|p| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        match img.pixel(x, y) {
            Some(p) => p,
            None => panic!("({x}, {y}) is outside the {}x{} image", img.width, img.height),
        }
    }

    /// The bounding box of the pixels that differ from `bg`, as `(x0, y0, x1, y1)`, exclusive on
    /// the far side.
    fn ink_box(img: &Image, bg: [u8; 4]) -> Option<(u32, u32, u32, u32)> {
        let mut out: Option<(u32, u32, u32, u32)> = None;
        for y in 0..img.height {
            for x in 0..img.width {
                if img.pixel(x, y) == Some(bg) {
                    continue;
                }
                out = Some(match out {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
        out
    }

    fn ink_count(img: &Image, bg: [u8; 4]) -> usize {
        (0..img.height).flat_map(|y| (0..img.width).map(move |x| (x, y))).filter(|(x, y)| img.pixel(*x, *y) != Some(bg)).count()
    }

    /// Every distinct colour in a rectangle, as packed `0xRRGGBB`.
    fn shades(img: &Image, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(p) = img.pixel(x, y) {
                    let c = u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]);
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    /// Where a world point lands on the image, worked out here from the public matrices rather
    /// than from the rasteriser, so a silhouette test does not grade the rasteriser with itself.
    fn project(camera: &Camera, width: u32, height: u32, p: Vec3) -> Vec2 {
        let aspect = f64::from(width) / f64::from(height);
        let (m, pr) = (camera.view_matrix(), camera.projection(aspect));
        let v = [
            m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
        ];
        let w = -v[2];
        Vec2::new((pr[0][0] * v[0] / w * 0.5 + 0.5) * f64::from(width), (0.5 - pr[1][1] * v[1] / w * 0.5) * f64::from(height))
    }

    /// The eight corners of a box.
    fn corners(center: Vec3, half: f64) -> [Vec3; 8] {
        let (dx, dy, dz) = (Vec3::new(half, 0.0, 0.0), Vec3::new(0.0, half, 0.0), Vec3::new(0.0, 0.0, half));
        [
            center - dx - dy - dz,
            center + dx - dy - dz,
            center + dx + dy - dz,
            center - dx + dy - dz,
            center - dx - dy + dz,
            center + dx - dy + dz,
            center + dx + dy + dz,
            center - dx + dy + dz,
        ]
    }

    fn scene_with(solids: Vec<(Solid, Material)>) -> Scene {
        Scene { camera: camera(), solids, workplane: None }
    }

    /// A single square face in the XY plane, normal `+Z`. [`Solid::from_faces`] only checks the
    /// cheap invariants, so an open face is a legal solid — which is what makes it the honest
    /// subject of a culling test: only the cull can stop its triangles being drawn.
    fn open_square(flip: bool) -> Solid {
        let face = solid(Face::new(Plane::xy(0.0), support::rect(-1.0, -1.0, 1.0, 1.0)), "face");
        let face = solid(if flip { face.flipped() } else { Ok(face) }, "flip");
        solid(Solid::from_faces(vec![face]), "from_faces")
    }

    /// Straight down the Z axis, so a `+Z` face is front-facing from one end and not the other.
    fn axis_camera(z: f64) -> Camera {
        Camera { eye: Vec3::new(0.0, 0.0, z), target: Vec3::ZERO, up: Vec3::Y, fov_degrees: 60.0, near: 0.1, far: 50.0 }
    }

    // ------------------------------------------------------------------ 1

    #[test]
    fn a_cube_has_the_silhouette_its_corners_project_to() {
        let cam = camera();
        let o = opts(160, 160);
        let bg = o.background.to_u8();
        let img = must(render(&scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]), &o), "render");

        let cs = corners(Vec3::ZERO, 1.0);
        let pr = cs.iter().map(|p| project(&cam, o.width, o.height, *p)).collect::<Vec<_>>();
        let (ex0, ex1, ey0, ey1) =
            pr.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |a, p| (a.0.min(p.x), a.1.max(p.x), a.2.min(p.y), a.3.max(p.y)));

        let Some((x0, y0, x1, y1)) = ink_box(&img, bg) else { panic!("the cube drew nothing at all") };
        // The silhouette reaches the projected corners and no further, to within the pixel a
        // sample centre is worth: a pixel is inked when its centre is covered, so the inked box
        // sits inside the projected one by at most half a pixel a side.
        assert!((f64::from(x0) - ex0).abs() <= 2.0, "left edge {x0} vs projected {ex0:.2}");
        assert!((f64::from(x1) - ex1).abs() <= 2.0, "right edge {x1} vs projected {ex1:.2}");
        assert!((f64::from(y0) - ey0).abs() <= 2.0, "top edge {y0} vs projected {ey0:.2}");
        assert!((f64::from(y1) - ey1).abs() <= 2.0, "bottom edge {y1} vs projected {ey1:.2}");

        // A projected cube is roughly half the frame, and sits about where the camera points —
        // not exactly on the image centre, because a central projection does not carry a body's
        // central symmetry to the image (the near half is magnified more than the far one).
        assert!((55..=105).contains(&(x1 - x0)), "silhouette is {} px wide of 160", x1 - x0);
        assert!((55..=105).contains(&(y1 - y0)), "silhouette is {} px tall of 160", y1 - y0);
        assert!(((f64::from(x0) + f64::from(x1)) / 2.0 - 80.0).abs() <= 6.0, "the silhouette is nowhere near the middle of the frame");
        assert!(((f64::from(y0) + f64::from(y1)) / 2.0 - 80.0).abs() <= 6.0, "the silhouette is nowhere near the middle of the frame");

        // Centre lit, corners background: the body is where it should be and nowhere else.
        assert_ne!(px(&img, 80, 80), bg, "the centre of the image is background: nothing was drawn");
        let lit = luma(&img, 80, 80).unwrap_or(0.0);
        assert!(lit > 40.0, "the centre is barely lit: {lit:.1}");
        for (x, y) in [(0, 0), (159, 0), (0, 159), (159, 159)] {
            assert_eq!(px(&img, x, y), bg, "corner ({x}, {y}) should be background");
        }
        assert!(ink_count(&img, bg) > 2500, "only {} pixels drawn", ink_count(&img, bg));
    }

    #[test]
    fn the_cube_silhouette_is_a_filled_hexagon_not_its_bounding_rectangle() {
        let o = opts(160, 160);
        let bg = o.background.to_u8();
        let img = must(render(&scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]), &o), "render");
        let Some(box_rect) = ink_box(&img, bg) else { panic!("nothing drawn") };
        let (x0, y0, x1, y1) = box_rect;
        let ink = ink_count(&img, bg) as u32;
        let area = (x1 - x0) * (y1 - y0);
        // A projected cube is a hexagon, so it does not reach the corners of its own bounding
        // box — a renderer filling that box was drawing its silhouette wrong.
        for (x, y) in [(x0, y0), (x1 - 1, y0), (x0, y1 - 1), (x1 - 1, y1 - 1)] {
            assert_eq!(px(&img, x, y), bg, "the corner ({x}, {y}) of the bounding box is inked: that is a rectangle, not a cube");
        }
        assert!(ink < area, "ink {ink} fills the whole {area} box");
        assert!(ink as f64 > 0.6 * f64::from(area), "ink {ink} of {area}: too thin to be a cube's silhouette");
    }

    // ------------------------------------------------------------------ 2

    #[test]
    fn the_left_and_right_faces_of_a_box_are_lit_differently() {
        // The light comes from −X and below, so the box's +X face (which this camera sees on the
        // right) is lit by the ambient term only while the −Y face (on the left) gets a real
        // lambert. A renderer that drew the triangles without lighting them would show a uniform
        // grey box, which is what this catches.
        let o = RenderOptions { width: 200, height: 200, light: Vec3::new(-1.0, -0.3, 0.4), ..opts(200, 200) };
        let cam = level_camera();
        let scene = Scene { camera: cam, ..scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]) };
        let bg = o.background.to_u8();
        let img = must(render(&scene, &o), "render");
        let Some((x0, y0, x1, y1)) = ink_box(&img, bg) else { panic!("nothing drawn") };

        // Sample the middle of each of the two side faces, at the point each face's own centre
        // projects to: an interior point of a front-facing, unoccluded face, so the pixel there
        // is that face and nothing else.
        let l = project(&cam, o.width, o.height, Vec3::new(0.0, -1.0, 0.0));
        let r = project(&cam, o.width, o.height, Vec3::new(1.0, 0.0, 0.0));
        let (lx, rx) = (l.x.round() as u32, r.x.round() as u32);
        let (ly, ry) = (l.y.round() as u32, r.y.round() as u32);
        assert!(lx < rx, "the −Y face is not to the left of the +X face: the test is measuring the wrong pixels");
        assert!(lx > x0 && rx < x1 && ly > y0 && ry < y1, "a face centre fell outside the silhouette");
        let left = luma(&img, lx, ly).unwrap_or(-1.0);
        let right = luma(&img, rx, ry).unwrap_or(-1.0);
        assert_ne!(px(&img, lx, ly), bg, "the −Y face's centre is background");
        assert_ne!(px(&img, rx, ry), bg, "the +X face's centre is background");
        assert!((left - right).abs() >= 10.0, "left {left:.1} vs right {right:.1}: a flat-shaded box must not be uniform grey");
        assert!(right < left, "the light comes from −X, so the +X face ({right:.1}) should be the darker one, not {left:.1}");

        // And the body is not one colour: three faces, at least two shades.
        let shades = shades(&img, (x0, y0, x1, y1));
        assert!(shades.len() >= 2, "every pixel of the box has the same colour {shades:?}");
        assert!(shades.len() <= 8, "{} distinct colours: that is not flat shading", shades.len());
    }

    // ------------------------------------------------------------------ 3

    #[test]
    fn the_nearer_of_two_overlapping_boxes_wins() {
        // The two boxes are on one view ray, so the near one certainly covers the far one's
        // middle. A broken depth buffer lets the far one show through.
        let cam = camera();
        let along = (cam.target - cam.eye).normalized();
        let o = opts(160, 160);
        let red = Material::new(Rgba::new(0.8, 0.02, 0.02));
        let blue = Material::new(Rgba::new(0.02, 0.05, 0.8));
        // Five units in front of the eye, and eight behind it. The far box is the bigger one, so
        // it still shows around the near one and the test can tell "the near box won" from
        // "nothing overlapped at all".
        let front = box_at(cam.eye + along * 5.0, 0.25);
        let back = box_at(Vec3::ZERO, 1.5);

        let both = must(render(&scene_with(vec![(front.clone(), red), (back.clone(), blue)]), &o), "both");
        let only_front = must(render(&scene_with(vec![(front.clone(), red)]), &o), "front only");
        let only_back = must(render(&scene_with(vec![(back.clone(), blue)]), &o), "back only");

        // The image centre is on the view ray, so all three boxes project there.
        let c = project(&cam, o.width, o.height, Vec3::ZERO);
        let (x, y) = (c.x.round() as u32, c.y.round() as u32);
        assert_eq!(px(&both, x, y), px(&only_front, x, y), "the near box lost the depth test at ({x}, {y})");
        assert_ne!(px(&both, x, y), px(&only_back, x, y), "the two boxes look the same at the centre: they are not overlapping");
        let p = px(&both, x, y);
        assert!(f64::from(p[0]) > f64::from(p[2]) + 40.0, "the centre pixel is {p:?}: the far box won");

        // The far box still shows where the near one does not cover it. The far box's
        // silhouette is well over 20 px half-wide and the near one's under 10, so 16 px out from
        // the centre is unambiguously inside the first and outside the second.
        let far = px(&only_back, x, y);
        assert!(f64::from(far[2]) > f64::from(far[0]) + 40.0, "the far box's own centre is {far:?}");
        let bg = o.background.to_u8();
        let (ox, oy) = (x + 16, y);
        assert_eq!(px(&only_front, ox, oy), bg, "the near box reaches 16 px out from the centre");
        assert_eq!(px(&both, ox, oy), px(&only_back, ox, oy), "the far box is not visible beside the near one");
        assert_ne!(px(&both, ox, oy), bg, "nothing was drawn 16 px out from the centre");
        assert!(ink_count(&both, bg) > ink_count(&only_front, bg), "the far box is entirely hidden behind the near one");
    }

    // ------------------------------------------------------------------ 4

    #[test]
    fn a_back_facing_triangle_contributes_nothing() {
        let o = opts(120, 120);
        let bg = o.background.to_u8();
        let scene = |z: f64, flip: bool| Scene { camera: axis_camera(z), ..scene_with(vec![(open_square(flip), Material::default())]) };

        // The face's normal is +Z. Seen from −Z it is back-facing and culled away entirely.
        let behind = must(render(&scene(-5.0, false), &o), "behind");
        assert_eq!(ink_count(&behind, bg), 0, "a back-facing triangle was drawn");

        // Seen from +Z it is front-facing and fills most of the frame.
        let front = must(render(&scene(5.0, false), &o), "front");
        assert!(ink_count(&front, bg) > 1000, "a front-facing triangle drew only {} pixels", ink_count(&front, bg));

        // With culling off the back face draws too: the cull is what dropped it.
        let nocull = must(render(&scene(-5.0, false), &RenderOptions { cull_back_faces: false, ..o }), "no cull");
        assert!(ink_count(&nocull, bg) > 1000, "culling off still drew nothing: the triangle is not there at all");

        // The same square wound the other way swaps which side sees it, which is the kernel's
        // convention showing through rather than the renderer's own.
        let reversed = must(render(&scene(-5.0, true), &o), "reversed");
        assert!(ink_count(&reversed, bg) > 1000, "the reversed square is not visible from behind: {} pixels", ink_count(&reversed, bg));
        assert_eq!(ink_count(&must(render(&scene(5.0, true), &o), "reversed front"), bg), 0, "the reversed square is visible from its own side too");
    }

    // ------------------------------------------------------------------ 5

    #[test]
    fn the_same_scene_renders_byte_for_byte_the_same_twice() {
        let o = RenderOptions { width: 96, height: 72, supersample: 3, ..opts(96, 72) };
        let scene = Scene {
            camera: camera(),
            solids: vec![
                (box_at(Vec3::ZERO, 1.0), Material::default()),
                (box_at(Vec3::new(1.4, 0.3, 0.2), 0.5), Material::new(Rgba::new(0.8, 0.3, 0.1))),
            ],
            workplane: Some(Workplane::xy_at(0.0, 1.0, 3.0)),
        };
        let a = must(render(&scene, &o), "first");
        let b = must(render(&scene, &o), "second");
        assert_eq!(a.data, b.data, "two renders of one scene differ");
        assert_eq!((a.width, a.height, a.data.len()), (96, 72, 96 * 72 * 4));
        // Not trivially identical: the picture has something in it to compare.
        assert!(ink_count(&a, o.background.to_u8()) > 1000, "the scene drew nothing to compare");
    }

    // ------------------------------------------------------------------ 6

    #[test]
    fn the_workplane_grid_lies_on_its_plane_at_its_spacing() {
        let wp = Workplane { origin: Vec3::new(1.0, 2.0, 3.0), normal: Vec3::Z, u: Vec3::X, v: Vec3::Y, grid_spacing: 0.5, extent: 1.0 };
        assert!(wp.is_valid());
        let lines = wp.grid_lines();
        // Two sets of five lines: k = −2…2 at a half-unit spacing covers ±1.
        assert_eq!(lines.len(), 10, "line count {}", lines.len());

        for s in &lines {
            assert!(s.is_finite());
            assert!(s.length() > 0.0, "a zero-length grid line {s:?}");
            for p in [s.a, s.b] {
                let d = (p - wp.origin).dot(wp.normal);
                assert!(d.abs() <= 1e-12, "{p:?} is {d} off the plane");
            }
            // Every segment runs along one in-plane axis.
            let du = (s.b - s.a).dot(wp.u);
            let dv = (s.b - s.a).dot(wp.v);
            assert!(du == 0.0 || dv == 0.0, "{s:?} is not along an in-plane axis");
        }

        // The lines parallel to v sit at u-coordinates that are multiples of the spacing, evenly,
        // and the ones parallel to u at v-coordinates that are.
        let (mut along_v, mut along_u): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
        for s in &lines {
            let m = s.midpoint() - wp.origin;
            if (s.b - s.a).dot(wp.u) == 0.0 {
                along_v.push(m.dot(wp.u) / wp.grid_spacing);
            } else {
                along_u.push(m.dot(wp.v) / wp.grid_spacing);
            }
        }
        assert_eq!(along_v.len(), 5, "five lines along v");
        assert_eq!(along_u.len(), 5, "five lines along u");
        along_v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        along_u.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        assert_eq!(along_v, vec![-2.0, -1.0, 0.0, 1.0, 2.0], "u-coordinates are not evenly spaced by grid_spacing");
        assert_eq!(along_u, vec![-2.0, -1.0, 0.0, 1.0, 2.0], "v-coordinates are not evenly spaced by grid_spacing");

        // Consecutive grid lines really are one spacing apart, in world units too.
        for pair in along_v.windows(2) {
            assert!((pair[1] - pair[0] - 1.0).abs() <= 1e-12, "gap {} != one spacing", pair[1] - pair[0]);
        }
        for s in &lines {
            assert!((s.length() - 2.0).abs() <= 1e-12, "line length {} != 2·extent", s.length());
        }

        // A hostile plane draws nothing instead of looping.
        assert!(Workplane { grid_spacing: 0.0, ..wp }.grid_lines().is_empty());
        assert!(Workplane { grid_spacing: -1.0, ..wp }.grid_lines().is_empty());
        assert!(Workplane { extent: -1.0, ..wp }.grid_lines().is_empty());
        assert!(Workplane { normal: Vec3::new(f64::NAN, 0.0, 1.0), ..wp }.grid_lines().is_empty());
        assert!(Workplane { u: Vec3::Z, ..wp }.grid_lines().is_empty(), "u along the normal is not a plane");
        assert!(Workplane { grid_spacing: 1e-12, extent: 1e12, ..wp }.grid_lines().len() <= 2 * MAX_GRID_LINES);
    }

    // ------------------------------------------------------------------ 7

    #[test]
    fn hostile_input_is_an_error_or_a_clean_image() {
        let o = opts(32, 32);
        let scene = scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]);
        let bg = o.background.to_u8();

        // A zero side is refused, not drawn.
        assert_eq!(render(&scene, &RenderOptions { width: 0, ..o }), Err(SceneError::EmptyImage));
        assert_eq!(render(&scene, &RenderOptions { height: 0, ..o }), Err(SceneError::EmptyImage));
        assert_eq!(Image::new(0, 4, bg_color()).err(), Some(SceneError::EmptyImage));
        assert!(matches!(render(&scene, &RenderOptions { width: 8192, height: 8192, ..o }), Err(SceneError::ImageTooLarge { .. })));

        // NaN cameras are refused rather than poisoning every pixel.
        for cam in [
            Camera { eye: Vec3::new(f64::NAN, 0.0, 0.0), ..camera() },
            Camera { target: Vec3::new(0.0, f64::INFINITY, 0.0), ..camera() },
            Camera { up: Vec3::new(f64::NAN, 1.0, 0.0), ..camera() },
            Camera { fov_degrees: f64::NAN, ..camera() },
            Camera { near: f64::NAN, ..camera() },
        ] {
            assert!(render(&Scene { camera: cam, ..scene.clone() }, &o).is_err(), "a NaN camera must be refused");
        }
        // Degenerate but finite cameras too.
        for cam in [
            Camera { fov_degrees: 0.0, ..camera() },
            Camera { fov_degrees: 180.0, ..camera() },
            Camera { fov_degrees: -30.0, ..camera() },
            Camera { near: 0.0, ..camera() },
            Camera { near: -1.0, ..camera() },
            Camera { near: 10.0, far: 1.0, ..camera() },
            Camera { eye: Vec3::ZERO, target: Vec3::ZERO, ..camera() },
            Camera { up: Vec3::new(4.0, -6.0, 4.0), ..camera() },
            Camera { up: Vec3::ZERO, ..camera() },
        ] {
            assert!(matches!(render(&Scene { camera: cam, ..scene.clone() }, &o), Err(SceneError::BadCamera(_))), "{cam:?} must be refused");
        }
        // An absurd but finite camera renders rather than exploding: the numbers it produces are
        // clamped, and the picture it draws is simply empty.
        let absurd = must(render(&Scene { camera: Camera { eye: Vec3::new(1e300, 1e300, 1e300), ..camera() }, ..scene.clone() }, &o), "absurd");
        assert_eq!((absurd.width, absurd.height, absurd.data.len()), (32, 32, 32 * 32 * 4));

        // A camera inside the body is not an error and does not panic: the near clip eats the faces
        // between it and the near plane, and what is left is the *inside of the far side*, whose
        // normals point at the camera and are therefore front-facing. A clean image, not a NaN.
        let inside = must(render(&Scene { camera: Camera { eye: Vec3::new(0.5, -0.3, 0.4), ..camera() }, ..scene.clone() }, &o), "inside");
        assert_eq!((inside.width, inside.height, inside.data.len()), (32, 32, 32 * 32 * 4));
        assert_eq!(
            inside.data,
            must(render(&Scene { camera: Camera { eye: Vec3::new(0.5, -0.3, 0.4), ..camera() }, ..scene.clone() }, &o), "inside").data
        );

        // A camera just inside a corner: most of the box is nearer than the near plane, so the
        // clipper is doing real work and still must not panic.
        let shaved = must(render(&Scene { camera: Camera { eye: Vec3::new(0.55, -0.55, 0.55), ..camera() }, ..scene.clone() }, &o), "shaved");
        assert_eq!(shaved.data.len(), 32 * 32 * 4);

        // A NaN colour renders black, not NaN bytes, and the silhouette is still there.
        let nan = must(render(&scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::new(Rgba::new(f64::NAN, 0.0, 0.0)))]), &o), "nan colour");
        assert!(ink_count(&nan, bg) > 0, "a NaN material still draws its silhouette");
        for y in 0..nan.height {
            for x in 0..nan.width {
                assert!(nan.pixel(x, y).is_some());
            }
        }

        // A workplane with a NaN axis is refused, and grid_lines draws nothing.
        let bad_plane = Scene { workplane: Some(Workplane { u: Vec3::new(0.0, f64::NAN, 0.0), ..Workplane::xy_at(0.0, 1.0, 2.0) }), ..scene.clone() };
        assert!(matches!(render(&bad_plane, &o), Err(SceneError::NonFinite(_))));
        assert!(bad_plane.workplane.is_some_and(|w| w.grid_lines().is_empty()));

        // A body far off screen renders clean, empty, and the same twice.
        let far = scene_with(vec![(box_at(Vec3::new(1e6, 1e6, 1e6), 1.0), Material::default())]);
        let a = must(render(&far, &o), "far");
        assert_eq!(a.data, must(render(&far, &o), "far").data);
        assert_eq!(ink_count(&a, bg), 0);

        // Supersampling outside 1..=MAX_SUPERSAMPLE is clamped, never refused and never absurd.
        assert!(render(&scene, &RenderOptions { supersample: 0, ..o }).is_ok());
        assert!(render(&scene, &RenderOptions { supersample: 65535, ..o }).is_ok());
        assert!(render(&scene, &RenderOptions { supersample: 3, ..o }).is_ok());
    }

    #[test]
    fn no_hostile_camera_can_panic_the_rasteriser() {
        // A sweep over hostile values in eye, target and field of view: every one either renders
        // or errors, and none of them unwinds. This is the regression test the no-panic rule asks
        // for, and it deliberately has no `#[should_panic]`.
        let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e300, -1e300, 1e-300];
        let scene = Scene {
            camera: Camera::default(),
            solids: vec![
                (box_at(Vec3::ZERO, 1.0), Material::default()),
                (
                    solid(support::box_at(Vec3::new(-5.0, -5.0, -5.0), Vec3::new(-4.0, -4.0, -4.0)), "far box"),
                    Material::new(Rgba::new(0.1, 0.9, 0.4)),
                ),
            ],
            workplane: Some(Workplane::xy_at(0.0, 1.0, 2.0)),
        };
        let o = RenderOptions { width: 8, height: 6, supersample: 2, ..RenderOptions::default() };
        let mut rendered = 0;
        for ex in bad {
            for ey in bad {
                for fov in bad {
                    for near in [0.0, -1.0, f64::NAN, 0.5] {
                        let cam = Camera { eye: Vec3::new(ex, ey, 3.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: fov, near, far: 100.0 };
                        if let Ok(img) = render(&Scene { camera: cam, ..scene.clone() }, &o) {
                            rendered += 1;
                            assert_eq!(img.data.len(), 8 * 6 * 4);
                        }
                    }
                }
            }
        }
        assert!(rendered > 0, "the sweep never rendered anything: it is not testing the rasteriser");
    }

    // ------------------------------------------------------------------ 8

    #[test]
    fn the_winding_convention_matches_the_kernels() {
        // The kernel winds outer loops counter-clockwise seen from outside and points the face
        // normal out (its invariant I4), so a correctly built box must be visible and lit.
        let o = opts(120, 120);
        let bg = o.background.to_u8();
        let outward = must(render(&scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]), &o), "outward");
        assert!(ink_count(&outward, bg) > 2000, "a correctly wound box drew only {} pixels", ink_count(&outward, bg));
        assert!(luma(&outward, 60, 60).unwrap_or(0.0) > 40.0, "the centre of a lit box is dark");
        // Each visible face carries its own shade, which only happens if the normals come out of
        // the winding the same way the kernel put them.
        assert!(shades(&outward, (0, 0, 120, 120)).len() >= 2);

        // Turned inside out — the same faces, the other way round — every triangle is back-facing,
        // so the cull drops the near side and the camera sees the *inside of the far side*. The
        // silhouette is unchanged (a closed solid's is) but every pixel is a different face with
        // a different normal, which is exactly what "the cull follows the winding" means.
        let inside_out =
            must(render(&scene_with(vec![(solid(box_at(Vec3::ZERO, 1.0).flipped(), "flipped"), Material::default())]), &o), "inside out");
        assert!(ink_count(&inside_out, bg) > 2000, "the inside-out box drew nothing: the cull dropped every triangle");
        assert_ne!(inside_out.data, outward.data, "the inside-out box is drawn exactly like the right-way one: the cull does not follow the winding");
        assert_ne!(
            shades(&inside_out, (0, 0, 120, 120)),
            shades(&outward, (0, 0, 120, 120)),
            "the inside-out box has the same shading: its normals came out the same way"
        );

        // The same statement without a closed shell to hide behind: a single square face is
        // visible from the side its normal points at and culled from the other, and turning that
        // face over swaps the two. This is the assertion that pins the convention down.
        let lit = RenderOptions { light: Vec3::new(0.0, 0.0, 1.0), ..o };
        let square = |z: f64, flip: bool| Scene { camera: axis_camera(z), ..scene_with(vec![(open_square(flip), Material::default())]) };
        let (front, behind, rev_front, rev_behind) = (
            must(render(&square(5.0, false), &lit), "front"),
            must(render(&square(-5.0, false), &lit), "behind"),
            must(render(&square(5.0, true), &lit), "reversed front"),
            must(render(&square(-5.0, true), &lit), "reversed behind"),
        );
        assert!(ink_count(&front, bg) > 1000, "a face is not visible from the side its normal points at");
        assert_eq!(ink_count(&behind, bg), 0, "a face is visible from the side its normal points away from");
        assert_eq!(ink_count(&rev_front, bg), 0, "a reversed face is visible from the wrong side");
        assert!(ink_count(&rev_behind, bg) > 1000, "a reversed face is not visible from the side its normal points at");
    }

    // ---------------------------------------------------------------- extras

    #[test]
    fn the_view_and_projection_matrices_are_the_documented_ones() {
        let cam = camera();
        let v = cam.view_matrix();
        let apply = |m: &[[f64; 4]; 4], p: Vec3| {
            Vec3::new(
                m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
                m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
                m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
            )
        };
        assert!(apply(&v, cam.eye).dist(Vec3::ZERO) < 1e-12, "the eye maps to {:?}", apply(&v, cam.eye));
        let look = apply(&v, cam.target - cam.eye);
        assert!(look.x.abs() < 1e-12 && look.y.abs() < 1e-12, "the view direction is not down −Z: {look:?}");
        assert!(look.z < 0.0, "the camera looks up");

        // OpenGL depth: the near plane is 0, the far plane 1.
        let p = cam.projection(1.5);
        let ndc = |z: f64| (p[2][2] * z + p[2][3]) / -z;
        assert!(ndc(-cam.near).abs() < 1e-12, "near maps to {}", ndc(-cam.near));
        assert!((ndc(-cam.far) - 1.0).abs() < 1e-12, "far maps to {}", ndc(-cam.far));
        assert!((p[0][0] / p[1][1] - 1.0 / 1.5).abs() < 1e-12, "the aspect is not width/height");

        // An unusable camera never returns a NaN, from either matrix.
        for bad in [
            Camera { eye: Vec3::new(f64::NAN, 0.0, 0.0), ..cam },
            Camera { up: Vec3::ZERO, ..cam },
            Camera { eye: Vec3::ZERO, target: Vec3::ZERO, ..cam },
            Camera { fov_degrees: 0.0, ..cam },
        ] {
            for m in [bad.view_matrix(), bad.projection(1.0)] {
                assert!(m.iter().flatten().all(|v| v.is_finite()), "a bad camera produced {m:?}");
            }
        }
        assert!(cam.projection(f64::NAN).iter().flatten().all(|v| *v == 0.0));
        assert!(cam.projection(0.0).iter().flatten().all(|v| *v == 0.0));
    }

    #[test]
    fn the_shading_equation_is_the_one_the_docs_say() {
        let m = Material::new(Rgba::new(0.5, 0.4, 0.2));
        let (n, l) = (Vec3::Z, Vec3::new(0.0, 0.0, 1.0));
        // Facing the light: ambient + diffuse, exactly.
        let lit = m.shade(n, l, 0.15, 0.85);
        assert!((lit.r - 0.5).abs() < 1e-12 && (lit.g - 0.4).abs() < 1e-12, "{lit:?}");
        // Facing away: the ambient term only.
        assert!((m.shade(n, l * -1.0, 0.15, 0.85).r - 0.075).abs() < 1e-12);
        // Half way: cos 45°.
        let half = m.shade(n, l.lerp(Vec3::X, 0.5).normalized(), 0.0, 1.0);
        assert!((half.r - 0.5 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9, "{half:?}");
        // A metal gets no diffuse at all.
        assert!((Material { metallic: 1.0, ..m }.shade(n, l, 0.15, 0.85).r - 0.075).abs() < 1e-12);
        // Non-finite anywhere is opaque black, never a NaN.
        for bad in [Rgba::new(f64::NAN, 0.0, 0.0), Rgba::new(0.0, 0.0, 0.0)] {
            assert!(Material::new(bad).shade(n, Vec3::new(f64::INFINITY, 0.0, 0.0), 0.15, 0.85).is_finite());
        }
        assert_eq!(Material::new(Rgba::new(f64::NAN, 0.0, 0.0)).shade(n, l, 0.15, 0.85), Rgba::new(0.0, 0.0, 0.0));
        assert!(Material::new(Rgba::new(0.1, 0.1, 0.1)).is_finite());

        // The gamma encode is the sRGB-ish power, and it is monotone.
        assert_eq!(encode_channel(0.0), 0);
        assert_eq!(encode_channel(1.0), 255);
        assert!(i32::from(encode_channel(1.0)) - i32::from(encode_channel(0.5)) > 50);
        assert_eq!(encode_channel(f64::NAN), 0);
        assert_eq!(encode_channel(f64::INFINITY), 0, "non-finite input is refused, not clamped");
        assert_eq!(encode_channel(-1.0), 0);
        assert_eq!(encode_channel(2.0), 255);
        // A round trip through from_srgb8 lands back on the same code value: the encode and
        // the decode are one curve and its inverse.
        for c in [0u8, 1, 10, 64, 128, 200, 255] {
            assert_eq!(Rgba::from_srgb8(c, c, c).to_u8()[0], c, "round trip of {c}");
        }
        // Alpha is written straight, with no transfer function.
        assert_eq!(Rgba::with_alpha(0.0, 0.0, 0.0, 0.5).to_u8()[3], 128);
    }

    #[test]
    fn a_cube_with_a_grid_renders_the_grid_around_it() {
        let o = opts(200, 160);
        let scene = Scene {
            camera: camera(),
            solids: vec![(box_at(Vec3::new(0.0, 0.0, 0.75), 0.75), Material::default())],
            workplane: Some(Workplane::xy_at(0.0, 0.5, 4.0)),
        };
        let bg = o.background.to_u8();
        let grid = o.grid_color.to_u8();
        let img = must(render(&scene, &o), "render");
        let Some((x0, y0, x1, y1)) = ink_box(&img, bg) else { panic!("nothing drawn") };
        // The grid reaches well past the box on both sides.
        assert!(x1 - x0 > 90, "the grid is only {} px wide", x1 - x0);
        assert!(y1 - y0 > 70, "the grid is only {} px tall", y1 - y0);
        let seen = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|(x, y)| px(&img, *x, *y) == grid).count();
        assert!(seen > 200, "only {seen} grid pixels");
        // The box's middle is neither the background nor a grid line.
        let centre = px(&img, 100, 80);
        assert_ne!(centre, bg);
        assert_ne!(centre, grid, "the middle of the image is grid, not the box");
        // And with no workplane the grid colour is nowhere in the picture.
        let bare = must(render(&scene_with(vec![scene.solids[0].clone()]), &o), "bare");
        assert!(!shades(&bare, (0, 0, 200, 160)).contains(&(u32::from(grid[0]) << 16 | u32::from(grid[1]) << 8 | u32::from(grid[2]))));
    }

    #[test]
    fn supersampling_softens_the_edge_without_moving_the_middle() {
        let o1 = opts(64, 64);
        let o2 = RenderOptions { supersample: 3, ..o1 };
        let scene = scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]);
        let bg = o1.background.to_u8();
        let sharp = must(render(&scene, &o1), "sharp");
        let smooth = must(render(&scene, &o2), "smooth");
        // The same silhouette, a softer edge: no more than a quarter of the ink moves.
        let (a, b) = (ink_count(&sharp, bg), ink_count(&smooth, bg));
        assert!(a.abs_diff(b) < a / 4, "the silhouette moved: {a} vs {b} pixels");
        assert_eq!(px(&sharp, 32, 32), px(&smooth, 32, 32), "the middle changed with the sampling factor");
        assert_eq!(smooth.data.len(), 64 * 64 * 4);
        // Factor 1 is the identity on the box filter.
        assert_eq!(must(render(&scene, &RenderOptions { supersample: 1, ..o1 }), "one").data, sharp.data);
    }

    #[test]
    fn an_image_encodes_as_a_png_of_the_right_size() {
        let o = opts(48, 32);
        let img = must(render(&scene_with(vec![(box_at(Vec3::ZERO, 1.0), Material::default())]), &o), "render");
        let png = must(img.encode_png(), "encode");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        assert_eq!(&png[12..16], b"IHDR", "no IHDR");
        // IHDR: length 13, "IHDR", then width and height as big-endian u32.
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (48, 32));
        assert!(png.len() > 100, "a {} byte PNG is empty", png.len());
        // A flat background still encodes to a real file.
        assert!(must(Image::new(48, 32, bg_color()).and_then(|i| i.encode_png()), "bg png").len() > 60);
    }

    #[test]
    fn the_image_helpers_are_safe_outside_the_buffer() {
        let o = opts(8, 6);
        let mut img = must(Image::new(o.width, o.height, bg_color()), "image");
        assert_eq!(img.pixel(0, 0), Some(bg_color().to_u8()));
        assert_eq!(img.pixel(8, 0), None);
        assert_eq!(img.pixel(0, 6), None);
        assert_eq!(img.pixel(u32::MAX, u32::MAX), None);
        img.set_pixel(u32::MAX, 0, [1, 2, 3, 4]);
        img.set_pixel(0, u32::MAX, [1, 2, 3, 4]);
        assert_eq!(img.pixel(0, 0), Some(bg_color().to_u8()), "an out-of-range write changed the image");
        img.set_pixel(1, 1, [1, 2, 3, 4]);
        assert_eq!(img.pixel(1, 1), Some([1, 2, 3, 4]));
    }

    /// The background the tests compare against: opaque black.
    fn bg_color() -> Rgba {
        Rgba::new(0.0, 0.0, 0.0)
    }

    /// Writes the cube-and-grid picture to `$CADCRAFT_SCENE3_PNG` — the artefact the renderer is
    /// judged on. Unset (as in a normal test run) it does nothing.
    #[test]
    fn writes_the_demo_png_where_the_env_var_points() {
        let Ok(path) = std::env::var("CADCRAFT_SCENE3_PNG") else { return };
        let o = RenderOptions { width: 640, height: 480, supersample: 3, ..RenderOptions::default() };
        let scene = Scene {
            camera: Camera {
                eye: Vec3::new(9.0, -13.0, 9.0),
                target: Vec3::new(0.0, 0.0, 1.2),
                up: Vec3::Z,
                fov_degrees: 40.0,
                near: 0.5,
                far: 200.0,
            },
            solids: vec![(box_at(Vec3::new(0.0, 0.0, 1.5), 1.5), Material::new(Rgba::from_srgb8(196, 122, 64)))],
            workplane: Some(Workplane::xy_at(0.0, 1.0, 9.0)),
        };
        let img = must(render(&scene, &o), "render");
        let png = must(img.encode_png(), "encode");
        if let Err(e) = std::fs::write(&path, &png) {
            panic!("{path}: {e}");
        }
    }
}
