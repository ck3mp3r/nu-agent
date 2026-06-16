use super::{BUILTIN_MAKER_CONTENT, BUILTIN_PLANNER_CONTENT, is_builtin_persona};
use crate::protocol::persona::{
    FrontMatterParser, PulldownCmarkFrontMatterParser, interpret_front_matter,
};

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
fn test_builtin_content_has_valid_front_matter() {
    let parser = PulldownCmarkFrontMatterParser;

    // Planner
    let planner_raw = parser
        .parse(BUILTIN_PLANNER_CONTENT)
        .expect("planner content should parse");
    let planner_parsed =
        interpret_front_matter(planner_raw.front_matter.as_ref(), planner_raw.body)
            .expect("planner front matter should be interpretable");
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
        .expect("maker content should parse");
    let maker_parsed = interpret_front_matter(maker_raw.front_matter.as_ref(), maker_raw.body)
        .expect("maker front matter should be interpretable");
    assert!(
        maker_parsed.name.is_some(),
        "maker front matter must contain a name"
    );
    assert!(
        maker_parsed.description.is_some(),
        "maker front matter must contain a description"
    );
}
