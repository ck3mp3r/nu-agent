//! The picker table.
//!
//! One component owns every picker's table surface: the styled header row, the
//! column spacing, the selection symbol, the focused row's style, and the
//! stateful render. A picker arm supplies its header labels, its rows, the column
//! layout it resolves, and the index it highlights.
//!
//! The section rows come from this module too. The options carry their section
//! as data, and `rows_with_sections` derives one head row per contiguous run of
//! the same key from the FILTERED options. Heads are never stored as items, so a
//! section whose options the query filtered out loses its head with them.
//!
//! The section heads are the only rows that paint the `picker_section_header`
//! style. They sit flush-left in the table's first column; the child rows carry
//! their own indent, which reads as one hierarchy step under the head.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    widgets::{Cell, Row, Table, TableState},
};

use crate::{
    rendering::theme::TuiTheme, runtime::panels::PICKER_HIGHLIGHT_SYMBOL, state::PickerOption,
};

// region:    --- Types

/// A picker table's column layout: the constraints the table solver resolves and
/// the spacing between the columns. Each picker arm owns its own layout, so the
/// table takes the pair as one value.
pub(super) struct PickerTableLayout<'a> {
    pub columns: &'a [Constraint],
    pub spacing: u16,
}

/// A picker table's rows with their derived section heads, plus the row index
/// that holds the selection.
pub(super) struct SectionedRows {
    pub rows: Vec<Row<'static>>,
    /// The table row of the selected option. `None` when the picker has no
    /// options; an out-of-range selection falls back to the first data row.
    pub selected: Option<usize>,
}

// endregion: --- Types

// region:    --- Support

/// A section band row: the label on a single cell in the picker's
/// section-header style, flush-left in the table's first column.
pub(super) fn section_header_row(label: &str, theme: &TuiTheme) -> Row<'static> {
    Row::new(vec![Cell::from(label.to_string())]).style(theme.picker_section_header)
}

/// Whether `next` opens a new section after `previous`: entering any section
/// does, and a flat option (`None`) never does.
fn opens_section(previous: Option<&str>, next: Option<&str>) -> bool {
    next.is_some() && previous != next
}

/// The head rows the walk emits for `options`: one per contiguous run of the
/// same section key. A picker that needs the count before it renders (the
/// session picker's overflow cue) reads it here; the walk below uses the same
/// predicate, so the count and the emitted heads cannot disagree.
pub(crate) fn section_head_count(options: &[PickerOption]) -> usize {
    let mut heads = 0usize;
    let mut current: Option<&str> = None;
    for option in options {
        if opens_section(current, option.section.as_deref()) {
            heads += 1;
        }
        current = option.section.as_deref();
    }
    heads
}

/// Walk the FILTERED options and build the table rows: a head row at every
/// section transition, then the arm's own row for the option. `row_of` maps an
/// option to its cells; `head_label_of` labels the option that opens a section,
/// so an arm can derive the label at render time (the session days read
/// Today/Yesterday off the clock). The returned selection index accounts for the
/// heads the walk inserted.
pub(super) fn rows_with_sections(
    options: &[PickerOption],
    selection: usize,
    theme: &TuiTheme,
    row_of: impl Fn(&PickerOption) -> Row<'static>,
    head_label_of: impl Fn(&PickerOption) -> String,
) -> SectionedRows {
    let mut rows: Vec<Row<'static>> = Vec::new();
    let mut current: Option<&str> = None;
    let mut selected = None;
    let mut first_data_row = None;
    for (index, option) in options.iter().enumerate() {
        if opens_section(current, option.section.as_deref()) {
            rows.push(section_header_row(&head_label_of(option), theme));
        }
        current = option.section.as_deref();
        first_data_row.get_or_insert(rows.len());
        rows.push(row_of(option));
        if index == selection {
            selected = Some(rows.len() - 1);
        }
    }
    SectionedRows {
        selected: selected.or(first_data_row),
        rows,
    }
}

/// The picker table: a styled header, the caller's rows, the caller's column
/// layout, and the focused selection. Header cells arrive unstyled; the table
/// owns the header style, the column spacing, the selection symbol, and the
/// focus style.
pub(super) fn render_picker_table(
    frame: &mut Frame,
    rect: Rect,
    header: Row<'static>,
    rows: impl IntoIterator<Item = Row<'static>>,
    layout: PickerTableLayout<'_>,
    selected: Option<usize>,
    theme: &TuiTheme,
) {
    let table = Table::new(rows, layout.columns)
        .header(header.style(theme.subtle_meta))
        .column_spacing(layout.spacing)
        .highlight_symbol(PICKER_HIGHLIGHT_SYMBOL)
        .row_highlight_style(theme.focus);
    let mut table_state = TableState::default();
    table_state.select(selected);
    frame.render_stateful_widget(table, rect, &mut table_state);
}

// endregion: --- Support
