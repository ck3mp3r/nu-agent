use super::*;

/// The nu fence pipeline: a ```nu block maps tokens to code hints, the
/// `nushell` alias canonicalizes to nu, and an unknown language stays plain.
#[test]
fn markdown_projection_nu_fence_table() -> Result<()> {
    // -- Setup & Fixtures
    let nu_md = "```nu\nlet x = \"hello\" # comment\n```";
    let alias_md = "```nushell\nls | where size > 1mb\n```";
    let unknown_md = "```not-a-language\nlet x = \"hello\" # comment\n```";

    // -- Exec
    let nu_lines = project_markdown_to_lines(nu_md, None);
    let alias_lines = project_markdown_to_lines(alias_md, None);
    let unknown_lines = project_markdown_to_lines(unknown_md, None);

    // -- Check: nu fence maps `let`, the string, and the comment to code hints.
    let nu_line = nu_lines
        .iter()
        .find(|line| plain_line(line).contains("let x"))
        .ok_or("nu code line should be projected")?;
    assert!(
        line_has_code_hint(nu_line, StyleHint::MdCodeKeyword),
        "nu fence should map `let` to MdCodeKeyword"
    );
    assert!(
        line_has_code_hint(nu_line, StyleHint::MdCodeString),
        "nu fence should map the double-quoted string to MdCodeString"
    );
    assert!(
        line_has_code_hint(nu_line, StyleHint::MdCodeComment),
        "nu fence should map `# comment` to MdCodeComment"
    );

    // -- Check: the `nushell` alias reaches the nu syntax.
    let alias_line = alias_lines
        .iter()
        .find(|line| plain_line(line).contains("where"))
        .ok_or("nushell code line should be projected")?;
    assert!(
        line_has_non_default_token_style(alias_line),
        "nushell alias fence should carry non-plain token hints"
    );

    // -- Check: an unknown fence language stays plain.
    let unknown_line = unknown_lines
        .iter()
        .find(|line| plain_line(line).contains("let x"))
        .ok_or("unknown-language code line should be projected")?;
    assert!(
        unknown_line
            .spans
            .iter()
            .all(|span| matches!(span.hint, StyleHint::Normal | StyleHint::MdCodePlain)),
        "unknown fence language must produce only Normal/MdCodePlain hints"
    );
    Ok(())
}
