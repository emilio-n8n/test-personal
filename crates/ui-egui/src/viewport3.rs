//! The 3D viewport panel: the `scene3` picture in an egui widget, and the pointer and keyboard
//! that move the camera.
//!
//! # It owns nothing
//!
//! The panel is a function over data the host owns. It is handed the [`Scene`] the picture was
//! drawn from, the [`Frame`] that was rasterised for it, and the [`ViewportState`] to fold this
//! frame's input into; it hands back egui's own [`egui::Response`] plus a [`ViewportDelta`] saying
//! what the host has to do as a result. It reads no document, holds no model, keeps no selection of
//! its own, and runs no command. A host that disagrees with any of that can still use it, because
//! there is nothing else to disagree with.
//!
//! # The picture the host hands over
//!
//! Drawing is [`cadcraft_render::scene3`]'s CPU rasteriser, which is a validation renderer and says
//! in its own docs that it is not built for interactive redraw. So the host rasterises when the
//! camera or the model changes — on a worker thread, if it has one — and hands the result back as
//! a [`Frame`]. The picture therefore lags the camera by one frame, which is what every
//! incremental renderer does, and [`ViewportDelta::camera_changed`] is the host's cue to render the
//! next one.
//!
//! # Interaction
//!
//! - **Left drag** orbits, **middle or right drag** pans, **wheel** dollies. The deltas reach
//!   [`crate::viewport_state::OrbitCamera`] as pixels and notches, which is what makes the result
//!   independent of the frame rate; nothing here has momentum.
//! - A **click** — a press and release with no drag — queues a [`PickRequest`] in the state and says
//!   so in the delta. Resolving it is the host's job, because the host owns the scene and the model.
//! - With the panel focused: **arrows** orbit, **`+`/`-`** dolly, **`F`** frames the body, **Home**
//!   returns to the standard view. Without focus the panel takes no keys, so the command line and
//!   the drawing keep them.
//!
//! Every egui call this file makes is here. The camera arithmetic and its limits live in
//! `viewport_state.rs`, which has no egui in it and is unit-tested without a window.

use cadcraft_geom::Vec2;
use cadcraft_render::scene3::{Image, Scene};

use crate::theme::Tokens;
use crate::viewport_state::{Gesture, PickRequest, ViewportState};

/// egui memory key of the uploaded picture.
const FRAME_ID: &str = "cadcraft_viewport3_frame";

/// The smallest the panel will draw, so a collapsed dock still shows something.
const MIN_PANEL: egui::Vec2 = egui::Vec2::new(160.0, 120.0);

/// The box each view button is placed in, in points.
const BUTTON: egui::Vec2 = egui::Vec2::new(56.0, 22.0);

/// How far the view buttons keep from the picture's right edge, and from each other.
const BUTTON_GAP: f32 = 6.0;

/// A rasterised picture and the identity the host gives it.
///
/// `revision` is the host's word for "this is a different picture": bump it whenever the camera or
/// the model changed and produced new pixels. The panel uses it to decide whether to upload, and
/// uploads once per revision rather than once per frame — a per-frame upload of a 900 × 700 RGBA
/// buffer asks the GPU to re-take 2.5 MB forty times a second for a picture that did not change.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Bumped by the host whenever `image` is a different picture.
    pub revision: u64,
    /// The picture, as [`cadcraft_render::scene3::render`] wrote it: RGBA8, row 0 at the top.
    pub image: Image,
}

impl Default for Frame {
    /// No picture at all: revision zero and a zero-sized image. [`cadcraft_render::scene3::Image`]
    /// has no `Default` of its own — an image is always something the renderer wrote — so the
    /// empty frame is built here, and [`Frame::is_empty`] is what reports it.
    fn default() -> Frame {
        Frame { revision: 0, image: Image { width: 0, height: 0, data: Vec::new() } }
    }
}

impl Frame {
    /// The picture, with the identity the host gives it.
    pub fn new(revision: u64, image: Image) -> Frame {
        Frame { revision, image }
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.image.width == 0 || self.image.height == 0
    }

