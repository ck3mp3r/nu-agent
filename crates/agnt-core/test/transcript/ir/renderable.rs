use super::*;

// ── Message Renderable impl ──────────────────────────────────────────────────

#[test]
fn message_user_lanes_marker_and_fills_full() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(msg.lane(), Lane::Marker("▏"));
    assert_eq!(msg.fill(), Fill::Full);
}

#[test]
fn message_assistant_is_blank_lane_with_no_fill() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(msg.lane(), Lane::Blank);
    assert_eq!(msg.fill(), Fill::None);
}

// ── Spacer / Banner Renderable impls ────────────────────────────────────────

#[test]
fn spacer_source_is_spacer_with_blank_lane_and_no_fill() {
    // -- Setup & Fixtures
    let spacer = Spacer;

    // -- Exec & Check
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);
}

#[test]
fn banner_source_is_blank_system_lane_with_no_fill() {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        banner.source(),
        BlockSource::Banner {
            text: "line1\nline2".to_string()
        }
    );
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
}
