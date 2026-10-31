//! Transcript entry tests: the tool-kind classification and the entry payloads.

use super::{BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry};
use crate::protocol::events::ToolCall;
use crate::protocol::tool_args::CallLine;
use crate::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use crate::transcript::renderer::ItemStatus;

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Tool identity is classified once, from the tool name: `edit` and `nu` map to
/// their kinds, every other builtin and every unregistered name is `Generic`.
#[test]
fn test_entry_tool_kind_classifies_from_the_tool_name() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("edit", ToolKind::Edit),
        ("nu", ToolKind::Nu),
        ("read", ToolKind::Generic),
        ("grep", ToolKind::Generic),
        ("unregistered_tool", ToolKind::Generic),
    ];

    // -- Exec & Check
    for (name, expected) in cases {
        assert_eq!(
            ToolKind::from_name(&ToolName(name.to_string())),
            expected,
            "tool name {name:?} classified wrong"
        );
    }
    Ok(())
}

/// A tool entry carries its typed call, the preview and result displays, and
/// its status; the entry itself reports them unchanged.
#[test]
fn test_entry_tool_entry_carries_call_preview_result_and_status() -> Result<()> {
    // -- Setup & Fixtures
    let call = ToolCall::new(
        ToolName("edit".to_string()),
        "closure",
        r#"{"path":"a.rs"}"#,
        CallLine {
            summary: "→ a.rs".to_string(),
        },
    );
    let entry = TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call: call.clone(),
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    });

    // -- Exec & Check
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("entry must be a tool entry".into());
    };
    assert_eq!(tool.kind, ToolKind::Edit);
    assert_eq!(tool.call, call);
    assert!(tool.preview.is_none());
    assert!(tool.result.is_none());
    assert_eq!(tool.status, ItemStatus::InProgress);
    Ok(())
}

/// Component-owned padding: prose, notices, and banners are flush — the pair
/// table carries their spacing. A tool entry whose display paints a code or
/// diff surface closes with one blank row below its band. The padding is the
/// entry's own property, derived from its payload — never a renderer heuristic.
#[test]
fn test_entry_padding_belongs_to_the_entry() -> Result<()> {
    // -- Setup & Fixtures
    let call = ToolCall::new(
        ToolName("read".to_string()),
        "",
        "",
        CallLine {
            summary: "→ a.rs".to_string(),
        },
    );
    let cases = [
        (
            TranscriptEntry::Message(MessageEntry {
                role: MessageRole::User,
                markdown: "hi".to_string(),
            }),
            (0usize, 0usize),
        ),
        (
            TranscriptEntry::Message(MessageEntry {
                role: MessageRole::Assistant,
                markdown: "hi".to_string(),
            }),
            (0, 0),
        ),
        (
            TranscriptEntry::Tool(ToolEntry {
                kind: ToolKind::from_name(&call.name),
                call,
                preview: None,
                result: None,
                status: ItemStatus::Done,
            }),
            (0, 0),
        ),
        (
            TranscriptEntry::Tool(ToolEntry {
                kind: ToolKind::Generic,
                call: ToolCall::new(
                    ToolName("nu".to_string()),
                    "",
                    "",
                    CallLine {
                        summary: "→ ls".to_string(),
                    },
                ),
                preview: None,
                result: Some(Display {
                    title: None,
                    sections: vec![DisplaySection {
                        label: None,
                        kind: ContentKind::Code {
                            language: "nu".to_string(),
                        },
                        content: "ls".to_string(),
                        stats: None,
                    }],
                }),
                status: ItemStatus::Done,
            }),
            (0, 0),
        ),
        (
            TranscriptEntry::Tool(ToolEntry {
                kind: ToolKind::Generic,
                call: ToolCall::new(
                    ToolName("nu".to_string()),
                    "",
                    "",
                    CallLine {
                        summary: "→ ls".to_string(),
                    },
                ),
                preview: Some(Display {
                    title: None,
                    sections: vec![DisplaySection {
                        label: None,
                        kind: ContentKind::Code {
                            language: "nu".to_string(),
                        },
                        content: "ls".to_string(),
                        stats: None,
                    }],
                }),
                result: None,
                status: ItemStatus::InProgress,
            }),
            (0, 1),
        ),
        (
            TranscriptEntry::Tool(ToolEntry {
                kind: ToolKind::Generic,
                call: ToolCall::new(
                    ToolName("edit".to_string()),
                    "",
                    "",
                    CallLine {
                        summary: "→ a.rs".to_string(),
                    },
                ),
                preview: None,
                result: Some(Display {
                    title: None,
                    sections: vec![DisplaySection {
                        label: None,
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                            path: "a.rs".to_string(),
                        },
                        content: "@@ -1 +1 @@".to_string(),
                        stats: None,
                    }],
                }),
                status: ItemStatus::Done,
            }),
            (0, 1),
        ),
        (
            TranscriptEntry::Notice(NoticeEntry {
                kind: NoticeKind::System,
                text: "note".to_string(),
            }),
            (0, 0),
        ),
        (
            TranscriptEntry::Banner(BannerEntry {
                text: "logo".to_string(),
            }),
            (0, 0),
        ),
    ];

    // -- Exec & Check
    for (entry, expected) in cases {
        assert_eq!(entry.padding(), expected, "padding of {entry:?}");
    }
    Ok(())
}

/// The message, notice, and banner entries carry their role/kind/text payload
/// unchanged.
#[test]
fn test_entry_message_notice_banner_payloads() -> Result<()> {
    // -- Setup & Fixtures
    let message = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: "hello".to_string(),
    });
    let notice = TranscriptEntry::Notice(NoticeEntry {
        kind: NoticeKind::Compaction,
        text: "Compaction".to_string(),
    });
    let banner = TranscriptEntry::Banner(BannerEntry {
        text: "logo".to_string(),
    });

    // -- Exec & Check
    let TranscriptEntry::Message(message) = message else {
        return Err("entry must be a message entry".into());
    };
    assert_eq!(message.role, MessageRole::User);
    assert_eq!(message.markdown, "hello");

    let TranscriptEntry::Notice(notice) = notice else {
        return Err("entry must be a notice entry".into());
    };
    assert_eq!(notice.kind, NoticeKind::Compaction);
    assert_eq!(notice.text, "Compaction");

    let TranscriptEntry::Banner(banner) = banner else {
        return Err("entry must be a banner entry".into());
    };
    assert_eq!(banner.text, "logo");
    Ok(())
}
