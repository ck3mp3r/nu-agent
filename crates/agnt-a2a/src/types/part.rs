use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Part content types — wrapper structs for the untagged file/data variants
// ---------------------------------------------------------------------------

/// Content of a file part (A2A spec §6.7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileContent {
    pub url: String,
    pub filename: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
}

/// Content of a data part (A2A spec §6.7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataContent {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub schema: Value,
}

// ---------------------------------------------------------------------------
// Part (A2A spec §6.7)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    Text { text: String },
    File { file: FileContent },
    Data { data: DataContent },
}
