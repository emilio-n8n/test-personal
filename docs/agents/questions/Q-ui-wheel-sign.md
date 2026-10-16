# Q-ui-wheel-sign

- **Agent session:** unknown (3D UI compile subagent)
- **Task:** make the three 3D UI modules compile (viewport3 / viewport_state / panels3)
- **Owned files:** crates/ui-egui/src/viewport3.rs, crates/ui-egui/src/viewport_state.rs, crates/ui-egui/src/panels3/mod.rs, crates/ui-egui/src/panels3/feature_tree.rs, crates/ui-egui/src/panels3/parameters.rs
- **Status:** ASSUMED-AND-CONTINUING

## What is ambiguous

`viewport3.rs` `the_wheel_dollies_and_stops_at_the_limits` pins scroll-up (`MouseWheel` `delta.y = +1`, `Line`) to the eye moving AWAY (distance increases), and scroll-down to closer. The 2D canvas (`engine::wheel_zoom_factor`, documented "up zooms in") and AutoCAD do the opposite: scroll-up zooms in. Both cannot be the house convention.

## The options I can see

1. Keep the test-pinned direction (scroll-up pulls back), matching the module's own tests and comments verbatim, but inconsistent with the 2D canvas.
2. Make 3D match 2D (scroll-up dollies in) and fix the test's asserts and comments instead.

## What I am doing meanwhile

Option 1: `wheel_notches` in `viewport3.rs` negates scroll points into `Gesture.wheel` so `delta.y = +1` yields `dolly(+1)` (pull back), and `apply` keeps `dolly(-wheel)`. Least damaging if wrong: one negation plus two test asserts/comments to flip.

## What would change if the answer is the other option

Remove the negation in `wheel_notches`, and update `the_wheel_dollies_and_stops_at_the_limits` (direction asserts and the "moves the eye away / brings it closer" comments). Note: pinch (`Event::Zoom` / `zoom_delta`) currently feeds nothing in the 3D viewport; only the wheel does.
