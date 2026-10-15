use super::*;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn bounded_tree_cache_table() {
    // -- Setup & Fixtures
    #[derive(Clone)]
    enum Op {
        Insert(String, u32),
        Get(String),
    }

    struct Case {
        name: &'static str,
        ops: Vec<Op>,
        expected_len: usize,
        expected: Vec<(String, Option<u32>)>,
    }

    let fill: Vec<Op> = (0..TREE_CACHE_CAPACITY)
        .map(|i| Op::Insert(format!("file-{i}.rs"), i as u32))
        .collect();

    let mut evict_ops = fill.clone();
    evict_ops.push(Op::Insert(
        format!("file-{TREE_CACHE_CAPACITY}.rs"),
        TREE_CACHE_CAPACITY as u32,
    ));

    let mut lru_ops = fill.clone();
    lru_ops.push(Op::Get("file-0.rs".to_string()));
    lru_ops.push(Op::Insert("file-new.rs".to_string(), 999));

    let cases = vec![
        Case {
            name: "evicts the oldest entry when over capacity",
            ops: evict_ops,
            expected_len: TREE_CACHE_CAPACITY,
            expected: vec![
                ("file-0.rs".to_string(), None),
                (
                    format!("file-{TREE_CACHE_CAPACITY}.rs"),
                    Some(TREE_CACHE_CAPACITY as u32),
                ),
            ],
        },
        Case {
            name: "get refreshes the LRU order",
            ops: lru_ops,
            expected_len: TREE_CACHE_CAPACITY,
            expected: vec![
                ("file-1.rs".to_string(), None),
                ("file-0.rs".to_string(), Some(0)),
                ("file-new.rs".to_string(), Some(999)),
            ],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let mut cache = BoundedTreeCache::<u32>::new();
        for op in &case.ops {
            match op {
                Op::Insert(path, value) => {
                    cache.insert(PathBuf::from(path), *value);
                }
                Op::Get(path) => {
                    cache.get(Path::new(path));
                }
            }
        }
        assert_eq!(
            cache.map.len(),
            case.expected_len,
            "case '{}': cache len",
            case.name
        );
        for (path, expected) in &case.expected {
            assert_eq!(
                cache.get(Path::new(path)),
                expected.as_ref(),
                "case '{}': cache entry for {path}",
                case.name
            );
        }
    }
}

#[test]
fn bounded_tree_cache_reinsert_refreshes_the_existing_entry() {
    // -- Setup & Fixtures
    let mut cache = BoundedTreeCache::<u32>::new();
    cache.insert(PathBuf::from("a.rs"), 1);
    cache.insert(PathBuf::from("b.rs"), 2);

    // -- Exec
    cache.insert(PathBuf::from("a.rs"), 10);

    // -- Check
    assert_eq!(cache.map.len(), 2, "a re-insert must not grow the cache");
    assert_eq!(cache.get(Path::new("a.rs")), Some(&10));
    assert_eq!(
        cache.order.len(),
        2,
        "a re-insert must not duplicate the LRU order"
    );
    assert_eq!(
        cache.order.back(),
        Some(&PathBuf::from("a.rs")),
        "a re-insert must move the entry to the most-recently-used end"
    );
}

#[tokio::test]
async fn ast_tools_missing_required_arg_table() {
    // -- Setup & Fixtures
    enum Tool {
        Query,
        Nodes,
        Refs,
        Tree,
    }

    let cases: Vec<(&str, Tool, serde_json::Value)> = vec![
        (
            "ast_query missing query",
            Tool::Query,
            serde_json::json!({"language": "rust"}),
        ),
        (
            "ast_nodes missing node_type",
            Tool::Nodes,
            serde_json::json!({"language": "rust"}),
        ),
        (
            "ast_refs missing name",
            Tool::Refs,
            serde_json::json!({"language": "rust"}),
        ),
        (
            "ast_tree missing language",
            Tool::Tree,
            serde_json::json!({}),
        ),
    ];

    // -- Exec & Check
    for (name, tool, mut args) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        args["path"] = serde_json::json!(path.to_str().unwrap());

        let err = match tool {
            Tool::Query => AstQueryTool::execute(&args, dir.path(), &Bus::default()).await,
            Tool::Nodes => AstNodesTool::execute(&args, dir.path(), &Bus::default()).await,
            Tool::Refs => AstRefsTool::execute(&args, dir.path(), &Bus::default()).await,
            Tool::Tree => AstTreeTool::execute(&args, dir.path(), &Bus::default()).await,
        }
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Validation, "case '{name}': kind");
    }
}

