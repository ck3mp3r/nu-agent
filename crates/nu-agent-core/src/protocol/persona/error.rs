use std::fmt;
use std::path::PathBuf;

/// Error type for persona file resolution
#[derive(Debug)]
pub enum PersonaError {
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
            PersonaError::NotFound {
                cwd_path,
                config_path,
            } => {
                write!(
                    f,
                    "Persona file not found. Checked:\n  - {}\n  - {}",
                    cwd_path.display(),
                    config_path.display()
                )
            }
            PersonaError::ReadFailed { path, source } => {
                write!(
                    f,
                    "Failed to read persona file at {}: {}",
                    path.display(),
                    source
                )
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
pub enum FrontMatterError {
    YamlParseFailed {
        source: noyalib::Error,
    },
    InvalidField {
        key: String,
        expected: String,
        got: String,
    },
}

impl fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrontMatterError::YamlParseFailed { source } => {
                write!(f, "Failed to parse YAML front matter: {source}")
            }
            FrontMatterError::InvalidField { key, expected, got } => {
                write!(f, "Invalid field '{key}': expected {expected}, got {got}")
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
