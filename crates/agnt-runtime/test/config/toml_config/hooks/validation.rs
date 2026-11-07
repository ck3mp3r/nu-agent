//! `[[hooks]]` validation: the entries the loader rejects.

use super::*;

#[test]
fn load_rejects_an_unknown_hook_event() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started", "bogus_event"]
script = "audit.nu"
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an unknown hook event must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("audit"),
            "the message must name the hook: {message}"
        );
        assert!(
            message.contains("bogus_event"),
            "the message must name the invalid event: {message}"
        );
        for name in HookEvent::NAMES {
            assert!(
                message.contains(name),
                "the message must list the valid name {name}: {message}"
            );
        }
        Ok(())
    })
}

#[test]
fn load_rejects_a_hook_with_an_empty_name() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = ""
events = ["turn_started"]
script = "audit.nu"
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an empty hook name must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("name"),
            "the message must name the offending field: {message}"
        );
        Ok(())
    })
}

#[test]
fn load_rejects_a_hook_with_an_empty_script() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started"]
script = ""
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an empty hook script must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("audit"),
            "the message must name the hook: {message}"
        );
        assert!(
            message.contains("script"),
            "the message must name the offending field: {message}"
        );
        Ok(())
    })
}
