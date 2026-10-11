use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag, TagEnd};

use serde_json::Value as JsonValue;

use super::error::FrontMatterError;

/// Raw parsed persona with optional front matter and body content (parser output)
pub struct RawParsedPersona {
    pub front_matter: Option<noyalib::Mapping>,
    pub body: String,
}

/// Parsed persona with typed front matter fields and body content
pub struct ParsedPersona {
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub permissions: Option<noyalib::Mapping>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub max_tool_turns: Option<u32>,
    pub max_tool_calls_per_subturn: Option<usize>,
    pub max_tool_result_bytes: Option<usize>,
    pub additional_params: Option<JsonValue>,
    pub icon: Option<String>,
    pub body: String,
}

/// Interprets front matter into typed ParsedPersona struct
pub fn interpret_front_matter(
    front_matter: Option<&noyalib::Mapping>,
    body: String,
) -> Result<ParsedPersona, FrontMatterError> {
    log::debug!(
        "interpret_front_matter: has_front_matter={}",
        front_matter.is_some()
    );
    let Some(mapping) = front_matter else {
        return Ok(ParsedPersona {
            name: None,
            description: None,
            model: None,
            permissions: None,
            temperature: None,
            max_tokens: None,
            max_tool_turns: None,
            max_tool_calls_per_subturn: None,
            max_tool_result_bytes: None,
            additional_params: None,
            icon: None,
            body,
        });
    };

    let mut name = None;
    let mut description = None;
    let mut model = None;
    let mut permissions = None;
    let mut temperature = None;
    let mut max_tokens = None;
    let mut max_tool_turns = None;
    let mut max_tool_calls_per_subturn = None;
    let mut max_tool_result_bytes = None;
    let mut additional_params = None;
    let mut icon = None;

    for (key, value) in mapping.iter() {
        match key.as_str() {
            "name" => {
                name = Some(
                    value
                        .as_str()
                        .ok_or_else(|| FrontMatterError::InvalidField {
                            key: "name".to_string(),
                            expected: "string".to_string(),
                            got: value_type_name(value),
                        })?
                        .to_string(),
                );
                log::trace!("interpret_front_matter: name={name:?}");
            }
            "description" => {
                description = Some(
                    value
                        .as_str()
                        .ok_or_else(|| FrontMatterError::InvalidField {
                            key: "description".to_string(),
                            expected: "string".to_string(),
                            got: value_type_name(value),
                        })?
                        .to_string(),
                );
                log::trace!("interpret_front_matter: description={description:?}");
            }
            "model" => {
                model = Some(
                    value
                        .as_str()
                        .ok_or_else(|| FrontMatterError::InvalidField {
                            key: "model".to_string(),
                            expected: "string".to_string(),
                            got: value_type_name(value),
                        })?
                        .to_string(),
                );
                log::trace!("interpret_front_matter: model={model:?}");
            }
            "icon" => {
                icon = Some(
                    value
                        .as_str()
                        .ok_or_else(|| FrontMatterError::InvalidField {
                            key: "icon".to_string(),
                            expected: "string".to_string(),
                            got: value_type_name(value),
                        })?
                        .to_string(),
                );
                log::trace!("interpret_front_matter: icon={icon:?}");
            }
            "permissions" => {
                permissions = Some(
                    value
                        .as_mapping()
                        .ok_or_else(|| FrontMatterError::InvalidField {
                            key: "permissions".to_string(),
                            expected: "mapping".to_string(),
                            got: value_type_name(value),
                        })?
                        .clone(),
                );
                log::trace!("interpret_front_matter: has_permissions=true");
            }
            "temperature" => {
                temperature = Some(match value {
                    noyalib::Value::Number(n) => Ok(n.as_f64()),
                    _ => Err(FrontMatterError::InvalidField {
                        key: "temperature".to_string(),
                        expected: "number".to_string(),
                        got: value_type_name(value),
                    }),
                }?);
                log::trace!("interpret_front_matter: temperature={temperature:?}");
            }
            "max_tokens" => {
                let n = match value {
                    noyalib::Value::Number(n) => {
                        n.as_u64().ok_or_else(|| FrontMatterError::InvalidField {
                            key: "max_tokens".to_string(),
                            expected: "unsigned integer".to_string(),
                            got: value_type_name(value),
                        })
                    }
                    _ => Err(FrontMatterError::InvalidField {
                        key: "max_tokens".to_string(),
                        expected: "unsigned integer".to_string(),
                        got: value_type_name(value),
                    }),
                }?;
                max_tokens = Some(n as u32);
                log::trace!("interpret_front_matter: max_tokens={max_tokens:?}");
            }
            "max_tool_turns" => {
                let n = match value {
                    noyalib::Value::Number(n) => {
                        n.as_u64().ok_or_else(|| FrontMatterError::InvalidField {
                            key: "max_tool_turns".to_string(),
                            expected: "unsigned integer".to_string(),
                            got: value_type_name(value),
                        })
                    }
                    _ => Err(FrontMatterError::InvalidField {
                        key: "max_tool_turns".to_string(),
                        expected: "unsigned integer".to_string(),
                        got: value_type_name(value),
                    }),
                }?;
                max_tool_turns = Some(n as u32);
                log::trace!("interpret_front_matter: max_tool_turns={max_tool_turns:?}");
            }
            "max_tool_calls_per_subturn" => {
                let n = match value {
                    noyalib::Value::Number(n) => {
                        n.as_u64().ok_or_else(|| FrontMatterError::InvalidField {
                            key: "max_tool_calls_per_subturn".to_string(),
                            expected: "unsigned integer".to_string(),
                            got: value_type_name(value),
                        })
                    }
                    _ => Err(FrontMatterError::InvalidField {
                        key: "max_tool_calls_per_subturn".to_string(),
                        expected: "unsigned integer".to_string(),
                        got: value_type_name(value),
                    }),
                }?;
                max_tool_calls_per_subturn = Some(n as usize);
                log::trace!(
                    "interpret_front_matter: max_tool_calls_per_subturn={max_tool_calls_per_subturn:?}"
                );
            }
            "max_tool_result_bytes" => {
                let n = match value {
                    noyalib::Value::Number(n) => {
                        n.as_u64().ok_or_else(|| FrontMatterError::InvalidField {
                            key: "max_tool_result_bytes".to_string(),
                            expected: "unsigned integer".to_string(),
                            got: value_type_name(value),
                        })
                    }
                    _ => Err(FrontMatterError::InvalidField {
                        key: "max_tool_result_bytes".to_string(),
                        expected: "unsigned integer".to_string(),
                        got: value_type_name(value),
                    }),
                }?;
                max_tool_result_bytes = Some(n as usize);
                log::trace!(
                    "interpret_front_matter: max_tool_result_bytes={max_tool_result_bytes:?}"
                );
            }
            "additional_params" => {
                let m = value
                    .as_mapping()
                    .ok_or_else(|| FrontMatterError::InvalidField {
                        key: "additional_params".to_string(),
                        expected: "mapping".to_string(),
                        got: value_type_name(value),
                    })?;
                additional_params =
                    Some(
                        serde_json::to_value(m).map_err(|e| FrontMatterError::InvalidField {
                            key: "additional_params".to_string(),
                            expected: "JSON-serialisable mapping".to_string(),
                            got: e.to_string(),
                        })?,
                    );
                log::trace!("interpret_front_matter: has_additional_params=true");
            }
            _ => {
                // Unknown keys are silently ignored
                log::trace!("interpret_front_matter: ignoring unknown key={key:?}");
            }
        }
    }

    Ok(ParsedPersona {
        name,
        description,
        model,
        permissions,
        temperature,
        max_tokens,
        max_tool_turns,
        max_tool_calls_per_subturn,
        max_tool_result_bytes,
        additional_params,
        icon,
        body,
    })
}