    /// The picture's size in `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        (self.image.width, self.image.height)
    }
}

/// What one frame of the viewport asks its host to do.
///
/// Everything here is a *request*. The panel has rasterised nothing and picked nothing; it is
/// telling the host what it would like done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewportDelta {
    /// The camera moved: the picture has to be rasterised again.
    pub camera_changed: bool,
    /// A click queued a pick, which is waiting in [`ViewportState::pending_pick`].
    pub pick_requested: bool,
}

impl ViewportDelta {
    /// Whether this frame asked the host for anything at all.
    pub fn is_empty(&self) -> bool {
        !self.camera_changed && !self.pick_requested
    }
}

/// Draw the viewport, fold this frame's input into `state`, and say what the host should do.
///
/// `scene` is what the picture was rasterised from — the panel needs it for its bounds, which is
/// what `F` frames. `frame` is the picture; an empty one draws the empty-scene hint instead and
/// takes no clicks, because a click on nothing to hit is not a pick.
pub fn show(ui: &mut egui::Ui, scene: &Scene, frame: &Frame, state: &mut ViewportState) -> (egui::Response, ViewportDelta) {
    let t = Tokens::get();
    let size = ui.available_size().max(MIN_PANEL);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());

    // A click takes the keyboard, and the keyboard is only ours while we have it.
    let wants_keys = response.clicked() || ui.memory(|m| m.has_focus(response.id));
    if response.clicked() {
        response.request_focus();
    }
    let mut gesture = gesture_of(ui, &response, wants_keys);

    {
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, t.canvas);
        if let Some(texture) = texture(ui.ctx(), frame) {
            p.image(texture.id(), rect, whole_texture(), egui::Color32::WHITE);
        }
        p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    }

    // The view buttons, over the picture.
    buttons(ui, rect, &mut gesture);
    let panning = is_panning(&response);

    // Fold the input in. The picture is already drawn, so the frame lags the camera by one.
    let per_pixel = state.world_per_pixel(f64::from(rect.height()));
    let camera_changed = state.camera.apply(gesture, per_pixel, Some(&scene.bounds()));

    // A click queues a pick, in the picture's pixels rather than the panel's points.
    let mut pick_requested = false;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((sx, sy)) = pixel_scale(rect, frame)
    {
        pick_requested = state.request_pick(PickRequest::new(
            f64::from(pos.x - rect.left()) * sx,
            f64::from(pos.y - rect.top()) * sy,
            f64::from(frame.image.width),
            f64::from(frame.image.height),
        ));
    }

    if scene.solids.is_empty() {
        hint(ui, rect, crate::tl!("No solid to show"));
    }

    let cursor = cursor_of(&response, panning);
    (response.on_hover_cursor(cursor), ViewportDelta { camera_changed, pick_requested })
}

/// Whether a drag on the panel is a pan rather than an orbit: the middle or the right button is
/// the one dragging. egui answers that per button through
/// [`egui::Response::dragged_by`], so a pan is only claimed once the pointer has actually moved —
/// which is exactly when a drag is a drag and not a click.
fn is_panning(response: &egui::Response) -> bool {
    response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary)
}

