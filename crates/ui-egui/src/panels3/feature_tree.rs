//! The feature tree panel: the model's features, in order, each with the state of the last rebuild
//! of that one.
//!
//! # Why a feature that failed says so in the list
//!
//! A failed edit is the ordinary case, not an edge case: a depth of `-5` is a typo somebody has to
//! find. If the tree replaced a broken feature with an icon, or put the message in a tooltip, the
//! person fixing the model would have to hover every row and read them one at a time to find the
//! one that broke. So the message is a **second line of the row itself**, in the warning colour,
//! with the message the kernel gave — not a summary, not a tooltip. The same is true of the rows
//! the failure *blocked*: they say which feature blocked them, for the same reason.
//!
//! # The row data
//!
//! [`FeatureRow`] is plain data with no reference to `cadcraft_model`. The model crate owns the
//! features; the shell reads their state and builds rows; this panel draws rows. A new feature kind
//! is a change to the model and to whichever command reads it, never to this file.
//!
//! # Markers
//!
//! Two markers are drawn in code (there is no iconography in this repository that did not come out
//! of a renderer): a **crossed-out eye** for a suppressed feature, which is still part of the
//! rebuild but hidden from the list, and a **turning arrow** for a rolled-back feature, whose
//! definition is an earlier one the user chose. Both are also in the tooltip, because a 14-pixel
//! glyph should not be the only way to find out what it means.

use egui::{Align, Color32, Layout, Rect, RichText, Sense, Stroke};

use crate::theme::Tokens;

/// How one feature came out of the last rebuild of the model.
///
/// This is the model's own four states, restated so the panel does not import it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FeatureState {
    /// Built, and up to date.
    #[default]
    Built,
    /// Not built yet, or deliberately skipped.
    Pending,
    /// Built once, but the current inputs no longer produce a solid. The last good shape is still
    /// there; `String` is the reason it could not be replaced.
    Failed(String),
    /// An earlier feature failed, so this one never ran. `by` names that feature.
    Blocked { by: String, message: String },
}

impl FeatureState {
    /// Whether the feature itself is broken.
    pub fn is_failed(&self) -> bool {
        matches!(self, FeatureState::Failed(_))
    }

    /// Whether the feature is waiting for another one.
    pub fn is_blocked(&self) -> bool {
        matches!(self, FeatureState::Blocked { .. })
    }

    /// Whether the feature is neither built, waiting nor broken — which is what every healthy row
    /// is, and the only state the row draws without a word of explanation.
    pub fn is_built(&self) -> bool {
        matches!(self, FeatureState::Built)
    }

    /// The reason to show on the row, if there is one.
    pub fn message(&self) -> Option<&str> {
        match self {
            FeatureState::Built | FeatureState::Pending => None,
            FeatureState::Failed(m) => Some(m.as_str()),
            FeatureState::Blocked { message, .. } => Some(message.as_str()),
        }
    }

    /// The feature that blocks this one, if it is blocked.
    pub fn blocked_by(&self) -> Option<&str> {
        match self {
            FeatureState::Blocked { by, .. } => Some(by.as_str()),
            _ => None,
        }
    }

    /// The label of the state, as a lookup key.
    pub fn label(&self) -> &'static str {
        match self {
            FeatureState::Built => "Built",
            FeatureState::Pending => "Pending",
            FeatureState::Failed(_) => "Failed",
            FeatureState::Blocked { .. } => "Blocked",
        }
    }
}

/// One feature as the panel draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureRow {
    pub name: String,
    /// The model's own word for what the feature does: "extrude", "cut", "fillet", ...
    pub kind: String,
    pub state: FeatureState,
    /// Hidden from the list. The rebuild still uses it.
    pub suppressed: bool,
    /// The feature's definition is an earlier one the user rolled back to.
    pub rolled_back: bool,
    /// How far to indent the row: 0 for the body, 1 for a feature of a body.
    pub depth: u8,
}

impl FeatureRow {
    /// A feature in the ordinary case: built, visible, at the top level.
    pub fn built(name: &str, kind: &str) -> FeatureRow {
        FeatureRow { name: name.to_string(), kind: kind.to_string(), ..FeatureRow::default() }
    }

