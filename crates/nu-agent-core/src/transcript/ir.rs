#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
    ToolDisplay,
    System,
    Compaction,
    Separator,
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

/// A single projected content row. `hang_indent` is the number of display
/// columns the line's leading marker occupies; when the line wraps, the
/// renderer indents continuation rows by this amount so list item text stays
/// aligned under the marker text (task 7bd175d2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub spans: Vec<Span>,
    pub hang_indent: usize,
}

impl ContentLine {
    pub fn single(text: String, hint: StyleHint) -> Self {
        Self {
            spans: vec![Span::new(text, hint)],
            hang_indent: 0,
        }
    }

    pub fn from_spans(spans: Vec<Span>) -> Self {
        Self {
            spans,
            hang_indent: 0,
        }
    }

    pub fn empty() -> Self {
        Self {
            spans: vec![],
            hang_indent: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBlock {
    pub role: Role,
    pub lines: Vec<ContentLine>,
    /// Raw markdown source, present for `User` and `Assistant` prose blocks.
    /// When `Some`, the renderer should project at render time using the
    /// available canvas width rather than consuming `lines` directly.
    pub markdown: Option<String>,
    pub center: bool,
    pub suppress_prefix: bool,
}
