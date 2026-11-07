//! The transcript record: one persisted row of a session's transcript, and the
//! identity it derives from its own content.
//!
//! The store owns the transcript fold (D19). The kernel's stream records are the
//! fold's input; this record is its output, so the persisted shape does not move
//! when the kernel's record vocabulary does: no field here names an `agnt-kernel`
//! symbol.
//!
//! The record carries data only. How a row is drawn — the call line, the
//! sections, the mark of a call that produced no result — is the drawer's
//! derivation from these fields, never a field here.

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
    session: String,
    row_seq: i64,
    row: TranscriptRow,
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
    /// `arguments`, `result`, `success` and `status` are each absent when the
    /// store never recorded them: a call the turn stopped carries its arguments
    /// and no result, and a call no attribution reached carries no status.
    Tool {
        name: String,
        arguments: Option<String>,
        result: Option<String>,
        success: Option<bool>,
        status: Option<ToolCallStatus>,
    },
    /// The summary a compaction left in place of the region it swept.
    Compaction { content: String },
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

/// Identity of one transcript row, derived from the row's own field set.
///
/// The fingerprint is 64 bits of FNV-1a over the canonical encoding
/// [`TranscriptRecord::id`] takes. The collision domain is **per store**: within
/// one database a row is a position of one session's transcript, and that
/// position is part of the encoded field set, so two rows of one store collide
/// only when they are the same row. Across stores the same content folds to the
/// same id, which is what lets a replayed row be the row it replays.
///
/// Nothing supplies an id: the record derives it, and no constructor accepts one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranscriptId(String);

// endregion: --- Types

// region:    --- Constructors

impl TranscriptRecord {
    /// The row at position `row_seq` of `session`.
    pub fn new(session: impl Into<String>, row_seq: i64, row: TranscriptRow) -> Self {
        Self {
            session: session.into(),
            row_seq,
            row,
        }
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

    /// The kind of row this is, with its payload.
    pub fn row(&self) -> &TranscriptRow {
        &self.row
    }

    /// The row's identity, derived from its content.
    ///
    /// The field set is encoded in one order, every string length-prefixed and
    /// every optional part tagged, so two different rows never encode alike and
    /// the same row encodes alike in any process. The scheme is written out
    /// here rather than borrowed from a library: a persisted id that moved with
    /// a toolchain release would orphan every row it named.
    pub fn id(&self) -> TranscriptId {
        let mut fold = Fold::new();
        fold.text(&self.session);
        fold.int(self.row_seq);
        match &self.row {
            TranscriptRow::User { content, stamp } => {
                fold.byte(TAG_USER).text(content);
                match stamp {
                    Some(stamp) => {
                        fold.byte(PRESENT)
                            .int(stamp.millis)
                            .opt_text(stamp.model.as_deref())
                            .opt_text(stamp.agent.as_deref());
                    }
                    None => {
                        fold.byte(ABSENT);
                    }
                }
            }
            TranscriptRow::Assistant { content } => {
                fold.byte(TAG_ASSISTANT).text(content);
            }
            TranscriptRow::System { content } => {
                fold.byte(TAG_SYSTEM).text(content);
            }
            TranscriptRow::Tool {
                name,
                arguments,
                result,
                success,
                status,
            } => {
                fold.byte(TAG_TOOL)
                    .text(name)
                    .opt_text(arguments.as_deref())
                    .opt_text(result.as_deref())
                    .opt_flag(*success)
                    .opt_status(*status);
            }
            TranscriptRow::Compaction { content } => {
                fold.byte(TAG_COMPACTION).text(content);
            }
        }
        TranscriptId(format!("transcript-{:016x}", fold.digest()))
    }
}

impl TranscriptId {
    /// The id as text: the value the store keys a row by.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// endregion:    --- Accessors

// region:    --- Support

/// The 64-bit FNV-1a offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// The 64-bit FNV-1a prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The tag that marks an optional part as carried.
const PRESENT: u8 = 1;

/// The tag that marks an optional part as absent.
const ABSENT: u8 = 0;

/// The tag that opens a user row's encoding.
const TAG_USER: u8 = 1;

/// The tag that opens an assistant row's encoding.
const TAG_ASSISTANT: u8 = 2;

/// The tag that opens a system row's encoding.
const TAG_SYSTEM: u8 = 3;

/// The tag that opens a tool row's encoding.
const TAG_TOOL: u8 = 4;

/// The tag that opens a compaction row's encoding.
const TAG_COMPACTION: u8 = 5;

/// The canonical byte stream a row's identity hashes.
///
/// Every write is self-delimiting: a string carries its length in front of its
/// bytes, an integer is eight bytes, and an optional part carries a presence
/// byte. A different field set therefore cannot produce the same bytes.
struct Fold {
    hash: u64,
}

impl Fold {
    fn new() -> Self {
        Self { hash: FNV_OFFSET }
    }

    fn byte(&mut self, byte: u8) -> &mut Self {
        self.hash = (self.hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        self
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        for byte in bytes {
            self.byte(*byte);
        }
        self
    }

    fn text(&mut self, text: &str) -> &mut Self {
        self.bytes(&(text.len() as u64).to_le_bytes())
            .bytes(text.as_bytes())
    }

    fn int(&mut self, value: i64) -> &mut Self {
        self.bytes(&value.to_le_bytes())
    }

    fn opt_text(&mut self, text: Option<&str>) -> &mut Self {
        match text {
            Some(text) => self.byte(PRESENT).text(text),
            None => self.byte(ABSENT),
        }
    }

    fn opt_flag(&mut self, value: Option<bool>) -> &mut Self {
        match value {
            Some(value) => self.byte(PRESENT).byte(u8::from(value)),
            None => self.byte(ABSENT),
        }
    }

    fn opt_status(&mut self, status: Option<ToolCallStatus>) -> &mut Self {
        match status {
            Some(status) => self.byte(PRESENT).text(status.as_str()),
            None => self.byte(ABSENT),
        }
    }

    fn digest(&self) -> u64 {
        self.hash
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/transcript.rs"]
mod transcript_test;

// endregion: --- Tests
