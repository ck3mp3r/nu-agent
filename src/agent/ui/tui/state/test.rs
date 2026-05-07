use crate::agent::ui::tui::state::{
    AppState,
    InputMode,
    PaneFocus,
    PromptStatus,
    ToolCallStatus,
    TranscriptLineStatus,
    TranscriptRole,
    UiPhase,
};

#[test]
fn defaults_start_idle_with_unlocked_input_and_no_abort_pending() {
    let state = AppState::new();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
    assert!(state.input.buffer.is_empty());
    assert!(!state.abort.pending);
    assert_eq!(state.abort.confirmation_marker, 0);
    assert!(state.transcript_preview.is_empty());
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
    assert!(state.status_line.is_empty());
    assert_eq!(state.latest_input_tokens, None);
    assert_eq!(state.latest_output_tokens, None);
    assert_eq!(state.latest_total_tokens, None);
    assert_eq!(state.session_total_tokens, 0);
}

#[test]
fn submit_acceptance_clears_input_and_keeps_input_editable() {
    let mut state = AppState::new();
    for ch in "check cluster status".chars() {
        state.append_input_char(ch);
    }

    state.enqueue_prompt("check cluster status".to_string());

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input.locked);
    assert!(state.input.buffer.is_empty());
}

#[test]
fn non_idle_phase_keeps_input_editable_for_queueing() {
    let mut state = AppState::new();

    state.enqueue_prompt("one".to_string());
    assert!(!state.input.locked);
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Queued);

    let _ = state.activate_next_prompt();
    assert_eq!(state.active_prompt_id(), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    state.request_abort_confirmation();
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(!state.input.locked);
}

#[test]
fn abort_pending_requires_busy_context() {
    let mut state = AppState::new();
    assert!(!state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::Idle);

    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    assert!(state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
}

#[test]
fn finalize_resets_abort_pending_and_unlocks_input() {
    let mut state = AppState::new();
    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    let marker = state.abort.confirmation_marker;
    assert!(state.request_abort_confirmation());
    assert!(state.abort.confirmation_marker > marker);

    state.finalize_cycle();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
    assert!(!state.abort.pending);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
}

#[test]
fn prompt_queue_lifecycle_is_fifo_and_single_in_progress() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());

    assert_eq!(state.pending_prompt_ids().iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);

    let first = state.activate_next_prompt();
    assert_eq!(first, Some(1));
    assert_eq!(state.active_prompt_id(), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[1].status, PromptStatus::Queued);
    assert_eq!(state.prompt_items()[2].status, PromptStatus::Queued);

    state.complete_active_prompt();
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);

    let second = state.activate_next_prompt();
    assert_eq!(second, Some(2));
    assert_eq!(state.active_prompt_id(), Some(2));

    state.complete_active_prompt();
    let third = state.activate_next_prompt();
    assert_eq!(third, Some(3));
    state.complete_active_prompt();

    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![PromptStatus::Done, PromptStatus::Done, PromptStatus::Done]
    );
}

#[test]
fn global_abort_cancels_active_and_all_pending_prompts() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());
    let _ = state.activate_next_prompt();

    state.cancel_active_and_pending_prompts();

    assert_eq!(state.active_prompt_id(), None);
    assert!(state.pending_prompt_ids().is_empty());
    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![
            PromptStatus::Cancelled,
            PromptStatus::Cancelled,
            PromptStatus::Cancelled
        ]
    );
}

#[test]
fn input_cursor_and_edit_operations_handle_middle_insert_delete_and_backspace() {
    let mut state = AppState::new();
    for ch in ['a', 'c'] {
        state.append_input_char(ch);
    }

    state.move_cursor_left();
    state.append_input_char('b');
    assert_eq!(state.input.buffer, "abc");
    assert_eq!(state.input.cursor, 2);

    state.backspace_input_char();
    assert_eq!(state.input.buffer, "ac");
    assert_eq!(state.input.cursor, 1);

    state.delete_input_char();
    assert_eq!(state.input.buffer, "a");
    assert_eq!(state.input.cursor, 1);

    state.move_cursor_home();
    assert_eq!(state.input.cursor, 0);
    state.move_cursor_end();
    assert_eq!(state.input.cursor, state.input.buffer.len());
}

#[test]
fn transcript_scroll_follow_tail_pause_and_resume_behaves_as_expected() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }

    state.scroll_transcript_page_up(8);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 6);

    state.scroll_transcript_page_down(4);
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 4);

    state.scroll_transcript_page_down(4);
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
}

#[test]
fn turn_separator_is_inserted_between_user_and_assistant_turns() {
    let mut state = AppState::new();

    state.push_transcript_line(TranscriptRole::User, "prompt one");
    state.push_transcript_line(TranscriptRole::Assistant, "response one");

    assert_eq!(state.transcript_preview.len(), 3);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[1].role, TranscriptRole::Separator);
    assert_eq!(state.transcript_preview[2].role, TranscriptRole::Assistant);
}

#[test]
fn turn_separator_is_not_repeated_for_same_role_sequences() {
    let mut state = AppState::new();

    state.push_transcript_line(TranscriptRole::Assistant, "line one");
    state.push_transcript_line(TranscriptRole::Assistant, "line two");

    assert_eq!(
        state
            .transcript_preview
            .iter()
            .filter(|line| line.role == TranscriptRole::Separator)
            .count(),
        0
    );
}

