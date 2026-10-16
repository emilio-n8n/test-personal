# Q-solid-edges-empty-shell

- **Agent session:** verifier subagent on `crates/solid/tests/property_surface.rs` (no session id known); see
  `docs/agents/answers/A-solid-property-surface.md`
- **Task:** judgement of the 10 reported failures of `property_surface.rs` (no formal task id known to me)
- **Owned files:** `docs/agents/answers/A-solid-property-surface.md`, this file — nothing else
- **Status:** ASKED

## What is ambiguous

`Solid::edges()` (`crates/solid/src/solid.rs`, `pub fn edges`) returns `Some(vec![])` for `Solid::default()`:
its rule is "None when some edge is not shared by exactly two faces", which cannot fire when the solid has no
edges at all. `tests/property_surface.rs::counts_reports_faces_loops_and_boundary_segments` asserts
`empty.edges().is_none()` and fails. Neither the code nor the doc states which way an *empty* shell resolves;
the test comment ("and has no edge list") and the function doc ("an open shell has no well-defined edge list")
pull in opposite directions for the degenerate case, and `chamfer::resolve` handles `None` with "the shell has
no edge list" but `Some([])` with "no edge passes through" — both honest refusals, different messages.

## The options I can see

1. **The vacuous answer is right** (`Some([])`): an empty shell's edge list is empty; "exactly two" holds of
   every edge because there are none. The test's expectation is wrong and must change to
   `assert_eq!(empty.edges(), Some(vec![]))` (or drop the line).
2. **A non-shell has no edge list at all** (`None` when `faces.is_empty()`): then `edges()` gains a guard, the
   doc sentence must be amended ("…or the shell is empty"), and the message any blend produces changes from
   "no edge passes through" to "the shell has no edge list".

## What I am doing meanwhile

Verdict recorded as **TEST WRONG as written, OR design call pending**, confidence medium, in
`A-solid-property-surface.md` §1 (`counts_reports_faces…` row) — I did not change either file, and I did not
silently pick a side. The least-damaging reading is option 1 (it matches the sentence actually written in
`solid.rs`, and a fixer told "make the test pass" who takes option 2 would edit a public contract nobody
signed off). The verification task is otherwise complete: all 17 tests judged from hand-derived geometry.

## What would change if the answer is the other option

If option 2: one guard in `Solid::edges` plus its doc comment, the `counts_reports…` test line stays as is,
and every blend entry point that calls `resolve` reports the "no edge list" reason for empty input — check
`tests/property.rs`' hostile-input cases for any expectation that names the other message. If option 1: only
the test's last assertion changes; the kernel is untouched.
