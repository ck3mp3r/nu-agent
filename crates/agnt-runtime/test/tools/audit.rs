use super::*;
use chrono::Utc;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn logs_three_entries_to_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("tool_audit.log");
    let logger = AuditLogger::new(log_path.clone());

    // Log 3 entries
    for i in 0..3 {
        let entry = AuditEntry {
            timestamp: Utc::now(),
            tool_name: format!("tool_{}", i),
            args: serde_json::json!({"arg": i}),
            result: AuditResult::Ok(serde_json::json!(i * 2)),
            duration_ms: 100 + i * 10,
        };
        logger.log(entry).await.unwrap();
    }

    // Verify 3 JSON lines
    let content = fs::read_to_string(&log_path).await.unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);

    // Verify each line is valid JSON
    for (i, line) in lines.iter().enumerate() {
        let entry: AuditEntry = serde_json::from_str(line).unwrap();
        assert_eq!(entry.tool_name, format!("tool_{}", i));
        assert!(entry.duration_ms > 0);
    }
}

#[tokio::test]
async fn requires_parent_directory_exists() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir
        .path()
        .join("nested")
        .join("path")
        .join("tool_audit.log");

    // Parent directory does NOT exist
    assert!(!log_path.parent().unwrap().exists());

    let logger = AuditLogger::new(log_path.clone());

    let entry = AuditEntry {
        timestamp: Utc::now(),
        tool_name: "test_tool".to_string(),
        args: serde_json::json!({"test": "arg"}),
        result: AuditResult::Ok(serde_json::json!("success")),
        duration_ms: 42,
    };

    // Should FAIL because directory doesn't exist
    // This validates the CORRECT contract: caller creates directory, logger only logs
    let result = logger.log(entry).await;
    assert!(
        result.is_err(),
        "Logger should fail when parent directory doesn't exist"
    );

    // Now create directory and try again
    tokio::fs::create_dir_all(log_path.parent().unwrap())
        .await
        .unwrap();

    let entry2 = AuditEntry {
        timestamp: Utc::now(),
        tool_name: "test_tool_2".to_string(),
        args: serde_json::json!({"test": "arg2"}),
        result: AuditResult::Ok(serde_json::json!("success2")),
        duration_ms: 100,
    };

    // Should SUCCEED now that directory exists
    logger.log(entry2).await.unwrap();
    assert!(
        log_path.exists(),
        "Log file should exist after successful log"
    );
}
