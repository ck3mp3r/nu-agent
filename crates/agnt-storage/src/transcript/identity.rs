//! The identity a transcript row derives from its own field set.

// region:    --- Modules

use super::record::{
    BASE64, FILE_ID, MediaSource, RAW, TranscriptRecord, TranscriptRow, UNKNOWN, URL,
};
use crate::services::ToolCallStatus;

// endregion: --- Modules

// region:    --- Types

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

// endregion:    --- Types

// region:    --- Accessors

impl TranscriptRecord {
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
        fold.opt_int(self.turn_seq);
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
                call_id,
                name,
                executed_name,
                arguments,
                result,
                success,
                status,
            } => {
                fold.byte(TAG_TOOL)
                    .text(call_id)
                    .text(name)
                    .opt_text(executed_name.as_deref())
                    .opt_text(arguments.as_deref())
                    .opt_text(result.as_deref())
                    .opt_flag(*success)
                    .opt_status(*status);
            }
            TranscriptRow::Compaction { content } => {
                fold.byte(TAG_COMPACTION).text(content);
            }
            TranscriptRow::Media {
                owner,
                kind,
                source,
                text,
                media_type,
                detail,
            } => {
                fold.byte(TAG_MEDIA)
                    .text(owner.as_str())
                    .text(kind.as_str());
                match source {
                    MediaSource::Url(url) => fold.text(URL).text(url),
                    MediaSource::Base64(encoded) => fold.text(BASE64).text(encoded),
                    MediaSource::FileId(id) => fold.text(FILE_ID).text(id),
                    MediaSource::Raw(bytes) => fold.text(RAW).blob(bytes),
                    MediaSource::Unknown => fold.text(UNKNOWN),
                };
                fold.opt_text(text.as_deref())
                    .opt_text(media_type.as_deref())
                    .opt_text(detail.as_deref());
            }
            TranscriptRow::Reasoning { text, redacted } => {
                fold.byte(TAG_REASONING)
                    .text(text)
                    .byte(u8::from(*redacted));
            }
            TranscriptRow::Opaque { item, replay } => {
                fold.byte(TAG_OPAQUE).text(item).byte(u8::from(*replay));
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

/// The tag that opens a media row's encoding.
const TAG_MEDIA: u8 = 6;

/// The tag that opens a reasoning row's encoding.
const TAG_REASONING: u8 = 7;

/// The tag that opens an opaque row's encoding.
const TAG_OPAQUE: u8 = 8;

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

    fn blob(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes(&(bytes.len() as u64).to_le_bytes()).bytes(bytes)
    }

    fn int(&mut self, value: i64) -> &mut Self {
        self.bytes(&value.to_le_bytes())
    }

    fn opt_int(&mut self, value: Option<i64>) -> &mut Self {
        match value {
            Some(value) => self.byte(PRESENT).int(value),
            None => self.byte(ABSENT),
        }
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
