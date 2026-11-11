//! The picker's option vocabulary: what a row is, what it carries, and the
//! selection state one picker kind holds.

use crate::state::CommandPaletteAction;
use agnt_runtime::protocol::slash::SlashCommand;

use super::fuzzy::fuzzy_matches;

/// Rows a PgUp/PgDn (Ctrl-U/Ctrl-D) page moves in any popup.
pub(crate) const PANEL_PAGE_LINES: usize = 8;

/// Depth of each picker's reply hand. A catalog is answered once per request
/// and a surface opens one picker at a time, so the hand never needs to hold
/// more than the answer to the request in flight.
pub(crate) const PICKER_REPLY_CAPACITY: usize = 4;

/// A single sort-key part attached at the conversion boundary. The picker
/// orders options by comparing these parts with ONE generic comparator; an
/// empty key under a stable sort preserves insertion order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PickerSortKeyPart {
    /// Ascending string part (producer lowercases).
    Asc(String),
    /// Boolean part: false sorts before true; pass `!flag` when flag-true
    /// should sort first.
    First(bool),
    /// Recency part, latest sorts first (wrapped in Reverse so the derived
    /// Ord yields latest-first under a plain `cmp`).
    Recent(std::cmp::Reverse<chrono::DateTime<chrono::Utc>>),
    /// Rows with no recorded instant. Declared last, so they sort after every
    /// row that has one: the picker never invents a time to sort by.
    Unknown,
}

pub trait PickerItem: Clone {
    fn display(&self) -> String;
    fn id(&self) -> String;
    fn matches_query(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        fuzzy_matches(&q, &self.display().to_ascii_lowercase())
            || fuzzy_matches(&q, &self.id().to_ascii_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct PickerOption {
    pub id: String,
    pub display: String,
    pub search_text: String,
    /// The group this option belongs to; `None` renders the picker flat. The
    /// picker table derives one section-head row per contiguous run of the same
    /// key from the FILTERED options, so the heads are never stored as items.
    pub section: Option<String>,
    pub payload: PickerPayload,
    /// Sort key attached at the conversion boundary; empty = keep insertion order.
    pub sort_key: Vec<PickerSortKeyPart>,
}

impl PickerItem for PickerOption {
    fn display(&self) -> String {
        self.display.clone()
    }
    fn id(&self) -> String {
        self.id.clone()
    }
    fn matches_query(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        fuzzy_matches(&q, &self.display.to_ascii_lowercase())
            || fuzzy_matches(&q, &self.id.to_ascii_lowercase())
            || fuzzy_matches(&q, &self.search_text.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone)]
pub enum PickerPayload {
    Model {
        identity: String,
        provider: String,
        provider_display_name: String,
        configured: bool,
        context_window: Option<u32>,
        cost: Option<String>,
    },
    Agent {
        name: String,
        description: Option<String>,
    },
    Session {
        session_id: String,
        title: Option<String>,
        /// The recorded change instant, or `None` when the store holds none.
        created_at: Option<chrono::DateTime<chrono::Utc>>,
        message_count: usize,
        /// The agent of the session's most recent recorded turn, when named.
        agent: Option<String>,
    },
    Theme,
    Command(CommandPaletteAction),
    Slash(SlashCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchRequest {
    Model(String),
    Agent(String),
    Session(String),
    Theme(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitAction {
    Switch(SwitchRequest),
    Launch(ActivePicker),
    Command(CommandPaletteAction),
    SlashAccept,
}

/// What the active picker did with a key. `Pass` hands the key to the lenses
/// below the picker (the permission card, the agent chord, the keymap).
pub enum PickerKeyOutcome {
    /// The picker owns the key; the payload reports whether state changed.
    Consumed(bool),
    /// The picker resolved a submit action for the reducer to apply.
    Submit(SubmitAction),
    /// The command palette resolves Ctrl-C to a quit.
    Quit,
    /// Not the picker's key.
    Pass,
}

#[derive(Debug, Clone)]
pub struct PickerEntry {
    pub kind: ActivePicker,
    pub state: PickerState<PickerOption>,
    pub edit_query: bool,
    pub submit: SubmitAction,
}

#[derive(Debug, Clone)]
pub struct PickerState<T: PickerItem> {
    pub open: bool,
    pub query: String,
    pub selection: usize,
    pub options: Vec<T>,
}

impl<T: PickerItem> Default for PickerState<T> {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selection: 0,
            options: Vec::new(),
        }
    }
}

impl<T: PickerItem> PickerState<T> {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selection = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selection = 0;
    }
    pub fn filtered(&self) -> Vec<T> {
        if self.query.is_empty() {
            return self.options.clone();
        }
        let q = self.query.to_ascii_lowercase();
        self.options
            .iter()
            .filter(|o| o.matches_query(&q))
            .cloned()
            .collect()
    }
    pub fn selected(&self) -> Option<T> {
        self.filtered().get(self.selection).cloned()
    }
    pub fn move_up(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = if self.selection == 0 {
            len.saturating_sub(1)
        } else {
            self.selection.saturating_sub(1)
        };
    }
    pub fn move_down(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = (self.selection + 1) % len;
    }
    /// Move the selection one page up, clamped to the first row.
    pub fn move_page_up(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = self.selection.saturating_sub(PANEL_PAGE_LINES).min(len - 1);
    }
    /// Move the selection one page down, clamped to the last row.
    pub fn move_page_down(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.selection = 0;
            return;
        }
        self.selection = self.selection.saturating_add(PANEL_PAGE_LINES).min(len - 1);
    }
    pub fn append_query_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selection = 0;
    }
    pub fn backspace_query_char(&mut self) {
        self.query.pop();
        self.selection = 0;
    }
    pub fn clamp_selection(&mut self, len: usize) {
        if len == 0 {
            self.selection = 0;
        } else if self.selection >= len {
            self.selection = len.saturating_sub(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePicker {
    CommandPalette,
    Model,
    Agent,
    Session,
    Theme,
    InlineSlash,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerRenderKind {
    CommandPalette,
    Model,
    Agent,
    Session,
    Theme,
    InlineSlash,
}

#[derive(Debug, Clone)]
pub struct PickerContainer {
    pub entries: Vec<PickerEntry>,
}
