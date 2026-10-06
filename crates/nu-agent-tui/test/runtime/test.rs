//! Runtime coordinator test suite.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

mod branch_resolver;
mod command_palette;
mod compaction;
mod help_panel;
mod input_cursor;
mod input_events;
mod lane_2;
mod mcp_panel;
mod pickers;
mod render_animation;
mod render_misc;
mod renderer_events;
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
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::rendering::layout::wrapped_input_rows;
use crate::rendering::theme::TuiTheme;
use crate::runtime::renderer_test::{CapturingRenderer, FakeRenderer};
use crate::runtime::status::test::{init_repo_with_branch, run_git};
use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};
use crate::test_support::{markdown_fixture, open_command_palette_for_test};
use crate::{
    interaction::input::{TerminalEvent, TerminalKey},
    platform::safety::RestoreRunError,
    platform::terminal::{
        TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
    },
    runtime::{
        InputSourceDiagnostics, RuntimeCoordinator, RuntimeRunError, ScriptedTerminalEvents,
        TerminalEventSource, TuiRuntimeRenderer, command_palette_table_model_for_test,
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
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nu_agent_core::orchestrator::{OrchestratorEvent, UiStateEvent};
use nu_agent_core::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use nu_agent_core::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplay, UiEvent,
};
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::transcript::ir::{BlockSource, MessageRole};
use nu_agent_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
