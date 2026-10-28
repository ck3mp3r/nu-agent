use super::resolve_task_id;
use crate::client::{MockHttpClient, page_body};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Rules 1-3 — validation before any HTTP request
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_resolve_task_id_uuid_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();

    // -- Exec
    let resolved = resolve_task_id(
        &mock,
        "http://example.com",
        "550E8400-E29B-41D4-A716-446655440000",
    )
    .await;

    // -- Check
    assert_eq!(
        resolved
            .ok()
            .ok_or("should pass a full UUID through unchanged")?,
        "550E8400-E29B-41D4-A716-446655440000"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_invalid_charset() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();

    // -- Exec
    let err = resolve_task_id(&mock, "http://example.com", "xyz12345")
        .await
        .err()
        .ok_or("should reject a non-hex task id")?;

    // -- Check
    assert!(
        err.contains("invalid task id 'xyz12345'"),
        "unexpected error: {err}"
    );
    assert!(
        !err.contains("no mock response registered"),
        "should not issue an HTTP request: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_too_short() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();

    // -- Exec
    let err = resolve_task_id(&mock, "http://example.com", "abc1234")
        .await
        .err()
        .ok_or("should reject a short task id")?;

    // -- Check
    assert!(
        err.contains("minimum of 8 characters"),
        "unexpected error: {err}"
    );
    assert!(
        !err.contains("no mock response registered"),
        "should not issue an HTTP request: {err}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Rule 4 — prefix matching against the full task list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_resolve_task_id_unique_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?pageSize=100",
        page_body(
            &[
                ("bbbbbbbb-0000-0000-0000-000000000001", "TASK_STATE_WORKING"),
                (
                    "cccccccc-0000-0000-0000-000000000002",
                    "TASK_STATE_COMPLETED",
                ),
            ],
            "",
        )?,
    );

    // -- Exec
    let resolved = resolve_task_id(&mock, "http://example.com", "BBBBBBBB").await;

    // -- Check
    assert_eq!(
        resolved
            .ok()
            .ok_or("should resolve a unique prefix case-insensitively")?,
        "bbbbbbbb-0000-0000-0000-000000000001"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_uppercase_server_id_with_lowercase_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?pageSize=100",
        page_body(
            &[("AAAAAAAA-0000-0000-0000-000000000001", "TASK_STATE_WORKING")],
            "",
        )?,
    );

    // -- Exec
    let resolved = resolve_task_id(&mock, "http://example.com", "aaaaaaaa").await;

    // -- Check
    assert_eq!(
        resolved
            .ok()
            .ok_or("should match an uppercase server ID case-insensitively")?,
        "AAAAAAAA-0000-0000-0000-000000000001",
        "resolved ID must be the server's own string"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_mixed_case_server_id_with_mixed_case_input() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?pageSize=100",
        page_body(
            &[(
                "aAaAaAaA-0000-0000-0000-000000000001",
                "TASK_STATE_COMPLETED",
            )],
            "",
        )?,
    );

    // -- Exec
    let resolved = resolve_task_id(&mock, "http://example.com", "AaAaAaAa").await;

    // -- Check
    assert_eq!(
        resolved
            .ok()
            .ok_or("should match a mixed-case server ID regardless of input case")?,
        "aAaAaAaA-0000-0000-0000-000000000001",
        "resolved ID must be the server's own string"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_no_match() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?pageSize=100",
        page_body(
            &[("bbbbbbbb-0000-0000-0000-000000000001", "TASK_STATE_WORKING")],
            "",
        )?,
    );

    // -- Exec
    let err = resolve_task_id(&mock, "http://example.com", "dddddddd")
        .await
        .err()
        .ok_or("should report no match")?;

    // -- Check
    assert!(
        err.contains("no task found with id prefix 'dddddddd'"),
        "unexpected error: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_resolve_task_id_ambiguous() -> Result<()> {
    // -- Setup & Fixtures
    let mock = MockHttpClient::default();
    mock.expect_get_ok(
        "http://example.com/tasks?pageSize=100",
        page_body(
            &[
                ("aaaaaaaa-0000-0000-0000-000000000001", "TASK_STATE_WORKING"),
                (
                    "aaaaaaaa-0000-0000-0000-000000000002",
                    "TASK_STATE_COMPLETED",
                ),
            ],
            "",
        )?,
    );

    // -- Exec
    let err = resolve_task_id(&mock, "http://example.com", "aaaaaaaa")
        .await
        .err()
        .ok_or("should report an ambiguous prefix")?;

    // -- Check
    assert!(
        err.contains("ambiguous task id prefix 'aaaaaaaa' matches 2 tasks"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("aaaaaaaa-0000-0000-0000-000000000001 (working)"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("aaaaaaaa-0000-0000-0000-000000000002 (completed)"),
        "unexpected error: {err}"
    );
    Ok(())
}
