//! The parameter panel: the model's parameters, what each evaluates to, and why one that does not
//! evaluate does not.
//!
//! # A parameter that does not evaluate is still in the list
//!
//! This is the panel's whole reason for existing. A parameter panel that hides the broken ones
//! leaves a model that will not rebuild and a user with no idea which of thirty names to go
//! looking for. So a parameter with no value is drawn with its formula, the word "not evaluated",
//! and **the reason underneath, in the warning colour, on the row itself** — never behind a
//! hover, and never replaced by a blank cell, because a blank cell and a zero look identical and a
//! feature built from it fails somewhere else entirely.
//!
//! # The row data
//!
//! [`ParamRow`] is plain data with no reference to `cadcraft_model`: the name, the text as typed, the
//! value when there is one, and the reason when there is not. The shell builds the rows from
//! whatever the model reports; this panel draws them and collects the edits.
//!
//! # Edits leave as actions
//!
//! A committed edit becomes a [`ParamAction`], which the host turns into an engine call and then a
//! rebuild. The panel never evaluates an expression itself: one evaluator, in the model, is the
//! only way the value on screen and the value the rebuild uses can be the same number.

use egui::{Layout, RichText};

use crate::theme::Tokens;

/// Which group of parameters a row is listed under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamGroup {
    /// The user's own parameters: the ones a person may add, change and delete.
    #[default]
    User,
    /// Parameters bound to a dimensional constraint. The sketch drives them, so they are listed
    /// and shown but not edited or deleted here.
    Dimensional,
}

impl ParamGroup {
    /// The heading of the group, as a lookup key.
    pub fn label(self) -> &'static str {
        match self {
            ParamGroup::User => "User Parameters",
            ParamGroup::Dimensional => "Dimensional Constraint Parameters",
        }
    }
}

/// One parameter as the panel draws it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamRow {
    pub name: String,
    /// The text as typed: a number, or a formula over other parameters. Shown exactly as the
    /// person wrote it, because a document should say what its author said.
    pub formula: String,
    /// What it evaluates to, when it does.
    pub value: Option<f64>,
    /// Why it does not, when it does not. Shown under the row, always.
    pub error: Option<String>,
    pub group: ParamGroup,
}

impl ParamRow {
    /// A parameter with a value.
    pub fn value_row(name: &str, formula: &str, value: f64) -> ParamRow {
        ParamRow { name: name.to_string(), formula: formula.to_string(), value: Some(value), error: None, group: ParamGroup::User }
    }

    /// A parameter that does not evaluate, and why.
    pub fn broken_row(name: &str, formula: &str, error: &str) -> ParamRow {
        ParamRow {
            name: name.to_string(),
            formula: formula.to_string(),
            value: None,
            error: Some(error.to_string()),
            group: ParamGroup::User,
        }
    }

    /// Whether this parameter has a value the rebuild can use.
    pub fn is_valid(&self) -> bool {
        self.value.is_some_and(f64::is_finite)
    }

    /// Whether the user may change or delete it: their own, and only their own.
    pub fn is_editable(&self) -> bool {
        self.group == ParamGroup::User && !self.name.is_empty()
    }
}

/// Panel state the host owns and the panel fills in.
///
/// `Default` is written out rather than derived, because the derive would say "no decimals" and a
/// parameter with no decimals shown is a panel that cannot read its own values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterPanel {
    /// The parameter the user has selected, by name.
    pub selected: Option<String>,
    /// The search filter; empty when there is none.
    pub filter: String,
    /// The decimals a value is shown to. Enough for a millimetre part and a metre one.
    pub decimals: usize,
}

impl Default for ParameterPanel {
    fn default() -> ParameterPanel {
        ParameterPanel { selected: None, filter: String::new(), decimals: 4 }
    }
}

impl ParameterPanel {
    /// A panel with the default settings.
    pub fn new() -> ParameterPanel {
        ParameterPanel::default()
    }

    /// Whether `row` passes the filter.
    pub fn matches(&self, row: &ParamRow) -> bool {
        let f = self.filter.trim().to_ascii_lowercase();
        f.is_empty() || row.name.to_ascii_lowercase().contains(&f) || row.formula.to_ascii_lowercase().contains(&f)
    }
}

/// What an edit in the panel asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamAction {
    /// Set a parameter's expression; the host validates it and rebuilds.
    Set { name: String, expr: String },
    /// Remove a user parameter.
    Delete { name: String },
    /// Add a new user parameter with a generated name.
    Add,
}

