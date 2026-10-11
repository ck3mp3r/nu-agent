/// Shared SSE result type used by streaming handlers.
pub type SseResult = Result<axum::response::sse::Event, std::convert::Infallible>;
