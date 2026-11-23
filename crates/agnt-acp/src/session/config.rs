//! The session's config options: read them, and switch model or agent.

use agent_client_protocol::schema::v1::SessionConfigOption;
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::protocol::model_switching::ModelSwitching;

use crate::error::{AcpError, Result};

use super::support::{agent_options, model_options, select_values};
use super::types::{AcpSessionManager, RuntimeBuilder};

impl<R: RuntimeBuilder> AcpSessionManager<R> {
    /// The model config options advertised for `session_id`.
    ///
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn model_config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        Ok(model_options(&runtime, self.models_cache.as_deref()))
    }

    /// The agent config options advertised for `session_id`.
    ///
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn agent_config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        Ok(agent_options(&runtime))
    }

    /// The agent and model config options advertised for `session_id`.
    ///
    /// The agent option comes first. Every response that changes one selector
    /// carries both, so the client never drops the other.
    /// Returns [`AcpError::SessionNotFound`] when no live session has that id.
    pub async fn config_options(&self, session_id: &str) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let runtime = session.runtime.lock().await;
        let mut options = agent_options(&runtime);
        options.extend(model_options(&runtime, self.models_cache.as_deref()));
        Ok(options)
    }

    /// Switch `session_id` to `model_value` and return the updated config options.
    ///
    /// `model_value` must be one of the values advertised by
    /// [`Self::model_config_options`]; anything else is rejected with
    /// [`AcpError::Config`]. The model is rebuilt from the session's cached
    /// provider client, so no new transport is created. Returns
    /// [`AcpError::SessionNotFound`] when no live session has that id.
    pub(crate) async fn set_model(
        &self,
        session_id: &str,
        model_value: &str,
    ) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let mut runtime = session.runtime.lock().await;
        let options = model_options(&runtime, self.models_cache.as_deref());
        if !select_values(&options)
            .iter()
            .any(|value| value == model_value)
        {
            return Err(AcpError::Config(format!(
                "unknown model '{model_value}' for session '{session_id}'"
            )));
        }
        // The identity in use before the switch, so the event reports where the
        // session came from. `Config` formats as `provider/model`, matching the
        // identity the core switch publishes.
        let from = format!(
            "{}/{}",
            runtime.provider_config().provider,
            runtime.provider_config().model
        );
        let model = model_value
            .split_once('/')
            .map(|(_, model)| model)
            .unwrap_or(model_value);
        let previous_model = runtime.rename_model(model);
        let dyn_model = match runtime.build_dyn_model().await {
            Ok(dyn_model) => dyn_model,
            Err(e) => {
                runtime.rename_model(&previous_model);
                return Err(AcpError::Runtime(format!(
                    "failed to build model '{model_value}': {e}"
                )));
            }
        };
        runtime.replace_dyn_model(dyn_model);
        let to = format!(
            "{}/{}",
            runtime.provider_config().provider,
            runtime.provider_config().model
        );
        // Hook envelopes follow the active model, and the switch publishes the
        // event that tells a hook it moved. This path swaps the model without
        // the core switch, so both steps live here too. The send is best
        // effort: the switch already took effect. The other core-switch steps
        // stay deliberately absent here — no context-window gate, no
        // compaction-threshold refresh, no session state record — which is
        // pre-existing behavior, not an omission of this publish.
        runtime.hook_context().set_model(to.clone());
        let _ = session
            .bus
            .lifecycle()
            .publish(LifecycleEvent::ModelSwitched { from, to });
        let mut options = model_options(&runtime, self.models_cache.as_deref());
        options.extend(agent_options(&runtime));
        Ok(options)
    }

    /// Switch `session_id` to `agent_name` and return the updated config options.
    ///
    /// `agent_name` must name an available persona; anything else is rejected
    /// with [`AcpError::Config`]. The switch runs the core agent-switch flow, so
    /// the persona body, model, config overrides, permissions overlay, session
    /// grants, and tool definitions all follow the same path as the plugin and
    /// TUI runtimes. Returns [`AcpError::SessionNotFound`] when no live session
    /// has that id.
    pub async fn set_agent(
        &self,
        session_id: &str,
        agent_name: &str,
    ) -> Result<Vec<SessionConfigOption>> {
        let session = self.session(session_id)?;
        let mut runtime = session.runtime.lock().await;
        runtime
            .switch_agent(agent_name)
            .await
            .map_err(AcpError::Config)?;
        let mut options = agent_options(&runtime);
        options.extend(model_options(&runtime, self.models_cache.as_deref()));
        Ok(options)
    }
}
