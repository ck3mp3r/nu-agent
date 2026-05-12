use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct MixedLayoutViolation {
    root_module: PathBuf,
    nested_test: PathBuf,
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("failed to read directory {}: {err}", dir.display()));

        for entry in entries {
            let entry = entry.unwrap_or_else(|err| {
                panic!("failed to read entry under {}: {err}", dir.display())
            });
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    files
}

fn find_mixed_layout_violations(root: &Path) -> Vec<MixedLayoutViolation> {
    let files = collect_rs_files(root);
    let file_set: std::collections::HashSet<PathBuf> = files.iter().cloned().collect();

    files
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "test.rs"))
        .filter_map(|test_file| {
            let module_dir = test_file.parent()?;
            let module_name = module_dir.file_name()?.to_str()?;
            let root_module = module_dir
                .parent()
                .unwrap_or(root)
                .join(format!("{module_name}.rs"));

            if file_set.contains(&root_module) {
                Some(MixedLayoutViolation {
                    root_module,
                    nested_test: test_file.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn module_test_layout_rejects_mixed_root_and_nested_test_pattern() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = find_mixed_layout_violations(&src_root);

    let detail = if violations.is_empty() {
        "none".to_string()
    } else {
        violations
            .iter()
            .map(|v| {
                format!(
                    "{} + {}",
                    v.root_module
                        .strip_prefix(&src_root)
                        .unwrap_or(&v.root_module)
                        .display(),
                    v.nested_test
                        .strip_prefix(&src_root)
                        .unwrap_or(&v.nested_test)
                        .display(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert!(
        violations.is_empty(),
        "mixed module/test layout detected. Forbidden: foo.rs + foo/test.rs\n{detail}"
    );
}

#[test]
fn mixed_layout_guard_detects_synthetic_violation() {
    let temp = tempfile::tempdir().expect("temp dir");
    let src = temp.path().join("src");
    let module_dir = src.join("foo");
    std::fs::create_dir_all(&module_dir).expect("create module dir");
    std::fs::write(src.join("foo.rs"), "pub fn f() {}\n").expect("write foo.rs");
    std::fs::write(module_dir.join("test.rs"), "#[test]\nfn it_works() {}\n")
        .expect("write foo/test.rs");

    let violations = find_mixed_layout_violations(&src);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].root_module, src.join("foo.rs"));
    assert_eq!(violations[0].nested_test, src.join("foo/test.rs"));
}
