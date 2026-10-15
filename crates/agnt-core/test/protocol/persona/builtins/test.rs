use super::{BUILTIN_MAKER_CONTENT, BUILTIN_PERSONAS, BUILTIN_PLANNER_CONTENT, is_builtin_persona};
use crate::protocol::persona::{
    FrontMatterParser, PulldownCmarkFrontMatterParser, interpret_front_matter,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn builtin_persona_registry_entries() {
    // Builtin contents are non-empty.
    assert!(
        !BUILTIN_PLANNER_CONTENT.is_empty(),
        "planner content must not be empty"
    );
    assert!(
        !BUILTIN_MAKER_CONTENT.is_empty(),
        "maker content must not be empty"
    );

    // Known builtin names are recognized; unknown names are not.
    assert!(is_builtin_persona("planner"));
    assert!(is_builtin_persona("maker"));
    assert!(!is_builtin_persona("custom"));
}

#[test]
fn builtin_personas_parse_with_name_description_icon() -> Result<()> {
    let parser = PulldownCmarkFrontMatterParser;

    // Planner front matter carries a name and a description.
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

    // Maker front matter carries a name and a description.
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

    // Every builtin persona has an icon.
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
