pub fn summarize_tool_arguments(arguments: &str) -> String {
    const MAX_LEN: usize = 120;
    let compact = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    let spaced = compact.replace(", ", ",").replace(",", ", ");
    if spaced.chars().count() <= MAX_LEN {
        return spaced;
    }

    let mut truncated = spaced.chars().take(MAX_LEN).collect::<String>();
    truncated.push('…');
    truncated
}