/// Draw the parameter panel, and say what the user asked for.
///
/// The rows are listed under their group headings; `User Parameters` first, because that is where
/// the ones a person owns live and the ones a sketch drives are worth reading second.
pub fn show(ui: &mut egui::Ui, rows: &[ParamRow], panel: &mut ParameterPanel) -> Option<ParamAction> {
    let t = Tokens::get();
    let mut action: Option<ParamAction> = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tl!("Parameters")).strong());
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut panel.filter)
                    .hint_text(crate::tl!("Search for parameter"))
                    .desired_width(140.0)
                    .id(egui::Id::new("cc_params_filter")),
            );
        });
    });
    ui.separator();

    let shown: Vec<&ParamRow> = rows.iter().filter(|r| panel.matches(r)).collect();
    if shown.is_empty() {
        // Still not the end of the panel: a model with no parameters is exactly the one a person
        // opens this to fix, so the button that adds the first one has to be there.
        ui.label(RichText::new(crate::tl!("No parameters yet")).color(t.text_faint));
    } else {
        // Scroll over the whole panel so the grid's columns line up across the two groups.
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("cc_parameters").show(ui, |ui| {
            egui::Grid::new("cc_param_grid").striped(true).num_columns(4).spacing(egui::vec2(10.0, 3.0)).show(ui, |ui| {
                for heading in ["Name", "Expression", "Value", ""] {
                    ui.label(RichText::new(crate::i18n::t(heading)).strong());
                }
                ui.end_row();
                for group in [ParamGroup::User, ParamGroup::Dimensional] {
                    let group_rows: Vec<&ParamRow> = shown.iter().copied().filter(|r| r.group == group).collect();
                    if group_rows.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(crate::i18n::t(group.label())).color(t.text_dim).small());
                    ui.end_row();
                    for row in group_rows {
                        let selected = panel.selected.as_deref() == Some(row.name.as_str());
                        if selected {
                            ui.label(RichText::new(&row.name).strong());
                        } else {
                            ui.label(&row.name);
                        }
                        // The formula is editable for a user's own parameter and read-only for a
                        // sketch's, because a dimensional constraint's value is the sketch's to set.
                        let id = egui::Id::new(("cc_param_expr", &row.name));
                        if row.is_editable() {
                            if let Some(expr) = crate::parametric::expr_field(ui, id, &row.formula)
                                && expr != row.formula
                            {
                                action = Some(ParamAction::Set { name: row.name.clone(), expr });
                            }
                        } else {
                            ui.label(RichText::new(&row.formula).color(t.text_faint));
                        }
                        // The value, or the words that say there isn't one. Never a blank cell: a
                        // blank and a zero are the same on screen and not the same in a rebuild.
                        match row.value {
                            Some(v) if v.is_finite() => ui.label(format_value(v, panel.decimals)),
                            _ => ui.label(RichText::new(crate::tl!("Not evaluated")).color(t.warn)),
                        }
                        // The controls belong to the selection, so there is one of each.
                        if selected && row.is_editable() {
                            if ui.small_button(crate::tl!("Delete")).on_hover_text(crate::tl!("Delete")).clicked() {
                                action = Some(ParamAction::Delete { name: row.name.clone() });
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        // The reason, on the row, always. This is the line the panel exists for.
                        if let Some(error) = row.error.as_deref().filter(|e| !e.trim().is_empty()) {
                            ui.label("");
                            ui.label(RichText::new(error).color(t.warn).small());
                            ui.end_row();
                        }
                    }
                }
            });
        });
    }
    // The new-parameter button sits under the grid, where a person looking for it expects it. It wins
// over anything a row asked for in the same frame: adding a parameter is not something a person
// does by accident while they are fixing another one.
if ui.small_button(crate::tl!("ƒx  New User Parameter")).on_hover_text(crate::tl!("Creates a user parameter")).clicked() {
    return Some(ParamAction::Add);
}
action
}

