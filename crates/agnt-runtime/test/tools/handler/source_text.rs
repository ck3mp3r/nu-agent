use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn text_table_labels_each_source() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (ToolSource::Closure, "closure"),
        (ToolSource::Mcp, "mcp"),
        (ToolSource::Builtin, "builtin"),
        (ToolSource::Unknown, "unknown"),
    ];

    // -- Exec & Check
    for (source, expected) in cases {
        assert_eq!(source.text(), expected, "case {source:?}");
    }
    Ok(())
}
