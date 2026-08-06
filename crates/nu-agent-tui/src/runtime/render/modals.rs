use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    text::{Line, Text},
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
};

use crate::runtime::panels::{
    AGENT_PICKER_EMPTY_STATE_MESSAGE, MCP_STATUS_COLUMN_WIDTH, MODEL_PICKER_EMPTY_STATE_MESSAGE,
};
use crate::runtime::{
    command_palette_table_model, command_palette_title, help_panel_lines, help_panel_max_scroll,
    help_panel_overflow_cue, mcp_details_height_for_inner_height, mcp_panel_controls_line,
    mcp_selected_details_lines, mcp_table_model, skills_panel_lines, status_panel_lines,
};
use crate::{
    runtime::{
        render::{render_modal_frame, render_scroll_text_panel},
        render_frame::{ModalPanelKind, modal_rect_for_panel},
    },
    state::InfoPanel,
};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_command_palette(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::CommandPalette);
        let popup_width = popup.width;
        let popup_height = popup.height;

        let model = command_palette_table_model(&self.state, popup_width, popup_height);

        let inner = render_modal_frame(
            frame,
            popup,
            command_palette_title(model.overflow_cue.as_deref()),
        );
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(Line::from(model.query_line.clone())),
            rows[0],
        );

        let header = Row::new(vec!["Action", "Summary"]);

        let table_rows = model
            .rows
            .iter()
            .map(|row| Row::new(vec![Cell::from(row[0].clone()), Cell::from(row[1].clone())]));

        let widths = vec![Constraint::Length(8), Constraint::Min(12)];

        let table = Table::new(table_rows, widths)
            .header(header)
            .column_spacing(2)
            .highlight_symbol("❯ ");
        let mut table_state = TableState::default();
        table_state.select(model.selected);
        frame.render_stateful_widget(table, rows[1], &mut table_state);
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

                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Min(1),
                        Constraint::Length(details_height),
                    ])
                    .split(inner);

                let model = mcp_table_model(&self.state, rows[1].height);
                let title = if let Some(cue) = model.overflow_cue.as_deref() {
                    format!("MCPs ({cue})")
                } else {
                    "MCPs".to_string()
                };
                render_modal_frame(frame, popup, title);

                frame.render_widget(
                    Paragraph::new(Line::from(mcp_panel_controls_line())),
                    rows[0],
                );

                let header = Row::new(model.columns.clone());
                let table_rows = model.rows.iter().map(|row| {
                    Row::new(vec![
                        Cell::from(row[0].clone()),
                        Cell::from(row[1].clone()),
                        Cell::from(row[2].clone()),
                    ])
                });
                let widths = [
                    Constraint::Length(18),
                    Constraint::Length(14),
                    Constraint::Length(MCP_STATUS_COLUMN_WIDTH),
                ];
                let table = Table::new(table_rows, widths)
                    .header(header)
                    .column_spacing(1)
                    .highlight_symbol("❯ ");
                let mut table_state = TableState::default();
                table_state.select(model.selected);
                frame.render_stateful_widget(table, rows[1], &mut table_state);

                if details_height > 0 {
                    let details_lines =
                        mcp_selected_details_lines(&self.state, details_height, rows[2].width);
                    if !details_lines.is_empty() {
                        let details_widget =
                            Paragraph::new(Text::from(details_lines)).wrap(Wrap { trim: false });
                        frame.render_widget(details_widget, rows[2]);
                    }
                }
            }
            _ => {
                let (title, lines) = match panel {
                    InfoPanel::Help => help_panel_lines(),
                    InfoPanel::Status => {
                        status_panel_lines(&self.state, &self.active_model_identity)
                    }
                    InfoPanel::Skills => skills_panel_lines(&self.state),
                    InfoPanel::Mcps => unreachable!("handled above"),
                };

                let panel_inner_height = popup.height.saturating_sub(2);
                let panel_inner_width = popup.width.saturating_sub(2);
                let panel_scroll = self.state.info_panel_scroll.min(help_panel_max_scroll(
                    &lines,
                    panel_inner_height,
                    panel_inner_width,
                ));
                let panel_title = match panel {
                    InfoPanel::Help => {
                        if let Some(cue) = help_panel_overflow_cue(
                            &lines,
                            panel_inner_height,
                            panel_inner_width,
                            panel_scroll,
                        ) {
                            format!("{title} ({cue})")
                        } else {
                            title.to_string()
                        }
                    }
                    _ => title.to_string(),
                };

                render_scroll_text_panel(
                    frame,
                    popup,
                    panel_title,
                    Text::from(lines),
                    panel_scroll,
                );
            }
        }
    }

    pub(crate) fn render_model_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Models);
        let inner = render_modal_frame(frame, popup, "Models (↑/↓ or Ctrl-N · Enter · Esc)");
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                "Query: {}",
                self.state.model_picker_query
            ))),
            rows[0],
        );

        let options = self.state.model_picker_filtered_options();
        if options.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(MODEL_PICKER_EMPTY_STATE_MESSAGE)),
                rows[1],
            );
        } else {
            let table_rows = options.iter().map(|option| {
                let active = if option.active { "*" } else { "" };
                Row::new(vec![
                    Cell::from(option.identity.clone()),
                    Cell::from(active.to_string()),
                ])
            });
            let table = Table::new(table_rows, [Constraint::Min(12), Constraint::Length(1)])
                .header(Row::new(vec!["Model", "A"]))
                .column_spacing(1)
                .highlight_symbol("❯ ");
            let mut table_state = TableState::default();
            table_state.select(Some(self.state.model_picker_selection));
            frame.render_stateful_widget(table, rows[1], &mut table_state);
        }
    }

    pub(crate) fn render_agent_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Agents);
        let inner = render_modal_frame(frame, popup, "Agent (↑/↓ · Enter · Esc)");
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                "Query: {}",
                self.state.agent_picker_query
            ))),
            rows[0],
        );

        let options = self.state.agent_picker_filtered_options();
        if options.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(AGENT_PICKER_EMPTY_STATE_MESSAGE)),
                rows[1],
            );
        } else {
            let table_rows: Vec<Row> = options
                .iter()
                .map(|option| {
                    let active = if option.active { "*" } else { "" };
                    let desc = option.description.as_deref().unwrap_or("");
                    Row::new(vec![
                        Cell::from(option.name.clone()),
                        Cell::from(desc.to_string()),
                        Cell::from(active.to_string()),
                    ])
                })
                .collect();
            let table = Table::new(
                table_rows,
                [
                    Constraint::Min(12),
                    Constraint::Min(20),
                    Constraint::Length(1),
                ],
            )
            .header(Row::new(vec!["Agent", "Description", "A"]))
            .column_spacing(1)
            .highlight_symbol("❯ ");
            let mut table_state = TableState::default();
            table_state.select(Some(self.state.agent_picker_selection));
            frame.render_stateful_widget(table, rows[1], &mut table_state);
        }
    }

    pub(crate) fn render_session_picker(&self, frame: &mut Frame, area: Rect) {
        let popup = modal_rect_for_panel(area, ModalPanelKind::Sessions);
        let model =
            crate::runtime::session_picker::session_picker_table_model(&self.state, popup.height);
        let title = if let Some(cue) = model.overflow_cue.as_deref() {
            format!("Sessions ({cue}) (↑/↓ · Enter · Esc)")
        } else {
            "Sessions (↑/↓ · Enter · Esc)".to_string()
        };
        let inner = render_modal_frame(frame, popup, title);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(Paragraph::new(model.query_line), rows[0]);

        if model.rows.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(
                    crate::runtime::session_picker::SESSION_PICKER_EMPTY_STATE_MESSAGE,
                )),
                rows[1],
            );
        } else {
            let header = Row::new(vec!["When", "Title", "ID"]);
            let table_rows = model.rows.iter().map(|row| {
                Row::new(vec![
                    Cell::from(row[0].clone()),
                    Cell::from(row[1].clone()),
                    Cell::from(row[2].clone()),
                ])
            });
            let table = Table::new(
                table_rows,
                [
                    Constraint::Length(10),
                    Constraint::Min(12),
                    Constraint::Length(15),
                ],
            )
            .header(header)
            .column_spacing(1)
            .highlight_symbol("❯ ");
            let mut table_state = TableState::default();
            table_state.select(model.selected);
            frame.render_stateful_widget(table, rows[1], &mut table_state);
        }
    }
}
