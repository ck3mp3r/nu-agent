//! Shared fixtures for the doom loop test modules.
//!
//! Holds the imports, the `TestResult` alias, and the action-inspection
//! helpers every topical doom-loop test file needs. Sub-files pull them in
//! with `use super::*;`.

pub(super) use crate::bus::Bus;
pub(super) use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopDetector};
pub(super) use rig::agent::DispatchAction;
pub(super) use rig::error::ErrorKind;

pub(super) type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

use std::sync::{Arc, Mutex};

use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::NuCompactor;
use crate::hook::HookChain;
use crate::hook::agent_hook::HookState;
use crate::hook::chain::HookEnv;
use crate::hook::doom_loop::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::session::{CachedMemory, RigStoreConfig};
use rig::test_utils::MockCompletionModel;

/// A resolver that allows every call, so a dispatch reaches the doom-loop
/// detector instead of stopping at the permission gate. The doom-loop ladder
/// is what the harness-driven test exercises, not the policy.
#[derive(Clone)]
pub(super) struct AllowAllResolver;

impl AsyncPermissionResolver for AllowAllResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _stream: &crate::bus::Stream,
        _facts: &crate::bus::FactsTx,
        _cancel: &crate::bus::CancelTx,
    ) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

/// Build a hook chain over `bus` with the repetition guard on, so a tool call
/// runs the doom-loop ladder. Compaction never runs here, so the compactor
/// carries no model spec.
pub(super) fn doom_loop_chain(bus: &Bus) -> HookChain<AllowAllResolver> {
    HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        AllowAllResolver,
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        None,
        HookState {
            circuit_breaker: Arc::new(Mutex::new(
                crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
            )),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: true,
            memory: Arc::new(CachedMemory::new(RigStoreConfig::in_memory())),
            conversation_id: "doom-loop-conv".to_string(),
            compaction: CompactionConfig {
                compactor: NuCompactor::new(
                    MockCompletionModel::text("summary").erase(),
                    Bus::default(),
                    rig_memory::CompactionPolicy::default(),
                    None,
                ),
                reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
            },
            model: MockCompletionModel::default().erase(),
            meter: crate::hook::AnchorMeter::default(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            store_latch: Default::default(),
            pending_summary: Default::default(),
        },
    )
}

/// Drives the detector through `DOOM_LOOP_THRESHOLD` identical calls, returning
/// every action in order. Call `i < DOOM_LOOP_THRESHOLD - 1` is under threshold;
/// the final call is the first detection.
pub(super) async fn drive_to_first_detection(
    detector: &DoomLoopDetector,
    bus: &Bus,
) -> Vec<Option<DispatchAction>> {
    let mut actions = Vec::new();
    for _ in 0..DOOM_LOOP_THRESHOLD {
        let action = detector
            .check_and_record(
                "read_file",
                "{\"path\": \"same\"}",
                bus.narration(),
                bus.stream(),
                &crate::hook::agent_hook::StopLedger::default(),
            )
            .await;
        actions.push(action);
    }
    actions
}

/// Whether `action` is a skip denial: a `DispatchAction::Deny` whose report is
/// not a cancellation (a cancellation is a stop).
pub(super) fn is_skip(action: &Option<DispatchAction>) -> bool {
    matches!(action, Some(DispatchAction::Deny(report)) if report.kind != ErrorKind::Cancelled)
}

/// The reason of a skip denial, if `action` is one.
pub(super) fn skip_reason(action: &Option<DispatchAction>) -> Option<&str> {
    match action {
        Some(DispatchAction::Deny(report)) if report.kind != ErrorKind::Cancelled => {
            Some(report.message.as_str())
        }
        _ => None,
    }
}

/// The reason of a stop denial, if `action` is one.
pub(super) fn stop_reason(action: &Option<DispatchAction>) -> Option<&str> {
    match action {
        Some(DispatchAction::Deny(report)) if report.kind == ErrorKind::Cancelled => {
            Some(report.message.as_str())
        }
        _ => None,
    }
}
