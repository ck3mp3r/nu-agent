pub use agnt_kernel::ToolName;

/// What kind of content a section carries. The drawer's own classification:
/// how a section is projected and styled is this crate's business, not the
/// producer's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentKind {
    Diff { language: String, path: String },
    Code { language: String },
    Plain,
}

impl ContentKind {
    /// The language marker this content kind carries; empty for plain content.
    pub fn language(&self) -> &str {
        match self {
            Self::Diff { language, .. } | Self::Code { language } => language,
            Self::Plain => "",
        }
    }
}

/// The counters a display section reports, as the drawer reads them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayStats {
    pub files_changed: Option<usize>,
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
    pub diff_truncated: Option<bool>,
    pub omitted_files: Option<usize>,
    pub omitted_hunks: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    System,
}

/// A pre-execution preview display (edit diff, nu code). Produced by
/// `Previewable::preview()` on the tool type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub title: Option<String>,
    pub sections: Vec<DisplaySection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplaySection {
    pub label: Option<String>,
    pub kind: ContentKind,
    pub content: String,
    pub stats: Option<DisplayStats>,
}

/// The stats row a display section renders: one line, or `None` when no field
/// carries a value. The row renders from this one formatting, so the restored
/// preview rows match the live completion path.
pub fn format_stats_line(stats: &DisplayStats) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(files_changed) = stats.files_changed {
        parts.push(format!("files={files_changed}"));
    }
    if let Some(insertions) = stats.insertions {
        parts.push(format!("+{insertions}"));
    }
    if let Some(deletions) = stats.deletions {
        parts.push(format!("-{deletions}"));
    }
    if let Some(true) = stats.diff_truncated {
        parts.push("truncated=true".to_string());
    }
    if let Some(omitted_files) = stats.omitted_files {
        parts.push(format!("omitted={omitted_files}"));
    }
    if let Some(omitted_hunks) = stats.omitted_hunks {
        parts.push(format!("hunks_omitted={omitted_hunks}"));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

impl Display {
    /// Returns true when any section carries diff or code content — the
    /// signal the planner reads to decide whether an entry is filled.
    pub fn has_code_or_diff(&self) -> bool {
        self.sections
            .iter()
            .any(|section| !matches!(section.kind, ContentKind::Plain))
    }

    /// Returns true when any section carries diff content. A diff body paints
    /// its row tints on whatever surface it sits on, and it closes with one
    /// blank row before the next tool call — the row the pre-wave pane put
    /// between a diff display and its neighbour.
    pub fn has_diff(&self) -> bool {
        self.sections
            .iter()
            .any(|section| matches!(section.kind, ContentKind::Diff { .. }))
    }

    /// Project this display into content lines: the title row (when the
    /// producer set one), each section's `label (language)` row (when the
    /// producer set a label), its stats row, and its projected content. The
    /// producer decides what is redundant — the renderer never inspects tool
    /// identity.
    ///
    /// This is the single display projection: the tool block's preview path
    /// and the TUI's preview block both call it, so the two cannot drift apart.
    pub fn project_lines(&self) -> Vec<ContentLine> {
        let mut lines = Vec::new();
        if let Some(title) = &self.title {
            lines.push(ContentLine::single(title.clone(), StyleHint::Normal));
        }

        for section in &self.sections {
            if let Some(label) = &section.label {
                lines.push(ContentLine::single(
                    format!("{} ({})", label, section.kind.language()),
                    StyleHint::Normal,
                ));
            }
            if let Some(stats_line) = section.stats.as_ref().and_then(format_stats_line) {
                lines.push(ContentLine::single(stats_line, StyleHint::Muted));
            }
            match &section.kind {
                ContentKind::Diff { language, .. } => {
                    lines.extend(crate::transcript::markdown::project_diff_lines(
                        &section.content,
                        language,
                    ));
                }
                ContentKind::Code { language } => {
                    lines.extend(crate::transcript::markdown::project_code_block_lines(
                        language,
                        &section.content,
                    ));
                }
                ContentKind::Plain => lines.push(ContentLine::single(
                    section.content.clone(),
                    StyleHint::Normal,
                )),
            }
        }
        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleHint {
    Normal,
    Emphasis,
    Meta,
    Muted,
    Success,
    Error,
    DiffAdd,
    DiffRemove,
    DiffHunk,
    Cancelled,
    MdBold,
    MdItalic,
    MdBoldItalic,
    MdInlineCode,
    MdCodeKeyword,
    MdCodeType,
    MdCodeFunction,
    MdCodeVariable,
    MdCodeConstant,
    MdCodeString,
    MdCodeNumber,
    MdCodeOperator,
    MdCodePunctuation,
    MdCodeComment,
    MdCodePlain,
    MdHeading(u8),
    MdLink,
    MdLinkUrl,
    MdQuote,
    MdListMarker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub hint: StyleHint,
}

impl Span {
    pub fn new(text: String, hint: StyleHint) -> Self {
        Self { text, hint }
    }

    pub fn normal(text: String) -> Self {
        Self {
            text,
            hint: StyleHint::Normal,
        }
    }

    pub fn emphasis(text: String) -> Self {
        Self {
            text,
            hint: StyleHint::Emphasis,
        }
    }

    pub fn meta(text: String) -> Self {
        Self {
            text,
            hint: StyleHint::Meta,
        }
    }

    pub fn muted(text: String) -> Self {
        Self {
            text,
            hint: StyleHint::Muted,
        }
    }
}

/// Line-level diff background tint. The renderer applies the tint to the
/// whole row while each span keeps its own foreground syntax color. `None`
/// (no tint) is represented by `Option<DiffTint>` being `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTint {
    Add,
    Remove,
    Context,
}

/// A single projected content row. `hang_indent` is the number of display
/// columns the line's leading marker occupies; when the line wraps, the
/// renderer indents continuation rows by this amount so list item text stays
/// aligned under the marker text (task 7bd175d2). `diff_tint` is the optional
/// line-level diff background; `None` means no diff background.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentLine {
    pub spans: Vec<Span>,
    pub hang_indent: usize,
    pub diff_tint: Option<DiffTint>,
}

impl ContentLine {
    pub fn single(text: String, hint: StyleHint) -> Self {
        Self {
            spans: vec![Span::new(text, hint)],
            hang_indent: 0,
            diff_tint: None,
        }
    }

    pub fn from_spans(spans: Vec<Span>) -> Self {
        Self {
            spans,
            hang_indent: 0,
            diff_tint: None,
        }
    }

    pub fn single_with_tint(text: String, hint: StyleHint, diff_tint: DiffTint) -> Self {
        Self {
            spans: vec![Span::new(text, hint)],
            hang_indent: 0,
            diff_tint: Some(diff_tint),
        }
    }

    pub fn empty() -> Self {
        Self {
            spans: vec![],
            hang_indent: 0,
            diff_tint: None,
        }
    }
}

/// Classify one raw diff line into the [`StyleHint`] that paints it: hunk
/// headers, file headers, the no-newline marker, added lines, and removed
/// lines each take their own hint; every other line is `Normal`.
pub fn annotate_diff_hint(text: &str) -> StyleHint {
    let trimmed = text.trim_start();
    if trimmed.starts_with("@@ ") {
        return StyleHint::DiffHunk;
    }
    if trimmed.starts_with("--- ") || trimmed.starts_with("+++ ") {
        return StyleHint::Meta;
    }
    if trimmed.starts_with('+') {
        return StyleHint::DiffAdd;
    }
    if trimmed.starts_with('-') {
        return StyleHint::DiffRemove;
    }
    if trimmed.starts_with("\\ ") {
        return StyleHint::Meta;
    }
    StyleHint::Normal
}