/// This frame's input, in the camera's own units.
fn gesture_of(ui: &egui::Ui, response: &egui::Response, wants_keys: bool) -> Gesture {
    let mut gesture = Gesture::default();
    if response.dragged() {
        let d = response.drag_delta();
        gesture.drag = Vec2::new(f64::from(d.x), f64::from(d.y));
        gesture.panning = is_panning(response);
    }
    // The wheel belongs to the panel whenever the pointer is over it, focused or not: a person
    // scrolls over the thing they are looking at. A wheel anywhere else in the panel belongs to
    // whatever is under the pointer there. Read from the raw wheel events, as the 2D canvas
    // does: egui's smoothed delta spreads a notch over several frames, and `zoom_delta` is the
    // pinch factor (1.0 when idle), not the wheel.
    gesture.wheel = if response.hovered() {
        let opts = ui.ctx().options(|o| o.input_options);
        ui.input(|i| wheel_notches(&i.raw.events, &opts, response.rect.height()))
    } else {
        0.0
    };
    if wants_keys {
        use egui::Key;
        ui.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                gesture.keys.x -= 1.0;
            }
            if i.key_pressed(Key::ArrowRight) {
                gesture.keys.x += 1.0;
            }
            if i.key_pressed(Key::ArrowUp) {
                gesture.keys.y += 1.0;
            }
            if i.key_pressed(Key::ArrowDown) {
                gesture.keys.y -= 1.0;
            }
            if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                gesture.dolly_keys += 1.0;
            }
            if i.key_pressed(Key::Minus) {
                gesture.dolly_keys -= 1.0;
            }
            if i.key_pressed(Key::F) {
                gesture.frame = true;
            }
            if i.key_pressed(Key::Home) {
                gesture.home = true;
            }
        });
    }
    gesture
}

/// This frame's wheel input in notches, from the raw wheel events: one `Line` step is one
/// notch, however egui smooths it. Units convert to points as egui does, and events egui turns
/// into a pinch zoom or a sideways scroll are left out, as it does. Negated into the camera's
/// sense (positive zooms in): a notch rolled away from the user pulls the eye back, which is
/// what this panel's own tests pin (see `docs/agents/questions/Q-ui-wheel-sign.md`).
fn wheel_notches(events: &[egui::Event], opts: &egui::InputOptions, page: f32) -> f64 {
    let scale = opts.line_scroll_speed;
    if !scale.is_finite() || scale <= 0.0 {
        return 0.0;
    }
    let points: f32 = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. }
                if !modifiers.matches_any(opts.zoom_modifier) && !modifiers.matches_any(opts.horizontal_scroll_modifier) =>
            {
                let per = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => scale,
                    egui::MouseWheelUnit::Page => page,
                };
                Some(delta.y * per)
            }
            _ => None,
        })
        .sum();
    if points.is_finite() { -f64::from(points / scale) } else { 0.0 }
}

/// How many image pixels one point of the panel is, on each axis, or `None` when there is no
/// picture to map onto.
fn pixel_scale(rect: egui::Rect, frame: &Frame) -> Option<(f64, f64)> {
    if frame.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    Some((f64::from(frame.image.width) / f64::from(rect.width()), f64::from(frame.image.height) / f64::from(rect.height())))
}

/// The pointer shape that says what a drag will do.
fn cursor_of(response: &egui::Response, panning: bool) -> egui::CursorIcon {
    // egui has no separate closed-hand icon: `Grabbing` is the closed hand, whether the drag
    // orbits or pans. The hover icons below are what says which one it will do.
    if response.dragged() {
        return egui::CursorIcon::Grabbing;
    }
    if response.hovered() {
        return if panning { egui::CursorIcon::Move } else { egui::CursorIcon::Grab };
    }
    egui::CursorIcon::Default
}

/// The `Home` and `Fit` buttons, over the top right of the picture.
///
/// **Placed, not laid out.** [`egui::Ui::place`] puts each button in a rectangle this file chooses,
/// so the two of them sit over the picture without asking egui for any layout space — a viewport
/// panel has none to give. The rectangles are fixed-size, which is why a translated button is wider
/// than its box: the button sizes itself to its text and the box only says where it starts.
fn buttons(ui: &mut egui::Ui, rect: egui::Rect, gesture: &mut Gesture) {
    let margin = rect.left() + BUTTON_GAP;
    let right = (rect.right() - BUTTON_GAP - BUTTON.x).max(margin);
    let left = (right - BUTTON.x - BUTTON_GAP).max(margin);
    let top = rect.top() + BUTTON_GAP;

    let home = ui.place(egui::Rect::from_min_size(egui::pos2(right, top), BUTTON), egui::Button::new(crate::tl!("Home")));
    if home.on_hover_text(crate::tl!("Standard view")).clicked() {
        gesture.home = true;
    }
    let fit = ui.place(egui::Rect::from_min_size(egui::pos2(left, top), BUTTON), egui::Button::new(crate::tl!("Fit")));
    if fit.on_hover_text(crate::tl!("Frame the model")).clicked() {
        gesture.frame = true;
    }
}

