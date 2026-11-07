use agnt_kernel::ToolSource;

/// The text this layer writes wherever it must emit a source as text.
pub(crate) trait ToolSourceText {
    fn text(self) -> &'static str;
}

impl ToolSourceText for ToolSource {
    fn text(self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
            Self::Builtin => "builtin",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
#[path = "../../../test/tools/handler/source_text.rs"]
mod source_text_test;
