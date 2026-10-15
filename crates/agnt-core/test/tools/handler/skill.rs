use super::*;
use crate::bus::Bus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn skill_lookup_table() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".agents").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("test.md"), "# Test skill\ncontent here\n").unwrap();

    let cases: Vec<(&str, &str, bool)> = vec![
        ("found skill", "test", true),
        ("missing skill", "nonexistent", false),
    ];

    // -- Exec & Check
    for (name, skill_name, expected_found) in cases {
        let args = serde_json::json!({ "name": skill_name });
        let result = SkillTool::execute(&args, dir.path(), &Bus::default())
            .await
            .map_err(|e| format!("{e:?}"))?;

        assert_eq!(result["name"], skill_name, "case '{name}': name");
        if expected_found {
            assert!(
                result["content"].as_str().unwrap().contains("content here"),
                "case '{name}': content"
            );
            assert_eq!(result["source"], "local", "case '{name}': source");
        } else {
            assert_eq!(result["found"], false, "case '{name}': found");
        }
    }
    Ok(())
}

#[tokio::test]
async fn skill_missing_name_arg_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({});
    let result = SkillTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().kind,
        super::super::ToolErrorKind::Validation
    );
}