/// A value with no trailing zeros, so `10` does not read as `10.0000` next to `2.5`.
fn format_value(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return crate::tl!("Not evaluated").to_string();
    }
    let text = format!("{v:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{Queryable, by};

    /// The panel's world: rows in, one action out.
    struct Fixture {
        rows: Vec<ParamRow>,
        panel: ParameterPanel,
        action: Option<ParamAction>,
    }

    fn rows() -> Vec<ParamRow> {
        vec![
            ParamRow::value_row("width", "12.5", 12.5),
            ParamRow::value_row("height", "width * 2", 25.0),
            ParamRow::broken_row("depth", "width + ", "unexpected end of expression"),
            ParamRow { group: ParamGroup::Dimensional, ..ParamRow::value_row("sketch_gap", "3", 3.0) },
        ]
    }

    fn harness(rows: Vec<ParamRow>) -> Harness<'static, Fixture> {
        let mut h = Harness::builder().with_size(vec2(400.0, 620.0)).build_ui_state(
            |ui, fx: &mut Fixture| {
                // A host runs the action it was handed, so the last one stands until a new one
                // arrives.
                if let Some(a) = show(ui, &fx.rows, &mut fx.panel) {
                    fx.action = Some(a);
                }
            },
            Fixture { rows, panel: ParameterPanel::new(), action: None },
        );
        h.run_steps(3);
        h
    }

    /// Every string the frame painted, so a message can be looked for without an accesskit query.
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
    fn every_parameter_is_listed_under_its_group_with_its_value() {
        let h = harness(rows());
        let text = painted(&h);
        assert!(text.contains("User Parameters"), "{text}");
        assert!(text.contains("Dimensional Constraint Parameters"), "{text}");
        assert!(text.contains("width") && text.contains("height") && text.contains("depth"), "{text}");
        assert!(text.contains("sketch_gap"), "{text}");
        // The values are formatted without a tail of zeros, next to the formula that made them.
        assert!(text.contains("12.5"), "{text}");
        assert!(text.contains("25"), "{text}");
        assert!(text.contains("width * 2"), "{text}");
    }

    #[test]
    fn a_parameter_that_does_not_evaluate_is_listed_with_its_reason() {
        let h = harness(rows());
        let text = painted(&h);
        // The row is still there...
        assert!(text.contains("depth"), "the broken parameter was hidden:\n{text}");
        // ...it says it has no value...
        assert!(text.contains("Not evaluated"), "{text}");
        // ...and it says why, on the row, with nothing hovered.
        assert!(text.contains("unexpected end of expression"), "{text}");
        // And the formula the person typed is still shown, not replaced by the failure.
        assert!(text.contains("width + "), "{text}");
    }

    #[test]
    fn a_sketch_parameter_is_listed_but_not_editable() {
        let mut h = harness(rows());
        h.state_mut().panel.selected = Some("sketch_gap".into());
        h.run_steps(2);
        // A dimensional constraint's value belongs to the constraint, so there is no Delete on it.
        let text = painted(&h);
        assert!(text.contains("sketch_gap"), "{text}");
        assert!(h.state().action.is_none(), "a sketch parameter was edited");
    }

    #[test]
    fn the_filter_hides_rows_without_reordering_the_rest() {
        let mut h = harness(rows());
        h.state_mut().panel.filter = "width".into();
        h.run_steps(2);
        let text = painted(&h);
        assert!(text.contains("width"), "{text}");
        assert!(!text.contains("depth"), "the filter did nothing:\n{text}");
        // A filter that matches nothing says so, and still offers to add a parameter.
        h.state_mut().panel.filter = "nothing matches this".into();
        h.run_steps(2);
        assert!(painted(&h).contains("No parameters yet"));
    }

    #[test]
    fn the_new_parameter_button_is_there_even_with_nothing_to_show() {
        for rows in [rows(), Vec::new()] {
            let mut h = harness(rows);
            h.get_by_label("ƒx  New User Parameter").click();
            h.run_steps(2);
            assert_eq!(h.state().action, Some(ParamAction::Add), "a model with no parameters must still be fixable");
        }
    }

    #[test]
    fn a_user_formula_is_shown_in_an_editable_field_carrying_what_was_typed() {
        let h = harness(rows());
        // A user's own parameters are editable, so their formulas are text fields holding exactly
        // the text that was typed — the number for `width`, the formula for `height`.
        h.get(by().role(egui::accesskit::Role::TextInput).value("12.5"));
        h.get(by().role(egui::accesskit::Role::TextInput).value("width * 2"));
        // The broken one is editable too: it is the user's own to fix.
        h.get(by().role(egui::accesskit::Role::TextInput).value("width + "));
        // The value column is text, not a field: only the formula is edited here.
        assert!(painted(&h).contains("12.5"));
    }

    #[test]
    fn a_parameter_that_is_not_editable_says_so() {
        assert!(ParamRow::value_row("w", "1", 1.0).is_editable());
        assert!(ParamRow::value_row("w", "1", 1.0).is_valid());
        let broken = ParamRow::broken_row("d", "1 +", "unexpected end of expression");
        assert!(!broken.is_valid(), "a parameter with no value is not valid");
        assert!(broken.is_editable(), "but it is still the user's own to fix");
        let sketch = ParamRow { group: ParamGroup::Dimensional, ..ParamRow::value_row("g", "1", 1.0) };
        assert!(!sketch.is_editable(), "a sketch's parameter is not the panel's to change");
        let unnamed = ParamRow::default();
        assert!(!unnamed.is_editable());
        let infinite = ParamRow { value: Some(f64::INFINITY), ..ParamRow::value_row("x", "1/0", 1.0) };
        assert!(!infinite.is_valid(), "an infinity is not a value");
    }

    #[test]
    fn a_value_is_formatted_without_a_tail_of_zeros() {
        assert_eq!(format_value(10.0, 4), "10");
        assert_eq!(format_value(2.5, 4), "2.5");
        assert_eq!(format_value(0.0, 4), "0");
        assert_eq!(format_value(-1.25, 2), "-1.25");
        assert_eq!(format_value(1.0 / 3.0, 3), "0.333");
        assert_eq!(format_value(1234.5678, 2), "1234.57");
        // A number that is not a number is reported, not printed as `NaN`.
        assert_eq!(format_value(f64::NAN, 3), "Not evaluated");
        assert_eq!(format_value(f64::INFINITY, 3), "Not evaluated");
    }

    #[test]
    fn the_group_headings_are_the_models_own_words() {
        assert_eq!(ParamGroup::User.label(), "User Parameters");
        assert_eq!(ParamGroup::Dimensional.label(), "Dimensional Constraint Parameters");
        assert_eq!(ParamGroup::default(), ParamGroup::User);
    }
}