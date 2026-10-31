use agnt_core::protocol::event::PermissionDecision;
use agnt_core::protocol::events::{Key, SystemEvent};

use crate::{
    interaction::{cancel::CancelController, input::TerminalEvent},
    state::{
        AppState, InfoPanel, InputMode, PANEL_PAGE_LINES, PickerKeyOutcome, ScrollAction,
        SubmitAction,
    },
};

pub fn dispatch_terminal_event(
    state: &mut AppState,
    event: &TerminalEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    state.reduce_terminal_event(event, cancel_controller)
}

/// What the input-boundary lens does with a key.
///
/// The lens owns the surfaces that sit above the reducer's keymap — an open
/// info panel, an open picker, the permission card, the agent-cycle chord. It
/// reports the outcome instead of rewriting an action, so the reducer applies
/// the behavior against its own state.
pub(crate) enum KeyLens {
    /// The lens handled the key; the payload reports whether state changed.
    Consumed(bool),
    /// The card's scroll keys: the reducer applies this scroll action.
    Scroll(ScrollAction),
    /// The picker resolved a submit action.
    PickerSubmit(SubmitAction),
    /// The card's decision keys become a system event.
    Event(Box<SystemEvent>),
    /// The panel lens resolves Ctrl-C to a quit.
    Quit,
    /// Escape without the busy-mode escalation: the panel lens closes itself
    /// and the key keeps the plain escape behavior.
    Escape,
    /// Not the lens's key: the reducer keymap handles it.
    Pass,
}

/// Run the pre-keymap lenses in their fixed order: info panel, picker, the
/// permission card, the agent-cycle chord. Everything else is the reducer's.
pub(crate) fn rewrite_action(state: &mut AppState, key: Key) -> KeyLens {
    if let Some(panel) = state.info_panel {
        return rewrite_panel_key(state, panel, key);
    }

    if state.picker.active().is_some() {
        match state.picker.handle_key(key) {
            PickerKeyOutcome::Consumed(changed) => return KeyLens::Consumed(changed),
            PickerKeyOutcome::Submit(submit) => return KeyLens::PickerSubmit(submit),
            PickerKeyOutcome::Quit => return KeyLens::Quit,
            PickerKeyOutcome::Pass => {}
        }
    }

    // The card owns its decision keys, but not Esc: Esc falls through to the
    // busy-mode escalation and the keymap below, so it exits insert mode or
    // arms the turn-cancel confirmation instead of denying.
    if state.permission.has_prompt() && key != Key::Esc {
        return rewrite_card_key(state, key);
    }

    // Tab cycles agents in insert mode
    if key == Key::Tab && state.input.mode == InputMode::Insert && state.has_agents_to_cycle() {
        state.queue_cycle_agent_request();
        return KeyLens::Consumed(true);
    }

    KeyLens::Pass
}

/// The info-panel lens: the panel keeps its own scroll cursor and, for the MCP
/// panel, its query. The frame repaints only while the dirty flag is set, so a
/// mutation reports whether it changed what the panel draws — a move that wraps
/// onto the same row changes nothing.
fn rewrite_panel_key(state: &mut AppState, panel: InfoPanel, key: Key) -> KeyLens {
    if panel != InfoPanel::Mcps {
        return match key {
            Key::Esc => KeyLens::Escape,
            Key::CtrlC => KeyLens::Quit,
            Key::Up | Key::CtrlP | Key::CtrlK => {
                state.info_panel_scroll = state.info_panel_scroll.saturating_sub(1);
                KeyLens::Consumed(true)
            }
            Key::Down | Key::CtrlN | Key::CtrlJ => {
                state.info_panel_scroll = state.info_panel_scroll.saturating_add(1);
                KeyLens::Consumed(true)
            }
            Key::PageUp | Key::CtrlU => {
                state.info_panel_scroll = state.info_panel_scroll.saturating_sub(PANEL_PAGE_LINES);
                KeyLens::Consumed(true)
            }
            Key::PageDown | Key::CtrlD => {
                state.info_panel_scroll = state.info_panel_scroll.saturating_add(PANEL_PAGE_LINES);
                KeyLens::Consumed(true)
            }
            _ => KeyLens::Consumed(false),
        };
    }

    match key {
        Key::Esc => {
            state.status.mcp.clear_mcp_panel_query();
            KeyLens::Escape
        }
        Key::CtrlC => KeyLens::Quit,
        Key::Up | Key::CtrlP | Key::CtrlK => {
            let before = state.status.mcp.mcp_panel_selection;
            state.status.mcp.mcp_panel_move_up();
            KeyLens::Consumed(state.status.mcp.mcp_panel_selection != before)
        }
        Key::Down | Key::CtrlN | Key::CtrlJ => {
            let before = state.status.mcp.mcp_panel_selection;
            state.status.mcp.mcp_panel_move_down();
            KeyLens::Consumed(state.status.mcp.mcp_panel_selection != before)
        }
        Key::PageUp | Key::CtrlU => {
            let before = state.status.mcp.mcp_panel_selection;
            state.status.mcp.mcp_panel_page_up();
            KeyLens::Consumed(state.status.mcp.mcp_panel_selection != before)
        }
        Key::PageDown | Key::CtrlD => {
            let before = state.status.mcp.mcp_panel_selection;
            state.status.mcp.mcp_panel_page_down();
            KeyLens::Consumed(state.status.mcp.mcp_panel_selection != before)
        }
        Key::Enter | Key::Char(' ') => {
            KeyLens::Consumed(state.status.mcp.queue_selected_mcp_toggle_request())
        }
        // Every other character filters the panel, as it does in the palette and
        // the pickers. Space stays with the toggle.
        Key::Char(ch) => {
            state.status.mcp.append_mcp_panel_query_char(ch);
            KeyLens::Consumed(true)
        }
        Key::Backspace => KeyLens::Consumed(state.status.mcp.backspace_mcp_panel_query_char()),
        _ => KeyLens::Consumed(false),
    }
}

/// The permission card's decision and scroll keys. Every other key is consumed
/// without effect: the card owns the input while it is up, so nothing leaks
/// into the textarea and no decision fires.
fn rewrite_card_key(state: &AppState, key: Key) -> KeyLens {
    let select = state.input.mode == InputMode::Visual;
    match key {
        Key::Char('a') => KeyLens::Event(Box::new(SystemEvent::PermissionDecided {
            decision: PermissionDecision::AllowOnce,
        })),
        Key::Char('A') => KeyLens::Event(Box::new(SystemEvent::PermissionDecided {
            decision: PermissionDecision::AllowAlways,
        })),
        Key::Char('d') => KeyLens::Event(Box::new(SystemEvent::PermissionDecided {
            decision: PermissionDecision::Deny,
        })),
        Key::Up => KeyLens::Scroll(ScrollAction::LineUp { select }),
        Key::Down => KeyLens::Scroll(ScrollAction::LineDown { select }),
        Key::PageUp | Key::CtrlU => KeyLens::Scroll(ScrollAction::PageUp { select }),
        Key::PageDown | Key::CtrlD => KeyLens::Scroll(ScrollAction::PageDown { select }),
        _ => KeyLens::Consumed(true),
    }
}
