//! Parsing of the per-model `pricing` keys (pricing ladder rung 4).

use super::*;

// region:    --- per-model pricing (rung 4)

#[test]
fn load_parses_per_model_pricing() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[providers.ollama]

[providers.ollama.models."llama3.3".pricing]
input = 0.0
output = 0.0
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        let ollama = config.providers.get("ollama").ok_or("ollama provider")?;
        let llama = ollama.models.get("llama3.3").ok_or("llama3.3 model")?;
        let pricing = llama.pricing.as_ref().ok_or("pricing must parse")?;
        assert_eq!(pricing.input, 0.0);
        assert_eq!(pricing.output, 0.0);
        assert_eq!(pricing.cache_read, None);
        assert_eq!(pricing.cache_write, None);
        Ok(())
    })
}

#[test]
fn load_parses_per_model_pricing_cache_rates() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[providers.corp]

[providers.corp.models.big.pricing]
input = 1.5
output = 6.0
cache_read = 0.15
cache_write = 1.875
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        let corp = config.providers.get("corp").ok_or("corp provider")?;
        let big = corp.models.get("big").ok_or("big model")?;
        let pricing = big.pricing.as_ref().ok_or("pricing must parse")?;
        assert_eq!(pricing.input, 1.5);
        assert_eq!(pricing.output, 6.0);
        assert_eq!(pricing.cache_read, Some(0.15));
        assert_eq!(pricing.cache_write, Some(1.875));
        Ok(())
    })
}

#[test]
fn load_rejects_a_pricing_without_output() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[providers.ollama]

[providers.ollama.models."llama3.3".pricing]
input = 1.0
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("a pricing without output must not parse".into()),
            Err(e) => e,
        };
        assert!(
            matches!(err, TomlConfigError::Parse(_)),
            "expected a parse error, got {err:?}"
        );
        Ok(())
    })
}

#[test]
fn load_leaves_pricing_unset_when_the_key_is_absent() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[providers.ollama]

[providers.ollama.models."llama3.3"]
temperature = 0.2
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        let ollama = config.providers.get("ollama").ok_or("ollama provider")?;
        let llama = ollama.models.get("llama3.3").ok_or("llama3.3 model")?;
        assert_eq!(llama.pricing, None);
        Ok(())
    })
}

// endregion: --- per-model pricing (rung 4)
