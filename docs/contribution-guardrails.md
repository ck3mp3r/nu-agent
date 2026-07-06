# Contribution guardrails

This page is for contributors changing tool handling, permission UX, or TUI transcript rendering.

See also:

- [Usage: interactive permission prompt behavior](./usage.md#interactive-permission-prompt-behavior-tui)

## 1) Permission prompt rendering model (inline, non-modal)

- Permission ask UI is an **inline transcript card**, attached after the active tool row.
- It is **not** a global modal and must not activate dimmed modal backdrop behavior.
- Decision controls are rendered in a **sticky footer row** (outside transcript rows):
  - `a allow_once`
  - `A allow_always (session)`
  - `d/Esc deny`

Code + tests:

- Runtime injection/footer: `crates/nu-agent-tui/src/runtime/mod.rs`
- Permission prompt state: `crates/nu-agent-tui/src/state/permissions.rs`
- Tests: `crates/nu-agent-tui/src/runtime/test.rs`, `crates/nu-agent-tui/src/state/test.rs`
  - `permission_prompt_does_not_open_global_dimmed_modal_backdrop`
  - `permission_prompt_open_sets_required_status_and_presence`
  - `permission_prompt_open_scrolls_to_bottom`

## 2) Viewport invariants (R7/R8)

When permission ask is active:

1. **Required-row invariant**: a required permission prompt context line must remain selectable for viewport fitting logic.
2. **Sticky-controls invariant**: controls stay visible via reserved footer row, even with tiny/narrow viewport and heavy wrapping.
3. **Manual-scroll invariant**: user scroll override disables forced recentering jitter but preserves required-row state.
4. **Bounds invariant**: required-line fallback clamps to transcript bounds when anchors drift.

Code + tests:

- Viewport sync: `crates/nu-agent-tui/src/runtime/mod.rs` (`sync_transcript_viewport_lines_with_layout`)
- Scroll state: `crates/nu-agent-tui/src/state/mod.rs`
- Tests: `crates/nu-agent-tui/src/runtime/test.rs`, `crates/nu-agent-tui/src/state/test.rs`
  - `transcript_bottom_detection_uses_effective_viewport_after_input_chrome_and_margins`
  - `main_pane_vertical_split_has_no_overlap_or_bottom_cutoff`
  - `push_transcript_item_follows_tail_when_at_last_item`
  - `push_transcript_item_stays_put_when_scrolled_up`

## 3) ToolSource taxonomy and permission gating

Every tool call is classified into one of five `ToolSource` variants before authorization:

| Variant | Tools | Permission gating |
|---|---|---|
| `Builtin` | `read`, `skill`, `spawn_agent`, `send_message`, `list_agents` | **Bypasses** — agent-coordination and read-only tools are always allowed |
| `BuiltinFs` | `edit`, `patch` | **Gated** — filesystem-mutating tools go through the full permission flow |
| `Closure` | user-defined Nushell tools | **Gated** |
| `Mcp` | MCP server tools | **Gated** |
| `Unknown` | unrecognized tool names | **Gated** |

Security rationale:
- `Builtin` tools are either read-only (`read`, `skill`) or purely agent-coordination (`spawn_agent`, `send_message`, `list_agents`) — they carry no filesystem mutation risk and bypassing permissions keeps overhead low for safe operations.
- `BuiltinFs` tools (`edit`, `patch`) mutate the filesystem and must be subject to the same permission policy as MCP/closure tools. They are not implicitly trusted even though they are built-in.

See `crates/nu-agent-core/src/tools/handler/types.rs` (`ToolSource`) and `crates/nu-agent-core/src/tools/handler/authz_gate.rs` for the enforcement point.

## 4) Tool/authz architecture direction

Keep handler dependencies one-way:

- `dispatch` orchestrates only.
- `authz_gate` owns policy/ask/session-grant resolution.
- `pre_authorize` is side-effect free preview/context generation.
- `builtin_kinds` owns builtin tool name parsing and classification.
- `fs` owns builtin filesystem tool dispatch contracts.
- `result` owns output/failure shaping.
- `types` owns shared data model types.

Source:

- Facade/wiring: `crates/nu-agent-core/src/tools/handler/mod.rs`
- Submodules: `crates/nu-agent-core/src/tools/handler/{dispatch,authz_gate,pre_authorize,builtin_kinds,fs,result,types}.rs`
- Module contract tests: `crates/nu-agent-core/src/tools/handler/authz_gate_test.rs`, `dispatch_test.rs`

## 5) Contributor checklist: adding new tools or permission UX

Before code:

- [ ] Confirm tool source classification and ownership boundary (`dispatch` vs `builtin_kinds`/`fs` vs MCP); see ToolSource taxonomy table (section 3) for correct variant.
- [ ] Confirm permission DSL mapping and precedence impact (`*` -> tool -> nested field rule).
- [ ] Confirm whether pre-authorize preview is required and remains zero-write.

During code:

- [ ] Keep handler dependency direction (no new reverse edges).
- [ ] If TUI permission UI changes, preserve inline-card + sticky-footer model.
- [ ] Preserve required-row + viewport invariants under tiny height/width + wrapped diff previews.
- [ ] Keep non-interactive `ask` fallback semantics deterministic.

Tests/docs before review:

- [ ] Add/adjust focused tests in:
  - `crates/nu-agent-core/src/tools/handler/authz_gate_test.rs`
  - `crates/nu-agent-core/src/tools/handler/dispatch_test.rs`
  - `crates/nu-agent-core/src/protocol/permission_test.rs`
  - `crates/nu-agent-tui/src/runtime/test.rs`
- [ ] Update `docs/usage.md` when user-visible behavior changes.
- [ ] Update this page when introducing/relaxing guardrails.
