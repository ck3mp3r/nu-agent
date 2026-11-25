//! The compaction's summary requests, fitted to the model's input budget.
//!
//! rig-memory's pi builders ([`rig_memory::summary_request`],
//! [`rig_memory::turn_prefix_request`]) serialize the whole slice and leave the
//! fitting to the consumer. Its own test is the normative sequence
//! (rig-memory-0.47.0/src/compaction/tests.rs:295-316):
//! [`rig_memory::serialize_conversation`], then [`rig_memory::fit_to_model`], then
//! [`rig_memory::summarization_request`] with the fitted text as the prompt and
//! the fitter's `most` as the output limit. This module is that sequence for both
//! arms.
//!
//! The framing is pi's, which rig does not export: the prompt is built as pi's
//! builders build it (rig-memory-0.47.0/src/compaction/pi.rs:751-794), over the
//! fitted text instead of the whole serialization. The system line is rig's
//! exported [`rig_memory::SUMMARIZATION_SYSTEM_PROMPT`].

use core::borrow::Borrow;

use rig::catalog::ModelSpec;
use rig::completion::{CompletionRequest, UnsupportedOption};
use rig_memory::CompactionSettings;

use crate::types::Message;

/// The most tokens a compaction summary may write: the budget every summary
/// request asks for. `fit_to_model` holds it to the model's own output limit and
/// to the room the fitted input leaves.
const SUMMARY_TOKENS: u64 = 32_768;

/// The base instruction of a history summary, as pi words it
/// (rig-memory-0.47.0/src/compaction/pi.rs:566).
const SUMMARIZATION_PROMPT: &str = "The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.\n\nUse this EXACT format:\n\n## Goal\n[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned by user]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Current work]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [Ordered list of what should happen next]\n\n## Critical Context\n- [Any data, examples, or references needed to continue]\n- [Or \"(none)\" if not applicable]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// The base instruction of a history summary that updates `previous`, as pi words
/// it (rig-memory-0.47.0/src/compaction/pi.rs:599).
const UPDATE_SUMMARIZATION_PROMPT: &str = "The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.\n\nUpdate the existing structured summary with new information. RULES:\n- PRESERVE all existing information from the previous summary\n- ADD new progress, decisions, and context from the new messages\n- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed\n- UPDATE \"Next Steps\" based on what was accomplished\n- PRESERVE exact file paths, function names, and error messages\n- If something is no longer relevant, you may remove it\n\nUse this EXACT format:\n\n## Goal\n[Preserve existing goals, add new ones if the task expanded]\n\n## Constraints & Preferences\n- [Preserve existing, add new ones discovered]\n\n## Progress\n### Done\n- [x] [Include previously done items AND newly completed items]\n\n### In Progress\n- [ ] [Current work - update based on progress]\n\n### Blocked\n- [Current blockers - remove if resolved]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale] (preserve all previous, add new)\n\n## Next Steps\n1. [Update based on current state]\n\n## Critical Context\n- [Preserve important context, add new if needed]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// The instruction of a split turn's earlier-part summary, as pi words it
/// (rig-memory-0.47.0/src/compaction/pi.rs:617).
const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = "The messages above are earlier context from an ongoing conversation. Later messages are stored separately and do not need to be reconstructed.\n\nCreate a concise checkpoint of the user's request and the progress shown above. This checkpoint will be placed before the later messages so the conversation can continue with the necessary context.\n\n## Original Request\n[What did the user ask for?]\n\n## Progress So Far\n- [Key decisions and work completed in these messages]\n\n## Context Needed to Continue\n- [Information from these messages needed to understand the later work]\n\nOnly summarize information explicitly present above. Do not infer or recreate later messages.";

/// Why a summary request cannot be built.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RequestError {
    /// The model's context window is unknown, so no input budget can be derived
    /// and the fit would send the whole slice.
    #[error("Compaction cannot fit its summary request: model '{0}' has no known context window")]
    UnknownWindow(String),
    /// rig rejected the spec's default options.
    #[error("Compaction request error: {0}")]
    Unsupported(#[from] UnsupportedOption),
}

