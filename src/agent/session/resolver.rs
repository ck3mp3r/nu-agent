use nu_protocol::LabeledError;

use crate::agent::protocol::contracts::UiMessageSnapshot;
use crate::session::{Session, SessionStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionRequest {
    None,
    Attach(String),
    Create(String),
}

#[derive(Debug, Clone)]
pub(crate) struct SessionResolutionInput {
    pub use_tui: bool,
    pub input_is_nothing: bool,
    pub session_id: Option<String>,
    pub new_session: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionResolution {
    pub final_session_id: Option<String>,
    pub session: Option<Session>,
    pub tui_should_hydrate_transcript: bool,
    pub tui_initial_messages: Vec<UiMessageSnapshot>,
}

pub(crate) trait SessionResolver {
    fn resolve(&self, input: SessionResolutionInput) -> Result<SessionResolution, LabeledError>;
}

pub(crate) struct DefaultSessionResolver<'a> {
    store: &'a SessionStore,
}

impl<'a> DefaultSessionResolver<'a> {
    pub fn new(store: &'a SessionStore) -> Self {
        Self { store }
    }
}

impl SessionResolver for DefaultSessionResolver<'_> {
    fn resolve(&self, input: SessionResolutionInput) -> Result<SessionResolution, LabeledError> {
        let request = resolve_session_request(input.use_tui, input.session_id, input.new_session);
        match request {
            SessionRequest::None => Ok(SessionResolution {
                final_session_id: None,
                session: None,
                tui_should_hydrate_transcript: false,
                tui_initial_messages: Vec::new(),
            }),
            SessionRequest::Attach(id) => {
                let (session, tui_hydration_messages) = if input.use_tui {
                    let (session, existed_before_attach) =
                        load_or_create_tui_session(self.store, &id)?;
                    let messages = if input.input_is_nothing && existed_before_attach {
                        session
                            .messages()
                            .iter()
                            .map(|message| {
                                UiMessageSnapshot::new(message.role(), message.content())
                                    .with_tool_details(
                                        message.tool_arguments().map(ToOwned::to_owned),
                                        message.tool_result().map(ToOwned::to_owned),
                                        message.tool_success(),
                                    )
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    (session, messages)
                } else {
                    let session = self.store.get_or_create(Some(id.clone())).map_err(|e| {
                        LabeledError::new(format!("Failed to load/create session: {e}"))
                    })?;

                    (session, Vec::new())
                };

                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    tui_should_hydrate_transcript: !tui_hydration_messages.is_empty(),
                    tui_initial_messages: tui_hydration_messages,
                })
            }
            SessionRequest::Create(id) => {
                let _ = (input.use_tui, input.input_is_nothing);
                let session = self.store.get_or_create(Some(id.clone())).map_err(|e| {
                    LabeledError::new(format!("Failed to load/create session: {e}"))
                })?;
                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    tui_should_hydrate_transcript: false,
                    tui_initial_messages: Vec::new(),
                })
            }
        }
    }
}

pub(crate) fn generate_session_id() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!(
        "session-{}-{}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_micros()
    )
}

pub(crate) fn resolve_session_request(
    use_tui: bool,
    session_id: Option<String>,
    new_session: bool,
) -> SessionRequest {
    match (use_tui, session_id, new_session) {
        // TUI explicit session policy is centralized here:
        // always attempt to attach first (resolver handles load-then-create fallback).
        (true, Some(id), _) => SessionRequest::Attach(id),
        (_, Some(id), _) => SessionRequest::Create(id),
        (true, None, _) => SessionRequest::Create(generate_session_id()),
        (false, None, true) => SessionRequest::Create(generate_session_id()),
        (false, None, false) => SessionRequest::None,
    }
}

fn load_or_create_tui_session(
    store: &SessionStore,
    session_id: &str,
) -> Result<(Session, bool), LabeledError> {
    match store.load_session(session_id) {
        Ok(session) => Ok((session, true)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => store
            .get_or_create(Some(session_id.to_string()))
            .map_err(|create_err| {
                LabeledError::new(format!(
                    "Failed to create missing session '{session_id}': {create_err}"
                ))
            })
            .map(|session| (session, false)),
        Err(err) => Err(LabeledError::new(format!(
            "Failed to attach session '{session_id}': {err}"
        ))),
    }
}
