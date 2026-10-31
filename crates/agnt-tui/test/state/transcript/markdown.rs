use super::*;

/// Markdown projection is deterministic for the same input and does not depend
/// on a cache.
#[test]
fn markdown_projection_is_deterministic() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "```rust\nfn main() {\n    let x = 42;\n}\n```";

    // -- Exec
    let first = crate::markdown::render_markdown_lines(markdown, None);
    let second = crate::markdown::render_markdown_lines(markdown, None);

    // -- Check
    assert_eq!(first, second);

    // -- Setup & Fixtures: plain text.
    let plain = "hello world";

    // -- Exec
    let first = crate::markdown::render_markdown_lines(plain, None);
    let second = crate::markdown::render_markdown_lines(plain, None);

    // -- Check: projecting again must produce the identical output.
    assert_eq!(first, second, "projection must not depend on a cache");
    Ok(())
}

/// Pushing transcript items follows the tail while the user is at the last
/// item, and stays put once the user has scrolled up.
#[test]
fn push_transcript_item_tail_follow() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: following_tail starts true and stays true while tailing.
    assert!(state.scroll.following_tail);
    push_user(&mut state, "first");
    assert!(state.scroll.following_tail);
    push_assistant(&mut state, "second");
    assert!(state.scroll.following_tail);
    push_user(&mut state, "third");
    assert!(state.scroll.following_tail);

    // -- Exec: the user scrolls to the top.
    state.scroll.scroll_transcript_to_top();
    assert!(!state.scroll.following_tail);
    assert_eq!(state.scroll.scroll_offset, 0);

    // -- Check: a new push does not re-enable following.
    push_assistant(&mut state, "fourth");
    assert!(
        !state.scroll.following_tail,
        "following_tail should stay false when user has scrolled up"
    );
    assert_eq!(
        state.scroll.scroll_offset, 0,
        "scroll offset should stay at top when user has scrolled up"
    );
    Ok(())
}

/// User and assistant prose project identically; bold spans carry MdBold and a
/// fenced code block projects to multiple lines.
#[test]
fn push_transcript_item_markdown_projection() -> Result<()> {
    // -- Setup & Fixtures
    let mut user = AppState::default();
    push_user(&mut user, "hello **world**");

    // -- Check: the user entry's raw markdown projects to MdBold.
    let markdown = last_message_markdown(&user)?;
    let bold = crate::markdown::render_markdown_lines(&markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, agnt_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");

    // -- Setup & Fixtures: an assistant entry.
    let mut assistant = AppState::default();
    push_assistant(&mut assistant, "hello **world**");

    // -- Check
    let markdown = last_message_markdown(&assistant)?;
    let bold = crate::markdown::render_markdown_lines(&markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, agnt_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");

    // -- Setup & Fixtures: identical text for user and assistant.
    let mut s1 = AppState::default();
    let mut s2 = AppState::default();
    let text = "**bold** and *italic* and `code`".to_string();
    push_user(&mut s1, &text);
    push_assistant(&mut s2, &text);

    // -- Check: user and assistant prose must be byte-identical.
    let u = last_message_markdown(&s1)?;
    let a = last_message_markdown(&s2)?;
    assert_eq!(u, a, "user and assistant prose must be byte-identical");

    // -- Setup & Fixtures: a fenced code block.
    let mut fenced = AppState::default();
    push_user(&mut fenced, "```rust\nfn a() {}\nfn b() {}\n```");

    // -- Check: projection yields multiple lines.
    let markdown = last_message_markdown(&fenced)?;
    let projected = crate::markdown::render_markdown_lines(&markdown, None);
    assert!(projected.len() >= 2);
    Ok(())
}

/// The raw markdown of the last stored message entry.
fn last_message_markdown(state: &AppState) -> Result<String> {
    let last = state
        .transcript
        .entries()
        .last()
        .ok_or("should have last transcript entry")?;
    let TranscriptEntry::Message(message) = last else {
        return Err("expected message entry".into());
    };
    Ok(message.markdown.clone())
}
