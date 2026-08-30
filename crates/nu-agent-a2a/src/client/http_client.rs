use serde_json::Value;

use crate::A2aError;

/// HTTP client abstraction for A2A operations.
///
/// Only two primitive operations — all typed A2A methods are free functions
/// in [`super::functions`] that use this trait.
pub trait A2aHttpClient: Clone + Send + Sync + 'static {
    /// POST JSON body, return parsed JSON response.
    ///
    /// Handles HTTP-level errors (connection refused, timeout, non-2xx
    /// status) and maps 404 to [`A2aError::TaskNotFound`].
    fn post_json(
        &self,
        url: &str,
        body: Value,
    ) -> impl std::future::Future<Output = Result<Value, A2aError>> + Send;

    /// GET URL, return raw bytes.
    ///
    /// Handles HTTP-level errors (connection refused, timeout, non-2xx
    /// status) and maps 404 to [`A2aError::TaskNotFound`].
    fn get_bytes(
        &self,
        url: &str,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, A2aError>> + Send;
}
