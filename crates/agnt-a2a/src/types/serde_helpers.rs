use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serializer};

// ---------------------------------------------------------------------------
// RFC 3339 timestamps with `Z` suffix (A2A spec §5.6.1)
// ---------------------------------------------------------------------------

/// Serialize a `DateTime<Utc>` as `YYYY-MM-DDTHH:mm:ss.sssZ`.
///
/// Spec §5.6.1: timestamps MUST NOT include timezone offsets other than `Z`.
pub fn serialize_rfc3339_z<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// Deserialize a `DateTime<Utc>` from RFC 3339.
///
/// Accepts both the `Z` suffix and an explicit `+00:00` offset so clients that
/// send the pre-§5.6.1 format keep working.
pub fn deserialize_rfc3339_flexible<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_rfc3339(&raw).map_err(serde::de::Error::custom)
}

/// Serialize an optional `DateTime<Utc>`; `None` becomes JSON `null`.
pub fn serialize_opt_rfc3339_z<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match dt {
        Some(dt) => serialize_rfc3339_z(dt, serializer),
        None => serializer.serialize_none(),
    }
}

/// Deserialize an optional `DateTime<Utc>`; JSON `null` becomes `None`.
pub fn deserialize_opt_rfc3339_flexible<'de, D>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw {
        Some(raw) => parse_rfc3339(&raw)
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

// region:    --- Support

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("invalid RFC 3339 timestamp '{raw}': {e}"))
}

// endregion: --- Support