/// The history summary request for `messages`, updating `previous` when it holds
/// anything: rig's sequence over the fitted serialization.
///
/// # Errors
///
/// When no input budget can be derived for `spec`, or when `spec` does not take
/// its default options.
pub(crate) fn history_request(
    messages: &[impl Borrow<Message>],
    previous: Option<&rig_memory::SummaryState>,
    instructions: &str,
    spec: &ModelSpec,
    settings: CompactionSettings,
) -> Result<CompletionRequest, RequestError> {
    let spec = bounded_spec(spec, settings)?;
    let (fitted, most) = fit(messages, &spec);
    let mut prompt = format!("<conversation>\n{fitted}\n</conversation>\n\n");
    let previous = previous.and_then(rig_memory::SummaryState::previous_summary);
    if let Some(previous) = &previous {
        prompt.push_str(&format!(
            "<previous-summary>\n{previous}\n</previous-summary>\n\n"
        ));
    }
    let base = match previous {
        Some(_) => UPDATE_SUMMARIZATION_PROMPT,
        None => SUMMARIZATION_PROMPT,
    };
    prompt.push_str(&with_focus(base, instructions));
    Ok(rig_memory::summarization_request(
        (rig_memory::SUMMARIZATION_SYSTEM_PROMPT, prompt),
        &spec,
        None,
        most,
    )?)
}

/// The summary request for a split turn's earlier part, `messages`: rig's
/// sequence over the fitted serialization.
///
/// # Errors
///
/// As [`history_request`].
pub(crate) fn turn_prefix_request(
    messages: &[impl Borrow<Message>],
    instructions: &str,
    spec: &ModelSpec,
    settings: CompactionSettings,
) -> Result<CompletionRequest, RequestError> {
    let spec = bounded_spec(spec, settings)?;
    let (fitted, most) = fit(messages, &spec);
    let prompt =
        format!("# Conversation\n{fitted}\n\n# Instructions\n{TURN_PREFIX_SUMMARIZATION_PROMPT}");
    let prompt = with_focus(&prompt, instructions);
    Ok(rig_memory::summarization_request(
        (rig_memory::SUMMARIZATION_SYSTEM_PROMPT, prompt),
        &spec,
        None,
        most,
    )?)
}

/// The most input tokens a summary request may send to `spec`: its context window
/// less the reply reserve and the summary budget, or the window itself when that
/// reserve does not fit inside it.
///
/// The window is the one the planning spec carries. `resolve_model_spec_for`
/// fills it from the config's `limit.context` — `model_context_tokens`, else
/// `max_context_tokens`, which the loader fills from the models-cache row's
/// `limit.context` (`config/resolver.rs`) — else
/// [`defaults::MAX_CONTEXT_TOKENS`](crate::config::defaults::MAX_CONTEXT_TOKENS).
///
/// # Errors
///
/// When the window is unknown: no budget can be derived, and the fit would send
/// the whole slice.
pub(crate) fn effective_input_limit(
    spec: &ModelSpec,
    settings: CompactionSettings,
) -> Result<u32, RequestError> {
    let Some(window) = spec.context_window else {
        return Err(RequestError::UnknownWindow(spec.id.clone()));
    };
    let budget = u64::from(window)
        .saturating_sub(settings.reserve_tokens)
        .saturating_sub(SUMMARY_TOKENS);
    // A window under the reserve has no room to give up: it is its own limit, and
    // rig's fit splits what the window leaves.
    let limit = if budget == 0 {
        window
    } else {
        u32::try_from(budget).unwrap_or(window)
    };
    Ok(limit.max(1))
}

// region:    --- Support

/// `spec` with the derived input limit set when it carries none, so the fit runs
/// its bounded branch and never sends the whole slice.
fn bounded_spec(spec: &ModelSpec, settings: CompactionSettings) -> Result<ModelSpec, RequestError> {
    if spec.max_input_tokens.is_some() {
        return Ok(spec.clone());
    }
    let limit = effective_input_limit(spec, settings)?;
    Ok(spec.clone().with_max_input_tokens(limit))
}

/// rig's sequence over `messages`: the serialization, fitted to `spec`, and the
/// output limit to ask for.
fn fit(messages: &[impl Borrow<Message>], spec: &ModelSpec) -> (String, Option<u64>) {
    let serialized = rig_memory::serialize_conversation(messages);
    let (fitted, most) = rig_memory::fit_to_model(&serialized, spec, SUMMARY_TOKENS);
    (fitted.into_owned(), most)
}

/// `prompt` followed by the user's `instructions`, when there are any.
fn with_focus(prompt: &str, instructions: &str) -> String {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        prompt.to_owned()
    } else {
        format!("{prompt}\n\nAdditional focus: {instructions}")
    }
}

// endregion: --- Support
