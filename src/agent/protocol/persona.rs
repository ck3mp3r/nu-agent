use std::fmt;
use std::path::{Path, PathBuf};
use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag, TagEnd};

/// Error type for persona file resolution
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum PersonaError {
    /// Persona file not found in either cwd or config directory
    NotFound {
        cwd_path: PathBuf,
        config_path: PathBuf,
    },
    /// Failed to read persona file
    ReadFailed {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for PersonaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersonaError::NotFound { cwd_path, config_path } => {
                write!(
                    f,
                    "Persona file not found. Checked:\n  - {}\n  - {}",
                    cwd_path.display(),
                    config_path.display()
                )
            }
            PersonaError::ReadFailed { path, source } => {
                write!(f, "Failed to read persona file at {}: {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for PersonaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersonaError::NotFound { .. } => None,
            PersonaError::ReadFailed { source, .. } => Some(source),
        }
    }
}

/// Error type for front matter parsing
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FrontMatterError {
    YamlParseFailed { source: serde_yml::Error },
    InvalidField { key: String, expected: String, got: String },
}

impl fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrontMatterError::YamlParseFailed { source } => {
                write!(f, "Failed to parse YAML front matter: {}", source)
            }
            FrontMatterError::InvalidField { key, expected, got } => {
                write!(f, "Invalid field '{}': expected {}, got {}", key, expected, got)
            }
        }
    }
}

impl std::error::Error for FrontMatterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrontMatterError::YamlParseFailed { source } => Some(source),
            FrontMatterError::InvalidField { .. } => None,
        }
    }
}

/// Raw parsed persona with optional front matter and body content (parser output)
#[allow(dead_code)]
pub(crate) struct RawParsedPersona {
    pub front_matter: Option<serde_yml::Mapping>,
    pub body: String,
}

/// Parsed persona with typed front matter fields and body content
#[allow(dead_code)]
pub(crate) struct ParsedPersona {
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub tool_filter: Option<Vec<String>>,
    pub permissions: Option<serde_yml::Mapping>,
    pub body: String,
}

/// Interprets front matter into typed ParsedPersona struct
#[allow(dead_code)]
pub(crate) fn interpret_front_matter(
    front_matter: Option<&serde_yml::Mapping>,
    body: String,
) -> Result<ParsedPersona, FrontMatterError> {
    log::debug!("interpret_front_matter: has_front_matter={}", front_matter.is_some());
    let Some(mapping) = front_matter else {
        return Ok(ParsedPersona {
            name: None,
            description: None,
            model: None,
            tool_filter: None,
            permissions: None,
            body,
        });
    };

    let mut name = None;
    let mut description = None;
    let mut model = None;
    let mut tool_filter = None;
    let mut permissions = None;

    for (key, value) in mapping.iter() {
        let key_str = match key.as_str() {
            Some(s) => s,
            None => continue, // Non-string keys are ignored
        };

        match key_str {
            "name" => {
                name = Some(value.as_str().ok_or_else(|| FrontMatterError::InvalidField {
                    key: "name".to_string(),
                    expected: "string".to_string(),
                    got: value_type_name(value),
                })?.to_string());
                log::trace!("interpret_front_matter: name={name:?}");
            }
            "description" => {
                description = Some(value.as_str().ok_or_else(|| FrontMatterError::InvalidField {
                    key: "description".to_string(),
                    expected: "string".to_string(),
                    got: value_type_name(value),
                })?.to_string());
                log::trace!("interpret_front_matter: description={description:?}");
            }
            "model" => {
                model = Some(value.as_str().ok_or_else(|| FrontMatterError::InvalidField {
                    key: "model".to_string(),
                    expected: "string".to_string(),
                    got: value_type_name(value),
                })?.to_string());
                log::trace!("interpret_front_matter: model={model:?}");
            }
            "tool_filter" => {
                let seq = value.as_sequence().ok_or_else(|| FrontMatterError::InvalidField {
                    key: "tool_filter".to_string(),
                    expected: "sequence".to_string(),
                    got: value_type_name(value),
                })?;

                let mut tools = Vec::new();
                for item in seq {
                    let tool = item.as_str().ok_or_else(|| FrontMatterError::InvalidField {
                        key: "tool_filter".to_string(),
                        expected: "sequence of strings".to_string(),
                        got: "sequence with non-string element".to_string(),
                    })?;
                    tools.push(tool.to_string());
                }
                tool_filter = Some(tools);
                log::trace!("interpret_front_matter: tool_filter={tool_filter:?}");
            }
            "permissions" => {
                permissions = Some(value.as_mapping().ok_or_else(|| FrontMatterError::InvalidField {
                    key: "permissions".to_string(),
                    expected: "mapping".to_string(),
                    got: value_type_name(value),
                })?.clone());
                log::trace!("interpret_front_matter: has_permissions=true");
            }
            _ => {
                // Unknown keys are silently ignored
                log::trace!("interpret_front_matter: ignoring unknown key={key_str:?}");
            }
        }
    }

    Ok(ParsedPersona {
        name,
        description,
        model,
        tool_filter,
        permissions,
        body,
    })
}

