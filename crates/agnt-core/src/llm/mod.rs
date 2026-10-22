mod cost;
mod format;
mod types;

pub use cost::SessionCost;
pub(crate) use cost::{PricedOutcome, PricingSkip, computed_cost};
pub use format::format_response;
pub use types::{LlmResponse, LlmUsage, ToolCallMetadata};

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/llm/computed_cost.rs"]
mod computed_cost_test;

#[cfg(test)]
#[path = "../../test/llm/cost.rs"]
mod cost_test;

#[cfg(test)]
#[path = "../../test/llm/format.rs"]
mod format_test;

// endregion: --- Tests
