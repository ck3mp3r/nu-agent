use super::*;

/// Assistant markdown is stored raw and projects to styled spans; streaming
/// replaces the prior render rather than appending.
#[test]
fn assistant_markdown_projection_and_streaming_replace() -> Result<()> {
    // -- Setup & Fixtures
    let mut bold = AppState::default();

    // -- Exec
    reduce_llm(&mut bold, assistant_message("hello **bold**"));

    // -- Check: the raw markdown is stored and projects to MdBold.
    assert!(assistant_markdown_entries(&bold).iter().any(|md| {
        project_spans(md)
            .iter()
            .any(|(t, h)| t == "bold" && matches!(h, agnt_core::transcript::ir::StyleHint::MdBold))
    }));

    // -- Setup & Fixtures: streaming.
    let mut stream = AppState::default();

    // -- Exec
    for text in ["hello", "hello world"] {
        reduce_llm(&mut stream, assistant_message(text));
    }

    // -- Check: a single assistant block with the final text.
    let markdowns = assistant_markdown_entries(&stream);
    let concat: String = markdowns.join("");
    assert!(concat.contains("hello world"));
    assert!(!concat.contains("hellohello"));
    Ok(())
}
