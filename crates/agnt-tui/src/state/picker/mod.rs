//! The picker surface: its option vocabulary, the container state machine, the
//! row construction, the match, and the reply hands.

// region:    --- Modules

mod container;
mod fuzzy;
mod replies;
mod requests;
mod rows;
mod types;

pub use types::{
    ActivePicker, PickerContainer, PickerEntry, PickerItem, PickerKeyOutcome, PickerOption,
    PickerPayload, PickerRenderKind, PickerSortKeyPart, PickerState, SubmitAction, SwitchRequest,
};

pub(crate) use fuzzy::fuzzy_matches;
pub(crate) use rows::session_day;
pub(crate) use types::{PANEL_PAGE_LINES, PICKER_REPLY_CAPACITY};

// endregion: --- Modules