/// Helper to get type name for YAML value
fn value_type_name(value: &noyalib::Value) -> String {
    match value {
        noyalib::Value::Null => "null".to_string(),
        noyalib::Value::Bool(_) => "boolean".to_string(),
        noyalib::Value::Number(_) => "number".to_string(),
        noyalib::Value::String(_) => "string".to_string(),
        noyalib::Value::Sequence(_) => "sequence".to_string(),
        noyalib::Value::Mapping(_) => "mapping".to_string(),
        noyalib::Value::Tagged(_) => "tagged".to_string(),
    }
}

/// Trait for parsing front matter from markdown content
pub trait FrontMatterParser {
    fn parse(&self, input: &str) -> Result<RawParsedPersona, FrontMatterError>;
}

/// Pulldown-cmark based front matter parser
pub struct PulldownCmarkFrontMatterParser;

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
                Event::Text(text) if in_metadata => {
                    yaml_text.push_str(&text);
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
            let trimmed = yaml_text.trim();
            if trimmed.is_empty()
                || trimmed
                    .lines()
                    .all(|l| l.trim_start().starts_with('#') || l.trim().is_empty())
            {
                Some(noyalib::Mapping::new())
            } else {
                match noyalib::from_str::<noyalib::Mapping>(&yaml_text) {
                    Ok(mapping) => Some(mapping),
                    Err(e) => return Err(FrontMatterError::YamlParseFailed { source: e }),
                }
            }
        } else {
            None
        };

        log::debug!(
            "persona parse: has_front_matter={}, metadata_end_offset={metadata_end_offset:?}",
            front_matter.is_some()
        );

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