    /// Whether the row carries something the user has to act on.
    pub fn needs_attention(&self) -> bool {
        self.state.is_failed() || self.state.is_blocked()
    }
}

/// Panel state the host owns and the panel fills in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureTree {
    /// The feature the user has selected, by name.
    pub selected: Option<String>,
    /// The search filter; empty when there is none.
    pub filter: String,
}

impl FeatureTree {
    /// Whether `row` passes the filter.
    pub fn matches(&self, row: &FeatureRow) -> bool {
        let f = self.filter.trim().to_ascii_lowercase();
        f.is_empty() || row.name.to_ascii_lowercase().contains(&f) || row.kind.to_ascii_lowercase().contains(&f)
    }
}

/// What a click on the tree asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureAction {
    Select { name: String },
    Edit { name: String },
    ToggleSuppressed { name: String },
    Delete { name: String },
}

/// How far one level of nesting is indented, in points.
const INDENT: f32 = 12.0;
/// The side of a state glyph.
const GLYPH: f32 = 13.0;
/// The gap between the glyphs and the name.
const GLYPH_GAP: f32 = 4.0;

/// Draw the feature tree, and say what the user asked for.
///
/// `rows` is the model's feature list **in tree order**, which is the order it must be drawn in:
/// a feature means nothing without the features below it. The filter hides rows but never reorders
/// them.
pub fn show(ui: &mut egui::Ui, rows: &[FeatureRow], tree: &mut FeatureTree) -> Option<FeatureAction> {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tl!("Features")).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut tree.filter)
                    .hint_text(crate::tl!("Search for feature"))
                    .desired_width(140.0)
                    .id(egui::Id::new("cc_features_filter")),
            );
        });
    });
    ui.separator();

    let mut action: Option<FeatureAction> = None;
    let shown: Vec<&FeatureRow> = rows.iter().filter(|r| tree.matches(r)).collect();
    if shown.is_empty() {
        ui.label(RichText::new(crate::tl!("No features yet")).color(t.text_faint));
        return None;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("cc_features").show(ui, |ui| {
        for row in &shown {
            if let Some(a) = row_ui(ui, row, tree, &t) {
                action = Some(a);
            }
        }
    });
    action
}

/// One feature: the row itself, then its message underneath when it has one.
fn row_ui(ui: &mut egui::Ui, row: &FeatureRow, tree: &mut FeatureTree, t: &Tokens) -> Option<FeatureAction> {
    let selected = tree.selected.as_deref() == Some(row.name.as_str());
    let mut action = None;
    let response = ui.push_id(&row.name, |ui| {
        let inner = ui.horizontal(|ui| {
            ui.add_space(f32::from(row.depth) * INDENT);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(GLYPH, GLYPH), Sense::hover());
            paint_state(ui.painter(), rect, row);
            ui.add_space(GLYPH_GAP);
            if row.suppressed {
                let (mark, _) = ui.allocate_exact_size(egui::vec2(GLYPH, GLYPH), Sense::hover());
                paint_hidden(ui.painter(), mark, t.text_dim);
                ui.add_space(GLYPH_GAP);
            }
            if row.rolled_back {
                let (mark, _) = ui.allocate_exact_size(egui::vec2(GLYPH, GLYPH), Sense::hover());
                paint_rolled_back(ui.painter(), mark, t.text_dim);
                ui.add_space(GLYPH_GAP);
            }
            let name = if row.suppressed {
                RichText::new(&row.name).color(t.text_faint)
            } else if selected {
                RichText::new(&row.name).strong()
            } else {
                RichText::new(&row.name)
            };
            ui.label(name);
            ui.label(RichText::new(&row.kind).color(t.text_faint).small());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if selected {
                    if ui
                        .add(egui::Button::new(crate::tl!("Suppressed")).selected(row.suppressed).small())
                        .on_hover_text(crate::tl!("Suppressed"))
                        .clicked()
                    {
                        return Some(FeatureAction::ToggleSuppressed { name: row.name.clone() });
                    }
                    if ui.small_button(crate::tl!("Delete")).on_hover_text(crate::tl!("Delete")).clicked() {
                        return Some(FeatureAction::Delete { name: row.name.clone() });
                    }
                }
                if !row.state.is_built() {
                    ui.label(RichText::new(crate::i18n::t(row.state.label())).color(state_color(row, t)).small());
                }
                None
            })
            .inner
        });
        inner.response
    });
    if response.clicked() {
        tree.selected = Some(row.name.clone());
        action = Some(FeatureAction::Select { name: row.name.clone() });
    }
    if response.double_clicked() {
        tree.selected = Some(row.name.clone());
        action = Some(FeatureAction::Edit { name: row.name.clone() });
    }
    // The message is the row's own second line, always visible. Not a tooltip: a tooltip is a thing
    // a person has to know to look for, and this is the one line they came to read.
    if let Some(message) = row.state.message() {
        ui.push_id(&row.name, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(f32::from(row.depth) * INDENT + GLYPH + GLYPH_GAP);
                ui.label(RichText::new(message).color(t.warn).small().weak());
            });
        });
    }
    if let Some(by) = row.state.blocked_by() {
        ui.push_id(&row.name, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(f32::from(row.depth) * INDENT + GLYPH + GLYPH_GAP);
                ui.label(RichText::new(crate::tf!("Blocked by {feature}", feature = by)).color(t.text_faint).small());
            });
        });
    }
    action
}

