use super::*;

// ── Renderable impls ────────────────────────────────────────────────────────

/// Message, Spacer, and Banner answer the three `Renderable` questions:
/// source, lane, and fill.
#[test]
fn renderable_impls_table() -> Result<()> {
    // -- Setup & Fixtures
    let user = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };
    let assistant = Message {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
    };
    let spacer = Spacer;
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec & Check: user message — marker lane, full fill, markdown source.
    assert_eq!(
        user.source(),
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(user.lane(), Lane::Marker("▏"));
    assert_eq!(user.fill(), Fill::Full);

    // -- Check: assistant message — blank lane, no fill, markdown source.
    assert_eq!(
        assistant.source(),
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(assistant.lane(), Lane::Blank);
    assert_eq!(assistant.fill(), Fill::None);

    // -- Check: spacer — Spacer source, blank lane, no fill.
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);

    // -- Check: banner — Banner source, system-blank lane, no fill.
    assert_eq!(
        banner.source(),
        BlockSource::Banner {
            text: "line1\nline2".to_string()
        }
    );
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
    Ok(())
}
