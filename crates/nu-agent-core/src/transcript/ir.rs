use super::renderer::ItemStatus;
use crate::protocol::event::ToolDisplayStats;
use crate::protocol::tool_args::CallLine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub source: BlockSource,
    pub lane: Lane,
    pub fill: Fill,
    pub status: Option<ItemStatus>,
}

/// Source data the renderer projects at render time. Carries enough for the
/// renderer to produce lines without knowing the concrete type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockSource {
    /// User or assistant prose — raw markdown, projected at render width.
    Markdown { role: MessageRole, markdown: String },
    /// Tool call — the tool name, its summary line, and an optional
    /// pre-execution preview display. The result is NOT here — it is raw
    /// JSON for the LLM, never rendered.
    Tool {
        name: ToolName,
        call: CallLine,
        preview: Option<Display>,
    },
    /// Compaction or system notice.
    Notice { kind: NoticeKind, text: String },
    /// Tool display content rendered as pre-projected styled lines (diff
    /// annotations, syntax-highlighted code). The lines are produced by the
    /// projection helpers and stored verbatim; the renderer emits them
    /// without re-projection.
    ToolDisplay { lines: Vec<ContentLine> },
    /// Empty spacer row between blocks.
    Spacer,
    /// Centered banner text (e.g. startup ASCII art).
    Banner { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
}

/// Self-classification of a [`BlockSource`] for peer-aware separator
/// decisions. A block does not know its neighbours; it only reports which
/// family it belongs to, so a separator state machine can decide the gap
/// between two adjacent blocks without inspecting their content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockFamily {
    Tool,
    ToolDisplay,
    User,
    Assistant,
    Notice,
    Banner,
}

impl BlockFamily {
    /// Whether this family is part of the tool group (`Tool` or
    /// `ToolDisplay`). The family owns its own classification, so the
    /// separator state machine never re-derives it from a source match.
    pub fn is_tool_family(&self) -> bool {
        matches!(self, BlockFamily::Tool | BlockFamily::ToolDisplay)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Compaction,
    System,
}

/// Lane prefix style. Applied to row 0; continuation rows are blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Fixed marker glyph: "▏" User, "⚙" Tool, "~" Compaction, "·" System.
    Marker(&'static str),
    /// Assistant prose and Spacer — blank prefix, assistant/neutral styling.
    Blank,
    /// Blank prefix with system styling — the banner (startup logo).
    /// Distinct from [`Lane::Blank`] so the lane itself carries the styling:
    /// the renderer reads the variant and never inspects the block source.
    SystemBlank,
}

/// Background fill behavior, decided at construction time from the concrete
/// type's data — never by sniffing rendered strings at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Full-width user background — bleeds into adjacent Spacer blocks.
    Full,
    /// Code/diff background — margin rows inserted above and below.
    Code,
    /// No background.
    None,
}

impl Fill {
    /// Fill decision from a tool preview: a preview carrying diff or code
    /// content gets the code background, everything else none.
    pub fn from_preview(preview: &Option<Display>) -> Self {
        match preview {
            Some(display) if display.has_code_or_diff() => Self::Code,
            _ => Self::None,
        }
    }

    /// Number of margin rows this fill inserts inside the block (one above and
    /// one below the content). `Fill::Code` inserts two so the code surface has
    /// internal padding; every other fill inserts none. The renderer owns
    /// *where* those rows go and what colour they take.
    pub fn margin_row_count(&self) -> usize {
        match self {
            Self::Code => 2,
            Self::Full | Self::None => 0,
        }
    }
}

/// Projected, wrappable text. Produced by the renderer at render time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub lines: Vec<ContentLine>,
}

/// A tool call in the transcript. Lifecycle: created at tool start with
/// status `InProgress`, `preview` is set by the permission Ask path, and
/// `result` stores the raw JSON returned to the LLM — never rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: ToolName,
    pub call: CallLine,
    pub preview: Option<Display>,
    pub result: Option<String>,
    pub status: ItemStatus,
}

/// A tool's name. Newtype so tool identity stays typed at the block level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolName(pub String);

/// A pre-execution preview display (edit diff, nu code). Produced by
/// `Previewable::preview()` on the tool type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub title: String,
    pub sections: Vec<DisplaySection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplaySection {
    pub label: String,
    pub kind: ContentKind,
    pub content: String,
    pub stats: Option<DisplayStats>,
}

/// What kind of content a display section carries. Determines how the
/// renderer projects and styles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentKind {
    Diff { language: String },
    Code { language: String },
    Plain,
}

impl ContentKind {
    pub fn language(&self) -> &str {
        match self {
            Self::Diff { language } | Self::Code { language } => language,
            Self::Plain => "",
        }
    }
}

/// Section statistics on a display. Alias over the protocol stats type so
/// the renderer does not depend on the event layer; subtask 2a decides
/// whether this stays an alias or becomes a standalone struct.
pub type DisplayStats = ToolDisplayStats;