/// The colour a state is drawn in: healthy states are quiet, broken ones are not.
fn state_color(row: &FeatureRow, t: &Tokens) -> Color32 {
    match &row.state {
        FeatureState::Built => t.accent,
        FeatureState::Pending => t.text_faint,
        FeatureState::Failed(_) => Color32::from_rgb(0xff, 0x8a, 0x6a),
        FeatureState::Blocked { .. } => t.warn,
    }
}

/// The state dot: a filled disc for a built feature, a broken one for a failure, an empty ring for
/// anything that is waiting.
fn paint_state(p: &egui::Painter, r: Rect, row: &FeatureRow) {
    let c = state_color(row, &Tokens::get());
    let c = r.center();
    match &row.state {
        FeatureState::Pending | FeatureState::Blocked { .. } => p.circle_stroke(c, GLYPH * 0.3, Stroke::new(1.6, c)),
        _ => p.circle_filled(c, GLYPH * 0.3, c),
    }
}

/// A closed eye with a slash through it: the marker for a suppressed feature. Drawn in code, like
/// every other icon in this application.
fn paint_hidden(p: &egui::Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.2, c);
    let mid = r.center();
    let (w, h) = (r.width() * 0.5 - 1.0, r.height() * 0.28);
    // The lens: two arcs meeting at the middle. An egui circle is round, so the lid is drawn as a
    // chord line per half — enough to read as an eye at 13 points, which is the whole requirement.
    p.line_segment([egui::pos2(mid.x - w, mid.y), egui::pos2(mid.x, mid.y - h)], s);
    p.line_segment([egui::pos2(mid.x, mid.y - h), egui::pos2(mid.x + w, mid.y)], s);
    p.line_segment([egui::pos2(mid.x - w, mid.y), egui::pos2(mid.x, mid.y + h)], s);
    p.line_segment([egui::pos2(mid.x, mid.y + h), egui::pos2(mid.x + w, mid.y)], s);
    p.circle_stroke(mid, h * 0.85, s);
    // The slash.
    p.line_segment([egui::pos2(r.left() + 1.0, r.bottom() - 1.0), egui::pos2(r.right() - 1.0, r.top() + 1.0)], Stroke::new(1.3, c));
}

