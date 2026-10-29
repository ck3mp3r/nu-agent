use ratatui::{
    Frame,
    layout::{Constraint, Margin, Rect},
    text::{Line, Text},
    widgets::{Cell, Paragraph, Row},
};

use unicode_width::UnicodeWidthStr;

use crate::runtime::panels::{
    AGENT_PICKER_EMPTY_STATE_MESSAGE, COMMAND_PALETTE_COLUMN_SPACING, COMMAND_PALETTE_COLUMNS,
    MCP_TABLE_COLUMN_SPACING, MCP_TABLE_COLUMNS, MODEL_PICKER_EMPTY_STATE_MESSAGE,
    PICKER_HIGHLIGHT_SYMBOL, THEME_PICKER_EMPTY_STATE_MESSAGE, picker_query_line,
};
use crate::runtime::{
    command_palette_table_model, command_palette_title, help_panel_lines, help_panel_max_scroll,
    help_panel_overflow_cue, mcp_details_height_for_inner_height, mcp_details_rows,
    mcp_panel_title, mcp_selected_details_lines, mcp_table_model, picker_action_keys,
    picker_details_height_for_inner_height, popup_title, scroll_panel_title,
    session_picker::SessionPickerRow, skills_panel_lines, status::format::ellipsize,
    status_panel_lines,
};
use crate::{
    runtime::{
        render::frame::{
            ModalPanelKind, modal_rect_for_panel, picker_column_budgets, popup_body_rows,
        },
        render::picker_table::{PickerTableLayout, render_picker_table, section_header_row},
        render::{PickerFooter, render_modal_frame, render_scroll_text_panel},
    },
    state::{InfoPanel, PickerPayload},
};

use crate::runtime::RuntimeCoordinator;

/// The models picker's column constraints: MODEL, CTX, PRICE, the active
/// marker, and the configured marker. Shared with the width unit test so the
/// asserted budgets are the rendered ones (task 5f57acbb).
pub(super) const MODEL_PICKER_COLUMNS: [Constraint; 5] = [
    Constraint::Min(20),
    Constraint::Length(6),
    Constraint::Length(16),
    Constraint::Length(1),
    Constraint::Length(1),
];

/// Column spacing of the models picker table, matching the other pickers.
pub(super) const MODEL_PICKER_COLUMN_SPACING: u16 = 1;

/// The column the models picker's identity text starts at, counted from the
/// popup's inner edge: the two-cell selection gutter the table reserves for
/// every row plus the two-space indent the identity cell carries. The footer
/// text and the provider headers start on it too, so the popup has one left
/// edge.
const MODEL_PICKER_TEXT_INDENT: u16 = 4;

/// The column the agent and session tables' first cell starts at, counted from
/// the popup's inner edge: the two-cell selection gutter the table reserves for
/// every row. The agent and session footers start on it too.
const LIST_TEXT_INDENT: u16 = 2;

impl RuntimeCoordinator {
    pub(crate) fn render_command_palette(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::CommandPalette);
        // The modal frame insets the popup by one cell per side, so the table
        // width is known before the frame renders and the model's cell budgets
        // come from the widths the table solver resolves for it.
        let table_width = popup.width.saturating_sub(2);
        let popup_height = popup.height;

        let model = command_palette_table_model(&self.state, table_width, popup_height);

        let inner = render_modal_frame(
            frame,
            popup,
            command_palette_title(model.overflow_cue.as_deref(), popup.width),
            &self.theme,
        );
        let rows = popup_body_rows(inner, 0);

        super::render_popup_hint_row(frame, rows[0], model.query_line.clone().into(), &self.theme);

        let table_rows = model
            .rows
            .iter()
            .map(|row| Row::new(vec![Cell::from(row[0].clone()), Cell::from(row[1].clone())]));

