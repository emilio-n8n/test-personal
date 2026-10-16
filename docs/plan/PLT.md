# PLT — platform: the build gate, wasm, layering, docs, performance, releases, credits, assets

Task index for the `PLT` domain (4 % of the programme). Owned by the orchestrator's platform lane:
`xtask/**`, `docs/**`, `.github/workflows/**`, `packaging/**`, `contributors/**`, root `Cargo.toml`,
and `ATTRIBUTION.md`.

**Status:** this file is a **specification**. Nothing in it has been compiled, tested or executed
(`docs/agents/SYSTEM_PROMPT.md` §2).

**Owner letters**: **PLT** (this file's owner: `xtask`, `docs`, `.github`, `packaging`,
`contributors`, `ATTRIBUTION.md`, root `Cargo.toml`), **A** kernel, **Q** quality, **D** interchange,
**U** ui/viewport, **C** orchestrator, plus the domain letters the sibling indexes use (`KER`, `MOD`,
`DAT`, `REN`, `UI`, `DRW`, `SIM`, `CAM`, `QAT`). A task whose **Files** field names another letter's
file *and modifies it* is `BLOCKED` on that owner (`master-plan.md` §6).

**Two non-negotiables that apply to every task below**, restated in each task that touches them:

1. **A task that creates a crate names both registration sites**: a row in `xtask/src/layers.rs`
   `TABLE` (`layers.rs:35-56`) *and* a `cadcraft-* = { path = … }` line in root `Cargo.toml`
   `[workspace.dependencies]` (`Cargo.toml:13-28`). Miss the first and `cargo xtask layers` fails with
   `Violation::Unregistered`; miss the second and the dependent crate does not resolve.
2. **A task that touches an asset names its `ATTRIBUTION.md` row**, enforced by `cargo xtask assets`
   (`xtask/src/assets.rs:10-24,26-41`), which fails when an asset-extension file under
   `assets/`, `docs/images/` or `examples/` is not listed as `` `path` `` with author, source and
   licence. `PLT-ASSET-008` decides the rule for *generated* fixtures; until it is decided, **no new
   asset file is committed**.

---

## Invariants of this domain

1. **One gate.** `cargo xtask ci` (`xtask/src/main.rs:151-199`) is the gate: `fmt`, `clippy
   --workspace --all-targets -- -D warnings`, `test --workspace`, `assets`, `layers`, `wasm`, in that
   order, stopping at the first failure and printing a summary of what passed and what was skipped.
   Every CI workflow runs **that command**, not a hand-written subset — a CI job that runs a
   different list of steps is a different gate, and two gates is none.
2. **`xtask` is pure Rust with no new dependencies**: std + `serde_json` (`xtask/src/main.rs:3-4`).
   Its rule logic must be extractable into pure functions with unit tests, because the code that
   decides whether CI passes is the code nobody runs on a pull request until it is too late.
3. **Every reported number is measured on a named machine on a named day.** The house convention
   (`docs/plan/DAT.md` invariant 2) applies: a benchmark, a wall time or a parity percentage is
   written down with how it was produced, and a number nobody can reproduce is deleted rather than
   kept.
4. **No claim without a check.** "CADCraft exports STEP", "the menu parity is 83 %", "the build takes
   4 minutes" — each is finished only when something other than the author confirmed it
   (`docs/master-plan.md` §8).
5. **A new dependency is a coordinator decision**, never an agent's. Every task below that would need
   one is written so it does not: std-only, or an existing dependency.
6. **Docs live in `docs/` and are owned by PLT**; `plan/` is gitignored and local. A doc under
   `docs/` is part of the product and `cargo xtask assets` may apply to its images.
7. **Layering is enforced, not documented.** `cargo xtask layers` (`layers.rs:149-199`) is in `ci`,
   and the table it enforces is `layers.rs::TABLE`; `plan/architecture.md` §3 is the prose that
   explains it.
8. **Every task in this file states its effect on `cargo xtask ci` wall time**, in its **Acceptance**
   field, in seconds or in "no new work". A platform change whose cost is unknown is not finished.

### What `xtask` already is (evidence, so tasks build on it rather than beside it)

| Reusable | Where | Why |
|---|---|---|
| `cmd_ci()` — the six steps, the summary, the exit code | `main.rs:151-199` | The gate. Every CI task extends it; none replaces it. |
| `metadata()` | `main.rs:77-83` | `cargo metadata --format-version 1 --no-deps`; the input to `layers::from_metadata` and to every "which crates exist" question. |
| `run(cmd, what)` | `main.rs:71-75` | One place that spawns a process and turns a failure into `Err(String)`; every xtask subprocess goes through it. |
| `wasm_set()` | `main.rs:121-132` | Which crates must compile for `wasm32-unknown-unknown`: every `Class::Layer(0..=4)`, every `Standalone`, and `ui-egui`. A new crate in that range is automatically in the wasm check. |
| `layers::{TABLE, INTRA_LAYER_ORDER, UI_CRATES, UI_MIN_LAYER, check, from_metadata}` | `layers.rs:35-199` | The whole layering rule engine, already unit-tested (`layers.rs:236-337`) with `cargo metadata` isolated behind `from_metadata`. |
| `assets::{ASSET_EXT, ASSET_DIRS, is_asset, missing, run}` | `assets.rs:10-41` | The attribution rule; `missing(files, md)` is a **pure function** already unit-tested, so extending it needs no git. |
| `parity::{leaves, run, OUT}` | `parity.rs:10-111` | Regenerates `docs/parity-checklist.md` from `xtask/data/menu-catalog.txt` and the live command registry; pure parts already tested. |
| `version::{read, replace, validate, run}` | `version.rs:9-107` | The single version source, atomic write, `Cargo.lock` refresh; tested against the real manifest (`version.rs:147`). |
| `ico::{png_size, pack}` | `ico.rs:9-45` | Deterministic `.ico` packing from square PNGs; pure and tested. |
| `stats::run` | `stats.rs:65` | Test and line counts per crate, with `--exact`. |
| `.github/workflows/release.yml`, `packaging-lint.yml` | `.github/workflows/` | The release pipeline and the packaging lint already exist; **no CI workflow for pull requests exists** — that is `PLT-CI-002`, and `docs/gaps.md` lists it as a top-10 alpha blocker. |

---

# PLT-CI — the gate, and running it on every pull request

### PLT-CI-001 — `cargo xtask ci` as the one gate, with its steps factored into testable functions
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/main.rs::{cmd_ci, USAGE}`, `xtask/src/ci.rs` (new; owner **PLT**)
- **Contract:**
  ```rust
  /// The gate's steps, in order. Adding a step means adding a row here and nothing else.
  pub const CI_STEPS: &[(&str, Step)] = &[("fmt", …), ("clippy", …), ("test", …),
                                         ("assets", …), ("layers", …), ("wasm", …)];
  pub struct CiReport { pub done: Vec<&'static str>, pub failed: Option<&'static str>,
                        pub skipped: Vec<&'static str>, pub elapsed: Vec<(&'static str, f64)> }
  pub fn run_ci(step: &dyn Fn(&str) -> Result<(), String>, clock: &dyn Fn() -> f64) -> CiReport;
  pub fn summary_lines(r: &CiReport) -> Vec<String>;   // what `ci failed at` prints today
  ```
- **Behaviour:** The step list, the "stop at the first failure" rule, the summary block and the exit
  code move out of `cmd_ci` into `ci.rs` as data plus two pure functions. `run_ci` takes the step
  runner and the clock as parameters so a unit test can run the whole gate with a fake that fails at
  a chosen step and assert that the later steps are reported `skip`. The order and the six names stay
  **exactly** as `main.rs:153-181` has them; this task changes no behaviour.
- **Refusals:** `run_ci` with an empty step list is refused (`Err("the gate has no steps")`); a step
  name that appears twice is refused naming it — a duplicated step is a silent double cost.
- **Tests:** `the_step_list_is_the_six_steps` — pinned against the order in `main.rs`. `a_failure_at_
  step_n_skips_the_rest`. `the_summary_lists_done_failed_and_skipped`. `elapsed_is_recorded_per_
  step`. `a_duplicate_step_name_is_refused`. `an_empty_step_list_is_refused`.
- **Acceptance:** `cargo xtask ci` behaves identically before and after (same steps, same order, same
  messages); the new unit tests pass. Effect on `cargo xtask ci` wall time: **0 s** (the same six
  subprocesses; the refactor is in-process).
- **Risk:** a refactor of the gate itself, which is the one thing everybody runs. Fallback: the step
  list is a `const` and the test pins it, so a behavioural change fails a test rather than passing
  quietly.

### PLT-CI-002 — A pull-request workflow that runs `cargo xtask ci`
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `.github/workflows/ci.yml` (new; owner **PLT**)
- **Contract:** `name: CI`, `on: pull_request` and `push: [main]`, one job per the matrix of
  `PLT-CI-003`, each running exactly:
  ```yaml
  - run: cargo xtask ci
  ```
- **Behaviour:** This closes `docs/gaps.md` top-10 item 6 ("No CI runs the Rust tests on pull
  requests"), listed as an **alpha blocker**. The workflow installs the pinned Rust toolchain
  (`rust-toolchain.toml`, `PLT-CI-007`) and the `wasm32-unknown-unknown` target, sets the cache keys
  of `PLT-CI-005`, runs `cargo xtask ci`, and uploads nothing but the log on failure. It does **not**
  reimplement a step: a second list of commands here is the failure mode this task exists to prevent.
- **Refusals:** a workflow that runs a subset of the gate is refused at review; a workflow that runs
  the gate with `--no-default-features`-style flags is refused unless `PLT-CI-003` asks for it.
- **Tests:** `the_workflow_runs_exactly_cargo_xtask_ci` — a check that reads `ci.yml` and asserts the
  only `run:` step containing `cargo` is `cargo xtask ci` (a small test in `xtask/src/ci.rs`).
  `the_workflow_triggers_on_pull_request`. `the_workflow_pins_the_toolchain`.
- **Acceptance:** a pull request runs the gate; `docs/gaps.md`'s row 6 is updated in the same change
  (the file's own rule). Effect on `cargo xtask ci` wall time: **0 s** for the command; the workflow
  adds only the runner's clone/toolchain overhead (~20 s cold).
- **Risk:** CI becomes the gate and a flaky test now blocks every PR. Fallback: mark a flaky test
  `#[ignore]` **with the reason and an owner** (`PLT-CI-014` counts them), which is a decision
  somebody makes, not an accident.

### PLT-CI-003 — The matrix: Linux, macOS, Windows
- **State:** READY
- **Depends on:** PLT-CI-002
- **Files:** `.github/workflows/ci.yml` (owner **PLT**)
- **Contract:** a `strategy.matrix.os: [ubuntu-latest, macos-latest, windows-latest]`, `fail-fast:
  false`, so one platform's failure still reports the other two.
- **Behaviour:** The three platforms the product ships on (`docs/releasing.md:7`). The wasm step is
  run on Linux only and the others skip it (`if: matrix.os == 'ubuntu-latest'`) because
  `wasm32-unknown-unknown` is checked, not run, and it costs a full `cargo check` per crate
  (`PLT-CI-009`); the platform-specific tests (path handling, `sync_all` on a directory, the atomic
  rename, `DAT-057`/`DAT-058`) run everywhere.
- **Refusals:** `fail-fast: true` is refused: a macOS-only failure must not hide a Linux one.
- **Tests:** `the_matrix_has_three_platforms`. `fail_fast_is_false`. `the_wasm_step_is_linux_only`.
- **Acceptance:** the three jobs run; the summary names all three verdicts. Effect on
  `cargo xtask ci` wall time: **0 s** per job (the same gate; the matrix is 3× the wall time in
  parallel, not per run).
- **Risk:** macOS runners cost 10× a Linux minute. Fallback: `macos-latest` on pull requests and on
  `main`; Windows only on `main` and on release branches, if the minutes become a problem — a
  decision recorded in the workflow's comment.

### PLT-CI-004 — `cargo xtask ci --quick`: fmt and clippy only, for the inner loop
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/main.rs::{cmd_ci, USAGE}`, `xtask/src/ci.rs` (owner **PLT**)
- **Contract:** `cargo xtask ci --quick` runs `fmt` and `clippy` and stops; `--only <step>` runs one
  named step; both are rejected if the step name is not in `CI_STEPS`.
- **Behaviour:** The two steps that catch most mistakes in under a minute, for a developer with a
  dirty `target/`. `--only wasm` is how one debugs the slowest step on its own.
- **Refusals:** an unknown step name is refused listing the six; `--quick` together with `--only` is
  refused as contradictory.
- **Tests:** `quick_runs_fmt_and_clippy_only`. `only_runs_the_named_step`. `an_unknown_step_is_refused`.
  `quick_and_only_together_is_refused`.
- **Acceptance:** `--quick` passes when the gate passes. Effect on `cargo xtask ci` wall time:
  **−X s** for a developer's inner loop (the `test`, `assets`, `layers` and `wasm` steps are skipped);
  the full gate is unchanged.
- **Risk:** a `--quick` habit that never runs the full gate. Fallback: `USAGE` prints
  "`cargo xtask ci` is the gate; `--quick` is for while you type" in the text itself.

### PLT-CI-005 — The cache keys, pinned to what actually invalidates them
- **State:** READY
- **Depends on:** PLT-CI-002
- **Files:** `.github/workflows/ci.yml` (owner **PLT**)
- **Contract:** `actions/cache` over `~/.cargo/registry`, `~/.cargo/git` and `target/`, keyed on
  `hashFiles('Cargo.lock')` + the toolchain file + the job's `matrix.os`, with a rolling key so a
  week's cache does not fill the disk.
- **Behaviour:** `Cargo.lock` is the correct key for the dependency cache, and the **toolchain** is
  the correct key for `target/` — a toolchain change must invalidate the build cache or CI compiles
  with the wrong incremental state and produces unexplainable test failures. `target/` is cached for
  Linux only; macOS and Windows runners are ephemeral and a cache upload costs more than it saves.
- **Refusals:** a key that does not include `Cargo.lock` is refused at review; a cache key including
  a branch name is refused (it makes every branch a cold build for no benefit).
- **Tests:** `the_cache_key_includes_cargo_lock`. `the_cache_key_includes_the_toolchain`. `the_target_
  cache_is_linux_only`. (Checked by the same small YAML-asserting test as `PLT-CI-002`.)
- **Acceptance:** the second run on an unchanged lock file is measurably faster and the recorded time
  says so. Effect on `cargo xtask ci` wall time: **−40 % to −70 % on a warm cache** for the `clippy`
  and `test` steps (the biggest single win available, and it costs nothing).
- **Risk:** a stale `target/` cache that makes a failure unreproducible. Fallback: `target/` caching
  is keyed on the toolchain **and** restored with `--locked`, and a suspected cache problem is solved
  by re-running with the cache disabled (one workflow input).

### PLT-CI-006 — Clippy: the exact invocation, and what `-D warnings` means here
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/ci.rs`, root `Cargo.toml` `[workspace.lints]` (owner **PLT**)
- **Contract:**
  ```rust
  pub const CLIPPY_ARGS: &[&str] = &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"];
  ```
  and the workspace lint table stays as it is (`Cargo.toml:59-66`): `unsafe_code = "forbid"`,
  `dbg_macro = "warn"`, `todo = "warn"`, `too_many_arguments`/`type_complexity` allowed.
- **Behaviour:** One constant for the invocation, so a workflow and `xtask` cannot drift. `--all-targets`
  is deliberate: it lints tests, benches and examples too, which is why a `#[cfg(test)]` block cannot
  smuggle in a lint the library avoids. The house per-crate `#![deny(clippy::unwrap_used, …)]` lines
  are **not** in the workspace table on purpose — they are per-crate so a new crate must write them,
  and `PLT-LAYER-007` is where that is checked.
- **Refusals:** a lint added to the workspace table that contradicts a crate's own `deny` is refused;
  `--cap-lints` in a workflow is refused (it silently downgrades the gate).
- **Tests:** `clippy_args_are_the_pinned_four`. `the_workspace_table_keeps_unsafe_forbidden`. `no_
  workflow_uses_cap_lints` (YAML assert). `the_lint_table_does_not_relax_a_crate_deny`.
- **Acceptance:** the invocation is unchanged from `main.rs:166` and the constant is its only
  definition. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** `--all-targets` lints the ~1 400 tests' worth of test code, which is the slowest part of
  the step. Fallback: none needed; it is where the value is.

### PLT-CI-007 — `fmt`: the toolchain pin and the config that makes the check meaningful
- **State:** READY
- **Depends on:** —
- **Files:** `rustfmt.toml`, `rust-toolchain.toml` (new; owner **PLT**), `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```toml
  # rust-toolchain.toml
  [toolchain]
  channel = "1.90.0"          # matches `rust-version` in Cargo.toml:10
  components = ["rustfmt", "clippy"]
  targets = ["wasm32-unknown-unknown"]
  ```
- **Behaviour:** A pinned channel means `cargo fmt --check` in CI and in a developer's editor produce
  the same result; today `rust-version = "1.90"` (`Cargo.toml:10`) is a minimum, not a pin, and a
  developer's stable rustfmt may reformat differently from CI's. `targets` puts the wasm target in the
  default toolchain, which `PLT-WASM-003` needs.
- **Refusals:** a toolchain file that disagrees with `Cargo.toml`'s `rust-version` is refused (a test
  compares them); pinning to `nightly` is refused without a reason in the file.
- **Tests:** `the_toolchain_matches_rust_version`. `the_toolchain_has_rustfmt_and_clippy`. `the_
  toolchain_has_the_wasm_target`. `cargo_fmt_check_is_clean` (the step itself).
- **Acceptance:** `cargo fmt --all -- --check` passes with the pinned toolchain on all three platforms.
  Effect on `cargo xtask ci` wall time: **0 s** for a matching developer toolchain; **+2 s** for the
  one-time toolchain download on a cold CI runner.
- **Risk:** pinning to a patch version means new contributors need `rustup` (which they have). The
  alternative, floating, is what produces "CI says unformatted, my editor does not". Fallback: bump the
  pin in one place, deliberately.

### PLT-CI-008 — The `test` step: what "green" means, and how long it may take
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub const TEST_ARGS: &[&str] = &["test", "--workspace"];
  /// The budget, in seconds, past which the step is reported as over budget (not failed).
  pub const TEST_BUDGET_S: f64 = 900.0;
  ```
- **Behaviour:** `cargo test --workspace` unchanged (`main.rs:174`), with the wall time recorded and
  compared against a **budget** (`QAT-004`'s "> 10 %" rule is for benchmarks; this is the wall-clock
  version). Over budget is a **warning in the summary**, not a failure, because a failure nobody can
  act on trains people to ignore CI.
- **Refusals:** a test binary that panics is a failure (that is cargo's own behaviour); a step that
  exceeds 3× the budget is reported as a failure candidate in the summary, naming the step.
- **Tests:** `the_test_step_is_the_pinned_invocation`. `elapsed_is_recorded`. `over_budget_is_a_
  warning_not_a_failure`. `three_times_the_budget_is_flagged`.
- **Acceptance:** the budget's number is written down in `PLT-PERF-003` with the measurement that
  produced it. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a budget nobody updates becomes a lie. Fallback: the budget is read from a committed
  text file (`PLT-PERF-003`), so changing it is a visible diff.

### PLT-CI-009 — The `wasm` step: thirteen `cargo check` runs become one
- **State:** READY
- **Depends on:** PLT-CI-001, PLT-WASM-001
- **Files:** `xtask/src/main.rs::{cmd_wasm, wasm_set}`, `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  /// One invocation for every crate in `wasm_set()`, instead of one per crate.
  pub fn wasm_command(set: &[String]) -> Command;   // cargo check --target wasm32-unknown-unknown -p a -p b …
  pub fn wasm_failures(set: &[String], ok: bool) -> Vec<String>;  // which crates, when the whole run failed
  ```
- **Behaviour:** `cmd_wasm` (`main.rs:134-149`) spawns **one** `cargo check` per crate today — 13
  cargo invocations for 13 crates, each re-resolving the dependency graph and re-locking `target/`.
  One invocation with repeated `-p` checks all of them in a single graph walk, and cargo's own output
  names the failing crates, so the per-crate table in the summary is built from parsing it. The
  printed table (crate → ok/FAIL) is preserved, because it is the diagnostic people read.
- **Refusals:** a crate in `wasm_set()` that is **not** a workspace member is refused naming it (a
  typo in `TABLE` would otherwise check nothing); an empty set is refused.
- **Tests:** `wasm_command_lists_every_crate_with_dash_p`. `a_failing_crate_is_named_in_the_output`.
  `an_empty_set_is_refused`. `wasm_set_includes_layer_0_to_4_and_ui_egui` — the set's membership rule,
  pinned (a new crate at L2 is automatically in it).
- **Acceptance:** `cargo xtask wasm` prints the same table. Effect on `cargo xtask ci` wall time:
  **−60 % to −80 % of the `wasm` step** on a cold cache (13 graph walks → 1; on a warm cache it is
  mostly per-invocation overhead, so the saving is smaller in relative terms) — this is the largest
  single-step win in the gate and it changes no check.
- **Risk:** one invocation loses the per-crate "ok" attribution if cargo's output format changes
  across versions. Fallback: the pinned toolchain (`PLT-CI-007`) fixes cargo's output format, and if
  parsing fails the summary prints "see the log above" rather than guessing.

### PLT-CI-010 — The `assets` step: its cost, and a fast path that is still correct
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/assets.rs::{run, is_asset, missing}`, `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn run(root: &Path) -> Result<(), String>;                       // today: `git ls-files`, one process
  pub fn missing_from_paths(paths: &[String], assets_md: &str) -> Vec<String>;  // the pure part, exposed
  ```
- **Behaviour:** The step costs one `git ls-files` subprocess (`assets.rs:27-32`) and a substring
  search per file. Its cost is small but it is a **process spawn and a full working-tree listing** on
  every CI run. The fast path: when `CI` is set and the change touches no asset path (no
  `assets/`, `docs/images/`, `examples/` and no asset-extension file in the diff), the step may report
  `skip (no asset in this change)` — and **only** when the diff is available and non-empty. A push to
  `main` (a full-tree event) always runs it in full.
- **Refusals:** a diff that cannot be determined (no base ref) is **not** a skip: the step runs in
  full. A skip with any asset path in the diff is a bug and the test says so.
- **Tests:** `a_change_with_an_asset_runs_the_full_check`. `a_change_without_an_asset_may_skip`. `an_
  undeterminable_diff_runs_the_full_check`. `missing_from_paths_matches_missing`. `the_existing_
  detects_unattributed_assets_test_still_passes`.
- **Acceptance:** the step's verdict is printed either way (`ok` or `skip (reason)`), never nothing.
  Effect on `cargo xtask ci` wall time: **−1 s to −3 s** on a typical PR (one process and a listing);
  0 s on `main`.
- **Risk:** a skip that hides a newly added asset. Fallback: the "asset path in the diff" test is
  exhaustive over `ASSET_EXT` and `ASSET_DIRS`, and the rule is that a **push to `main` never skips**.

### PLT-CI-011 — The `layers` step: it depends on `cargo metadata`, and that is the cost
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/layers.rs::{from_metadata, check}`, `xtask/src/main.rs::cmd_layers` (owner
  **PLT**)
- **Contract:**
  ```rust
  pub fn from_metadata(meta: &Value) -> Result<Vec<Crate>, String>;   // unchanged: the only cargo coupling
  pub fn check(crates: &[Crate]) -> Vec<Violation>;                   // pure, already tested
  ```
- **Behaviour:** The step spawns `cargo metadata --format-version 1 --no-deps` (`main.rs:87`), builds
  the crate graph and prints the table with a violation count. Its cost is dominated by the metadata
  spawn; the rule engine itself is microseconds. The task therefore exists to make the **pure** part
  unambiguously pure (it already is: `check(&[Crate])` takes no I/O, `layers.rs:149`) and to state
  that a change to the graph shape must not add I/O to `check`.
- **Refusals:** metadata that fails to parse is `Err` with cargo's stderr (today's behaviour,
  `main.rs:79-82`), never an empty crate list that "passes".
- **Tests:** `check_is_pure` — no filesystem or process access in `check`'s signature, asserted by
  compiling it against a fixed `&[Crate]`. `a_new_crate_must_be_in_the_table` — a table over
  `members` asserting every crate classifies (this is the check that makes `PLT-LAYER-004`'s note
  real). `an_unregistered_crate_is_an_error` (existing, `layers.rs:297`).
