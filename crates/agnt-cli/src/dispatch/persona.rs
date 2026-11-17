//! clap-based persona resolution for the CLI.

use agnt_runtime::config::AgentsConfig;
use agnt_runtime::protocol::persona::{
    ParsedPersona,
    resolve::{PersonaResolution as SeamPersonaResolution, resolve_persona},
};
use agnt_runtime::tools::authz::PermissionsOverlay;
use nu_protocol::{LabeledError, Span};

use crate::error::{Error, Result};

// region:    --- Types

/// Persona and identity resolved from clap flags and config.
pub struct PersonaResolution {
    pub persona: Option<ParsedPersona>,
    /// The persona slug: the `--agent` value (or the configured default) when a
    /// persona loaded, `None` otherwise. This is the value hook envelopes carry
    /// as `agent`; it is not the front-matter display name.
    pub agent_slug: Option<String>,
    pub agent_identity: Option<String>,
    pub messaging_identity: Option<String>,
    pub agent_permissions_overlay: Option<PermissionsOverlay>,
}

// endregion: --- Types

// region:    --- Functions

/// Resolve the effective persona, identities, and permissions overlay.
///
/// Effective agent name precedence:
/// 1. `--agent` flag (validated against disabled built-ins)
/// 2. `agents.default` / `agents.fallback` from config.toml
pub fn resolve_persona_from_clap(
    agent_name: Option<String>,
    cli_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &std::path::Path,
    agents_dir: &std::path::Path,
) -> Result<PersonaResolution> {
    let span = Span::unknown();
    let seam: SeamPersonaResolution =
        resolve_persona(agent_name, cli_name, agents_config, cwd, agents_dir, span)
            .map_err(Error::Runtime)?;

    Ok(PersonaResolution {
        persona: seam.persona,
        agent_slug: seam.agent_slug,
        agent_identity: seam.agent_identity,
        messaging_identity: seam.messaging_identity,
        agent_permissions_overlay: seam.agent_permissions_overlay,
    })
}

/// Resolve the persona an A2A peer serves under.
///
/// A peer stamps its persona into its hook envelopes, its settled facts, its
/// agent card, and its telemetry. The configured default persona is an
/// interactive convenience, so a peer must name its own and the peer path
/// refuses to serve without one.
pub(crate) fn resolve_peer_persona_from_clap(
    agent_name: Option<String>,
    cli_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &std::path::Path,
    agents_dir: &std::path::Path,
) -> Result<PersonaResolution> {
    if agent_name.is_none() {
        return Err(Error::Runtime(
            LabeledError::new(
                "a2a peer requires a persona: pass --agent <name> (the peer's identity, \
                 hook envelopes, facts, and agent card stamp from it)",
            )
            .with_label("missing --agent", Span::unknown()),
        ));
    }

    resolve_persona_from_clap(agent_name, cli_name, agents_config, cwd, agents_dir)
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/dispatch/persona.rs"]
mod persona_test;

// endregion: --- Tests