/// What the panel says when there is no body to look at.
fn hint(ui: &egui::Ui, rect: egui::Rect, text: &str) {
    let t = Tokens::get();
    ui.painter_at(rect).text(rect.center(), egui::Align2::CENTER_CENTER, text.to_owned(), crate::theme::body(), t.text_faint);
}

/// The uv rectangle a whole texture is drawn from: the unit square, `0..1` on both axes.
fn whole_texture() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
}

/// The uploaded picture, kept in egui memory.
#[derive(Clone)]
struct Cached {
    revision: u64,
    size: [usize; 2],
    texture: egui::TextureHandle,
}

/// The texture for `frame`, uploading it only when the host's revision or the size changed.
fn texture(ctx: &egui::Context, frame: &Frame) -> Option<egui::TextureHandle> {
    let w = usize::try_from(frame.image.width).ok()?;
    let h = usize::try_from(frame.image.height).ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    let id = egui::Id::new(FRAME_ID);
    let cached = ctx.data(|d| d.get_temp::<Cached>(id));
    if let Some(c) = cached
        && c.revision == frame.revision
        && c.size == [w, h]
    {
        return Some(c.texture);
    }
    // A hand-built frame whose buffer is not `w × h × 4` uploads nothing: epaint asserts
    // the exact size, and a panic here would take the session's drawing with it.
    if frame.image.data.len() != w.checked_mul(h).and_then(|p| p.checked_mul(4)).unwrap_or(usize::MAX) {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &frame.image.data);
    let texture = ctx.load_texture(FRAME_ID, image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, Cached { revision: frame.revision, size: [w, h], texture: texture.clone() }));
    Some(texture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;
    use cadcraft_render::scene3::{Camera, Rgba};
    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// The panel's world: exactly what a host would own, and nothing the panel keeps for itself.
    struct Fixture {
        state: ViewportState,
        scene: Scene,
        frame: Frame,
        delta: ViewportDelta,
        rect: Option<Rect>,
    }

    impl Fixture {
        fn new() -> Fixture {
            let state = ViewportState::new();
            let scene = Scene::new(state.camera.camera());
            let image = Image::new(64, 48, Rgba::new(0.1, 0.11, 0.13)).expect("a small background image");
            Fixture { state, scene, frame: Frame::new(1, image), delta: ViewportDelta::default(), rect: None }
        }
    }

    fn harness() -> Harness<'static, Fixture> {
        let mut h = Harness::builder().with_size(vec2(420.0, 340.0)).build_ui_state(
            |ui, fx: &mut Fixture| {
                let (response, delta) = show(ui, &fx.scene, &fx.frame, &mut fx.state);
                fx.rect = Some(response.rect);
                // A host answers a request as soon as it sees one, so the delta it reads is the
                // newest request or an earlier one it has not answered yet — never a lost one.
                fx.delta.camera_changed |= delta.camera_changed;
                fx.delta.pick_requested |= delta.pick_requested;
            },
            Fixture::new(),
        );
        h.run_steps(3);
        h
    }

    /// The panel's rectangle in the window.
    fn rect(h: &Harness<'static, Fixture>) -> Rect {
        h.state().rect.expect("the panel drew itself")
    }

    fn press(h: &mut Harness<'static, Fixture>, at: Pos2, button: PointerButton) {
        let m = Modifiers::default();
        h.event(Event::PointerMoved(at));
        h.event(Event::PointerButton { pos: at, button, pressed: true, modifiers: m });
        h.event(Event::PointerButton { pos: at, button, pressed: false, modifiers: m });
        h.run_steps(2);
    }

    /// A real drag: press, eight moves, release — the way the control channel and a real pointer
    /// deliver one.
    fn drag(h: &mut Harness<'static, Fixture>, from: Pos2, to: Pos2, button: PointerButton) {
        let m = Modifiers::default();
        h.event(Event::PointerMoved(from));
        h.event(Event::PointerButton { pos: from, button, pressed: true, modifiers: m });
        for i in 1..=8 {
            h.event(Event::PointerMoved(from.lerp(to, f32::from(i) / 8.0)));
        }
        h.event(Event::PointerButton { pos: to, button, pressed: false, modifiers: m });
        h.run_steps(2);
    }

    fn wheel(h: &mut Harness<'static, Fixture>, notches: f32) {
        // The panel takes the wheel only while the pointer is over the picture, which is where a
        // person scrolls anyway — so put it there first, as a person would have.
        let at = rect(h).center();
        h.event(Event::PointerMoved(at));
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, notches),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::default(),
        });
        h.run_steps(2);
    }

    fn key(h: &mut Harness<'static, Fixture>, k: egui::Key) {
        h.event(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
        h.run_steps(2);
    }

    /// Every string the frame painted, so a label can be looked for without an accesskit query.
    fn painted(h: &Harness<'static, Fixture>) -> String {
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(s) => {
                    text.push_str(s.galley.text());
                    text.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for shape in &h.output().shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn the_panel_draws_and_the_camera_it_describes_is_one_scene3_accepts() {
        let h = harness();
        let r = rect(&h);
        assert!(r.width() > 0.0 && r.height() > 0.0, "{r:?}");
        assert!(h.state().delta.is_empty(), "nothing asked for anything: {:?}", h.state().delta);
        assert!(h.state().state.is_finite(), "{:?}", h.state().state);
        assert!(h.state().state.camera.camera().is_valid());
    }

    #[test]
    fn a_left_drag_orbits_and_the_delta_asks_for_a_redraw() {
        let mut h = harness();
        let before = h.state().state.camera;
        let c = rect(&h).center();
        drag(&mut h, c, c + vec2(40.0, 0.0), PointerButton::Primary);
        let after = h.state().state.camera;
        assert!(after.azimuth != before.azimuth, "the drag did not orbit: {before:?} then {after:?}");
        assert!(after.distance == before.distance, "a left drag orbits, it does not dolly");
        assert!(after.target == before.target, "a left drag orbits, it does not pan");
        assert!(h.state().delta.camera_changed, "{:?}", h.state().delta);
        assert!(h.state().state.pending_pick.is_none(), "a drag is not a click");
    }

    #[test]
    fn a_middle_drag_pans_instead_of_orbiting() {
        let mut h = harness();
        let before = h.state().state.camera;
        let c = rect(&h).center();
        drag(&mut h, c, c + vec2(30.0, 20.0), PointerButton::Middle);
        let after = h.state().state.camera;
        assert!(after.target != before.target, "the drag did not pan: {before:?} then {after:?}");
        assert!(after.azimuth == before.azimuth, "a middle drag pans, it does not orbit");
        assert!(after.distance == before.distance, "a middle drag pans, it does not dolly");
        assert!(after.target.is_finite());
    }

    #[test]
    fn the_wheel_dollies_and_stops_at_the_limits() {
        let mut h = harness();
        let start = h.state().state.camera.distance;
        wheel(&mut h, 1.0);
        assert!(h.state().state.camera.distance > start, "a notch in moves the eye away: {}", h.state().state.camera.distance);
        wheel(&mut h, -1.0);
        let back = h.state().state.camera.distance;
        assert!(back < start, "a notch out brings it closer: {back}");
        // Drive it far past the model: the distance clamps and the eye never reaches the target.
        for _ in 0..200 {
            wheel(&mut h, 1.0);
        }
        let c = h.state().state.camera;
        assert!(c.distance >= crate::viewport_state::MIN_DISTANCE, "distance {}", c.distance);
        assert!(c.eye().dist(c.target) > 0.0, "the eye reached the target");
        assert!(c.camera().is_valid(), "{c:?}");
        for _ in 0..300 {
            wheel(&mut h, -1.0);
        }
        let c = h.state().state.camera;
        assert!(c.distance <= crate::viewport_state::MAX_DISTANCE, "distance {}", c.distance);
        assert!(c.eye().dist(c.target) > 0.0);
        assert!(c.camera().is_valid(), "{c:?}");
    }

    #[test]
    fn a_wheel_away_from_the_picture_is_not_the_viewports() {
        let mut h = harness();
        let start = h.state().state.camera;
        // The pointer leaves the picture, so the notch belongs to whatever is under it there.
        let r = rect(&h);
        h.event(Event::PointerMoved(egui::pos2(r.right() + 4.0, r.bottom() + 4.0)));
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::default(),
        });
        h.run_steps(2);
        assert_eq!(h.state().state.camera, start, "a wheel outside the picture moved the camera");
        assert!(h.state().delta.is_empty(), "{:?}", h.state().delta);
    }

    #[test]
    fn a_click_queues_a_pick_the_host_can_answer() {
        let mut h = harness();
        assert!(h.state().state.pending_pick.is_none());
        let r = rect(&h);
        let at = r.center();
        press(&mut h, at, PointerButton::Primary);
        assert!(h.state().delta.pick_requested, "{:?}", h.state().delta);
        let request = h.state().state.pending_pick.expect("the click queued a pick");
        assert!(request.is_valid(), "{request:?}");
        // The request is in the picture's pixels, not the panel's points, and inside the picture.
        assert!(request.px > 0.0 && request.px < f64::from(h.state().frame.image.width), "{request:?}");
        assert!(request.py > 0.0 && request.py < f64::from(h.state().frame.image.height), "{request:?}");
        assert_eq!(request.width, f64::from(h.state().frame.image.width));
        // The host drains it once and resolves it against the scene it owns.
        assert_eq!(h.state().state.take_pick(), Some(request));
        assert!(h.state().state.take_pick().is_none(), "a pick is answered once");
        assert!(h.state().state.pick_ray(request).is_some(), "the request has to be answerable");
    }

    #[test]
    fn a_click_with_no_picture_to_hit_is_not_a_pick() {
        let mut h = harness();
        h.state_mut().frame = Frame::default();
        h.run_steps(2);
        press(&mut h, rect(&h).center(), PointerButton::Primary);
        assert!(h.state().state.pending_pick.is_none(), "a click on an empty frame queued a pick");
        assert!(!h.state().delta.pick_requested);
    }

    #[test]
    fn the_keys_are_only_ours_while_the_panel_has_them() {
        let mut h = harness();
        let before = h.state().state.camera;
        // No focus yet: the arrow keys belong to whatever else is listening.
        key(&mut h, egui::Key::ArrowLeft);
        key(&mut h, egui::Key::Minus);
        assert_eq!(h.state().state.camera, before, "an unfocused viewport took the keyboard");
        // After a click the panel has the keys.
        press(&mut h, rect(&h).center(), PointerButton::Primary);
        let after_click = h.state().state.camera;
        key(&mut h, egui::Key::ArrowLeft);
        assert!(h.state().state.camera.azimuth != after_click.azimuth, "an arrow key did not orbit");
        let after_arrow = h.state().state.camera;
        assert!(after_arrow.distance == after_click.distance, "an arrow key does not dolly");
        key(&mut h, egui::Key::Minus);
        assert!(h.state().state.camera.distance > after_arrow.distance, "- must pull back");
        key(&mut h, egui::Key::Plus);
        assert!(h.state().state.camera.distance < after_arrow.distance * 1.5, "+ must come closer");
    }

    #[test]
    fn the_home_button_returns_to_the_standard_view() {
        let mut h = harness();
        let c = rect(&h).center();
        drag(&mut h, c, c + vec2(60.0, 30.0), PointerButton::Primary);
        assert_ne!(h.state().state.camera.azimuth, crate::viewport_state::OrbitCamera::default().azimuth);
        h.get_by_label("Home").click();
        h.run_steps(3);
        assert_eq!(h.state().state.camera, crate::viewport_state::OrbitCamera::default(), "Home is the standard view");
        assert!(h.state().delta.camera_changed);
    }

    #[test]
    fn an_empty_scene_says_so_and_fit_leaves_the_camera_alone() {
        let mut h = harness();
        h.get_by_label("Fit").click();
        h.run_steps(3);
        // This scene has no solids, so there is nothing to frame: the panel says why instead of
        // inventing a view. A host with a body would see it framed.
        let painted = painted(&h);
        assert!(painted.contains("No solid to show"), "{painted}");
        assert!(painted.contains("Fit") && painted.contains("Home"), "{painted}");
        assert!(h.state().state.camera.is_valid());
    }

    #[test]
    fn framing_an_empty_body_is_refused_rather_than_inventing_a_view() {
        let scene = Scene::new(Camera { eye: Vec3::new(0.0, -5.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, ..Camera::default() });
        assert!(scene.bounds().is_empty());
        let mut camera = crate::viewport_state::OrbitCamera::default();
        camera.orbit(20.0, 5.0);
        let before = camera;
        assert!(!camera.frame(&scene.bounds()));
        assert_eq!(camera, before, "framing nothing moved the camera");
    }

    #[test]
    fn the_picture_is_uploaded_once_per_revision() {
        let ctx = egui::Context::default();
        let frame = Frame::new(1, Image::new(8, 6, Rgba::new(0.0, 0.0, 0.0)).expect("an image"));
        let first = texture(&ctx, &frame).map(|t| t.id()).expect("the picture was uploaded");
        let second = texture(&ctx, &frame).map(|t| t.id()).expect("still there");
        assert_eq!(first, second, "the same revision must not be uploaded again");
        let cached = ctx.data(|d| d.get_temp::<Cached>(egui::Id::new(FRAME_ID))).expect("the upload is remembered");
        assert_eq!(cached.revision, 1);
        assert_eq!(cached.size, [8, 6]);
        // A new revision, and a new size, each upload again.
        let mut moved = Frame::new(2, frame.image.clone());
        let _ = texture(&ctx, &moved);
        assert_eq!(ctx.data(|d| d.get_temp::<Cached>(egui::Id::new(FRAME_ID)).map(|c| c.revision)), Some(2));
        moved.revision = 3;
        moved.image = Image::new(16, 12, Rgba::new(0.0, 0.0, 0.0)).expect("a bigger image");
        let _ = texture(&ctx, &moved);
        assert_eq!(ctx.data(|d| d.get_temp::<Cached>(egui::Id::new(FRAME_ID)).map(|c| c.size)), Some([16, 12]));
        // An empty picture uploads nothing.
        assert!(texture(&ctx, &Frame::default()).is_none());
    }

    #[test]
    fn a_pick_maps_the_pointer_into_the_pictures_pixels() {
        let r = Rect::from_min_size(Pos2::ZERO, vec2(320.0, 240.0));
        let frame = Frame::new(1, Image::new(160, 120, Rgba::BLACK).expect("an image"));
        // The panel is twice the picture, so one point is half a pixel.
        assert_eq!(pixel_scale(r, &frame), Some((0.5, 0.5)));
        assert_eq!(pixel_scale(r, &Frame::default()), None, "no picture, no mapping");
        let collapsed = Rect::from_min_size(Pos2::ZERO, vec2(0.0, 240.0));
        assert_eq!(pixel_scale(collapsed, &frame), None, "a collapsed panel");
    }

    #[test]
    fn an_empty_scene_resolves_a_pick_to_nothing_without_complaining() {
        let mut h = harness();
        press(&mut h, rect(&h).center(), PointerButton::Primary);
        let request = h.state().state.pending_pick.expect("the click still queues a pick");
        assert!(request.is_valid());
        let camera = h.state().state.camera.camera();
        let scene = h.state().scene.clone();
        assert!(cadcraft_render::picking::pick_at(&camera, &scene, request.px, request.py, request.width, request.height).is_none());
        assert!(h.state().state.is_finite());
    }
}
