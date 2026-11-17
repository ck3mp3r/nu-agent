use nu_plugin::EvaluatedCall;
use nu_protocol::LabeledError;

use agnt_runtime::config::{AgentsConfig, Config};
use agnt_runtime::protocol::persona::resolve::{
    PersonaResolution, resolve_persona as resolve_persona_seam,
};

pub(crate) fn resolve_persona(
    agent_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &std::path::Path,
    agents_dir: &std::path::Path,
    call: &EvaluatedCall,
    config: &mut Config,
) -> Result<PersonaResolution, LabeledError> {
    let (_agent_name, cli_name) = super::args::extract_agent_flags(call);

    let resolution = resolve_persona_seam(
        agent_name,
        cli_name,
        agents_config,
        cwd,
        agents_dir,
        call.head,
    )?;

    log::debug!("resolved agent_identity={:?}", resolution.agent_identity);
    log::debug!(
        "agent_permissions_overlay present={}",
        resolution.agent_permissions_overlay.is_some()
    );
    log::debug!(
        "effective model after persona merge: provider={}, model={}",
        config.provider,
        config.model
    );
    log::debug!(
        "effective config after persona config merge: temperature={:?}, max_tokens={:?}, max_tool_turns={:?}",
        config.temperature,
        config.max_tokens,
        config.max_tool_turns
    );

    Ok(resolution)
}

#[cfg(test)]
#[path = "../../../test/command/agent/persona.rs"]
mod persona_test;
