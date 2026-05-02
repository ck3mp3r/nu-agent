use crate::commands::agent::ui::{
    event::UiEvent,
    policy::{UiPolicy, Verbosity},
    renderer::UiRenderer,
    stderr::StderrUiRenderer,
};

#[test]
fn stderr_renderer_writes_only_to_stderr_sink() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmEnd {
        response_chars: 5,
        tool_calls: 0,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.trim().is_empty());
}

#[test]
fn quiet_mode_suppresses_non_essential_progress_but_keeps_warnings() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.emit(&UiEvent::Warning {
        message: "essential warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("completed"));
    assert!(stderr_out.contains("essential warning"));
}

#[test]
fn default_busy_flow_uses_spinner_without_redundant_persistent_busy_lines() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmEnd {
        response_chars: 42,
        tool_calls: 0,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("response ready"));
    assert!(!stderr_out.contains("response chars="));
}

#[test]
fn spinner_is_disabled_on_non_tty_or_quiet_and_enabled_on_interactive_tty() {
    let renderer_non_tty = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    assert!(!renderer_non_tty.spinner_enabled_for_test());

    let renderer_quiet = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        true,
    );
    assert!(!renderer_quiet.spinner_enabled_for_test());

    let renderer_tty = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );
    assert!(renderer_tty.spinner_enabled_for_test());
}

#[test]
fn spinner_pauses_for_persistent_lines_and_stops_on_completion() {
    let mut renderer = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStart);
    assert!(renderer.spinner_active_for_test());

    renderer.emit(&UiEvent::Tick);

    renderer.emit(&UiEvent::ToolStart {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
    });
    assert!(renderer.spinner_active_for_test());
    assert!(!renderer.spinner_suspended_for_test());

    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    assert!(!renderer.spinner_active_for_test());
}

#[test]
fn default_tool_lifecycle_is_single_completion_line_with_result_block() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::ToolStart {
        name: "gh__list_prs".to_string(),
        source: "mcp".to_string(),
        arguments: "{}".to_string(),
    });
    renderer.emit(&UiEvent::ToolEnd {
        name: "gh__list_prs".to_string(),
        source: "mcp".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "[]".to_string(),
        error_kind: None,
        message: None,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("✓ tool gh__list_prs args={}"));
    assert!(stderr_out.contains("\n[]"));
    assert!(!stderr_out.contains("→ tool gh__list_prs"));
}

#[test]
fn default_tool_lifecycle_prints_non_empty_payloads() {
    for payload in ["[]", "{}", "null", ""] {
        let mut stderr_bytes = Vec::<u8>::new();
        let mut renderer = StderrUiRenderer::new(
            &mut stderr_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );

        renderer.emit(&UiEvent::ToolStart {
            name: "gh__list_prs".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        });
        renderer.emit(&UiEvent::ToolEnd {
            name: "gh__list_prs".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: payload.to_string(),
            error_kind: None,
            message: None,
        });
        renderer.flush();

        let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
        assert!(stderr_out.contains("✓ tool gh__list_prs args={}"));
        if payload.is_empty() {
            assert!(!stderr_out.contains("\n[]"));
            assert!(!stderr_out.contains("\n{}"));
            assert!(!stderr_out.contains("\nnull"));
        } else {
            assert!(stderr_out.contains(&format!("\n{payload}")));
        }
    }
}

#[test]
fn spinner_tick_advances_frame_on_tty_only() {
    let mut tty_renderer = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );
    tty_renderer.emit(&UiEvent::LlmStart);
    let frame_before = tty_renderer.spinner_frame_for_test().to_string();
    tty_renderer.emit(&UiEvent::Tick);
    let frame_after = tty_renderer.spinner_frame_for_test().to_string();
    assert_ne!(frame_before, frame_after);

    let mut non_tty_renderer = StderrUiRenderer::new(
        Vec::<u8>::new(),
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    non_tty_renderer.emit(&UiEvent::LlmStart);
    non_tty_renderer.emit(&UiEvent::Tick);
    assert!(!non_tty_renderer.spinner_active_for_test());
}