- **Acceptance:** the printed table keeps its shape. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** none; this task is mostly a guard rail.

### PLT-CI-012 — The failure summary: what a reader needs in the first screen
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/ci.rs::{summary_lines, CiReport}` (owner **PLT**)
- **Contract:**
  ```rust
  pub struct CiReport { …, pub failure_detail: Option<String> }   // the last stderr lines, capped
  pub const SUMMARY_DETAIL_LINES: usize = 40;
  ```
- **Behaviour:** Today's summary is a good shape (`main.rs:187-195`: done / FAIL / skip) and is kept.
  What is added is the **detail**: the last 40 lines of the failing step's output, trimmed, so a PR
  shows *why* without opening the log. Long output is trimmed from the **middle** with a count, so the
  first and last errors both survive.
- **Refusals:** detail over the cap is trimmed with a stated line count; a step whose output is
  empty gets `no output` rather than a blank section.
- **Tests:** `the_detail_is_the_failing_steps_tail`. `long_output_is_trimmed_in_the_middle`. `an_empty_
  step_output_says_so`. `the_summary_still_lists_done_and_skipped`.
- **Acceptance:** a failing run's first screen names the step and the error. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** the summary growing until the log is unreadable. Fallback: the 40-line cap.

### PLT-CI-013 — The per-step budget file, and where the numbers come from
- **State:** READY
- **Depends on:** PLT-CI-008, PLT-PERF-003
- **Files:** `xtask/ci-budget.toml` (new; owner **PLT**), `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct StepBudget { pub step: String, pub seconds: f64, pub machine: String, pub date: String }
  pub fn budget(path: &str, step: &str) -> Result<Option<StepBudget>, String>;
  pub fn verdict(step: &str, elapsed: f64, budget: Option<&StepBudget>) -> Verdict;  // Ok | Over(f64)
  ```
- **Behaviour:** One committed file with the six steps' measured times, the machine they were measured
  on and the date — the house convention (`docs/plan/DAT.md` invariant 2, and `PLT-PERF-003`). The gate
  compares its own step times against it and prints `ok` / `over by N s`. It **never fails** on time:
  a time regression is information, and the failure gate is a benchmark (`PLT-PERF-011`).
- **Refusals:** a budget file with no `machine` or no `date` is refused when read (a time without a
  machine is not a measurement); a budget for a step that does not exist is refused.
- **Tests:** `a_budget_is_read_with_its_machine_and_date`. `a_budget_without_a_machine_is_refused`. `an_
  unknown_step_in_the_budget_is_refused`. `over_budget_is_reported_not_failed`. `elapsed_under_
  budget_is_ok`.
- **Acceptance:** `docs/roadmap.md`'s stability section carries the current numbers with their date.
  Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a checked-in budget that nobody updates becomes fiction. Fallback: `verdict` prints
  "budget is N days old" when `date` is more than 30 days old, so the staleness is visible in the log.

### PLT-CI-014 — Ignored tests are counted and shown, never silent
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/ci.rs`, `xtask/src/stats.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn count_ignored(lines: &[&str]) -> usize;          // parse `… ignored` lines
  pub fn ignored_report(count: usize, tests: usize) -> String;
  ```
