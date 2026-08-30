pub fn merge_prompt_with_context(prompt: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx) if !ctx.trim().is_empty() => {
            format!("{ctx}\n\n---\n\n{prompt}")
        }
        _ => prompt.to_string(),
    }
}

pub fn merge_preamble_with_prompt_and_context(
    prompt: &str,
    context: Option<&str>,
    preamble: Option<&str>,
) -> String {
    let merged_prompt = merge_prompt_with_context(prompt, context);

    match preamble {
        Some(p) if !p.trim().is_empty() => {
            merge_prompt_with_context(&merged_prompt, Some(p.trim()))
        }
        _ => merged_prompt,
    }
}
