use nu_plugin::EvaluatedCall;
use nu_protocol::LabeledError;

use nu_agent_core::config::{AgentsConfig, Config};
use nu_agent_core::tools::authz::PermissionsOverlay;

pub(crate) struct PersonaResolution {
    pub(crate) persona: Option<nu_agent_core::protocol::persona::ParsedPersona>,
    pub(crate) agent_identity: Option<String>,
    pub(crate) messaging_identity: Option<String>,
    pub(crate) agent_permissions_overlay: Option<PermissionsOverlay>,
}

pub(crate) fn resolve_persona(
    agent_name: Option<String>,
    cli_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &std::path::Path,
    call: &EvaluatedCall,
    config: &mut Config,
) -> Result<PersonaResolution, LabeledError> {
    use super::permissions::{is_builtin_enabled, resolve_default_agent};
    use nu_agent_core::protocol::persona::{
        FrontMatterParser, FsPersonaResolver, PersonaFileResolver, PulldownCmarkFrontMatterParser,
        interpret_front_matter,
    };

    // Determine effective agent name:
    // 1. CLI --agent flag provided → validate it's not a disabled built-in
    // 2. No CLI flag → resolve from config default/fallback
    let effective_agent_name = if let Some(name) = agent_name {
        if nu_agent_core::protocol::persona::builtins::is_builtin_persona(&name)
            && !is_builtin_enabled(&name, agents_config)
        {
            return Err(LabeledError::new(format!(
                "Agent '{}' is disabled in config. Enable it or use a different agent.",
                name
            ))
            .with_label("disabled agent", call.head));
        }
        Some(name)
    } else {
        resolve_default_agent(agents_config)?
    };
    log::debug!("effective_agent_name={effective_agent_name:?}");

    let persona = if let Some(name) = &effective_agent_name {
        let cwd = std::path::PathBuf::from(cwd);
        let config_dir = nu_agent_core::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .map_err(|e| {
                LabeledError::new("Cannot determine config directory")
                    .with_label(e.to_string(), call.head)
            })?;

        let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
        let (_path, contents) = resolver.resolve(name).map_err(|e| {
            LabeledError::new("Agent persona not found").with_label(e.to_string(), call.head)
        })?;

        let parser = PulldownCmarkFrontMatterParser;
        let raw = parser.parse(&contents).map_err(|e| {
            LabeledError::new("Invalid agent persona front matter")
                .with_label(e.to_string(), call.head)
        })?;

        // Interpret front matter into typed fields
        Some(
            interpret_front_matter(raw.front_matter.as_ref(), raw.body).map_err(|e| {
                LabeledError::new("Invalid agent persona front matter")
                    .with_label(e.to_string(), call.head)
            })?,
        )
    } else {
        None
    };
    log::debug!(
        "persona loaded: name={:?}, model={:?}, has_permissions={}, body_len={}",
        persona.as_ref().and_then(|p| p.name.as_ref()),
        persona.as_ref().and_then(|p| p.model.as_ref()),
        persona.as_ref().is_some_and(|p| p.permissions.is_some()),
        persona.as_ref().map_or(0, |p| p.body.len())
    );

    // Display identity: persona name > effective agent name (never --name)
    let agent_identity = persona
        .as_ref()
        .and_then(|p| p.name.clone())
        .or_else(|| effective_agent_name.clone());
    // Messaging identity: --name > display identity (for multi-agent communication)
    let messaging_identity = cli_name.or_else(|| agent_identity.clone());
    log::debug!(
        "resolved agent_identity={agent_identity:?}, messaging_identity={messaging_identity:?}"
    );

    // Wire permissions field
    let agent_permissions_overlay = persona
        .as_ref()
        .and_then(|p| p.permissions.as_ref())
        .map(PermissionsOverlay::parse_from_yaml)
        .transpose()
        .map_err(|msg| LabeledError::new("Invalid agent permissions").with_label(msg, call.head))?;
    log::debug!(
        "agent_permissions_overlay present={}",
        agent_permissions_overlay.is_some()
    );

    // Wire model with precedence: CLI --model > front matter model > plugin config
    // Config already has plugin/env/default merged, we just need to inject persona model if CLI didn't provide one.
    // NOTE: apply_persona_model is called from run_command.rs where PluginConfig is available
    // for role label resolution. The persona model value is passed through the PersonaResolution.
    log::debug!(
        "effective model after persona merge: provider={}, model={}",
        config.provider,
        config.model
    );

    // NOTE: apply_persona_config is now called from run_command.rs after
    // apply_persona_model, so persona front matter overrides are applied
    // in the correct order relative to CLI flags.
    log::debug!(
        "effective config after persona config merge: temperature={:?}, max_tokens={:?}, max_tool_turns={:?}",
        config.temperature,
        config.max_tokens,
        config.max_tool_turns
    );

    Ok(PersonaResolution {
        persona,
        agent_identity,
        messaging_identity,
        agent_permissions_overlay,
    })
}

#[cfg(test)]
#[path = "persona_test.rs"]
mod persona_test;
