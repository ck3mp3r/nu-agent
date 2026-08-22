use super::StreamingMarkdownBuffer;

#[test]
fn no_open_construct_returns_entire_delta() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("hello world"), "hello world");
}

#[test]
fn unclosed_bold_buffers_marker() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("hello **bold"), "hello ");
}

#[test]
fn closing_bold_releases_buffered_text() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("hello **bold"), "hello ");
    assert_eq!(buf.push("**"), "**bold**");
}

#[test]
fn open_fence_buffers_until_close() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("before "), "before ");
    assert_eq!(
        buf.push("```rust\nfn main() {}\n```"),
        "```rust\nfn main() {}\n```"
    );
}

#[test]
fn content_inside_code_block_is_literal() {
    let mut buf = StreamingMarkdownBuffer::new();
    // A fully-closed fence with ** inside: bold markers are not tracked inside
    // a code block, so the whole block is released at once.
    assert_eq!(buf.push("```\n**not bold**\n```"), "```\n**not bold**\n```");
}

#[test]
fn flush_returns_unclosed_construct() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("text **bold"), "text ");
    assert_eq!(buf.flush(), "**bold");
}

#[test]
fn reset_clears_state() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("**bold"), "");
    buf.reset();
    assert_eq!(buf.push("fresh"), "fresh");
}

#[test]
fn inline_code_is_buffered() {
    let mut buf = StreamingMarkdownBuffer::new();
    assert_eq!(buf.push("use `code"), "use ");
    assert_eq!(buf.push("` here"), "`code` here");
}
