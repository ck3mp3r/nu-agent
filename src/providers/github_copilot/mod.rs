//! GitHub Copilot provider for rig.rs
//!
//! This provider implements the OpenAI-compatible GitHub Copilot API
//! with GitHub-specific authentication and headers.
//!
//! # Features
//!
//! - Full rig.rs integration (agents, tools, streaming)
//! - GitHub-specific headers required for API compatibility
//! - Support for multiple models (GPT-4o, Claude Sonnet, O1, etc.)
//! - Environment variable configuration (GITHUB_TOKEN)
//!
//! # Example
//!
//! ```no_run
//! use nu_plugin_agent::providers::github_copilot::{self, Client};
//! use rig::client::CompletionClient;
//! use rig::completion::Prompt;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create client with API key
//! let client = github_copilot::Client::builder()
//!     .api_key("your-github-token")
//!     .build()?;
//!
//! // Create agent
//! let agent = client.agent(github_copilot::GPT_4O).build();
//!
//! // Send prompt
//! let response = agent.prompt("Hello!").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Configuration
//!
//! The provider can be configured via:
//! - Direct API key: `Client::builder().api_key(token)`
//! - Environment variable: `GITHUB_TOKEN`
//! - Custom base URL: `Client::builder().base_url(url)` (for testing)
//!
//! # Architecture contract
//!
//! One-time selection in `model::factory` chooses a concrete provider implementation.
//! After selection, provider behavior is fully encapsulated in that concrete type:
//! endpoint, intent header, request mapping, response mapping, error mapping,
//! and execute transport logic.
//!
//! No shared endpoint helper APIs and no runtime endpoint switch executor.

pub mod completion;
pub mod core;
pub mod model;
pub mod providers;

pub use core::client::{
    Client, ClientBuilder, ClientExt, GitHubCopilotExt, GitHubCopilotExtBuilder,
};
pub use core::error::Error;
pub use model::factory::Agent;

/// Claude Sonnet 4.5 model identifier
pub const CLAUDE_SONNET_4_5: &str = "claude-sonnet-4.5";

/// GPT-4o model identifier (latest GPT-4 optimized)
pub const GPT_4O: &str = "gpt-4o";

/// GPT-4o-mini model identifier (faster, cheaper GPT-4)
pub const GPT_4O_MINI: &str = "gpt-4o-mini";

/// O1 Preview model identifier (reasoning model)
pub const O1_PREVIEW: &str = "o1-preview";

/// O1 Mini model identifier (compact reasoning model)
pub const O1_MINI: &str = "o1-mini";
