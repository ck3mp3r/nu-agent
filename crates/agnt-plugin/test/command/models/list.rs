use super::{AgentModelsList, model_record};
use agnt_core::config::ModelSpec;
use agnt_core::config::models_cache::{ModelCost, ModelLimit};
use nu_plugin::SimplePluginCommand;
use nu_protocol::{Span, SyntaxShape};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_models_list_name_returns_command_name() {
    let command = AgentModelsList;
    assert_eq!(
        SimplePluginCommand::name(&command),
        "agent models list",
        "Command name should be 'agent models list'"
    );
}

#[test]
fn test_models_list_signature_has_provider_flag() -> Result<()> {
    let command = AgentModelsList;
    let sig = SimplePluginCommand::signature(&command);

    assert_eq!(sig.name, "agent models list", "Signature name should match");

    let provider_flag = sig
        .named
        .iter()
        .find(|f| f.long == "provider")
        .ok_or("should have --provider flag")?;

    match provider_flag.arg {
        Some(SyntaxShape::String) => (),
        _ => panic!("--provider flag should accept String argument"),
    }
    Ok(())
}

#[test]
fn test_models_list_model_record_with_cost_sets_price_fields() -> Result<()> {
    // -- Setup & Fixtures
    let spec = model_spec(Some(ModelCost {
        input: 3.0,
        output: 15.0,
        cache_read: None,
        cache_write: None,
    }));

    // -- Exec
    let value = model_record("ollama", "llama3", &spec, Span::unknown());

    // -- Check
    let record = value.as_record()?;
    let input = record.get("input_cost").ok_or("should have input_cost")?;
    let output = record.get("output_cost").ok_or("should have output_cost")?;
    assert_eq!(input.as_float()?, 3.0);
    assert_eq!(output.as_float()?, 15.0);
    Ok(())
}

#[test]
fn test_models_list_model_record_without_cost_sets_null_price_fields() -> Result<()> {
    // -- Setup & Fixtures
    let spec = model_spec(None);

    // -- Exec
    let value = model_record("ollama", "llama3", &spec, Span::unknown());

    // -- Check
    let record = value.as_record()?;
    let input = record.get("input_cost").ok_or("should have input_cost")?;
    let output = record.get("output_cost").ok_or("should have output_cost")?;
    assert!(input.is_nothing(), "input_cost should be null");
    assert!(output.is_nothing(), "output_cost should be null");
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
