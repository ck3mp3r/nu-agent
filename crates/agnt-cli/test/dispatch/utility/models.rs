use super::*;

use agnt_core::config::models_cache::{ModelCost, ModelLimit};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_utility_models_cost_cell_with_cost_formats_both_prices() -> Result<()> {
    // -- Setup & Fixtures
    let spec = model_spec(Some(ModelCost {
        input: 0.5,
        output: 2.0,
        cache_read: None,
        cache_write: None,
    }));

    // -- Exec
    let cell = cost_cell(&spec);

    // -- Check
    assert_eq!(cell, "$0.50/$2.00");
    Ok(())
}

#[test]
fn test_utility_models_cost_cell_without_cost_renders_dash() -> Result<()> {
    // -- Setup & Fixtures
    let spec = model_spec(None);

    // -- Exec
    let cell = cost_cell(&spec);

    // -- Check
    assert_eq!(cell, "-");
    Ok(())
}

// -- Test Support

fn model_spec(cost: Option<ModelCost>) -> ModelSpec {
    ModelSpec {
        id: "llama3".to_string(),
        name: "Llama 3".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 8192,
            output: 2048,
        },
        cost,
        modalities: None,
    }
}
