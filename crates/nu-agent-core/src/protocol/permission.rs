use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::protocol::event::{
    PermissionDecision, PermissionDecisionSubmission, PermissionRequestContext, UiEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub request_id: String,
    pub context: PermissionRequestContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResolution {
    Decision {
        decision: PermissionDecision,
        matched_rule_identity: String,
    },
    TimedOut,
    ChannelClosed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    DuplicateRequestId,
    AlreadyWaiting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    Ignored { reason: &'static str },
}

#[derive(Debug, Clone)]
pub struct PermissionController {
    state: Arc<Mutex<State>>,
    timeout: Duration,
}

#[derive(Debug)]
struct State {
    active: Option<ActiveRequest>,
    seen_request_ids: HashSet<String>,
}

#[derive(Debug)]
struct ActiveRequest {
    request_id: String,
    matched_rule_identity: String,
    decision_rx: UnboundedReceiver<PermissionDecisionSubmission>,
}

#[derive(Debug)]
pub struct PermissionRequestToken {
    request_id: String,
    matched_rule_identity: String,
    decision_tx: UnboundedSender<PermissionDecisionSubmission>,
}

static ACTIVE_PERMISSION_SUBMISSION_SENDER: Mutex<
    Option<UnboundedSender<PermissionDecisionSubmission>>,
> = Mutex::new(None);

impl PermissionController {
    pub fn new(timeout: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                active: None,
                seen_request_ids: HashSet::new(),
            })),
            timeout,
        }
    }

    pub fn begin_request(
        &self,
        request: PermissionRequest,
    ) -> Result<(PermissionRequestToken, UiEvent), RequestError> {
        let (decision_tx, decision_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = self.state.lock().expect("permission state lock poisoned");
        if state.active.is_some() {
            return Err(RequestError::AlreadyWaiting);
        }
        if state.seen_request_ids.contains(&request.request_id) {
            return Err(RequestError::DuplicateRequestId);
        }

        state.seen_request_ids.insert(request.request_id.clone());
        state.active = Some(ActiveRequest {
            request_id: request.request_id.clone(),
            matched_rule_identity: request.context.matched_rule_identity.clone(),
            decision_rx,
        });

        Ok((
            PermissionRequestToken {
                request_id: request.request_id.clone(),
                matched_rule_identity: request.context.matched_rule_identity.clone(),
                decision_tx,
            },
            UiEvent::PermissionRequested {
                request_id: request.request_id,
                context: request.context,
            },
        ))
    }

    pub async fn await_resolution(
        &self,
        token: &PermissionRequestToken,
    ) -> (PermissionResolution, Vec<UiEvent>) {
        let (request_id, expected_rule, mut rx) = {
            let mut state = self.state.lock().expect("permission state lock poisoned");
            let Some(active) = state.active.take() else {
                return (
                    PermissionResolution::ChannelClosed,
                    vec![UiEvent::PermissionDecisionIgnored {
                        request_id: token.request_id.clone(),
                        reason: "missing_active_request".to_string(),
                    }],
                );
            };
            if active.request_id != token.request_id {
                return (
                    PermissionResolution::ChannelClosed,
                    vec![UiEvent::PermissionDecisionIgnored {
                        request_id: token.request_id.clone(),
                        reason: "stale_or_unknown_request".to_string(),
                    }],
                );
            }
            (
                active.request_id,
                active.matched_rule_identity,
                active.decision_rx,
            )
        };

        let mut events = Vec::new();
        loop {
            match tokio::time::timeout(self.timeout, rx.recv()).await {
                Ok(Some(submitted)) => {
                    if submitted.request_id != request_id {
                        events.push(UiEvent::PermissionDecisionIgnored {
                            request_id: submitted.request_id,
                            reason: "stale_or_unknown_request".to_string(),
                        });
                        continue;
                    }
                    if submitted.matched_rule_identity != expected_rule {
                        events.push(UiEvent::PermissionDecisionIgnored {
                            request_id: submitted.request_id,
                            reason: "rule_identity_mismatch".to_string(),
                        });
                        continue;
                    }

                    events.push(UiEvent::PermissionDecisionSubmitted {
                        request_id,
                        decision: submitted.decision,
                        matched_rule_identity: submitted.matched_rule_identity.clone(),
                    });
                    return (
                        PermissionResolution::Decision {
                            decision: submitted.decision,
                            matched_rule_identity: submitted.matched_rule_identity,
                        },
                        events,
                    );
                }
                Ok(None) => {
                    events.push(UiEvent::PermissionDecisionIgnored {
                        request_id: request_id.clone(),
                        reason: "decision_channel_closed".to_string(),
                    });
                    return (PermissionResolution::ChannelClosed, events);
                }
                Err(_elapsed) => {
                    events.push(UiEvent::PermissionDecisionTimedOut {
                        request_id: request_id.clone(),
                    });
                    return (PermissionResolution::TimedOut, events);
                }
            }
        }
    }
}

impl PermissionRequestToken {
    pub fn request_id(&self) -> &str {
        self.request_id.as_str()
    }

    pub fn matched_rule_identity(&self) -> &str {
        self.matched_rule_identity.as_str()
    }

    pub fn sender_clone(&self) -> UnboundedSender<PermissionDecisionSubmission> {
        self.decision_tx.clone()
    }

    pub fn submit(&self, submission: PermissionDecisionSubmission) -> SubmitOutcome {
        if submission.request_id != self.request_id {
            return SubmitOutcome::Ignored {
                reason: "stale_or_unknown_request",
            };
        }
        if submission.matched_rule_identity != self.matched_rule_identity {
            return SubmitOutcome::Ignored {
                reason: "rule_identity_mismatch",
            };
        }
        if self.decision_tx.send(submission).is_ok() {
            SubmitOutcome::Accepted
        } else {
            SubmitOutcome::Ignored {
                reason: "decision_channel_closed",
            }
        }
    }
}

pub fn install_active_permission_submission_sender(
    sender: Option<UnboundedSender<PermissionDecisionSubmission>>,
) {
    let mut slot = ACTIVE_PERMISSION_SUBMISSION_SENDER
        .lock()
        .expect("permission submission sender lock poisoned");
    *slot = sender;
}

pub fn submit_active_permission_decision(
    request_id: String,
    decision: PermissionDecision,
    matched_rule_identity: String,
) -> SubmitOutcome {
    let sender = {
        let slot = ACTIVE_PERMISSION_SUBMISSION_SENDER
            .lock()
            .expect("permission submission sender lock poisoned");
        slot.clone()
    };
    let Some(sender) = sender else {
        return SubmitOutcome::Ignored {
            reason: "stale_or_unknown_request",
        };
    };

    if sender
        .send(PermissionDecisionSubmission {
            request_id,
            decision,
            matched_rule_identity,
        })
        .is_ok()
    {
        SubmitOutcome::Accepted
    } else {
        SubmitOutcome::Ignored {
            reason: "decision_channel_closed",
        }
    }
}
