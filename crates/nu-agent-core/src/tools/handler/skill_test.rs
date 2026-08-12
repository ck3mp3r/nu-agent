use super::*;
use crate::bus::Bus;

#[tokio::test]
async fn skill_returns_content_when_found() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".agents").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("test.md"), "# Test skill\ncontent here\n").unwrap();

    let args = serde_json::json!({ "name": "test" });
    let result = SkillTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert!(result["content"].as_str().unwrap().contains("content here"));
    assert_eq!(result["name"], "test");
    assert_eq!(result["source"], "local");
}

#[tokio::test]
async fn skill_returns_not_found_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "name": "nonexistent" });
    let result = SkillTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["found"], false);
    assert_eq!(result["name"], "nonexistent");
}

#[tokio::test]
async fn skill_missing_name_arg_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({});
    let result = SkillTool::execute(&args, dir.path(), &Bus::new()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().kind,
        super::super::ToolErrorKind::Validation
    );
}
