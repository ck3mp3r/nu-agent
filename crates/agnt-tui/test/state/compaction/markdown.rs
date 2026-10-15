use super::*;

/// Compaction summary chunks project as markdown: italic spans render as
/// MdItalic, and a fenced body renders as markdown, not raw text.
#[test]
fn compaction_summary_chunks_render_as_markdown() -> Result<()> {
    // -- Setup & Fixtures
    let mut italic = AppState::default();
    reduce_compaction(
        &mut italic,
        chunk("history", "summary *italic*", "summary *italic*"),
    );

    // -- Check: italic spans carry MdItalic.
    assert!(assistant_markdown_entries(&italic).iter().any(|md| {
        project_spans(md)
            .iter()
            .any(|(t, h)| t == "italic" && matches!(h, StyleHint::MdItalic))
    }));

    // -- Setup & Fixtures: fenced body.
    let mut fenced = AppState::default();
    reduce_compaction(&mut fenced, started("auto"));
    reduce_compaction(
        &mut fenced,
        completed(
            "auto",
            "",
            "```\n## Work State\n### Completed\n- Mapped c5t notes for project `63e90e73`; confirmed `7722bef9`.\n```",
        ),
    );

    // -- Check: the fenced wrapper must not leak raw markdown into the body.
    let markdowns = assistant_markdown_entries(&fenced);
    let concat: String = markdowns.join("\n");
    let projected = project_spans(&concat);
    assert!(
        projected.iter().any(|(t, _)| t == "Work State"),
        "heading text must render; got: {projected:?}"
    );
    assert!(
        projected.iter().any(|(t, _)| t.starts_with('•')),
        "bullet marker must render as '•'; got: {projected:?}"
    );
    assert!(
        !projected.iter().any(|(t, _)| t.starts_with("##")),
        "raw '##' must not appear in rendered body; got: {projected:?}"
    );
    Ok(())
}