#[tokio::test]
#[serial_test::serial]
async fn missing_config_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    // A language no grammar test loads, so a warm grammar cache still reports
    // the unknown language instead of parsing successfully.
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "cobol" });
    // Point TREE_SITTER_DIR at a nonexistent path so tree_sitter_config finds no
    // config regardless of the host's actual tree-sitter setup.
    let missing_dir = dir.path().join("nonexistent-tree-sitter-dir");
    let previous = std::env::var_os("TREE_SITTER_DIR");
    unsafe { std::env::set_var("TREE_SITTER_DIR", &missing_dir) };
    let result = AstTreeTool::execute(&args, dir.path(), &Bus::default()).await;
    match previous {
        Some(v) => unsafe { std::env::set_var("TREE_SITTER_DIR", v) },
        None => unsafe { std::env::remove_var("TREE_SITTER_DIR") },
    }
    // The grammar cache is a process-global `OnceLock`: once any test has
    // loaded a grammar, the missing config is never consulted again and the
    // tool reports the unknown language instead. Both outcomes are failures
    // that name tree-sitter, which is what this test pins.
    let err = result.unwrap_err();
    match err.kind {
        ToolErrorKind::Runtime => assert!(
            err.message.contains("No tree-sitter config found"),
            "a cold grammar cache must report the missing config: {err:?}"
        ),
        ToolErrorKind::Validation => assert!(
            err.message.contains("No tree-sitter grammar found"),
            "a warm grammar cache must report the unknown language: {err:?}"
        ),
        other => panic!("unexpected error kind: {other:?}"),
    }
}

// ── E2E tests with fixtures ────────────────────────────────────────────────

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/tools/handler/testdata")
        .join(name)
}

#[tokio::test]
async fn e2e_nonexistent_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.rs");
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust", "query": "(function_item)" });
    let err = AstQueryTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Runtime);
    assert!(err.message.contains("File not found"));
}

