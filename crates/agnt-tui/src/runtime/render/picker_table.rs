//! The picker table.
//!
//! One component owns every picker's table surface: the styled header row, the
//! column spacing, the selection symbol, the focused row's style, and the
//! stateful render. A picker arm supplies its header labels, its rows, the column
//! layout it resolves, and the index it highlights.
//!
//! The section band rows come from this module too: they are the only rows that
//! paint the `picker_section_header` style, and each arm supplies its own indent
//! because the two surfaces put the label on different columns.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    widgets::{Cell, Row, Table, TableState},
};

use crate::{rendering::theme::TuiTheme, runtime::panels::PICKER_HIGHLIGHT_SYMBOL};

// region:    --- Types

/// A picker table's column layout: the constraints the table solver resolves and
/// the spacing between the columns. Each picker arm owns its own layout, so the
/// table takes the pair as one value.
pub(super) struct PickerTableLayout<'a> {
    pub columns: &'a [Constraint],
    pub spacing: u16,
}

// endregion: --- Types

// region:    --- Support

/// A section band row: the label on a single cell in the picker's
/// section-header style. The label carries its own indent — the models provider
/// headers indent two cells inside the MODEL column, the session day headers
/// start on the When column — so the row adds none.
pub(super) fn section_header_row(label: &str, theme: &TuiTheme) -> Row<'static> {
    Row::new(vec![Cell::from(label.to_string())]).style(theme.picker_section_header)
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
