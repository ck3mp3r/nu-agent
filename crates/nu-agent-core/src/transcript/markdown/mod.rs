mod code_blocks;
mod projector;
mod render;
mod sanitize;
mod unified;

#[cfg(test)]
mod test;

pub use render::{
    project_code_block_lines, project_diff_lines, project_markdown_to_lines,
    rendered_line_to_plain_text, unwrap_single_fenced_block,
};
pub use unified::render_markdown_lines;
