# A-solid-edges-empty-shell

- **Question:** `Q-solid-edges-empty-shell.md`
- **Decision:** **Option 1 — the vacuous answer is right. `Solid::edges()` returns `Some(vec![])` for an
  empty shell, and the test's expectation changes.**

## Reasoning

`edges()` answers a question about topology, and the rule written in `solid.rs` is "`None` when some
edge is not shared by exactly two faces". On an empty shell there is no edge that violates that, so
the universal holds vacuously and the honest answer is an empty list. Option 1 is what the code
already does and what the sentence already says.

Option 2 asks for a rule nobody wrote: "a non-shell has no edge list at all". That is a claim about
`Solid::default()`-shaped input, not about the shell's edges, and it would silently change what
`chamfer::resolve` reports for empty input from "no edge passes through" to "the shell has no edge
list" — two refusals that mean different things, and the first is the more informative one because it
says what the *reference* failed to find.

`Solid::default()` is an empty container, not an invalid solid, and the crate already refuses it where
it matters (`validate` returns `Empty`). A method that describes what edges exist should not be the
place where emptiness becomes illegal.

## Files the asker must revisit

None — the verifier owns no production file. The fixer agent correcting
`crates/solid/tests/property_surface.rs` is told this decision and changes the `counts_reports…` line
to `assert_eq!(empty.edges(), Some(Vec::new()))`.

No guard is added to `Solid::edges`, its doc comment is unchanged, and no blend message changes.
