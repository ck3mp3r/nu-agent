//! Row construction: the catalog types the runtime hands the surface, turned
//! into picker options with their sort keys and section heads.

use agnt_runtime::protocol::picker::{AgentPickerOption, ModelPickerOption};

use super::types::{PickerOption, PickerPayload, PickerSortKeyPart};

impl From<ModelPickerOption> for PickerOption {
    fn from(opt: ModelPickerOption) -> Self {
        let search_text = format!(
            "{} {} {}",
            opt.identity, opt.provider, opt.provider_display_name
        );
        Self {
            id: opt.identity.clone(),
            display: opt.display.clone(),
            search_text,
            section: Some(opt.provider.clone()),
            sort_key: vec![
                PickerSortKeyPart::Asc(opt.provider.to_ascii_lowercase()),
                PickerSortKeyPart::First(!opt.configured),
                PickerSortKeyPart::Asc(opt.identity.to_ascii_lowercase()),
            ],
            payload: PickerPayload::Model {
                identity: opt.identity,
                provider: opt.provider,
                provider_display_name: opt.provider_display_name,
                configured: opt.configured,
                context_window: opt.context_window,
                cost: opt.cost,
            },
        }
    }
}

impl From<AgentPickerOption> for PickerOption {
    fn from(opt: AgentPickerOption) -> Self {
        let search_text = format!("{} {}", opt.name, opt.description.as_deref().unwrap_or(""));
        Self {
            id: opt.name.clone(),
            display: opt.display.clone(),
            search_text,
            section: None,
            sort_key: vec![PickerSortKeyPart::Asc(opt.name.to_ascii_lowercase())],
            payload: PickerPayload::Agent {
                name: opt.name,
                description: opt.description,
            },
        }
    }
}

impl From<agnt_runtime::session::SessionInfo> for PickerOption {
    fn from(info: agnt_runtime::session::SessionInfo) -> Self {
        let title = info.title.clone();
        // A known title shows as stored; an unknown one is an empty cell — never
        // an invented label.
        let display = title.clone().unwrap_or_default();
        // A row with no recorded instant carries no day head and sorts after the
        // rows that have one.
        let section = info
            .last_active
            .map(|instant| session_day(instant).to_string());
        let sort_key = match info.last_active {
            Some(instant) => vec![PickerSortKeyPart::Recent(std::cmp::Reverse(instant))],
            None => vec![PickerSortKeyPart::Unknown],
        };
        Self {
            id: info.id.clone(),
            display: display.clone(),
            search_text: display,
            section,
            sort_key,
            payload: PickerPayload::Session {
                session_id: info.id,
                title,
                created_at: info.last_active,
                message_count: info.message_count,
                agent: info.agent,
            },
        }
    }
}

pub(super) fn sort_picker_options(options: &mut [PickerOption]) {
    options.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
}

/// The local calendar day a timestamp falls on. The session options carry it
/// as their section key and the session picker labels its day heads from it, so
/// both agree.
pub(crate) fn session_day(timestamp: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDate {
    timestamp.with_timezone(&chrono::Local).date_naive()
}