- **Behaviour:** `cargo test` prints `N tests ignored`. The gate counts it and prints
  `"1 432 tests, 6 ignored"` in the summary, so a green CI that is quietly skipping tests is visible.
  Every `#[ignore]` in the repo carries a reason string (the repo's own convention), and
  `PLT-QAT`'s external-reader harnesses (`DAT-014`) are the sanctioned users of `#[ignore]` with the
  variable name in the reason.
- **Refusals:** an `#[ignore]` with an empty reason is refused at review (not by a test — a lint for
  it needs a proc macro, which is a new dependency, so it is a review rule and a grep in
  `xtask/src/stats.rs` that counts them).
- **Tests:** `count_ignored_reads_cargo_output`. `the_summary_shows_the_ignored_count`. `every_ignore_
  has_a_reason` — a grep test over the workspace sources.
- **Acceptance:** the count appears in the summary. Effect on `cargo xtask ci` wall time: **0 s**
  (one pass over cargo's own output).
- **Risk:** the count grows and nobody cares. Fallback: the number is printed with the **total**, so
  the ratio is visible; a ratio over 2 % is called out.

### PLT-CI-015 — `xtask`'s own tests: the gate's logic is tested
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/{ci,layers,assets,parity,version,ico}.rs` `#[cfg(test)]` blocks (owner **PLT**)
- **Contract:** every rule in `xtask` is a pure function with unit tests — `layers::check` (10 tests,
  `layers.rs:236-337`), `assets::missing` (1, `assets.rs:46-54`), `parity::leaves` (2,
  `parity.rs:114-128`), `version::validate` (5, `version.rs:109-152`), plus the new `ci` functions.
- **Behaviour:** The test count is asserted by `cargo xtask stats`, so deleting a test is visible in
  the diff of the count. `xtask` has no `tests/` directory (it is `Class::Exempt`, `layers.rs:55`), so
  the tests live in the modules — the same discipline `KER.md` §0.5 imposes on the kernel.
- **Refusals:** a pure function that reads the filesystem or spawns a process is refused at review;
  `xtask` may not gain a dependency (`main.rs:3-4`).
- **Tests:** `xtask_has_no_dependencies` — a test reading `xtask/Cargo.toml` and asserting its
  dependency list is the std set plus `serde_json`. `every_xtask_module_has_tests` — a grep over
  `xtask/src/*.rs`.
- **Acceptance:** `cargo test -p xtask` passes and the count is in `docs/roadmap.md`. Effect on
  `cargo xtask ci` wall time: **+0.2 s** (one more small test binary in the `test` step).
- **Risk:** the gate's tests run *inside* the gate, which is circular but is the point — a broken
  rule fails the gate that uses it. Fallback: none; this is the correct arrangement.

### PLT-CI-016 — Concurrency: one gate per ref, cancelled when superseded
- **State:** READY
- **Depends on:** PLT-CI-002
- **Files:** `.github/workflows/ci.yml` (owner **PLT**)
- **Contract:**
  ```yaml
  concurrency:
    group: ci-${{ github.ref }}
    cancel-in-progress: true
  ```
- **Behaviour:** A push to a PR branch supersedes the previous run on the same ref; cancelling the old
  one is correct and saves the minutes. The same pattern `packaging-lint.yml:15-17` already uses.
- **Refusals:** `cancel-in-progress: false` is refused for `pull_request` — a superseded run's
  cancellation means a required check is missing for a while.
- **Tests:** `the_workflow_has_a_concurrency_group`. `cancel_in_progress_is_true`.
- **Acceptance:** a second push cancels the first run. Effect on `cargo xtask ci` wall time: **−T s**
  of wasted machine time per superseded run (the command itself is unchanged).
- **Risk:** a required status check disappears when a run is cancelled. Fallback: GitHub treats a
  cancelled run of the *same* ref as superseded, and `main` pushes are never cancelled (different
  group per ref).

### PLT-CI-017 — Refuse a nightly job, and say why in the file
- **State:** READY
- **Depends on:** PLT-CI-002
- **Files:** `.github/workflows/ci.yml` comment (owner **PLT**)
- **Contract:** no `nightly` toolchain job in `ci.yml`; the decision is a comment in the file.
- **Behaviour:** Nightly would catch future-incompatibilities early, which is real value, but it also
  produces failures nobody can act on until the pinned toolchain moves, and it doubles the matrix. The
  decision is **deferred, not refused**, with the condition for revisiting: when the pinned toolchain
  is within one release of end of support, a nightly **build-only** job (no tests) is added.
- **Refusals:** a nightly job that runs the full gate is refused; a nightly **check-only** job is the
  permitted form.
- **Tests:** `no_nightly_toolchain_in_ci` (YAML assert).
- **Acceptance:** the comment exists with the revisit condition. Effect on `cargo xtask ci` wall
  time: **0 s**.
- **Risk:** discovering a nightly break at the moment the toolchain moves. Fallback: the release
  workflow builds with the pinned toolchain too, so a break surfaces there at worst — which is why the
  revisit condition is written down.

### PLT-CI-018 — No pre-commit hook: the decision, written where a developer will see it
- **State:** READY
- **Depends on:** PLT-CI-002
- **Files:** `docs/agents/SYSTEM_PROMPT.md` (a short note), `AGENTS.md`'s gate line (owner **PLT**)
- **Contract:** no `.pre-commit-config`, no `git` hook shipped; the decision and the two alternatives
  are written in `docs/agents/SYSTEM_PROMPT.md`.
- **Behaviour:** A hook that runs `cargo fmt` on commit is convenient and is also the thing agents and
  contributors trip over (a hook that rewrites files inside a commit is surprising, and a hook that
  fails blocks a commit for a reason unrelated to the commit). The two alternatives are named: run
  `cargo xtask ci --quick` yourself (`PLT-CI-004`), or let the PR workflow do it (`PLT-CI-002`).
- **Refusals:** a hook that **modifies** files on commit is refused outright. A read-only hook that
  only prints a reminder is permitted and documented as opt-in.
- **Tests:** `no_git_hook_is_shipped` — a grep for `.git/hooks` content and for a hook installer in
  `packaging/` and `xtask/`.
- **Acceptance:** the note exists in the system prompt an agent reads. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** a contributor lands an unformatted commit and discovers it in CI. Fallback: the CI
  failure is one line long (`cargo fmt --all`).

---

# PLT-WASM — the browser build stays green, and stays honest about what it is

`cargo xtask wasm` runs `cargo check --target wasm32-unknown-unknown` on every crate the layering
table puts in `Layer(0..=4)`, every `Standalone` crate and `ui-egui` (`main.rs:121-132`). The web app
is `apps/cadcraft-web` (Trunk, `Trunk.toml`, `packaging/web/`). The rule the master plan states is
one line long: **never break wasm**.

### PLT-WASM-001 — The wasm set is the layering set, and its membership rule is pinned
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/main.rs::wasm_set`, `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  /// Every `Class::Layer(0..=4)`, every `Class::Standalone`, and `ui-egui`.
  pub fn wasm_set() -> Result<Vec<String>, String>;              // unchanged signature
  pub fn wasm_expected(crates: &[Crate]) -> Vec<String>;          // the pure membership rule
  ```
- **Behaviour:** The rule is derived from the layering table, so a new crate at L0–L4 is in the wasm
  check **without anyone editing a list**: `wasm_expected` is the pure form and a unit test walks the
  table. `Standalone` (L0, no workspace deps) is in it because it is the foundation of the DXF/DWG
  crates; `Testkit` (L6) and `Exempt` are out.
- **Refusals:** a crate classified `Layer(5)` or higher other than `ui-egui` is **not** in the wasm
  set; that is not a failure today (only `ui-egui` exists at L5) but a future L5 library crate must
  either be wasm-clean or be exempted **explicitly in `TABLE`**, never by accident.
- **Tests:** `wasm_expected_includes_every_layer_0_to_4_crate`. `ui_egui_is_in_the_set`. `testkit_and_
  exempt_are_out`. `a_new_l2_crate_would_be_included` — asserted against a synthetic crate list, so
  the rule is what makes `DRW`'s L2 crate wasm-checked automatically.
- **Acceptance:** the set matches `wasm_expected` for the real workspace. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** a crate that *cannot* be wasm-clean (a file-system-heavy L2) would be added anyway.
  Fallback: `PLT-WASM-005`'s refusals move such work behind a seam; there is no exemption mechanism,
  by design.

### PLT-WASM-002 — The new-crate checklist: what a `DRW`/`SIM`/`CAM` crate must do on day one
- **State:** READY
- **Depends on:** PLT-LAYER-004
- **Files:** `docs/plan/PLT.md`'s section is the source; the checklist is a comment block in
  `xtask/src/layers.rs` (owner **PLT**) and a row in `crates/*/README` if present
- **Contract:**
  ```rust
  /// CHECKLIST for a new crate (kept next to TABLE so the next author reads it):
  /// 1. TABLE row in xtask/src/layers.rs        -> else Violation::Unregistered and `xtask layers` fails
  /// 2. `cadcraft-x = { path = "crates/x" }` in root Cargo.toml [workspace.dependencies]
  /// 3. `#![deny(clippy::unwrap_used, …)]` + `#![forbid(unsafe_code)]` in lib.rs   (PLT-LAYER-007)
  /// 4. no `std::fs`/`std::time::Instant`/threads below L5 without a seam           (PLT-WASM-005)
  /// 5. at L0..=L4 it is in `wasm_set()` automatically; prove it with `cargo xtask wasm`
  /// 6. it must not appear in ASSET_EXT by accident (see PLT-ASSET-008)
  ```
- **Behaviour:** The two registration sites (1 and 2) are the ones that fail **late**: step 1 fails
  `xtask layers` but not the build; step 2 fails the build of every dependent crate. The checklist
  sits twelve lines above the code that enforces it.
- **Refusals:** a crate without both registration sites is refused at review; the checklist is the
  review's reference.
- **Tests:** `the_checklist_mentions_both_registration_sites` — a grep test over `layers.rs`.
  `every_workspace_member_is_registered` (from `PLT-CI-011`).
- **Acceptance:** `DRW-VIEW-001`, `ASM-001` and any future `SIM`/`CAM` crate satisfy it; the review of
  each finds the checklist already applied. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the checklist rots. Fallback: items 3–6 are each enforced by a test elsewhere in this
  epic, so a stale checklist line disagrees with a machine and the reviewer sees which is wrong.

### PLT-WASM-003 — The wasm target in the pinned toolchain, and a smoke build
- **State:** READY
- **Depends on:** PLT-CI-007
- **Files:** `rust-toolchain.toml` (owner **PLT**)
- **Contract:** `targets = ["wasm32-unknown-unknown"]` in the toolchain file; `cargo xtask wasm` then
  needs no `rustup target add`.
- **Behaviour:** The step currently assumes the target is installed, and on a fresh CI runner it is
  not, which is why the workflow installs it explicitly. With the target in the pinned toolchain, both
  the developer and CI get it from one file.
- **Refusals:** a missing target is a named error from `cargo check`, not a silent pass — the step
  must not treat "target not installed" as success.
- **Tests:** `the_toolchain_lists_the_wasm_target` (shared with `PLT-CI-007`).
- **Acceptance:** `cargo xtask wasm` on a machine that has only the pinned toolchain succeeds.
  Effect on `cargo xtask ci` wall time: **−10 s to −20 s** on a cold runner (no separate
  `rustup target add` step).
- **Risk:** the pinned toolchain's wasm target lags a stable release. Fallback: it is the same pin as
  the compiler, so they cannot disagree.

### PLT-WASM-004 — One check for the whole set, and the failure list
- **State:** BLOCKED — same work as `PLT-CI-009`; one implementation, referenced from both
- **Depends on:** PLT-CI-009, PLT-WASM-001
- **Files:** `xtask/src/main.rs::{cmd_wasm, wasm_set}`, `xtask/src/wasm.rs` (new; owner **PLT**)
- **Contract:** `pub fn wasm_command(set: &[String]) -> Command;` and
  `pub fn wasm_failures(set: &[String], output: &str) -> Vec<String>;`
- **Behaviour:** **This task and `PLT-CI-009` are the same task**: `PLT-CI-009` owns the change (it is
  inside the gate), this task owns the module split (`xtask/src/wasm.rs`) and the pure failure
  parser, which is what the unit tests exercise. Two tasks, one implementation, one owner — stated
  here so nobody implements it twice.
- **Refusals:** a parse that cannot attribute the failure to a crate is `Err` with the raw tail, not
  an empty list.
- **Tests:** `wasm_failures_reads_cargo_output`. `every_failing_crate_is_named`. `an_unattributable_
  failure_is_an_error`.
- **Acceptance:** see `PLT-CI-009`. Effect on `cargo xtask ci` wall time: as `PLT-CI-009` (**−60 % to
  −80 %** of the wasm step).
- **Risk:** duplicate implementation. Fallback: this task's **State** says which one is `BLOCKED` on
  which, and both name the same two signatures.

### PLT-WASM-005 — The wasm refusals: no filesystem, no clock, no threads below L5
- **State:** READY
- **Depends on:** PLT-WASM-001
- **Files:** `xtask/src/wasm.rs` (new; owner **PLT**)
- **Contract:**
  ```rust
  /// The forbidden std items below L5, as source-level patterns with the reason.
  pub const WASM_FORBIDDEN: &[(&str, &str)] = &[
      ("std::fs", "the browser has no filesystem"),
      ("std::thread", "no threads without wasm atomics"),
      ("std::process", "no subprocesses"),
      ("std::net", "the browser's network is fetch"),
  ];
  pub fn scan_crate(files: &[(String, String)]) -> Vec<String>;   // pure: path + text → findings
  ```
- **Behaviour:** `std::fs` compiles for `wasm32-unknown-unknown` (it panics at run time), so a type
  check does **not** catch it — the web build is checked, not run. `scan_crate` is a source scan with
  `//` and `/* */` stripped first, run over every crate in the wasm set as part of the `wasm` step,
  reporting file and line. `std::time::Instant` is **allowed** with a documented caveat: it panics on
  wasm, so it is reported as a warning with the seam to use instead (a caller-supplied clock, the
  pattern `PLT-PERF-001` establishes).
- **Refusals:** a finding in a `#[cfg(test)]` block is a warning (tests never run on wasm); a finding
  in a `#[cfg(target_arch = "wasm32")]`-excluded file is not a finding at all; a scan over a crate
  with a file over 1 MB is refused (it is not source).
- **Tests:** `a_workspace_crate_with_no_fs_has_no_findings` — run against the real crates; this is the
  acceptance. `a_file_with_std_fs_in_non_test_code_is_found_with_its_line`. `a_file_in_cfg_test_is_a_
  warning`. `comments_do_not_produce_findings`. `a_huge_file_is_refused`.
- **Acceptance:** the scan runs in the wasm step and the real workspace is clean. Effect on
  `cargo xtask ci` wall time: **+1 s** (a source scan of ~150 k lines, pure std).
- **Risk:** a false positive on a doc comment or a string literal. Fallback: comments are stripped
  before matching and the message names the line, so a reviewer can judge it in one look.

### PLT-WASM-006 — The web build: Trunk, the pinned profile and a reproducible dist
- **State:** READY
- **Depends on:** PLT-WASM-003
- **Files:** `apps/cadcraft-web/{Trunk.toml,index.html,Cargo.toml,src/**}`, `packaging/web/package.sh`
  (owner **PLT** for the packaging script; **U** for the app)
- **Contract:** `trunk build --release` from `apps/cadcraft-web` writes `dist/web` (`Trunk.toml:3-4`)
  with `public_url = "./"` and `filehash = true`; `packaging/web/package.sh` zips it and **fails** if
  the built `index.html` has a root-absolute URL (`package.sh:22-25` — already implemented).
- **Behaviour:** `public_url = "./"` is what makes the zip work from any path and inside an iframe,
  and the script's grep is the guard. `filehash = true` gives cache-busting content hashes. The task
  records the web build as a **release** artefact (`docs/releasing.md:7`) and states the pinned Trunk
  version.
- **Refusals:** a build whose `index.html` has a root-absolute URL is refused by the existing script
  (`package.sh:23`); a missing `dist/web/index.html` is refused (`package.sh:20`).
- **Tests:** `the_web_package_fails_on_root_absolute_urls` — the script's own rule, asserted by running
  it against a fixture (it is a shell script, so this is a grep test over its text plus a negative
  fixture in `packaging/web/`). `the_zip_contains_index_html`. `the_public_url_is_relative`.
- **Acceptance:** the web zip is one of the release artefacts and the release workflow's web job
  produces it. Effect on `cargo xtask ci` wall time: **0 s** — the web build is a **release** job, not
  a PR gate (a PR gate would add minutes for a build whose output is not tested).
- **Risk:** the web build breaks in a way `cargo check --target wasm32` does not catch (a Trunk
  config error, a missing `index.html` element). Fallback: `PLT-WASM-008` adds a cheap PR job that
  runs only the Trunk build, no wasm size analysis.

### PLT-WASM-007 — What the web build can and cannot do, printed in the app
- **State:** READY
- **Depends on:** PLT-WASM-005
- **Files:** `crates/ui-egui/src/i18n/*.tsv` keys, `docs/localization-parity.md` (owner **U** for the
  app, **PLT** for the doc)
- **Contract:** the web build's limitations are stated where a user meets them — no native file
  dialog (`rfd` has no wasm implementation), no `std::process`, no direct disk access, and therefore
  **the 3D and file-dialog parts of the product are not available in the browser**. Each gets a
  localized message string in every `i18n/*.tsv` catalog (13 catalogs exist today:
  `crates/ui-egui/src/i18n/`), because a UI that silently lacks a feature is worse than one that says
  so.
- **Behaviour:** The strings are added through the existing i18n mechanism (a TSV row per language, per
  `crates/ui-egui/src/i18n/mod.rs`), and the parity doc gains a row per limitation. Every new string
  is a **translation task**, not an English-only string: a missing translation falls back to English
  today, and the catalog's completeness is `PLT-CI-014`-style visible.
- **Refusals:** a capability that is unavailable in the browser is **disabled with a reason**, never
  hidden and never silently failing; a command that needs a file dialog in the browser is refused with
  a message naming the limitation.
- **Tests:** `every_limitation_has_a_localized_message` — the key exists in every catalog. `a_
  disabled_capability_shows_its_reason`. `the_web_and_desktop_capability_lists_differ_only_where_
  stated`.
- **Acceptance:** `docs/localization-parity.md` lists every web limitation with its message key.
  Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the list drifts as the product grows. Fallback: the list is generated from one table in
  `crates/ui-egui` (a `WASM_LIMITATIONS` const with its reason and its message key), and a test
  asserts the doc's rows match the const — so a new limitation is added in one place.

### PLT-WASM-008 — A cheap PR job that runs the Trunk build only
- **State:** BACKLOG
- **Depends on:** PLT-WASM-006
- **Files:** `.github/workflows/ci.yml` (owner **PLT**)
- **Contract:** a second job `web-build` on `pull_request`, running
  `cargo install trunk --locked && trunk build --release` in `apps/cadcraft-web`, with no size analysis
  and no test run.
- **Behaviour:** `cargo check --target wasm32` proves the *crates* compile for wasm; it does not prove
  the app links or that `index.html` is well formed. A build-only job catches the second class for a
  few minutes of runner time.
- **Refusals:** this job is **not** allowed to become a full gate (it would double the PR time for a
  result the Rust gate already covers); if it needs more, that is a conversation, not a quiet
  expansion.
- **Tests:** `the_web_job_runs_trunk_build`. `the_web_job_does_not_run_the_gate`.
- **Acceptance:** a PR that breaks the web build says so. Effect on `cargo xtask ci` wall time: **0 s**
  for the Rust gate; **+3 min** of parallel runner time for the PR as a whole.
- **Risk:** `cargo install trunk` takes minutes. Fallback: cache `~/.cargo/bin/trunk` and pin the
  version in the workflow (Trunk's version is a single line).

### PLT-WASM-009 — Autosave in the browser: what it saves and where
- **State:** BLOCKED — needs `UI-APP-009`'s autosave (desktop) and a web storage decision
- **Depends on:** PLT-WASM-007, UI-APP-009
- **Files:** `crates/ui-egui/src/**` (owner **U**), `docs/gaps.md` (owner **PLT**)
- **Contract:** the browser build's autosave writes the **2D drawing** to `localStorage`/IndexedDB in
  DXF (`crates/io`'s writer, owner **D**) at `SAVETIME` intervals, and says so in the status bar. It
  does **not** claim to save a 3D model: the `.ccm` writer (`crates/model/src/file.rs`, owner **D**)
  and the browser's storage quota make that a decision, not an implementation.
- **Behaviour:** `docs/gaps.md` top-10 item 5 is "No autosave in the web build"; this task scopes the
  honest version: **2D drawings autosave; 3D models do not, and the UI says which is which.** The
  storage write is the existing DXF writer, so no new format is introduced in the browser.
- **Refusals:** a 2D drawing above the browser's quota (5 MB typical) is refused with the size and the
  limit, and the user is told autosave is off for that drawing — **never** silently truncated.
- **Tests:** `the_web_build_autosaves_a_2d_drawing`. `an_oversized_drawing_is_refused_not_truncated`. `the
  _ui_states_that_3d_models_are_not_autosaved_in_the_browser`.
- **Acceptance:** `docs/gaps.md` item 5 is updated in the same change, and the limitation is in the
  table from `PLT-WASM-007`. Effect on `cargo xtask ci` wall time: **0 s** (a feature, not a gate
  change).
- **Risk:** the browser drops storage under pressure (Safari's 7-day cap). Fallback: the write is
  timestamped and the recovery flow reads the newest, and the limitation is in the printed list.

### PLT-WASM-010 — The wasm binary size, measured and recorded
- **State:** BACKLOG
- **Depends on:** PLT-WASM-006
- **Files:** `xtask/src/wasm.rs`, `docs/roadmap.md` (owner **PLT**)
- **Contract:**
  ```rust
  pub struct WasmSize { pub raw_kb: u64, pub gz_kb: u64, pub date: String }
  pub fn measure(dist: &Path) -> Result<Vec<WasmSize>, String>;   // one row per .wasm, gzip by the stored size
  ```
- **Behaviour:** The size is **recorded**, not gated, in the first instance: a number that gates CI
  before anybody has a baseline just blocks everyone. The measurement walks `dist/web`, sizes each
  `.wasm` raw and gzipped (`flate2` is **not** added; the gzipped size is reported by the packaging
  script's `zip` output or by the CI's own summary — the task says which, and prefers the script).
- **Refusals:** no `.wasm` in `dist/web` is `Err("no wasm to measure; run trunk build --release")`
  rather than a silent zero.
- **Tests:** `measure_finds_every_wasm`. `a_dist_without_wasm_is_an_error`. `the_date_is_recorded`.
- **Acceptance:** `docs/roadmap.md` carries the size with its date and the Trunk version.
  Effect on `cargo xtask ci` wall time: **0 s** (release-only measurement).
- **Risk:** size regresses unnoticed. Fallback: a future task may gate it once a baseline exists; this
  one records it so the gating decision can be made with a number.

### PLT-WASM-011 — wasm-bindgen and the web-sys dependency policy: refused, and why
- **State:** READY
- **Depends on:** PLT-WASM-001
- **Files:** `docs/architecture.md` §3's dependency section, `apps/cadcraft-web/Cargo.toml` (owner
  **PLT** for the policy, **U** for the file)
- **Contract:** the rule, written: **`wasm-bindgen`/`web-sys`/`js-sys` may be used only in
  `apps/cadcraft-web` and `crates/ui-egui`**, and only for the shell (window, canvas, file input,
  storage). No library crate below L5 may depend on them, because a library that needs the browser is
  not a library the CLI can use.
- **Behaviour:** The enforcement is the layering table plus a grep test in `xtask/src/wasm.rs`
  (`PLT-WASM-005`'s scan, extended with the three crate names), because `cargo check --target
  wasm32` cannot catch a library that compiles but imports the DOM.
- **Refusals:** a workspace crate below L5 with a `wasm-bindgen` dependency is refused at review and
  by the grep test; `apps/cadcraft-web` using anything else to reach the browser (a hand-written JS
  file, a `<script>` tag) is refused — the master plan's "no handwritten JS/TS" rule
  (`master-plan.md:19`).
- **Tests:** `no_crate_below_l5_depends_on_wasm_bindgen` — a grep test over every `Cargo.toml`.
  `only_the_shell_crates_use_web_sys`. `no_handwritten_js_in_the_web_app` — a grep over
  `apps/cadcraft-web/`.
- **Acceptance:** the policy is in `docs/architecture.md` and the tests pass. Effect on `cargo xtask ci`
  wall time: **+0.5 s** (two more greps).
- **Risk:** someone needs the DOM in a library for a good reason. Fallback: the seam is an interface
  the library defines and the shell implements (the pattern `PLT-PERF-001` uses for the clock), which
  is the answer rather than an exemption.

### PLT-WASM-012 — The 3D viewport in the browser: what it needs, and the honest answer
- **State:** BACKLOG
- **Depends on:** PLT-WASM-011, REN-001
- **Files:** `crates/render/**` (owner **U**), `docs/localization-parity.md` (owner **PLT**)
- **Contract:** the browser build's 3D viewport needs the **GPU render path** (`REN-001`) on WebGL2
  through `egui-wgpu`, which is a wasm build of wgpu with its own constraints (no compute shaders on
  some targets, texture size limits, no `std::thread` without atomics). This task's content is the
  **scoping statement and the limitation list**, written before any of it is built.
- **Behaviour:** The 3D model *is* usable in the browser today (the CPU rasteriser `scene3` is pure
  Rust and wasm-clean), and what is missing is the **interactive** GPU viewport. The stated limit: the
  browser build shows 3D through the CPU path or not at all, and the UI says which.
- **Refusals:** a browser build that silently falls back from GPU to CPU with no indication is
  refused; the limitation table from `PLT-WASM-007` is the place it is stated.
- **Tests:** `the_browser_3d_limitation_is_stated`. `the_cpu_path_is_wasm_clean` — the existing
  `render` crate is in the wasm set, so this is already true and is asserted rather than assumed.
- **Acceptance:** the limitation is in the table and in `docs/roadmap.md`. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** the statement goes stale when `REN-001` lands. Fallback: the limitation list is a generated
  table (`PLT-WASM-007`) so a landing task removes its row.

### PLT-WASM-013 — `wasm-bindgen-test` for browser-only code: refused, and what covers it instead
- **State:** READY
- **Depends on:** PLT-WASM-001
- **Files:** `xtask/src/wasm.rs` (owner **PLT**)
- **Contract:** no `wasm-bindgen-test` harness; browser-only behaviour is covered by (a) the type
  check, (b) unit tests of the pure logic with the DOM behind a trait the test implements, and (c) the
  documented manual check in the PR template.
- **Behaviour:** `wasm-bindgen-test` would be a new dependency and a second test runner, for tests of
  code that is mostly a thin shell over `egui`. The rule instead is: **logic in a library crate (tested
  normally), DOM access in the shell (typed, small, and covered by the compile check)**. A test that
  needs a real canvas is a manual check with a stated procedure, in `docs/agents/SYSTEM_PROMPT.md`.
- **Refusals:** adding `wasm-bindgen-test` is refused without a coordinator decision and a written
  reason.
- **Tests:** `no_wasm_bindgen_test_dependency` — a grep over every `Cargo.toml`.
- **Acceptance:** the policy is written down. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a browser-only bug reaches production. Fallback: the manual procedure, and the browser is
  a secondary platform (the roadmap's main platform is macOS, `docs/roadmap.md`).

### PLT-WASM-014 — Close-out: the wasm matrix in one table, and the step that proves it
- **State:** READY
- **Depends on:** PLT-WASM-004, PLT-WASM-005, PLT-WASM-008
- **Files:** `xtask/src/wasm.rs`, `docs/roadmap.md` (owner **PLT**)
- **Contract:** the committed table: **crates in the wasm set** (from `wasm_expected`), **crates
  checked but not run**, **crates refused** (none, by rule), **known gaps** (from
  `PLT-WASM-007`'s table). `pub fn wasm_table_is_current() -> bool;` asserts the doc's crate list
  equals `wasm_expected`'s for the real workspace.
- **Behaviour:** One table in `docs/roadmap.md` that cannot drift, because a test compares it with the
  live set — the same anti-drift shape as `DRW-XRF-024` and `DAT-020`.
- **Refusals:** a crate in the doc's table that is not in the set (or vice versa) is a failing test.
- **Tests:** `wasm_table_is_current`. `the_table_names_the_known_gaps`. `the_table_says_checked_not_
  run`.
- **Acceptance:** the table is in `docs/roadmap.md` and the test passes. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** the table is regenerated-equal by a test, but the *known gaps* column is prose and can go
  stale. Fallback: `PLT-WASM-007`'s generated limitation list is the source for that column, so a
  landed browser capability removes its row there and the test notices the count.

---

# PLT-LAYER — the dependency rule, enforced, and honest about what it does not model

`xtask/src/layers.rs` is 337 lines of a small, well-tested rule engine: `Class`
(`Layer(n) | Standalone | Testkit | Exempt`), a `TABLE` of 18 crates, `INTRA_LAYER_ORDER` for
same-layer chains, `UI_CRATES` with a minimum layer, and `check()` producing five `Violation` variants.
This epic keeps it true as the workspace grows to include `cadcraft-asm`, `cadcraft-drawing`,
`cadcraft-sim` and `cadcraft-cam`.

### PLT-LAYER-001 — The rule engine's five violations are the whole contract
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/layers.rs::{Violation, check}` (owner **PLT**)
- **Contract:**
  ```rust
  pub enum Violation {
      Unregistered { krate: String },
      Upward { krate: String, dep: String, from: u8, to: u8, kind: DepKind },
      StandaloneHasWorkspaceDep { krate: String, dep: String },
      TestkitAsNormalDep { krate: String },
      UiBelowL6 { krate: String, dep: String, layer: u8 },
  }
  ```
  **unchanged** — this task adds no variant.
- **Behaviour:** The five violations are the five rules, and their `Display` messages
  (`layers.rs:128-146`) are the ones a contributor sees. They name the crate, the dependency, the
  layers and, where relevant, `plan/architecture.md` §3 — which is **gitignored** and therefore a
  message a contributor cannot open. That is the one defect in this area.
- **Refusals:** adding a sixth violation (for example "cyclic") is refused until a real case exists
  that the five cannot express — a cycle among L4 crates is not a layering violation today and
  checking it belongs elsewhere.
- **Tests:** the existing ten tests in `layers.rs:236-337` stay; `every_violation_has_a_display` is
  added.
- **Acceptance:** the five variants and their messages are unchanged except for `PLT-LAYER-002`.
  Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the `UiBelowL6` variant's name says L6 while the rule is `layer < UI_MIN_LAYER` (5) —
  a stale name that will confuse the next reader. Fallback: renaming a public enum variant in a
  private xtask module is free; this task renames it to `UiBelowMinLayer` **only** if the
  orchestrator asks, because the message text is what contributors read and it is already correct.

### PLT-LAYER-002 — The violation messages must point at a file that exists in the repository
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs` (`Violation`'s `Display`, `Unregistered`'s message at `layers.rs:130`),
  `docs/architecture.md` (new; owner **PLT**)
- **Contract:** the `Unregistered` message names `docs/architecture.md §3` (committed) instead of
  `plan/architecture.md §3` (gitignored, `AGENTS.md` says `plan/` is local only).
- **Behaviour:** A contributor who sees "register it in plan/architecture.md §3" opens a file that is
  not in the repository. The committed `docs/architecture.md` holds the layering section, so the
  message points there. This task writes the section's content from the table it documents
  (`TABLE`, `INTRA_LAYER_ORDER`, `UI_MIN_LAYER`) and states that **the table is the enforcement and the
  document is the explanation**.
- **Refusals:** a doc that contradicts the table is refused: the doc's layer list is checked against
  `classify` by a test.
- **Tests:** `the_unregistered_message_names_a_committed_file` — asserts the path exists in the repo.
  `the_documented_layers_match_the_table` — the doc's table equals `TABLE`'s.
- **Acceptance:** every violation message names a committed path. Effect on `cargo xtask ci` wall
  time: **0 s**.
- **Risk:** two documents (the table and the prose) can disagree. Fallback: the prose is generated
  from the table by a test, or the test asserts equality — this task asserts equality.

### PLT-LAYER-003 — `INTRA_LAYER_ORDER`: what it is for, and when to extend it
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs::{INTRA_LAYER_ORDER, intra_layer_allowed}`, `docs/architecture.md`
  (owner **PLT**)
- **Contract:**
  ```rust
  /// Earlier may be used by later, within one layer.
  pub const INTRA_LAYER_ORDER: &[&[&str]] = &[&["geom", "color", "solid"], &["fonts", "render"]];
  ```
- **Behaviour:** Two chains exist because two real pairs exist: the L0 foundation (`geom` →
  `color` → `solid`) and the text/render pair (`fonts` → `render`, a deliberate rule because a
  renderer that could not be used by a text stack was an accident). Extending it is the escape hatch
  for a legitimate same-layer dependency, and it is exactly as powerful as a deleted rule: **one chain
  entry can legalise an entire layer's worth of sideways dependencies.**
- **Refusals:** a chain with more than three entries is refused at review unless the doc explains it; a
  chain that is a *cycle* is refused by `check` (position `t < f`, `layers.rs:65-71`); a chain entry
  naming a crate not in `TABLE` is refused.
- **Tests:** `a_chain_entry_must_name_a_registered_crate`. `a_chain_of_four_is_refused` — the test
  asserts the refusal function exists and rejects it. `the_two_existing_chains_are_justified` — each
  chain's doc comment names why.
- **Acceptance:** the rule for extending a chain is written in `docs/architecture.md`. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** a chain is added to make one dependency legal and silently legalises three others.
  Fallback: `docs/architecture.md` requires a chain addition to name the **one** dependency that needs
  it, and a review reads the diff's other effects.

### PLT-LAYER-004 — A new crate's two registration sites, checked by a test
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs::TABLE`, root `Cargo.toml` `[workspace.dependencies]`, `xtask/src/ci.rs`
  (owner **PLT**)
- **Contract:**
  ```rust
  /// Every workspace member is either in TABLE or is one of the four exempt names.
  pub fn registered(members: &[String]) -> Vec<String>;     // unregistered members, sorted
  pub const EXEMPT: &[&str] = &["cadcraft", "cli", "web", "xtask"];
  ```
- **Behaviour:** `members = ["crates/*", "apps/*", "xtask"]` (`Cargo.toml:3`) means a new directory
  is a new member automatically — including a directory somebody created by accident. `registered`
  turns "I forgot the `TABLE` row" from a confusing build error into a named one, and the `ci` step
  prints it. This is the test that makes `DRW-VIEW-001`, `ASM-001` and every future crate's checklist
  (`PLT-WASM-002`) enforceable rather than advisory.
- **Refusals:** a member that is neither registered nor exempt is `Violation::Unregistered` (today's
  behaviour, `layers.rs:98-101`) with the message naming **both** registration sites.
- **Tests:** `registered_reports_a_new_crate`. `the_four_exempt_crates_are_exempt`. `every_current_
  member_is_registered`. `the_unregistered_message_names_both_sites`.
- **Acceptance:** creating `crates/x` with a `Cargo.toml` and nothing else produces a `layers` failure
  naming `TABLE` **and** `[workspace.dependencies]`. Effect on `cargo xtask ci` wall time: **0 s**
  (pure).
- **Risk:** the check runs only in `ci`, so a locally built workspace can drift. Fallback: `cargo xtask
  layers` is cheap enough to run in the pre-push habit and in `PLT-CI-004`'s `--only layers`.

### PLT-LAYER-005 — `Standalone`, `Testkit`, `Exempt`: what each class is for
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs::{Class, classify, describe}` (owner **PLT**)
- **Contract:** `Class::Standalone` = layer 0 **and** no workspace dependency at all (dxf, dwg);
  `Class::Testkit` = sits at L6 for the rules, usable by others **only as a dev-dependency**
  (testkit); `Class::Exempt` = binaries and build tooling (cadcraft, cli, web, xtask).
- **Behaviour:** The three special classes exist because three real cases do not fit a plain layer.
  The rule engine implements each precisely: `StandaloneHasWorkspaceDep` (`layers.rs:166-169`),
  `TestkitAsNormalDep` (`layers.rs:172-175`), and `class == Exempt → continue` (`layers.rs:156-158`).
- **Refusals:** adding a crate to `EXEMPT` to silence a violation is refused at review — the four
  names are the apps and the build tool, and a fifth is a hole in the rule.
- **Tests:** `standalone_with_a_workspace_dep_is_flagged` (existing, `layers.rs:289`). `testkit_as_a_
  normal_dep_is_flagged` (existing, `layers.rs:302`). `apps_and_xtask_are_exempt` (existing,
  `layers.rs:311`). `a_fifth_exempt_name_is_refused` — the function that would allow it does not exist.
- **Acceptance:** the three classes keep their exact semantics. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** `Exempt` means "no rules", which is a strong statement about a crate. Fallback: `PLT-
  LAYER-006` records what each exemption is trusted to do.

### PLT-LAYER-006 — The exemptions are trust statements, and they are written down
- **State:** READY
- **Depends on:** PLT-LAYER-005
- **Files:** `docs/architecture.md` (owner **PLT**)
- **Contract:** for each of `cadcraft`, `cli`, `web`, `xtask`, the doc states **what it is trusted to
  do**: that it may depend on every crate (they are the binaries and the browser shell), that they
  contain no reusable library, and that a library moved out of one of them must be registered.
- **Behaviour:** `Exempt` is a hole by design. Writing down what each hole assumes is what turns four
  holes into four **stated** trust statements, and it is the difference between "we don't check the
  app" and "we don't check the app, and here is what would be lost if it were wrong".
- **Refusals:** an exemption whose crate contains a `pub` library API used by another crate is
  refused — that crate should be a registered library.
- **Tests:** `the_exemption_doc_names_all_four`. `each_exemption_states_what_it_may_depend_on`.
- **Acceptance:** the doc exists; a reviewer adding a fifth crate is pointed at this section.
  Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the statement becomes stale. Fallback: `PLT-LAYER-004`'s `EXEMPT` const is the list, and
  the doc's test asserts the two agree.

### PLT-LAYER-007 — Every production crate root denies the unwrap family and forbids unsafe
- **State:** READY
- **Depends on:** —
- **Files:** `crates/*/src/lib.rs`, `xtask/src/ci.rs` (owner **PLT** for the check; each crate root
  is its domain's)
- **Contract:**
  ```rust
  pub const REQUIRED_CRATE_LINTS: &str =
      "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, \
       clippy::todo, clippy::unreachable)]\n#![forbid(unsafe_code)]";
  pub fn missing_crate_lints(members: &[(String, String)]) -> Vec<String>;   // (crate, path)
  ```
- **Behaviour:** `AGENTS.md` requires every production crate root to carry those two lines, and
  `Cargo.toml:59-66` has `[workspace.lints]` but the **deny family is per-crate on purpose** (a new
  crate must write it). Nothing checks that today: a crate can lose its `deny` line in a merge and
  only the crash-safety convention suffers silently. This task is the check — a source scan, the same
  shape as `PLT-WASM-005`.
- **Refusals:** a crate with no `lib.rs` (a binary-only crate such as `apps/cli`) is **exempt** and
  reported as exempt; a `#[cfg(test)]`-only file is not a crate root and is not scanned.
- **Tests:** `every_crate_root_has_the_deny_line` — run against the real workspace; this is the
  acceptance. `a_binary_only_crate_is_exempt`. `a_crate_with_only_unsafe_forbidden_is_reported`.
- **Acceptance:** the scan runs in the `clippy` step (where the lint would fire anyway, so a
  regression is caught at the same moment, with a better message). Effect on `cargo xtask ci` wall
  time: **+0.3 s** (fourteen crate roots read).
- **Risk:** the workspace `unsafe_code = "forbid"` already covers unsafe (`Cargo.toml:60`), so half the
  check is redundant. Fallback: keep both; the deny family is the one that is per-crate.

### PLT-LAYER-008 — Feature flags must not change the layering
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs::{from_metadata, Dep}` (owner **PLT**)
- **Contract:**
  ```rust
  pub struct Dep { pub name: String, pub kind: DepKind, pub workspace: bool }
  /// A dependency that is optional is still a dependency: `check` sees it whether or not the
  /// feature is on. This task asserts that property, it does not change it.
  pub fn optional_dep_is_still_checked(meta: &Value) -> Vec<Violation>;
  ```
- **Behaviour:** `cargo metadata` reports a dependency's features but `from_metadata`
  (`layers.rs:202-223`) reads only `name`, `kind` and `path`. An **optional** dependency is therefore
  always checked — which is the right default: a feature that lets a crate reach upward is a hole in
  the rule, not an exemption. The task pins that with a test so a future refactor that starts skipping
  optional deps is caught.
- **Refusals:** a crate that needs a different rule for an optional upward dependency is refused; the
  rule does not have an optional escape.
- **Tests:** `an_optional_workspace_dep_is_still_flagged`. `a_build_dependency_is_checked` —
  `DepKind::Build` goes through the same rules (`layers.rs:160-190`), asserted.
- **Acceptance:** the properties are pinned by tests. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a legitimate case (a build script needing a tool crate) appears and forces an exemption.
  Fallback: `Exempt` for a **build-only** tool crate, which is a stated trust statement
  (`PLT-LAYER-006`), never a new rule.

### PLT-LAYER-009 — `cadcraft-asm` at L3: the `model` problem, named and decided
- **State:** BLOCKED — the decision is the orchestrator's; the evidence is here
- **Depends on:** PLT-LAYER-003, ASM-001
- **Files:** `xtask/src/layers.rs::{TABLE, INTRA_LAYER_ORDER}`, `docs/plan/ASM.md` (owner **PLT** for
  the table, **ASM** for its index)
- **Contract:** `ASM.md` §8 says `cadcraft-asm` "is **L3**: it may use … `cadcraft-model` (L3)". But
  `TABLE` has `model` at `Class::Layer(3)` and `INTRA_LAYER_ORDER` contains no L3 chain, so
  `check()` (`layers.rs:180`) flags `asm (L3) -> model (L3)` as `Violation::Upward`. **The two
  documents cannot both be right.** The three ways out:
  1. `INTRA_LAYER_ORDER` gains `&["model", "asm"]` — one line, legalises exactly that direction;
  2. `model` moves to L4 and `engine` moves to L5 — a large, visible refactor;
  3. `asm` does not depend on `model` (it takes its solids as `ScenePart`s, as `DRW-VIEW-001` does).
- **Behaviour:** The task's output is the **decision plus the evidence**, recorded here and in the
  report. The evidence favours option 1 with option 3 as the direction of travel: `asm` needs the
  model's *solid* and its *records*, which is a narrow surface, and every narrowing of it is a
  layering improvement. This task does **not** make the change.
- **Refusals:** an option chosen because it is one line, with the cost unexamined, is refused.
- **Tests:** after the decision, `asm_to_model_is_legal` (or `asm_does_not_depend_on_model`) — a test
  asserting the chosen state, so the decision is enforced rather than recorded.
- **Acceptance:** the decision is in `docs/architecture.md` §3 and in `docs/plan/ASM.md`, and `cargo
  xtask layers` is green under it. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the inconsistency is discovered by an agent halfway through a task. Fallback: this task
  exists precisely to surface it before `ASM-001` lands; the interim is that `cadcraft-asm` cannot be
  created green until the decision is made.

### PLT-LAYER-010 — `cadcraft-drawing` at L2, and what it therefore may not use
- **State:** READY
- **Depends on:** PLT-LAYER-003
- **Files:** `xtask/src/layers.rs::TABLE`, `docs/plan/DRW.md`'s invariant 7 (owner **PLT** for the
  table; **DRW** for its index)
- **Contract:** `("drawing", Class::Layer(2))` in `TABLE`; allowed: `geom` (L0), `solid` (L0),
  `doc` (L1). **Refused**: `render` (L2 sideways — `INTRA_LAYER_ORDER` has only `fonts → render`),
  `constraints` (L2 sideways), `io` (L3), `model` (L3), `asm` (L3), `engine` (L4), `ui-egui` (L5).
- **Behaviour:** The consequences are stated in `DRW.md`'s invariant 7 and repeated here so the rule
  and its consequence are in one place: dimension geometry is drawn by the existing renderer rather
  than computed by the drawing crate; hidden-line removal computes geometry and never pixels; the
  viewport's hidden-line style is served by `ENG` (`DRW-XRF-011`), the one crate that may hold both;
  an assembly enters as `ScenePart`s.
- **Refusals:** `render` and `constraints` in `cadcraft-drawing` are refused — L2 sideways, the
  `check` rule with `intra_layer_allowed` (`layers.rs:180`).
- **Tests:** `drawing_is_l2`. `drawing_may_use_geom_solid_and_doc`. `drawing_may_not_use_render_`
  or_constraints` — asserted against a synthetic graph, so the rule is pinned before the crate
  exists. `a_new_l2_crate_is_in_the_wasm_set` (shared with `PLT-WASM-001`).
- **Acceptance:** `cargo xtask layers` shows `cadcraft-drawing L2 geom, solid, doc` with no
  violation, and the constraints are enforced by `check`, not by discipline. Effect on
  `cargo xtask ci` wall time: **0 s** at steady state; **+0.2 s** while the crate is new (one more
  crate in the metadata walk).
- **Risk:** an agent adds `render` to solve a real problem (it is the natural place to look for text
  metrics). Fallback: the seam is `Dimension`/`Tolerance` entities that the renderer already draws —
  the answer exists and is written in `DRW.md`.

### PLT-LAYER-011 — `cadcraft-sim` and `cadcraft-cam`: the layer before they exist
- **State:** BACKLOG
- **Depends on:** PLT-LAYER-010, SIM-001
- **Files:** `xtask/src/layers.rs::TABLE`, `docs/architecture.md` §3 (owner **PLT**)
- **Contract:** the placement, decided now: **`cadcraft-sim` at L2** (a solver is a library over
  `geom`; it may not read a document or render) and **`cadcraft-cam` at L3** (a toolpath is computed
  from the solid and the model, so it may reach `model`). Both are new crates and both therefore need
  the two registration sites of `PLT-LAYER-004`.
- **Behaviour:** Deciding the layer before the crate exists is the cheapest possible time: a crate
  created at the wrong layer is a refactor of every dependent crate. `SIM.md`'s `SIM-MSH` epic puts
  meshing in `cadcraft-sim`, and `SIM.md`'s reuse ledger does not let `sim` reach `render`; `CAM`
  consumes the model. The doc states the reason per crate in one line each.
- **Refusals:** a placement decided because it makes the dependency graph draw nicely is refused.
- **Tests:** `sim_at_l2_may_use_geom_and_solid`. `cam_at_l3_may_use_model`. `neither_may_use_render`.
- **Acceptance:** the decision is in `docs/architecture.md` before the crates exist. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** the decision is wrong for `cam`, which may need `io` (post-processors write files).
  Fallback: `cam` at L3 may **not** use `io` (L3 sideways); file writing stays in `io` or `engine`, and
  the post-processor's output is a value, not a write.

### PLT-LAYER-012 — `cargo xtask layers` in the CI output, not only in the failure path
- **State:** READY
- **Depends on:** PLT-CI-011
- **Files:** `xtask/src/main.rs::cmd_layers` (owner **PLT**)
- **Contract:** the step already prints the whole crate table (`main.rs:89-104`) and the violation
  count. This task pins the output format and adds the **one line** that answers "what changed":
  `crates: 14 · violations: 0 · layers: L0:3 L1:1 L2:3 L3:2 L4:1 L5:2 · exempt: 4`.
- **Behaviour:** A summary line makes the step's result readable at a glance in a PR that is mostly
  green, and it is the number `PLT-LAYER-004`'s new-crate rule makes people look at.
- **Refusals:** the table itself is unchanged (it is the diagnostic).
- **Tests:** `the_summary_line_counts_the_classes` — computed from `classify`, so it cannot be wrong.
- **Acceptance:** the step's first line is the summary. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a summary line that counts crates is wrong the moment a crate is added, which is exactly
  when it is read. Fallback: it is computed from `classify` in the same run, so it cannot be stale.

### PLT-LAYER-013 — The layering rule does not check build scripts, examples or benches
- **State:** READY
- **Depends on:** PLT-LAYER-001
- **Files:** `xtask/src/layers.rs::check` (owner **PLT**)
- **Contract:** unchanged behaviour, made explicit: `DepKind::Build`, `Dev` and `Normal` are all
  checked (`layers.rs:160-190`); examples and benches are not modelled at all, because
  `cargo metadata` reports them as targets, not as crates.
- **Behaviour:** An **example** that reaches upward (a `use cadcraft_engine::…` in an example of
  `cadcraft-render`) is not caught today. This task records that limitation, and adds the cheap check:
  examples' `Cargo.toml` files (a per-crate `examples/` manifest) are scanned for workspace dependencies,
  and a violation is reported **as a warning with the crate name** — because an example legitimately
  demonstrates a higher-layer API and a hard failure there would be wrong.
- **Refusals:** a hard failure on an example is refused (examples exist to use the public API of
  whatever layer they belong to, which may be high).
- **Tests:** `a_build_dependency_is_checked` (shared with `PLT-LAYER-008`). `an_example_with_an_
  upward_dependency_is_a_named_warning`. `benches_are_not_modelled` — asserted, so the limitation is
  a fact rather than an oversight.
- **Acceptance:** the limitation is in `docs/architecture.md` §3. Effect on `cargo xtask ci` wall time:
  **+0.2 s**.
- **Risk:** the scan misses an example that adds a dependency dynamically, and a false "clean" is worse
  than no scan. Fallback: the limitation is stated as a limitation in the doc (targets are not
  modelled), so nobody reads the scan as a proof.

### PLT-LAYER-014 — Close-out: one architecture page that cannot drift from the table
- **State:** READY
- **Depends on:** PLT-LAYER-002, PLT-LAYER-010, PLT-LAYER-012
- **Files:** `docs/architecture.md` (owner **PLT**), `xtask/src/layers.rs` (owner **PLT**)
- **Contract:** `docs/architecture.md` §3 documents: the layer table (generated-equal), the intra-layer
  chains and why each exists, `UI_MIN_LAYER` and `UI_CRATES`, the three special classes, the four
  exemptions and what each is trusted to do, the two registration sites for a new crate, and the **five
  things the rule does not check** (`PLT-LAYER-008`, `013`, plus targets and cyclic dependencies).
- **Behaviour:** One page, generated-equal by test, stating the rule **and its blind spots**. The blind
  spots are the valuable half: a reader who knows what is not checked can reason about the rest.
- **Refusals:** a doc that claims completeness is refused — every blind spot named in this task must
  appear.
- **Tests:** `the_documented_layers_match_the_table`. `the_doc_names_every_blind_spot`. `every_link_
  in_the_doc_resolves`.
- **Acceptance:** the page exists and the three tests pass. Effect on `cargo xtask ci` wall time:
  **+0.3 s** (the link check is one pass over one file).
- **Risk:** a page that enumerates the rule engine becomes a second thing to update. Fallback: the
  page's layer table is checked for **equality** with `TABLE` rather than paraphrased, so drift is a
  test failure and the prose around it is advisory.

---

# PLT-DOC — the documents are part of the product

The house rule is craftrules' `standards/progress-docs.md`, and it is specific: the assessment is
`docs/target-app-parity.md`, the work list is `docs/gaps.md`, the milestones are
`docs/roadmap.md`, the one-page summary is `ROADMAP.md`, and deep checklists are
`docs/{geometry,ui,file-format,hardware,localization}-parity.md`. Every one of those files carries a
`> **Last reviewed:** … · **Last updated:** … · **Change:** … · **Target:** …` header
(`docs/gaps.md:3`, `docs/roadmap.md:3`) and a revision history. This epic keeps that structure honest
and adds the checks that make a documentation claim falsifiable.

### PLT-DOC-001 — The docs map: which file answers which question, and a test that every link resolves
- **State:** READY
- **Depends on:** PLT-LAYER-014
- **Files:** `docs/README.md` (new; owner **PLT**), `xtask/src/docs.rs` (new; owner **PLT**)
- **Contract:**
  ```rust
  pub struct DocEntry { pub path: String, pub answers: &'static str, pub owner: &'static str }
  pub const DOCS: &[DocEntry] = &[ /* the 22 files under docs/ plus ROADMAP.md and PROGRESS.md */ ];
  pub fn broken_links(root: &Path) -> Result<Vec<String>, String>;   // every ](link) in every doc
  pub fn unlisted_docs(root: &Path) -> Result<Vec<String>, String>; // a doc not in DOCS
  ```
- **Behaviour:** `docs/` has 22 entries today (the listing `AGENTS.md` shows) and a reader who finds
  one of them does not know what it is for or who owns it. `docs/README.md` is a table: file → the
  question it answers → its owner. The link check walks every Markdown file under `docs/`, `AGENTS.md`,
  `README.md`, `ROADMAP.md`, `PROGRESS.md` and `CLAUDE.md`, resolves every relative link, and fails
  on a broken one — including the **very common** case of a doc that links to a `docs/plan/*.md` file
  that is gitignored (so the link works locally and fails in CI).
- **Refusals:** an external link (http) is not fetched (no network in CI); a link to a path outside
  the repository is refused naming it.
- **Tests:** `broken_links_is_empty` — run over the repository; this is the acceptance. `every_doc_is_
  listed_in_the_map`. `a_link_to_a_gitignored_file_is_found` — a synthetic case, because this is the
  failure mode that actually happens. `an_external_link_is_not_fetched`.
- **Acceptance:** `docs/README.md` exists; the link check is in `ci`. Effect on `cargo xtask ci` wall
  time: **+0.5 s** (a few dozen Markdown files, ~1 MB).
- **Risk:** a doc links to a file that is planned but not written, and the check blocks the commit.
  Fallback: the check distinguishes a link to a **missing** file (fails) from a link inside a
  `TODO`-marked block (skipped), and the map lists planned docs with `"(planned)"` so a link to a
  planned doc is legal.

### PLT-DOC-002 — `docs/gaps.md` is the work list, and a row changes in the PR that closes it
- **State:** READY
- **Depends on:** PLT-DOC-001
- **Files:** `docs/gaps.md` (owner **PLT**); every domain task that closes a gap updates the row
- **Contract:**
  ```rust
  /// A row's shape, checked mechanically: | # | Gap | Evidence | User impact | Est. |
  pub fn gaps_are_ranked_within_their_section() -> bool;
  pub fn gaps_evidence_paths_exist() -> Result<Vec<String>, String>;  // every `path:line` in an Evidence cell
  ```
- **Behaviour:** `docs/gaps.md` is the file agents pick work from ("agents pick from here, top of a
  section first"), so its two properties matter: a row's **Evidence** cell names a real file (and
  often a line — `cmd/blocks/attedit.rs`), and a row that a PR closes is updated **in that PR** (the
  file's own rule, line 7). `gaps_evidence_paths_exist` resolves every backticked path in an Evidence
  cell, so a row cannot cite a file that was renamed.
- **Refusals:** a row citing a path that does not exist fails the check; a row with an "Est." outside
  `1–200` hours fails (it is a judgement with a scale).
- **Tests:** `gaps_evidence_paths_exist` — the acceptance. `the_rows_are_ranked_within_a_section`.
  `the_header_carries_reviewed_updated_change_and_target` — the `> **Last reviewed:**` line
  (`docs/gaps.md:3`) must be present and its date not older than the last change to the file.
- **Acceptance:** the check runs in `ci`. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** the line numbers in Evidence cells go stale with every edit. Fallback: a line number that
  is out of date is **not** a failure (only a missing *file* is); the line is a convenience.

### PLT-DOC-003 — `docs/target-app-parity.md`: the numbers, and the two-number rule
- **State:** READY
- **Depends on:** PLT-DOC-002
- **Files:** `docs/target-app-parity.md` (owner **PLT**)
- **Contract:**
  ```rust
  /// Per `docs/master-plan.md` §7: coverage and maturity are two numbers, never one.
  pub fn parity_reports_coverage_and_maturity() -> bool;
  pub fn maturity_is_in_range() -> bool;   // 0..=7, the scale in master-plan §7
  ```
- **Behaviour:** The master plan is explicit that a **single percentage is a lie** ("this document
  will not carry a global percentage", `master-plan.md:42`), and that coverage and maturity are
  reported separately (`§7`). This task is the check that the parity doc keeps that shape: a coverage
  number with its denominator, a maturity number in `0..=7`, and never a single "X % done" headline
  without the second number beside it.
- **Refusals:** a single percentage reported alone is refused — it is the specific failure the
  standard names.
- **Tests:** `parity_reports_coverage_and_maturity`. `maturity_is_in_range`. `no_bare_percentage_
  headline` — a regex over the doc's headings.
- **Acceptance:** the check passes and the doc's headline carries both numbers. Effect on
  `cargo xtask ci` wall time: **0 s** (one file).
- **Risk:** a future summary wants one number. Fallback: the check names the rule, so the violation is
  a **failed CI step** with a pointer to `master-plan.md` §7.

### PLT-DOC-004 — `docs/roadmap.md`: milestones, the alpha gate, and the current focus
- **State:** READY
- **Depends on:** PLT-DOC-003
- **Files:** `docs/roadmap.md` (owner **PLT**); `ROADMAP.md` and `PROGRESS.md` mirror it
- **Contract:**
  ```rust
  pub fn roadmap_has_current_focus_alpha_gate_and_review_header() -> bool;
  pub fn roadmap_rows_reference_a_gap_id() -> bool;   // each focus item cites a gaps.md id (FF1, F5, U3…)
  ```
- **Behaviour:** `docs/roadmap.md` has three structures that must stay: **Current focus** (numbered,
  estimated, with the gap ids they close), the **alpha gate** table (`docs/roadmap.md:27-40`, whose
  rows are "works end to end?" with evidence), and the `> **Last reviewed:**` header. The check
  asserts the focus items cite gap ids, which is what keeps the roadmap and the work list connected —
  a focus item that closes nothing measurable is removed.
- **Refusals:** a focus item with no gap id is refused; a gate row with no evidence cell is refused.
- **Tests:** `roadmap_rows_reference_a_gap_id`. `the_alpha_gate_rows_have_evidence`. `the_review_header_
  is_present_and_current`.
- **Acceptance:** the three checks pass. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the gate table is a judgement and a stale row is worse than no row. Fallback: the review
  header's date is checked against the file's mtime in git, so an unreviewed table is visible.

### PLT-DOC-005 — `ROADMAP.md` and `PROGRESS.md`: the one-page summary and its mirror rule
- **State:** READY
- **Depends on:** PLT-DOC-004
- **Files:** `ROADMAP.md`, `PROGRESS.md` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn summary_mirrors_the_roadmap() -> bool;      // same stage, same headline numbers
  pub fn progress_counts_match_xtask_stats() -> Result<Vec<String>, String>;
  ```
- **Behaviour:** `AGENTS.md` says `ROADMAP.md` is "the one-page summary" and "follows craftrules'
  standards/progress-docs.md"; `PROGRESS.md` is a running progress log. The second test is the useful
  one: the counts in `PROGRESS.md` (crates, tests, lines) are recomputed by `cargo xtask stats`
  (`stats.rs:65`), so a stale number fails instead of misleading. Running `xtask stats` inside a test
  is a process spawn, so this runs in `ci` as a **step output comparison**, not a unit test.
- **Refusals:** a number in `PROGRESS.md` that `xtask stats` does not produce is refused (rather than
  guessed).
- **Tests:** `summary_mirrors_the_roadmap` — the stage and the headline numbers appear in both.
  `progress_counts_match_xtask_stats` — run in `ci`, comparing the two sources.
- **Acceptance:** the counts are current. Effect on `cargo xtask ci` wall time: **+2 s** (one
  `xtask stats` run, no Rust build).
- **Risk:** two files to keep in sync is exactly what breaks. Fallback: the summary **links** the
  detail and repeats only four numbers, which is what a one-page summary is.

### PLT-DOC-006 — The five parity checklists: one row per item, a status line and a revision history
- **State:** READY
- **Depends on:** PLT-DOC-003
- **Files:** `docs/{geometry,ui,file-format,hardware,localization}-parity.md` (owner **PLT**)
- **Contract:**
  ```rust
  pub const PARITY_DOCS: &[&str] = &["geometry", "ui", "file-format", "hardware", "localization"];
  pub fn each_parity_doc_has_a_status_line_and_a_history() -> Result<Vec<String>, String>;
  pub fn no_parity_row_claims_more_than_its_test() -> Result<Vec<String>, String>;
  ```
- **Behaviour:** The house convention (`AGENTS.md`, "When work lands, update the affected rows and the
  docs' status lines and revision history") is enforced for the five files: each has a status line and
  a revision history, and a row that claims a capability names the test or the external check that
  demonstrates it. The second check is the honest one: a parity row saying "works" must cite a test or
  an external reader's verdict, which is the `DAT-014`/`DRW-EXP-016` discipline applied to prose.
- **Refusals:** a row claiming a capability with no citation is refused. **An external-reader claim
  with no report file is refused** — a report that was not produced by a run is not a report
  (`docs/plan/DAT.md` invariant 2).
- **Tests:** `each_parity_doc_has_a_status_line_and_a_history`. `a_row_without_a_citation_is_refused`.
  `an_external_claim_names_its_report_file`.
- **Acceptance:** the five files pass. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** a citation that later becomes wrong (a test renamed). Fallback: a citation is a path; a
  broken path is found by `PLT-DOC-001`'s link check.

### PLT-DOC-007 — `cargo xtask parity` regenerates the checklist, and a hand edit is a CI failure
- **State:** READY
- **Depends on:** PLT-CI-001
- **Files:** `xtask/src/parity.rs`, `docs/parity-checklist.md` (owner **PLT**)
- **Contract:**
  ```rust
  pub const OUT: &str = "docs/parity-checklist.md";                  // unchanged (parity.rs:29)
  pub fn leaves(catalog: &str) -> Vec<String>;                        // unchanged, pure, tested
  pub fn is_generated(text: &str) -> bool;                            // the "> **Generated:**" header
  pub fn check_fresh(root: &Path, catalog: &str, commands_json: &str) -> Result<bool, String>;
  ```
- **Behaviour:** The file is generated (`parity.rs:95` writes a
  `> **Generated:** {date} by \`cargo xtask parity\`. Do not edit by hand.` header). The gap is that
  nothing checks whether the committed file matches the commands, so it goes stale silently and its
  percentage becomes a claim about a past. `check_fresh` compares the committed file's **content
  minus the date line** with what would be generated now, so a day passing is not a diff.
- **Refusals:** a missing catalogue (`xtask/data/menu-catalog.txt`) is refused naming it; a
  hand-edited file is refused with the first differing line.
- **Tests:** `leaves_with_paths` (existing, `parity.rs:125`). `the_checklist_is_not_hand_edited` —
  the header assertion. `a_stale_checklist_is_detected` — a synthetic case. `the_percentage_is_
  recomputed_not_remembered`.
- **Acceptance:** a stale `docs/parity-checklist.md` fails `ci` with "run `cargo xtask parity`".
  Effect on `cargo xtask ci` wall time: **+1 s** (one `cadcraft-cli commands` spawn — or reuse the
  binary the `test` step already built, which is the cheaper form this task should take).
- **Risk:** the check needs the command registry, which means a built binary. Fallback: run it in the
  `test` step (the binary is built there) rather than adding a step, and skip with a named reason if
  the binary is absent.

### PLT-DOC-008 — The menu catalogue: its provenance, and how it grows
- **State:** READY
- **Depends on:** PLT-DOC-007
- **Files:** `xtask/data/menu-catalog.txt` (owner **PLT**); the reference app's menu tree is
  `plan/autocad/04-menu-tree.txt` — **gitignored**, so it never enters the repo
- **Contract:**
  ```rust
  pub fn is_catalogue_line_valid(line: &str) -> bool;   // indentation is spaces; leaves are >= 1 level
  pub fn catalogue_has_no_tabs() -> bool;
  ```
- **Behaviour:** The catalogue is a **list of menu item names**, observed black-box from the reference
  app (`AGENTS.md`: "names/listings only", and screenshots live only in `plan/autocad/screenshots/`).
  It is the one committed artefact derived from observation, so its rules are stated: indentation with
  spaces (the parser counts leading whitespace, `parity.rs:16`), no tabs, a leaf is any line followed
  by a line at the same or lower indent (`parity.rs:20-25`), and blank lines are ignored.
- **Refusals:** a tab-indented line is refused naming the line number; a line with trailing
  whitespace is refused (it changes the leaf string); adding an item requires the observation note
  beside it.
- **Tests:** `catalogue_lines_use_spaces`. `a_tab_is_refused`. `the_catalogue_parses_to_leaves` —
  round trip through `leaves`. `every_catalogue_entry_has_a_note` — a count assertion against a
  committed count of observed items.
- **Acceptance:** the catalogue is valid and its count matches the note. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** the catalogue grows from a *name* list into an assessment. Fallback: the file holds names
  only; the assessment is `docs/target-app-parity.md`.

### PLT-DOC-009 — The agent prompts: one system prompt, specialised ones that bind it
- **State:** READY
- **Depends on:** —
- **Files:** `docs/agents/SYSTEM_PROMPT.md`, `docs/agents/*.md` (owner **PLT**)
- **Contract:** `docs/agents/SYSTEM_PROMPT.md` is "the single source of truth for that prompt"
  (its own line 3); every specialised prompt begins by **binding** it verbatim and then adds only its
  own lot. A check asserts that each specialised file's first section quotes the bound text's
  version marker.
- **Behaviour:** The prompts are how this whole programme executes. Their one non-negotiable content
  is that an agent has **no terminal** and must therefore never claim execution. The version marker
  (a line in the bound section) is what lets a stale copy be detected.
- **Refusals:** a prompt that **paraphrases** the system prompt is refused (the file says so
  explicitly: "Do not paraphrase it: an agent that receives a weakened copy is an agent that will
  guess").
- **Tests:** `every_specialised_prompt_binds_the_system_prompt`. `the_system_prompt_states_no_
  execution_claims`. `the_version_marker_matches`.
- **Acceptance:** the checks pass; a new domain index (like this one) names its system prompt.
  Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** an agent is spawned with only the specialised lot. Fallback: every specialised prompt's
  binding section is quoted in full, which the check enforces.

### PLT-DOC-010 — The master plan and the per-domain indexes: the update rule
- **State:** READY
- **Depends on:** PLT-DOC-009
- **Files:** `docs/master-plan.md`, `docs/plan/*.md` (owner **PLT** for the plan; each index's own
  owner for its content)
- **Contract:**
  ```rust
  pub const INDEXES: &[&str] = &["KER", "SKT", "MOD", "ASM", "DRW", "SIM", "CAM", "DAT", "REN",
                                "UI", "QAT", "PLT"];
  pub fn every_task_id_is_unique_across_indexes() -> Result<Vec<String>, String>;
  pub fn every_dependency_names_an_existing_task() -> Result<Vec<String>, String>;
  ```
- **Behaviour:** The master plan says the per-domain indexes carry the expanded task lists and that
  the master plan carries the structure and the index the count. With eight indexes and hundreds of
  tasks, two mechanical facts must hold and neither is checked: **a task id appears exactly once**
  (so `KER-BOOL-004` is one task), and **a `Depends on:` that names a task names a task that exists**
  (so a `BLOCKED` task's prerequisite is real, not a typo).
- **Refusals:** a duplicate id is refused naming both files and lines; a dependency naming an unknown
  task is refused naming the id and the file; a cross-domain reference in the `NEEDS KER-…` form is
  allowed (a kernel task not yet written is a stated block, not a typo) **only** if the id matches
  `^(KER|MOD|ASM|DRW|SIM|CAM|DAT|REN|UI|QAT|PLT)-[A-Z]+-\d{3}$`.
- **Tests:** `every_task_id_is_unique_across_indexes`. `every_dependency_names_an_existing_task`. `a_
  cross_domain_contract_id_is_well_formed`. `every_index_has_a_task_count_table` — the tables at the
  end of each index (`DAT.md:1791-1804`).
- **Acceptance:** the checks pass over all indexes; a new index must add itself to `INDEXES`.
  Effect on `cargo xtask ci` wall time: **+1 s** (a few thousand lines of Markdown, pure).
- **Risk:** a cross-domain block names a task the other domain never planned (as `DRW.md`'s KER
  requests do). Fallback: the id's *form* is checked, and the coordinator reconciles the set once the
  other index lands — which is the normal sequence.

### PLT-DOC-011 — The two agent-facing surfaces: the control channel and MCP
- **State:** READY
- **Depends on:** —
- **Files:** `docs/control-protocol.md`, `docs/mcp.md`, `crates/ui-egui/src/control.rs::METHODS`,
  `crates/mcp/**` (owner **PLT** for the docs; **U** for `control.rs`; **C** for `mcp`)
- **Contract:**
  ```rust
  pub fn every_control_method_is_documented() -> Result<Vec<String>, String>;
  pub fn every_mcp_tool_is_documented() -> Result<Vec<String>, String>;
  ```
  `control.rs:191` holds the `METHODS` table (method, params, description) and the doc is the prose
  around it.
- **Behaviour:** The control channel (`crates/ui-egui/src/control.rs`) and the MCP server
  (`crates/mcp`) are **how an agent drives this application**, so their docs are load-bearing: an
  agent reads `docs/control-protocol.md` and types JSON lines. Every method in `METHODS` must appear
  in the doc with its params, and every MCP tool in `docs/mcp.md`.
- **Refusals:** a method in `METHODS` missing from the doc is refused naming it; a documented method
  not in `METHODS` is refused too (a doc that promises a method that does not exist is worse than a
  missing doc).
- **Tests:** `every_control_method_is_documented`. `every_mcp_tool_is_documented`. `the_documented_
  method_count_matches_the_table`.
- **Acceptance:** both pass; the docs carry a method count. Effect on `cargo xtask ci` wall time:
  **+0.5 s**.
- **Risk:** `METHODS` grows with every command. Fallback: the check makes growth cheap — adding a
  method without a doc line fails CI, which is the point.

### PLT-DOC-012 — `docs/architecture.md` versus the local `plan/architecture.md`
- **State:** READY
- **Depends on:** PLT-LAYER-002
- **Files:** `docs/architecture.md` (owner **PLT**)
- **Contract:** the committed architecture document holds the layering section (`PLT-LAYER-002`),
  the crate map and the module boundaries; the local `plan/architecture.md` (gitignored) holds
  working notes. Nothing in a committed file may point at the gitignored one.
- **Behaviour:** `AGENTS.md` tells agents to read `plan/architecture.md`, which is **local only** —
  so an agent on another machine, and a contributor, do not have it. The committed `docs/
  architecture.md` becomes the reference, and `AGENTS.md`'s reference is updated to both (local notes
  and the committed reference). The layering table is documented **from** `TABLE`
  (`PLT-LAYER-002`), so it cannot drift.
- **Refusals:** a committed doc linking to `plan/**` is refused (it is the exact bug this task
  fixes — `PLT-DOC-001`'s link check finds it).
- **Tests:** `no_committed_file_links_into_plan` — a grep over `docs/`, `*.md` at the root, and
  `crates/**/src/**.rs` doc comments. `the_committed_architecture_has_the_layering_section`.
- **Acceptance:** no committed file links into `plan/`. Effect on `cargo xtask ci` wall time: **+0.5 s**
  (one grep over the sources' doc comments).
- **Risk:** the local notes and the committed doc drift. Fallback: the committed doc is generated-equal
  for the layering section and is the one referenced from `AGENTS.md`.

### PLT-DOC-013 — `docs/releasing.md` and the playbook it defers to
- **State:** READY
- **Depends on:** —
- **Files:** `docs/releasing.md` (owner **PLT**)
- **Contract:** the file holds **only** what is CADCraft-specific (version source, release trigger, the
  twelve artefacts, the twelve signing secrets, the Windows wizard's shortcut page, the file types,
  the local packaging scripts) and links craftrules' `release/playbook.md` for the general recipe
  (`docs/releasing.md:3`). Its content is verified by `PLT-REL-001..012`.
- **Behaviour:** Nothing new; this task is the check that the file still matches the workflow: the
  trigger (`git push origin main:release`), the artefact list (`docs/releasing.md:7`), and the secrets'
  optionality (line 8) must match `release.yml` and `PLT-REL-008`.
- **Refusals:** a documented artefact the workflow does not build is refused; a built artefact the doc
  omits is refused. The doc and the pipeline are one fact.
- **Tests:** `every_documented_artefact_is_built` — the list compared with the workflow's jobs.
  `every_built_artefact_is_documented`. `the_doc_links_the_playbook`.
- **Acceptance:** the check passes against `release.yml`. Effect on `cargo xtask ci` wall time: **+0.2 s**
  (one YAML parse, std-only — no YAML dependency: compare the job names as text).
- **Risk:** the artefact list is compared as **text**, so a reworded job name fails the check for a
  cosmetic change. Fallback: the comparison is on a normalised job-name list and the message names
  both sides, so a cosmetic rename is a one-line fix.

### PLT-DOC-014 — The doc checks are one step, not eight
- **State:** READY
- **Depends on:** PLT-DOC-001, PLT-DOC-002, PLT-DOC-003, PLT-DOC-006, PLT-DOC-010
- **Files:** `xtask/src/ci.rs`, `xtask/src/docs.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn docs_step() -> Result<(), String>;   // runs every docs check in one process
  // CI_STEPS gains ("docs", …) after ("layers", …) and before ("wasm", …)
  ```
- **Behaviour:** Eight separate steps would add eight process spawns and eight summaries. One `docs`
  step runs every check in-process and prints `docs: 11 checks, 0 failures`, naming each check on
  failure. The step is **pure std** (no Markdown parser dependency): the checks are line/regex
  passes, which is why they are cheap enough to always run.
- **Refusals:** a docs check that needs a network fetch is refused (CI has no network guarantee for
  links).
- **Tests:** `docs_step_runs_every_check`. `a_failing_docs_check_names_itself`. `the_step_is_pure_std`.
- **Acceptance:** one step, one summary line. Effect on `cargo xtask ci` wall time: **+2 s total** for
  all eleven documentation checks (this is the aggregate; each task above states its own share).
- **Risk:** one step means one failure hides the other ten checks, and a contributor fixes one and is
  surprised by the next. Fallback: `docs_step` runs **every** check and reports all failures with
  their names in one block, not the first.

### PLT-DOC-015 — `ATTRIBUTION.md` is documentation with teeth: every row is checkable
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `ATTRIBUTION.md`, `xtask/src/assets.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn attribution_rows_are_well_formed(md: &str) -> Vec<String>;  // 5 columns, a path, a licence
  pub const REQUIRED_COLUMNS: &[&str] = &["Asset", "Author", "Source", "Licence", "Notes"];
  ```
- **Behaviour:** `cargo xtask assets` checks that an asset **path** appears in the file
  (`assets.rs:22-24`) — it does not check that the row says anything. The row's content is the
  attribution itself, so a row that is a bare path is worthless. This check asserts every row has the
  five columns, a licence that names something (`MIT`, `Apache`, `CC0`, an ArtCraft brand term), and
  an author that is not empty.
- **Refusals:** a row with a missing column is refused naming the row; a licence of "free" or
  "open" is refused (it names nothing); a row for a file that does not exist is refused (a stale row
  is a false claim).
- **Tests:** `attribution_rows_are_well_formed`. `every_row_names_a_licence`. `no_row_names_a_missing_
  file`. `the_generated_in_code_list_is_a_list_not_a_table` — the five generated assets
  (`ATTRIBUTION.md:16-22`) have no file to attribute, and the check asserts they are **not** in the
  table.
- **Acceptance:** every row is checkable and the generated assets stay file-free. Effect on
  `cargo xtask ci` wall time: **+0.2 s** (one file, 60 rows).
- **Risk:** the brand rows (`docs/brand/`) are trademarks, not open source, and their licence cell
  says so. Fallback: `LICENSE-brand.txt` is accepted as a licence value for those rows, and the
  check asserts it is the **only** non-open licence in the file.

### PLT-DOC-016 — A new crate ships with a README that says what it does not do
- **State:** BACKLOG
- **Depends on:** PLT-DOC-001, PLT-LAYER-004
- **Files:** `crates/*/README.md` (new, one per crate; owner each domain, **PLT** for the rule)
- **Contract:**
  ```rust
  pub const CRATE_READMES_REQUIRED: &[&str] = &["solid", "model", "io", "doc", "render", "engine",
                                             "drawing", "asm", "sim", "cam"];
  pub fn readme_states_the_layer_and_the_limits() -> bool;
  ```
- **Behaviour:** The house convention is that a public item's doc says what it does **not** do
  (`crates/solid/src/lib.rs:35-51` is the reference). A crate README states the same at crate level:
  the layer, the one-line purpose, the **limits**, and the sibling crates it must not depend on. It is
  the document a new agent reads before touching the crate.
- **Refusals:** a README without a limits section is refused. **Generated documentation is not a
  README**: `cargo doc` output is not committed, and a README that duplicates it rots.
- **Tests:** `every_required_crate_has_a_readme`. `each_readme_states_its_layer`. `each_readme_has_
  a_limits_section`. `no_readme_duplicates_rustdoc` — a crude check that the README has no
  `pub fn` signature lines.
- **Acceptance:** the ten crates have one each; `DRW.md`'s `DRW-VIEW-001` writes
  `crates/drawing/README.md` as part of the crate skeleton. Effect on `cargo xtask ci` wall time:
  **+0.2 s**.
- **Risk:** eleven READMEs to keep current. Fallback: each README is under 40 lines and links to the
  module docs, which are the thing that must stay current anyway.

---

# PLT-PERF — measure, record the baseline, and fail the build on a regression

No new dependency: `criterion` is **not** added. A benchmark in this workspace is a binary or a test
that times itself with `std::time::Instant` and writes a JSON line, and the comparison is a text file.
This epic establishes that machinery and the rules for using it, and states the effect each task has
on `cargo xtask ci`.

### PLT-PERF-001 — The seam: the caller supplies the clock
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/perf.rs` (new; owner **PLT**)
- **Contract:**
  ```rust
  pub trait Clock { fn now(&self) -> f64; }         // seconds, monotonic
  pub struct InstantClock;                          // std::time::Instant; panics on wasm (PLT-WASM-005)
  pub struct ManualClock(pub std::cell::Cell<f64>); // tests
  pub fn measure<T>(clock: &dyn Clock, budget_ms: f64, f: impl FnOnce() -> T) -> Result<(T, f64), String>;
  ```
- **Behaviour:** Every measurement in the workspace goes through `Clock`, so a test can drive time
  deterministically (`ManualClock`) and the wasm build compiles (a `Clock` implementation is the only
  place `Instant` appears, and the web build supplies a `Clock` that reports the browser's clock or,
  failing that, refuses). `measure` returns the value **and** the elapsed time, and refuses when the
  budget is exceeded only if the caller asks it to (`budget_ms` is reported, not enforced by default).
- **Refusals:** a clock that returns a non-finite or decreasing time is refused (`Err("the clock went
  backwards")` — the same rule `DAT-012` states for the STEP timestamp); a `budget_ms` that is
  negative is refused.
- **Tests:** `measure_returns_the_value_and_the_time`. `a_backwards_clock_is_refused`. `a_negative_
  budget_is_refused`. `the_manual_clock_drives_a_deterministic_test`.
- **Acceptance:** `xtask` compiles for wasm (it is `Exempt` and **not** in the wasm set — so this task
  also states that `perf.rs` is host-only and says so in its module doc). Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** a benchmark that times itself is distorted by the timing. Fallback: run each case in its
  own process (`std::process::exit` after reporting) so one case's allocation does not warm another's
  cache — `PLT-PERF-006` requires it.

### PLT-PERF-002 — The benchmark harness: a binary, not a dependency
- **State:** BLOCKED — needs `KER-PERF-001`, which owns `crates/solid/benches/**` (`KER.md` §0.5)
- **Depends on:** PLT-PERF-001, KER-PERF-001
- **Files:** `crates/solid/benches/**` (owner **KER-PERF**, per `KER.md` §0.5 — **not** PLT's),
  `xtask/src/perf.rs` (owner **PLT**)
- **Contract:**
  ```rust
  // in xtask (PLT's file, the shared machinery):
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct Sample { pub name: String, pub n: usize, pub median_ms: f64, pub p95_ms: f64, pub bytes: u64 }
  pub fn summarise(name: &str, mut times_ms: Vec<f64>, bytes: u64) -> Sample;   // median + p95
  pub fn read_baseline(path: &str) -> Result<BTreeMap<String, Sample>, String>;
  pub fn compare(baseline: &BTreeMap<String, Sample>, now: &[Sample], tolerance: f64)
      -> Result<Vec<Regression>, String>;   // slower by > tolerance
  ```
- **Behaviour:** `summarise` reports the **median** and the 95th percentile of `n` runs, which is the
  minimum that makes a wall-time number meaningful on a shared machine. `compare` is the gate: a case
  slower by more than the tolerance (10 %, per `QAT-004`) is a `Regression`. Each case runs in its own
  process so nothing it allocates warms the next case's cache.
- **Refusals:** a case with `n < 5` is refused (a median of three runs is noise); a baseline file with
  a case that no longer exists is **reported and kept** (a removed case is not a regression); a
  baseline file with a malformed line is refused naming the line.
- **Tests:** `summarise_computes_median_and_p95`. `a_case_with_few_runs_is_refused`. `a_ten_percent_
  regression_is_reported`. `a_five_percent_change_is_not`. `a_removed_case_is_kept_in_the_baseline`.
  `a_malformed_baseline_is_refused_naming_the_line`.
- **Acceptance:** the machinery is in `xtask` and `KER-PERF-001` can use it. Effect on
  `cargo xtask ci` wall time: **0 s** (the harness is not run by the gate; `PLT-PERF-011` decides when).
- **Risk:** the kernel's benches are Q/KER-owned, so PLT cannot add them. Fallback: the machinery lives
  in `xtask` (PLT's) and is used by whoever owns the benches; the interface request is recorded.

### PLT-PERF-003 — The baseline file, with its machine and its date
- **State:** READY
- **Depends on:** PLT-PERF-001, PLT-CI-013
- **Files:** `xtask/perf-baseline.toml` (new; owner **PLT**), `docs/roadmap.md` (owner **PLT**)
- **Contract:**
  ```rust
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct Baseline {
      pub machine: String,      // "linux-x86_64, 8 vCPU, rustc 1.90.0, release"
      pub date: String,         // YYYY-MM-DD
      pub cases: BTreeMap<String, Sample>,
  }
  ```
- **Behaviour:** One committed file per benchmark group, each with the machine it was measured on and
  the date, because a wall time on someone else's laptop is not a baseline. The house convention
  (`docs/plan/DAT.md` invariant 2) applied to numbers: *how it was produced* is recorded with it.
  Updating a baseline is a visible diff in a PR, which is the review that a number deserves.
- **Refusals:** a baseline with no `machine` or no `date` is refused (a number without provenance is
  not a measurement); a baseline older than 180 days is reported as stale in `cargo xtask perf`.
- **Tests:** `a_baseline_without_a_machine_is_refused`. `a_baseline_without_a_date_is_refused`. `an_
  old_baseline_is_reported_stale`. `the_baseline_round_trips`.
- **Acceptance:** the file exists with a machine and a date, and `docs/roadmap.md` links it.
  Effect on `cargo xtask ci` wall time: **0 s** — the gate does not run benchmarks unless
  `PLT-PERF-011` says so.
- **Risk:** a baseline from a fast machine makes every future run a "regression". Fallback:
  `compare` uses a **relative** threshold and reports the machine in the message, and re-baselining is
  a one-line diff.

### PLT-PERF-004 — `cargo xtask perf`: run, compare, report
- **State:** BACKLOG
- **Depends on:** PLT-PERF-002, PLT-PERF-003
- **Files:** `xtask/src/perf.rs`, `xtask/src/main.rs::{USAGE, main}` (owner **PLT**)
- **Contract:** `cargo xtask perf [--group solid|drawing|render] [--update-baseline] [--runs n]`.
  Output: one line per case (`name  median  p95  vs baseline`), then `N cases, M regressions`.
- **Behaviour:** It runs the benchmark **binaries** (each a separate process, `PLT-PERF-002`), parses
  their JSON lines on stdout, compares with the baseline and reports. `--update-baseline` rewrites the
  file — deliberately a separate flag, because a run that updates its own baseline can never fail.
- **Refusals:** an unknown group is refused listing the groups; `--update-baseline` with a run that
  produced fewer than `n` samples is refused; a missing baseline with no `--update-baseline` is refused
  with the exact command to create it.
- **Tests:** `perf_reports_one_line_per_case`. `a_regression_is_reported_with_both_numbers`. `update_
  baseline_writes_the_file`. `an_unknown_group_is_refused`. `running_without_a_baseline_is_refused_
  with_the_command`.
- **Acceptance:** `cargo xtask perf` exists and reports. Effect on `cargo xtask ci` wall time: **0 s**
  (not part of the gate unless `PLT-PERF-011` says so).
- **Risk:** it is a fifth command in `USAGE` and the list grows. Fallback: `USAGE` is grouped, and
  every subcommand's line states the gate it belongs to (`PLT-CI-001`).

### PLT-PERF-005 — The gate's own wall time: measured, budgeted, and reported per step
- **State:** READY
- **Depends on:** PLT-CI-008, PLT-CI-013
- **Files:** `xtask/src/ci.rs`, `xtask/ci-budget.toml` (owner **PLT**)
- **Contract:** `PLT-CI-013`'s `StepBudget` and `Verdict`, applied to the six steps, with the budgets
  filled in from a measured run.
- **Behaviour:** The first job is to **measure**: run the gate on a warm cache and a cold one, per
  step, on the three platforms, and write the numbers into `xtask/ci-budget.toml` with the machine and
  the date. Over budget prints `over by N s`; 3× over is flagged as a failure candidate
  (`PLT-CI-008`'s rule).
- **Refusals:** a budget of 0 is refused (an unmeasured budget is not a budget); a budget file whose
  date is over 30 days old prints a staleness note (never fails).
- **Tests:** `every_step_has_a_budget`. `over_budget_is_reported`. `a_stale_budget_is_noted`.
  `the_budget_file_names_its_machine`.
- **Acceptance:** the file exists with six measured rows. Effect on `cargo xtask ci` wall time: **0 s**
  (it measures the gate; it does not change it) — and it is the number that makes `PLT-CI-009`'s and
  `PLT-WASM-004`'s savings checkable.
- **Risk:** the budget is measured on one machine and applied on three. Fallback: the file has a row
  per platform, and the verdict compares against the row for the running platform when it exists.

### PLT-PERF-006 — A benchmark case runs in its own process
- **State:** READY
- **Depends on:** PLT-PERF-001
- **Files:** `xtask/src/perf.rs` (owner **PLT**)
- **Contract:**
  ```rust
  /// Run `f` in a fresh process if possible; otherwise say that the numbers are warm.
  pub fn isolated(case: &str) -> Result<Isolated, String>;
  #[derive(Debug, PartialEq)]
  pub enum Isolated { Child(std::process::Child), Warm }
  ```
- **Behaviour:** In-process benchmarking measures the allocator's warmed state. Each case runs in its
  own process (`cargo xtask perf` spawns the binary once per case) and the case binary exits after
  reporting. When a case cannot be isolated (an in-crate `#[test]`), it is reported as `Warm` and the
  numbers are labelled as such in the output — **never** presented as isolated.
- **Refusals:** a case that claims isolation without it is refused by the test that reads the label.
- **Tests:** `isolated_spawns_a_child`. `a_warm_case_says_warm`. `the_label_is_in_the_output`.
- **Acceptance:** every benchmark line states `cold` or `warm`. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** a process per case is slower to run. Fallback: the cost is seconds, against benchmarks
  that take minutes; and it only runs when `cargo xtask perf` runs.

### PLT-PERF-007 — Release profile: what the binary costs and what the settings buy
- **State:** READY
- **Depends on:** —
- **Files:** root `Cargo.toml` `[profile.release]` (owner **PLT**), `docs/roadmap.md` (owner **PLT**)
- **Contract:** the profile stays `opt-level = 3`, `lto = "thin"`, `codegen-units = 16`, `debug = 0`
  (`Cargo.toml:74-78`) unless this task's measurement says otherwise; the measured build time and
  binary size are written into `docs/roadmap.md` with the machine and the date.
- **Behaviour:** The task is **to measure and record**, not to tune: `cargo build --release` wall time
  and the binary size, with `lto = "fat"` and `codegen-units = 1` as the two settings that would cost
  the most time. The decision to keep the current profile is a decision made with numbers, which is
  the house rule.
- **Refusals:** a profile change without a measured before/after is refused. `strip = true` is
  **refused**: symbols are what a crash report needs (`AGENTS.md`'s crash-safety priority), and the
  size saving is not worth losing them.
- **Tests:** `the_profile_matches_the_recorded_settings` — the doc's numbers and the manifest agree.
  `release_does_not_strip` — a grep over the profile.
- **Acceptance:** the numbers are in `docs/roadmap.md` with a date. Effect on `cargo xtask ci` wall
  time: **0 s** (the gate does not build `--release`).
- **Risk:** someone flips a setting and CI's clippy run gets slower. Fallback: the recorded build time
  is the reference, and `PLT-PERF-005`'s budgets are the visible symptom.

### PLT-PERF-008 — Dev profile: `opt-level = 1` in dev, 2 in dependencies
- **State:** READY
- **Depends on:** —
- **Files:** root `Cargo.toml` `[profile.dev]`, `[profile.dev.package."*"]` (owner **PLT**)
- **Contract:** unchanged — `dev` is `opt-level = 1` and `dev.package."*"` is `opt-level = 2`
  (`Cargo.toml:68-72`) — and this task records **why**, plus what it costs: a clean `cargo build`
  time and an incremental-compile time for one crate.
- **Behaviour:** The settings exist so the CPU rasteriser (`render`) is usable in a debug build —
  `opt-level = 0` makes `scene3` roughly an order of magnitude slower, which makes the geometry
  validation work unusable. The task records the measurement that justifies it, so a future contributor
  does not "clean this up".
- **Refusals:** setting `opt-level = 0` anywhere in `dev` is refused for the same reason.
- **Tests:** `the_dev_profile_optimises_dependencies`. `the_dev_profile_is_not_zero`.
- **Acceptance:** the reasoning and the number are in `docs/roadmap.md`. Effect on `cargo xtask ci`
  wall time: **0 s** (the gate builds the default profile, which is what these settings affect — the
  recorded numbers are for developers).
- **Risk:** a debug build is slower to run but faster to build, and the gate uses it. Fallback: the
  numbers are recorded so the trade is visible.

### PLT-PERF-009 — Cold-cache CI time, warm-cache CI time, and the ratio
- **State:** READY
- **Depends on:** PLT-CI-005, PLT-PERF-005
- **Files:** `docs/roadmap.md`, `xtask/ci-budget.toml` (owner **PLT**)
- **Contract:** two budget rows per step: `seconds_cold` and `seconds_warm`, both with the machine and
  the date.
- **Behaviour:** The two numbers answer different questions: cold is what a **contributor's first CI
  run** costs, warm is what every subsequent run costs. Reporting only one of them is how a project
  ends up with a 40-minute gate that nobody notices is 6 minutes warm.
- **Refusals:** a warm figure without a cold one (or the reverse) is refused when the file is read.
- **Tests:** `both_numbers_are_present_per_step`. `the_warm_number_is_lower_than_the_cold` — asserted
  on the real data, which is a property of caching and therefore a bug detector.
- **Acceptance:** the file carries both. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the cold number is dominated by runner queueing, not by the work. Fallback: the cold number
  is measured from the step's own elapsed time inside the job, not the job's wall clock.

### PLT-PERF-010 — The `test` step's wall time: what dominates it, and the honest split
- **State:** BACKLOG
- **Depends on:** PLT-PERF-005
- **Files:** `docs/roadmap.md`, `xtask/ci-budget.toml` (owner **PLT**)
- **Contract:** a recorded split of the `test` step: build time, per-crate test time (from
  `cargo xtask stats --exact` and per-binary timing), and the slowest ten tests.
- **Behaviour:** The measurement that decides whether the gate can afford to grow. `cargo test
  --workspace` builds everything then runs everything; the split says which dominates, and therefore
  whether the answer is "faster tests" or "sharding" or "fewer tests" (the last one being the correct
  answer more often than anyone admits).
- **Refusals:** a recommendation to shard without the split is refused — sharding a build-bound step
  costs more time than it saves.
- **Tests:** the split is recorded in `docs/roadmap.md` with a date, a machine, and the two component
  numbers; a doc whose split has no machine is refused.
- **Acceptance:** the split is written down with its date and machine. Effect on `cargo xtask ci` wall
  time: **0 s** (measurement only — it changes nothing the gate does).
- **Risk:** the slowest tests are the property tests (`crates/solid/tests/property.rs`), which are the
  ones worth running. Fallback: the split names them and the recommendation is to keep them, not to
  remove them.

### PLT-PERF-011 — What fails the build on a regression, and what is reported
- **State:** READY
- **Depends on:** PLT-PERF-002, PLT-PERF-003
- **Files:** `xtask/src/perf.rs`, `xtask/src/ci.rs` (owner **PLT**)
- **Contract:**
  ```rust
  pub enum Policy { Report, Fail { tolerance: f64 } }
  pub fn apply(p: Policy, regressions: &[Regression]) -> Result<(), String>;
  ```
- **Behaviour:** The decision, stated: **a benchmark regression of more than 10 % fails `ci`** (that is
  `QAT-004`, which the master plan states as the rule), and everything else is reported. Two
  exemptions, both explicit: a case whose `n < 5` is never a failure (noise), and a **shared CI runner**
  may only `Report`, because a noisy machine makes a 10 % threshold a coin flip — so the gate enforces
  it on a dedicated runner and reports it elsewhere, and the policy says which mode it is in.
- **Refusals:** a `Fail` policy on a runner whose variance exceeds the tolerance is refused (the
  variance is measured by running the same case twice).
- **Tests:** `over_ten_percent_fails`. `under_ten_percent_reports`. `a_low_n_case_never_fails`. `a_
  shared_runner_is_report_only`. `the_policy_is_printed_in_the_summary`.
- **Acceptance:** the policy is stated in `docs/roadmap.md` and in the gate's summary. Effect on
  `cargo xtask ci` wall time: **+T s** where T is the benchmark group's runtime (this is the only task
  that can make the gate minutes longer, and it is the one that makes a regression visible at all).
- **Risk:** the 10 % rule makes contributors afraid to touch a hot path. Fallback: a regression can be
  waived with a one-line reason **in the PR**, recorded next to the baseline — the same discipline
  `PLT-CI-014` uses for `#[ignore]`.

### PLT-PERF-012 — Binary and artefact size: measured per release, recorded once
- **State:** BACKLOG
- **Depends on:** PLT-PERF-007, PLT-WASM-010
- **Files:** `docs/roadmap.md`, `packaging/**` (owner **PLT**)
- **Contract:** the release artefacts' sizes are recorded with the version and the date: macOS `.dmg`,
  Windows `.msi`, Linux `.AppImage`, the CLI zip, the web zip (`docs/releasing.md:7`). No gate.
- **Behaviour:** Size is a user-visible property (a 300 MB download is a real cost) and it is not
  currently measured. The measurement reads the files the release produced, so it is a release-time
  task rather than a CI step.
- **Refusals:** a size recorded without a version and a date is refused.
- **Tests:** the recorded row has a version and a date; a row with neither is refused by
  `PLT-PERF-003`'s provenance rule.
- **Acceptance:** the sizes are in `docs/roadmap.md` with the version and the date they describe.
  Effect on `cargo xtask ci` wall time: **0 s** (a release-time measurement, not a gate).
- **Risk:** the numbers drift silently. Fallback: the release workflow prints them, so a maintainer
  sees them at the moment they change.

### PLT-PERF-013 — Memory: measured where it can be, refused where it cannot
- **State:** BACKLOG
- **Depends on:** PLT-PERF-001
- **Files:** `xtask/src/perf.rs` (owner **PLT**)
- **Contract:**
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub enum Memory { PeakRss(u64), Unavailable(&'static str) }
  pub fn peak_rss() -> Memory;   // Linux: /proc/self/status; macOS/Windows: Unavailable
  ```
- **Behaviour:** `getrusage` needs libc, and **no new dependency** means no `getrusage`. So the
  measurement reads `/proc/self/status` on Linux and is **`Unavailable` elsewhere** — reported as
  unavailable rather than guessed. The alternative, reading the process's own allocator counters, is
  wrong (it does not see the framework). The task's honesty is the point: the memory claim on a
  non-Linux platform is **not made**.
- **Refusals:** `Unavailable` is never converted to 0 — a zero peak is a false claim.
- **Tests:** `peak_rss_is_available_on_linux`. `peak_rss_is_unavailable_elsewhere`. `unavailable_is_
  never_zero`. `the_platform_is_reported`.
- **Acceptance:** the report names the platform. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a memory regression goes unseen on macOS, the roadmap's main platform. Fallback: a
  **manual** measurement procedure in `docs/roadmap.md` (Activity Monitor's footprint, three runs,
  median) with the numbers recorded — a human measurement with a stated method, which the house rules
  accept.

### PLT-PERF-014 — Startup time: what the first frame costs
- **State:** BACKLOG
- **Depends on:** PLT-PERF-001
- **Files:** `xtask/src/perf.rs`, `docs/roadmap.md` (owner **PLT**)
- **Contract:**
  ```rust
  pub fn startup_ms(command: &[&str]) -> Result<Startup, String>;
  #[derive(Debug, Clone, PartialEq)]
  pub struct Startup { pub total_ms: f64, pub first_frame_ms: Option<f64>, pub version_ms: f64 }
  ```
  Measured on `cadcraft --version` (process start to exit) and, where the headless path allows it, on
  `cadcraft-cli run --sample --export out.png`, whose end-to-end time is a first-frame proxy.
- **Behaviour:** A desktop CAD that takes ten seconds to show its window is a product problem; nobody
  measures it, so it is measured here with a stated method and a stated machine.
- **Refusals:** a startup number without the machine and the run count is refused (the same rule as
  every other number).
- **Tests:** `startup_is_measured_five_times_and_reports_the_median`. `version_ms_is_small` — a smoke
  assertion, not a budget. `the_sample_render_is_the_first_frame_proxy`.
- **Acceptance:** the number is in `docs/roadmap.md`. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk: `--version` is not a window. Fallback: the number is labelled `process start`, not "window
  shown", and the label is in the output.

### PLT-PERF-015 — The renderer benchmark, and the property that it is measuring
- **State:** BACKLOG
- **Depends on:** PLT-PERF-002, REN-001
- **Files:** `crates/render/**` (owner **U**), `crates/render/benches/**` (owner **QAT** per
  `master-plan.md` §3 — **not** PLT's)
- **Contract:**
  ```rust
  /// Request to REN/QAT: a scene fixture (the sample drawing's solid, a fixed camera, a fixed
  /// resolution) that every renderer benchmark uses, so a number means "this scene".
  // pub fn scene_fixture() -> (cadcraft_render::scene3::Scene, u32, u32);
  ```
- **Behaviour:** The benchmark's validity is its **fixture**: same scene, same camera, same resolution,
  declared in one place so two runs are comparable. `render`'s own benchmark must use it. This task
  asks REN/QAT for the fixture and states that PLT owns the harness and they own the scene.
- **Refusals:** a benchmark whose fixture is defined inside the benchmark file is refused (it cannot
  be compared to anything).
- **Tests:** `every_render_benchmark_uses_the_shared_fixture`. `the_fixture_is_deterministic`.
- **Acceptance:** the fixture exists in one place. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a scene that is too simple to be representative. Fallback: the fixture is versioned
  (`fixture_v1`), so changing it is a deliberate act that re-baselines.

### PLT-PERF-016 — Close-out: the performance report, with its numbers and its caveats
- **State:** READY
- **Depends on:** PLT-PERF-005, PLT-PERF-009, PLT-PERF-011
- **Files:** `docs/roadmap.md` (owner **PLT**)
- **Contract:** one section, "Performance", carrying: the gate's per-step wall time (cold and warm),
  the release build time and binary size, the benchmark baselines with their machines, the startup
  number, the memory number **with its platform**, and **what is not measured** (macOS/Windows memory,
  wasm size before the web build, GPU time).
- **Behaviour:** The section's last paragraph is the caveat list, which is the part that keeps the
  numbers honest: a reader must be able to tell a measured number from an absent one. `docs/roadmap.md`
  is the right home because it already carries "Current focus" and the alpha gate, and the house rule
  says the milestones live there.
- **Refusals:** a number with no machine or no date in this section is refused; an absent measurement
  that is **not** listed in the caveat paragraph is refused.
- **Tests:** `every_number_in_the_performance_section_has_a_machine_and_a_date`. `the_caveat_list_
  mentions_every_absent_measurement`. `the_section_links_the_baseline_file`.
- **Acceptance:** the section exists and the checks pass. Effect on `cargo xtask ci` wall time:
  **+0.2 s**.
- **Risk:** performance numbers decay; a section of last quarter's numbers is a liability. Fallback:
  every number carries a date and the caveat paragraph names every **absent** measurement, so the
  reader can tell a stale number from a missing one.

---

# PLT-REL — versions, artefacts and the release pipeline

`docs/releasing.md` is accurate today and this epic keeps it accurate: one version source
(`xtask/src/version.rs`), one release trigger (`git push origin main:release`), twelve artefacts, twelve
optional signing secrets, and a packaging lint workflow that already exists
(`.github/workflows/packaging-lint.yml`).

### PLT-REL-001 — One version source, and the three places it appears
- **State:** READY
- **Depends on:** —
- **Files:** root `Cargo.toml` `[workspace.package] version`, `xtask/src/version.rs`,
  `docs/releasing.md` (owner **PLT**)
- **Contract:** `pub fn read(manifest: &str) -> Result<String, String>;` /
  `pub fn replace(manifest: &str, new: &str) -> Result<String, String>;` /
  `pub fn validate(v: &str) -> Result<(), String>;` — **unchanged signatures**
  (`version.rs:9-41`), and the manifest keeps `version.workspace = true` per crate.
- **Behaviour:** The version is read once and shown in three places (About, `cadcraft --version`,
  `cadcraft-cli --version`) plus the `CADCRAFT_BUILD_SHA` embedded in release builds. This task
  asserts the three readers read the same value — the drift case is a CLI reporting `0.7.0` while the
  app reports `0.7.1`.
- **Refusals:** an invalid version is refused by `validate` with the three-part rule (`version.rs:41`);
  a manifest with no `version` line in `[workspace.package]` is refused (`version.rs:73`); `+meta`
  (build metadata) is refused, as semver says it has no effect on precedence.
- **Tests:** the five existing tests (`version.rs:109-152`) plus `all_three_readers_report_the_same_
  version` and `the_version_matches_the_workspace_manifest`.
- **Acceptance:** one value, three readers, asserted. Effect on `cargo xtask ci` wall time: **0 s**
  (a text read and a compare).
- **Risk:** none; this is already the best-factored piece of `xtask`.

### PLT-REL-002 — `cargo xtask version set`: the write path, and why it is atomic
- **State:** READY
- **Depends on:** PLT-REL-001
- **Files:** `xtask/src/version.rs::run` (owner **PLT**)
- **Contract:** `cargo xtask version set X.Y.Z[-pre]` — writes `target/Cargo.toml.xtask-version`,
  then `fs::rename` onto the manifest, then `cargo update --workspace --offline`
  (`version.rs:90-104`). Unchanged.
- **Behaviour:** The rename is atomic, so a half-written root `Cargo.toml` — which would break every
  crate in the workspace — cannot happen. The `Cargo.lock` refresh is `update`, never `add`/`upgrade`,
  so a version bump never upgrades a dependency. The `--offline` first with a network retry is the
  right order: offline avoids the network, and the retry covers a cold cache.
- **Refusals:** an invalid version is refused **before** anything is written; a rename that fails
  leaves the original intact and reports.
- **Tests:** `the_manifest_is_written_atomically` — a dry run against a temp manifest; `an_invalid_
  version_writes_nothing`; `the_lock_is_refreshed_offline_first`.
- **Acceptance:** a bump changes exactly one line of the manifest and one version line of the lock.
  Effect on `cargo xtask ci` wall time: **0 s** (a version bump triggers a rebuild, which is the
  build, not this task).
- **Risk:** `cargo update --workspace` can rewrite the lock more than the version. Fallback: the
  offline-first attempt plus a diff review of `Cargo.lock` in the release PR; the lock's only expected
  change is the workspace crates' own version.

### PLT-REL-003 — The release workflow's trigger, and the dry-run mode
- **State:** READY
- **Depends on:** PLT-REL-001
- **Files:** `.github/workflows/release.yml`, `docs/releasing.md` (owner **PLT**)
- **Contract:** the trigger and the three modes, unchanged: a push of `main` to `release` drafts the
  release; `workflow_dispatch` with `version: X.Y.Z-rc.N` makes a test build; a dispatch on any
  other branch is a **dry run** — the unsigned jobs build and the `release` environment refuses the
  signing jobs (`docs/releasing.md:6`).
- **Behaviour:** The dry run is the safety property: a workflow run cannot publish by accident, and the
  signing secrets are only reachable from the `release` branch because the environment is deployable
  only from it. The task asserts that property in the workflow, because it is the single most
  important line in the file.
- **Refusals:** a signing job without an `environment:` guard is refused; a draft release created from
  a non-`release` branch is refused.
- **Tests:** `every_signing_job_is_guarded_by_the_release_environment`. `a_non_release_branch_cannot_
  draft`. `the_version_input_is_validated`.
- **Acceptance:** the three modes behave as documented. Effect on `cargo xtask ci` wall time: **0 s**
  (a separate workflow; the PR gate does not run it).
- **Risk:** the `release` environment's protection is a repository setting, not a file, so the test
  cannot verify it. Fallback: the workflow's comment names the setting and its required value, and
  `PLT-REL-004`'s report asks a maintainer to confirm it once per release.

### PLT-REL-004 — The release report: what was built, what was signed, what was skipped
- **State:** BACKLOG
- **Depends on:** PLT-REL-003
- **Files:** `.github/workflows/release.yml`, `docs/releasing.md` (owner **PLT**)
- **Contract:** the release job prints one line per artefact: name, platform, size, signed
  (yes/no/notarized), and the reason when it is not. The twelve signing secrets are **all optional**
  (`docs/releasing.md:8`): a missing one produces an unsigned artefact **with a warning**, never a
  failed release.
- **Behaviour:** A release with unsigned artefacts is a real situation (a contributor's fork, a
  runner without the secret), and the honest output is a table with a column per signature status. The
  task therefore needs no new machinery: it is a summary block in the existing jobs and a section in
  the draft release's notes.
- **Refusals:** a release that is *created* with every artefact unsigned is refused — an unsigned
  release is allowed only when the notes say so explicitly.
- **Tests:** `every_artifact_line_names_its_signature_status`. `a_missing_secret_is_a_warning_not_a_
  failure`. `the_notes_state_when_nothing_was_signed`.
- **Acceptance:** the report exists in each release's notes. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** users download an unsigned build without noticing. Fallback: the artefact name carries
  `-unsigned` when it is unsigned, so it is visible in the download list.

### PLT-REL-005 — macOS: universal binary, DMG, Developer ID signing, notarisation, stapling
- **State:** READY
- **Depends on:** PLT-REL-003
- **Files:** `packaging/macos/**`, `.github/workflows/release.yml` (owner **PLT**)
- **Contract:** `packaging/macos/package.sh` (local, ad-hoc signed DMG — `docs/releasing.md:11`); the
  release job builds a **universal** `.dmg` (x86_64 + aarch64), signs with Developer ID, notarises,
  staples, and produces the CLI `.zip` (`docs/releasing.md:7`).
- **Behaviour:** The DMG's Finder window background is `packaging/macos/dmg/background.svg`, rendered
  to `background.tiff` by `generate.py` — and that file already has its `ATTRIBUTION.md` row
  (`ATTRIBUTION.md:40-41`, author `@XusBadia`, MIT OR Apache-2.0), which is the model for every other
  asset. Nothing here is proprietary: our own icon, our own background.
- **Refusals:** a missing signing identity is a warning plus an `-unsigned` artefact (`PLT-REL-004`);
  a notarisation failure fails the job (an un-notarised signed build prompts users on first launch,
  which is worse than unsigned).
- **Tests:** the packaging-lint workflow covers the script's syntax (`.github/workflows/
  packaging-lint.yml:19-23` uses the org's shared lint). The release job's assertions are manual (they
  need Apple infrastructure) and the manual procedure is in `docs/releasing.md`.
- **Acceptance:** a `.dmg` per release, and the `ATTRIBUTION.md` rows for the background stay current
  (the tiff is regenerated, the row names the generator). Effect on `cargo xtask ci` wall time: **0 s**
  (a release-only job).
- **Risk:** a regenerated `background.tiff` changes bytes every run (a renderer version difference),
  which makes a diff noisy. Fallback: the row names the source `.svg` as the asset and the `.tiff` as
  generated — which is what `ATTRIBUTION.md:41` already says.

### PLT-REL-006 — Windows: MSI (x64, x86, arm64), portable zip, Azure Trusted Signing
- **State:** READY
- **Depends on:** PLT-REL-003
- **Files:** `packaging/windows/cadcraft.wxs` (WiX v5 + `WixToolset.UI.wixext`),
  `.github/workflows/release.yml` (owner **PLT**)
- **Contract:** three MSIs and one portable `.zip` per release (`docs/relearing.md:7`), signed with
  Azure Trusted Signing; the installer wizard has a **Shortcuts page** whose desktop shortcut is
  opt-in via `DESKTOPSHORTCUT=1`/`0` and whose default on an upgrade is the previous choice
  (`docs/releasing.md:9`).
- **Behaviour:** The `DESKTOPSHORTCUT` behaviour is the kind of detail that is a support ticket if it
  is wrong, so it is stated in the doc and asserted in the workflow's property installation (the
  MSBuild property is passed explicitly, never defaulted inside the WiX project where it cannot be
  seen).
- **Refusals:** a missing signing secret is a warning (`PLT-REL-004`); a missing `DESKTOPSHORTCUT`
  property is **refused** rather than defaulted silently — that is the upgrade-breaking case.
- **Tests:** the property is passed in the workflow's build step; the packaging lint covers the WiX
  project's syntax.
- **Acceptance:** four artefacts per release, and the desktop-shortcut behaviour is documented and
  passed. Effect on `cargo xtask ci` wall time: **0 s** (release-only).
- **Risk:** an upgrade that un-checks the desktop shortcut. Fallback: the WiX `UpgradeGuid` and the
  property's upgrade default, plus a manual check on one upgrade per release (in the report).

### PLT-REL-007 — Linux: AppImage (+ zsync), Flatpak, deb, rpm, tar.gz, riscv64
- **State:** READY
- **Depends on:** PLT-REL-003
- **Files:** `packaging/linux/**`, `.github/workflows/release.yml` (owner **PLT**)
- **Contract:** x86_64 and aarch64 `.AppImage` (plus `.AppImage.zsync` for delta updates),
  `.flatpak` (repackaging the tarball via `packaging/linux/flatpak-bundle.sh`), `.deb`, `.rpm`,
  `.tar.gz`, and a riscv64 `.tar.gz` cross-compiled on Ubuntu 24.04 against `ports.ubuntu.com` and
  CLI-smoke-tested under QEMU (`docs/releasing.md:7`).
- **Behaviour:** Six artefacts on two architectures from one build; the riscv64 one is the unusual case
  and its **QEMU smoke test** is what makes it worth shipping — a tarball that untars is not a binary
  that runs.
- **Refusals:** a riscv64 build that fails its QEMU smoke test is refused, not shipped untested; a
  Flatpak bundle that fails `flatpak-builder` is refused.
- **Tests:** the packaging lint covers `shellcheck` over `packaging/linux/*.sh` (the workflow's shared
  lint, `.github/workflows/packaging-lint.yml:2`); the QEMU smoke test runs in the release job.
- **Acceptance:** the six artefacts per architecture, with the riscv64 one tested. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** six packaging formats is six ways to be broken. Fallback: each is a script with a local
  equivalent (`packaging/linux/package.sh`, `docs/releasing.md:11`) so a maintainer can produce one
  locally.

### PLT-REL-008 — FreeBSD and the CLI zip
- **State:** READY
- **Depends on:** PLT-REL-003
- **Files:** `.github/workflows/release.yml` (owner **PLT**)
- **Contract:** a FreeBSD x86_64 `.tar.gz` (built in a FreeBSD VM — `docs/releasing.md:7`) and the
  CLI `.zip` for macOS and Windows.
- **Behaviour:** The FreeBSD build is a **VM job**, not a cross-compile, which is the honest way (the
  platform's ABI is the thing being tested). The CLI zip is the same binary in a zip, and it is built
  per platform alongside the app.
- **Refusals:** a FreeBSD job that fails to provision the VM fails the release with a named reason
  (rather than silently skipping the artefact).
- **Tests:** the workflow's job list names each artefact (checked by `PLT-DOC-013`).
- **Acceptance:** both artefacts per release. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the FreeBSD VM job is the most fragile in the workflow. Fallback: it is the last job, so
  its failure does not block the other artefacts — and the report says which are missing.

### PLT-REL-009 — The web artefact
- **State:** READY
- **Depends on:** PLT-WASM-006, PLT-REL-003
- **Files:** `packaging/web/package.sh`, `.github/workflows/release.yml` (owner **PLT**)
- **Contract:** `cadcraft-web-<version>.zip` holding a self-contained static site that works from any
  URL path and inside an `<iframe>` (`package.sh:1-9`), plus the sample `_headers`/`.htaccess` and
  `HOSTING.md` (`package.sh:33-35`).
- **Behaviour:** The script's **two guards** are the content of this task: it refuses root-absolute
  URLs in `index.html` (`package.sh:22-25`) and it requires `dist/web/index.html` to exist
  (`package.sh:20`). Both are already implemented; this task makes them release-visible by naming them
  in the release report.
- **Refusals:** a build with root-absolute URLs is refused by the script (a real, already-tested
  failure mode); a missing `trunk` is refused with the install command (`package.sh:15`).
- **Tests:** `the_web_package_fails_on_root_absolute_urls` (from `PLT-WASM-006`) is the substantive
  test here too; `the_zip_holds_a_self_contained_site`.
- **Acceptance:** the zip is a release artefact and passes the two guards. Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** the web artefact is the largest and the least used. Fallback: it is unsigned and last; its
  failure does not block the others.

### PLT-REL-010 — `SHA256SUMS.txt` and the icons pipeline
- **State:** READY
- **Depends on:** PLT-REL-005, PLT-REL-006
- **Files:** `.github/workflows/release.yml`, `packaging/icons.sh`, `xtask/src/ico.rs` (owner **PLT**)
- **Contract:** every artefact is listed in `SHA256SUMS.txt` (`docs/releasing.md:7`), and the icons are
  generated by `packaging/icons.sh` using `cargo xtask ico <out.ico> <in.png>…` (`main.rs:44`,
  `ico.rs:20-45`).
- **Behaviour:** The icons are **ours**: `assets/app-icon/cadcraft-1024.png` and its derivatives, all
  with `ATTRIBUTION.md` rows (`ATTRIBUTION.md:26-39`, "CADCraft contributors · generated · MIT OR
  Apache-2.0"). `xtask ico` refuses a non-square or over-256 px input (`ico.rs:31-33`) and stores PNGs
  as they are (Vista+ reads PNG-compressed entries, `ico.rs:2-3`). The sums file is produced by the
  standard tool (`sha256sum`/`shasum`), not by Rust, and is verified before the release is published.
- **Refusals:** a missing checksum is refused; a checksum that does not match the artefact is refused
  and the release does not publish; an icon input that is not a PNG is refused by `png_size`
  (`ico.rs:14-18`).
- **Tests:** `ico_rejects_a_non_square_input` (the existing `pack` behaviour),
  `every_artefact_has_a_checksum` — the report lists `N artefacts, N sums`.
- **Acceptance:** the sums file covers every artefact; regenerating the icons produces byte-identical
  output (the pipeline is deterministic — `pack` writes a fixed header). Effect on `cargo xtask ci`
  wall time: **0 s**.
- **Risk:** a checksum computed on the wrong artefact (a stale `dist/`) ships a sum that does not match
  what the user downloads. Fallback: the sums are produced **from the published artefacts** in the same
  job, and the verification step reads each file back before the release is published.
- **Asset note:** every icon file is in `assets/` and therefore in `ASSET_DIRS`
  (`assets.rs:14`), so **all of them need an `ATTRIBUTION.md` row**; regenerating an icon does not
  change its row, but **adding** one does, and `cargo xtask assets` fails without it.

### PLT-REL-011 — The packaging lint workflow, and what it does not check
- **State:** READY
- **Depends on:** PLT-REL-005
- **Files:** `.github/workflows/packaging-lint.yml` (owner **PLT**)
- **Contract:** `on: push [main, release]` and `pull_request`, paths-limited to
  `.github/workflows/**`, `packaging/**`, `assets/app-icon/**`; `permissions: contents: read`;
  `concurrency` with `cancel-in-progress` (lines 5-17); one reusable job
  (`storytold/craft-common-actions/.github/workflows/packaging-lint.yml@main`).
- **Behaviour:** Seconds, no Rust build: workflow syntax, the shell/PowerShell packaging scripts
  (`shellcheck` — `.shellcheckrc` is committed at `packaging/.shellcheckrc`), and the Linux
  desktop/AppStream/Flatpak metadata. The path filter is the cost control: a Rust-only change does not
  run it. The task's content is stating the **blind spots**: it does not build any artefact, so a
  packaging change can be lint-clean and broken.
- **Refusals:** removing the path filter (a packaging workflow that runs on every Rust change wastes
  minutes) is refused; removing `permissions: contents: read` (least privilege) is refused.
- **Tests:** `the_packaging_lint_has_a_path_filter` (YAML assert), `it_requests_read_only`, `it_cancels_
  superseded_runs`.
- **Acceptance:** the workflow stays as it is, with its scope documented. Effect on
  `cargo xtask ci` wall time: **0 s** for the Rust gate; the lint is a separate workflow, ~10 s.
- **Risk:** the reusable workflow is tracked at `@main`, not a SHA — deliberate (the file's own comment
  says so, line 22) and org-wide, so a change there is not reviewed in this repository. Fallback: the
  decision is recorded in the file with its reason, which is the correct handling for a shared
  org-level workflow.

### PLT-REL-012 — File types per platform: macOS documents, Windows "Open with", Linux MIME
- **State:** READY
- **Depends on:** PLT-REL-005, PLT-REL-006, PLT-REL-007
- **Files:** `packaging/{macos,windows,linux}/**`, `packaging/linux/*.desktop`, `*.appdata.xml`
  (owner **PLT**)
- **Contract:** `.dxf` and `.dwg` are registered on all three (`docs/releasing.md:10`), plus
  `.ccm`, `.stl`, `.step`, `.stp`, `.iges`, `.igs`, `.3mf`, `.gltf`, `.glb` as the formats land. Every
  MIME declaration names a MIME type **and** a shared-mime-info XML for Linux, so `xdg-mime` resolves.
- **Behaviour:** The registration is data (a `.plist`, a WiX fragment, a `.desktop` + XML), so it is
  lintable — and the packaging lint checks exactly that (`packaging-lint.yml:2`, "Linux desktop/
  AppStream/Flatpak metadata").
- **Refusals:** a format that CADCraft cannot yet **read** is not registered as an openable document
  type; registration is a promise the product keeps, and `Format3::is_readable`
  (`docs/plan/DAT.md` `DAT-002`) is the fact it comes from.
- **Tests:** `every_registered_extension_is_readable` — the mapping file's extensions are checked
  against the read/write table's readable arms. `the_mime_xml_exists_for_every_type`.
- **Acceptance:** the three platforms register exactly what CADCraft reads. Effect on
  `cargo xtask ci` wall time: **+0.2 s** (one data-file read).
- **Risk:** a registered type that fails to open is worse than an unregistered one. Fallback: the test
  above makes it a **build failure**, which is the right severity.

### PLT-REL-013 — The release checklist: what blocks a release, written down
- **State:** READY
- **Depends on:** PLT-REL-003
- **Files:** `docs/releasing.md` (owner **PLT**)
- **Contract:** a checklist with the blocking conditions: `cargo xtask ci` green on `main`, the
  version bumped and committed, the parity docs updated (`AGENTS.md`'s rule), no unreleased
  `ATTRIBUTION.md` gap, and the release notes written.
- **Behaviour:** The blocking conditions are the ones that have actually blocked a release, and each
  is checkable: CI green is observable; the version is `cargo xtask version`; the attribution gap is
  `cargo xtask assets`; the docs are the parity-doc rule. The **non-blocking** items are listed too
  (an unsigned artefact is a warning), because a checklist that blocks on everything blocks on
  nothing.
- **Refusals:** an item that is not checkable is refused as a blocking item (it becomes a reminder).
- **Tests:** `every_blocking_item_names_a_command` — each has a command that verifies it. `the_
  checklist_separates_blocking_from_warnings`.
- **Acceptance:** the checklist exists in `docs/releasing.md`. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** a checklist nobody follows. Fallback: every item is a command, so a release can be run
  from the checklist alone.

### PLT-REL-014 — The version in the About window, and the build SHA
- **State:** READY
- **Depends on:** PLT-REL-001
- **Files:** `crates/ui-egui/src/**` (About; owner **U**), `packaging/**` (owner **PLT**), `xtask/src/
  version.rs` (owner **PLT**)
- **Contract:** `env!("CARGO_PKG_VERSION")` for the version and `option_env!("CADCRAFT_BUILD_SHA")`
  for the commit, both shown in About; release builds embed the SHA, debug builds show "dev"
  (`docs/releasing.md:5`).
- **Behaviour:** The About window is also where the **contributors** come from (`AGENTS.md`:
  "compiled into the binary from `contributors/contributors.json` and `contributors/people.toml`"), so
  this task covers the version half and `PLT-CRED-005` the credits half. A build without the SHA
  environment variable shows `unknown`, never a stale SHA.
- **Refusals:** a build that embeds a SHA it cannot resolve is refused (`option_env!` returning `None`
  is reported as `unknown` in the UI, not as an empty string that looks like a bug).
- **Tests:** `about_reports_the_workspace_version`. `a_build_without_a_sha_reports_unknown`. `the_
  version_in_about_matches_xtask_version`.
- **Acceptance:** the About window's version equals `cargo xtask version`. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** the two versions diverge if someone bumps the manifest and forgets a rebuild. Fallback: the
  test above compares the compiled-in value with the manifest's.

---

# PLT-CRED — contributors, licences and attribution

`AGENTS.md` is unusually specific about this, and so is this epic: `contributors/contributors.json` is
**generated, never hand-edited**; `contributors/people.toml` holds names people chose for themselves;
**an agent must never add, edit, guess or copy anyone else's entry**; and `ATTRIBUTION.md` is enforced by
`cargo xtask assets`.

### PLT-CRED-001 — `contributors.json` is generated, and the generator is the only writer
- **State:** READY
- **Depends on:** —
- **Files:** `contributors/contributors.json`, `docs/contributors.md` (owner **PLT**)
- **Contract:** the file is produced by craftrules' script:
  `python3 ../../craftrules/scripts/contributors.py .` — and **no hand edit is ever accepted**
  (`AGENTS.md`: "generated, never hand-edit").
- **Behaviour:** The file's shape is a list of `{login, commits, first, last}` derived from `git log`.
  This task states the rule where a contributor will hit it (the file's own first line and
  `docs/contributors.md`) and adds a check that the file's shape is what the generator produces.
- **Refusals:** a hand edit is detected by the next regeneration and reverted; the check below asserts
  only the **shape** (a JSON array of objects with those four keys), which is what a consumer needs.
- **Tests:** `contributors_json_is_an_array_of_login_commits_first_last`. `the_file_is_valid_json`. `a_
  hand_edited_shape_fails`.
- **Acceptance:** the shape check passes. Effect on `cargo xtask ci` wall time: **+0.1 s**.
- **Risk:** a regeneration in a PR with a huge diff. Fallback: maintainers regenerate on release
  (`AGENTS.md`), not per PR.

### PLT-CRED-002 — `people.toml`: only a person's own entry, and the three questions
- **State:** READY
- **Depends on:** PLT-CRED-001
- **Files:** `contributors/people.toml` (owner **PLT**, but the **entries** are the people
  themselves)
- **Contract:** the shape is `[people.<github_username>]` with optional `real_name`,
  `display_name` and `sync_github_name = true` (`AGENTS.md`). The agent rule, verbatim in effect: check
  whether the human's username has an entry; if not, **ask once**; add only their own entry; change
  nothing if they decline.
- **Behaviour:** The file is the only place a person's chosen name lives, and the rule exists because
  guessing a name is a real harm. This task documents the three questions (real name? display name?
  public GitHub profile name?) and the "add only their own entry" limit, and asserts the file's shape.
- **Refusals:** an entry for a person who did not choose it is refused at review. A `real_name` is
  never inferred from a commit author or a profile.
- **Tests:** `people_toml_is_a_table_of_people` — shape only. `every_entry_has_a_username`. `no_entry_
  contains_an_inferred_name` — a review rule, stated.
- **Acceptance:** the shape check passes; the rules are in `AGENTS.md` (already) and in
  `docs/contributors.md`. Effect on `cargo xtask ci` wall time: **+0.1 s**.
- **Risk:** a person's name is in the repo and cannot be removed from history easily. Fallback: an
  entry is removed by PR at the person's request, and the rule is stated where an agent reads it
  (`docs/agents/SYSTEM_PROMPT.md`, `PLT-DOC-009`).

### PLT-CRED-003 — An agent's one question, and the one-line script that answers it
- **State:** READY
- **Depends on:** PLT-CRED-002
- **Files:** `docs/agents/SYSTEM_PROMPT.md`, `docs/contributors.md` (owner **PLT**)
- **Contract:** when an agent prepares a PR for a human contributor, it checks
  `contributors/people.toml` for a `[people.<username>]` entry; if absent it **asks once** and then
  either adds only that entry (via
  `python3 ../../craftrules/scripts/contributors.py --add-me . --real-name "…" --sync-github-name`)
  or changes nothing. The agent is already credited as `@username` either way.
- **Behaviour:** The task is to make this one question cheap and unmissable — it is a single grep, and
  the prompt says so in the section a preparing agent reads (`AGENTS.md`'s "Agents working for a
  contributor").
- **Refusals:** asking more than once is refused; adding anyone else's entry is refused; adding an
  entry without being asked is refused.
- **Tests:** `the_prompt_states_the_one_question` (a grep over the prompt text). `the_add_me_script_
  path_is_correct` — the documented path exists in craftrules' repo layout, asserted as a string.
- **Acceptance:** the rule is in the prompt an agent reads. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** an agent adds an entry unasked. Fallback: the "no entry contains an inferred name" review
  rule (`PLT-CRED-002`), and a diff in `people.toml` is small enough to review by eye.

### PLT-CRED-004 — The contributor stats refresh, and its cost
- **State:** READY
- **Depends on:** PLT-CRED-001
- **Files:** `docs/contributors.md` (owner **PLT**)
- **Contract:** `python3 ../../craftrules/scripts/contributors.py .` refreshes `contributors.json` and
  **re-verifies who wrote each `people.toml` entry** (`AGENTS.md`: the refresh "also re-verifies who
  wrote each `people.toml` entry; `--check` only verifies").
- **Behaviour:** The task records when to run it (before a release, not per PR), what it costs (it
  walks the whole `git log`, which on this repository is seconds), and what `--check` is for (a
  verification-only run for a maintainer who does not want to regenerate).
- **Refusals:** a refresh that would rewrite a `people.toml` entry is refused without the person's
  consent — the verification reports, it does not overwrite.
- **Tests:** `the_refresh_command_is_documented`. `the_check_command_is_distinguished`. `the_refresh_
  is_not_part_of_ci` — a grep, so nobody wires a 30-second git walk into every PR.
- **Acceptance:** the two commands and the cost are in `docs/contributors.md`. Effect on
  `cargo xtask ci` wall time: **0 s** (it is not in the gate).
- **Risk:** a release commit with a large `contributors.json` diff. Fallback: it is one file in the
  release PR, which is where a regenerated file belongs.

### PLT-CRED-005 — The About window's Contributors panel reads the two files and nothing else
- **State:** READY
- **Depends on:** PLT-CRED-001, PLT-CRED-002
- **Files:** `crates/ui-egui/src/**` (owner **U**), `contributors/*` (owner **PLT**)
- **Contract:** the panel shows `people.toml`'s display names where an entry exists and the GitHub
  handle otherwise, with the commit counts from `contributors.json` — and **no other source**. The data
  is compiled into the binary (an `include_str!`), so About works offline.
- **Behaviour:** Two sources, one rule: a name comes from `people.toml` **only**; a handle and a count
  come from `contributors.json` only. A person with no `people.toml` entry appears as `@handle`
  (`AGENTS.md`), which is the correct default and is visible in the panel.
- **Refusals:** a name that is not in `people.toml` is never displayed, whatever the source; a missing
  `contributors.json` is an empty list with a note, never a panic (the panel is UI code and must obey
  the same no-panic rule).
- **Tests:** `the_panel_shows_the_display_name_when_present`. `an_absent_entry_shows_the_handle`. `no_
  other_name_source_is_read` — a grep over the panel's module for `include_str!`/reads. `a_missing_
  json_is_an_empty_list_not_a_panic`.
- **Acceptance:** the panel's data source is exactly two files. Effect on `cargo xtask ci` wall time:
  **0 s**.
- **Risk:** a stale compiled-in list after a contributor adds an entry. Fallback: a rebuild is needed
  and a release does it; the panel's count line says the build date.

### PLT-CRED-006 — Every `.tsv` translation has an `ATTRIBUTION.md` row naming its translator
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `ATTRIBUTION.md` (the "Interface translations" table, `ATTRIBUTION.md:55-70`),
  `crates/ui-egui/src/i18n/*.tsv` (owner **PLT** for the rows, **U** for the catalogs)
- **Contract:**
  ```rust
  pub fn every_translation_has_a_row() -> Result<Vec<String>, String>;   // 13 catalogs today
  pub fn every_translation_row_names_a_translator() -> Result<Vec<String>, String>;
  ```
- **Behaviour:** Translating a CAD interface is real work and every catalog names its translator today
  (`uk` @dmatviichuk, `de` the contributors, `es` @PedroGuille, `pt_br` @WladmirJunior, `cs`
  @Losminek, `ru`/`fr` @pechnikov2026-star, `zh` @mayloglog, `sv` @holmbom, `it`, `ko` @huk082, `ar`
  @hasan-sdaif). The rule is absolute and checked: **no proprietary localization resources are copied**
  (every row says so). A new `.tsv` without a row fails `cargo xtask assets`? No — `.tsv` is not in
  `ASSET_EXT` (`assets.rs:10-13`), so this check is the **only** thing that catches it. That is why it
  exists.
- **Refusals:** a catalog with no row is refused naming it; a row with an empty author is refused; a
  row claiming a third-party source is refused (no proprietary localization data).
- **Tests:** `every_translation_has_a_row` — the acceptance, run over the 13 catalogs. `every_
  translation_row_names_a_translator`. `no_row_claims_a_proprietary_source`. `the_row_count_matches_
  the_catalog_count`.
- **Acceptance:** every catalog has a row, and `ATTRIBUTION.md`'s table has no row for a file that does
  not exist. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** `.tsv` is not an asset extension, so a **new** catalog is invisible to `xtask assets`.
  Fallback: this task could also add `tsv` to `ASSET_EXT` — but then every TSV needs a row, which is
  exactly what this task requires, so the two agree. The decision belongs to `PLT-ASSET-008`.

### PLT-CRED-007 — The brand exception: one directory, one licence, named in `ATTRIBUTION.md`
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `docs/brand/**`, `docs/brand/LICENSE-brand.txt`, `ATTRIBUTION.md:46-53` (owner **PLT**)
- **Contract:** the **only** non-open asset class in the repository is the ArtCraft brand in
  `docs/brand/` — ArtCraft trademarks under `docs/brand/LICENSE-brand.txt` (`ATTRIBUTION.md:14`,
  `AGENTS.md`). Eight rows name it.
- **Behaviour:** The exception is stated, scoped to one directory, and its licence is named in every
  row. Everything else in `ATTRIBUTION.md` is original, CC0, OSI, or a redistributable Creative Commons
  asset.
- **Refusals:** a brand asset **outside** `docs/brand/` is refused; a brand row without the brand
  licence is refused; a brand file used as an app asset (an icon, a splash) is refused — it is a
  trademark, not the product's artwork.
- **Tests:** `the_brand_rows_are_the_only_non_open_licences`. `every_brand_asset_is_in_docs_brand`.
  `the_brand_licence_file_exists`.
- **Acceptance:** the exception is exactly one directory with a named licence. Effect on
  `cargo xtask ci` wall time: **+0.1 s**.
- **Risk:** the exception is a precedent that grows. Fallback: `PLT-CRED-007`'s first test makes any
  new non-open licence a failure, so growth requires deleting a test.

### PLT-CRED-008 — `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`: one licence answer for the whole repo
- **State:** READY
- **Depends on:** —
- **Files:** `LICENSE-APACHE`, `LICENSE-MIT`, `NOTICE`, root `Cargo.toml` `license` (owner **PLT**)
- **Contract:** `license = "MIT OR Apache-2.0"` (`Cargo.toml:8`) — the dual licence every crate
  inherits, and the one every `ATTRIBUTION.md` row cites (`MIT OR Apache-2.0`).
- **Behaviour:** The three files are unmodified upstream texts; `NOTICE` names what a distribution must
  carry. The task asserts the three agree with the manifest and with the attribution table's licence
  strings, so a row cannot say `MIT` while the project is `MIT OR Apache-2.0` without failing.
- **Refusals:** a licence string in `ATTRIBUTION.md` that is not one of the project's two (or the
  brand licence of `PLT-CRED-007`) is refused.
- **Tests:** `the_manifest_licence_matches_the_files`. `every_attribution_licence_is_one_of_the_two`.
  `notice_names_the_licences`.
- **Acceptance:** the three agree. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** an asset under a third licence (CC-BY-SA, say) enters the table. Fallback: the test fails
  and the decision is made explicitly — a third licence is allowed if the test is updated with the
  reason, which is the correct place for the decision.

### PLT-CRED-009 — The asset policy's negative half, as a review checklist
- **State:** READY
- **Depends on:** PLT-CRED-008
- **Files:** `AGENTS.md`'s "Assets — absolutely essential" section (owner **PLT**), `docs/contributors.md`
- **Contract:** `cargo xtask assets` checks **attribution**, not **origin**. It cannot tell whether a PNG
  was drawn by a contributor or copied from Autodesk's installation. So the negative half — *no
  Autodesk, Adobe or Avid iconography, images, fonts, patterns or artwork, ever* — is enforced by
  review, and this task writes the checklist that review uses: the source of every new asset, how it
  was made, that it is not derived from the reference application, and that no screenshot of the
  reference application is committed.
- **Behaviour:** The checklist is five questions with the file/line to cite for each, and it is the
  same list `docs/agents/SYSTEM_PROMPT.md` gives an agent that might add an asset. It also names what
  the rule covers beyond files: **font names** (an SHX name in a linetype table), **pattern values**
  (an AutoCAD `.pat`'s dash pattern copied), and **screenshot files** under `docs/images/`.
- **Refusals:** an asset whose origin cannot be stated is refused, whatever its licence.
- **Tests:** `the_checklist_mentions_font_names_pattern_values_and_screenshots`. `the_checklist_is_
  linked_from_the_agents_prompt`.
- **Acceptance:** the checklist exists where reviewers and agents read it. Effect on
  `cargo xtask ci` wall time: **0 s**.
- **Risk:** the most serious rule in `AGENTS.md` is the one a machine cannot check. Fallback: the
  checklist plus the review gate; and `PLT-ASSET-001`'s scan covers file-level attribution, so a
  copied asset with no row fails CI even if review missed it.

### PLT-CRED-010 — Third-party dependencies: a licence list, generated, with no new tooling
- **State:** READY
- **Depends on:** PLT-CRED-008
- **Files:** `docs/dependencies.md` (new; owner **PLT**), `xtask/src/deps.rs` (new; owner **PLT**)
- **Contract:**
  ```rust
  pub fn dependency_licences(metadata: &serde_json::Value) -> Result<Vec<(String, String)>, String>;
  // (crate name, licence) for every dependency in `cargo metadata`, workspace members included
  ```
- **Behaviour:** A generated table of every dependency and its licence, written to a doc. `cargo
  metadata` already carries each package's `license` field, so the generator is pure std +
  `serde_json` — **no** `cargo-about`, no `cargo-deny`, no new dependency. A dependency with **no**
  licence field is listed as `UNKNOWN`, which is a finding, not a pass.
- **Refusals:** a dependency whose licence is a copyleft (GPL/AGPL) is **refused by the check**, naming
  the crate — `AGENTS.md` forbids linking GPL/LGPL/AGPL code. A `UNKNOWN` is refused at review, not
  automatically.
- **Tests:** `every_dependency_has_a_licence` — the acceptance. `a_copyleft_dependency_is_refused`.
  `an_unknown_licence_is_listed_as_unknown`. `the_doc_is_regenerable` — running the generator twice
  gives the same file.
- **Acceptance:** `docs/dependencies.md` exists, is generated, and has no `UNKNOWN` and no copyleft.
  Effect on `cargo xtask ci` wall time: **+0.5 s** (one `cargo metadata` — shared with
  `PLT-CI-011`, so it can reuse the same invocation).
- **Risk:** a licence field can be wrong upstream. Fallback: the table is a pointer, and
  `NOTICE`/`ATTRIBUTION` are the authoritative statements.

### PLT-CRED-011 — The release notes' credits section
- **State:** BACKLOG
- **Depends on:** PLT-CRED-004
- **Files:** `.github/workflows/release.yml` (the draft release's notes; owner **PLT**),
  `docs/contributors.md`
- **Contract:** each release's notes carry a **Credits** section generated from `contributors.json`
  (the contributors of the release range) and the `people.toml` display names.
- **Behaviour:** Generated, not written by hand, because hand-written credits go stale within two
  releases. The range is the previous tag to the new one.
- **Refusals:** a credits section that names anyone not in the two files is refused; the section is
  generated or it is absent.
- **Tests:** `the_credits_section_is_generated_from_the_two_files`. `no_credit_outside_the_files`.
- **Acceptance:** the release draft carries the section. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** a name in `people.toml` that a person has since removed. Fallback: the section is
  generated at release time, so a removal takes effect at the next release — which is the correct
  latency for a credit.

### PLT-CRED-012 — Close-out: one credits policy page
- **State:** READY
- **Depends on:** PLT-CRED-003, PLT-CRED-006, PLT-CRED-009
- **Files:** `docs/contributors.md` (owner **PLT**)
- **Contract:** one page: what `contributors.json` is (generated), what `people.toml` is (chosen by
  the person), the agent's one question, the refresh command, the attribution rules, the brand
  exception, and the dependency licence table's link. Everything in `AGENTS.md` about credits, in one
  readable place.
- **Behaviour:** The consolidation, plus the links (`AGENTS.md` stays the authoritative rule; this is
  the page a contributor reads).
- **Refusals:** the page duplicating `AGENTS.md`'s **rules** verbatim is refused — it links them.
- **Tests:** `the_page_covers_all_seven_topics`. `every_link_resolves` (`PLT-DOC-001`).
- **Acceptance:** the page exists. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** consolidating seven topics into one page invites the page becoming the place where rules
  live, which would put policy in a file nobody reviews. Fallback: the page **links** `AGENTS.md` for
  every rule and owns only the explanation; the rules stay in the prompt and in `ATTRIBUTION.md`.

---

# PLT-ASSET — every asset is ours, drawn in code, and attributed

`cargo xtask assets` is 41 lines and does one thing: it lists the git-tracked files, keeps those whose
extension is in `ASSET_EXT` (`assets.rs:10-13`) or that live under `assets/`, `docs/images/` or
`examples/` (`assets.rs:14`), and fails when any of them is not written as `` `path` `` in
`ATTRIBUTION.md` (`assets.rs:22-24`). Everything else about assets is policy, and this epic turns the
policy into checks.

### PLT-ASSET-001 — The asset scan, restated: what counts as an asset here
- **State:** READY
- **Depends on:** —
- **Files:** `xtask/src/assets.rs::{ASSET_EXT, ASSET_DIRS, is_asset, missing}` (owner **PLT**)
- **Contract:** **unchanged** — `ASSET_EXT` (31 extensions: images, icons, fonts, documents, CAD
  formats, media), `ASSET_DIRS = ["assets/", "docs/images/", "examples/"]`, `is_asset(path) -> bool`,
  `missing(files, assets_md) -> Vec<String>` (`assets.rs:10-24`).
- **Behaviour:** The rule is an extension list plus three directories. Two consequences worth stating
  because they surprise people: **`.pdf` and `.svg` and `.png` are in the list**, so a committed
  screenshot, a committed exported sheet and a committed diagram all need a row; and a file under
  `examples/` needs a row **whatever its extension**, so a `.json` example is attributed too.
- **Refusals:** an untracked-but-present file is still scanned (`git ls-files --others
  --exclude-standard`, `assets.rs:29`), which is right: an asset staged for the first time must be
  attributed before it is committed.
- **Tests:** the existing `detects_unattributed_assets` (`assets.rs:47`) plus `a_pdf_is_an_asset` and
  `an_svg_is_an_asset` — **asserting the current surprising behaviour**, so a future change to
  `ASSET_EXT` is deliberate. `a_file_under_examples_is_an_asset_whatever_its_extension`.
- **Acceptance:** the scan's behaviour is pinned by tests, including the surprises. Effect on
  `cargo xtask ci` wall time: **0 s** (covered by `PLT-CI-010`).
- **Risk:** a new file type (a `.step` fixture, a `.ccm` fixture) is not in the list and passes
  unexamined. Fallback: `PLT-ASSET-008` decides that rule.

### PLT-ASSET-002 — The policy, in `ATTRIBUTION.md` and `AGENTS.md`, saying the same thing
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `ATTRIBUTION.md:6-14`, `AGENTS.md`'s "Assets" section (owner **PLT**)
- **Contract:** the policy is **no Autodesk, Adobe or Avid iconography, images, fonts, hatch patterns,
  linetypes, templates, presets or artwork, ever**; every asset is original, public domain/CC0,
  OSI-licensed, or redistributable Creative Commons, and **must have a row**; the only exception is the
  ArtCraft brand under `docs/brand/`.
- **Behaviour:** Two files state it (`ATTRIBUTION.md:7-14` and `AGENTS.md`). The task is to keep them
  consistent and to state the **enforcement**: the row is enforced by `cargo xtask assets`, the origin
  is enforced by review (`PLT-CRED-009`).
- **Refusals:** a divergence between the two statements is refused; one is authoritative and the other
  points at it.
- **Tests:** `the_policy_is_stated_in_both_files`. `both_statements_name_the_brand_exception`.
- **Acceptance:** both files agree. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the policy is long and drifts. Fallback: `ATTRIBUTION.md` holds it and `AGENTS.md` quotes
  the first paragraph; the test compares the first paragraph.

### PLT-ASSET-003 — Generated-in-code assets: the list, and why they need no row
- **State:** READY
- **Depends on:** PLT-ASSET-002
- **Files:** `ATTRIBUTION.md:16-22` (owner **PLT**); the code is `crates/ui-egui/src/icons.rs`,
  `crates/fonts/src/stroke.rs`, `crates/doc/src/library.rs`, `crates/color/src/lib.rs`,
  `crates/engine/src/sample.rs` (owner each domain)
- **Contract:** the five generated assets, listed by file path:
  1. the UI icon set (`crates/ui-egui/src/icons.rs`);
  2. the "CADCraft Stroke" drafting font (`crates/fonts/src/stroke.rs`);
  3. the linetype and hatch-pattern libraries (`crates/doc/src/library.rs`) — *industry-common names,
  our own dash/spacing values*;
  4. the colour index palette, generated from its structure (`crates/color/src/lib.rs`);
  5. the sample drawings (`crates/engine/src/sample.rs`).
- **Behaviour:** These have no **file**, so `xtask assets` finds nothing to attribute, and the list
  exists to say so — with the reason each one is original. The hatch library is the subtle one: names
  like `ANSI31` are industry-common (a name cannot be copyrighted) while the dash pattern is ours, and
  that distinction is worth a sentence because a reviewer will ask.
- **Refusals:** a **new** generated asset must be added to this list in the same change (that is the
  rule); a generated asset that writes a file (the icons are `.svg`? no — they are geometry) needs a
  row for the file.
- **Tests:** `every_generated_asset_path_exists`. `the_list_names_five_assets`. `no_generated_asset_
  has_a_row` (`PLT-DOC-015`'s test) — the two must not overlap.
- **Acceptance:** the five paths exist and none has a table row. Effect on `cargo xtask ci` wall time:
  **+0.1 s**.
- **Risk:** a sixth generated asset is added without a list entry. Fallback: the list check counts the
  entries, so a sixth changes the count and the test fails until someone says what it is.

### PLT-ASSET-004 — Fonts live in `storytold/craft-fonts`, and the code works without them
- **State:** READY
- **Depends on:** PLT-ASSET-003
- **Files:** `crates/fonts/build.rs`, `crates/fonts/Cargo.toml`, `ATTRIBUTION.md:16-19` (owner **PLT**
  for the policy; the fonts crate is a domain's)
- **Contract:** the shared fonts are an **optional build input** at `CRAFT_FONTS_DIR`, embedded by
  `crates/fonts/build.rs`, **never a `Cargo.toml` dependency** (`AGENTS.md`); code and tests work
  without them. Font files are **not** in `ATTRIBUTION.md` — they live in the other repository.
- **Behaviour:** The build script embeds the fonts when the directory is present and falls back to the
  built-in stroke font when it is not. That is why the stroke font exists: it is the *fallback*, and
  the fallback is what CI exercises, every day, without the fonts.
- **Refusals:** a build that **fails** when `CRAFT_FONTS_DIR` is absent is refused (CI has no fonts);
  a font file committed into this repository is refused by `AGENTS.md`.
- **Tests:** `the_crate_builds_without_craft_fonts_dir`. `the_fallback_font_is_used`. `no_font_file_is_
  committed` — a grep for `.ttf`/`.otf` in the tree, which is also an `ASSET_EXT` check.
- **Acceptance:** a clean checkout with no fonts builds and the tests pass. Effect on
  `cargo xtask ci` wall time: **0 s** (this is the state today; the task keeps it).
- **Risk:** a contributor's machine has fonts and CI does not, so a layout difference appears only in
  CI. Fallback: the fallback font is the one CI measures against, so a test that depends on the
  optional fonts must be `#[cfg(feature = …)]`-gated or use the fallback explicitly.

### PLT-ASSET-005 — Icons are drawn in code, and the count is checkable
- **State:** READY
- **Depends on:** PLT-ASSET-003
- **Files:** `crates/ui-egui/src/icons.rs` (owner **U**), `ATTRIBUTION.md:17` (owner **PLT**)
- **Contract:** every icon in the interface is drawn by code in `icons.rs`; there is no icon **file**
  in the repository and no icon font.
- **Behaviour:** The check is the inverse of the others: `cargo xtask assets` finds nothing, and that is
  correct. This task adds a check that the icon set is **complete against the command registry** —
  every registered command with a menu has an icon, or the menu shows text only, which is a deliberate
  fallback. The count is asserted so a removed icon is visible.
- **Refusals:** adding an icon **file** to the UI crate is refused; a command whose icon is drawn from
  a font is refused.
- **Tests:** `the_icon_set_has_no_files` — a glob over `crates/ui-egui/**` for image extensions.
  `every_command_with_a_menu_has_an_icon_or_text_fallback`. `the_icon_count_is_stable`.
- **Acceptance:** no image files under `crates/ui-egui/`. Effect on `cargo xtask ci` wall time:
  **+0.2 s**.
- **Risk:** a designer adds a PNG for one icon. Fallback: `cargo xtask assets` fails on it (a `.png` in
  the tree under a path not in the three directories… wait — a `.png` anywhere is in `ASSET_EXT`, so it
  fails and needs a row). Fallback: the row is possible, so the check is "no image files without a
  row", which the existing scan already enforces.

### PLT-ASSET-006 — The hatch and linetype libraries: names are common, values are ours
- **State:** READY
- **Depends on:** PLT-ASSET-003
- **Files:** `crates/doc/src/library.rs` (owner **C2**), `ATTRIBUTION.md:19-20` (owner **PLT**)
- **Contract:** the library defines 23 hatch patterns and the standard linetypes with **our own**
  dash/spacing values (`ATTRIBUTION.md:19-20`; `docs/gaps.md` F13 says AutoCAD has ~85 and ours must
  stay original).
- **Behaviour:** The legal and practical line, stated once: a **name** (`ANSI31`, `DASHED`) is
  industry-common and not protectable; the **geometry** (the dash pattern, the angle, the spacing) is
  ours. Copying a `.pat` file's numbers would be copying Autodesk's expression of a standard pattern,
  which the project has never done.
- **Refusals:** a pattern whose values are copied from a `.pat`/`.lin` file is refused; a pattern file
  committed to the repository is refused (`AGENTS.md`).
- **Tests:** `no_pattern_file_is_committed` — a grep for `.pat`/`.lin`. `every_pattern_is_defined_in_
  code`. `the_pattern_count_is_stable`.
- **Acceptance:** no pattern files; the library is code. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** a user expects `ANSI31`'s exact spacing. Fallback: the values are documented in the library
  and the deviation is stated, so it is a known difference rather than a surprise.

### PLT-ASSET-007 — Documentation images: every one is a CADCraft screenshot or our own diagram
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `docs/images/` (owner **PLT**), `ATTRIBUTION.md:42-45` (owner **PLT**)
- **Contract:** the directory holds three PNGs today (`ui-apartment.png`, `ui-layout.png`,
  `ui-bracket.png`) plus the brand PNGs in `docs/brand/`; each has a row saying it is a screenshot of
  **CADCraft itself** with **no Autodesk UI** (`ATTRIBUTION.md:43-45`).
- **Behaviour:** The rule that matters: a screenshot of the reference application is **never**
  committed (`AGENTS.md`), and `docs/images/` is exactly where such a file would land — so the review
  checklist (`PLT-CRED-009`) covers this directory explicitly, and the rows are evidence.
- **Refusals:** an image under `docs/images/` with no row is refused by the scan; a row claiming a
  screenshot "of AutoCAD" is refused by review, not by the scan — and the scan's failure is the backstop
  for a row that exists but is wrong.
- **Tests:** `every_docs_image_has_a_row` (shared with `PLT-CRED-006`'s shape check). `every_row_says_
  cadcraft`. `no_image_outside_docs_images_and_docs_brand` — a glob over the tree.
- **Acceptance:** every image in the repository is attributed and none is a reference-application
  screenshot. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** a diagram made in another tool (Figma, Inkscape) exported as SVG needs a row naming the
  tool. Fallback: the row's Source column is exactly for that, and an original diagram is
  distributable; the check only requires the row.

### PLT-ASSET-008 — The rule for **generated** assets: fixtures, exports and reports
- **State:** READY
- **Depends on:** PLT-ASSET-001
- **Files:** `xtask/src/assets.rs::{is_asset, ASSET_EXT}`, `ATTRIBUTION.md` (owner **PLT**); the
  cross-references are `docs/plan/DAT.md` `DAT-014`'s Risk field, `DRW-EXP-016` and `DRW-EXP-006`
- **Contract:**
  ```rust
  pub enum Generated {
      /// Produced by our own code, deterministic, committed as a test fixture.
      Fixture { generator: String, command: String },
      /// Produced by our own code, committed for a human to look at.
      Example { generator: String, command: String },
      /// Produced by an external tool, and needs a licence.
      ThirdParty { tool: String, licence: String },
  }
  /// Whether a path needs an ATTRIBUTION.md row, given what it is.
  pub fn needs_row(path: &str, kind: Generated) -> bool;
  /// One row per generated file, in the table the scan reads.
  pub fn generated_row(path: &str, kind: &Generated) -> String;
  ```
- **Behaviour:** **The decision this task makes**, because two committed indexes already defer to it:
  - A **text fixture generated by our own code** (`.ccm`, `.step`, `.svg`, `.json`) is **not** an asset
    and needs **no row**: it is our own output, deterministic, machine-checkable, and the same class as
    a source file. `DAT-014`'s Risk field already reasoned this way ("`ASSET_EXT` has no `step` entry,
    so committing fixtures raises no attribution question today"). This task makes it a rule.
  - A **raster or document fixture** (`.png`, `.pdf`, `.svg`) **does** need a row, even when our own
    code produced it, because `ASSET_EXT` lists it and because a human will look at it. The row is the
    generated form: `| path | CADCraft contributors | generated (command) | MIT OR Apache-2.0 | … |`.
    This is what `DRW-EXP-006`/`016` must record when they commit a sheet export.
  - A file from an **external tool** always needs a row naming the tool and its licence
    (`packaging/macos/dmg/background.svg` is the worked example, `ATTRIBUTION.md:40`).
  - The rule **does not change `ASSET_EXT`**: adding `step`/`ccm` to the extension list would make
    every fixture an asset and every asset a row, which is bookkeeping with no legal content. The
    decision is recorded in `ASSET_EXT`'s doc comment.
- **Refusals:** a raster/document fixture committed without a generated row is refused by the scan
  (unchanged behaviour — this task does not weaken it); a `ThirdParty` row without a licence is
  refused.
- **Tests:** `a_text_fixture_needs_no_row`. `a_png_fixture_needs_a_generated_row`. `a_third_party_
  asset_names_its_tool_and_licence`. `the_generated_row_carries_the_command`. `asset_ext_is_
  unchanged` — pinned, so the decision is explicit rather than a side effect.
- **Acceptance:** `DAT-014`, `DRW-EXP-006` and `DRW-EXP-016` all resolve their asset question against
  this task's rule, and `ATTRIBUTION.md` gains a short paragraph stating it. Effect on
  `cargo xtask ci` wall time: **+0.3 s**.
- **Risk:** "our own output" becomes a loophole for a file copied from elsewhere and re-exported.
  Fallback: the generated row carries the **command**, so a reviewer can re-run it and compare bytes —
  and `PLT-CRED-009`'s checklist asks how it was made, which is the question a re-export does not
  answer.

### PLT-ASSET-009 — The icon pipeline: from one PNG to nine platform files
- **State:** READY
- **Depends on:** PLT-ASSET-005, PLT-REL-010
- **Files:** `packaging/icons.sh`, `xtask/src/ico.rs`, `assets/app-icon/**`, `ATTRIBUTION.md:26-39`
  (owner **PLT**)
- **Contract:** `packaging/icons.sh` generates the hicolor set, the `.icns`, the `.ico` (through
  `cargo xtask ico`), the `-64`/`-256` PNGs and the macOS margin variant from
  `assets/app-icon/cadcraft-1024.png`; the nine rows in `ATTRIBUTION.md:26-39` each say
  "generated from `assets/app-icon/cadcraft-1024.png`".
- **Behaviour:** One source, nine derived files, nine rows that name the source — which is the model
  for every generated asset in the repository and the pattern `PLT-ASSET-008`'s generated row
  generalises. `xtask ico` is deterministic (`ico.rs:20-45` writes a fixed header), so re-running the
  pipeline produces identical bytes.
- **Refusals:** an icon derived from a source that is not in `assets/app-icon/` is refused; a derived
  icon with no row is refused by the scan.
- **Tests:** `the_icon_pipeline_is_deterministic` — run it and compare bytes. `every_icon_row_names_
  its_source`. `every_icon_file_exists`.
- **Acceptance:** regenerating the icons changes nothing in the diff. Effect on `cargo xtask ci` wall
  time: **0 s** (a release-time task; `packaging-lint.yml` covers the script's syntax).
- **Risk:** a contributor edits `cadcraft-1024.png` and only regenerates some of the nine. Fallback: the
  row names the source, so a reviewer sees which source changed, and the pipeline is one script.

### PLT-ASSET-010 — The macOS DMG background: an SVG we own, rendered to a TIFF
- **State:** READY
- **Depends on:** PLT-REL-005, PLT-ASSET-008
- **Files:** `packaging/macos/dmg/{background.svg,generate.py,README.md,background.tiff}`,
  `ATTRIBUTION.md:40-41` (owner **PLT**)
- **Contract:** `background.svg` is original work (author `@XusBadia`, "original work for this
  repository (2026-10-08)"); it references the app icon rather than copying it, and its text is
  outlined glyph paths from the OFL fonts the project already uses (`ATTRIBUTION.md:40`).
  `generate.py` renders it to `background.tiff` (1x + 2x HiDPI).
- **Behaviour:** This is the **worked example** of `PLT-ASSET-008`'s third-party case: a tool-produced
  file (`background.tiff`) with a row naming the tool (`generate.py`), and the row notes the embedded
  sRGB profile's licence ("No copyright, use freely", `ATTRIBUTION.md:41`).
- **Refusals:** a rendered background from a source that is not in this repository is refused; a
  background containing Autodesk iconography is refused (it is packaging, not documentation, and the
  asset rule is absolute).
- **Tests:** `the_background_rows_name_author_and_generator`. `the_svg_is_committed_and_the_tiff_is_
  generated`. `no_third_party_image_is_referenced` — a grep for remote references in the SVG.
- **Acceptance:** both rows are current and the SVG is the source of record. Effect on
  `cargo xtask ci` wall time: **+0.1 s**.
- **Risk:** the outlined glyph paths in the SVG are large and make a noisy diff. Fallback: the SVG is
  committed once and rarely touched; the TIFF is regenerated only when the SVG changes.

### PLT-ASSET-011 — Examples: `examples/apartment.dxf` is generated by our own commands
- **State:** READY
- **Depends on:** PLT-ASSET-007
- **Files:** `examples/apartment.dxf`, `ATTRIBUTION.md:42` (owner **PLT**)
- **Contract:** the example is a **sample drawing made with `cadcraft-cli` commands**
  (`ATTRIBUTION.md:42`) — original, MIT OR Apache-2.0 — and lives under `examples/`, which is one of
  the three `ASSET_DIRS`, so it is scanned whatever its extension.
- **Behaviour:** Being generated by our own commands means the example can be **regenerated and
  compared**: the script that makes it is committed, and a test asserts the committed file matches what
  the script produces. That is what makes an example a fixture as well as a sample.
- **Refusals:** an example under `examples/` without a row is refused; an example copied from a
  third-party repository is refused whatever its licence.
- **Tests:** `the_example_regenerates_byte_identically` — run the commands and compare. `every_
  examples_file_has_a_row`. `the_example_is_a_dxf_we_can_read`.
- **Acceptance:** the example regenerates identically and opens. Effect on `cargo xtask ci` wall time:
  **+1 s** (one `cadcraft-cli` run — or reuse the binary the `test` step built).
- **Risk:** a binary-identical DXF depends on the writer being deterministic, which is a real property
  to want anyway. Fallback: the comparison is on the DXF's **text** form (DXF is text), so a
  timestamp-only difference is visible and reportable rather than fatal.

### PLT-ASSET-012 — `packaging/` metadata: desktop files, AppStream and Flatpak manifests
- **State:** READY
- **Depends on:** PLT-REL-007
- **Files:** `packaging/linux/**` (`*.desktop`, `*.appdata.xml`, `*.metainfo.xml`,
  `*.flatpak.yml`, `flatpak-bundle.sh`) (owner **PLT**)
- **Contract:** every desktop entry, AppStream metainfo and Flatpak manifest is **checked** by the
  packaging lint (`.github/workflows/packaging-lint.yml:2`: "Linux desktop/AppStream/Flatpak metadata"),
  and none of them is an asset (they are XML/desktop text under `packaging/`, not in `ASSET_DIRS`).
- **Behaviour:** The Flatpak manifest lists the from-source Flathub manifest as a separate file
  (`docs/releasing.md:7`), so the sandboxed build's provenance is the same project, not a third party.
- **Refusals:** a manifest naming an application ID that is not `ai.storyteller.*` is refused (that is
  the registered ID: `assets/app-icon/hicolor/*/apps/ai.storyteller.cadcraft.png`,
  `ATTRIBUTION.md:26-33`); an icon referenced by a manifest that is not in `assets/` is refused.
- **Tests:** `every_manifest_uses_the_project_application_id`. `every_manifest_icon_exists`. `the_
  packaging_lint_covers_the_metadata` (YAML assert).
- **Acceptance:** the lint passes; the metadata is consistent. Effect on `cargo xtask ci` wall time:
  **0 s** for the Rust gate; the packaging lint is a separate workflow (~10 s).
- **Risk:** an AppStream `release` entry drifts from the real release notes. Fallback: the entry is a
  short summary and the full notes are the link.

### PLT-ASSET-013 — The brand assets are the only non-original files, and that is asserted
- **State:** READY
- **Depends on:** PLT-CRED-007
- **Files:** `docs/brand/**`, `ATTRIBUTION.md:46-53`, `xtask/src/assets.rs` (owner **PLT**)
- **Contract:** the eight brand rows are the only rows whose licence is not `MIT OR Apache-2.0`; they
  are ArtCraft trademarks under `docs/brand/LICENSE-brand.txt`, and they are **referenced**, never
  modified and never used as product artwork.
- **Behaviour:** The test asserts the **negative**: every row outside `docs/brand/` is original,
  public domain/CC0, OSI, or redistributable Creative Commons. This is the file-level half of the
  asset policy, and it is the half a machine can check.
- **Refusals:** a modified brand file is refused (its digest changes; the brand is not ours to edit);
  a brand asset outside `docs/brand/` is refused.
- **Tests:** `no_row_outside_docs_brand_is_non_open`. `the_brand_files_are_unmodified` — digests
  against the values recorded in `ATTRIBUTION.md`'s Notes. `every_brand_row_names_the_brand_licence`.
- **Acceptance:** the assertion holds. Effect on `cargo xtask ci` wall time: **+0.2 s**.
- **Risk:** the brand files are updated upstream, so a digest check fails on a legitimate update.
  Fallback: the digest is recorded **with** the upstream version in the row, so the failure is
  informative.

### PLT-ASSET-014 — Close-out: the asset policy as a checkable list
- **State:** READY
- **Depends on:** PLT-ASSET-008, PLT-ASSET-009, PLT-ASSET-013
- **Files:** `ATTRIBUTION.md`, `xtask/src/assets.rs`, `docs/contributors.md` (owner **PLT**)
- **Contract:** the committed list, one row per rule with the check that enforces it:
  | rule | enforced by |
  |---|---|
  | every asset file has a row | `cargo xtask assets` (`assets.rs:26-41`) |
  | every row has author, source, licence | `PLT-DOC-015` |
  | no proprietary source | review (`PLT-CRED-009`'s checklist) |
  | generated-in-code assets need no row | `PLT-ASSET-003` |
  | generated fixtures follow `PLT-ASSET-008` | `PLT-ASSET-008`'s tests |
  | fonts live elsewhere | `PLT-ASSET-004` |
  | the brand exception is one directory | `PLT-ASSET-013` |
  `pub fn asset_policy_is_checked() -> bool;`
- **Behaviour:** One table where each rule names its enforcement and its owner, so a contributor
  asking "do I need a row?" gets an answer from a table rather than from a person. Half the rules are
  machine-checked and the table says which half.
- **Refusals:** a rule with no enforcement and no named owner is refused.
- **Tests:** `asset_policy_is_checked` — every row's "enforced by" names a task that exists in
  `PLT-CRED-006`'s table.
- **Acceptance:** the table exists in `ATTRIBUTION.md`. Effect on `cargo xtask ci` wall time: **0 s**.
- **Risk:** the table's "enforced by" column points at checks that are specified here but not yet
  implemented, so a reader may look for a gate CI is not running. Fallback: the column reads
  rule → enforcement, and an unimplemented enforcement is marked *(specified)*, so the table never
  claims a check that does not exist yet.

---

## Task count

| Epic | Tasks | IDs | Entry point |
|---|---:|---|---|
| `PLT-CI` | 18 | `PLT-CI-001..018` | the gate, the PR workflow, the matrix, caching, the per-step budget |
| `PLT-WASM` | 14 | `PLT-WASM-001..014` | the wasm set, the scan, the web build, what the browser cannot do |
| `PLT-LAYER` | 14 | `PLT-LAYER-001..014` | the rule engine, the table, the new-crate checklist, the blind spots |
| `PLT-DOC` | 16 | `PLT-DOC-001..016` | the docs map, gaps, parity, the checks that make a doc claim falsifiable |
| `PLT-PERF` | 16 | `PLT-PERF-001..016` | measure, baseline, and fail the build on a regression — no new dependency |
| `PLT-REL` | 14 | `PLT-REL-001..014` | versions, twelve artefacts, the release checklist |
| `PLT-CRED` | 12 | `PLT-CRED-001..012` | contributors, licences, attribution, the one question an agent asks |
| `PLT-ASSET` | 14 | `PLT-ASSET-001..014` | the scan, the policy as checks, the generated-fixture rule (`PLT-ASSET-008`) |
| **total** | **118** | | |

States at the time of writing: 101 `READY`, 13 `BACKLOG`, 4 `BLOCKED`. Every task's **Acceptance**
states its effect on `cargo xtask ci` wall time, as `master-plan.md` §5's `PLT-001..003` and
`AGENTS.md`'s gate rule require. Nothing here has been compiled or executed.