/// End-to-end AST tool behaviour. These cases need a real tree-sitter grammar
/// installation, so the whole table is `#[ignore]`d.
#[tokio::test]
#[ignore]
async fn ast_tools_end_to_end_table() -> Result<()> {
    // -- Setup & Fixtures
    enum Source {
        Inline(&'static str),
        Fixture(&'static str),
    }

    enum Expect {
        QueryNames(Vec<&'static str>),
        NodeType(&'static str),
        RefTexts(&'static str),
        TreeContains(Vec<&'static str>),
    }

    struct Case {
        name: &'static str,
        source: Source,
        args: serde_json::Value,
        expect: Expect,
    }

    let cases = vec![
        Case {
            name: "ast_query finds function names",
            source: Source::Inline("fn foo() {}\nfn bar() {}\n"),
            args: serde_json::json!({
                "language": "rust",
                "query": "(function_item name: (identifier) @fn-name)"
            }),
            expect: Expect::QueryNames(vec!["foo", "bar"]),
        },
        Case {
            name: "ast_nodes finds match arms",
            source: Source::Inline(
                "fn main() {\n    let x = 1;\n    match x {\n        1 => println!(\"one\"),\n        _ => println!(\"other\"),\n    }\n}\n",
            ),
            args: serde_json::json!({"language": "rust", "node_type": "match_arm"}),
            expect: Expect::NodeType("match_arm"),
        },
        Case {
            name: "ast_refs finds a symbol",
            source: Source::Inline("struct Foo {}\nfn main() {\n    let x: Foo = Foo;\n}\n"),
            args: serde_json::json!({"language": "rust", "name": "Foo"}),
            expect: Expect::RefTexts("Foo"),
        },
        Case {
            name: "ast_tree returns sexp",
            source: Source::Inline("fn main() {}"),
            args: serde_json::json!({"language": "rust"}),
            expect: Expect::TreeContains(vec!["source_file", "function_item"]),
        },
        Case {
            name: "ast_tree with max_depth 1 truncates",
            source: Source::Inline("fn main() {}"),
            args: serde_json::json!({"language": "rust", "max_depth": 1}),
            expect: Expect::TreeContains(vec!["..."]),
        },
        Case {
            name: "e2e ast_query on sample.rs returns function matches",
            source: Source::Fixture("sample.rs"),
            args: serde_json::json!({
                "language": "rust",
                "query": "(function_item name: (identifier) @fn-name)"
            }),
            expect: Expect::QueryNames(vec!["process", "main"]),
        },
        Case {
            name: "e2e ast_nodes on sample.rs returns match arms",
            source: Source::Fixture("sample.rs"),
            args: serde_json::json!({"language": "rust", "node_type": "match_arm"}),
            expect: Expect::NodeType("match_arm"),
        },
        Case {
            name: "e2e ast_refs on sample.rs finds status occurrences",
            source: Source::Fixture("sample.rs"),
            args: serde_json::json!({"language": "rust", "name": "Status"}),
            expect: Expect::RefTexts("Status"),
        },
        Case {
            name: "e2e ast_tree on sample.rs returns valid sexp",
            source: Source::Fixture("sample.rs"),
            args: serde_json::json!({"language": "rust"}),
            expect: Expect::TreeContains(vec!["source_file", "function_item", "struct_item"]),
        },
        Case {
            name: "e2e ast_tree with max_depth 1 truncates",
            source: Source::Fixture("sample.rs"),
            args: serde_json::json!({"language": "rust", "max_depth": 1}),
            expect: Expect::TreeContains(vec!["..."]),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = match case.source {
            Source::Inline(source) => {
                let path = dir.path().join("main.rs");
                std::fs::write(&path, source)?;
                path
            }
            Source::Fixture(name) => fixture_path(name),
        };
        let cwd = path.parent().ok_or("source path must have a parent")?;
        let mut args = case.args.clone();
        args["path"] = serde_json::json!(path.to_str().ok_or("path must be UTF-8")?);

        let result = match &case.expect {
            Expect::QueryNames(_) => AstQueryTool::execute(&args, cwd, &Bus::default()).await,
            Expect::NodeType(_) => AstNodesTool::execute(&args, cwd, &Bus::default()).await,
            Expect::RefTexts(_) => AstRefsTool::execute(&args, cwd, &Bus::default()).await,
            Expect::TreeContains(_) => AstTreeTool::execute(&args, cwd, &Bus::default()).await,
        }
        .map_err(|e| format!("{e:?}"))?;

        match &case.expect {
            Expect::QueryNames(names) => {
                let matches = result["matches"]
                    .as_array()
                    .ok_or("matches must be an array")?;
                assert!(
                    !matches.is_empty(),
                    "case '{}': matches must not be empty",
                    case.name
                );
                let found: Vec<String> = matches
                    .iter()
                    .filter_map(|m| m["captures"]["fn-name"]["text"].as_str().map(String::from))
                    .collect();
                for name in names {
                    assert!(
                        found.contains(&name.to_string()),
                        "case '{}': expected {name}, got {found:?}",
                        case.name
                    );
                }
            }
            Expect::NodeType(node_type) => {
                let nodes = result["nodes"].as_array().ok_or("nodes must be an array")?;
                assert!(
                    !nodes.is_empty(),
                    "case '{}': nodes must not be empty",
                    case.name
                );
                assert_eq!(
                    result["node_type"], *node_type,
                    "case '{}': node_type",
                    case.name
                );
            }
            Expect::RefTexts(expected) => {
                let matches = result["matches"]
                    .as_array()
                    .ok_or("matches must be an array")?;
                assert!(
                    !matches.is_empty(),
                    "case '{}': matches must not be empty",
                    case.name
                );
                let texts: Vec<String> = matches
                    .iter()
                    .filter_map(|m| m["captures"]["name"]["text"].as_str().map(String::from))
                    .collect();
                assert!(
                    texts.iter().all(|t| t == expected),
                    "case '{}': every capture must be {expected}, got {texts:?}",
                    case.name
                );
            }
            Expect::TreeContains(fragments) => {
                let tree = result["tree"].as_str().ok_or("tree must be a string")?;
                for fragment in fragments {
                    assert!(
                        tree.contains(fragment),
                        "case '{}': tree must contain {fragment}, got {tree}",
                        case.name
                    );
                }
            }
        }
    }
    Ok(())
}
