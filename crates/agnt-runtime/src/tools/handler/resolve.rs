use agnt_kernel::BuiltinKind;

pub fn is_builtin_tool_name(tool_name: &str) -> bool {
    tool_name.parse::<BuiltinKind>().is_ok()
}

pub fn resolve_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}