/// A turning arrow: the marker for a feature rolled back to an earlier definition.
fn paint_rolled_back(p: &egui::Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.2, c);
    let mid = r.center();
    let rad = r.height() * 0.3;
    // A three-quarter arc, open at the top right, with the arrow head on the open end.
    let mut pts = Vec::with_capacity(13);
    for i in 0..=12 {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * 1.5 * (i as f32 / 12.0);
        pts.push(egui::pos2(mid.x + rad * a.cos(), mid.y - rad * a.sin()));
    }
    p.add(egui::Shape::line(pts, s));
    let tip = pts.first().copied().unwrap_or(mid);
    p.add(egui::Shape::convex_polygon(vec![tip, tip + egui::vec2(-3.0, -1.0), tip + egui::vec2(-1.0, 3.0)], c, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// The panel's world: rows in, one action out, nothing kept behind the panel's back.
    struct Fixture {
        rows: Vec<FeatureRow>,
        tree: FeatureTree,
        action: Option<FeatureAction>,
    }

    fn rows() -> Vec<FeatureRow> {
        vec![
            FeatureRow::built("Body1", "extrude"),
            FeatureRow {
                name: "Filleted".into(),
                kind: "fillet".into(),
                state: FeatureState::Failed("the blend size 4 runs off the end of edge (2, 0, 0)".into()),
                depth: 1,
                ..FeatureRow::default()
            },
            FeatureRow {
                name: "Cut1".into(),
                kind: "cut".into(),
                state: FeatureState::Blocked { by: "Filleted".into(), message: "Filleted did not build".into() },
                depth: 1,
                ..FeatureRow::default()
            },
            FeatureRow { name: "Sketch2".into(), kind: "sketch".into(), state: FeatureState::Pending, suppressed: true, ..FeatureRow::default() },
            FeatureRow { name: "OldPocket".into(), kind: "cut".into(), rolled_back: true, ..FeatureRow::default() },
        ]
    }

    fn harness(rows: Vec<FeatureRow>) -> Harness<'static, Fixture> {
        // 60 frames a second: the default step spreads two clicks past the double-click delay.
        let mut h = Harness::builder().with_size(vec2(380.0, 620.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, fx: &mut Fixture| {
                // A host runs the action it was handed, so the last one stands until a new one
                // arrives.
                if let Some(a) = show(ui, &fx.rows, &mut fx.tree) {
                    fx.action = Some(a);
                }
            },
            Fixture { rows, tree: FeatureTree::default(), action: None },
        );
        h.run_steps(3);
        h
    }

    /// Every string the frame painted, so a message can be looked for without an accesskit query.
    fn painted(h: &Harness<'static, Fixture>) -> String {
        let mut text = String::new();
        for (label, _) in texts(h) {
            text.push_str(&label);
            text.push('\n');
        }
        text
    }

    /// Every painted string and the rectangle it was painted at.
    fn texts(h: &Harness<'static, Fixture>) -> Vec<(String, Rect)> {
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(s) => out.push((s.galley.text().to_string(), s.visual_bounding_rect())),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for shape in &h.output().shapes {
            collect(&shape.shape, &mut out);
        }
        out
    }

    /// Where the label `needle` was painted, so a test can click exactly it.
    fn label_pos(h: &Harness<'static, Fixture>, needle: &str) -> Pos2 {
        texts(h)
            .iter()
            .find(|(t, _)| t == needle)
            .map(|(_, r)| r.center())
            .unwrap_or_else(|| panic!("{needle} was not painted:\n{}", painted(h)))
    }

    fn click(h: &mut Harness<'static, Fixture>, at: Pos2) {
        h.event(Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
        h.run_steps(2);
    }

    fn double_click(h: &mut Harness<'static, Fixture>, at: Pos2) {
        h.event(Event::PointerMoved(at));
        h.run_steps(2);
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
                h.run_steps(1);
            }
        }
        h.run_steps(2);
    }

    #[test]
    fn every_feature_is_listed_in_order_with_its_state() {
        let h = harness(rows());
        let text = painted(&h);
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} is missing from:\n{text}"));
        assert!(at("Body1") < at("Filleted"), "the tree order was not kept");
        assert!(at("Filleted") < at("Cut1"));
        assert!(at("Cut1") < at("Sketch2"));
        assert!(at("Sketch2") < at("OldPocket"));
        // The healthy rows say what they are and nothing more.
        assert!(text.contains("extrude") && text.contains("fillet") && text.contains("cut"));
        assert!(text.contains("Pending"), "the pending row says so:\n{text}");
    }

    #[test]
    fn a_failed_feature_shows_its_message_without_a_hover() {
        let h = harness(rows());
        let text = painted(&h);
        // The message the kernel gave, verbatim, on the frame with nothing hovered and nothing
        // focused: not a tooltip, not a status line, the row itself.
        assert!(text.contains("the blend size 4 runs off the end of edge (2, 0, 0)"), "{text}");
        assert!(text.contains("Failed"), "{text}");
    }

    #[test]
    fn a_blocked_feature_says_which_feature_blocked_it() {
        let h = harness(rows());
        let text = painted(&h);
        assert!(text.contains("Blocked by Filleted"), "{text}");
        assert!(text.contains("Filleted did not build"), "the blocked row explains itself too:\n{text}");
    }

    #[test]
    fn suppressed_and_rolled_back_features_are_marked_and_still_listed() {
        let h = harness(rows());
        let text = painted(&h);
        assert!(text.contains("Sketch2"), "a suppressed feature is still in the list:\n{text}");
        assert!(text.contains("OldPocket"), "a rolled-back feature is still in the list:\n{text}");
    }

    #[test]
    fn clicking_a_row_selects_it_and_offers_its_controls() {
        let mut h = harness(rows());
        assert!(h.state().tree.selected.is_none());
        let at = label_pos(&h, "Body1");
        click(&mut h, at);
        assert_eq!(h.state().tree.selected.as_deref(), Some("Body1"));
        assert_eq!(h.state().action, Some(FeatureAction::Select { name: "Body1".into() }));
        // The controls belong to the selection, so they exist once and can be reached.
        h.get_by_label("Suppressed").click();
        h.run_steps(2);
        assert_eq!(h.state().action, Some(FeatureAction::ToggleSuppressed { name: "Body1".into() }));
        h.get_by_label("Delete").click();
        h.run_steps(2);
        assert_eq!(h.state().action, Some(FeatureAction::Delete { name: "Body1".into() }));
    }

    #[test]
    fn a_double_click_asks_for_the_editor() {
        let mut h = harness(rows());
        let at = label_pos(&h, "Cut1");
        double_click(&mut h, at);
        assert_eq!(h.state().tree.selected.as_deref(), Some("Cut1"));
        assert_eq!(h.state().action, Some(FeatureAction::Edit { name: "Cut1".into() }));
    }

    #[test]
    fn the_filter_hides_rows_without_reordering_the_rest() {
        let mut h = harness(rows());
        h.state_mut().tree.filter = "cut".into();
        h.run_steps(2);
        let text = painted(&h);
        assert!(text.contains("Cut1") && text.contains("OldPocket"), "{text}");
        assert!(!text.contains("Body1"), "the filter did nothing:\n{text}");
        // The rows that are left are still in tree order.
        assert!(text.find("Cut1") < text.find("OldPocket"));
        h.state_mut().tree.filter = "nothing matches this".into();
        h.run_steps(2);
        assert!(painted(&h).contains("No features yet"));
    }

    #[test]
    fn an_empty_model_says_so_instead_of_drawing_nothing() {
        let h = harness(Vec::new());
        assert!(painted(&h).contains("No features yet"));
        assert!(h.state().action.is_none());
    }

    #[test]
    fn the_state_helpers_say_what_they_mean() {
        assert!(FeatureState::default().is_built());
        assert!(FeatureState::Pending.message().is_none());
        assert!(!FeatureState::Pending.is_failed() && !FeatureState::Pending.is_blocked());
        let failed = FeatureState::Failed("no solid".into());
        assert!(failed.is_failed() && !failed.is_blocked());
        assert_eq!(failed.message(), Some("no solid"));
        assert_eq!(failed.label(), "Failed");
        assert_eq!(failed.blocked_by(), None);
        let blocked = FeatureState::Blocked { by: "Pad".into(), message: "Pad did not build".into() };
        assert!(blocked.is_blocked() && !blocked.is_failed());
        assert_eq!(blocked.blocked_by(), Some("Pad"));
        assert_eq!(blocked.message(), Some("Pad did not build"));
        assert_eq!(blocked.label(), "Blocked");
        assert!(!FeatureRow::built("a", "cut").needs_attention());
        assert!(FeatureRow { state: FeatureState::Failed("x".into()), ..FeatureRow::default() }.needs_attention(), "a failed row needs attention");
        assert!(FeatureRow { state: FeatureState::Blocked { by: "a".into(), message: String::new() }, ..FeatureRow::default() }.needs_attention(), "a blocked row too");
        assert_eq!(FeatureRow::default().state, FeatureState::Built, "a row that says nothing is built");
    }
}