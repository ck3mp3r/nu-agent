use super::ir::{BlockSource, Fill, Lane, MessageRole};
use super::items::{Banner, Message, Spacer};
use super::renderer::*;

// ── FrameContext ─────────────────────────────────────────────────────────────

/// `FrameContext` stores its per-frame fields and clones them verbatim.
#[test]
fn frame_context_table() {
    // -- Setup & Fixtures
    let ctx = FrameContext {
        width: 80,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    let selected = FrameContext {
        width: 40,
        now_millis: 100,
        cursor: true,
        selected: true,
    };

    // -- Exec & Check: fields are stored verbatim.
    assert_eq!(ctx.width, 80);
    assert_eq!(ctx.now_millis, 0);
    assert!(!ctx.cursor);
    assert!(!ctx.selected);

    // -- Check: clone preserves every field.
    let cloned = selected.clone();
    assert_eq!(cloned.width, 40);
    assert_eq!(cloned.now_millis, 100);
    assert!(cloned.cursor);
    assert!(cloned.selected);
}

// ── ItemStatus ───────────────────────────────────────────────────────────────

/// Terminal statuses render fixed glyphs; `InProgress` cycles one spinner
/// frame every 100 ms and wraps after the full cycle.
#[test]
fn item_status_table() {
    // -- Exec & Check: equality distinguishes terminal statuses.
    assert_eq!(ItemStatus::Done, ItemStatus::Done);
    assert_ne!(ItemStatus::Done, ItemStatus::Failed);

    // -- Check: terminal statuses render fixed glyphs.
    assert_eq!(ItemStatus::Done.indicator_char(0), "✓");
    assert_eq!(ItemStatus::Failed.indicator_char(0), "✕");
    assert_eq!(ItemStatus::Cancelled.indicator_char(0), "⊘");
    assert_eq!(ItemStatus::Queued.indicator_char(0), "•");
    assert_eq!(ItemStatus::Unknown.indicator_char(0), "?");

    // -- Check: InProgress cycles one frame every 100 ms and wraps.
    assert_eq!(ItemStatus::InProgress.indicator_char(0), "⠋");
    assert_eq!(ItemStatus::InProgress.indicator_char(100), "⠙");
    assert_eq!(ItemStatus::InProgress.indicator_char(900), "⠏");
    assert_eq!(ItemStatus::InProgress.indicator_char(1000), "⠋");
}

// ── Renderable trait (three questions) ──────────────────────────────────────

#[test]
fn renderable_impls_answer_three_questions_for_message_spacer_banner() {
    // -- Setup & Fixtures
    let user = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };
    let assistant = Message {
        role: MessageRole::Assistant,
        markdown: "yo".to_string(),
    };
    let spacer = Spacer;
    let banner = Banner {
        text: "logo".to_string(),
    };

    // -- Exec & Check
    // User: marker lane, full fill.
    assert_eq!(user.lane(), Lane::Marker("╎"));
    assert_eq!(user.fill(), Fill::Full);
    // Assistant: blank lane, no fill.
    assert_eq!(assistant.lane(), Lane::Blank);
    assert_eq!(assistant.fill(), Fill::None);
    // Spacer: blank lane, no fill, Spacer source.
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    // Banner: system-blank lane, no fill, Banner source.
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
    assert!(matches!(banner.source(), BlockSource::Banner { .. }));
}
