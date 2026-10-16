# Q-sketch-anchor-weight

- **Agent session:** unknown (subagent; its session id is not exposed to it)
- **Task:** replace the duplicate LM port in `crates/model/src/sketch.rs` with `cadcraft-constraints::lm` and get its 10 tests passing
- **Owned files:** `crates/model/src/sketch.rs`
- **Status:** ASSUMED-AND-CONTINUING

## What is ambiguous

`rectangle_solves_and_moves_with_parameters` expects that changing `width` from 4 to 6 moves
p1 to (6,0) and p2 to (6,3), i.e. the change lands entirely on the driven edge and the other
corner stays put. The solver is a *minimum-norm* Gauss–Newton: with unit weights it splits the
2-unit correction evenly (p0 → (−1,0), p1 → (5,0)), which is defensible CAD behaviour in its
own right (Fusion splits too). There is no `keep`/`prefer_move` handle a sketch entity could
map to — `SolveOptions` carries `cadcraft_doc::Handle`s, which `PointId`s are not — so the
module had no way to express "this corner holds still". The test cannot be edited.

## The options I can see

1. **Anchor the first point by weight** (`ANCHOR_WEIGHT = 1e9` on the first
   `SketchEntity::Point`'s two parameters). The anchor drifts by ≤ correction/1e9 (nanometres),
   DOF is unaffected (a weight is not a lock), and every test passes. Risk: the policy is
   invisible to a user of the API — there is no way to choose a different anchor.
2. **Change the test** to accept the split. Forbidden by the task.
3. **Add an explicit anchor/ground concept to `Sketch`** (e.g. `Sketch::anchor: Option<PointId>`
   plus a `Ground` constraint). Cleanest long term, but it changes a public `Serialize`/`Deserialize`
   struct and is beyond this task's scope.

## What am I doing meanwhile

Option 1. It is the least damaging if wrong: it is ~10 lines, fully documented at
`ANCHOR_WEIGHT` in `crates/model/src/sketch.rs`, does not change any public signature, and
option 3 can subsume it later (the weight simply becomes an implementation of grounding). It
also matches the main solver's own vocabulary — `cadcraft_constraints` weights "keep"
geometry at 1e4 rather than locking it.

## What would change if the answer is the other option

If the split (option 2) were declared correct, `ANCHOR_WEIGHT` and the anchor block in
`Sketch::solve` would be deleted and the rectangle test's final assertions would have to be
rewritten to p1=(5,0), p2=(5,3). If option 3 were chosen, the anchor block would be replaced
by grounding the declared anchor point and the new field would need serde migration.