/// Helper to get type name for YAML value
fn value_type_name(value: &serde_yml::Value) -> String {
    match value {
        serde_yml::Value::Null => "null".to_string(),
        serde_yml::Value::Bool(_) => "boolean".to_string(),
        serde_yml::Value::Number(_) => "number".to_string(),
        serde_yml::Value::String(_) => "string".to_string(),
        serde_yml::Value::Sequence(_) => "sequence".to_string(),
        serde_yml::Value::Mapping(_) => "mapping".to_string(),
        serde_yml::Value::Tagged(_) => "tagged".to_string(),
    }
}

/// Trait for parsing front matter from markdown content
#[allow(dead_code)]
pub(crate) trait FrontMatterParser {
    fn parse(&self, input: &str) -> Result<RawParsedPersona, FrontMatterError>;
}

/// Trait for resolving persona files
#[allow(dead_code)]
pub(crate) trait PersonaFileResolver {
    /// Resolves a persona file by name, returning the path and contents
    fn resolve(&self, persona_name: &str) -> Result<(PathBuf, String), PersonaError>;
}

/// Filesystem-based persona resolver
#[allow(dead_code)]
pub(crate) struct FsPersonaResolver {
    cwd: PathBuf,
    config_dir: PathBuf,
}

#[allow(dead_code)]
impl FsPersonaResolver {
    pub fn new(cwd: PathBuf, config_dir: PathBuf) -> Self {
        Self { cwd, config_dir }
    }

    fn try_read_persona(&self, base_dir: &Path, persona_name: &str) -> Option<Result<(PathBuf, String), PersonaError>> {
        let path = base_dir.join(".agents").join(format!("{}.md", persona_name));
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

    fn try_read_persona_xdg(&self, persona_name: &str) -> Option<Result<(PathBuf, String), PersonaError>> {
        let path = self.config_dir.join("agents").join(format!("{}.md", persona_name));
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
}

impl PersonaFileResolver for FsPersonaResolver {
    fn resolve(&self, persona_name: &str) -> Result<(PathBuf, String), PersonaError> {
        log::debug!("persona resolve: name={persona_name:?}, cwd={:?}, config_dir={:?}", self.cwd, self.config_dir);
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
            cwd_path: self.cwd.join(".agents").join(format!("{}.md", persona_name)),
            config_path: self.config_dir.join("agents").join(format!("{}.md", persona_name)),
        })
    }
}

/// Pulldown-cmark based front matter parser
#[allow(dead_code)]
pub(crate) struct PulldownCmarkFrontMatterParser;

impl FrontMatterParser for PulldownCmarkFrontMatterParser {
    fn parse(&self, input: &str) -> Result<RawParsedPersona, FrontMatterError> {
        log::trace!("persona parse: input_len={}", input.len());
        let mut options = Options::empty();
        options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        
        let parser = Parser::new_ext(input, options);
        
        let mut yaml_text = String::new();
        let mut in_metadata = false;
        let mut metadata_end_offset: Option<usize> = None;
        
        for (event, range) in parser.into_offset_iter() {
            match event {
                Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)) => {
                    in_metadata = true;
                }
                Event::Text(ref text) if in_metadata => {
                    yaml_text.push_str(text);
                }
                Event::End(TagEnd::MetadataBlock(MetadataBlockKind::YamlStyle)) => {
                    in_metadata = false;
                    metadata_end_offset = Some(range.end);
                }
                _ => {}
            }
        }
        
        // Parse front matter if we found a metadata block
        let front_matter = if metadata_end_offset.is_some() {
            // Empty YAML should parse to an empty mapping
            match serde_yml::from_str::<serde_yml::Mapping>(&yaml_text) {
                Ok(mapping) => Some(mapping),
                Err(e) => return Err(FrontMatterError::YamlParseFailed { source: e }),
            }
        } else {
            None
        };
        
        log::debug!("persona parse: has_front_matter={}, metadata_end_offset={metadata_end_offset:?}", front_matter.is_some());
        
        // Extract body
        let body = if let Some(offset) = metadata_end_offset {
            // Find the actual end of the closing --- delimiter
            let remaining = &input[offset..];
            // Skip any trailing whitespace/newlines after the metadata block
            remaining.trim_start().to_string()
        } else {
            // No front matter, return entire input
            input.to_string()
        };
        
        Ok(RawParsedPersona { front_matter, body })
    }
}

/// Mock persona resolver for testing
#[cfg(test)]
pub(crate) struct MockPersonaResolver {
    pub result: Result<(PathBuf, String), PersonaError>,
}

#[cfg(test)]
impl PersonaFileResolver for MockPersonaResolver {
    fn resolve(&self, _persona_name: &str) -> Result<(PathBuf, String), PersonaError> {
        match &self.result {
            Ok((path, content)) => Ok((path.clone(), content.clone())),
            Err(PersonaError::NotFound { cwd_path, config_path }) => {
                Err(PersonaError::NotFound {
                    cwd_path: cwd_path.clone(),
                    config_path: config_path.clone(),
                })
            }
            Err(PersonaError::ReadFailed { path, source }) => {
                Err(PersonaError::ReadFailed {
                    path: path.clone(),
                    source: std::io::Error::new(source.kind(), source.to_string()),
                })
            }
        }
    }
}
