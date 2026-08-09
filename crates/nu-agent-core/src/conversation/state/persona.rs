use super::super::managers::PersonaManager;
use crate::config::Config;
use crate::tools::authz::PermissionsOverlay;

pub struct PersonaState {
    agent_persona_body: Option<String>,
    agent_identity: Option<String>,
    agent_description: Option<String>,
    agent_icon: Option<String>,
    cached_agents_chain: Option<String>,
    cached_available_skills: Option<String>,
    cached_sub_agent_instruction: Option<String>,
}

/// Result of a successful agent switch, carrying side-effects the caller must apply.
pub struct SwitchAgentResult {
    pub identity: String,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub max_tool_turns: Option<u32>,
    pub max_tool_calls_per_subturn: Option<usize>,
    pub max_tool_result_bytes: Option<usize>,
    pub additional_params: Option<serde_json::Value>,
    pub permissions_overlay: Option<PermissionsOverlay>,
}

impl PersonaState {
    pub fn new(
        agent_persona_body: Option<String>,
        agent_identity: Option<String>,
        agent_description: Option<String>,
        agent_icon: Option<String>,
        cached_agents_chain: Option<String>,
        cached_available_skills: Option<String>,
        cached_sub_agent_instruction: Option<String>,
    ) -> Self {
        Self {
            agent_persona_body,
            agent_identity,
            agent_description,
            agent_icon,
            cached_agents_chain,
            cached_available_skills,
            cached_sub_agent_instruction,
        }
    }

    pub fn agent_persona_body(&self) -> Option<&str> {
        self.agent_persona_body.as_deref()
    }
    pub fn agent_identity(&self) -> Option<&str> {
        self.agent_identity.as_deref()
    }
    pub fn agent_description(&self) -> Option<&str> {
        self.agent_description.as_deref()
    }
    pub fn agent_icon(&self) -> Option<&str> {
        self.agent_icon.as_deref()
    }
    pub fn cached_agents_chain(&self) -> Option<&str> {
        self.cached_agents_chain.as_deref()
    }
    pub fn cached_available_skills(&self) -> Option<&str> {
        self.cached_available_skills.as_deref()
    }
    pub fn cached_sub_agent_instruction(&self) -> Option<&str> {
        self.cached_sub_agent_instruction.as_deref()
    }
    pub fn persona_body_len(&self) -> Option<usize> {
        self.agent_persona_body.as_ref().map(|b| b.len())
    }

    pub fn switch_agent(
        &mut self,
        agent_name: &str,
        cwd: &std::path::Path,
        agents_config: &crate::config::AgentsConfig,
    ) -> Result<SwitchAgentResult, String> {
        use crate::protocol::persona::{
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
        self.agent_icon = parsed.icon;

        // Convert frontmatter permissions to PermissionsOverlay
        let permissions_overlay = match parsed.permissions.as_ref() {
            Some(mapping) => match PermissionsOverlay::parse_from_yaml(mapping) {
                Ok(overlay) => Some(overlay),
                Err(e) => {
                    return Err(format!("agent switch failed: invalid permissions: {e}"));
                }
            },
            None => None,
        };

        log::debug!(
            "switch_agent: switched to identity={identity:?}, model={:?}, body_len={}, has_permissions={}",
            parsed.model,
            self.agent_persona_body.as_ref().map_or(0, |b| b.len()),
            permissions_overlay.is_some()
        );

        Ok(SwitchAgentResult {
            identity,
            model: parsed.model,
            temperature: parsed.temperature,
            max_tokens: parsed.max_tokens,
            max_tool_turns: parsed.max_tool_turns,
            max_tool_calls_per_subturn: parsed.max_tool_calls_per_subturn,
            max_tool_result_bytes: parsed.max_tool_result_bytes,
            additional_params: parsed.additional_params,
            permissions_overlay,
        })
    }

    pub fn active_model_identity(&self, config: &Config) -> String {
        format!("{}/{}", config.provider, config.model)
    }
}

impl PersonaManager for PersonaState {
    fn agent_persona_body(&self) -> Option<&str> {
        self.agent_persona_body.as_deref()
    }

    fn cached_sub_agent_instruction(&self) -> Option<&str> {
        self.cached_sub_agent_instruction.as_deref()
    }

    fn cached_agents_chain(&self) -> Option<&str> {
        self.cached_agents_chain.as_deref()
    }

    fn cached_available_skills(&self) -> Option<&str> {
        self.cached_available_skills.as_deref()
    }

    fn agent_identity(&self) -> Option<&str> {
        self.agent_identity.as_deref()
    }

    fn persona_body_len(&self) -> Option<usize> {
        self.agent_persona_body.as_ref().map(|b| b.len())
    }

    fn agent_description(&self) -> Option<&str> {
        self.agent_description.as_deref()
    }

    fn agent_icon(&self) -> Option<&str> {
        self.agent_icon.as_deref()
    }

    fn active_model_identity(&self, config: &Config) -> String {
        self.active_model_identity(config)
    }

    fn switch_agent(
        &mut self,
        agent_name: &str,
        cwd: &std::path::Path,
        agents_config: &crate::config::AgentsConfig,
    ) -> Result<SwitchAgentResult, String> {
        self.switch_agent(agent_name, cwd, agents_config)
    }
}
