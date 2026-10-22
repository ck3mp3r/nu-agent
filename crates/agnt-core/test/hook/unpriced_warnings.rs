use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The first report of a model is the one that logs; a repeat is silent.
#[test]
fn test_unpriced_warnings_first_time_only_for_the_first_report() -> Result<()> {
    // -- Setup & Fixtures
    let warnings = UnpricedWarnings::default();

    // -- Exec & Check
    assert!(
        warnings.first_time("deepseek-v4.1-flash"),
        "the first report of a model must log"
    );
    assert!(
        !warnings.first_time("deepseek-v4.1-flash"),
        "a repeat report of the same model must stay silent"
    );
    Ok(())
}

/// Each model gets its own first report.
#[test]
fn test_unpriced_warnings_first_time_per_model() -> Result<()> {
    // -- Setup & Fixtures
    let warnings = UnpricedWarnings::default();

    // -- Exec & Check
    assert!(warnings.first_time("llama3.3"));
    assert!(
        warnings.first_time("gemma4:31b"),
        "a second model must get its own first report"
    );
    assert!(!warnings.first_time("llama3.3"));
    Ok(())
}

/// Clones share the set, so a cloned hook does not re-report a model.
#[test]
fn test_unpriced_warnings_clones_share_the_set() -> Result<()> {
    // -- Setup & Fixtures
    let warnings = UnpricedWarnings::default();
    let clone = warnings.clone();

    // -- Exec & Check
    assert!(warnings.first_time("llama3.3"));
    assert!(
        !clone.first_time("llama3.3"),
        "a clone must see the report the original recorded"
    );
    Ok(())
}
