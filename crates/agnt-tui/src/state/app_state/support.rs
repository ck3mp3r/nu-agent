//! The state root's private constants: the startup banner set and the quit
//! confirmation window.

use std::time::Duration;

/// The startup banners: the one logo, embedded at compile time.
pub(super) const STARTUP_LOGOS: &[&str] = &[include_str!("../../logos/00.txt")];

/// How long an armed Ctrl-C quit confirmation stays valid. Production keeps
/// the 5 s default; tests set
/// [`AppState::quit_confirm_window`](crate::state::AppState::quit_confirm_window)
/// directly.
pub(crate) const QUIT_CONFIRM_WINDOW: Duration = Duration::from_secs(5);
