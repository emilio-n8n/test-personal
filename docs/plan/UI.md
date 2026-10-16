# UI — application shell and interaction

Task index for the `UI` domain (6 % of the programme). Owned crate:
`crates/ui-egui/**` (the egui front end). The 3D panel lives in
`crates/ui-egui/src/{viewport3.rs,viewport_state.rs,panels3/**}`.

**Owner letters** are the wave-1 agents of `docs/master-plan.md` §6: **A** kernel, **Q** quality,
**D** interchange, **U** ui/viewport, **C** orchestrator. Later waves reuse the domain letter
(`UI`) or are marked "co-sequenced". This index is owned by **U** unless a task says otherwise;
cross-domain tasks name their real owner and are `BLOCKED` until that agent's prerequisite lands.

---

## Invariants of this domain

1. **The UI is thin.** `CadApp` (`crates/ui-egui/src/lib.rs:193`) is a shell over
   `cadcraft_engine::Session`: panels read engine state and act through `app.run(id, params)`,
   `app.start(id)` or `app.cmdline(text)`. Nothing in the UI crate owns drawing data, and no UI
   task may introduce a second document model. A task that needs a new engine capability is
   `BLOCKED` on the engine owner, not worked around here.
2. **The 2D application must not regress.** The 2D baseline — `geom`, `constraints`, `doc`,
   `engine`, `io` — is re-run at every wave boundary. A UI task that touches a 2D code path must
   state in its Acceptance that the 2D test suite is green and name the test files it touched.
3. **This VM has no display.** UI work cannot be verified visually here. Every UI task carries
   the acceptance criterion *"verified by reading the rendered text or the egui shapes, not by
   eyeballing a screenshot"* and states it. A task that truly needs a window says so in its Risk.
4. **Every label is translatable.** A user-visible string is a `crate::tl!("…")` or
   `crate::tf!("…")` literal, and the Ukrainian catalog (`crates/ui-egui/src/i18n/uk.tsv`) is
   the reference translation: the test `translations_cover_commands_menus_tools_and_snap_labels`
   asserts every `tl!`/`tf!` literal in the UI crate has a `uk.tsv` row. **Every UI task in this
   index adds at least one row to `uk.tsv` as an acceptance criterion.** A new language with
   grammatical plurals also adds a `plural_<lang>` function and a `LANGUAGES` row.
5. **No new dependencies.** The UI crate depends on `egui`, `eframe`, `egui-wgpu`, `wgpu`,
   `winit`, `serde`, `serde_json`, `cadcraft-engine`, `cadcraft-render`, `cadcraft-doc`,
   `cadcraft-geom`, `cadcraft-fonts`. A task that wants a new `use` of an external crate is
   `BLOCKED` on an orchestrator decision; the fallback is always "do it with what is there".
6. **Colours come from `theme::Tokens`.** No UI task invents a colour literal; it reads
   `Tokens::get()` and, for model-space drawing, `Tokens::with_model_background`. A task that
   needs a new token adds it to `Tokens`, to both `Tokens::DARK` and `Tokens::LIGHT`, and to the
   `visuals()` mapping.
7. **Commands are the boundary.** A UI action that affects the drawing runs an engine command
   through `app.run`. A UI-only action (show a panel, change a theme) is a `UI_COMMANDS` entry in
   `menus.rs`. Neither may open a dialog: `engine.execute` runs the JSON form with defaults; only
   menu/toolbar invocation starts the interactive prompt sequence.
8. **No panics, no `unwrap`.** The crate root carries
   `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
   and `#![forbid(unsafe_code)]`. Every error is a named `Err` or a `session.echo` of the message.

### What is reusable from `crates/ui-egui` (evidence)

| Reusable | Where | Why it is the right reuse |
|---|---|---|
| `CadApp`, `UiState`, `Services` | `lib.rs:193,69,188` | The shell, its persisted state, and the host-services seam. Every panel is a `&mut CadApp` reader. |
| `CadApp::run/start/cmdline` | `lib.rs:309,328,339` | The three ways to reach a command: JSON programmatic, interactive start, typed line. |
| `theme::Tokens`, `Tokens::get()` | `theme.rs:70,217` | The design-token store. Every colour, every font id (`small()`, `body()`, `mono()`). |
| `theme::Canvas`, `set_canvas` | `theme.rs:243,275` | User drawing-area colours (model/paper background, snap marker size, selection colour). |
| `i18n::{t, tr, menu, tr_menu, format, count}` | `i18n/mod.rs:242-338` | The six lookup functions. `t` is the ordinary label, `tr_menu` a menu heading, `count` a pluralised form. |
| `i18n::with_language` | `i18n/mod.rs:204` | Scoped rendering in one language; the headless-test and control-channel screenshot path. |
| `i18n::Preference`, `LANGUAGES` | `i18n/mod.rs:12,155` | The 14 preferences and the 13-language table with their plural rules. |
| `viewport3::Frame`, `ViewportDelta` | `viewport3.rs:55,79` | The host→panel picture hand-off and the panel→host request. |
| `viewport_state::OrbitCamera` | `viewport_state.rs` | The orbit camera with its hard limits (`MIN_DISTANCE`, `MAX_DISTANCE`, `POLE_GAP`, `FRAME_MARGIN`). |
| `panels3::FeatureState` | `panels3/feature_tree.rs:35` | The four feature states (`Built`, `Pending`, `Failed`, `Blocked`). |
| `parametric::Glyph`, `Dof`, `Cache` | `parametric.rs:22,47,65` | The constraint glyph, the degrees-of-freedom summary, and the per-revision cache. |
| `menus::{MENUS, UI_COMMANDS, run_ui_command}` | `menus.rs:9,17` | The menu tree, the UI-only command table, and the dispatcher. |
| `control::{ControlRequest, ControlResponse}` | `control.rs:34,36` | The JSON-lines control channel: agents, tests and MCP drive the app through it. |
| `spacemouse::{Settings, State, Status}` | `spacemouse.rs:12,29,55` | The 3D-mouse preferences and status; the desktop app owns device I/O. |
| `viewcube::{Layout, Part}` | `viewcube.rs:24,51` | The ViewCube layout and hit-testing; the cube's actions go through commands. |
| `options::dialog::TABS` | `options/dialog.rs:15` | The six Options tabs (display, files, drafting, selection, preferences, profiles). |
| `CadApp::prefs_json/load_prefs` | `lib.rs:365,383` | Preferences persisted as JSON for the host's storage. |

### The state of the 3D viewport, quoted from its own docs

`viewport3.rs` states: *"The panel is a function over data the host owns … it reads no document,
holds no model, keeps no selection of its own, and runs no command."* The host rasterises
`cadcraft_render::scene3::render` (a CPU validation renderer, *"not built for interactive redraw"*)
on a worker thread and hands the result back as a `Frame`. The panel's job is to upload the frame,
fold pointer and keyboard input into an `OrbitCamera`, and hand the host a `ViewportDelta` with
`camera_changed` and a queued `PickRequest`.

---

# UI-APP — the application shell

