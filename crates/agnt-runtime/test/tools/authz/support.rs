//! Shared fixtures for the tool authorization test modules.
//!
//! Holds the `permissions_toml` fixture every topical authz test file needs.
//! Sub-files pull it in with `use super::*;`.

pub(super) fn permissions_toml() -> toml::Value {
    toml::from_str(
        r#"
"*" = "ask"
read = "allow"
"c5t_get*" = "allow"

[shell.command]
"kubectl delete *" = "deny"
"*" = "ask"
"#,
    )
    .expect("valid toml")
}
