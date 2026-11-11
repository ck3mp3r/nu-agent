//! Runtime coordinator test suite.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

mod branch_resolver;
mod command_palette;
mod compaction;
mod ctrl_c_guard;
mod help_panel;
mod input_caret;
mod input_cursor;
mod input_events;
mod lane_2;
mod mcp_panel;
mod permission_lock;
mod pickers;
mod render_animation;
mod render_misc;
mod renderer_events;
mod replay_preview;
mod status_bar;
mod submit_cancel;
mod support;
mod terminal_restore;
mod transcript_hydration;

use support::*;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::runtime`. Sub-files
// now resolve `super::` against this module root, so the same helpers are
// re-exported here.
use crate::runtime::panels::*;
use crate::runtime::render;
use crate::runtime::status::help_test::*;

use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::rendering::theme::TuiTheme;
use crate::runtime::status::test::{init_repo_with_branch, run_git};
use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};
use crate::test_support::{
    compaction_completed, compaction_started, entry_text, inference_chunk, inference_started,
    markdown_fixture, open_command_palette_for_test, system_message, tool_completed, tool_started,
    turn_completed,
};
use crate::{
    interaction::{
        input::{TerminalEvent, TerminalKey},
        reducer::ESC_ABORT_CONFIRM_STATUS,
    },
    platform::safety::RestoreRunError,
    platform::terminal::{
        TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
    },
    runtime::{
        RuntimeCoordinator, RuntimeRunError, command_palette_table_model_for_test,
        cursor_style_for_test, help_panel_lines, help_panel_max_scroll_for_test,
        help_panel_overflow_cue_for_test, help_panel_visible_window_for_test,
        inline_slash_lines_for_test, input_line_for_test, input_line_for_test_at_millis,
        input_rows_with_prompt_for_test, mcp_table_model_for_test, run_with_terminal_restore_sync,
        status_panel_lines,
    },
    state::{
        ActivePicker, AppState, InputMode, InputState, McpServerUsabilityState, PickerOption,
        PickerPayload, PickerRenderKind, PromptStatus, TranscriptRole, UiPhase,
    },
};
use agnt_render::transcript::ir::MessageRole;
use agnt_render::transcript::ir::ToolName;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use agnt_runtime::protocol::event::PermissionDecision;
use agnt_runtime::protocol::events::{
    CompactionRecord, CompactionSource, SessionRecord, ToolCall, ToolOutcome,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
