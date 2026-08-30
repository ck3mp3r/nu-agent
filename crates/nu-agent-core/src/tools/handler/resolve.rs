use nu_plugin::EngineInterface;

use super::{ToolErrorKind, ToolHandlerError, builtin_kinds};

pub fn is_builtin_tool_name(tool_name: &str) -> bool {
    tool_name.parse::<builtin_kinds::BuiltinKind>().is_ok()
}

pub(crate) fn resolve_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

pub(crate) fn resolve_fs_path(
    path: &str,
    engine: &EngineInterface,
) -> Result<std::path::PathBuf, ToolHandlerError> {
    let cwd = engine.get_current_dir().map_err(|e| ToolHandlerError {
        kind: ToolErrorKind::Runtime,
        message: format!("Unable to resolve current working directory: {e}"),
        details: None,
    })?;
    Ok(resolve_fs_path_for_cwd(path, std::path::Path::new(&cwd)))
}
