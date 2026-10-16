//! Camera and interaction state for the 3D viewport — with no GUI in it.
//!
//! Everything the viewport needs to remember between frames lives here: the orbit camera, the pick
//! a pointer asked for and has not been answered yet, and what is selected. Nothing here imports
//! egui, so every limit below is a plain unit test rather than something that needs a window.
//!
//! # The camera
//!
//! An orbit camera is a [`target`](OrbitCamera::target), a [`distance`](OrbitCamera::distance),
//! an azimuth and an elevation measured from world **+Z** (the drafting up). The eye is derived,
//! never stored, so it can never drift from the point it is supposed to be looking at.
//!
//! # Frame-rate independence
//!
//! Every gesture is expressed **per pixel of pointer travel** or **per wheel notch**, never as a
//! per-frame velocity that is then integrated. That makes the result a function of the total
//! gesture and not of how the operating system happened to chop it into frames: one drag of 40
//! pixels and forty one-pixel frames of it move the camera the same amount, and three wheel notches
//! are the same as one notch three times. There is no inertia, so releasing the button leaves the
//! camera exactly where the pointer left it.
//!
//! # Hard limits
//!
//! A camera that has been dragged for an hour, scaled by a hostile number, or handed a NaN must
//! still be a camera. Every mutator therefore ends in [`OrbitCamera::sanitize`], and:
//!
//! - the distance is clamped to `MIN_DISTANCE..=MAX_DISTANCE` and is never zero, so the eye can
//!   never coincide with the target and `eye − target` can never divide by zero;
//! - the elevation is clamped to within [`POLE_GAP`] of the poles, where world +Z stops being a
//!   usable up hint;
//! - the azimuth is wrapped into `[-π, π)` so a long session cannot lose precision to it;
//! - the near and far planes are derived from the distance, so the depth range brackets the body at
//!   every zoom and a caller cannot leave the camera looking into its own near plane;
//! - anything non-finite is ignored or replaced, never propagated.
//!
//! Picking state is here too because it is the same kind of thing: [`PickRequest`] is what a click
//! asked for and [`Selection`] is what the model has selected, both plain data the host resolves.

use cadcraft_geom::{Bounds3, Vec2, Vec3};
use cadcraft_render::picking::Ray;
use cadcraft_render::scene3::Camera;

/// Nearest the eye may come to the target. Non-zero, so the eye can never pass through the point
/// it is orbiting and no view direction has to be recovered from a zero-length vector.
pub const MIN_DISTANCE: f64 = 1e-6;

/// Furthest the eye may go. Large enough for a site plan in millimetres, small enough that the
/// far plane stays a finite number.
pub const MAX_DISTANCE: f64 = 1e12;

/// Radians of azimuth/elevation per pixel of pointer travel.
pub const ORBIT_RADIANS_PER_PIXEL: f64 = 0.008;

/// The distance multiplier of one wheel notch (or one `+`/`-` press).
pub const DOLLY_FACTOR_PER_NOTCH: f64 = 1.2;

/// Radians of orbit of one arrow-key press: five degrees.
pub const KEY_ORBIT_RADIANS: f64 = 0.087_266_462_6;

/// Wheel notches past which a gesture is treated as a mistake rather than a request.
pub const MAX_DOLLY_NOTCHES: f64 = 64.0;

/// How far (radians) the elevation is kept from the poles. World +Z stops being a usable up hint
/// as the view direction lines up with it, so the camera refuses to go right over the top.
pub const POLE_GAP: f64 = 0.017_453_292_5;

/// How much bigger than the body a [`OrbitCamera::frame`] leaves the view, so it is not touching
/// the edge of the frame.
pub const FRAME_MARGIN: f64 = 1.25;

/// Lowest and highest vertical field of view a viewport camera may use, in degrees.
pub const MIN_FOV_DEGREES: f64 = 1.0;
pub const MAX_FOV_DEGREES: f64 = 179.0;

/// The default vertical field of view.
pub const DEFAULT_FOV_DEGREES: f64 = 45.0;

/// The default distance from the eye to the target.
pub const DEFAULT_DISTANCE: f64 = 10.0;

/// Near plane as a fraction of the distance: enough to keep a close-up out of it.
const NEAR_FRACTION: f64 = 1e-3;
/// The smallest near plane, for a camera almost on top of its target.
const MIN_NEAR: f64 = 1e-6;
/// Far plane as a multiple of the distance.
const FAR_FACTOR: f64 = 1e4;
/// The far plane is always at least this many near planes out.
const NEAR_FAR_RATIO: f64 = 10.0;

/// The default azimuth, in radians: the eye sits off the −X/+Y corner so a body's three faces are
/// visible, which is what every first look at a 3D part wants.
pub const DEFAULT_AZIMUTH: f64 = -0.955_316_6;
/// The default elevation, in radians: a little above the XY plane.
pub const DEFAULT_ELEVATION: f64 = 0.523_598_8;

