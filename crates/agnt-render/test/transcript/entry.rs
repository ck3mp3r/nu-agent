//! Transcript entry tests: the tool-kind classification and the entry payloads.

use super::{
    BannerEntry, MessageEntry, NoticeEntry, TaskEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use crate::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use crate::transcript::renderer::ItemStatus;
use agnt_kernel::{ToolCall, ToolSource};

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
        ToolSource::Closure,
        r#"{"path":"a.rs"}"#,
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
    let call = ToolCall::new(ToolName("read".to_string()), ToolSource::Unknown, "");
    let cases = [
        (
            TranscriptEntry::Message(MessageEntry {
                role: MessageRole::User,
                markdown: "hi".to_string(),
                timestamp_millis: None,
                model: None,
                agent: None,
            }),
            (0usize, 0usize),
        ),
        (
            TranscriptEntry::Message(MessageEntry {
                role: MessageRole::Assistant,
                markdown: "hi".to_string(),
                timestamp_millis: None,
                model: None,
                agent: None,
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
                call: ToolCall::new(ToolName("nu".to_string()), ToolSource::Unknown, ""),
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
                call: ToolCall::new(ToolName("nu".to_string()), ToolSource::Unknown, ""),
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
                call: ToolCall::new(ToolName("edit".to_string()), ToolSource::Unknown, ""),
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

/// The message, task, notice, and banner entries carry their payload unchanged.
#[test]
fn test_entry_message_task_notice_banner_payloads() -> Result<()> {
    // -- Setup & Fixtures
    let message = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: "hello".to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    });
    let task = TranscriptEntry::Task(TaskEntry {
        name: "Compaction".to_string(),
        summary: "auto".to_string(),
        status: ItemStatus::Done,
    });
    let notice = TranscriptEntry::Notice(NoticeEntry {
        kind: NoticeKind::System,
        text: "a system line".to_string(),
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

    let TranscriptEntry::Task(task) = task else {
        return Err("entry must be a task entry".into());
    };
    assert_eq!(task.name, "Compaction");
    assert_eq!(task.summary, "auto");
    assert_eq!(task.status, ItemStatus::Done);

    let TranscriptEntry::Notice(notice) = notice else {
        return Err("entry must be a notice entry".into());
    };
    assert_eq!(notice.kind, NoticeKind::System);
    assert_eq!(notice.text, "a system line");

    let TranscriptEntry::Banner(banner) = banner else {
        return Err("entry must be a banner entry".into());
    };
    assert_eq!(banner.text, "logo");
    Ok(())
}

/// The footer facts are slots: an entry carries exactly the facts its producer
/// holds, so no fact at all means no footer, and any one real fact means one.
#[test]
fn test_entry_message_has_footer_follows_the_facts() -> Result<()> {
    // -- Setup & Fixtures
    let entry = |timestamp_millis, model: Option<&str>, agent: Option<&str>| MessageEntry {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
        timestamp_millis,
        model: model.map(str::to_string),
        agent: agent.map(str::to_string),
    };
    let cases = [
        (entry(None, None, None), false),
        (entry(Some(1_700_000_000_000), None, None), true),
        (entry(None, Some("openai/gpt-5"), None), true),
        (entry(None, None, Some("coder")), true),
        (
            entry(Some(1_700_000_000_000), Some("openai/gpt-5"), Some("coder")),
            true,
        ),
    ];

    // -- Exec & Check
    for (entry, expected) in cases {
        assert_eq!(entry.has_footer(), expected, "has_footer of {entry:?}");
    }
    Ok(())
}
