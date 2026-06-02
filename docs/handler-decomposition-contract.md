# Handler Decomposition Contract (R1)

Status: architecture contract + migration plan only (no code moves in this task).

Scope: decomposition of `src/agent/tools/handler/mod.rs` to unblock R2–R6 and verification in R10.

## 1) Target module map

`src/agent/tools/handler/` becomes a directory module with this layout:

- `mod.rs` — wiring facade only (exports + `handle_tool_calls` entrypoint wiring)
- `dispatch.rs` — central routing/lifecycle orchestration for one tool call
- `authz_gate.rs` — policy evaluation + ask/session-grant orchestration
- `pre_authorize.rs` — read-only preview/context generation before authz prompt
- `builtin_fs.rs` — builtin `read`/`edit`/`patch`/`skill` behavior and contracts
- `result.rs` — result/failure/display shaping and serialization
- `types.rs` — shared handler structs/enums used across modules
- `test.rs` — handler module tests (kept as module-level contract tests)

## 2) Responsibility matrix

| Module | Owns | Must NOT own |
|---|---|---|
| `dispatch.rs` | `handle_single_tool_call` orchestration order: classify -> pre-authorize -> authz gate -> execute -> shape result | JSON payload shaping details, policy internals, fs edit planning internals |
| `authz_gate.rs` | `permissions.evaluate`, ask-hook flow, session grant override/application, deny decision output model — handles all `ToolSource` variants except `Builtin` (which bypasses entirely); `BuiltinFs` tools (`edit`, `patch`) go through the full flow here | Tool execution, direct filesystem mutation, MCP invocation, serialization formatting |
| `pre_authorize.rs` | Read-only preflight context generation (currently edit preview + ask context), zero-write guarantee | Permission decisions, write/apply operations, final result assembly |
| `builtin_fs.rs` | Builtin fs/tool argument models, parse/validate/dispatch for `read`/`edit`/`patch`/`skill`, deterministic edit contract responses | Permission policy decisions, MCP/closure dispatch routing, global result envelope shaping |
| `result.rs` | `ToolCallResult`/failure payload construction, display extraction/attachment, error-class mapping used for result shaping | Tool execution side effects, policy ask flow, fs planning logic |
| `types.rs` | Shared enums/structs (`ToolSource`, `ToolErrorKind`, context structs, pre-auth output carriers) | Business logic/orchestration/IO |
| `mod.rs` | Re-exports, module composition, minimal facade-level docs | Large implementation blocks, helper logic duplication |

## 3) Oversized single-file candidates (>=400 LOC) and decisions

Candidate discovery command used:

`ls src/agent/tools/**/*.rs | each {|f| {path: $f.name, lines: ((open $f.name | lines | length))} } | where lines >= 400`

Scope for this plan is `src/agent/tools/**` (aligned to R1–R6/R10).

| File | LOC | Decision | Rationale |
|---|---:|---|---|
| `src/agent/tools/handler/mod.rs` | 1706 | **Convert to directory module** | Primary refactor target; current file mixes dispatch/authz/pre-auth/fs/result/types concerns. |
| `src/agent/tools/handler/test.rs` | 1588 | **Keep single-file for now** | Already in required multi-file-module test location (`handler/test.rs`). In R10, split internally by `mod dispatch`, `mod authz_gate`, etc. if readability drops. |
| `src/agent/tools/authz.rs` | 617 | **Keep single-file for now** | Not part of current decomposition scope; provides stable policy DSL core consumed by `authz_gate.rs`. Revisit only if growth continues after R4. |
| `src/agent/tools/authz_test.rs` | 480 | **Keep single-file for now** | Contract-heavy test file; no immediate architectural coupling risk to handler split. |

## 4) Public interface contract + dependency direction rules

### Public surface (from `handler/mod.rs`)

`mod.rs` remains the public boundary and re-exports required symbols for existing call sites:

- Types: `ToolSource`, `ToolErrorKind`, `ToolFailureOutcome`, `ToolCallResult`, `McpToolRegistry`, `ToolAuthorizationContext`, `ToolHandlerContext`, `PreAuthorizeOutput`
- Functions: `handle_tool_calls`, `llm_visible_tool_definitions`, `is_builtin_fs_tool_name`, `json_to_nu_value`, `nu_value_to_json`

Rule: downstream modules outside `handler/` import from `crate::agent::tools::handler` (facade), not from private submodules.

### Dependency direction (strict)

- `types.rs` has no dependency on other handler modules.
- `result.rs` depends only on `types.rs` (+ protocol display types).
- `builtin_fs.rs` depends on `types.rs` (+ fs core/diff/protocol skill resolver), not on `dispatch.rs`.
- `pre_authorize.rs` depends on `types.rs` and `builtin_fs.rs` read-only helpers.
- `authz_gate.rs` depends on `types.rs` + `agent::tools::authz` primitives.
- `dispatch.rs` is the only module allowed to depend on all other handler submodules.
- `mod.rs` only declares modules + re-exports.

Forbidden edges:

- `authz_gate.rs` -> `dispatch.rs`
- `result.rs` -> `dispatch.rs`
- `builtin_fs.rs` -> `authz_gate.rs`
- any module -> `mod.rs`

## Migration plan mapped to R2–R6 and R10

1. **R2**: extract result/failure/display shaping to `result.rs` (no behavior change).
2. **R3**: extract pre-authorization preview/context generation to `pre_authorize.rs` (must remain side-effect free).
3. **R4**: extract ask/allow/deny orchestration to `authz_gate.rs` using existing `authz` primitives.
4. **R5**: extract builtin fs parse/dispatch + edit contract flows to `builtin_fs.rs`.
5. **R6**: shrink `mod.rs` to wiring facade + exports; keep `dispatch.rs` as orchestration owner.
6. **R10**: verify boundaries, behavior parity, and readability; check oversized-file decisions remain valid.

## Test mapping expectations

- Existing `handler/test.rs` remains the module-level contract suite through the split.
- R2–R5 add targeted test blocks keyed by module responsibility (result/pre_authorize/authz_gate/builtin_fs).
- R10 validates that tests still assert orchestration order and policy safety invariants.

## R7/R8 implementation notes (applies to handler + UI boundary)

R7/R8 introduced runtime/UI guardrails that rely on this decomposition:

- `pre_authorize.rs` must produce deterministic, side-effect-free preview/context suitable for inline TUI permission cards.
- `authz_gate.rs` must preserve rule-identity-driven ask/session-grant semantics (`allow_once`, `allow_always`, `deny`) and deterministic deny-on-timeout behavior.
- `dispatch.rs` must continue orchestration order as classify -> pre-authorize -> authz gate -> execute -> result shaping.

Reference implementation/tests:

- Handler contract coverage: `src/agent/tools/handler/test.rs`
- Permission controller stale/timeout handling: `src/agent/protocol/permission_test.rs`
- TUI inline prompt + viewport invariants: `src/agent/ui/tui/runtime/mod.rs`, `src/agent/ui/tui/runtime/transcript_window.rs`, `src/agent/ui/tui/runtime/test.rs`

Contributor checklist for these guardrails lives in:

- [Contribution guardrails (R9)](./contribution-guardrails.md)

## Non-goals for this task

- No production code movement.
- No behavior changes to tool execution/authz semantics.
- No public API contract change outside documentation.