/// `a` folded into `[-π, π)`, or `0.0` when it is not a number.
fn wrap_pi(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// `v` into `lo..=hi`, or `fallback` when it is not a number. Never panics on the arguments: the
/// bounds are constants, so they are never NaN and never inverted.
fn clamp_or(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if !v.is_finite() {
        return fallback;
    }
    v.clamp(lo, hi)
}

/// The near and far planes that bracket a view of something `distance` away.
fn depth_range(distance: f64) -> (f64, f64) {
    let d = if distance.is_finite() { distance.clamp(MIN_DISTANCE, MAX_DISTANCE) } else { DEFAULT_DISTANCE };
    let near = (d * NEAR_FRACTION).max(MIN_NEAR);
    let far = (d * FAR_FACTOR).max(near * NEAR_FAR_RATIO);
    (near, far)
}

/// An orbit camera: a target, a distance and a direction, with the eye derived from them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// What the camera looks at, in world coordinates.
    pub target: Vec3,
    /// Eye-to-target distance, always in `MIN_DISTANCE..=MAX_DISTANCE` after [`OrbitCamera::sanitize`].
    pub distance: f64,
    /// Radians, measured about world +Z from the +X axis.
    pub azimuth: f64,
    /// Radians above the XY plane, kept within [`POLE_GAP`] of the poles.
    pub elevation: f64,
    /// Vertical field of view, in degrees.
    pub fov_degrees: f64,
    /// Distance to the near plane; derived from `distance` by [`OrbitCamera::sanitize`].
    pub near: f64,
    /// Distance to the far plane; derived from `distance` by [`OrbitCamera::sanitize`].
    pub far: f64,
}

impl Default for OrbitCamera {
    fn default() -> OrbitCamera {
        let mut c = OrbitCamera {
            target: Vec3::ZERO,
            distance: DEFAULT_DISTANCE,
            azimuth: DEFAULT_AZIMUTH,
            elevation: DEFAULT_ELEVATION,
            fov_degrees: DEFAULT_FOV_DEGREES,
            near: 0.0,
            far: 0.0,
        };
        c.sanitize();
        c
    }
}

impl OrbitCamera {
    /// A camera looking at `target` from `distance` units along the default direction.
    pub fn looking_at(target: Vec3, distance: f64) -> OrbitCamera {
        let mut c = OrbitCamera { target, ..OrbitCamera::default() };
        c.distance = distance;
        c.sanitize();
        c
    }

    /// Force every field back into range: a camera that has been handed a hostile number, or
    /// written by hand with a NaN in it, becomes a usable camera. Idempotent.
    pub fn sanitize(&mut self) {
        if !self.target.is_finite() {
            self.target = Vec3::ZERO;
        }
        self.fov_degrees = clamp_or(self.fov_degrees, MIN_FOV_DEGREES, MAX_FOV_DEGREES, DEFAULT_FOV_DEGREES);
        self.distance = clamp_or(self.distance, MIN_DISTANCE, MAX_DISTANCE, DEFAULT_DISTANCE);
        self.azimuth = if self.azimuth.is_finite() { wrap_pi(self.azimuth) } else { DEFAULT_AZIMUTH };
        self.elevation = clamp_or(self.elevation, -POLE_LIMIT, POLE_LIMIT, DEFAULT_ELEVATION);
        let (n, f) = depth_range(self.distance);
        self.near = n;
        self.far = f;
    }

    /// This camera with every field in range (see [`OrbitCamera::sanitize`]).
    pub fn sanitized(&self) -> OrbitCamera {
        let mut c = *self;
        c.sanitize();
        c
    }

    /// Whether the fields are in range *and* the camera `scene3` will accept.
    ///
    /// A `false` here means the fields were written by hand or corrupted; the fix is
    /// [`OrbitCamera::sanitize`], which every mutator has already done.
    pub fn is_valid(&self) -> bool {
        *self == self.sanitized() && self.camera().is_valid()
    }

    /// The unit vector from the target towards the eye.
    pub fn view_direction(&self) -> Vec3 {
        let (sin_e, cos_e) = self.elevation.sin_cos();
        let (sin_a, cos_a) = self.azimuth.sin_cos();
        Vec3::new(cos_a * cos_e, sin_a * cos_e, sin_e)
    }

    /// Where the eye is, in world coordinates.
    pub fn eye(&self) -> Vec3 {
        self.target + self.view_direction() * self.distance
    }

    /// The camera's right and up axes on screen, both unit. Zero when the fields are unusable, so
    /// a pan that cannot be computed does nothing rather than producing a NaN.
    pub fn screen_axes(&self) -> (Vec3, Vec3) {
        let forward = (self.target - self.eye()).normalized();
        let right = forward.cross(Vec3::Z).normalized();
        (right, right.cross(forward).normalized())
    }

    /// The `scene3` camera this state describes.
    pub fn camera(&self) -> Camera {
        Camera {
            eye: self.eye(),
            target: self.target,
            // World +Z is the drafting up, and [`POLE_GAP`] is what keeps it a usable hint.
            up: Vec3::Z,
            fov_degrees: self.fov_degrees,
            near: self.near,
            far: self.far,
        }
    }

