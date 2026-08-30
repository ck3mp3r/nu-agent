use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value;

use super::A2aHttpClient;
use crate::A2aError;

/// Map from URL key ("POST {url}" / "GET {url}") to canned response.
type PostResponseMap = HashMap<String, Result<Value, A2aError>>;
/// Map from URL key to canned byte response.
type GetResponseMap = HashMap<String, Result<Vec<u8>, A2aError>>;

/// Mock HTTP client for testing. Returns canned responses.
///
/// Responses are registered by URL with `expect_post` and `expect_get`.
/// The client matches requests by method + URL and returns the pre-registered
/// response.
#[derive(Clone)]
pub struct MockHttpClient {
    post_responses: Arc<RwLock<PostResponseMap>>,
    get_responses: Arc<RwLock<GetResponseMap>>,
}

impl Default for MockHttpClient {
    fn default() -> Self {
        Self {
            post_responses: Arc::new(RwLock::new(HashMap::new())),
            get_responses: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl MockHttpClient {
    /// Register a canned response for a POST request.
    ///
    /// The key is `"POST {url}"`. Returns the old response if one was
    /// already registered for this URL.
    pub fn expect_post(
        &self,
        url: &str,
        response: Result<Value, A2aError>,
    ) -> Option<Result<Value, A2aError>> {
        let key = format!("POST {url}");
        self.post_responses
            .write()
            .ok()
            .and_then(|mut map| map.insert(key, response))
    }

    /// Register a canned response for a GET request.
    ///
    /// The key is `"GET {url}"`. Returns the old response if one was
    /// already registered for this URL.
    pub fn expect_get(
        &self,
        url: &str,
        response: Result<Vec<u8>, A2aError>,
    ) -> Option<Result<Vec<u8>, A2aError>> {
        let key = format!("GET {url}");
        self.get_responses
            .write()
            .ok()
            .and_then(|mut map| map.insert(key, response))
    }

    /// Convenience: register a successful POST response.
    pub fn expect_post_ok(&self, url: &str, value: Value) {
        self.expect_post(url, Ok(value));
    }

    /// Convenience: register a POST error.
    pub fn expect_post_error(&self, url: &str, error: A2aError) {
        self.expect_post(url, Err(error));
    }

    /// Convenience: register a successful GET response.
    pub fn expect_get_ok(&self, url: &str, data: Vec<u8>) {
        self.expect_get(url, Ok(data));
    }

    /// Convenience: register a GET error.
    pub fn expect_get_error(&self, url: &str, error: A2aError) {
        self.expect_get(url, Err(error));
    }
}

impl A2aHttpClient for MockHttpClient {
    async fn post_json(&self, url: &str, _body: Value) -> Result<Value, A2aError> {
        let key = format!("POST {url}");
        let map = self
            .post_responses
            .read()
            .map_err(|_| A2aError::Internal("mock lock poisoned".into()))?;
        map.get(&key).cloned().unwrap_or_else(|| {
            Err(A2aError::Internal(format!(
                "no mock response registered for POST {url}"
            )))
        })
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, A2aError> {
        let key = format!("GET {url}");
        let map = self
            .get_responses
            .read()
            .map_err(|_| A2aError::Internal("mock lock poisoned".into()))?;
        map.get(&key).cloned().unwrap_or_else(|| {
            Err(A2aError::Internal(format!(
                "no mock response registered for GET {url}"
            )))
        })
    }
}
