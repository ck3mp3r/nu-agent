//! The row's shapes: one block of a transcript, the payload its kind carries,
//! and the footer facts a user row records.

// region:    --- Modules

use crate::services::ToolCallStatus;

// endregion: --- Modules

// region:    --- Types

/// One transcript row: the data the store keeps for one block of a session's
/// transcript, and the identity it derives from that data.
///
/// The id is a function of the fields, so it can never disagree with them: there
/// is no constructor that takes one, and no field holds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptRecord {
    pub(super) session: String,
    pub(super) row_seq: i64,
    pub(super) turn_seq: Option<i64>,
    pub(super) row: TranscriptRow,
}

/// The kind of a transcript row, with the payload that kind carries.
///
/// The variant is the classification; a consumer that acts on the difference
/// matches it, and the layer that prints a block owns the text it prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptRow {
    /// The user's own text, with the footer facts its settled turn recorded.
    User {
        content: String,
        stamp: Option<TranscriptStamp>,
    },
    /// The model's answer.
    Assistant { content: String },
    /// A system message the session recorded.
    System { content: String },
    /// One tool call, and the outcome it settled with.
    ///
    /// `call_id` is the join key the entries carry: the id the model gave the
    /// call, which the answer to it names in turn. `name` is the name the model
    /// asked for; `executed_name` is the name the result carries, which differs
    /// after a hook repaired the call.
    ///
    /// `arguments`, `result`, `success` and `status` are each absent when the
    /// store never recorded them: a call the turn stopped carries its arguments
    /// and no result, and a call no attribution reached carries no status.
    Tool {
        call_id: String,
        name: String,
        executed_name: Option<String>,
        arguments: Option<String>,
        result: Option<String>,
        success: Option<bool>,
        status: Option<ToolCallStatus>,
    },
    /// The summary a compaction left in place of the region it swept.
    ///
    /// The variant is the author's classification: a block authored by the
    /// compaction is this row, and every other block is the user's own.
    Compaction { content: String },
    /// A block that is not text: an image, audio, video or document.
    ///
    /// `owner` names the entry kind that carried it. `text` is the document's
    /// own text, when the block carried text instead of a file. `detail` is the
    /// preference the block named, verbatim: a token this store never reads.
    Media {
        owner: MediaOwner,
        kind: MediaKind,
        source: MediaSource,
        text: Option<String>,
        media_type: Option<String>,
        detail: Option<String>,
    },
    /// The reasoning a model showed: its text, or a redacted block with none.
    Reasoning { text: String, redacted: bool },
    /// A provider item with no canonical meaning, carried verbatim.
    ///
    /// `replay` says whether the item goes back to the model that produced it;
    /// an item nothing answers is kept but never sent.
    Opaque { item: String, replay: bool },
}

/// The entry kind that carried a media block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOwner {
    /// The user's own content.
    User,
    /// The model's output.
    Assistant,
    /// The content a tool call answered with.
    ToolResult,
}

/// The kind of a media block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// An image.
    Image,
    /// Audio.
    Audio,
    /// Video.
    Video,
    /// A document.
    Document,
}

/// Where a media block's bytes are, or the bytes themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// A file URL.
    Url(String),
    /// A base-64 encoded string.
    Base64(String),
    /// A provider-side uploaded file identifier.
    FileId(String),
    /// The raw bytes.
    Raw(Vec<u8>),
    /// A source the block did not name.
    Unknown,
}

/// The footer facts one user row carries: the instant its turn was submitted,
/// and the identities the turn ran under.
///
/// A stamp exists only for a turn that recorded an instant which can stand
/// behind a footer, so `millis` is not optional. The identities are: a turn
/// without a persona records none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptStamp {
    /// When the turn was submitted, in milliseconds since the Unix epoch.
    pub millis: i64,
    /// The model identity the turn ran under, `vendor/model`.
    pub model: Option<String>,
    /// The persona identity that answered the turn.
    pub agent: Option<String>,
}

// endregion: --- Types

// region:    --- Constructors

impl TranscriptRecord {
    /// The row at position `row_seq` of `session`, with no turn bound yet.
    pub fn new(session: impl Into<String>, row_seq: i64, row: TranscriptRow) -> Self {
        Self {
            session: session.into(),
            row_seq,
            turn_seq: None,
            row,
        }
    }

    /// This row bound to the turn that persisted it.
    ///
    /// The binding is the row's causal anchor, so it sits beside the session
    /// and the position rather than inside any one kind's payload: every kind
    /// of row can be the one a turn wrote.
    pub fn with_turn_seq(mut self, turn_seq: Option<i64>) -> Self {
        self.turn_seq = turn_seq;
        self
    }
}

// endregion: --- Constructors

// region:    --- Accessors

impl TranscriptRecord {
    /// The session the row belongs to.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// The row's position within its session's transcript.
    pub fn row_seq(&self) -> i64 {
        self.row_seq
    }

    /// The turn that persisted the row, `None` when no settled turn holds it.
    pub fn turn_seq(&self) -> Option<i64> {
        self.turn_seq
    }

    /// The kind of row this is, with its payload.
    pub fn row(&self) -> &TranscriptRow {
        &self.row
    }
}

impl MediaOwner {
    /// The value the `media_owner` column's CHECK accepts.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolResult => "tool_result",
        }
    }

    /// The owner `stored` names; `None` when it names none of them.
    pub(crate) fn parse(stored: &str) -> Option<Self> {
        match stored {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool_result" => Some(Self::ToolResult),
            _ => None,
        }
    }
}

impl MediaKind {
    /// The value the `media_kind` column's CHECK accepts.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Document => "document",
        }
    }

    /// The kind `stored` names; `None` when it names none of them.
    pub(crate) fn parse(stored: &str) -> Option<Self> {
        match stored {
            "image" => Some(Self::Image),
            "audio" => Some(Self::Audio),
            "video" => Some(Self::Video),
            "document" => Some(Self::Document),
            _ => None,
        }
    }
}

impl MediaSource {
    /// The value the `media_source` column's CHECK accepts.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Url(_) => URL,
            Self::Base64(_) => BASE64,
            Self::FileId(_) => FILE_ID,
            Self::Raw(_) => RAW,
            Self::Unknown => UNKNOWN,
        }
    }

    /// The source `stored` names, carrying `value` or `bytes` as it takes them.
    ///
    /// A source that names none of the five is `None`: the store refuses a row
    /// it could not have written.
    pub(crate) fn parse(
        stored: &str,
        value: Option<String>,
        bytes: Option<Vec<u8>>,
    ) -> Option<Self> {
        match stored {
            URL => Some(Self::Url(value?)),
            BASE64 => Some(Self::Base64(value?)),
            FILE_ID => Some(Self::FileId(value?)),
            RAW => Some(Self::Raw(bytes?)),
            UNKNOWN => Some(Self::Unknown),
            _ => None,
        }
    }
}

// endregion: --- Accessors

// region:    --- Support

/// The token that names a URL source.
pub(super) const URL: &str = "url";

/// The token that names a base-64 source.
pub(super) const BASE64: &str = "base64";

/// The token that names a provider file id source.
pub(super) const FILE_ID: &str = "file_id";

/// The token that names a raw-bytes source.
pub(super) const RAW: &str = "raw";

/// The token that names a source the block did not name.
pub(super) const UNKNOWN: &str = "unknown";

// endregion: --- Support
