use super::core::{ReadRequest, read_file};
use std::fs;
use tempfile::tempdir;

#[test]
fn read_file_without_offset_limit_returns_full_content_and_metadata() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("sample.txt");
    fs::write(&file, "line1\nline2\nline3\n").expect("write file");

    let response = read_file(&file, ReadRequest::default()).expect("read file");

    assert_eq!(response.content, "line1\nline2\nline3\n");
    assert_eq!(response.total_lines, 3);
    assert_eq!(response.offset, None);
    assert_eq!(response.limit, None);
    assert!(!response.version.is_empty());
}

#[test]
fn read_file_with_offset_and_limit_returns_window_content_and_metadata() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("window.txt");
    fs::write(&file, "a\nb\nc\nd\n").expect("write file");

    let response = read_file(
        &file,
        ReadRequest {
            offset: Some(1),
            limit: Some(2),
        },
    )
    .expect("read file");

    assert_eq!(response.content, "b\nc\n");
    assert_eq!(response.total_lines, 4);
    assert_eq!(response.offset, Some(1));
    assert_eq!(response.limit, Some(2));
    assert!(!response.version.is_empty());
}

#[test]
fn read_file_empty_file_is_deterministic() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").expect("write file");

    let response = read_file(&file, ReadRequest::default()).expect("read file");

    assert_eq!(response.content, "");
    assert_eq!(response.total_lines, 0);
    assert_eq!(response.offset, None);
    assert_eq!(response.limit, None);
    assert!(!response.version.is_empty());
}

#[test]
fn read_file_offset_beyond_eof_returns_empty_window_deterministically() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("eof.txt");
    fs::write(&file, "x\ny\n").expect("write file");

    let response = read_file(
        &file,
        ReadRequest {
            offset: Some(10),
            limit: Some(3),
        },
    )
    .expect("read file");

    assert_eq!(response.content, "");
    assert_eq!(response.total_lines, 2);
    assert_eq!(response.offset, Some(10));
    assert_eq!(response.limit, Some(3));
    assert!(!response.version.is_empty());
}