        render_picker_table(
            frame,
            rows[1],
            Row::new(model.columns.clone()),
            table_rows,
            PickerTableLayout {
                columns: &COMMAND_PALETTE_COLUMNS,
                spacing: COMMAND_PALETTE_COLUMN_SPACING,
            },
            model.selected,
            &self.theme,
        );
    }

    pub(crate) fn render_info_panel(&self, frame: &mut Frame, area: Rect) {
        let Some(panel) = self.state.info_panel else {
            return;
        };
        let popup = modal_rect_for_panel(
            area,
            match panel {
                InfoPanel::Help => ModalPanelKind::Help,
                InfoPanel::Status => ModalPanelKind::Status,
                InfoPanel::Mcps => ModalPanelKind::Mcps,
                InfoPanel::Skills => ModalPanelKind::Skills,
            },
        );

        match panel {
            InfoPanel::Mcps => {
                let inner = popup.inner(Margin {
                    vertical: 1,
                    horizontal: 1,
                });
                let details_height = mcp_details_height_for_inner_height(inner.height);
                // The details rect is sized to the rows that actually render, so
                // the pane's last row is the popup's bottom inner row and the
                // table takes every row above it. The ladder value is the cap.
                let details_lines =
                    mcp_selected_details_lines(&self.state, details_height, inner.width);
                let details = mcp_details_rows(&details_lines, inner.width, details_height);
                let details_rows = details.len() as u16;
                // One blank row above the details keeps the table's last row off
                // the details' first line, the same gap the picker footer
                // carries. An empty pane claims no rows at all.
                let details_gap = u16::from(details_rows > 0);
                let details_rows_with_gap = details_rows + details_gap;

                let rows = popup_body_rows(inner, details_rows_with_gap);

                let model = mcp_table_model(&self.state, rows[1].width, rows[1].height);
                let title = mcp_panel_title(model.overflow_cue.as_deref(), popup.width);
                render_modal_frame(frame, popup, title, &self.theme);

                super::render_popup_hint_row(
                    frame,
                    rows[0],
                    Line::from(model.controls_line.clone()),
                    &self.theme,
                );

                let table_rows = model.rows.iter().map(|row| {
                    Row::new(vec![
                        Cell::from(row[0].clone()),
                        Cell::from(row[1].clone()),
                        Cell::from(row[2].clone()),
                    ])
                });
                render_picker_table(
                    frame,
                    rows[1],
                    Row::new(model.columns.clone()),
                    table_rows,
                    PickerTableLayout {
                        columns: &MCP_TABLE_COLUMNS,
                        spacing: MCP_TABLE_COLUMN_SPACING,
                    },
                    model.selected,
                    &self.theme,
                );

                if !details.is_empty() {
                    // The pane renders into the bottom `details_rows` of its rect,
                    // so the gap row above it stays base background.
                    let details_rect = Rect {
                        y: rows[2].y.saturating_add(details_gap),
                        height: details_rows,
                        ..rows[2]
                    };
                    frame.render_widget(Paragraph::new(Text::from(details)), details_rect);
                }
            }
            _ => {
                let panel_inner_height = popup.height.saturating_sub(2);
                let panel_inner_width = popup.width.saturating_sub(2);
                let (title, lines) = match panel {
                    InfoPanel::Help => help_panel_lines(&self.theme, panel_inner_width),
                    InfoPanel::Status => status_panel_lines(&self.state, panel_inner_width),
                    InfoPanel::Skills => skills_panel_lines(&self.state, panel_inner_width),
                    InfoPanel::Mcps => (
                        "MCPs",
                        // Handled by the dedicated Mcps arm above; empty is never rendered.
                        Vec::new(),
                    ),
                };

                let panel_scroll = self
                    .state
                    .info_panel_scroll
                    .min(help_panel_max_scroll(&lines, panel_inner_height));
                let panel_title = scroll_panel_title(
                    title,
                    &help_panel_overflow_cue(&lines, panel_inner_height, panel_scroll),
                    popup.width,
                );

                render_scroll_text_panel(
                    frame,
                    popup,
                    panel_title,
                    Text::from(lines),
                    panel_scroll,
                    &self.theme,
                );
            }
        }
    }

    pub(crate) fn render_model_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Models);
        let inner = render_modal_frame(
            frame,
            popup,
            popup_title("Models", Some(picker_action_keys()), None, popup.width),
            &self.theme,
        );
        let Some(picker_state) = self.state.picker.active_state() else {
            return;
        };
        let options = picker_state.filtered();
        // The footer component wraps the selected text, caps it at the ladder
        // value, and bottom-pins its rect against the inner area's last row.
        let reserved_height = picker_details_height_for_inner_height(inner.height);
        let footer_width = popup.width.saturating_sub(2);
        let footer = PickerFooter::measure(
            options
                .get(picker_state.selection)
                .and_then(|selected| match &selected.payload {
                    PickerPayload::Model { identity, .. } => Some(identity.as_str()),
                    _ => None,
                }),
            MODEL_PICKER_TEXT_INDENT,
            footer_width,
            reserved_height,
        );
        let rows = popup_body_rows(inner, footer.height());
        super::render_popup_hint_row(
            frame,
            rows[0],
            Line::from(picker_query_line(&picker_state.query)),
            &self.theme,
        );

        if options.is_empty() {
            super::render_empty_state(
                frame,
                rows[1],
                MODEL_PICKER_EMPTY_STATE_MESSAGE,
                &self.theme,
            );
        } else {
            // Build rows with provider headers
            // The MODEL column takes the table remainder, so its width comes from
            // the constraints the table solver resolves, the same way the agents
            // picker derives its cell budgets.
            let budgets = picker_column_budgets(
                rows[1].width,
                UnicodeWidthStr::width(PICKER_HIGHLIGHT_SYMBOL) as u16,
                MODEL_PICKER_COLUMN_SPACING,
                &MODEL_PICKER_COLUMNS,
            );
            // The identity cell carries a 2-space indent inside its column, so its
            // text budget is the MODEL column width minus that indent.
            let identity_budget = budgets
                .first()
                .copied()
                .unwrap_or_default()
                .saturating_sub(usize::from(MODEL_PICKER_TEXT_INDENT - LIST_TEXT_INDENT));
            let mut table_rows: Vec<Row> = Vec::new();
            let mut current_provider: Option<String> = None;
            let mut selected_row = 0;
            for (model_idx, opt) in options.iter().enumerate() {
                let (provider, provider_display_name, identity, configured, context_window, cost) =
                    match &opt.payload {
                        PickerPayload::Model {
                            provider,
                            provider_display_name,
                            identity,
                            configured,
                            context_window,
                            cost,
                        } => (
                            provider,
                            provider_display_name,
                            identity,
                            *configured,
                            *context_window,
                            cost,
                        ),
                        _ => unreachable!(),
                    };
                if current_provider.as_deref() != Some(provider.as_str()) {
                    let label = format!("  {provider_display_name}");
                    table_rows.push(section_header_row(&label, &self.theme));
                    current_provider = Some(provider.clone());
                }
                let active = if opt.id == self.state.status.identity.active_model_identity {
                    "*"
                } else {
                    ""
                };
                // A row is unavailable when the session's consumed tokens do not
                // fit its context window. The active model is not a switch
                // target, so it never dims.
                let unavailable = active.is_empty()
                    && self
                        .state
                        .status
                        .tokens
                        .latest_total_tokens
                        .zip(context_window.map(u64::from))
                        .is_some_and(|(consumed, window)| consumed >= window);
                let configured_glyph = if configured { "◆" } else { "" };
                let context_cell = context_window
                    .map(|n| format!("{}k", n / 1000))
                    .unwrap_or_default();
                let cost_cell = cost.clone().unwrap_or_default();
                let mut row = Row::new(vec![
                    Cell::from(format!("  {}", ellipsize(identity, identity_budget))),
                    Cell::from(Line::from(context_cell).right_aligned()),
                    Cell::from(Line::from(cost_cell).right_aligned()),
                    Cell::from(active),
                    Cell::from(configured_glyph),
                ]);
                if unavailable {
                    row = row.style(self.theme.picker_row_unavailable);
                }
                table_rows.push(row);
                if model_idx == picker_state.selection {
                    selected_row = table_rows.len() - 1;
                }
            }

            let header = Row::new(vec![
                Cell::from("MODEL"),
                Cell::from(Line::from("CTX").right_aligned()),
                Cell::from(Line::from("PRICE").right_aligned()),
                Cell::from(""),
                Cell::from(""),
            ]);
            render_picker_table(
                frame,
                rows[1],
                header,
                table_rows,
                PickerTableLayout {
                    columns: &MODEL_PICKER_COLUMNS,
                    spacing: MODEL_PICKER_COLUMN_SPACING,
                },
                Some(selected_row),
                &self.theme,
            );
        }

        // The ladder omits the bottom band when the footer claims no rows.
        if let Some(footer_rect) = rows.get(2) {
            footer.render(frame, *footer_rect, &self.theme);
        }
    }

    pub(crate) fn render_agent_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Agents);
        let inner = render_modal_frame(
            frame,
            popup,
            popup_title("Agent", Some(picker_action_keys()), None, popup.width),
            &self.theme,
        );
        let Some(picker_state) = self.state.picker.active_state() else {
            return;
        };
        let options = picker_state.filtered();
        let reserved_height = picker_details_height_for_inner_height(inner.height);
        let footer_width = popup.width.saturating_sub(2);
        let footer_text = options
            .get(picker_state.selection)
            .and_then(|selected| match &selected.payload {
                PickerPayload::Agent { name, description } => Some(match description {
                    Some(description) => format!("{name} — {description}"),
                    None => name.clone(),
                }),
                _ => None,
            });
        let footer = PickerFooter::measure(
            footer_text.as_deref(),
            LIST_TEXT_INDENT,
            footer_width,
            reserved_height,
        );
        let rows = popup_body_rows(inner, footer.height());
        super::render_popup_hint_row(
            frame,
            rows[0],
            Line::from(picker_query_line(&picker_state.query)),
            &self.theme,
        );

        if options.is_empty() {
            super::render_empty_state(
                frame,
                rows[1],
                AGENT_PICKER_EMPTY_STATE_MESSAGE,
                &self.theme,
            );
        } else {
            // The table resolves a selection column, the column spacing, and
            // then the constraints, so derive each cell budget from the width
            // the solver gives that column instead of assuming the two Min
            // columns equalize: below a 46-column table the Description
            // minimum pins that column and the name column takes the remainder.
            let columns = [
                Constraint::Min(12),
                Constraint::Min(20),
                Constraint::Length(1),
            ];
            let selection_width = 2;
            let column_spacing = 1;
            let budgets =
                picker_column_budgets(rows[1].width, selection_width, column_spacing, &columns);
            let name_budget = budgets.first().copied().unwrap_or_default();
            let description_budget = budgets.get(1).copied().unwrap_or_default();
            let table_rows: Vec<Row> = options
                .iter()
                .map(|opt| {
                    let (name, description) = match &opt.payload {
                        PickerPayload::Agent { name, description } => (name, description),
                        _ => unreachable!(),
                    };
                    let active = if Some(opt.id.as_str())
                        == self.state.status.identity.active_agent_identity.as_deref()
                    {
                        "*"
                    } else {
                        ""
                    };
                    Row::new(vec![
                        Cell::from(ellipsize(name, name_budget)),
                        Cell::from(ellipsize(
                            description.as_deref().unwrap_or_default(),
                            description_budget,
                        )),
                        Cell::from(active),
                    ])
                })
                .collect();
            render_picker_table(
                frame,
                rows[1],
                Row::new(vec!["Agent", "Description", "A"]),
                table_rows,
                PickerTableLayout {
                    columns: &columns,
                    spacing: column_spacing,
                },
                Some(picker_state.selection),
                &self.theme,
            );
        }

        // The ladder omits the bottom band when the footer claims no rows.
        if let Some(footer_rect) = rows.get(2) {
            footer.render(frame, *footer_rect, &self.theme);
        }
    }

    pub(crate) fn render_session_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Sessions);
        let reserved_height =
            picker_details_height_for_inner_height(popup.height.saturating_sub(2));
        let footer_width = popup.width.saturating_sub(2);
        // The table model builds the title cue from its row budget, so the
        // footer height resolves before the model call.
        let footer_text = self.state.picker.active_state().and_then(|picker_state| {
            let options = picker_state.filtered();
            let selected = options.get(picker_state.selection)?;
            match &selected.payload {
                PickerPayload::Session { title, .. } => {
                    Some(title.clone().unwrap_or_else(|| "(untitled)".to_string()))
                }
                _ => None,
            }
        });
        let footer = PickerFooter::measure(
            footer_text.as_deref(),
            LIST_TEXT_INDENT,
            footer_width,
            reserved_height,
        );
        let model = crate::runtime::session_picker::session_picker_table_model(
            &self.state,
            popup.height.saturating_sub(footer.height()),
            popup.width,
        );
        let title = popup_title(
            "Sessions",
            Some(picker_action_keys()),
            model.overflow_cue.as_deref(),
            popup.width,
        );
        let inner = render_modal_frame(frame, popup, title, &self.theme);
        let rows = popup_body_rows(inner, footer.height());
        super::render_popup_hint_row(frame, rows[0], model.query_line.clone(), &self.theme);

        if model.rows.is_empty() {
            super::render_empty_state(
                frame,
                rows[1],
                crate::runtime::session_picker::SESSION_PICKER_EMPTY_STATE_MESSAGE,
                &self.theme,
            );
        } else {
            let header = Row::new(vec!["When", "Title", "Msgs"]);
            let table_rows = model.rows.iter().map(|row| match row {
                SessionPickerRow::DayHeader(label) => section_header_row(label, &self.theme),
                SessionPickerRow::Session {
                    relative,
                    title,
                    message_count,
                } => Row::new(vec![
                    Cell::from(relative.clone()),
                    Cell::from(title.clone()),
                    Cell::from(message_count.to_string()),
                ]),
            });
            let columns = [
                // Wide enough for the longest day header ("Sep 30, 2025", 12
                // cells) on the When column.
                Constraint::Length(13),
                Constraint::Min(20),
                Constraint::Length(6),
            ];
            render_picker_table(
                frame,
                rows[1],
                header,
                table_rows,
                PickerTableLayout {
                    columns: &columns,
                    spacing: 1,
                },
                model.selected,
                &self.theme,
            );
        }

        // The ladder omits the bottom band when the footer claims no rows.
        if let Some(footer_rect) = rows.get(2) {
            footer.render(frame, *footer_rect, &self.theme);
        }
    }

    pub(crate) fn render_theme_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Themes);
        let inner = render_modal_frame(
            frame,
            popup,
            popup_title("Theme", Some(picker_action_keys()), None, popup.width),
            &self.theme,
        );
        let rows = popup_body_rows(inner, 0);
        super::render_popup_hint_row(
            frame,
            rows[0],
            Line::from("Select a color theme"),
            &self.theme,
        );

        let Some(picker_state) = self.state.picker.active_state() else {
            return;
        };
        let options = &picker_state.options;
        if options.is_empty() {
            super::render_empty_state(
                frame,
                rows[1],
                THEME_PICKER_EMPTY_STATE_MESSAGE,
                &self.theme,
            );
        } else {
            // The Theme column absorbs the width left over by the selection
            // column and the column spacing.
            let theme_budget = rows[1].width.saturating_sub(3) as usize;
            let table_rows = options
                .iter()
                .map(|opt| Row::new(vec![Cell::from(ellipsize(&opt.display, theme_budget))]));
            render_picker_table(
                frame,
                rows[1],
                Row::new(vec!["Theme"]),
                table_rows,
                PickerTableLayout {
                    columns: &[Constraint::Min(12)],
                    spacing: 1,
                },
                Some(picker_state.selection),
                &self.theme,
            );
        }
    }
}
