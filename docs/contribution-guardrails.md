# Contribution guardrails (R9)

This page is for contributors changing tool handling, permission UX, or TUI transcript rendering.

See also:

- [Handler decomposition contract](./handler-decomposition-contract.md)
- [Usage: interactive permission prompt behavior](./usage.md#interactive-permission-prompt-behavior-tui)

## 1) Permission prompt rendering model (inline, non-modal)

- Permission ask UI is an **inline transcript card**, attached after the active tool row.
- It is **not** a global modal and must not activate dimmed modal backdrop behavior.
- Decision controls are rendered in a **sticky footer row** (outside transcript rows):
  - `a allow_once`
  - `A allow_always (session)`
  - `d/Esc deny`

Code + tests:

- Runtime injection/footer: `src/agent/ui/tui/runtime/mod.rs`
- Inline/non-modal + controls coverage: `src/agent/ui/tui/runtime/test.rs`
  - `permission_prompt_is_injected_inline_after_tool_row_without_modal_overlay`
  - `permission_prompt_does_not_open_global_dimmed_modal_backdrop`
  - `permission_prompt_card_lines_are_compact_and_keybinding_complete`

## 2) Viewport invariants (R7/R8)

When permission ask is active:

1. **Required-row invariant**: a required permission prompt context line must remain selectable for viewport fitting logic.
2. **Sticky-controls invariant**: controls stay visible via reserved footer row, even with tiny/narrow viewport and heavy wrapping.
3. **Manual-scroll invariant**: user scroll override disables forced recentering jitter but preserves required-row state.
4. **Bounds invariant**: required-line fallback clamps to transcript bounds when anchors drift.

Code + tests:

- Window fitting: `src/agent/ui/tui/runtime/transcript_window.rs`
- Prompt recenter/preserve state: `src/agent/ui/tui/state/mod.rs`
- Guardrail tests: `src/agent/ui/tui/runtime/test.rs`
  - `required_permission_prompt_row_is_preserved_when_preview_pushes_controls_out_of_window`
  - `permission_controls_stay_visible_with_large_diff_and_tiny_viewport`
  - `permission_controls_stay_visible_with_narrow_width_and_heavy_wrapping`
  - `manual_scroll_override_prevents_repeated_prompt_recentering_jitter`
  - `required_permission_line_fallback_clamps_to_transcript_bounds_when_anchor_resolution_drifts`
  - `sticky_permission_footer_row_is_reserved_from_transcript_pane_height`

## 3) Tool/authz architecture direction

Keep handler dependencies one-way:

- `dispatch` orchestrates only.
- `authz_gate` owns policy/ask/session-grant resolution.
- `pre_authorize` is side-effect free preview/context generation.
- `builtin_fs` owns builtin tool parsing/dispatch contracts.
- `result` owns output/failure shaping.
- `types` owns shared data model types.

Source + contract:

- Facade/wiring exports: `src/agent/tools/handler/mod.rs`
- Submodules: `src/agent/tools/handler/{dispatch,authz_gate,pre_authorize,builtin_fs,result,types}.rs`
- Contract + forbidden edges: [handler-decomposition-contract.md](./handler-decomposition-contract.md)
- Module contract tests: `src/agent/tools/handler/test.rs`

## 4) Contributor checklist: adding new tools or permission UX

Before code:

- [ ] Confirm tool source classification and ownership boundary (`dispatch` vs `builtin_fs` vs MCP).
- [ ] Confirm permission DSL mapping and precedence impact (`*` -> tool -> nested field rule).
- [ ] Confirm whether pre-authorize preview is required and remains zero-write.

During code:

- [ ] Keep handler dependency direction (no new reverse edges).
- [ ] If TUI permission UI changes, preserve inline-card + sticky-footer model.
- [ ] Preserve required-row + viewport invariants under tiny height/width + wrapped diff previews.
- [ ] Keep non-interactive `ask` fallback semantics deterministic.

Tests/docs before review:

- [ ] Add/adjust focused tests in:
  - `src/agent/tools/handler/test.rs`
  - `src/agent/protocol/permission_test.rs`
  - `src/agent/ui/tui/runtime/test.rs`
- [ ] Update `docs/usage.md` when user-visible behavior changes.
- [ ] Update this page when introducing/relaxing guardrails.
