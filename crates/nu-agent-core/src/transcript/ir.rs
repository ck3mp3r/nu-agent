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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub spans: Vec<Span>,
}

impl ContentLine {
    pub fn single(text: String, hint: StyleHint) -> Self {
        Self {
            spans: vec![Span::new(text, hint)],
        }
    }

    pub fn from_spans(spans: Vec<Span>) -> Self {
        Self { spans }
    }

    pub fn empty() -> Self {
        Self { spans: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBlock {
    pub role: Role,
    pub lines: Vec<ContentLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    pub text: String,
    pub hint: StyleHint,
}

impl DisplayLine {
    pub fn new(text: String, hint: StyleHint) -> Self {
        Self { text, hint }
    }
}