### UI-APP-001 — Title bar and tool bar draw from the theme tokens
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/chrome.rs`, `crates/ui-egui/src/theme.rs` (owner U)
- **Contract:**
  ```rust
  pub fn title_and_toolbar(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** Draws the title bar (drawing name, saved state) and the tool bar (icon buttons
  for New/Open/Save/SaveAs/Plot/Undo/Redo) reading every colour from `Tokens::get()`. Icon buttons
  use `crate::icons` glyphs (drawn in code, no image files). The title bar's saved/unsaved indicator
  reads the engine's dirty flag through `app.session.state()`.
- **Refusals:** A drawing whose title is not valid UTF-8 is displayed lossily (never a panic). A
  missing icon falls back to a text label naming the command.
- **Tests:** `the_title_bar_draws_the_drawing_name` — render the title bar with a known drawing
  name and assert the painted text contains it. `the_toolbar_uses_token_colours` — assert every
  `Color32` used is a field of `Tokens::DARK` or `Tokens::LIGHT`, never a literal.
- **Acceptance:** the rendered text contains the drawing name; `cargo xtask ci` green; the
  Ukrainian catalog gains the row `Title bar	Панель заголовка` in `uk.tsv`.
- **Risk:** the chrome module is shared with 2D; a change here could regress the 2D look.
  Fallback: gate any visual change behind a `UiState` flag so 2D keeps today's appearance.

### UI-APP-002 — File tabs switch between open drawings
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/chrome.rs` (owner U)
- **Contract:**
  ```rust
  pub fn file_tabs(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** Draws one tab per open drawing (`app.session.docs`), showing the drawing name
  and a dirty marker. Clicking a tab calls the engine's `file.switch` command. A middle-click
  closes the tab (running `file.close`). The active tab is highlighted with `Tokens::tab_active`.
- **Refusals:** A drawing whose name is empty shows `Drawing` (the engine's own default). A close of
  the last drawing shows the start page, not an empty shell.
- **Tests:** `file_tabs_list_every_open_drawing` — open two drawings via the control channel and
  assert the painted text contains both names. `the_active_tab_is_highlighted` — assert the
  active tab's background is `Tokens::tab_active`.
- **Acceptance:** the tab bar appears only when `UiState.show_file_tabs` is true; the Ukrainian
  catalog gains `File Tab	Вкладка файлу` in `uk.tsv`.
- **Risk:** many open drawings overflow the tab bar. Fallback: a horizontal scroll, tested with
  20 drawings.

### UI-APP-003 — Status bar shows drafting toggles and the prompt
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/chrome.rs`, `crates/ui-egui/src/cmdline.rs` (owner U)
- **Contract:**
  ```rust
  pub fn status_bar(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** Draws the drafting toggles (Snap, Grid, Ortho, Polar, Osnap, DynInput) as toggle
  buttons using `Tokens::toggle_on` for the on state, plus the coordinate readout and the
  command-line prompt area. Each toggle runs the corresponding engine command on click.
- **Refusals:** A toggle whose engine command fails shows the error via `session.echo`, never a
  silent desync. A non-finite coordinate reads as `0` (the existing `format_linear` behaviour).
- **Tests:** `the_status_bar_lists_every_drafting_toggle` — assert the painted text contains
  `Snap`, `Grid`, `Ortho`, `Polar`, `Osnap`, `DynInput` in the active language. `a_toggle_click_runs_the_command`
  — click the Grid toggle via `ui.click` and assert the engine's grid flag flipped.
- **Acceptance:** the status bar appears only when `UiState.show_status_bar` is true; the
  Ukrainian catalog gains `Status Bar	Рядок стану` in `uk.tsv`.
- **Risk:** the status bar is 2D-shared. Fallback: any new toggle is gated behind a 3D check so
  the 2D status bar is unchanged.

### UI-APP-004 — Start page appears when no drawing is open
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/chrome.rs` (owner U)
- **Contract:**
  ```rust
  pub fn start_page(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** Draws the start page (New, Open, Open Sample, recent files list) when
  `app.session.docs.is_empty()` or `UiState.start_tab`. Recent files come from
  `UiState.recent_files` and are capped at `RECENT_FILES` (9). Each item runs the corresponding
  command (`file.new`, `file.open`, `ui.sample`).
- **Refusals:** A recent file that no longer exists is skipped with a `session.echo` naming it,
  never a panic. An empty recent-files list hides the section entirely.
- **Tests:** `the_start_page_shows_recent_files` — open a drawing, close it, and assert the start
  page's painted text contains its name. `nine_recent_files_are_kept` — open ten drawings and
  assert the list shows exactly nine.
- **Acceptance:** the start page renders headlessly (verified by reading the rendered text, not by
  eyeballing a screenshot); the Ukrainian catalog gains `Start	Початкова сторінка` in `uk.tsv`.
- **Risk:** the start page is the first thing a user sees; a broken start page means a broken
  first impression. Fallback: if the recent-files read fails, show the page without it.

### UI-APP-005 — Docking layout for panels
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/docking.rs` (owner U)
- **Contract:**
  ```rust
  pub fn show(app: &mut CadApp, ui: &mut egui::Ui);
  pub struct Workspace { /* persisted panel positions and sizes */ }
  ```
- **Behaviour:** Draws the dockable panels (Tool Sets, Layers, Properties, feature tree,
  parameters) using egui's docking. The layout is persisted in `UiState.docking` and restored on
  startup. A panel's visibility is controlled by `UiState.show_*` flags.
- **Refusals:** A corrupted persisted layout falls back to the default arrangement with a
  `session.echo` warning. A panel whose content is empty draws an empty frame, not a crash.
- **Tests:** `the_default_layout_has_a_tool_sets_panel` — render with a default `Workspace` and
  assert the painted text contains `Tool Sets`. `a_saved_layout_is_restored` — persist a layout,
  create a new `CadApp`, and assert the panels are in the same positions.
- **Acceptance:** `ui.resetpalettes` restores the default layout; the Ukrainian catalog gains
  `Arrange Panel	Розташувати панель` in `uk.tsv`.
- **Risk:** egui's docking API changes between versions. Fallback: pin the egui version and
  isolate the docking code behind a wrapper.

### UI-APP-006 — Options dialog: six tabs, one JSON path
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/options/dialog.rs` (owner U)
- **Contract:**
  ```rust
  pub fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool);
  pub const TABS: [(&str, &str); 6]; // display, files, drafting, selection, preferences, profiles
  ```
- **Behaviour:** Shows the Options dialog with its six tabs. Every change is collected into a
  `serde_json::Map` and applied through `app.run("options", …)`, so the dialog, scripts and the
  control channel share one path. The open tab is part of the dialog name (`"options:display"`),
  so `ui.set` can show any tab. Changes apply at once.
- **Refusals:** An `options` command failure is echoed to the command line and the dialog stays
  open with the unsaved changes highlighted. A tab with no changes does not run the command.
- **Tests:** `the_options_dialog_shows_six_tabs` — open the dialog and assert the painted text
  contains all six tab labels in the active language. `a_change_runs_the_options_command` —
  toggle a setting and assert the engine's state changed.
- **Acceptance:** the dialog renders headlessly; the Ukrainian catalog gains `Options…	Параметрі…`
  in `uk.tsv`.
- **Risk:** the `options` command is engine-owned (C). Fallback: the dialog's JSON shape is
  agreed with the engine owner before implementation.

### UI-APP-007 — Keyboard shortcuts reach commands
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/menus.rs`, `crates/ui-egui/src/shortcut_menus.rs` (owner U)
- **Contract:**
  ```rust
  pub fn shortcuts(app: &mut CadApp, ctx: &egui::Context);
  ```
- **Behaviour:** Registers the keyboard shortcuts from `UI_COMMANDS` (e.g. `Cmd+O` for Open,
  `Cmd+S` for Save, `F1` for Help). A shortcut press runs the corresponding command through
  `app.run` or `app.start`. Shortcuts are active only when the widget they belong to has focus,
  so the command line and the drawing keep their keys.
- **Refusals:** A shortcut whose command fails echoes the error. A conflicting shortcut (two
  commands on the same key) is resolved by registration order and reported in a test.
- **Tests:** `every_ui_command_shortcut_is_registered` — walk `UI_COMMANDS` and assert each
  shortcut's key combination is unique. `cmd_o_opens_a_drawing` — send the Cmd+O key event and
  assert the file-open dialog command ran.
- **Acceptance:** the shortcuts work in the headless control-channel test (verified by asserting
  the command ran, not by eyeballing a screenshot); the Ukrainian catalog gains `Command Reference
  Довідник команд` in `uk.tsv`.
- **Risk:** platform-specific modifiers (Cmd on macOS, Ctrl elsewhere). Fallback: use egui's
  `Modifiers` which handles this.

### UI-APP-008 — Recent files list persists across restarts
- **State:** READY
- **Depends on:** UI-APP-004
- **Files:** `crates/ui-egui/src/lib.rs` (owner U)
- **Contract:**
  ```rust
  pub const RECENT_FILES: usize = 9;
  // UiState.recent_files: Vec<String>, newest first
  ```
- **Behaviour:** `remember_recent` puts a path first in `UiState.recent_files` and truncates to
  `RECENT_FILES`. The list is persisted through `prefs_json`/`load_prefs` and shown on the start
  page and in File > Open Recent. A file that no longer exists is skipped on click.
- **Refusals:** A path that is not absolute is stored as-is (the host resolves it) but flagged in
  the UI with a dimmed colour. An empty path is never stored.
- **Tests:** `recent_files_are_capped_at_nine` — open ten drawings and assert the list has nine
  entries. `recent_files_survive_a_restart` — persist via `prefs_json`, load into a new `CadApp`,
  and assert the list is restored.
- **Acceptance:** the recent-files list survives a restart; the Ukrainian catalog gains `Open Recent
  Відкрити нещодавні` in `uk.tsv`.
- **Risk:** a malicious path in the recent-files list. Fallback: paths are opened through the
  engine's own `file.open` which validates them.

### UI-APP-009 — Autosave ticks on a timer
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/autosave.rs` (owner U)
- **Contract:**
  ```rust
  pub fn autosave_tick(app: &mut CadApp, ctx: &egui::Context);
  pub struct Recovery { /* autosave files a crashed app left behind */ }
  ```
- **Behaviour:** `autosave_tick` runs every frame and checks whether the autosave interval has
  elapsed. If so, it asks the engine to write an autosave file. The recovery panel (shown at
  startup) lists autosave files found and offers to recover them. Autosave is desktop-only
  (`Option<Autosaver>`); on web it is `None`.
- **Refusals:** An autosave write failure is echoed to the status bar, never shown as a crash. A
  recovery of a corrupted autosave file reports the error and offers to delete it.
- **Tests:** `autosave_writes_a_file_after_the_interval` — set the interval to zero, tick, and
  assert an autosave file exists. `recovery_lists_autosave_files` — write an autosave file and
  assert the recovery panel lists it.
- **Acceptance:** autosave can be enabled and disabled from the Options dialog; the Ukrainian
  catalog gains `Autosave	Автозбереження` in `uk.tsv`.
- **Risk:** autosave writing while the user is mid-command. Fallback: autosave waits for a quiet
  frame (no command running).

### UI-APP-010 — Closing a drawing with unsaved changes asks first
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/closing.rs` (owner U)
- **Contract:**
  ```rust
  pub fn prompt(app: &mut CadApp, ctx: &egui::Context);
  pub struct Closing { /* the close request and its answers */ }
  ```
- **Behaviour:** When a drawing with unsaved changes is closed (tab close, window close, quit),
  a prompt asks Save / Discard / Cancel. Save runs the engine's `save` command; Discard closes
  without saving; Cancel aborts the close. The prompt is modal and blocks other input.
- **Refusals:** A save failure keeps the drawing open and shows the error. A Cancel returns to the
  drawing unchanged.
- **Tests:** `closing_with_unsaved_changes_shows_a_prompt` — modify a drawing, close it, and
  assert the prompt appears. `discard_closes_without_saving` — choose Discard and assert the drawing
  is closed and the file is unchanged.
- **Acceptance:** the prompt renders headlessly; the Ukrainian catalog gains `Save changes?
  Зберегти зміни?` in `uk.tsv`.
- **Risk:** a prompt during a control-channel quit (which never asks). Fallback: the control
  channel's `app.quit` bypasses the prompt by design, documented in `control.rs`.

### UI-APP-011 — Drop a file onto the window to open it
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/lib.rs` (owner U)
- **Contract:**
  ```rust
  // in CadApp::logic:
  for f in ctx.input(|i| i.raw.dropped_files.clone()) { self.open_path(&p); }
  ```
- **Behaviour:** Dropped files are collected every frame from egui's raw input and opened through
  `open_path`. Multiple files are opened in order. A LISP file (`.lsp`) is loaded as APPLOAD loads
  it. On web, dropped files go through `open_bytes` (the picker path).
- **Refusals:** A file that fails to open is reported via `open_failed` (command line + status bar).
  A file over `MAX_OPEN_BYTES` is refused with the size error.
- **Tests:** `a_dropped_file_is_opened` — inject a synthetic dropped-file event and assert the
  drawing opens. `a_lisp_file_is_loaded_as_appload` — drop a `.lsp` file and assert `lisp_load`
  ran.
- **Acceptance:** the drop works in the headless test (verified by asserting the drawing opened,
  not by eyeballing a screenshot); the Ukrainian catalog gains `Open	Відкрити` in `uk.tsv`.
- **Risk:** a large number of dropped files at once. Fallback: queue them and open sequentially.

### UI-APP-012 — In-window menu bar mirrors the macOS native menu
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/menus.rs` (owner U)
- **Contract:**
  ```rust
  pub fn menu_bar(app: &mut CadApp, ui: &mut egui::Ui);
  pub const MENUS: &[&str]; // File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Window, Help
  ```
- **Behaviour:** Draws the in-window menu bar (used on platforms without a native menu bar, or on
  request). Each menu heading opens a dropdown with its items. Items are `UI_COMMANDS` entries or
  engine commands with their menu path. The menu tree is also exposed through the control channel
  (`ui.menu.list`, `ui.menu.invoke`).
- **Refusals:** A menu item whose command fails echoes the error. An empty menu draws nothing.
- **Tests:** `the_menu_bar_lists_every_menu` — render the menu bar and assert the painted text
  contains all 11 menu headings in the active language. `a_menu_item_runs_its_command` — invoke
  an item through `ui.menu.invoke` and assert the command ran.
- **Acceptance:** the menu bar can be toggled via `ui.toggle.menubar`; the Ukrainian catalog gains
  `In-window Menu Bar	Рядок меню у вікні` in `uk.tsv`.
- **Risk:** the menu tree is shared with 2D. Fallback: any 3D-specific menu item is gated behind
  a 3D check.

---

# UI-VP — the 3D viewport

### UI-VP-001 — Viewport panel: the host's picture in an egui widget
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub struct Frame { pub revision: u64, pub image: cadcraft_render::scene3::Image }
  pub fn show(app: &mut CadApp, ctx: &egui::Context, frame: &Frame, state: &mut ViewportState) -> (egui::Response, ViewportDelta);
  ```
- **Behaviour:** Uploads the host's `Frame` image as an egui texture (once per `revision`, not
  once per frame), draws it in the panel, and overlays the ViewCube and UCS icon. The panel's
  minimum size is `MIN_PANEL` (160x120) so a collapsed dock still shows something.
- **Refusals:** An empty frame (0x0) draws nothing and reports `ViewportDelta::default()`. A frame
  whose revision has not changed is not re-uploaded.
- **Tests:** `a_frame_is_uploaded_once_per_revision` — two frames with the same revision produce
  one texture upload. `an_empty_frame_draws_nothing` — a 0x0 frame produces an empty panel.
- **Acceptance:** the panel renders headlessly (verified by asserting the texture upload count,
  not by eyeballing a screenshot); the Ukrainian catalog gains `Viewport	Видовий екран` in `uk.tsv`.
- **Risk:** the texture upload path is wgpu-specific. Fallback: on web, the upload uses egui's
  own texture manager.

### UI-VP-002 — Orbit, pan and zoom gestures
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewport_state.rs`, `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub enum Gesture { Orbit, Pan, Dolly }
  // OrbitCamera::orbit(dx, dy), pan(dx, dy), dolly(notches)
  ```
- **Behaviour:** Left drag orbits, middle or right drag pans, wheel dollies. Every gesture is
  expressed per pixel of pointer travel or per wheel notch (`ORBIT_RADIANS_PER_PIXEL`,
  `DOLLY_FACTOR_PER_NOTCH`), never as a per-frame velocity. The camera has no inertia. With the
  panel focused: arrows orbit, `+`/`-` dolly, `F` frames the body, Home returns to the standard
  view.
- **Refusals:** A non-finite delta is ignored by `OrbitCamera::sanitize` (never propagated). A
  dolly past `MAX_DOLLY_NOTCHES` is clamped. A camera at the pole is clamped to `POLE_GAP`.
- **Tests:** `orbit_moves_the_camera_by_the_pointer_delta` — send a drag and assert the azimuth
  and elevation changed by `ORBIT_RADIANS_PER_PIXEL` times the pixel delta. `a_nan_delta_is_ignored`
  — send a NaN delta and assert the camera is unchanged.
- **Acceptance:** the gestures work in the headless test (verified by asserting the camera state,
  not by eyeballing a screenshot); the Ukrainian catalog gains `Orbit	Орбіта` in `uk.tsv`.
- **Risk:** egui's gesture recognition changes between versions. Fallback: the gesture detection
  is isolated in `viewport3.rs` and unit-tested through `ViewportState`.

### UI-VP-003 — Picking: a click queues a request
- **State:** BLOCKED
- **Depends on:** UI-VP-001, REN-002 (picking in the renderer)
- **Files:** `crates/ui-egui/src/viewport_state.rs`, `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub struct PickRequest { pub screen_pos: [f32; 2] }
  // ViewportState.pick(app, request) → the host resolves it
  ```
- **Behaviour:** A click (press + release with no drag) queues a `PickRequest` in the
  `ViewportState` and reports it in `ViewportDelta`. The host resolves it (the host owns the scene
  and the model). The pick result populates `ViewportState::selection`.
- **Refusals:** A click that is also a drag (past the drag threshold) does not queue a pick. A
  pick request the host cannot resolve (no scene, no model) returns an empty selection.
- **Tests:** `a_click_queues_a_pick_request` — send a click and assert the delta contains a
  `PickRequest`. `a_drag_does_not_queue_a_pick` — send a drag and assert no pick is queued.
- **Acceptance:** the pick request is resolved by the host and the selection is shown in the
  viewport (verified by asserting the selection state, not by eyeballing a screenshot); the
  Ukrainian catalog gains `Select	Вибрати` in `uk.tsv`.
- **Risk:** picking needs the renderer's picking module (`cadcraft-render::picking`), which is
  REN-owned. Fallback: the panel queues the request; the host (REN owner) implements resolution.

### UI-VP-004 — ViewCube: top face, home, compass ring
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewcube.rs` (owner U)
- **Contract:**
  ```rust
  pub struct Layout { pub center: egui::Pos2, pub ring: f32, pub face: egui::Rect, pub home: egui::Rect, pub pill: egui::Rect, pub bounds: egui::Rect }
  pub enum Part { Face, Home, Ring }
  ```
- **Behaviour:** Draws the ViewCube overlay (Top face, Home button, compass ring, WCS menu).
  Clicking the Top face runs `zoom.extents`; Home runs the standard view command. The compass
  ring explains that the 2D plan view has no view twist yet (`NO_TWIST` message). The overlay
  registers a widget that senses clicks *and* drags, so egui hands the pointer to the overlay
  instead of the canvas underneath.
- **Refusals:** A click on the compass ring in a 2D plan view shows the `NO_TWIST` message (the
  2D view has no view twist yet). A click outside the overlay bounds falls through to the canvas.
- **Tests:** `a_face_click_runs_zoom_extents` — click the Top face and assert `zoom.extents`
  ran. `a_ring_click_in_2d_shows_the_no_twist_message` — click the ring and assert the
  `NO_TWIST` message appears.
- **Acceptance:** the ViewCube renders headlessly; the Ukrainian catalog gains `ViewCube  Видовий
  куб` in `uk.tsv`.
- **Risk:** the ViewCube is a 3D overlay on a 2D canvas. Fallback: the compass ring's explanation
  is the honest answer for now; a real 3D twist comes with the 3D view direction.

### UI-VP-005 — UCS icon shows the coordinate system
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewcube.rs` (owner U)
- **Contract:**
  ```rust
  // drawn in code: an X axis arrow (Tokens.axis_x) and a Y axis arrow (Tokens.axis_y)
  ```
- **Behaviour:** Draws the UCS icon at the bottom-left of the viewport, showing the X and Y axes
  with the theme's axis colours. In the 2D plan view it shows the world coordinate system. A
  click on the icon runs the UCS menu command.
- **Refusals:** A non-finite axis direction (impossible from `OrbitCamera::sanitize`) is drawn as
  a zero-length arrow, never a NaN coordinate.
- **Tests:** `the_ucs_icon_draws_two_arrows` — render the viewport and assert the painted shapes
  contain two line segments in `axis_x` and `axis_y` colours. `a_click_runs_the_ucs_menu` —
  click the icon and assert the command ran.
- **Acceptance:** the UCS icon renders headlessly; the Ukrainian catalog gains `UCS Icon  Значок
  СК` in `uk.tsv`.
- **Risk:** the UCS icon is 2D-shared. Fallback: a 3D UCS icon (three axes) is gated behind a
  3D check.

### UI-VP-006 — Frame upload: one per revision, not per frame
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  const FRAME_ID: &str = "cadcraft_viewport3_frame";
  // egui texture id: egui::Id::new(FRAME_ID)
  ```
- **Behaviour:** The panel uploads the frame's image as an egui texture keyed by `FRAME_ID`,
  once per `revision`. A per-frame upload of a 900x700 RGBA buffer asks the GPU to re-take 2.5 MB
  forty times a second for a picture that did not change. The revision is the host's word for
  "this is a different picture".
- **Refusals:** A frame with a smaller image than the previous one reallocates the texture (never
  reuses a stale larger buffer). A frame with the same revision is not re-uploaded.
- **Tests:** `the_same_revision_uploads_once` — two frames with revision 1 produce one upload. `a_new_revision_uploads_again`
  — frame 1 then frame 2 produce two uploads.
- **Acceptance:** the upload count is asserted in a headless test; the Ukrainian catalog gains no
  new row (the ViewCube row from UI-VP-004 covers the viewport chrome).
- **Risk:** egui's texture manager changes between versions. Fallback: isolate the upload behind
  a helper that can be mocked.

### UI-VP-007 — Spacemouse navigation
- **State:** READY
- **Depends on:** UI-VP-002
- **Files:** `crates/ui-egui/src/spacemouse.rs` (owner U)
- **Contract:**
  ```rust
  pub struct Settings { pub enabled: bool, pub pan_speed: f64, pub zoom_speed: f64, pub reverse_zoom: bool }
  pub struct State { pub settings: Settings, pub status: Status, pub device: Option<String> }
  pub enum Status { Unavailable, Searching, Calibrating, Connected, OpenFailed, Disconnected, Error }
  ```
- **Behaviour:** The spacemouse state is host-independent: device I/O lives in the desktop app.
  The UI crate owns the settings (persisted in `prefs_json`), the status display, and the
  navigation request that the native host consumes. Settings: pan speed (visible extents per
  second at full deflection), zoom speed (exponential rate per second), reverse zoom toggle.
- **Refusals:** A device that fails to open shows `OpenFailed` with a message naming the likely
  cause ("3DxWare or missing device permissions"). A disconnected device shows `Disconnected`,
  never a panic.
- **Tests:** `the_status_messages_are_all_translated` — assert every `Status` message has a
  `uk.tsv` row. `the_settings_round_trip_through_prefs` — persist and restore and assert the
  settings are unchanged.
- **Acceptance:** the spacemouse can be enabled and disabled from the Options dialog; the
  Ukrainian catalog gains `3D Mouse...	3D-миша...` in `uk.tsv`.
- **Risk:** the desktop host's device I/O is not in this crate. Fallback: the UI crate's part is
  the settings and status; the device I/O is a co-sequenced task with the desktop app owner.

### UI-VP-008 — Zoom extents frames the body
- **State:** BLOCKED
- **Depends on:** UI-VP-002, REN-001 (render path)
- **Files:** `crates/ui-egui/src/viewport3.rs`, `crates/ui-egui/src/viewport_state.rs` (owner U)
- **Contract:**
  ```rust
  pub fn frame(&mut self, margin: f64); // OrbitCamera::frame, FRAME_MARGIN = 1.25
  ```
- **Behaviour:** `OrbitCamera::frame` sets the distance so the body fits with a 25% margin. It
  reads the body's bounds from the scene (the host's job to provide). The command `zoom.extents`
  is the entry point; it is run by the ViewCube's Top face and Home button.
- **Refusals:** A body with zero bounds (empty scene) is refused with a message naming it, not a
  camera at zero distance. A non-finite bound is sanitised by `OrbitCamera::sanitize`.
- **Tests:** `frame_sets_the_distance_for_the_bounds` — set bounds, call `frame`, and assert the
  distance matches the bounds and margin. `an_empty_scene_is_refused` — zero bounds produce an
  error.
- **Acceptance:** the command works in the headless test (verified by asserting the camera
  state, not by eyeballing a screenshot); the Ukrainian catalog gains `Extents	За межами
  об'єктів` in `uk.tsv`.
- **Risk:** the body bounds come from the renderer (REN-owned). Fallback: the camera arithmetic
  is in `viewport_state.rs` (U-owned); the host provides the bounds.

### UI-VP-009 — Standard views: top, front, right
- **State:** BLOCKED
- **Depends on:** UI-VP-008, DAT-066 (SavedView)
- **Files:** `crates/ui-egui/src/viewport_state.rs` (owner U)
- **Contract:**
  ```rust
  // OrbitCamera::standard_view(view: StandardView)
  pub enum StandardView { Top, Bottom, Front, Back, Left, Right }
  ```
- **Behaviour:** Sets the camera's azimuth and elevation to the six standard views (top = looking
  down -Z, front = looking down -Y, etc.). The views are generated from the model's bounds and the
  document's unit, never stored. `PLANESWITCH` in the engine is the 2D precedent.
- **Refusals:** A standard view whose elevation would hit the pole is clamped to `POLE_GAP`. A
  view with non-finite bounds is refused.
- **Tests:** `the_top_view_looks_down` — set Top and assert the elevation is at the pole gap. `the_front_view_looks_along_y`
  — set Front and assert the azimuth and elevation match.
- **Acceptance:** the standard views work in the headless test; the Ukrainian catalog gains
  `Standard View	Стандартний вид` in `uk.tsv`.
- **Risk:** the 2D plan view only has a top view. Fallback: the other five views are the 3D
  answer; the 2D view keeps only Top.

### UI-VP-010 — Viewport label shows the view name
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  // drawn at the bottom-left of the viewport: the view name and the visual style
  ```
- **Behaviour:** Draws the viewport label (e.g. "Top", "Front", "Custom view") at the
  bottom-left of the viewport, using `Tokens::canvas_ink` (which follows the model-space
  background, not the interface theme). The label updates when the view changes.
- **Refusals:** An empty view name shows nothing (the label is hidden). A non-finite view name is
  impossible (it is a `&'static str` or a `String` from a fixed set).
- **Tests:** `the_label_shows_the_current_view` — set the view and assert the painted text
  contains the view name. `an_empty_name_hides_the_label` — set an empty name and assert no
  label text.
- **Acceptance:** the label renders headlessly; the Ukrainian catalog gains `Viewport Label
  Назва видового екрана` in `uk.tsv`.
- **Risk:** the label is 2D-shared. Fallback: a 3D-specific label is gated behind a 3D check.

### UI-VP-011 — GPU render path uploads the frame
- **State:** BLOCKED
- **Depends on:** UI-VP-001, REN-001 (GPU render path)
- **Files:** `crates/ui-egui/src/gpu.rs`, `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub struct GpuFaults { /* device loss and error reporting */ }
  pub fn install(rs: &egui_wgpu::RenderState, faults: &GpuFaults, stencil: Option<TextureFormat>) -> GpuState;
  ```
- **Behaviour:** When the app has a wgpu render state (`CreationContext::wgpu_render_state`),
  the canvas draws on the GPU instead of the CPU. The viewport's frame upload uses the GPU's
  texture manager. GPU errors and device loss are reported through `GpuFaults` and shown in the
  status bar, never as a crash.
- **Refusals:** A GPU device loss shows a notice and falls back to the CPU rasteriser (the
  `gpu_notice` in `autosave`). A shader compilation failure is reported and the CPU path is used.
- **Tests:** `a_gpu_device_loss_falls_back_to_cpu` — simulate a device loss and assert the canvas
  still renders. `a_shader_failure_is_reported` — simulate a shader error and assert the notice
  appears.
- **Acceptance:** the GPU path renders headlessly (verified by asserting the texture upload,
  not by eyeballing a screenshot); the Ukrainian catalog gains no new row (the fault messages are
  in the autosave catalog).
- **Risk:** wgpu is a complex dependency. Fallback: the GPU path is opt-in (the app runs without
  it); the CPU rasteriser is always available.

### UI-VP-012 — Viewport shows a "Building…" card while the scene renders
- **State:** READY
- **Depends on:** UI-VP-001
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub fn building(&self) -> bool; // CanvasState::building
  ```
- **Behaviour:** While the host is rasterising a new frame (the camera or the model changed), the
  viewport shows a "Building…" card in `Tokens::warn` colour. The card is removed when the new
  frame arrives. This is the 3D equivalent of the 2D "Opening…" card.
- **Refusals:** A frame that never arrives (the host crashed) leaves the card shown indefinitely;
  the autosave recovery handles the crash. A card with a non-finite progress value is not shown.
- **Tests:** `a_building_scene_shows_the_card` — set `building = true` and assert the painted
  text contains `Building`. `a_finished_scene_removes_the_card` — set `building = false` and
  assert the card is gone.
- **Acceptance:** the card renders headlessly; the Ukrainian catalog gains `Building…	Побудова…`
  in `uk.tsv`.
- **Risk:** the building state is host-specific. Fallback: the panel reads `CanvasState::building`
  which the host sets.

### UI-VP-013 — Section box clips the view
- **State:** BLOCKED
- **Depends on:** UI-VP-001, KER-SRF-004 (section curves)
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub struct SectionBox { pub min: [f64; 3], pub max: [f64; 3] }
  // the host applies the section to the scene before rendering
  ```
- **Behaviour:** A section box is a rectangular clipping volume. The host applies it to the scene
  before rendering (the panel only draws the box's outline and handles its manipulation). The
  section box's outline is drawn with `Tokens::window_stroke`.
- **Refusals:** A section box with min > max is normalised (min and max are swapped), not refused.
  A section box that is degenerate (zero volume) is refused with a message naming it.
- **Tests:** `a_section_box_outline_is_drawn` — set a box and assert the painted shapes contain
  its outline. `a_degenerate_box_is_refused` — zero volume produces an error.
- **Acceptance:** the section box renders headlessly; the Ukrainian catalog gains `Section Box
  Січна рамка` in `uk.tsv`.
- **Risk:** the section itself needs kernel section curves (KER-SRF-004). Fallback: the panel
  draws the box and the host applies the clipping; the kernel section is a co-sequenced task.

### UI-VP-014 — Visual styles: wireframe, hidden line, shaded
- **State:** BLOCKED
- **Depends on:** UI-VP-001, REN-003 (visstyles)
- **Files:** `crates/ui-egui/src/viewport3.rs` (owner U)
- **Contract:**
  ```rust
  pub enum VisStyle { Wireframe, HiddenLine, Shaded, ShadedWithEdges }
  ```
- **Behaviour:** The visual style selects how the scene is rasterised: wireframe (edges only),
  hidden line (edges with hidden-line removal), shaded (faces with lighting), shaded with edges
  (both). The host applies the style to the scene before rendering; the panel only shows the
  current style and a selector.
- **Refusals:** A visual style the renderer does not support is refused with a message naming it.
  A style with a non-finite parameter is sanitised.
- **Tests:** `every_visual_style_is_offered` — assert the selector lists all four styles. `an_unsupported_style_is_refused`
  — select an unsupported style and assert the error.
- **Acceptance:** the selector renders headlessly; the Ukrainian catalog gains `Visual Style
  Візуальний стиль` in `uk.tsv`.
- **Risk:** the renderer (REN-owned) must support the styles. Fallback: the panel offers the
  styles; the renderer implements them over time.

---

# UI-TREE — the feature tree panel

### UI-TREE-001 — Feature tree panel: one row per feature
- **State:** BLOCKED
- **Depends on:** MOD-001 (sketch-backed features)
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs`, `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  pub struct FeatureRow { pub name: String, pub state: FeatureState, pub children: Vec<usize> }
  pub fn show(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** Draws the model's features in order, each with its `FeatureState` (Built, Pending,
  Failed, Blocked). A feature's children are shown indented below it. The panel reads the model's
  feature list through the engine's state; it does not import `cadcraft_model`. A new feature kind
  is a change to the model, never to this file.
- **Refusals:** A feature whose name is not valid UTF-8 is displayed lossily. An empty feature list
  shows an empty panel, not a crash.
- **Tests:** `the_tree_lists_every_feature` — build a model with three features and assert the
  painted text contains all three names. `an_empty_model_shows_an_empty_panel` — assert no rows.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Feature Tree  Дерево
  елементів` in `uk.tsv`.
- **Risk:** the feature list comes from the model (MOD-owned). Fallback: the panel reads the
  engine's state; the model provides the list.

### UI-TREE-002 — Failed feature shows the error on the row
- **State:** BLOCKED
- **Depends on:** UI-TREE-001
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // FeatureState::Failed(String) — the second line of the row, in Tokens::warn
  ```
- **Behaviour:** A failed feature's error message is a second line of the row itself, in the
  warning colour, with the message the kernel gave — not a summary, not a tooltip. The panel reads
  the message from `FeatureState::Failed`. A 14-pixel glyph should not be the only way to find out
  what it means.
- **Refusals:** An empty error message shows `Failed` with no second line. A message that is too
  long is truncated with an ellipsis and the full text is in the tooltip.
- **Tests:** `a_failed_feature_shows_its_message` — set a `Failed("bad depth")` and assert the
  painted text contains `bad depth`. `an_empty_message_shows_only_the_state` — assert no second
  line.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Failed	Не вдалося` in
  `uk.tsv`.
- **Risk:** the error message comes from the kernel (A-owned). Fallback: the panel displays
  whatever string the model provides.

### UI-TREE-003 — Blocked feature names what blocked it
- **State:** BLOCKED
- **Depends on:** UI-TREE-002
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // FeatureState::Blocked { by: String, message: String }
  ```
- **Behaviour:** A blocked feature (one that never ran because an earlier feature failed) shows
  the name of the feature that blocked it and its message. This is the same design as the failed
  row: the information is on the row, not in a tooltip.
- **Refusals:** A blocked feature with no blocker (impossible by construction) shows `Blocked`
  with no second line. A blocker name that is too long is truncated.
- **Tests:** `a_blocked_feature_names_its_blocker` — set `Blocked { by: "Extrude", message: "bad"
  }` and assert the painted text contains `Extrude` and `bad`.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Blocked	Заблоковано`
  in `uk.tsv`.
- **Risk:** the blocker name comes from the model (MOD-owned). Fallback: the panel displays
  whatever the model provides.

### UI-TREE-004 — Suppress a feature (skip its rebuild)
- **State:** BLOCKED
- **Depends on:** UI-TREE-001, MOD-005 (robust rebuild)
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the panel sends the command; the engine and model implement suppression
  ```
- **Behaviour:** A suppressed feature is still part of the rebuild but hidden from the list (a
  crossed-out eye marker). The panel sends the suppress command through `app.run`; the engine and
  model implement the suppression logic. The marker is drawn in code (no iconography).
- **Refusals:** A suppress of a feature that does not exist is refused with a message naming it. A
  suppress of the last feature is refused (the model would have no solid).
- **Tests:** `a_suppressed_feature_is_hidden` — suppress a feature and assert its row is gone. `a_suppressed_feature_is_marked`
  — assert the crossed-out eye marker is drawn.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Suppress	Придушити` in
  `uk.tsv`.
- **Risk:** suppression is a model (MOD-owned) feature. Fallback: the panel sends the command; the
  model implements it.

### UI-TREE-005 — Roll back to an earlier feature
- **State:** BLOCKED
- **Depends on:** UI-TREE-004
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the rollback marker: a turning arrow, drawn in code
  ```
- **Behaviour:** A rolled-back feature's definition is an earlier one the user chose. The panel
  shows a turning-arrow marker and a tooltip saying which feature it rolled back to. The rollback
  itself is a model operation (MOD-owned); the panel only shows the state.
- **Refusals:** A rollback to a feature that does not exist is refused. A rollback that would
  create a cycle is refused.
- **Tests:** `a_rolled_back_feature_shows_the_marker` — set a rollback and assert the marker is
  drawn. `a_rollback_to_a_missing_feature_is_refused` — assert the error.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Roll Back	Откотитися` in
  `uk.tsv`.
- **Risk:** rollback is a model (MOD-owned) feature. Fallback: the panel shows the marker; the
  model implements rollback.

### UI-TREE-006 — Drag-and-drop reorder of features
- **State:** BLOCKED
- **Depends on:** UI-TREE-001, MOD-003 (undo/redo as a feature-tree operation graph)
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the reorder is a model operation; the panel only drags and drops
  ```
- **Behaviour:** A feature can be dragged to a new position in the tree. The panel sends the
  reorder command; the model implements it (including the undo graph). The drag is shown with a
  highlight of the target position.
- **Refusals:** A reorder that would create a cycle is refused. A reorder of a feature with
  children moves the children too (or is refused, depending on the model's rule).
- **Tests:** `a_drag_reorders_the_feature` — drag a feature and assert the new order. `a_cycle_is_refused`
  — assert the error.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Reorder  Перевпорядкувати`
  in `uk.tsv`.
- **Risk:** reorder is a model (MOD-owned) feature. Fallback: the panel drags and drops; the
  model implements the reorder.

### UI-TREE-007 — Recompute indicator on the tree root
- **State:** BLOCKED
- **Depends on:** UI-TREE-001, MOD-005 (robust rebuild)
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the indicator: a spinning arrow in Tokens::warn while the rebuild runs
  ```
- **Behaviour:** While the model is rebuilding (a parameter changed, a feature was edited), the
  tree root shows a spinning arrow in the warning colour. The indicator is removed when the
  rebuild finishes. The panel reads the rebuild state from the engine.
- **Refusals:** A rebuild that never finishes (a bug) leaves the indicator spinning; the autosave
  recovery handles the crash. A non-finite progress value is not shown.
- **Tests:** `a_rebuilding_model_shows_the_indicator` — set the rebuild state and assert the
  indicator is drawn. `a_finished_rebuild_removes_it` — assert the indicator is gone.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Rebuild	Перебудувати`
  in `uk.tsv`.
- **Risk:** the rebuild state comes from the model (MOD-owned). Fallback: the panel reads the
  engine's state.

### UI-TREE-008 — Feature context menu: edit, suppress, roll back, delete
- **State:** BLOCKED
- **Depends on:** UI-TREE-004, UI-TREE-005
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the context menu: a right-click on a row opens it
  ```
- **Behaviour:** A right-click on a feature row opens a context menu with Edit, Suppress, Roll
  Back, Delete and Properties. Each item runs the corresponding command through `app.run`. The
  menu is drawn in code (no iconography).
- **Refusals:** A delete of a feature that is referenced by later features is refused with a
  message naming the referrer. A delete of the last feature is refused.
- **Tests:** `a_right_click_opens_the_context_menu` — right-click a row and assert the menu
  appears. `a_delete_of_a_referenced_feature_is_refused` — assert the error.
- **Acceptance:** the menu renders headlessly; the Ukrainian catalog gains `Properties  Властивості`
  in `uk.tsv`.
- **Risk:** the delete and edit commands are engine (C-owned) and model (MOD-owned) features.
  Fallback: the panel sends the commands; the engine and model implement them.

### UI-TREE-009 — Tree search and filter
- **State:** BLOCKED
- **Depends on:** UI-TREE-001
- **Files:** `crates/ui-egui/src/panels3/feature_tree.rs` (owner U)
- **Contract:**
  ```rust
  // the search box at the top of the tree; filters by name
  ```
- **Behaviour:** A search box at the top of the tree filters features by name (case-insensitive).
  Non-matching features are hidden; matching features are shown with their ancestors. The search
  is incremental (runs on every keystroke) and is cleared by an X button.
- **Refusals:** A search with no matches shows an empty tree with a "no matches" message, not a
  crash. A search with a non-UTF-8 pattern is impossible (egui's text input is UTF-8).
- **Tests:** `a_search_filters_the_tree` — type a query and assert only matching rows are shown. `a_search_with_no_matches_shows_a_message`
  — assert the message.
- **Acceptance:** the search renders headlessly; the Ukrainian catalog gains `Search for feature
  Пошук елемента` in `uk.tsv`.
- **Risk:** the filter is client-side (no model change needed). Fallback: none.

### UI-TREE-010 — Feature tree state persists across restarts
- **State:** READY
- **Depends on:** UI-TREE-001
- **Files:** `crates/ui-egui/src/panels3/mod.rs`, `crates/ui-egui/src/lib.rs` (owner U)
- **Contract:**
  ```rust
  // the tree state (expanded rows, selection) is persisted in UiState
  ```
- **Behaviour:** The feature tree's expanded/collapsed state and selection are persisted in
  `UiState` and restored on restart. The persistence is through `prefs_json`/`load_prefs`. The
  tree's *content* comes from the model (not persisted here).
- **Refusals:** A corrupted persisted state falls back to the default (all expanded, no
  selection) with a `session.echo` warning.
- **Tests:** `the_tree_state_survives_a_restart` — collapse a row, persist, restore, and assert
  the row is still collapsed. `a_corrupted_state_falls_back` — assert the default.
- **Acceptance:** the persistence works in the headless test; the Ukrainian catalog gains no new
  row (the tree labels are already covered).
- **Risk:** the persisted state grows with the number of features. Fallback: cap the persisted
  state at 1000 rows.

---

# UI-PARAM — the parameters panel

### UI-PARAM-001 — Parameters Manager: one row per parameter
- **State:** BLOCKED
- **Depends on:** SKT-003 (dimensional constraints with expressions)
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  pub fn show(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** The Parameters Manager shows every parameter (name, expression, value) in a
  table. A parameter can be edited inline (a text field for the expression) or deleted. Every edit
  runs `parameters {name, expr}` through `app.run`; the panel only draws and collects input. The
  panel reads the parameters through the engine's `constraints.inspect`.
- **Refusals:** An expression that does not parse is refused with a message naming the parameter
  and the parse error. A parameter name that is empty or duplicated is refused.
- **Tests:** `the_manager_lists_every_parameter` — build a model with two parameters and assert
  the painted text contains both names. `a_bad_expression_is_refused` — type a bad expression and
  assert the error.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Parameters Manager
  Диспетчер параметрів` in `uk.tsv`.
- **Risk:** the parameters come from the model (MOD/SKT-owned). Fallback: the panel reads the
  engine's state; the model provides the parameters.

### UI-PARAM-002 — Parameter types: length, angle, count, boolean
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  pub enum ParamType { Length, Angle, Count, Boolean }
  ```
- **Behaviour:** Each parameter has a type that determines its editor: a length is a number with
  a unit suffix, an angle is a number in the document's angle unit, a count is a positive
  integer, a boolean is a checkbox. The panel shows the appropriate editor for each type.
- **Refusals:** A negative count is refused. A non-finite length is refused. A boolean with a
  non-boolean value is impossible (the editor is a checkbox).
- **Tests:** `every_type_has_its_editor` — build a parameter of each type and assert the
  appropriate editor is shown. `a_negative_count_is_refused` — assert the error.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Parameter Type  Тип
  параметра` in `uk.tsv`.
- **Risk:** the types come from the model (MOD-owned). Fallback: the panel shows the editors; the
  model provides the types.

### UI-PARAM-003 — Expression editor with autocomplete
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the expression editor: a text field with autocomplete for parameter names and functions
  ```
- **Behaviour:** The expression editor is a text field with autocomplete: typing a letter shows
  a list of matching parameter names and built-in functions (sin, cos, sqrt, …). The autocomplete
  is drawn as an overlay; a Tab or Enter accepts the highlighted suggestion.
- **Refusals:** An empty expression is refused. An expression with unbalanced parentheses is
  refused with a message naming the parameter.
- **Tests:** `the_autocomplete_suggests_parameter_names` — type a letter and assert the
  suggestions appear. `a_tab_accepts_the_highlighted_suggestion` — assert the field's content.
- **Acceptance:** the editor renders headlessly; the Ukrainian catalog gains `Expression  Вираз` in
  `uk.tsv`.
- **Risk:** the autocomplete list comes from the model (MOD-owned). Fallback: the panel collects
  the suggestions; the model provides them.

### UI-PARAM-004 — Live rebuild on parameter change
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001, MOD-005 (robust rebuild)
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the rebuild is triggered by the engine command; the panel only shows the result
  ```
- **Behaviour:** When a parameter changes, the model rebuilds automatically (live). The panel
  shows the rebuild progress (a spinner in the warning colour) and the new values. The rebuild is
  debounced (e.g. 300 ms after the last keystroke) so a fast typer does not trigger a rebuild per
  keystroke.
- **Refusals:** A rebuild that fails shows the error on the affected rows and keeps the old
  values (the model's partial rebuild). A rebuild that is refused (the model is busy) is queued.
- **Tests:** `a_parameter_change_triggers_a_rebuild` — change a parameter and assert the values
  update. `a_failed_rebuild_keeps_the_old_values` — assert the old values are shown with an
  error.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Live Rebuild  Жива
  перебудова` in `uk.tsv`.
- **Risk:** the rebuild is a model (MOD-owned) feature. Fallback: the panel triggers the rebuild;
  the model implements it.

### UI-PARAM-005 — Constraint bars over the canvas
- **State:** BLOCKED
- **Depends on:** SKT-002 (constraint types)
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  pub struct Glyph { pub id: u32, pub kind: String, pub dimensional: bool, pub handles: Vec<String>, pub anchors: Vec<[f64; 2]>, pub name: String, pub expr: String, pub value: Option<f64>, pub satisfied: bool }
  ```
- **Behaviour:** Geometric constraint bars are drawn over the canvas for each constrained object.
  Each bar shows the constraint's kind, its handles (editable points), and whether it is
  satisfied. The bars are cached per drawing revision (`parametric::Cache`) because
  `constraints.inspect` is not free on big drawings.
- **Refusals:** A constraint whose handles are off-screen is not drawn (but is counted in the
  panel). A constraint with a non-finite anchor is sanitised.
- **Tests:** `a_constrained_object_shows_a_bar` — constrain an object and assert the bar is drawn.
  `an_off_screen_bar_is_not_drawn` — assert it is counted but not drawn.
- **Acceptance:** the bars render headlessly; the Ukrainian catalog gains `Constraint Bars  Панелі
  обмежень` in `uk.tsv`.
- **Risk:** the constraints come from the sketch solver (SKT-owned). Fallback: the panel reads
  the engine's state; the solver provides the constraints.

### UI-PARAM-006 — Degrees of freedom display
- **State:** BLOCKED
- **Depends on:** UI-PARAM-005, SKT-004 (solver)
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  pub struct Dof { pub dof: Option<usize>, pub status: String, pub objects: Vec<ObjectDof> }
  pub struct ObjectDof { pub handle: String, pub dof: Option<usize>, pub status: String }
  ```
- **Behaviour:** The degrees-of-freedom summary is drawn on the canvas: a fully-constrained
  drawing shows `Degrees of freedom: 0` in green, an under-constrained one shows the count in
  blue, an over-constrained one shows `Over-constrained` in orange. The summary comes from
  `constraints.dof`.
- **Refusals:** A DOF count that is not finite is not drawn (the solver's answer is always
  finite). A status that is not one of `under`, `fully`, `over`, `unknown` is drawn as `unknown`.
- **Tests:** `a_fully_constrained_drawing_shows_zero` — assert the green zero. `an_over_constrained_drawing_shows_the_warning`
  — assert the orange warning.
- **Acceptance:** the summary renders headlessly; the Ukrainian catalog gains `Degrees of Freedom
  Ступені вільності` in `uk.tsv`.
- **Risk:** the DOF comes from the solver (SKT-owned). Fallback: the panel reads the engine's
  state.

### UI-PARAM-007 — Inline edit of a dimensional constraint
- **State:** BLOCKED
- **Depends on:** UI-PARAM-005
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the editing state: (name, buffer, screen pos) — a text field over the constraint bar
  ```
- **Behaviour:** A dimensional constraint can be edited inline: clicking its bar opens a text
  field over it with the current expression. Typing a new expression and pressing Enter runs
  `parameters {name, expr}`. The edit is shown in `parametric::Cache::editing`.
- **Refusals:** An empty expression is refused. An expression that does not parse is refused with
  a message naming the constraint. An Escape cancels the edit.
- **Tests:** `a_click_opens_the_editor` — click a bar and assert the editor appears. `an_enter_runs_the_command`
  — type an expression and assert the command ran.
- **Acceptance:** the editor renders headlessly; the Ukrainian catalog gains `Edit Constraint
  Редагувати обмеження` in `uk.tsv`.
- **Risk:** the edit runs an engine command (C-owned). Fallback: the panel collects the input; the
  engine runs the command.

### UI-PARAM-008 — Parameter import and export
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the export: a CSV or JSON of the parameters; the import: the same format
  ```
- **Behaviour:** The Parameters Manager can export the parameters to a file (CSV or JSON) and
  import them from a file. The export is written through the engine's file commands; the import
  runs the `parameters` command for each imported parameter. The format is documented in the
  panel's help text.
- **Refusals:** An import whose format is not recognised is refused with a message naming the
  expected format. An import whose expression does not parse is refused per parameter.
- **Tests:** `an_export_round_trips` — export, import, and assert the parameters are the same. `a_bad_format_is_refused`
  — assert the error.
- **Acceptance:** the import/export renders headlessly; the Ukrainian catalog gains `Export Parameters
  Експортувати параметри` in `uk.tsv`.
- **Risk:** the file format is new. Fallback: the format is JSON (matching the `.ccm` style) so no
  new parser is needed.

### UI-PARAM-009 — Unit display in the document's unit
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001, DAT-042 (document unit)
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the display uses format_in(value_mm, unit, format) from DAT-045
  ```
- **Behaviour:** Parameter values are displayed in the document's unit (DAT-042), not in
  millimetres. The display uses `format_in` which converts the millimetre value to the document's
  unit before formatting. The unit suffix (mm, in, ft) is shown after the value.
- **Refusals:** A value that is not finite is shown as `0` (the existing `format_linear`
  behaviour). A unit that is not in the document's unit system is shown in millimetres with a
  warning.
- **Tests:** `a_length_shows_in_the_document_unit` — set the document to inches and assert a
  25.4 mm parameter shows `1"`. `a_non_finite_value_shows_zero` — assert `0`.
- **Acceptance:** the display renders headlessly; the Ukrainian catalog gains `Unit  Одиниця` in
  `uk.tsv`.
- **Risk:** the unit conversion is DAT-owned. Fallback: the panel calls `format_in`; the unit
  system provides the conversion.

### UI-PARAM-010 — Parameter groups and folders
- **State:** BLOCKED
- **Depends on:** UI-PARAM-001
- **Files:** `crates/ui-egui/src/parametric.rs` (owner U)
- **Contract:**
  ```rust
  // the group: a collapsible header with its parameters indented below
  ```
- **Behaviour:** Parameters can be grouped into folders (e.g. `Dimensions`, `Tolerances`,
  `Mass properties`). The group is a collapsible header; its parameters are indented below. The
  grouping is a model feature (MOD-owned); the panel only draws the groups.
- **Refusals:** An empty group shows the header with no children. A group name that is duplicated
  is shown as-is (the model handles uniqueness).
- **Tests:** `a_group_is_collapsible` — click a group header and assert its children are hidden. `an_empty_group_shows_only_the_header`
  — assert no children.
- **Acceptance:** the groups render headlessly; the Ukrainian catalog gains `Parameter Group  Група
  параметрів` in `uk.tsv`.
- **Risk:** the grouping is a model (MOD-owned) feature. Fallback: the panel draws the groups; the
  model provides them.

---

# UI-SKT — the sketch editor

### UI-SKT-001 — Sketch editor: a canvas for sketch geometry
- **State:** BLOCKED
- **Depends on:** SKT-001 (profile model)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs`, `crates/ui-egui/src/canvas/rebuild.rs` (owner U)
- **Contract:**
  ```rust
  pub fn show(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** The sketch editor is a canvas where the user draws sketch geometry (lines, arcs,
  circles) on a sketch plane. The geometry is derived from constraints (SKT-001), not stored as
  points. The panel sends sketch commands through `app.run`; the model owns the geometry.
- **Refusals:** A sketch command that fails (the geometry is invalid) is echoed to the command
  line. A sketch with no geometry shows an empty canvas.
- **Tests:** `the_editor_shows_the_sketch_plane` — open the editor and assert the plane is drawn. `a_sketch_command_runs`
  — draw a line and assert it appears.
- **Acceptance:** the editor renders headlessly; the Ukrainian catalog gains `Sketch Editor  Редактор
  ескізу` in `uk.tsv`.
- **Risk:** the sketch geometry comes from the solver (SKT-owned). Fallback: the panel sends
  commands; the model owns the geometry.

### UI-SKT-002 — Sketch constraints: display and toggle
- **State:** BLOCKED
- **Depends on:** UI-SKT-001, SKT-002 (constraint types)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the constraint display: a list of the sketch's constraints with their satisfaction
  ```
- **Behaviour:** The sketch editor shows the sketch's constraints (coincident, concentric,
  tangent, parallel, perpendicular, horizontal, vertical, equal, symmetric, fix) with their
  satisfaction state. A constraint can be toggled on/off. The display reads from the solver's
  output.
- **Refusals:** A constraint that the solver does not recognise is shown as `Unknown` and cannot
  be toggled. A toggle that would over-constrain the sketch is refused with a message.
- **Tests:** `the_constraints_are_listed` — constrain a sketch and assert the constraints appear. `a_toggle_is_refused_when_over_constrained`
  — assert the error.
- **Acceptance:** the display renders headlessly; the Ukrainian catalog gains `Sketch Constraints
  Обмеження ескізу` in `uk.tsv`.
- **Risk:** the constraints come from the solver (SKT-owned). Fallback: the panel reads the
  solver's output.

### UI-SKT-003 — Dimensional constraints in the sketch
- **State:** BLOCKED
- **Depends on:** UI-SKT-002, SKT-003 (dimensional constraints)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // a dimensional constraint: a value with a unit, editable inline
  ```
- **Behaviour:** Dimensional constraints (distance, angle, radius, diameter) are shown in the
  sketch with their value and unit. They can be edited inline (a text field over the dimension).
  The value is bound to a parameter expression (SKT-003).
- **Refusals:** A negative distance is refused. A non-finite value is refused. An expression that
  does not parse is refused.
- **Tests:** `a_dimensional_constraint_shows_its_value` — assert the value appears. `an_edit_runs_the_command`
  — type a new value and assert the command ran.
- **Acceptance:** the dimensions render headlessly; the Ukrainian catalog gains `Dimensional
  Constraint  Розмірне обмеження` in `uk.tsv`.
- **Risk:** the dimensional constraints come from the solver (SKT-owned). Fallback: the panel
  reads the solver's output.

### UI-SKT-004 — Sketch solving feedback: under, over, fully constrained
- **State:** BLOCKED
- **Depends on:** UI-SKT-002, SKT-004 (solver)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the solving feedback: a status line showing the constraint state
  ```
- **Behaviour:** The sketch editor shows the constraint state (under-constrained, fully
  constrained, over-constrained) as a status line. The state comes from the solver's output. The
  line is coloured: blue for under, green for fully, orange for over.
- **Refusals:** A state that is not one of the three is shown as `Unknown` in the dim colour. A
  non-finite DOF count is not shown.
- **Tests:** `an_under_constrained_sketch_shows_blue` — assert the blue status. `a_fully_constrained_sketch_shows_green`
  — assert the green status.
- **Acceptance:** the status line renders headlessly; the Ukrainian catalog gains `Solving Feedback
  Зворотний зв'язок розв'язування` in `uk.tsv`.
- **Risk:** the state comes from the solver (SKT-owned). Fallback: the panel reads the solver's
  output.

### UI-SKT-005 — Sketch plane selection
- **State:** BLOCKED
- **Depends on:** UI-SKT-001, SKT-006 (3D sketches)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the plane selection: a picker for the sketch plane (XY, YZ, ZX, or a face)
  ```
- **Behaviour:** The sketch editor starts with a plane selection: the user picks a plane (XY, YZ,
  ZX, or a face of the model). The plane is shown as a grid on the canvas. The selection is sent
  to the model as part of the sketch command.
- **Refusals:** A plane that is not on the model (a face that does not exist) is refused. A plane
  that is degenerate (zero normal) is refused.
- **Tests:** `the_plane_picker_shows_the_options` — assert the options appear. `a_face_that_does_not_exist_is_refused`
  — assert the error.
- **Acceptance:** the picker renders headlessly; the Ukrainian catalog gains `Sketch Plane  Площина
  ескізу` in `uk.tsv`.
- **Risk:** the face picking needs the model's geometry (MOD-owned). Fallback: the panel sends
  the plane choice; the model resolves it.

### UI-SKT-006 — Sketch entities: lines, arcs, circles, splines
- **State:** BLOCKED
- **Depends on:** UI-SKT-001
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the entity toolbar: buttons for line, arc, circle, spline
  ```
- **Behaviour:** The sketch editor has a toolbar with buttons for each entity type (line, arc,
  circle, spline). Selecting a tool starts the corresponding sketch command. The toolbar is drawn
  in code (no iconography).
- **Refusals:** A tool that the sketch does not support (a spline when the solver has no splines)
  is refused with a message. A tool whose command fails echoes the error.
- **Tests:** `every_entity_tool_is_offered` — assert the toolbar lists all four tools. `an_unsupported_tool_is_refused`
  — assert the error.
- **Acceptance:** the toolbar renders headlessly; the Ukrainian catalog gains `Sketch Entity  Об'єкт
  ескізу` in `uk.tsv`.
- **Risk:** the entity support comes from the solver (SKT-owned). Fallback: the panel offers the
  tools; the solver supports them over time.

### UI-SKT-007 — Sketch diagnostics: redundant and conflicting constraints
- **State:** BLOCKED
- **Depends on:** UI-SKT-002, SKT-005 (diagnostics)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the diagnostics: a list of redundant and conflicting constraints by name
  ```
- **Behaviour:** The sketch editor shows diagnostics from the solver: which constraints are
  redundant and which conflict, by name. The diagnostics are shown in a panel below the canvas.
  The panel reads the diagnostics from the solver's output.
- **Refusals:** A diagnostic that names a constraint that no longer exists is shown as `Unknown`.
  An empty diagnostics list shows a `No diagnostics` message.
- **Tests:** `a_redundant_constraint_is_listed` — create a redundant constraint and assert it
  appears. `a_conflicting_constraint_is_listed` — assert it appears.
- **Acceptance:** the diagnostics render headlessly; the Ukrainian catalog gains `Diagnostics
  Діагностика` in `uk.tsv`.
- **Risk:** the diagnostics come from the solver (SKT-owned). Fallback: the panel reads the
  solver's output.

### UI-SKT-008 — 3D sketch on an arbitrary plane
- **State:** BLOCKED
- **Depends on:** UI-SKT-005, SKT-006 (3D sketches)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the 3D sketch: the plane is a 3D plane, not just XY
  ```
- **Behaviour:** A 3D sketch is drawn on an arbitrary plane in 3D space (not just the XY plane).
  The plane can be rotated and translated. The sketch commands carry the plane's transform. The
  panel sends the transform as part of the sketch command.
- **Refusals:** A plane with a non-finite transform is refused. A plane that is degenerate (zero
  normal) is refused.
- **Tests:** `a_3d_sketch_is_drawn_on_its_plane` — set a plane and assert the grid is drawn. `a_non_finite_transform_is_refused`
  — assert the error.
- **Acceptance:** the 3D sketch renders headlessly; the Ukrainian catalog gains `3D Sketch  3D-ескіз`
  in `uk.tsv`.
- **Risk:** the 3D plane comes from the model (MOD-owned). Fallback: the panel sends the
  transform; the model resolves the plane.

### UI-SKT-009 — Sketch profile: closed region detection
- **State:** BLOCKED
- **Depends on:** UI-SKT-001, MOD-001 (sketch-backed features)
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the profile: a closed region of the sketch, used for extrude/revolve
  ```
- **Behaviour:** The sketch editor detects closed regions (profiles) in the sketch. A profile is a
  closed loop of sketch entities. The profile is highlighted and can be selected for an
  extrude/revolve command. The detection is a model feature (MOD-owned); the panel only shows the
  profiles.
- **Refusals:** A profile that is not closed (a gap in the loop) is highlighted as open and
  cannot be selected. A profile with zero area is refused.
- **Tests:** `a_closed_profile_is_highlighted` — draw a closed loop and assert it is highlighted. `an_open_profile_is_refused`
  — assert the error.
- **Acceptance:** the profiles render headlessly; the Ukrainian catalog gains `Sketch Profile  Профіль
  ескізу` in `uk.tsv`.
- **Risk:** the profile detection comes from the model (MOD-owned). Fallback: the panel shows the
  profiles; the model detects them.

### UI-SKT-010 — Sketch grid and snap
- **State:** READY
- **Depends on:** UI-SKT-001
- **Files:** `crates/ui-egui/src/canvas/blockedit.rs` (owner U)
- **Contract:**
  ```rust
  // the grid: drawn with Tokens::grid_minor and Tokens::grid_major
  ```
- **Behaviour:** The sketch editor draws a grid (minor and major lines) using the theme's grid
  colours. The grid spacing is configurable (e.g. 1, 5, 10 units). A snap mode constrains
  drawn points to the grid. The grid and snap are drawn in the canvas, not stored.
- **Refusals:** A grid spacing that is zero or negative is refused. A non-finite spacing is
  refused. A snap that is off shows the grid but does not constrain.
- **Tests:** `the_grid_is_drawn_with_the_theme_colours` — assert the grid lines use
  `grid_minor` and `grid_major`. `a_zero_spacing_is_refused` — assert the error.
- **Acceptance:** the grid renders headlessly; the Ukrainian catalog gains `Sketch Grid  Сітка
  ескізу` in `uk.tsv`.
- **Risk:** the grid is 2D-shared. Fallback: a 3D-specific grid is gated behind a 3D check.

---

# UI-ASM — the assembly browser

### UI-ASM-001 — Assembly browser: one row per component
- **State:** BLOCKED
- **Depends on:** ASM-001 (component/occurrence model)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  pub struct ComponentRow { pub name: String, pub state: ComponentState, pub children: Vec<usize> }
  pub fn show(app: &mut CadApp, ui: &mut egui::Ui);
  ```
- **Behaviour:** The assembly browser shows the assembly's components in a tree. Each component
  has a name, a state (built, pending, failed, blocked) and children (sub-components). The panel
  reads the assembly's component list through the engine's state.
- **Refusals:** A component whose name is not valid UTF-8 is displayed lossily. An empty assembly
  shows an empty panel.
- **Tests:** `the_browser_lists_every_component` — build an assembly with three components and
  assert the painted text contains all three names. `an_empty_assembly_shows_an_empty_panel` —
  assert no rows.
- **Acceptance:** the panel renders headlessly; the Ukrainian catalog gains `Assembly Browser  Оглядач
  збірки` in `uk.tsv`.
- **Risk:** the assembly model (ASM crate) does not exist yet. Fallback: the panel reads the
  engine's state; the ASM crate provides the component list when it lands.

### UI-ASM-002 — Joint display: type and state
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, ASM-002 (joint types)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the joint display: the joint type (rigid, revolute, slider, …) and its state
  ```
- **Behaviour:** Each component's joints are shown with their type (rigid, revolute, slider,
  cylindrical, spherical, planar, distance) and their state (satisfied, violated). The display
  reads from the assembly's joint list.
- **Refusals:** A joint type that is not recognised is shown as `Unknown`. A joint with a
  non-finite value is shown as `Unknown`.
- **Tests:** `a_joint_shows_its_type` — create a revolute joint and assert the type appears. `a_violated_joint_is_highlighted`
  — assert the warning colour.
- **Acceptance:** the display renders headlessly; the Ukrainian catalog gains `Joint  З'єднання` in
  `uk.tsv`.
- **Risk:** the joints come from the assembly model (ASM-owned). Fallback: the panel reads the
  assembly's state.

### UI-ASM-003 — Bill of materials panel
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, ASM-006 (BOM export)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the BOM: a table of component names, quantities, and materials
  ```
- **Behaviour:** The BOM panel shows a table of every component in the assembly with its name,
  quantity, and material. The table is sortable by any column. The BOM can be exported to a file
  (CSV) through the engine's file commands.
- **Refusals:** An export whose path is not writable is refused with a message naming the path. A
  BOM with no components shows an empty table.
- **Tests:** `the_bom_lists_every_component` — build an assembly and assert the table lists all
  components. `an_export_writes_a_file` — assert the file is written.
- **Acceptance:** the BOM renders headlessly; the Ukrainian catalog gains `Bill of Materials
  Специфікація` in `uk.tsv`.
- **Risk:** the BOM export is an ASM-owned feature. Fallback: the panel shows the table; the ASM
  crate implements the export.

### UI-ASM-004 — Occurrence selection in the viewport
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, UI-VP-003 (picking)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the selection: a click in the viewport selects the component
  ```
- **Behaviour:** Clicking a component in the viewport selects it (highlights it and shows its
  properties). The selection is sent to the assembly model. The panel shows the selected
  component's name in the browser.
- **Refusals:** A click that hits nothing clears the selection. A click that hits a component
  that is not in the assembly is ignored.
- **Tests:** `a_click_selects_the_component` — click a component and assert it is selected. `a_click_on_empty_space_clears_the_selection`
  — assert the selection is cleared.
- **Acceptance:** the selection renders headlessly; the Ukrainian catalog gains `Select Component
  Вибрати компонент` in `uk.tsv`.
- **Risk:** the picking is REN-owned and the selection is ASM-owned. Fallback: the panel sends
  the pick request; the ASM model handles the selection.

### UI-ASM-005 — Assembly constraints display
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, ASM-003 (assembly constraint solver)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the constraints: a list of assembly constraints and their satisfaction
  ```
- **Behaviour:** The assembly's constraints (mate, angle, distance, etc.) are shown with their
  satisfaction state. The display reads from the assembly's constraint solver.
- **Refusals:** A constraint that the solver does not recognise is shown as `Unknown`. A
  constraint with a non-finite value is shown as `Unknown`.
- **Tests:** `a_constraint_shows_its_state` — create a mate constraint and assert its state
  appears. `a_violated_constraint_is_highlighted` — assert the warning colour.
- **Acceptance:** the display renders headlessly; the Ukrainian catalog gains `Assembly Constraint
  Обмеження збірки` in `uk.tsv`.
- **Risk:** the constraints come from the assembly solver (ASM-owned). Fallback: the panel reads
  the assembly's state.

### UI-ASM-006 — Interference display
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, ASM-004 (interference detection)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the interference: a list of interfering component pairs and their overlap volume
  ```
- **Behaviour:** The interference panel shows pairs of components that interfere (overlap) and
  their overlap volume. The display reads from the assembly's interference detection.
- **Refusals:** An interference detection that fails (the kernel cannot compute the overlap) is
  reported as `Unknown`. A non-finite overlap volume is shown as `Unknown`.
- **Tests:** `an_interfering_pair_is_listed` — create an interference and assert it appears. `a_failed_detection_is_reported`
  — assert the error.
- **Acceptance:** the display renders headlessly; the Ukrainian catalog gains `Interference
  Перетин` in `uk.tsv`.
- **Risk:** the interference detection is ASM-owned. Fallback: the panel reads the assembly's
  state.

### UI-ASM-007 — Exploded view slider
- **State:** BLOCKED
- **Depends on:** UI-ASM-001, ASM-005 (exploded views)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the exploded view: a slider that separates the components along their assembly axes
  ```
- **Behaviour:** An exploded view separates the components along their assembly axes. The panel
  shows a slider (0 = assembled, 1 = fully exploded). The slider's value is sent to the assembly
  model, which updates the component positions.
- **Refusals:** A slider value outside 0..=1 is clamped. An exploded view that would cause
  components to coincide is not prevented (the kernel handles collisions).
- **Tests:** `the_slider_separates_the_components` — move the slider and assert the positions
  change. `a_value_past_one_is_clamped` — assert the clamp.
- **Acceptance:** the slider renders headlessly; the Ukrainian catalog gains `Exploded View  Розгорнутий
  вид` in `uk.tsv`.
- **Risk:** the exploded view is ASM-owned. Fallback: the panel shows the slider; the ASM model
  implements the separation.

### UI-ASM-008 — Motion study animation
- **State:** BLOCKED
- **Depends on:** UI-ASM-007, ASM-005 (motion studies)
- **Files:** `crates/ui-egui/src/panels3/mod.rs` (owner U)
- **Contract:**
  ```rust
  // the motion study: an animation of the assembly's motion
  ```
- **Behaviour:** A motion study animates the assembly's motion (e.g. a revolute joint rotating).
  The panel shows play/pause/stop controls and a timeline. The animation is driven by the
  assembly's motion solver.
- **Refusals:** A motion study that has no animated joints shows a `No motion` message. A motion
  study that fails (the solver diverges) is stopped and reported.
- **Tests:** `a_motion_study_plays` — create a revolute joint and assert the animation plays. `a_study_with_no_joints_shows_a_message`
  — assert the message.
- **Acceptance:** the animation renders headlessly; the Ukrainian catalog gains `Motion Study  Рухове
  дослідження` in `uk.tsv`.
- **Risk:** the motion study is ASM-owned. Fallback: the panel shows the controls; the ASM model
  drives the animation.

---

# UI-I18N — internationalization

### UI-I18N-001 — Onboard a new language: the checklist
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/i18n/mod.rs`, `crates/ui-egui/src/i18n/<lang>.tsv` (owner U)
- **Contract:**
  ```rust
  // 1. Add a Preference variant
  // 2. Add a LANGUAGES row with the plural rule and form count
  // 3. Create the .tsv catalog
  // 4. Add the font coverage test
  // 5. Add the live-switch test
  ```
- **Behaviour:** To add a language: (1) add a variant to `Preference`, (2) add a row to
  `LANGUAGES` with the language's name, source file, plural function and plural form count,
  (3) create the `.tsv` catalog with the header comments, (4) add a font coverage test (the
  default fonts must cover every letter in the catalog), (5) add a live-switch test (switching to
  the language translates the shell without mutating the drawing or automation). The Ukrainian
  catalog (`uk.tsv`) is the reference: every new language's gaps are reported, not enforced.
- **Refusals:** A language whose plural rule is wrong (e.g. a language with one form but two
  plural functions) fails the catalog test. A language whose font coverage is incomplete falls
  back to English with a status message (never boxes).
- **Tests:** `the_new_language_is_listed_in_the_picker` — assert `Preference::ALL` contains it. `the_new_language_switches_live`
  — switch and assert the shell translates. `the_new_language_needs_a_font` — assert the font
  coverage test passes or the fallback works.
- **Acceptance:** the new language appears in the language picker; every existing UI task's
  Ukrainian row requirement extends to the new language when its catalog is complete; the
  Ukrainian catalog gains `Language	Мова` in `uk.tsv`.
- **Risk:** a new language with a right-to-left script (like Arabic) needs the RTL layout.
  Fallback: Arabic already has a catalog; a new RTL language follows its pattern.

### UI-I18N-002 — Plural forms: the per-language rules
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/i18n/mod.rs` (owner U)
- **Contract:**
  ```rust
  pub fn plural_uk(n: u64) -> usize;   // one (1, 21), few (2–4), many (0, 5–20, 25…)
  pub fn plural_pt(n: u64) -> usize;   // one (0, 1), other
  pub fn plural_cs(n: u64) -> usize;   // one (1), few (2–4), other
  pub fn plural_arabic(n: u64) -> usize; // zero, one, two, few, many, other
  pub fn plural_en(n: u64) -> usize;   // one (1), other
  pub fn plural_ko(n: u64) -> usize;   // always 0
  ```
- **Behaviour:** Each language has its own plural rule, expressed as a `fn(u64) -> usize`. The
  `count` function looks up the `@plural` row in the catalog, splits it on `|`, and picks the
  form the rule selects. A language with no `@plural` row falls back to the English rule.
- **Refusals:** A `@plural` row with fewer forms than `plural_forms` is a catalog error (the test
  catches it). A non-fatal `n` (impossible from `u64`) is not a case.
- **Tests:** `every_plural_rule_is_tested` — for each language, assert the rule's output for
  representative `n`. `a_missing_plural_row_falls_back_to_english` — assert the fallback.
- **Acceptance:** every language's plural rule is tested; the Ukrainian catalog gains
  `{n} contributor	Учасників: {n}` (or the exact plural forms) in `uk.tsv`.
- **Risk:** a language with a complex rule (e.g. Arabic's six forms) is hard to get right.
  Fallback: the rule is a pure function and can be tested exhaustively for small `n`.

### UI-I18N-003 — Right-to-left layout for Arabic
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/mod.rs`, `crates/ui-egui/src/lib.rs` (owner U)
- **Contract:**
  ```rust
  // the RTL: egui's layout direction is set to right-to-left for Arabic
  ```
- **Behaviour:** Arabic is right-to-left: the shell's layout direction is set to RTL when the
  language is Arabic. egui keeps its left-to-right run order (the shell's own layout is LTR, but
  the text within it is RTL). An installed Arabic font (`theme::arabic_font`) covers the glyphs.
- **Refusals:** A language that is not RTL keeps the LTR layout. An Arabic font that is not
  installed falls back to English with a status message.
- **Tests:** `the_arabic_interface_is_rtl` — switch to Arabic and assert the layout direction. `the_arabic_font_fallback_works`
  — assert the fallback.
- **Acceptance:** the Arabic interface renders headlessly; the Ukrainian catalog gains `Мова  Мова`
  (no new row needed — the language picker already has the row).
- **Risk:** the RTL layout breaks the 2D canvas. Fallback: the canvas is always LTR (the
  drawing is coordinate-based, not text-based).

### UI-I18N-004 — Font coverage: every catalog letter is in the default fonts
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/tests.rs` (owner U)
- **Contract:**
  ```rust
  // for each language, the default fonts (egui's bundled + the embedded craft-fonts) must cover
  // every non-ASCII letter in the catalog
  ```
- **Behaviour:** For each language, a test collects every non-ASCII character in the catalog and
  asserts the default fonts (egui's bundled fonts plus the embedded craft-fonts) have a glyph for
  it. The test runs in the headless context (no system fonts). A language whose letters are not
  covered falls back to English with a status message.
- **Refusals:** A letter that is not in any font is a test failure (the catalog must not contain
  letters the product cannot render). A letter that is covered by a system font but not by the
  default fonts is a failure on web (where there are no system fonts).
- **Tests:** `default_fonts_cover_<language>_letters` — for each language, assert every
  non-ASCII letter has a glyph. `a_missing_glyph_fails_the_test` — assert the failure.
- **Acceptance:** every language's font coverage test passes; the Ukrainian catalog gains no new
  row (the font coverage is a test, not a UI string).
- **Risk:** a language with a large character set (e.g. Chinese) needs a CJK font. Fallback:
  Chinese already falls back to English without an installed CJK font; the test asserts the
  fallback.

### UI-I18N-005 — Translation coverage tooling: report what is missing
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/tests.rs` (owner U)
- **Contract:**
  ```rust
  // the coverage test: for each language, list the strings that fall back to English
  ```
- **Behaviour:** The coverage test (`translations_cover_commands_menus_tools_and_snap_labels`)
  walks every `tl!`/`tf!` literal in the UI crate and asserts each has a row in the required
  language's catalog. For non-required languages, the missing strings are reported (printed) but
  do not fail the test. The Ukrainian catalog is required: every literal must have a row.
- **Refusals:** A missing row in a required language fails the test. A missing row in a reported
  language prints a message. A literal that is not a string (e.g. a format string with a
  non-literal argument) is skipped.
- **Tests:** `the_ukrainian_catalog_covers_every_literal` — assert no missing rows. `a_missing_row_in_a_reported_language_is_printed`
  — assert the message is printed.
- **Acceptance:** the coverage test is part of `cargo xtask ci`; the Ukrainian catalog gains no
  new row (the coverage is a test, not a UI string).
- **Risk:** a new `tl!` literal added without a Ukrainian row fails the test. Fallback: the
  test prints the missing literal's file and line.

### UI-I18N-006 — Live language switch without mutating the drawing
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/mod.rs`, `crates/ui-egui/src/lib.rs` (owner U)
- **Contract:**
  ```rust
  pub fn with_language<R>(language: &'static str, draw: impl FnOnce() -> R) -> R;
  ```
- **Behaviour:** The interface language can be switched live (without a restart). The switch
  changes the shell's language (menus, dialogs, panels) but does not mutate the drawing, the
  command line state, or the automation (the control channel's command list and the menu tree
  keep their original labels and IDs). The `ui.language` command is the entry point.
- **Refusals:** A language code that is not in `Preference` is refused with a message naming the
  valid codes. A language whose font is not available falls back to English with a status
  message.
- **Tests:** `a_live_switch_translates_the_shell` — switch and assert the shell translates. `a_live_switch_does_not_mutate_the_drawing`
  — assert the drawing is unchanged. `a_live_switch_keeps_the_command_list` — assert the control
  channel's command list is unchanged.
- **Acceptance:** the live switch works in the headless control-channel test; the Ukrainian
  catalog gains `Interface Language	Мова інтерфейсу` in `uk.tsv`.
- **Risk:** a language switch during a command's interactive prompt. Fallback: the switch
  changes the shell's language; the command's prompts are in the language they started in.

### UI-I18N-007 — Context-specific translations: menu headings vs object labels
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/mod.rs` (owner U)
- **Contract:**
  ```rust
  pub fn tr_menu<'a>(language: &str, source: &'a str) -> &'a str;
  // the "menu" context: a menu heading can differ from the object label
  ```
- **Behaviour:** Menu headings can differ from command or object labels (e.g. Czech `Kótování`
  (menu) vs `Kóta` (object)). The `tr_menu` function looks up the `menu` context first, then
  falls back to the ordinary `tr`. A catalog without a `menu` entry retains the ordinary
  translation.
- **Refusals:** A `menu` entry that is empty falls back to the ordinary translation. A `menu`
  entry with a different set of placeholders than the ordinary one is a catalog error.
- **Tests:** `a_menu_heading_can_differ_from_the_label` — assert `tr_menu("cs", "Dimension") ==
  "Kótování"` and `tr("cs", "Dimension") == "Kóta"`. `a_catalog_without_a_menu_entry_falls_back`
  — assert the ordinary translation.
- **Acceptance:** the `menu` context works for every language that needs it; the Ukrainian
  catalog gains `menu	Dimension	Кótова` (or the Ukrainian menu heading) in `uk.tsv`.
- **Risk:** a language that needs a `menu` entry but does not have one. Fallback: the fallback
  to the ordinary translation is always correct.

### UI-I18N-008 — Format strings: placeholders and user data
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/mod.rs` (owner U)
- **Contract:**
  ```rust
  pub fn format(source: &str, values: &[(&str, String)]) -> String;
  // inserted values are never looked up or substituted again
  ```
- **Behaviour:** Named placeholders allow translations to reorder values. The `format` function
  replaces `{key}` with the corresponding value. Inserted values are never looked up or
  substituted again, so a user's name containing `{n}` is preserved verbatim. A placeholder with
  no matching value is left as-is.
- **Refusals:** A placeholder with no matching value is left in the output (not an error). A
  value that contains `{` is not re-scanned.
- **Tests:** `a_placeholder_is_replaced` — assert `format("Height: {height}", &[("height", "10")])
  == "Height: 10"`. `a_value_with_a_placeholder_is_not_rescanned` — assert the value is preserved.
- **Acceptance:** the `format` function works for every language; the Ukrainian catalog gains
  `Height: {height}	Висота: {height}` in `uk.tsv`.
- **Risk:** a translation that reorders placeholders must be tested per language. Fallback: the
  catalog test asserts the placeholder set matches.

### UI-I18N-009 — Catalog validation: duplicates, malformed rows, empty values
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/i18n/mod.rs` (owner U)
- **Contract:**
  ```rust
  fn parse(source: &str) -> (Catalog<'_>, Vec<String>);
  // malformed rows are ignored at runtime and diagnosed by the catalog tests
  ```
- **Behaviour:** The `parse` function reads a `.tsv` line by line. A line that is empty or starts
  with `#` is a comment. A line must have exactly three tab-separated columns (context, source,
  translation) with non-empty source and translation. Duplicate keys are reported as errors. A
  malformed row is ignored at runtime and diagnosed by the test.
- **Refusals:** A row with fewer than three columns is ignored. A row with an empty source or
  translation is ignored. A duplicate key overwrites the previous value and is reported.
- **Tests:** `a_duplicate_key_is_reported` — assert the error. `a_malformed_row_is_ignored` —
  assert the catalog has no entry for it. `an_empty_value_is_ignored` — assert no entry.
- **Acceptance:** the catalog test runs in `cargo xtask ci`; the Ukrainian catalog gains no new
  row (the validation is a test, not a UI string).
- **Risk:** a catalog with a duplicate key silently overwrites. Fallback: the test catches it.

### UI-I18N-010 — Language picker: the preference dialog
- **State:** READY
- **Depends on:** UI-I18N-001
- **Files:** `crates/ui-egui/src/dialogs.rs`, `crates/ui-egui/src/options/dialog.rs` (owner U)
- **Contract:**
  ```rust
  // the picker: a dropdown or a dialog listing Preference::ALL
  ```
- **Behaviour:** The language picker shows every `Preference` variant with its native name
  (e.g. `Українська`, `English`, `العربية`). A language whose font is not available is shown
  dimmed with a note saying why. The picker's selection is saved through `prefs_json`/`load_prefs`
  and restored on restart.
- **Refusals:** A language whose font is not available can still be selected (it falls back to
  English with a status message). An obsolete saved preference (a code that no longer exists)
  follows the system language.
- **Tests:** `the_picker_lists_every_preference` — assert the painted text contains every
  language's native name. `a_language_without_a_font_is_dimmed` — assert the dimmed display.
- **Acceptance:** the picker renders headlessly; the Ukrainian catalog gains `Interface Language
  Мова інтерфейсу` in `uk.tsv`.
- **Risk:** the picker is shared with 2D. Fallback: the 2D picker already works; the 3D picker
  reuses it.

---

# UI-ACC — accessibility

### UI-ACC-001 — Keyboard navigation: tab order and focus
- **State:** READY
- **Depends on:** —
- **Files:** `crates/ui-egui/src/menus.rs`, `crates/ui-egui/src/chrome.rs` (owner U)
- **Contract:**
  ```rust
  // the tab order: a logical traversal of the UI's widgets
  ```
- **Behaviour:** Every interactive widget in the UI is reachable by keyboard: Tab moves forward,
  Shift+Tab moves backward, Enter activates, Escape cancels. The tab order is logical (top to
  bottom, left to right). A widget that is not focusable (a label) is skipped.
- **Refusals:** A widget that cannot receive focus (a disabled button) is skipped. A modal dialog
  traps focus (Tab does not leave the dialog).
- **Tests:** `every_widget_is_reachable_by_tab` — walk the tab order and assert every interactive
  widget is visited. `a_modal_dialog_traps_focus` — assert Tab does not leave the dialog.
- **Acceptance:** the keyboard navigation works in the headless test (verified by asserting the
  focus state, not by eyeballing a screenshot); the Ukrainian catalog gains `Keyboard Navigation
  Клавіатурна навігація` in `uk.tsv`.
- **Risk:** egui's focus handling changes between versions. Fallback: the focus order is
  isolated in a wrapper that can be updated.

### UI-ACC-002 — High contrast theme
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/theme.rs` (owner U)
- **Contract:**
  ```rust
  // the high contrast: a Tokens variant with stronger borders and more distinct colours
  ```
- **Behaviour:** A high-contrast theme is a `Tokens` variant with stronger borders, more distinct
  colours, and no reliance on colour alone to convey state (icons and text labels are always
  present). The theme is selected through the interface theme preference.
- **Refusals:** A high-contrast token that is not distinct enough (two colours that are too
  similar) is a test failure. A state that is conveyed by colour alone is a failure.
- **Tests:** `the_high_contrast_theme_is_distinct` — assert the border colours are distinct. `no_state_is_conveyed_by_colour_alone`
  — assert every state has an icon or text label.
- **Acceptance:** the high-contrast theme renders headlessly; the Ukrainian catalog gains `High
  Контрастний` in `uk.tsv`.
- **Risk:** the high-contrast theme is a new `Tokens` variant. Fallback: the variant is additive
  (the existing themes are unchanged).

### UI-ACC-003 — Screen reader: labels and roles
- **State:** READY
- **Depends on:** UI-ACC-001
- **Files:** `crates/ui-egui/src/chrome.rs`, `crates/ui-egui/src/menus.rs` (owner U)
- **Contract:**
  ```rust
  // the screen reader: every widget has a label and a role
  ```
- **Behaviour:** Every widget in the UI has an accessible label (a text description) and a role
  (button, checkbox, text field, etc.). The label is the same text the widget displays (or a
  more descriptive version for icon-only buttons). The role is set through egui's accessibility
  API.
- **Refusals:** A widget with no label is a test failure. A widget with a label that does not
  describe its action is a failure.
- **Tests:** `every_widget_has_a_label` — walk the UI and assert every widget has a label. `every_widget_has_a_role`
  — assert every widget has a role.
- **Acceptance:** the labels and roles work in the headless test (verified by asserting the
  accessibility tree, not by eyeballing a screenshot); the Ukrainian catalog gains `Screen Reader
  Екранний диктор` in `uk.tsv`.
- **Risk:** egui's accessibility API changes between versions. Fallback: the labels are set
  through a wrapper that can be updated.

### UI-ACC-004 — Font scaling: respect the OS font size
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/theme.rs` (owner U)
- **Contract:**
  ```rust
  // the font scaling: the OS font size is read and applied to the interface fonts
  ```
- **Behaviour:** The interface respects the OS font size (or a user-set scale factor). The scale
  is applied to the interface fonts (not the drawing's text, which is in drawing units). A scale
  of 1.0 is the default; larger scales increase the font size.
- **Refusals:** A scale that is zero or negative is refused (clamped to 1.0). A scale that would
  make the interface unusable (e.g. 10x) is clamped to a maximum.
- **Tests:** `the_font_scale_is_applied` — set a scale and assert the font size changes. `a_zero_scale_is_clamped`
  — assert the clamp.
- **Acceptance:** the font scaling works in the headless test; the Ukrainian catalog gains `Font
  Scaling	Масштабування шрифту` in `uk.tsv`.
- **Risk:** a large font scale breaks the layout. Fallback: the layout is responsive (it wraps
  and scrolls).

### UI-ACC-005 — Reduced motion: respect the OS animation setting
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/animation.rs` (owner U)
- **Contract:**
  ```rust
  pub enum AnimationPref { System, On, Off }
  pub fn resolve(self, system: Option<bool>) -> bool;
  ```
- **Behaviour:** The interface respects the OS animation setting (or a user-set preference). When
  animations are off, dialog and menu fades are disabled and scrolling is not smoothed. The
  preference is saved through `prefs_json`/`load_prefs`.
- **Refusals:** An OS setting that is not reported (None) follows the user's preference. A
  preference that is `System` and an OS that reports nothing defaults to on.
- **Tests:** `the_animation_preference_is_respected` — set the preference and assert the
  animation state. `a_system_preference_follows_the_os` — assert the resolution.
- **Acceptance:** the reduced-motion preference works in the headless test; the Ukrainian
  catalog gains `Animation Effects	Ефекти анімації` in `uk.tsv`.
- **Risk:** the animation is 2D-shared. Fallback: the 2D animation preference already works; the
  3D preference reuses it.

### UI-ACC-006 — Focus indicator: a visible focus ring
- **State:** READY
- **Depends on:** UI-ACC-001
- **Files:** `crates/ui-egui/src/theme.rs` (owner U)
- **Contract:**
  ```rust
  // the focus ring: a border in Tokens::accent around the focused widget
  ```
- **Behaviour:** The focused widget has a visible focus ring (a border in the accent colour). The
  ring is drawn in code (no iconography). The ring is always visible (not only on keyboard
  focus) so a user can always see where they are.
- **Refusals:** A widget that cannot receive focus has no ring. A widget that is disabled has a
  dimmed ring.
- **Tests:** `the_focused_widget_has_a_ring` — focus a widget and assert the ring is drawn. `a_disabled_widget_has_a_dimmed_ring`
  — assert the dimmed ring.
- **Acceptance:** the focus ring renders headlessly; the Ukrainian catalog gains `Focus Ring  Кільце
  фокусу` in `uk.tsv`.
- **Risk:** the focus ring is 2D-shared. Fallback: the 2D focus ring already works; the 3D ring
  reuses it.

### UI-ACC-007 — Colour-blind modes: deuteranopia, protanopia, tritanopia
- **State:** READY
- **Depends on:** UI-ACC-002
- **Files:** `crates/ui-egui/src/theme.rs` (owner U)
- **Contract:**
  ```rust
  // the colour-blind modes: a Tokens variant with distinct colours for each type
  ```
- **Behaviour:** A colour-blind mode is a `Tokens` variant with colours that are distinct for
  people with deuteranopia, protanopia or tritanopia. The mode does not rely on red/green
  distinctions alone (it uses blue/yellow and shape differences too). The mode is selected through
  the interface theme preference.
- **Refusals:** A mode that is not distinct enough is a test failure. A state that is conveyed by
  a red/green distinction alone is a failure.
- **Tests:** `every_mode_is_distinct` — assert the colours are distinct for each type. `no_state_uses_red_green_alone`
  — assert every state has a non-colour cue.
- **Acceptance:** the colour-blind modes render headlessly; the Ukrainian catalog gains `Colour-blind
  Режим для дальтоніків` in `uk.tsv`.
- **Risk:** the colour-blind modes are new `Tokens` variants. Fallback: the variants are additive
  (the existing themes are unchanged).

### UI-ACC-008 — Help system: context-sensitive help
- **State:** READY
- **Depends on:** UI-APP-001
- **Files:** `crates/ui-egui/src/menus.rs` (owner U)
- **Contract:**
  ```rust
  // the help system: F1 opens the help for the current context
  ```
- **Behaviour:** The help system opens the help for the current context (the active command, the
  active dialog, the active panel). F1 is the shortcut. The help is shown in a web view or a
  dialog. The help content is not in this repository (it is a separate docs site); the help
  system only opens the URL.
- **Refusals:** A context that has no help entry opens the general help. A URL that cannot be
  opened is reported with a message.
- **Tests:** `f1_opens_the_help` — press F1 and assert the help URL is opened. `a_context_with_no_help_opens_the_general_help`
  — assert the general URL.
- **Acceptance:** the help system works in the headless test; the Ukrainian catalog gains `Help
  Довідка` in `uk.tsv`.
- **Risk:** the help content is not in this repository. Fallback: the help system opens a URL;
  the content is a separate concern.

---

## Task count

| Epic | Tasks |
|---|---|
| `UI-APP` | UI-APP-001 … UI-APP-012 — 12 |
| `UI-VP` | UI-VP-001 … UI-VP-014 — 14 |
| `UI-TREE` | UI-TREE-001 … UI-TREE-010 — 10 |
| `UI-PARAM` | UI-PARAM-001 … UI-PARAM-010 — 10 |
| `UI-SKT` | UI-SKT-001 … UI-SKT-010 — 10 |
| `UI-ASM` | UI-ASM-001 … UI-ASM-008 — 8 |
| `UI-I18N` | UI-I18N-001 … UI-I18N-010 — 10 |
| `UI-ACC` | UI-ACC-001 … UI-ACC-008 — 8 |
| **total** | **82** |