#[test]
fn record_token_usage_tracks_latest_and_accumulates_session_total() {
    let mut state = AppState::new();

    state.record_token_usage(7, 5, 12);
    assert_eq!(state.latest_input_tokens, Some(7));
    assert_eq!(state.latest_output_tokens, Some(5));
    assert_eq!(state.latest_total_tokens, Some(12));
    assert_eq!(state.session_total_tokens, 12);

    state.record_token_usage(2, 3, 5);
    assert_eq!(state.latest_input_tokens, Some(2));
    assert_eq!(state.latest_output_tokens, Some(3));
    assert_eq!(state.latest_total_tokens, Some(5));
    assert_eq!(state.session_total_tokens, 17);
}

#[test]
fn tool_call_lifecycle_tracks_transcript_line_status_by_same_row() {
    let mut state = AppState::new();

    state.start_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#);

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::Tool);
    assert!(state.transcript_preview[0].text.contains("tool[k8s__list_pods] args="));
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress))
    );

    state.finish_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#, true);
    assert!(state.transcript_preview[0].text.contains("· done"));
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done))
    );
}

#[test]
fn mode_helpers_toggle_between_insert_and_normal() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input_mode, InputMode::Normal);

    state.enter_insert_mode();
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn line_scroll_helpers_adjust_follow_tail_consistently() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    state.scroll_transcript_line_up();
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    state.scroll_transcript_line_down();
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
}

#[test]
fn line_up_from_bottom_detaches_follow_tail_and_moves_cursor_up() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(9));

    state.scroll_transcript_line_up();

    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(8));
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
}

#[test]
fn cursor_moves_within_viewport_before_scroll_when_moving_up_from_bottom() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }

    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_cursor_index(), Some(9));
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    state.scroll_transcript_line_up();
    assert_eq!(state.transcript_cursor_index(), Some(8));
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    state.scroll_transcript_line_up();
    assert_eq!(state.transcript_cursor_index(), Some(7));
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    state.scroll_transcript_line_up();
    assert_eq!(state.transcript_cursor_index(), Some(6));
    assert_eq!(state.transcript_scroll_lines_from_bottom, 1);
}

#[test]
fn normal_mode_defaults_focus_to_transcript_and_insert_focuses_input() {
    let mut state = AppState::new();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.enter_normal_mode();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.enter_insert_mode();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}

#[test]
fn pane_focus_can_cycle_left_and_right() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.focus_prev_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}

#[test]
fn visual_mode_selects_range_and_queues_clipboard_text() {
    let mut state = AppState::new();
    state.push_transcript_line(TranscriptRole::User, "u1");
    state.push_transcript_line(TranscriptRole::Assistant, "a1");
    state.push_transcript_line(TranscriptRole::Assistant, "a2");

    state.enter_visual_mode();
    state.extend_visual_cursor_line_up();
    let selected = state.selected_transcript_range().expect("selection");
    assert!(selected.0 <= selected.1);

    state.queue_visual_selection_to_clipboard();
    let copied = state.take_clipboard_request().expect("clipboard payload");
    assert!(copied.contains("a2") || copied.contains("a1"));
}

#[test]
fn visual_mode_anchor_uses_current_viewport_cursor_for_gg_and_g_positions() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.enter_normal_mode();

    state.scroll_transcript_to_top();
    state.enter_visual_mode();
    assert_eq!(state.visual_anchor_index(), Some(0));
    assert_eq!(state.visual_cursor_index(), Some(0));
    assert_eq!(state.selected_transcript_range(), Some((0, 0)));

    state.enter_normal_mode();
    state.scroll_transcript_to_bottom();
    state.enter_visual_mode();
    assert_eq!(state.visual_anchor_index(), Some(9));
    assert_eq!(state.visual_cursor_index(), Some(9));
    assert_eq!(state.selected_transcript_range(), Some((9, 9)));
}

#[test]
fn scroll_to_top_clamps_to_max_scroll_and_allows_line_scroll_down() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }

    state.scroll_transcript_to_top();
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);
    assert!(!state.transcript_follow_tail);

    state.scroll_transcript_line_down();
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);
    assert!(!state.transcript_follow_tail);
}

#[test]
fn append_newline_insert_and_boundary_deletes_work_across_lines() {
    let mut state = AppState::new();
    for ch in "ab".chars() {
        state.append_input_char(ch);
    }
    state.append_input_char('\n');
    for ch in "cd".chars() {
        state.append_input_char(ch);
    }

    assert_eq!(state.input.buffer, "ab\ncd");
    assert_eq!(state.input.cursor, state.input.buffer.len());

    state.move_cursor_left();
    state.move_cursor_left();
    state.backspace_input_char();
    assert_eq!(state.input.buffer, "abcd");

    state.move_cursor_left();
    state.move_cursor_left();
    state.delete_input_char();
    assert_eq!(state.input.buffer, "bcd");
}

#[test]
fn assistant_projection_cache_reuses_projected_markdown_for_same_input() {
    let mut state = AppState::new();
    let markdown = "```rust\nfn main() {\n    let x = 42;\n}\n```";

    let first = state.project_assistant_markdown_lines(markdown);
    let second = state.project_assistant_markdown_lines(markdown);

    assert_eq!(state.assistant_projection_cache_misses(), 1);
    assert_eq!(first, second);
}