impl DisplayStats {
    /// Format these stats into one row, or `None` when no field carries a
    /// value. Mirrors the pre-refactor `append_direct_tool_display_section`
    /// stat formatting so the restored preview rows match the live completion
    /// path. The stats type owns its own formatting — the caller only decides
    /// whether to render the row.
    pub fn format_stats_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(files_changed) = self.files_changed {
            parts.push(format!("files={files_changed}"));
        }
        if let Some(insertions) = self.insertions {
            parts.push(format!("+{insertions}"));
        }
        if let Some(deletions) = self.deletions {
            parts.push(format!("-{deletions}"));
        }
        if let Some(true) = self.diff_truncated {
            parts.push("truncated=true".to_string());
        }
        if let Some(omitted_files) = self.omitted_files {
            parts.push(format!("omitted={omitted_files}"));
        }
        if let Some(omitted_hunks) = self.omitted_hunks {
            parts.push(format!("hunks_omitted={omitted_hunks}"));
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

impl Display {
    /// Returns true when any section carries diff or code content — the
    /// signal the tool block uses to pick `Fill::Code`.
    pub fn has_code_or_diff(&self) -> bool {
        self.sections
            .iter()
            .any(|section| !matches!(section.kind, ContentKind::Plain))
    }

    /// Whether this display is exactly one diff section. A pure shape query
    /// on the display data — no tool identity, no policy. The caller combines
    /// it with typed tool identity (`ToolName::is_edit`) to decide redundant
    /// title suppression.
    pub fn is_single_diff_section(&self) -> bool {
        self.sections.len() == 1 && matches!(self.sections[0].kind, ContentKind::Diff { .. })
    }

    /// Whether this display is exactly one code section. The sibling shape
    /// query to [`Display::is_single_diff_section`] — no tool identity, no
    /// policy. The caller combines it with typed tool identity
    /// (`ToolName::is_nu`) to decide redundant title suppression.
    pub fn is_single_code_section(&self) -> bool {
        self.sections.len() == 1 && matches!(self.sections[0].kind, ContentKind::Code { .. })
    }

    /// Project this display into content lines: the title row, each section's
    /// `label (language)` row, its stats row, and its projected content —
    /// minus the rows the tool identity makes redundant. An `edit` tool whose
    /// only section is a diff already shows its payload on the call line, and
    /// a `nu` tool whose only section is code shows its command in this
    /// display, so their title, label, and stats rows are dropped.
    ///
    /// This is the single display projection: the tool block's preview path
    /// and the TUI's preview block both call it, so the two cannot drift apart.
    pub fn project_lines(&self, name: &ToolName) -> Vec<ContentLine> {
        let suppress_title = (name.is_edit() && self.is_single_diff_section())
            || (name.is_nu() && self.is_single_code_section());
        let suppress_single_section_stats = suppress_title && self.sections.len() == 1;

        let mut lines = Vec::new();
        if !suppress_title {
            lines.push(ContentLine::single(self.title.clone(), StyleHint::Normal));
        }

        for section in &self.sections {
            // The call line already shows `→ <path> (diff)` for an edit, so the
            // section label row is redundant for that suppressed shape. For a
            // nu the command is the section content itself, so the label row
            // would only repeat the tool name.
            if !suppress_title {
                lines.push(ContentLine::single(
                    format!("{} ({})", section.label, section.kind.language()),
                    StyleHint::Normal,
                ));
            }
            if !suppress_single_section_stats
                && let Some(stats_line) = section
                    .stats
                    .as_ref()
                    .and_then(|stats| stats.format_stats_line())
            {
                lines.push(ContentLine::single(stats_line, StyleHint::Muted));
            }
            match &section.kind {
                ContentKind::Diff { language } => {
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

impl BlockSource {
    /// Classify this source into a [`BlockFamily`] for peer-aware separator
    /// decisions. The block reports its own family; it never inspects a
    /// neighbour. A `Spacer` is not a content family — it reports `Notice`
    /// only as a neutral fallback; the store no longer inserts Spacer blocks.
    pub fn family(&self) -> BlockFamily {
        match self {
            Self::Tool { .. } => BlockFamily::Tool,
            Self::ToolDisplay { .. } => BlockFamily::ToolDisplay,
            Self::Markdown {
                role: MessageRole::User,
                ..
            } => BlockFamily::User,
            Self::Markdown {
                role: MessageRole::Assistant,
                ..
            } => BlockFamily::Assistant,
            Self::Notice { .. } | Self::Spacer => BlockFamily::Notice,
            Self::Banner { .. } => BlockFamily::Banner,
        }
    }

    /// Whether this source carries diff content. True only for a `ToolDisplay`
    /// whose lines include a span tagged `DiffAdd`, `DiffRemove`, or
    /// `DiffHunk`; every other variant is false. The separator state machine
    /// (in the render layer) uses this to keep a filled diff region visually
    /// separate from the next tool call.
    pub fn has_diff_content(&self) -> bool {
        let Self::ToolDisplay { lines } = self else {
            return false;
        };
        lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                matches!(
                    span.hint,
                    StyleHint::DiffAdd | StyleHint::DiffRemove | StyleHint::DiffHunk
                )
            })
        })
    }

    /// Flatten the source to plain text: the markdown body, the tool call
    /// summary, the tool display lines joined with "\n", the notice or
    /// banner text, or an empty string for a spacer. The single flattening
    /// used for comparisons and dedup assertions.
    pub fn plain_text(&self) -> String {
        match self {
            Self::Markdown { markdown, .. } => markdown.clone(),
            Self::Tool { call, .. } => call.summary.clone(),
            Self::ToolDisplay { lines } => lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<String>>()
                .join("\n"),
            Self::Notice { text, .. } => text.clone(),
            Self::Banner { text } => text.clone(),
            Self::Spacer => String::new(),
        }
    }

    /// Project this source into renderable [`ContentLine`]s at `width`
    /// display columns. Each variant owns its projection through a private
    /// method, so adding a variant or changing one variant's projection is a
    /// local edit — the renderer never grows a match over every source type
    /// (OCP). Defined here because the projection helpers
    /// (`crate::transcript::markdown`) and the [`ContentLine`] IR both live in
    /// this crate; the renderer calls this method, it does not own the logic.
    pub fn project(&self, width: usize) -> Vec<ContentLine> {
        match self {
            Self::Markdown { markdown, .. } => Self::project_markdown(markdown, width),
            Self::Tool {
                name,
                call,
                preview,
            } => Self::project_tool(name, call, preview),
            Self::Notice { text, .. } => Self::project_notice(text),
            Self::ToolDisplay { lines } => lines.clone(),
            Self::Spacer => vec![ContentLine::empty()],
            Self::Banner { text } => Self::project_banner(text),
        }
    }

    /// Markdown projection at the canvas width. Delegates to the shared
    /// pulldown-cmark pipeline.
    fn project_markdown(markdown: &str, width: usize) -> Vec<ContentLine> {
        let canvas_width = u16::try_from(width).unwrap_or(u16::MAX);
        crate::transcript::markdown::render_markdown_lines(markdown, Some(canvas_width))
    }

    /// Tool call line — the tool name in emphasis and the summary in muted,
    /// the call line the pre-Block model rendered (`Span::emphasis(name)`
    /// followed by `Span::muted(" {summary}")`). An empty summary renders the
    /// name alone with no trailing space. The preview display then
    /// contributes its title, and each section its `label (language)` row, its
    /// projection per [`ContentKind`], and its stats row — the same rows the
    /// pre-refactor `append_direct_tool_display` emitted. A nameless block (the
    /// legacy `push_transcript_line` path) renders the summary alone.
    ///
    /// Suppression (mirrors the live completion path): for an `edit` tool whose
    /// only section is a diff, the title and the single section label are
    /// redundant with the `→ <path> (diff)` call line, so both are dropped. For
    /// a `nu` tool whose only section is code, the title (`nu`), the label
    /// (`nu (nu)`), and the stats row are redundant with the preview block that
    /// carries the command, so all three are dropped.
    fn project_tool(
        name: &ToolName,
        call: &CallLine,
        preview: &Option<Display>,
    ) -> Vec<ContentLine> {
        let mut lines = Vec::new();
        let name_text = name.0.as_str();
        let mut spans = Vec::new();
        if !name_text.is_empty() {
            spans.push(Span::emphasis(name_text.to_string()));
            if !call.summary.is_empty() {
                spans.push(Span::muted(format!(" {}", call.summary)));
            }
        } else {
            spans.push(Span::muted(call.summary.clone()));
        }
        lines.push(ContentLine::from_spans(spans));

        let Some(display) = preview else {
            return lines;
        };
        // The display owns its own projection, including the redundant-row
        // suppression keyed on typed tool identity (`ToolName::is_edit` /
        // `ToolName::is_nu`) plus display shape. The IR layer never names a
        // concrete tool kind, and the TUI's preview block calls the same
        // method, so the two paths cannot drift apart.
        lines.extend(display.project_lines(name));
        lines
    }

    /// Compaction/system notice — a single line in the lane role style. The
    /// `Normal` hint resolves to the lane's `role_style`
    /// (`role_compaction` / `role_system`), matching the pre-refactor
    /// `SystemMessage` styling. `Meta` would override that with the muted
    /// tool-meta color (task 46ca79fe).
    fn project_notice(text: &str) -> Vec<ContentLine> {
        vec![ContentLine::single(text.to_string(), StyleHint::Normal)]
    }

    /// Centered banner text — one normal line per source line.
    fn project_banner(text: &str) -> Vec<ContentLine> {
        text.lines()
            .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
            .collect()
    }
}

impl Block {
    /// Whether this block renders a filled region — a background surface that
    /// needs visual separation from the next tool call. True when the block
    /// carries the code fill, or when its source carries diff content (the
    /// completion-path diff display keeps `Fill::None` with `Diff*` lines).
    /// Title, label, and stats text blocks carry neither, so they stay
    /// unfilled and get no separator inside one display region.
    pub fn has_filled_content(&self) -> bool {
        self.fill == Fill::Code || self.source.has_diff_content()
    }
}

// endregion: --- Froms

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
