use std::path::{Path, PathBuf};

use super::builtins;
use super::error::PersonaError;
use super::parser::{FrontMatterParser, PulldownCmarkFrontMatterParser, interpret_front_matter};

/// Summary of a discovered agent persona (name + description).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaSummary {
    pub name: String,
    pub description: Option<String>,
    pub builtin: bool,
}

/// Trait for resolving persona files
pub trait PersonaFileResolver {
    /// Resolves a persona file by name, returning the path and contents
    fn resolve(&self, persona_name: &str) -> Result<(PathBuf, String), PersonaError>;
}

/// Trait for listing all available persona files
pub trait PersonaLister {
    /// Lists all available personas, deduplicated (cwd takes precedence over XDG).
    fn list_available(&self) -> Vec<PersonaSummary>;
}

/// Filesystem-based persona resolver
pub struct FsPersonaResolver {
    cwd: PathBuf,
    config_dir: PathBuf,
    agents_config: crate::config::AgentsConfig,
}

impl FsPersonaResolver {
    pub fn new(
        cwd: PathBuf,
        config_dir: PathBuf,
        agents_config: crate::config::AgentsConfig,
    ) -> Self {
        Self {
            cwd,
            config_dir,
            agents_config,
        }
    }

    fn try_read_persona(
        &self,
        base_dir: &Path,
        persona_name: &str,
    ) -> Option<Result<(PathBuf, String), PersonaError>> {
        let path = base_dir.join(".agents").join(format!("{persona_name}.md"));
        log::trace!("try_read_persona: checking path={path:?}");

        if !path.exists() {
            log::trace!("try_read_persona: path does not exist");
            return None;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => Some(Ok((path, content))),
            Err(e) => Some(Err(PersonaError::ReadFailed { path, source: e })),
        }
    }

    fn try_read_persona_xdg(
        &self,
        persona_name: &str,
    ) -> Option<Result<(PathBuf, String), PersonaError>> {
        let path = self
            .config_dir
            .join("agents")
            .join(format!("{persona_name}.md"));
        log::trace!("try_read_persona_xdg: checking path={path:?}");

        if !path.exists() {
            log::trace!("try_read_persona_xdg: path does not exist");
            return None;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => Some(Ok((path, content))),
            Err(e) => Some(Err(PersonaError::ReadFailed { path, source: e })),
        }
    }

    fn resolve_builtin(&self, name: &str) -> Option<&'static str> {
        match name {
            n if n == builtins::BUILTIN_PLANNER_NAME && self.agents_config.planner_enabled => {
                Some(builtins::BUILTIN_PLANNER_CONTENT)
            }
            n if n == builtins::BUILTIN_MAKER_NAME && self.agents_config.maker_enabled => {
                Some(builtins::BUILTIN_MAKER_CONTENT)
            }
            _ => None,
        }
    }
}

impl PersonaLister for FsPersonaResolver {
    fn list_available(&self) -> Vec<PersonaSummary> {
        let parser = PulldownCmarkFrontMatterParser;
        let mut results = Vec::new();

        // Add enabled built-ins first
        for builtin in builtins::BUILTIN_PERSONAS {
            let enabled = match builtin.name {
                n if n == builtins::BUILTIN_PLANNER_NAME => self.agents_config.planner_enabled,
                n if n == builtins::BUILTIN_MAKER_NAME => self.agents_config.maker_enabled,
                _ => false,
            };
            if enabled {
                let description = parser
                    .parse(builtin.content)
                    .ok()
                    .and_then(|raw| {
                        interpret_front_matter(raw.front_matter.as_ref(), raw.body).ok()
                    })
                    .and_then(|p| p.description);
                results.push(PersonaSummary {
                    name: builtin.name.to_string(),
                    description,
                    builtin: true,
                });
            }
        }

        // Then scan filesystem (XDG first so cwd can override)
        let mut seen = std::collections::HashMap::<String, PersonaSummary>::new();
        let xdg_dir = self.config_dir.join("agents");
        Self::scan_dir(&xdg_dir, &parser, &mut seen);

        let cwd_dir = self.cwd.join(".agents");
        Self::scan_dir(&cwd_dir, &parser, &mut seen);

        // Deduplicate: filter out filesystem personas that share a name with an enabled built-in
        let builtin_names: std::collections::HashSet<&str> =
            results.iter().map(|s| s.name.as_str()).collect();

        let mut fs_results: Vec<PersonaSummary> = seen
            .into_values()
            .filter(|s| !builtin_names.contains(s.name.as_str()))
            .collect();
        fs_results.sort_by(|a, b| a.name.cmp(&b.name));
        results.extend(fs_results);
        results
    }
}

impl FsPersonaResolver {
    fn scan_dir(
        dir: &Path,
        parser: &PulldownCmarkFrontMatterParser,
        seen: &mut std::collections::HashMap<String, PersonaSummary>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let contents = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let raw = match parser.parse(&contents) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let parsed = match interpret_front_matter(raw.front_matter.as_ref(), raw.body) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let name = parsed.name.unwrap_or_else(|| stem.clone());
            let description = parsed.description;
            seen.insert(
                stem,
                PersonaSummary {
                    name,
                    description,
                    builtin: false,
                },
            );
        }
    }
}

impl PersonaFileResolver for FsPersonaResolver {
    fn resolve(&self, persona_name: &str) -> Result<(PathBuf, String), PersonaError> {
        log::debug!(
            "persona resolve: name={persona_name:?}, cwd={:?}, config_dir={:?}",
            self.cwd,
            self.config_dir
        );

        // Check built-ins first
        if let Some(content) = self.resolve_builtin(persona_name) {
            log::debug!("persona resolve: found as built-in");
            return Ok((
                PathBuf::from(format!("<builtin>/{persona_name}.md")),
                content.to_string(),
            ));
        }

        // Try cwd first
        if let Some(result) = self.try_read_persona(&self.cwd, persona_name) {
            log::debug!("persona resolve: found in cwd .agents/ dir");
            return result;
        }

        // Try XDG config dir second
        if let Some(result) = self.try_read_persona_xdg(persona_name) {
            log::debug!("persona resolve: found in XDG config dir");
            return result;
        }

        // Neither found
        log::debug!("persona resolve: not found for name={persona_name:?}");
        Err(PersonaError::NotFound {
            cwd_path: self.cwd.join(".agents").join(format!("{persona_name}.md")),
            config_path: self
                .config_dir
                .join("agents")
                .join(format!("{persona_name}.md")),
        })
    }
}
