use crate::config::Config;

pub(crate) struct PersonaState {
    pub(crate) agent_persona_body: Option<String>,
    pub(crate) agent_identity: Option<String>,
    pub(crate) agent_description: Option<String>,
    pub(crate) cached_agents_chain: Option<String>,
    pub(crate) cached_available_skills: Option<String>,
    pub(crate) cached_sub_agent_instruction: Option<String>,
}

/// Result of a successful agent switch, carrying side-effects the caller must apply.
pub(crate) struct SwitchAgentResult {
    pub(crate) identity: String,
    pub(crate) model: Option<String>,
}

impl PersonaState {
    pub(crate) fn switch_agent(
        &mut self,
        agent_name: &str,
        cwd: &std::path::Path,
        agents_config: &crate::config::AgentsConfig,
    ) -> Result<SwitchAgentResult, String> {
        use crate::agent::protocol::persona::{
            FrontMatterParser, FsPersonaResolver, PersonaFileResolver,
            PulldownCmarkFrontMatterParser, interpret_front_matter,
        };

        let config_dir = crate::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .map_err(|e| format!("agent switch failed: cannot determine config directory: {e}"))?;

        let resolver = FsPersonaResolver::new(cwd.to_path_buf(), config_dir, agents_config.clone());
        let (_path, contents) = resolver
            .resolve(agent_name)
            .map_err(|e| format!("agent switch failed: {e}"))?;

        let parser = PulldownCmarkFrontMatterParser;
        let raw = parser
            .parse(&contents)
            .map_err(|e| format!("agent switch failed: invalid front matter: {e}"))?;

        let parsed = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
            .map_err(|e| format!("agent switch failed: invalid front matter fields: {e}"))?;

        // Update persona body
        self.agent_persona_body = Some(parsed.body);

        // Resolve identity: front matter name > agent_name argument
        let identity = parsed.name.unwrap_or_else(|| agent_name.to_string());
        self.agent_identity = Some(identity.clone());
        self.agent_description = parsed.description;

        log::debug!(
            "switch_agent: switched to identity={identity:?}, model={:?}, body_len={}",
            parsed.model,
            self.agent_persona_body.as_ref().map_or(0, |b| b.len())
        );

        Ok(SwitchAgentResult {
            identity,
            model: parsed.model,
        })
    }

    pub(crate) fn active_model_identity(&self, config: &Config) -> String {
        format!("{}/{}", config.provider, config.model)
    }
}
