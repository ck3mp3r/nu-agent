//! The production composition of the four stage capabilities.

use crate::orchestrator::loop_impl::Stages;
use crate::orchestrator::stages::permission::PermissionStage;
use crate::orchestrator::stages::session::SessionStage;
use crate::orchestrator::stages::slash::SlashStage;
use crate::orchestrator::stages::ui_request::UiRequestStage;

/// The stages a production run dispatches to.
///
/// One owner per capability. The loop borrows each into [`Stages`] for the run,
/// so no phase builds a stage of its own.
pub(crate) struct ProductionStages {
    slash: SlashStage,
    permission: PermissionStage,
    ui_request: UiRequestStage,
    session: SessionStage,
}

impl ProductionStages {
    /// The production stages, with the UI request stage seeded from
    /// `initial_visible_count`.
    pub(crate) fn new(initial_visible_count: usize) -> Self {
        Self {
            slash: SlashStage,
            permission: PermissionStage,
            ui_request: UiRequestStage::new(initial_visible_count),
            session: SessionStage,
        }
    }

    /// Borrow each capability into the value the loop consumes.
    pub(crate) fn stages(
        &mut self,
    ) -> Stages<'_, SlashStage, PermissionStage, UiRequestStage, SessionStage> {
        Stages {
            slash: &mut self.slash,
            permission: &mut self.permission,
            ui_request: &mut self.ui_request,
            session: &mut self.session,
        }
    }
}