    /// Orbit by a pointer drag of `dx`/`dy` pixels: right moves the eye clockwise, down raises it.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.orbit_radians(dx * ORBIT_RADIANS_PER_PIXEL, dy * ORBIT_RADIANS_PER_PIXEL);
    }

    /// Orbit by radians, which is what the keyboard does. Absolute, so it splits the same way a
    /// pixel drag does.
    pub fn orbit_radians(&mut self, dx: f64, dy: f64) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.azimuth = wrap_pi(self.azimuth - dx);
        self.elevation = clamp_or(self.elevation + dy, -POLE_LIMIT, POLE_LIMIT, DEFAULT_ELEVATION);
        self.sanitize();
    }

    /// Pan by a pointer drag of `dx`/`dy` pixels, with `world_per_pixel` saying how big a pixel
    /// is where the camera is looking (see [`world_per_pixel`]).
    ///
    /// The scene follows the pointer, so the target moves against the drag. A drag is linear, so
    /// it splits the same way an orbit does.
    pub fn pan(&mut self, dx: f64, dy: f64, world_per_pixel: f64) {
        if !dx.is_finite() || !dy.is_finite() || !world_per_pixel.is_finite() || world_per_pixel <= 0.0 {
            return;
        }
        let (right, up) = self.screen_axes();
        let moved = self.target + right * (-dx * world_per_pixel) + up * (dy * world_per_pixel);
        if moved.is_finite() {
            self.target = moved;
        }
        self.sanitize();
    }

    /// Move the eye along its view axis. Positive `notches` pulls **back**; negative brings it
    /// closer, and the distance is clamped so the eye can never reach the target and pass it.
    ///
    /// Multiplicative, so three notches at once and one notch three times agree.
    pub fn dolly(&mut self, notches: f64) {
        if !notches.is_finite() || notches == 0.0 {
            return;
        }
        let n = notches.clamp(-MAX_DOLLY_NOTCHES, MAX_DOLLY_NOTCHES);
        let factor = DOLLY_FACTOR_PER_NOTCH.powf(n);
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let next = self.distance * factor;
        self.distance = if next.is_finite() {
            next
        } else if factor > 1.0 {
            MAX_DISTANCE
        } else {
            MIN_DISTANCE
        };
        self.sanitize();
    }

    /// Set the distance directly, clamped like a dolly.
    pub fn set_distance(&mut self, distance: f64) {
        self.distance = distance;
        self.sanitize();
    }

    /// Move the point the camera looks at. A non-finite point is refused, not adopted.
    pub fn set_target(&mut self, target: Vec3) {
        if target.is_finite() {
            self.target = target;
        }
        self.sanitize();
    }

    /// Frame `bounds`, keeping the viewing direction.
    ///
    /// `false`, with the camera untouched, for bounds that enclose nothing or are not finite —
    /// there is no such thing as a frame around no geometry, and inventing one would throw away
    /// where the user was looking.
    pub fn frame(&mut self, bounds: &Bounds3) -> bool {
        if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            return false;
        }
        let half = 0.5 * bounds.diagonal();
        let half = if half.is_finite() && half > 0.0 { half } else { 0.5 * DEFAULT_DISTANCE };
        let tan_half = (self.fov_degrees.to_radians() * 0.5).tan();
        let fit = if tan_half.is_finite() && tan_half > 0.0 { half / tan_half } else { half * 2.5 };
        self.target = bounds.center();
        self.distance = fit * FRAME_MARGIN;
        self.sanitize();
        true
    }

    /// Back to the standard view, keeping the field of view.
    pub fn home(&mut self) {
        let fov = self.fov_degrees;
        *self = OrbitCamera::default();
        self.fov_degrees = clamp_or(fov, MIN_FOV_DEGREES, MAX_FOV_DEGREES, DEFAULT_FOV_DEGREES);
        self.sanitize();
    }
}

/// The elevation limit, as a positive number: `90°` minus [`POLE_GAP`].
pub const POLE_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - POLE_GAP;

/// How many world units one point of screen **height** covers where the camera is looking.
///
/// `0.0` when it cannot be worked out (a camera at the target, a zero-sized panel, a field of
/// view outside `0..180`), which a pan treats as "nothing to do" rather than as a NaN.
pub fn world_per_pixel(distance: f64, fov_degrees: f64, height_points: f64) -> f64 {
    if !(distance.is_finite() && fov_degrees.is_finite() && height_points.is_finite()) {
        return 0.0;
    }
    if distance <= 0.0 || height_points <= 0.0 {
        return 0.0;
    }
    let tan_half = (fov_degrees.to_radians() * 0.5).tan();
    if !tan_half.is_finite() || tan_half <= 0.0 {
        return 0.0;
    }
    (2.0 * distance * tan_half / height_points).max(0.0)
}

/// One frame's worth of input, in the camera's own units.
///
/// Plain data on purpose: the egui panel fills this in from pointer and key events, a test fills
/// it in by hand, and nothing here knows which.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gesture {
    /// Pointer travel since the last frame, in points.
    pub drag: Vec2,
    /// The pan modifier is held, so the drag moves the target instead of orbiting.
    pub panning: bool,
    /// Wheel notches since the last frame, in the same sense as egui's `zoom_delta`:
    /// positive zooms **in**.
    pub wheel: f64,
    /// Arrow-key presses since the last frame, as (−1 … 1) per axis.
    pub keys: Vec2,
    /// `+`/`-` presses since the last frame, in notches: positive zooms in.
    pub dolly_keys: f64,
    /// Frame the bounds the host passes in.
    pub frame: bool,
    /// Go back to the standard view.
    pub home: bool,
}

impl Gesture {
    /// A copy with every non-finite field replaced by zero, so one poisoned number from the input
    /// queue cannot reach the camera.
    pub fn finite(self) -> Gesture {
        let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
        let vec = |v: Vec2| Vec2::new(finite(v.x), finite(v.y));
        Gesture {
            drag: vec(self.drag),
            panning: self.panning,
            wheel: finite(self.wheel),
            keys: vec(self.keys),
            dolly_keys: finite(self.dolly_keys),
            frame: self.frame,
            home: self.home,
        }
    }

