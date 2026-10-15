use super::{FrontMatterError, PersonaError};
use std::error::Error;
use std::path::PathBuf;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn not_found() -> PersonaError {
    PersonaError::NotFound {
        cwd_path: PathBuf::from("/cwd/persona.md"),
        config_path: PathBuf::from("/config/persona.md"),
    }
}

fn read_failed() -> PersonaError {
    PersonaError::ReadFailed {
        path: PathBuf::from("/cwd/persona.md"),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    }
}

// endregion: --- Test Support

#[test]
fn test_protocol_persona_error_persona_error_display() -> Result<()> {
    // -- Setup & Fixtures
    let not_found = not_found();
    let read_failed = read_failed();

    // -- Exec
    let not_found_msg = not_found.to_string();
    let read_failed_msg = read_failed.to_string();

    // -- Check
    assert!(not_found_msg.contains("Persona file not found"));
    assert!(not_found_msg.contains("/cwd/persona.md"));
    assert!(not_found_msg.contains("/config/persona.md"));
    assert!(read_failed_msg.contains("Failed to read persona file at /cwd/persona.md"));
    assert!(read_failed_msg.contains("denied"));
    Ok(())
}

#[test]
fn test_protocol_persona_error_persona_error_source() -> Result<()> {
    // -- Setup & Fixtures
    let not_found = not_found();
    let read_failed = read_failed();

    // -- Exec & Check
    assert!(not_found.source().is_none());
    let source = read_failed
        .source()
        .ok_or("ReadFailed must expose its source")?;
    assert_eq!(source.to_string(), "denied");
    Ok(())
}

#[test]
fn test_protocol_persona_error_front_matter_error_display() -> Result<()> {
    // -- Setup & Fixtures
    let yaml_failed = FrontMatterError::YamlParseFailed {
        source: noyalib::Error::Parse("bad yaml".to_string()),
    };
    let invalid_field = FrontMatterError::InvalidField {
        key: "model".to_string(),
        expected: "string".to_string(),
        got: "number".to_string(),
    };

    // -- Exec
    let yaml_msg = yaml_failed.to_string();
    let field_msg = invalid_field.to_string();

    // -- Check
    assert!(yaml_msg.contains("Failed to parse YAML front matter"));
    assert!(yaml_msg.contains("bad yaml"));
    assert_eq!(
        field_msg,
        "Invalid field 'model': expected string, got number"
    );
    Ok(())
}

#[test]
fn test_protocol_persona_error_front_matter_error_source() -> Result<()> {
    // -- Setup & Fixtures
    let yaml_failed = FrontMatterError::YamlParseFailed {
        source: noyalib::Error::Parse("bad yaml".to_string()),
    };
    let invalid_field = FrontMatterError::InvalidField {
        key: "model".to_string(),
        expected: "string".to_string(),
        got: "number".to_string(),
    };

    // -- Exec & Check
    assert!(yaml_failed.source().is_some());
    assert!(invalid_field.source().is_none());
    Ok(())
}
