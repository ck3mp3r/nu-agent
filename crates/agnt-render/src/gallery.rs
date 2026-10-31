//! The gallery: one example entry per family, rendered by the plain backend.
//!
//! The snapshots double as the golden-fixture source for the cross-backend
//! tests (design §3.4/§4.3). Adding a family adds one sample here.

use agnt_core::protocol::event::ToolDisplayStats;
use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_core::transcript::ir::{
    ContentKind, ContentLine, Display, DisplaySection, MessageRole, NoticeKind, StyleHint, ToolName,
};
use agnt_core::transcript::markdown::project_diff_lines;
use agnt_core::transcript::renderer::ItemStatus;

use crate::backend::{RenderBackend, RowRect};
use crate::catalog::{
    CallLineRow, CatalogComponent, CodeSurface, DiffView, LaneKind, LaneLine, StatRow, SurfaceRow,
};
use crate::plain::PlainBackend;
use crate::planner::EntryFamily;
use crate::theme::Theme;

// region:    --- Types

/// One example entry and the family it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GallerySample {
    /// Stable fixture name (kebab-case).
    pub name: &'static str,
    pub family: EntryFamily,
    pub entry: TranscriptEntry,
}

/// The plain-text rendering of one sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GallerySnapshot {
    pub name: &'static str,
    pub family: EntryFamily,
    pub text: String,
}

/// One catalog component example and the component it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryComponentSample {
    /// Stable fixture name (kebab-case).
    pub name: &'static str,
    pub component: CatalogComponent,
}

/// The plain-text rendering of one catalog component example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryComponentSnapshot {
    pub name: &'static str,
    pub text: String,
}

/// The per-family example harness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gallery;

// endregion: --- Types

// region:    --- Gallery

impl Gallery {
    /// Render width every snapshot uses.
    pub const WIDTH: usize = 40;

    /// One example entry per family, in plan order.
    pub fn samples() -> Vec<GallerySample> {
        vec![
            GallerySample {
                name: "message",
                family: EntryFamily::User,
                entry: TranscriptEntry::Message(MessageEntry {
                    role: MessageRole::User,
                    markdown: "What changed in the render loop?".to_string(),
                }),
            },
            GallerySample {
                name: "tool",
                family: EntryFamily::Tool,
                entry: TranscriptEntry::Tool(ToolEntry {
                    kind: ToolKind::Generic,
                    call: ToolCall::new(
                        ToolName("read".to_string()),
                        "",
                        "{\"path\":\"src/render.rs\"}",
                        CallLine {
                            summary: "→ src/render.rs".to_string(),
                        },
                    ),
                    preview: Some(Display {
                        title: None,
                        sections: vec![DisplaySection {
                            label: None,
                            kind: ContentKind::Plain,
                            content: "seam 2 now plans the gaps".to_string(),
                            stats: None,
                        }],
                    }),
                    result: None,
                    status: ItemStatus::Done,
                }),
            },
            GallerySample {
                name: "tool-result",
                family: EntryFamily::Tool,
                entry: TranscriptEntry::Tool(ToolEntry {
                    kind: ToolKind::Edit,
                    call: ToolCall::new(
                        ToolName("edit".to_string()),
                        "",
                        "{\"path\":\"src/render.rs\"}",
                        CallLine {
                            summary: "→ src/render.rs".to_string(),
                        },
                    ),
                    // The gate preview the pending call showed. The result
                    // below takes over once the call completes; the preview
                    // must not render beside it.
                    preview: Some(Display {
                        title: None,
                        sections: vec![DisplaySection {
                            label: None,
                            kind: ContentKind::Plain,
                            content: "gate preview".to_string(),
                            stats: None,
                        }],
                    }),
                    result: Some(Display {
                        title: None,
                        sections: vec![DisplaySection {
                            label: None,
                            kind: ContentKind::Plain,
                            content: "result body".to_string(),
                            stats: None,
                        }],
                    }),
                    status: ItemStatus::Done,
                }),
            },
            GallerySample {
                name: "notice",
                family: EntryFamily::Notice,
                entry: TranscriptEntry::Notice(NoticeEntry {
                    kind: NoticeKind::System,
                    text: "Compaction skipped: transcript is short".to_string(),
                }),
            },
            GallerySample {
                name: "banner",
                family: EntryFamily::Banner,
                entry: TranscriptEntry::Banner(BannerEntry {
                    text: "nu-agent".to_string(),
                }),
            },
            GallerySample {
                name: "tool-diff",
                family: EntryFamily::Tool,
                entry: TranscriptEntry::Tool(ToolEntry {
                    kind: ToolKind::Edit,
                    call: ToolCall::new(
                        ToolName("edit".to_string()),
                        "",
                        "{\"path\":\"crates/agnt-render/src/plain.rs\"}",
                        CallLine {
                            summary: "→ crates/agnt-render/src/plain.rs (diff)".to_string(),
                        },
                    ),
                    // The completion display carries the diff: the line-number
                    // gutter, the hunk header, the file meta, and the tinted
                    // body rows. The call line and the body lines are long
                    // enough to wrap, so the span slicing at a wrap boundary
                    // renders through both backends.
                    preview: None,
                    result: Some(Display {
                        title: None,
                        sections: vec![DisplaySection {
                            label: None,
                            kind: ContentKind::Diff {
                                language: "diff".to_string(),
                                path: "crates/agnt-render/src/plain.rs".to_string(),
                            },
                            content: "\
--- a/crates/agnt-render/src/plain.rs
+++ b/crates/agnt-render/src/plain.rs
@@ -118,7 +118,8 @@
 fn content_rows(entry: &TranscriptEntry, width: usize) -> Vec<String> {
     let width = width.max(1);
-    content_lines(entry, width)
+    let wrapping = WrapMode::first_fit(width);
+    content_lines(entry, wrapping)
         .iter()
         .flat_map(|line| wrap_line(&rendered_line_to_plain_text(line), width))
         .collect()
}
"
                            .to_string(),
                            stats: Some(ToolDisplayStats {
                                files_changed: Some(1),
                                insertions: Some(2),
                                deletions: Some(1),
                                ..ToolDisplayStats::default()
                            }),
                        }],
                    }),
                    status: ItemStatus::Done,
                }),
            },
            GallerySample {
                name: "message-blank-lines",
                family: EntryFamily::User,
                entry: TranscriptEntry::Message(MessageEntry {
                    // The assistant markdown projection keeps one blank row
                    // between blocks: the corpus pins whatever
                    // `render_markdown_lines` emits, both backends consume it.
                    role: MessageRole::Assistant,
                    markdown: "Two seams moved in this wave.\n\nThe planner now plans the gap rows, so a wrapped preview keeps its spacing.\n\nThe TTY strips the escapes back to the plain rows.".to_string(),
                }),
            },
        ]
    }

