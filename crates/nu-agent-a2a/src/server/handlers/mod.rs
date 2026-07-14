mod agent;
mod files;
mod push;
mod send;
mod send_stream;
mod subscribe;
mod task;

pub use agent::*;
pub use files::*;
pub use push::*;
pub use send::*;
pub use send_stream::*;
pub use subscribe::*;
pub use task::*;

/// Shared SSE result type used by streaming handlers.
pub type SseResult = Result<axum::response::sse::Event, std::convert::Infallible>;
