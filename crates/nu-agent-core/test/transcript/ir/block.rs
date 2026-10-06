use super::*;

// ── Block / BlockSource ──────────────────────────────────────────────────────

#[test]
fn block_construction_stores_source_lane_fill_status() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec
    let block = Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    };

    // -- Check
    assert_eq!(
        block.source,
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(block.lane, Lane::Marker("▏"));
    assert_eq!(block.fill, Fill::Full);
    assert!(block.status.is_none());
}

#[test]
fn block_holds_tool_source_with_call_line() -> Result<()> {
    // -- Setup & Fixtures
    let call = CallLine {
        summary: "→ {\"path\":\"a.rs\"}".to_string(),
    };

    // -- Exec
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("read".to_string()),
            call,
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(ItemStatus::InProgress),
    };

    // -- Check
    let BlockSource::Tool {
        name,
        call,
        preview,
    } = &block.source
    else {
        return Err("expected Tool source".into());
    };
    assert_eq!(name.0, "read");
    assert_eq!(call.summary, "→ {\"path\":\"a.rs\"}");
    assert!(preview.is_none());
    assert_eq!(block.status, Some(ItemStatus::InProgress));
    Ok(())
}

#[test]
fn block_holds_notice_banner_and_spacer_sources() {
    // -- Setup & Fixtures
    let sources = [
        BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted".to_string(),
        },
        BlockSource::Banner {
            text: "logo".to_string(),
        },
        BlockSource::Spacer,
    ];

    // -- Exec & Check
    assert!(
        matches!(
            sources[0],
            BlockSource::Notice {
                kind: NoticeKind::Compaction,
                ..
            }
        ),
        "first source must be a Compaction notice"
    );
    assert!(
        matches!(sources[1], BlockSource::Banner { .. }),
        "second source must be a Banner"
    );
    assert!(
        matches!(sources[2], BlockSource::Spacer),
        "third source must be a Spacer"
    );
}

// ── BlockFamily::is_tool_family ─────────────────────────────────────────────

/// `Tool` and `ToolDisplay` are the tool family; every other variant is not.
#[test]
fn block_family_is_tool_family_covers_every_variant() {
    // -- Exec & Check
    assert!(BlockFamily::Tool.is_tool_family(), "Tool is a tool family");
    assert!(
        BlockFamily::ToolDisplay.is_tool_family(),
        "ToolDisplay is a tool family"
    );
    assert!(!BlockFamily::User.is_tool_family(), "User is not");
    assert!(!BlockFamily::Assistant.is_tool_family(), "Assistant is not");
    assert!(!BlockFamily::Notice.is_tool_family(), "Notice is not");
    assert!(!BlockFamily::Banner.is_tool_family(), "Banner is not");
}

// ── BlockSource::plain_text ──────────────────────────────────────────────────

#[test]
fn block_source_plain_text_flattens_each_variant() -> Result<()> {
    // -- Setup & Fixtures
    let sources = [
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hello world".to_string(),
        },
        BlockSource::Tool {
            name: ToolName("read".to_string()),
            call: CallLine {
                summary: "→ read a.rs".to_string(),
            },
            preview: None,
        },
        BlockSource::ToolDisplay {
            lines: vec![
                ContentLine::from_spans(vec![
                    Span::normal("diff ".to_string()),
                    Span::muted("a.rs".to_string()),
                ]),
                ContentLine::single("context".to_string(), StyleHint::Normal),
            ],
        },
        BlockSource::Notice {
            kind: NoticeKind::System,
            text: "notice text".to_string(),
        },
        BlockSource::Banner {
            text: "banner".to_string(),
        },
        BlockSource::Spacer,
    ];
    let expected = [
        "hello world".to_string(),
        "→ read a.rs".to_string(),
        "diff a.rs\ncontext".to_string(),
        "notice text".to_string(),
        "banner".to_string(),
        String::new(),
    ];

    // -- Exec & Check
    for (source, expected) in sources.iter().zip(expected.iter()) {
        assert_eq!(
            &source.plain_text(),
            expected,
            "variant {source:?} flattened wrong"
        );
    }
    Ok(())
}