    /// Whether this gesture asks for nothing at all, so the panel can skip a frame's work.
    pub fn is_empty(&self) -> bool {
        let g = self.finite();
        !g.frame && !g.home && g.drag == Vec2::ZERO && g.keys == Vec2::ZERO && g.wheel == 0.0 && g.dolly_keys == 0.0
    }
}

impl OrbitCamera {
    /// Apply one frame of input and say whether the camera moved.
    ///
    /// `world_per_pixel` is the panel's answer to "how big is a pixel here"; `bounds` is the body
    /// to frame, needed only for [`Gesture::frame`]. Neither is read unless the gesture asks.
    pub fn apply(&mut self, gesture: Gesture, world_per_pixel: f64, bounds: Option<&Bounds3>) -> bool {
        let before = *self;
        let g = gesture.finite();
        if g.home {
            self.home();
        }
        if g.frame
            && let Some(b) = bounds
        {
            self.frame(b);
        }
        if g.drag != Vec2::ZERO {
            if g.panning {
                self.pan(g.drag.x, g.drag.y, world_per_pixel);
            } else {
                self.orbit(g.drag.x, g.drag.y);
            }
        }
        if g.keys != Vec2::ZERO {
            self.orbit_radians(g.keys.x * KEY_ORBIT_RADIANS, g.keys.y * KEY_ORBIT_RADIANS);
        }
        if g.wheel != 0.0 {
            // egui's wheel is positive for zoom in; `dolly` is positive for pull back.
            self.dolly(-g.wheel);
        }
        if g.dolly_keys != 0.0 {
            // `+`/`-` are notches in the same sense as the wheel: positive zooms in, while
            // `dolly` is positive for pull back.
            self.dolly(-g.dolly_keys);
        }
        self.sanitize();
        *self != before
    }
}

/// Where the pointer asked for a pick, in the pixels of a rendered image.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PickRequest {
    /// Horizontal pixel, `0` at the left edge, as [`cadcraft_render::picking`] counts them.
    pub px: f64,
    /// Vertical pixel, `0` at the **top** edge.
    pub py: f64,
    /// Width of the image the pixels belong to.
    pub width: f64,
    /// Height of the image.
    pub height: f64,
}

impl PickRequest {
    /// The request at `(px, py)` of a `width × height` image.
    pub fn new(px: f64, py: f64, width: f64, height: f64) -> PickRequest {
        PickRequest { px, py, width, height }
    }

    /// Every number finite, with a positive image to be a pixel of.
    pub fn is_valid(&self) -> bool {
        self.px.is_finite() && self.py.is_finite() && self.width.is_finite() && self.height.is_finite() && self.width > 0.0 && self.height > 0.0
    }
}

/// What the model currently has selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    /// Nothing.
    #[default]
    None,
    /// A feature in the tree, by name.
    Feature(String),
    /// A face of a solid, by index in the scene's bodies.
    Face { solid: usize, face: usize },
}

/// Everything the viewport remembers between frames.
///
/// The host owns this; the panel owns none of it. `pending_pick` is the seam between "the user
/// clicked" and "somebody worked out what was clicked": the panel sets it, the host drains it with
/// [`ViewportState::take_pick`] and resolves it with
/// [`cadcraft_render::picking::pick_scene`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportState {
    pub camera: OrbitCamera,
    /// A click waiting to be answered; `None` when there is nothing outstanding.
    pub pending_pick: Option<PickRequest>,
    pub selection: Selection,
}

impl ViewportState {
    /// A state with the default camera and nothing selected.
    pub fn new() -> ViewportState {
        ViewportState::default()
    }

    /// Ask for `request` to be answered. An invalid request (a zero-sized image, a NaN) is
    /// refused, and a second click replaces a first: the newest one is what the user meant.
    pub fn request_pick(&mut self, request: PickRequest) -> bool {
        if !request.is_valid() {
            return false;
        }
        self.pending_pick = Some(request);
        true
    }

    /// Take the outstanding pick, if any, leaving nothing behind.
    pub fn take_pick(&mut self) -> Option<PickRequest> {
        self.pending_pick.take()
    }

    /// Drop the outstanding pick without answering it.
    pub fn clear_pick(&mut self) {
        self.pending_pick = None;
    }

    /// Select something.
    pub fn select(&mut self, selection: Selection) {
        self.selection = selection;
    }

    /// Select nothing.
    pub fn clear_selection(&mut self) {
        self.selection = Selection::None;
    }

    /// The world ray `request` casts through the scene as this state currently sees it, or `None`
    /// when the request or the camera is unusable.
    pub fn pick_ray(&self, request: PickRequest) -> Option<Ray> {
        if !request.is_valid() {
            return None;
        }
        cadcraft_render::picking::ray_through_pixel(&self.camera.camera(), request.px, request.py, request.width, request.height)
    }

    /// How big a pixel of a panel `height_points` tall is, in world units.
    pub fn world_per_pixel(&self, height_points: f64) -> f64 {
        world_per_pixel(self.camera.distance, self.camera.fov_degrees, height_points)
    }

