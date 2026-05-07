pub fn merge_prompt_with_context(prompt: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx) if !ctx.trim().is_empty() => {
            format!("{}\n\n---\n\n{}", ctx, prompt)
        }
        _ => prompt.to_string(),
    }
}
