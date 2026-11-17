use std::fmt;
use std::path::PathBuf;

/// Error type for persona file resolution
#[derive(Debug)]
pub enum PersonaError {
    /// Persona file not found in either the cwd `.agents` dir or the resolved
    /// agents dir
    NotFound {
        cwd_path: PathBuf,
        agents_path: PathBuf,
    },
    /// Failed to read persona file
    ReadFailed {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Persona name is not a valid single path component
    InvalidName { name: String },
}

impl fmt::Display for PersonaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersonaError::NotFound {
                cwd_path,
                agents_path,
            } => {
                write!(
                    f,
                    "Persona file not found. Checked:\n  - {}\n  - {}",
                    cwd_path.display(),
                    agents_path.display()
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
            PersonaError::InvalidName { name } => {
                write!(f, "Invalid persona name: '{name}'")
            }
        }
    }
}

impl std::error::Error for PersonaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersonaError::NotFound { .. } => None,
            PersonaError::ReadFailed { source, .. } => Some(source),
            PersonaError::InvalidName { .. } => None,
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

/// Which leg of the shared persona load failed: the file read, the front-matter
/// parse, or the front-matter interpretation.
///
/// The variant carries the typed error and no message, so each caller composes
/// its own prefix over the leg it names.
#[derive(Debug)]
pub(crate) enum PersonaLoadError {
    Load(PersonaError),
    Parse(FrontMatterError),
    Interpret(FrontMatterError),
}

impl fmt::Display for PersonaLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersonaLoadError::Load(error) => write!(f, "{error}"),
            PersonaLoadError::Parse(error) | PersonaLoadError::Interpret(error) => {
                write!(f, "{error}")
            }
        }
    }
}

impl std::error::Error for PersonaLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersonaLoadError::Load(error) => Some(error),
            PersonaLoadError::Parse(error) | PersonaLoadError::Interpret(error) => Some(error),
        }
    }
}
