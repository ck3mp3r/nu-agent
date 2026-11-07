//! The dispatch gate: the cancel, sub-turn, doom-loop, circuit-breaker, and
//! permission checks a tool call passes before it runs.

use rig::agent::{DispatchAction, DispatchEvent, HookContext};
use rig::wasm_compat::WasmCompatSend;

use agnt_kernel::{SessionRecord, StopCause, ToolCall as StreamToolCall, ToolRecord};

use crate::hook::permission_resolver::{
    AsyncPermissionResolver, PermissionDecision, resolve_tool_source,
};
use crate::protocol::events::{SystemEvent, ToolCall};
use crate::types::ToolName;

use super::HookChain;
use super::not_run::{
    CANCELLED_STOP_REASON, NotRunCall, NotRunOutcome, NotRunStart, emit_not_run_dispatch,
};

impl<P: AsyncPermissionResolver> HookChain<P> {
    /// Steer or refuse the pending tool call, in the documented gate order.
    pub(super) fn gate_dispatch(
        &self,
        _ctx: &HookContext,
        event: DispatchEvent<'_>,
    ) -> impl std::future::Future<Output = DispatchAction> + WasmCompatSend {
        // `on_dispatch` is unified: it fires for tool calls and completions.
        // Only tool calls are steered here; a completion dispatch proceeds
        // without consuming any per-tool state (including the cancel signal).
        let tool_name = event.tool_name().map(str::to_string);
        let args = event.tool_args().map(str::to_string);
        let tool_call_id = event.call_id.map(|id| id.to_string());
        let stop = self.stop.clone();

        // Record the call before any gate runs. A stop that lands mid-batch
        // drops the stream, and this record is what keeps the call once rig's
        // history loses it. The cancel check below keeps its order.
        if let Some(name) = tool_name.as_deref()
            && let Some(call_id) = event.call_id
            && let Ok(name) = ToolName::new(name)
        {
            self.turn_tools
                .record_call(call_id.clone(), name, args.clone().unwrap_or_default());
        }

        // Pre-compute all synchronous checks and capture values for the async
        // block. Each is guarded on `tool_name` so a completion dispatch
        // neither consumes the cancel event nor advances the sub-turn cap.
        let cancelled = tool_name.is_some() && self.is_cancelled();
        let subturn_action = tool_name
            .as_deref()
            .and_then(|name| self.subturn.check_and_increment(name));
        let circuit_action = tool_name
            .as_deref()
            .and_then(|name| self.circuit.check_server_enabled(name, &self.mcp_registry));
        let source = tool_name
            .as_deref()
            .map(|name| resolve_tool_source(name, &self.closure_registry, &self.mcp_registry))
            .unwrap_or(crate::tools::handler::ToolSource::Unknown);

        let permission = self.permission.clone();
        let bus = self.bus.clone();

        async move {
            let Some(tool_name) = tool_name else {
                return DispatchAction::proceed();
            };
            let args = args.unwrap_or_default();

            log::trace!("on_dispatch: tool={tool_name}");

            if cancelled {
                // The stop lands before the `ToolStarted` send below and rig
                // fails the dispatch before `on_outcome`, so this pair is the
                // only host event the denied call produces.
                self.emit_cancelled_dispatch(&tool_name, source, &args)
                    .await;
                stop.narrate(
                    bus.stream(),
                    StopCause::UserCancelled,
                    CANCELLED_STOP_REASON,
                );
                return DispatchAction::stop(CANCELLED_STOP_REASON);
            }
            if let Some(action) = subturn_action {
                return action;
            }
            let doom_action = if self.repetition_guard {
                self.doom
                    .check_and_record(&tool_name, &args, bus.ui_event(), bus.stream(), &stop)
                    .await
            } else {
                None
            };
            if let Some(action) = doom_action {
                return action;
            }
            if let Some(action) = circuit_action {
                return action;
            }

            // The call line comes from the render registry. A tool without a
            // registered render fn — a dynamic or MCP tool, or a builtin whose
            // fn was never registered — takes the documented fallback
            // [`call_line_render_for`](crate::tools::handler::builtin_tool::call_line_render_for):
            // the builtin's tailored render when the name parses as a builtin
            // kind, otherwise the generic JSON summary
            // [`CallLine::from_json_summary`](crate::protocol::tool_args::CallLine::from_json_summary)
            // of the raw arguments. The fallback is display-only: the TUI
            // matches the pending call by `ToolCallId`, never by this text.
            let call = ToolCall::new(tool_name.clone(), source, args.clone());
            let _ = bus.ui_event().send(SystemEvent::ToolStarted { call }).await;
            // The record stream opens the pair with the same call as data:
            // identity, name, source, arguments — never the rendered line.
            let _ = bus
                .stream()
                .publish(SessionRecord::Tool(ToolRecord::Started {
                    call: StreamToolCall::new(tool_name.clone(), source, args.clone()),
                }));

            let decision = permission
                .resolve(
                    &tool_name,
                    &args,
                    tool_call_id,
                    bus.ui_event(),
                    bus.cancel(),
                )
                .await;
            match decision {
                PermissionDecision::Allow => DispatchAction::proceed(),
                PermissionDecision::Deny { reason } => {
                    // The tool never runs, but rig still reports the skipped
                    // result through on_outcome, whose tool-result arm would
                    // consume the flag via suppress_previewed_display. The
                    // early take keeps the discard independent of rig's
                    // skipped-result outcome behavior.
                    //
                    // A `false` answer means no preview reached the user: the
                    // policy path denies without a card, so the pair publishes
                    // the argument-sourced preview itself — the same event the
                    // flush path emits, and the same display rehydration
                    // builds for the not-run row. An Ask-path denial already
                    // anchored its preview in the permission card, so `true`
                    // suppresses the emission and no duplicate appears.
                    let preview = !permission.take_previewed(&tool_name, &args);
                    emit_not_run_dispatch(
                        bus.ui_event(),
                        bus.stream(),
                        &self.cwd,
                        NotRunCall {
                            tool_name: tool_name.as_str(),
                            source,
                            arguments: args.as_str(),
                            start: NotRunStart::AlreadyPublished,
                        },
                        NotRunOutcome::Denied { reason: &reason },
                        preview,
                    )
                    .await;
                    DispatchAction::skip(reason)
                }
            }
        }
    }
}
