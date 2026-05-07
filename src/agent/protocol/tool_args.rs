pub fn summarize_tool_arguments(arguments: &str) -> String {
    const MAX_LEN: usize = 120;
    let compact = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_LEN {
        return compact;
    }

    let mut truncated = compact.chars().take(MAX_LEN).collect::<String>();
    truncated.push('…');
    truncated
}
