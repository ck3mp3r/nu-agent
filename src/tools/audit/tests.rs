use super::*;
use chrono::Utc;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn logs_three_entries_to_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("tool_audit.log");
    let logger = AuditLogger::with_path(log_path.clone());

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
async fn creates_log_directory_if_missing() {
    let logger = AuditLogger::new().unwrap();
    assert!(logger.log_path().parent().unwrap().exists());
}
