use super::{BUILTIN_MAKER_CONTENT, BUILTIN_PERSONAS, BUILTIN_PLANNER_CONTENT, is_builtin_persona};
use crate::protocol::persona::{
    FrontMatterParser, PulldownCmarkFrontMatterParser, interpret_front_matter,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_builtin_planner_content_not_empty() {
    assert!(
        !BUILTIN_PLANNER_CONTENT.is_empty(),
        "planner content must not be empty"
    );
}

#[test]
fn test_builtin_maker_content_not_empty() {
    assert!(
        !BUILTIN_MAKER_CONTENT.is_empty(),
        "maker content must not be empty"
    );
}

#[test]
fn test_is_builtin_persona_planner() {
    assert!(is_builtin_persona("planner"));
}

#[test]
fn test_is_builtin_persona_maker() {
    assert!(is_builtin_persona("maker"));
}

#[test]
fn test_is_builtin_persona_unknown() {
    assert!(!is_builtin_persona("custom"));
}

#[test]
fn test_builtin_content_has_valid_front_matter() -> Result<()> {
    let parser = PulldownCmarkFrontMatterParser;

    // Planner
    let planner_raw = parser
        .parse(BUILTIN_PLANNER_CONTENT)
        .map_err(|e| format!("planner content should parse: {e:?}"))?;
    let planner_parsed =
        interpret_front_matter(planner_raw.front_matter.as_ref(), planner_raw.body)
            .map_err(|e| format!("planner front matter should be interpretable: {e:?}"))?;
    assert!(
        planner_parsed.name.is_some(),
        "planner front matter must contain a name"
    );
    assert!(
        planner_parsed.description.is_some(),
        "planner front matter must contain a description"
    );

    // Maker
    let maker_raw = parser
        .parse(BUILTIN_MAKER_CONTENT)
        .map_err(|e| format!("maker content should parse: {e:?}"))?;
    let maker_parsed = interpret_front_matter(maker_raw.front_matter.as_ref(), maker_raw.body)
        .map_err(|e| format!("maker front matter should be interpretable: {e:?}"))?;
    assert!(
        maker_parsed.name.is_some(),
        "maker front matter must contain a name"
    );
    assert!(
        maker_parsed.description.is_some(),
        "maker front matter must contain a description"
    );
    Ok(())
}

#[test]
fn builtin_personas_have_icon() -> Result<()> {
    let parser = PulldownCmarkFrontMatterParser;
    for builtin in BUILTIN_PERSONAS {
        let raw = parser
            .parse(builtin.content)
            .map_err(|e| format!("builtin must parse: {e:?}"))?;
        let persona = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
            .map_err(|e| format!("builtin front matter must be valid: {e:?}"))?;
        assert!(
            persona.icon.is_some(),
            "builtin '{}' must have an icon",
            builtin.name
        );
    }
    Ok(())
}
