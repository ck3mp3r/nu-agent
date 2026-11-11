//! The fold contract: the surface's intake vocabulary, the folds of both
//! vocabularies, and the entry point that wraps every fold in the bookkeeping
//! shift.

use crate::interaction::{cancel::CancelController, input::UiInput};
use crate::state::AppState;
use agnt_runtime::bus::{ControlCommand, LifecycleEvent};
use agnt_runtime::protocol::events::{NarrationEvent, SessionRecord};

use super::keys::{reduce_key, reduce_wheel_event};
use super::records::reduce_input;

/// One fold input at the surface boundary: a bus record or a surface input.
///
/// The surface folds both vocabularies through one entry point, so the
/// bookkeeping that follows every fold lives in one place.
pub trait Fold {
    fn fold(self, state: &mut AppState, cancel_controller: Option<&CancelController>) -> bool;
}

/// The TUI's intake: one datum from a concern the state fold reads.
///
/// The producer ships records and events; this enum only names which concern
/// arrived. How each one folds into [`AppState`] is this crate's business.
pub enum RenderInput<'a> {
    Record(&'a SessionRecord),
    Narration(&'a NarrationEvent),
    Lifecycle(&'a LifecycleEvent),
    Control(&'a ControlCommand),
}

impl Fold for RenderInput<'_> {
    fn fold(self, state: &mut AppState, _cancel_controller: Option<&CancelController>) -> bool {
        reduce_input(state, self)
    }
}

impl Fold for UiInput {
    fn fold(self, state: &mut AppState, cancel_controller: Option<&CancelController>) -> bool {
        match self {
            UiInput::KeyPressed { key } => reduce_key(state, key, cancel_controller),
            UiInput::WheelScrolled { up, column, row } => {
                reduce_wheel_event(state, up, column, row)
            }
            // A resize performs no state mutation; the coordinator owns the
            // layout.
            UiInput::Resized { .. } => false,
        }
    }
}

/// Fold one input through the reducer.
///
/// The fold only folds: a shared UI action the fold queued is applied by the
/// caller that owns the live surface, through
/// [`drain_launch_requests`](AppState::drain_launch_requests). A replayed log
/// must not re-open the panel a persisted slash command once opened, so
/// hydration folds through here and drops the queue.
pub fn reduce<F: Fold>(
    state: &mut AppState,
    fold: F,
    cancel_controller: Option<&CancelController>,
) -> bool {
    fold_with_bookkeeping(state, |state| fold.fold(state, cancel_controller))
}

/// One bookkeeping shift per fold: the store counts the blocks it dropped, so
/// every push inside `fold` is accounted for without threading an evicted
/// out-param through the domain reducers.
fn fold_with_bookkeeping(state: &mut AppState, fold: impl FnOnce(&mut AppState) -> bool) -> bool {
    let evicted_before = state.transcript.evicted_total();
    let changed = fold(state);
    let evicted = state
        .transcript
        .evicted_total()
        .saturating_sub(evicted_before);
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}