    /// Render every sample through the plain backend.
    pub fn snapshots() -> Vec<GallerySnapshot> {
        Self::samples()
            .into_iter()
            .map(|sample| {
                let width = Self::WIDTH;
                let rows = PlainBackend::default().measure(&sample.entry, width);
                let mut backend = PlainBackend::default();
                backend.render(
                    &sample.entry,
                    RowRect::new(0, width, rows),
                    &Theme::default(),
                );
                GallerySnapshot {
                    name: sample.name,
                    family: sample.family,
                    text: backend.to_text(),
                }
            })
            .collect()
    }

    /// One example per catalog component, in catalog order.
    pub fn component_samples() -> Vec<GalleryComponentSample> {
        vec![
            GalleryComponentSample {
                name: "lane-line",
                component: CatalogComponent::LaneLine(LaneLine::new(LaneKind::Tool)),
            },
            GalleryComponentSample {
                name: "call-line-row",
                component: CatalogComponent::CallLineRow(CallLineRow::new(
                    "read",
                    "→ src/render.rs",
                    Some(ItemStatus::Done),
                )),
            },
            GalleryComponentSample {
                name: "stat-row",
                component: CatalogComponent::StatRow(StatRow::new(ToolDisplayStats {
                    files_changed: Some(1),
                    insertions: Some(2),
                    deletions: Some(0),
                    ..ToolDisplayStats::default()
                })),
            },
            GalleryComponentSample {
                name: "diff-view",
                component: CatalogComponent::DiffView(DiffView::new(project_diff_lines(
                    "@@ -1 +1 @@\n-old\n+new\n",
                    "diff",
                ))),
            },
            GalleryComponentSample {
                name: "code-surface",
                component: CatalogComponent::CodeSurface(CodeSurface::new(vec![SurfaceRow::Text(
                    ContentLine::single("let a = 1;".to_string(), StyleHint::Normal),
                )])),
            },
        ]
    }

    /// Render every catalog component sample through the plain backend.
    pub fn component_snapshots() -> Vec<GalleryComponentSnapshot> {
        Self::component_samples()
            .into_iter()
            .map(|sample| {
                let mut backend = PlainBackend::default();
                backend.render_component(&sample.component);
                GalleryComponentSnapshot {
                    name: sample.name,
                    text: backend.to_text(),
                }
            })
            .collect()
    }
}

// endregion: --- Gallery

// region:    --- Tests

#[cfg(test)]
#[path = "../test/gallery.rs"]
mod gallery_test;

// endregion: --- Tests
