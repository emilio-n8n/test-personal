# Q-ui-wheel-roundtrip

- **Agent session:** unknown (3D UI compile subagent)
- **Task:** make the three 3D UI modules compile (viewport3 / viewport_state / panels3)
- **Owned files:** crates/ui-egui/src/viewport3.rs, crates/ui-egui/src/viewport_state.rs, crates/ui-egui/src/panels3/mod.rs, crates/ui-egui/src/panels3/feature_tree.rs, crates/ui-egui/src/panels3/parameters.rs
- **Status:** ASSUMED-AND-CONTINUING

## What is ambiguous

`viewport3.rs` `the_wheel_dollies_and_stops_at_the_limits` does one notch out (`dolly(+1)`) then one notch back (`dolly(-1)`) from `10.0` and asserts strict `back < start`. Dolly is multiplicative (`distance * 1.2^±1` via `powf`), so the round trip lands within ~1 ulp of `start` — above, on, or below it depending on libm `powf` fast paths. The assert may pass or fail by a rounding bit without anything being wrong.

## The options I can see

1. Keep the strict assert and accept a platform-dependent 1-ulp risk.
2. Weaken it to `back <= start` (or compare against a tolerance), which states the actual round-trip intent but touches a test to match the code.

## What I am doing meanwhile

Neither: the test is left exactly as written. Only an execution run can say which side of `start` this platform's `powf` lands on.

## What would change if the answer is the other option

Option 2 is a one-line test edit (`<=` or an epsilon comparison). No production code changes either way; `dolly` round-tripping to within 1 ulp is correct behaviour.
