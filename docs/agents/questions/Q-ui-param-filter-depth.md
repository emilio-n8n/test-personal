# Q-ui-param-filter-depth

- **Agent session:** unknown (3D UI compile subagent)
- **Task:** make the three 3D UI modules compile (viewport3 / viewport_state / panels3)
- **Owned files:** crates/ui-egui/src/viewport3.rs, crates/ui-egui/src/viewport_state.rs, crates/ui-egui/src/panels3/mod.rs, crates/ui-egui/src/panels3/feature_tree.rs, crates/ui-egui/src/panels3/parameters.rs
- **Status:** ASSUMED-AND-CONTINUING

## What is ambiguous

`parameters.rs` `the_filter_hides_rows_without_reordering_the_rest` sets the filter to `"width"` and asserts the `depth` row is hidden. But `ParameterPanel::matches` matches name OR formula, and the `depth` row's formula is `"width + "`, which contains `"width"` — so the row passes the filter and the assert fails.

## The options I can see

1. Keep `matches` as name-or-formula (mirrors `FeatureTree::matches`, which matches name-or-kind and whose own test relies on kind matching) and fix the test's fixture or expectation.
2. Change `matches` to name-only, which makes this test pass but removes formula search and diverges from the feature tree.

## What I am doing meanwhile

Neither: both the implementation and the test are left exactly as written. Changing either would prejudge the contract, and the protocol forbids changing a test to match the code.

## What would change if the answer is the other option

Option 1 needs a test-only edit (e.g. give the broken row a formula that does not contain the filter text, or assert it stays visible). Option 2 needs `matches` reduced to the name plus a doc-comment update. Either is a one-spot edit once decided.
