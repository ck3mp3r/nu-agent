use super::{ToolErrorKind, dispatch_grep};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn grep_finds_literal_match() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("foo.rs");
    std::fs::write(&file, "fn hello_world() {}\n").unwrap();

    let result = dispatch_grep("hello_world", dir.path(), None, false, 200).unwrap();

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0]["file"].as_str().unwrap().contains("foo.rs"));
    assert_eq!(matches[0]["line"], 1);
    assert!(
        matches[0]["content"]
            .as_str()
            .unwrap()
            .contains("hello_world")
    );
    assert_eq!(result["truncated"], false);
}

#[test]
fn grep_returns_empty_when_no_match() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "fn unrelated() {}\n").unwrap();

    let result = dispatch_grep("xyzzy_no_match", dir.path(), None, false, 200).unwrap();

    assert_eq!(result["matches"].as_array().unwrap().len(), 0);
    assert_eq!(result["total"], 0);
    assert_eq!(result["truncated"], false);
}

#[test]
fn grep_case_insensitive_flag() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "Hello World\n").unwrap();

    let sensitive = dispatch_grep("hello world", dir.path(), None, false, 200).unwrap();
    assert_eq!(sensitive["matches"].as_array().unwrap().len(), 0);

    let insensitive = dispatch_grep("hello world", dir.path(), None, true, 200).unwrap();
    assert_eq!(insensitive["matches"].as_array().unwrap().len(), 1);
}

#[test]
fn grep_glob_filter_limits_files() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "needle\n").unwrap();
    std::fs::write(dir.path().join("foo.txt"), "needle\n").unwrap();

    let rs_only = dispatch_grep("needle", dir.path(), Some("*.rs"), false, 200).unwrap();
    let txt_only = dispatch_grep("needle", dir.path(), Some("*.txt"), false, 200).unwrap();

    assert_eq!(rs_only["matches"].as_array().unwrap().len(), 1);
    assert!(
        rs_only["matches"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with(".rs")
    );

    assert_eq!(txt_only["matches"].as_array().unwrap().len(), 1);
    assert!(
        txt_only["matches"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with(".txt")
    );
}

#[test]
fn grep_respects_max_results_and_sets_truncated() {
    let dir = tempdir().unwrap();
    let content = (1..=10)
        .map(|i| format!("match line {i}\n"))
        .collect::<String>();
    std::fs::write(dir.path().join("foo.rs"), content).unwrap();

    let capped = dispatch_grep("match line", dir.path(), None, false, 3).unwrap();
    assert_eq!(capped["matches"].as_array().unwrap().len(), 3);
    assert_eq!(capped["truncated"], true);

    let uncapped = dispatch_grep("match line", dir.path(), None, false, 20).unwrap();
    assert_eq!(uncapped["matches"].as_array().unwrap().len(), 10);
    assert_eq!(uncapped["truncated"], false);
}

#[test]
fn grep_invalid_regex_returns_validation_error() {
    let dir = tempdir().unwrap();
    let err = dispatch_grep("[invalid", dir.path(), None, false, 200).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Invalid regex pattern"));
}

#[test]
fn grep_searches_subdirectories_recursively() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("subdir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("nested.rs"), "deep_match\n").unwrap();

    let result = dispatch_grep("deep_match", dir.path(), None, false, 200).unwrap();

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    let file = matches[0]["file"].as_str().unwrap();
    assert!(
        file.contains("subdir"),
        "expected subdir in path, got: {file}"
    );
    assert!(file.ends_with("nested.rs"));
}

#[test]
fn grep_skips_binary_files_without_error() {
    let dir = tempdir().unwrap();
    let mut f = std::fs::File::create(dir.path().join("binary.bin")).unwrap();
    f.write_all(&[0u8, 1u8, 2u8, 0u8]).unwrap();
    std::fs::write(dir.path().join("normal.rs"), "no match here\n").unwrap();

    let result = dispatch_grep("anything", dir.path(), None, false, 200);
    assert!(
        result.is_ok(),
        "should not error on binary files: {result:?}"
    );
}