    /// Whether every number in this state is usable — the check a host makes before handing the
    /// state to anything that would divide by it.
    pub fn is_finite(&self) -> bool {
        let p = self.pending_pick.is_some_and(|p| p.is_valid());
        let s = match &self.selection {
            Selection::None => true,
            Selection::Feature(name) => !name.is_empty(),
            Selection::Face { .. } => true,
        };
        self.camera.is_valid() && p && s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn camera() -> OrbitCamera {
        OrbitCamera::default()
    }

    #[test]
    fn the_default_camera_is_a_camera_scene3_accepts() {
        let c = camera();
        assert!(c.is_valid(), "{c:?}");
        assert!(c.camera().is_valid());
        assert!(c.camera().eye.is_finite());
        assert!(c.eye().dist(c.target) > 0.0, "the eye is not on the target");
        assert!(c.near > 0.0 && c.far > c.near, "depth range {}..{}", c.near, c.far);
        assert!(c.fov_degrees > 0.0 && c.fov_degrees < 180.0);
    }

    #[test]
    fn the_eye_is_derived_and_never_equals_the_target() {
        let c = camera();
        let eye = c.eye();
        // The same state always gives the same eye, and the distance is what it says.
        assert!(near(eye.dist(c.target), c.distance, 1e-12));
        assert!(near((eye - c.target).len(), 1.0, 1e-12), "the direction is a unit vector");
        for (az, el) in [(0.0, 0.0), (1.2, -0.5), (-2.9, 0.9), (3.0, POLE_LIMIT), (0.0, -POLE_LIMIT)] {
            let mut c = camera();
            c.azimuth = az;
            c.elevation = el;
            c.sanitize();
            assert!(c.eye().dist(c.target) > 0.0, "azimuth {az}, elevation {el}");
            assert!(c.eye().is_finite());
        }
    }

    #[test]
    fn the_distance_never_reaches_zero_and_the_eye_never_passes_through_the_target() {
        let mut c = camera();
        // Dolly in far past the model, notch by notch and in one go.
        for _ in 0..400 {
            c.dolly(-1.0);
        }
        assert!(c.distance >= MIN_DISTANCE, "distance {}", c.distance);
        assert!(c.eye().dist(c.target) >= MIN_DISTANCE);
        assert!(c.camera().is_valid(), "{c:?}");
        c.dolly(-1e9);
        assert!(near(c.distance, MIN_DISTANCE, 0.0), "distance {}", c.distance);
        assert!(c.eye().dist(c.target) > 0.0);
        assert!(c.camera().is_valid());
        // And out past the other end.
        for _ in 0..400 {
            c.dolly(1.0);
        }
        assert!(near(c.distance, MAX_DISTANCE, 0.0), "distance {}", c.distance);
        assert!(c.camera().is_valid(), "{c:?}");
        c.dolly(1e9);
        assert!(near(c.distance, MAX_DISTANCE, 0.0));
        assert!(c.eye().is_finite() && c.target.is_finite());
    }

    #[test]
    fn a_hand_written_zero_distance_is_flagged_and_fixes_itself() {
        let mut c = camera();
        c.distance = 0.0;
        assert!(!c.is_valid(), "a camera on its target is not usable");
        // Nothing non-finite escapes even before it is fixed.
        assert!(c.eye().is_finite());
        assert!(c.camera().eye.is_finite());
        c.sanitize();
        assert!(c.is_valid());
        assert!(c.distance >= MIN_DISTANCE);
    }

    #[test]
    fn hostile_fields_never_reach_the_camera() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e308, 0.0, -0.0] {
            let mut c = camera();
            c.target = Vec3::new(bad, 0.0, 0.0);
            c.distance = bad;
            c.azimuth = bad;
            c.elevation = bad;
            c.fov_degrees = bad;
            c.sanitize();
            assert!(c.is_valid(), "{bad}: {c:?}");
            let cam = c.camera();
            assert!(cam.is_valid(), "{bad}: {cam:?}");
            assert!(cam.eye.is_finite() && cam.target.is_finite());
            // And the same state as a fresh one is stable.
            assert_eq!(c.sanitized(), c, "sanitize must be idempotent");
        }
        // A non-finite point is refused outright rather than adopted.
        let mut c = camera();
        let at = c.target;
        c.set_target(Vec3::new(f64::NAN, 1.0, 2.0));
        assert_eq!(c.target, at, "a NaN target was adopted");
    }

    #[test]
    fn the_elevation_stops_short_of_the_poles() {
        let mut c = camera();
        c.elevation = 0.0;
        for _ in 0..10_000 {
            c.orbit(0.0, 1.0);
        }
        assert!(near(c.elevation, POLE_LIMIT, 1e-12), "elevation {}", c.elevation);
        assert!(c.camera().is_valid(), "world +Z must stay a usable up hint");
        for _ in 0..20_000 {
            c.orbit(0.0, -1.0);
        }
        assert!(near(c.elevation, -POLE_LIMIT, 1e-12), "elevation {}", c.elevation);
        assert!(c.camera().is_valid());
    }

    #[test]
    fn the_azimuth_wraps_so_a_long_session_keeps_its_precision() {
        let mut c = camera();
        let start = c.azimuth;
        // A few hundred thousand pixels is a long drag, not a test artefact.
        for _ in 0..100_000 {
            c.orbit(1.0, 0.0);
        }
        assert!(c.azimuth.is_finite());
        assert!(c.azimuth >= -std::f64::consts::PI && c.azimuth < std::f64::consts::PI, "azimuth {}", c.azimuth);
        // A whole turn of pointer travel comes back to where it started, however it was chopped up.
        assert!(near(wrap_pi(0.0), 0.0, 1e-15));
        assert!(near(wrap_pi(std::f64::consts::TAU), 0.0, 1e-15));
        assert!(near(wrap_pi(-std::f64::consts::TAU * 3.0), 0.0, 1e-12));
        assert_eq!(wrap_pi(f64::NAN), 0.0);
        assert!(wrap_pi(100.0) < std::f64::consts::PI && wrap_pi(100.0) >= -std::f64::consts::PI);
        let start = c.azimuth;
        c.azimuth = start;
        c.orbit(std::f64::consts::TAU / ORBIT_RADIANS_PER_PIXEL, 0.0);
        assert!(near(wrap_pi(c.azimuth), start, 1e-12), "azimuth {} from {start}", c.azimuth);
    }

    #[test]
    fn a_gesture_does_not_depend_on_how_the_frames_fell() {
        let wpp = world_per_pixel(10.0, 45.0, 600.0);
        assert!(wpp > 0.0);

        // Orbit: one drag of 40 pixels, and forty one-pixel frames of it.
        let mut whole = camera();
        whole.orbit(40.0, -17.0);
        let mut split = camera();
        for _ in 0..40 {
            split.orbit(1.0, 0.0);
        }
        for _ in 0..17 {
            split.orbit(0.0, -1.0);
        }
        assert!(near(whole.azimuth, split.azimuth, 1e-12), "{} vs {}", whole.azimuth, split.azimuth);
        assert!(near(whole.elevation, split.elevation, 1e-12));

        // Pan is linear, so it splits exactly.
        let mut whole = camera();
        whole.pan(120.0, 45.0, wpp);
        let mut split = camera();
        for _ in 0..120 {
            split.pan(1.0, 0.0, wpp);
        }
        for _ in 0..45 {
            split.pan(0.0, 1.0, wpp);
        }
        assert!(whole.target.near(split.target, 1e-9), "{:?} vs {:?}", whole.target, split.target);

        // Dolly is multiplicative: three notches, once or three times.
        let mut whole = camera();
        whole.dolly(3.0);
        let mut split = camera();
        for _ in 0..3 {
            split.dolly(1.0);
        }
        assert!(near(whole.distance, split.distance, 1e-9 * whole.distance), "{} vs {}", whole.distance, split.distance);

        // And the whole gesture applied through `apply`, split and whole, agrees the same way.
        let mut whole = camera();
        whole.apply(Gesture { drag: Vec2::new(30.0, -12.0), wheel: -2.0, ..Gesture::default() }, wpp, None);
        let mut split = camera();
        for _ in 0..30 {
            split.apply(Gesture { drag: Vec2::new(1.0, 0.0), ..Gesture::default() }, wpp, None);
        }
        for _ in 0..12 {
            split.apply(Gesture { drag: Vec2::new(0.0, -1.0), ..Gesture::default() }, wpp, None);
        }
        for _ in 0..2 {
            split.apply(Gesture { wheel: -1.0, ..Gesture::default() }, wpp, None);
        }
        assert!(near(whole.azimuth, split.azimuth, 1e-9), "{} vs {}", whole.azimuth, split.azimuth);
        assert!(near(whole.elevation, split.elevation, 1e-9));
        assert!(near(whole.distance, split.distance, 1e-9 * whole.distance), "{} vs {}", whole.distance, split.distance);
    }

    #[test]
    fn dolly_splits_however_it_is_cut() {
        let mut c = camera();
        let base = c.distance;
        c.dolly(5.0);
        let all_at_once = c.distance;
        let mut d = camera();
        for n in [1.0, 1.0, 1.0, 1.0, 1.0] {
            d.dolly(n);
        }
        assert!(near(all_at_once, d.distance, 1e-9 * all_at_once), "{all_at_once} vs {}", d.distance);
        // Three notches in is three times one notch in, to within a rounding bit.
        c.dolly(-5.0);
        assert!(near(c.distance, base, 1e-9 * base), "{} vs {base}", c.distance);
    }

    #[test]
    fn dolly_refuses_rather_than_propagating_a_hostile_number() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e308] {
            let mut c = camera();
            let before = c.distance;
            c.dolly(bad);
            assert_eq!(c.distance, before, "{bad} changed the distance");
            assert!(c.is_valid());
            // Same through the gesture, which is where a poisoned number really arrives.
            let mut d = camera();
            let at = d.distance;
            d.apply(Gesture { wheel: bad, ..Gesture::default() }, 0.0, None);
            assert_eq!(d.distance, at, "{bad} changed the distance");
        }
    }

    #[test]
    fn pan_needs_a_pixel_size_and_moves_the_target_against_the_drag() {
        let mut c = camera();
        let (right, up) = c.screen_axes();
        assert!(near(right.len(), 1.0, 1e-12) && near(up.len(), 1.0, 1e-12));
        let before = c.target;
        // Dragging right moves the model right, which moves the point we look at left.
        c.pan(10.0, 0.0, 0.5);
        assert!(c.target.near(before - right * 5.0, 1e-9), "{:?}", c.target);
        c.pan(0.0, 10.0, 0.5);
        // Dragging down moves the model down, which moves the target up the screen.
        let moved = c.target - (before - right * 5.0);
        assert!(moved.near(up * 5.0, 1e-9), "{moved:?}");
        // A pan that cannot be sized does nothing.
        for wpp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let at = c.target;
            c.pan(10.0, 10.0, wpp);
            assert_eq!(c.target, at, "{wpp} moved the target");
            assert!(c.target.is_finite());
        }
        let at = c.target;
        c.pan(f64::NAN, 0.0, 1.0);
        assert_eq!(c.target, at);
    }

    #[test]
    fn world_per_pixel_scales_with_the_distance_and_refuses_the_impossible() {
        let a = world_per_pixel(10.0, 45.0, 600.0);
        let b = world_per_pixel(20.0, 45.0, 600.0);
        assert!(near(b, 2.0 * a, 1e-12), "{a} then {b}");
        let tall = world_per_pixel(10.0, 45.0, 300.0);
        assert!(near(tall, 2.0 * a, 1e-12));
        for bad in [(0.0, 45.0, 600.0), (10.0, 0.0, 600.0), (10.0, 200.0, 600.0), (10.0, 45.0, 0.0), (f64::NAN, 45.0, 600.0), (-1.0, 45.0, 600.0)] {
            assert_eq!(world_per_pixel(bad.0, bad.1, bad.2), 0.0, "{bad:?}");
        }
    }

    #[test]
    fn framing_fits_the_body_and_keeps_the_view_direction() {
        let mut c = camera();
        c.azimuth = 0.7;
        c.elevation = -0.3;
        let bounds = Bounds3::new(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0));
        assert!(c.frame(&bounds));
        assert!(c.target.near(Vec3::ZERO, 1e-12), "{:?}", c.target);
        assert!(near(c.azimuth, 0.7, 1e-15) && near(c.elevation, -0.3, 1e-15), "the direction was kept");
        // Far enough that every corner of the body is inside the frustum.
        let half = 10.0 * 3f64.sqrt();
        assert!(c.distance >= half / (45.0f64.to_radians() * 0.5).tan(), "distance {}", c.distance);
        assert!(c.camera().is_valid());
        // A body far from the origin moves the target to it.
        let away = Bounds3::new(Vec3::new(1000.0, 2000.0, 0.0), Vec3::new(1010.0, 2010.0, 5.0));
        assert!(c.frame(&away));
        assert!(c.target.near(Vec3::new(1005.0, 2005.0, 2.5), 1e-9), "{:?}", c.target);
    }

    #[test]
    fn framing_nothing_leaves_the_camera_where_the_user_left_it() {
        let mut c = camera();
        c.orbit(23.0, -7.0);
        c.pan(11.0, 4.0, 0.25);
        let before = c;
        assert!(!c.frame(&Bounds3::EMPTY));
        assert_eq!(c, before, "an empty frame must not throw the camera away");
        // `Bounds3::new` would swallow the NaN (`f64::min` ignores it), so build it outright.
        let nan = Bounds3 { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::X };
        assert!(!c.frame(&nan));
        assert_eq!(c, before);
        // A single point has no size; it still frames to something usable rather than to zero.
        assert!(c.frame(&Bounds3::new(Vec3::new(5.0, 5.0, 5.0), Vec3::new(5.0, 5.0, 5.0))));
        assert!(c.is_valid(), "{c:?}");
        assert!(c.distance > 0.0);
    }

    #[test]
    fn the_depth_range_follows_the_zoom() {
        let mut c = camera();
        let near_before = c.near;
        let far_before = c.far;
        c.dolly(-20.0);
        assert!(c.near < near_before && c.far < far_before, "{}..{} then {}..{}", near_before, far_before, c.near, c.far);
        assert!(c.far > c.near);
        assert!(c.camera().validate().is_ok(), "{:?}", c.camera().validate());
        // Even at both ends of the distance range the range is finite and ordered.
        for d in [MIN_DISTANCE, 1.0, 1e6, MAX_DISTANCE] {
            c.set_distance(d);
            assert!(c.near.is_finite() && c.far.is_finite(), "distance {d}: {}..{}", c.near, c.far);
            assert!(c.near > 0.0 && c.far > c.near, "distance {d}: {}..{}", c.near, c.far);
            assert!(c.camera().is_valid());
        }
    }

    #[test]
    fn home_returns_to_the_standard_view_and_keeps_the_field_of_view() {
        let mut c = camera();
        c.fov_degrees = 30.0;
        c.orbit(200.0, 100.0);
        c.pan(50.0, 50.0, 1.0);
        c.dolly(4.0);
        c.home();
        assert!(near(c.fov_degrees, 30.0, 1e-12), "the field of view is the user's choice");
        assert_eq!(c, OrbitCamera { fov_degrees: 30.0, ..OrbitCamera::default() });
        assert!(c.is_valid());
    }

    #[test]
    fn a_gesture_says_whether_it_wants_anything() {
        assert!(Gesture::default().is_empty());
        assert!(Gesture { wheel: f64::NAN, ..Gesture::default() }.is_empty(), "a poisoned number is not a gesture");
        for g in [
            Gesture { drag: Vec2::new(1.0, 0.0), ..Gesture::default() },
            Gesture { wheel: 0.5, ..Gesture::default() },
            Gesture { keys: Vec2::new(0.0, -1.0), ..Gesture::default() },
            Gesture { dolly_keys: 1.0, ..Gesture::default() },
            Gesture { frame: true, ..Gesture::default() },
            Gesture { home: true, ..Gesture::default() },
        ] {
            assert!(!g.is_empty(), "{g:?}");
            let finite = g.finite();
            assert!(finite.drag.x.is_finite() && finite.drag.y.is_finite(), "{g:?}");
            assert!(finite.keys.x.is_finite() && finite.keys.y.is_finite(), "{g:?}");
            assert!(finite.wheel.is_finite() && finite.dolly_keys.is_finite(), "{g:?}");
        }
        // The pan modifier alone is not a gesture: nothing has moved.
        assert!(Gesture { panning: true, ..Gesture::default() }.is_empty());
    }

    #[test]
    fn a_gesture_only_frames_when_the_host_gave_it_a_body() {
        let bounds = Bounds3::new(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let mut c = camera();
        c.orbit(10.0, 0.0);
        // No bounds: the request cannot be carried out, and the camera stays where the user left it.
        let at = c;
        assert!(!c.apply(Gesture { frame: true, ..Gesture::default() }, 0.0, None));
        assert_eq!(c, at, "framing nothing moved the camera");
        let mut with_bounds = camera();
        assert!(with_bounds.apply(Gesture { frame: true, ..Gesture::default() }, 0.0, Some(&bounds)));
        assert!(with_bounds.target.near(Vec3::new(2.0, 2.0, 2.0), 1e-9), "{:?}", with_bounds.target);
    }

    #[test]
    fn an_outstanding_pick_is_answered_once_and_the_newest_click_wins() {
        let mut state = ViewportState::new();
        assert!(state.pending_pick.is_none());
        assert!(!state.request_pick(PickRequest::new(f64::NAN, 1.0, 10.0, 10.0)), "a poisoned pixel");
        assert!(!state.request_pick(PickRequest::new(1.0, 1.0, 0.0, 10.0)), "a zero-width image");
        assert!(state.pending_pick.is_none(), "an invalid request was queued anyway");
        assert!(state.request_pick(PickRequest::new(1.0, 1.0, 10.0, 10.0)));
        assert!(state.request_pick(PickRequest::new(7.0, 3.0, 10.0, 10.0)));
        assert_eq!(state.take_pick(), Some(PickRequest::new(7.0, 3.0, 10.0, 10.0)), "the newest click");
        assert!(state.take_pick().is_none(), "a pick is answered once");
        state.request_pick(PickRequest::new(1.0, 1.0, 10.0, 10.0));
        state.clear_pick();
        assert!(state.take_pick().is_none(), "a cleared pick is not answered");
    }

    #[test]
    fn a_pick_request_becomes_a_ray_through_the_scene() {
        let mut state = ViewportState::new();
        state.camera = OrbitCamera::looking_at(Vec3::ZERO, 20.0);
        // Dead centre, 64 x 48: the ray runs along the view direction.
        let r = state.pick_ray(PickRequest::new(32.0, 24.0, 64.0, 48.0)).expect("a ray");
        assert!(r.dir.near(state.camera.camera().target - state.camera.eye(), 1e-9), "{:?}", r.dir);
        // Above centre, the ray tilts up; a ray with no direction is refused.
        let up = state.pick_ray(PickRequest::new(32.0, 4.0, 64.0, 48.0)).expect("a ray");
        assert!(up.dir.z > r.dir.z, "{:?} vs {:?}", up.dir, r.dir);
        assert!(state.pick_ray(PickRequest::new(1.0, 1.0, 0.0, 48.0)).is_none());
        assert!(state.pick_ray(PickRequest::new(f64::NAN, 1.0, 64.0, 48.0)).is_none());
        // A camera that cannot see anything has no ray.
        state.camera.distance = 0.0;
        state.camera.fov_degrees = 0.0;
        assert!(state.pick_ray(PickRequest::new(32.0, 24.0, 64.0, 48.0)).is_none());
    }

    #[test]
    fn the_selection_is_whatever_the_host_last_set() {
        let mut state = ViewportState::new();
        assert_eq!(state.selection, Selection::None);
        state.select(Selection::Feature("Body1".into()));
        assert_eq!(state.selection, Selection::Feature("Body1".into()));
        assert!(state.is_finite());
        state.select(Selection::Face { solid: 0, face: 3 });
        assert!(state.is_finite());
        state.select(Selection::None);
        state.clear_selection();
        assert_eq!(state.selection, Selection::None);
        // A camera that has been corrupted, or a queued request that is not one, is not finite.
        state.camera.distance = 0.0;
        assert!(!state.is_finite());
        state.camera = camera();
        state.pending_pick = Some(PickRequest::new(f64::INFINITY, 0.0, 1.0, 1.0));
        assert!(!state.is_finite());
        state.clear_pick();
        assert!(state.is_finite());
    }

    #[test]
    fn the_viewport_state_starts_from_a_camera_scene3_accepts() {
        let state = ViewportState::new();
        assert!(state.is_finite(), "{state:?}");
        assert!(state.camera.is_valid());
        assert_eq!(state.pending_pick, None);
        assert_eq!(state.selection, Selection::None);
        assert!(state.world_per_pixel(600.0) > 0.0);
    }
}
