use rig::completion::request::{CompletionError, CompletionRequest as CoreCompletionRequest};
use rig::http_client::HttpClientExt;

use crate::providers::github_copilot::GitHubCopilotExt;

pub type CopilotResponse = rig::providers::openai::completion::CompletionResponse;
pub type CopilotCompletion = rig::completion::CompletionResponse<CopilotResponse>;

pub trait GitHubCopilotProvider: Default + Clone + Copy + Send + Sync {
    const NAME: &'static str;
    const ENDPOINT_PATH: &'static str;
    const INTENT_HEADER: &'static str;

    fn map_request(
        model: &str,
        completion_request: CoreCompletionRequest,
    ) -> Result<Vec<u8>, CompletionError>;

    fn map_response(text: &str) -> Result<CopilotResponse, CompletionError>;

    fn map_error(status: reqwest::StatusCode, text: &str) -> CompletionError;

    fn execute<'a, H>(
        client: &'a rig::client::Client<GitHubCopilotExt, H>,
        model: &'a str,
        completion_request: CoreCompletionRequest,
    ) -> impl std::future::Future<Output = Result<CopilotCompletion, CompletionError>> + Send + 'a
    where
        H: HttpClientExt
            + Default
            + std::fmt::Debug
            + Clone
            + rig::wasm_compat::WasmCompatSend
            + rig::wasm_compat::WasmCompatSync
            + 'static;
}
