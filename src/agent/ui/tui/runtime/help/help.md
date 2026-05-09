# Getting started

- Type your request in the input box.
- Press `Enter` to send.
- Press `Ctrl-C` to quit and restore your terminal.

# Modes (insert vs normal)

- `Insert`: keys type in the input.
- `Normal`: keys navigate panes/transcript.
- Press `Esc` to leave insert mode.
- Press `i` to return to insert mode.

# Core keys (with explanations)

- `Enter` sends the prompt.
- `Esc` closes overlays, or leaves insert/visual mode.
- `j` / `k` scroll line-by-line.
- `PgUp` / `PgDn` scroll by page.
- `h` / `l` move pane focus.
- `v` starts visual selection; `y` copies selection.
- `gg` jumps to top; `G` jumps to bottom.

# Command palette (Ctrl-P)

- Press `Ctrl-P` to open the command palette.
- Type to filter, then press `Enter`.
- Use it to open Help, Status, MCPs, or Skills.
- Press `Esc` to close the palette.

# Inline slash suggestions

- Inline slash suggestions open when input starts with `/`.
- Filtering is deterministic and filters incrementally as input grows (`/c`, `/co`, ...).
- Suggestions are non-modal and independent of the command palette popup/table/state.
- If the slash prefix is removed, suggestions close cleanly without falling back to command palette.
- Up/Down selects a suggestion; `Enter` submits the selected slash command.
- Supported commands: `/compact`, `/mcp`, `/help`, `/status`.
- `/help`, `/status`, and `/mcp` map to the same handlers as Ctrl-P actions.
- Unknown slash commands warn deterministically and continue the loop.

# MCP basics (enabled/disabled/failed + where to toggle)

- Open MCP controls from the command palette: `Ctrl-P` -> MCPs.
- `enabled`: server is available for tool calls.
- `disabled`: configured, but off for this session.
- `failed`: startup/runtime check failed.
- In MCPs panel, use `j`/`k` or arrows, then `Enter` to toggle.
- Toggles are session-only (not written to config).

# Slash commands

- `/compact` runs immediate force compaction and bypasses threshold gating.
- `/compact` uses the same compaction execution path as auto-threshold compaction.
- Unknown slash commands show a warning and continue the loop.
- Slash command text is handled by the runtime and is not sent to the LLM.
- Slash command text is not echoed into the transcript; only resulting artifacts (for example compaction summary output) may appear.

# Compaction mode

- Current runtime has a single active compaction mode: `sliding_summary`.
- Legacy serialized values (`truncate`, `sliding`, `summarize`) normalize to `sliding_summary`.
- This single active compaction mode is the only mode presented in docs/help UX.

# Auto compaction behavior

- Auto compaction fires when usage reaches `threshold - tolerance`.
- Default tolerance is `0` unless configured.
- After firing, compaction is disarmed to avoid duplicate triggers near the boundary.
- Rearm occurs only when usage drops to `threshold - (tolerance + hysteresis_margin)`.

# Session memory persistence

- Compaction updates are persisted to the session JSONL file.
- Persisted updates include compacted message set and incremented `compaction_count` metadata.
- Compaction emits a transcript-visible summary artifact including source and summarized/kept counts.

# Troubleshooting

- If keys feel wrong, press `Esc` once to return to normal mode.
- If the assistant is stuck, press `Esc` twice to request abort.
- If MCP tools are missing, open `Ctrl-P` -> MCPs and check `failed` servers.
- If text looks stale after resize, resize once more or switch focus (`h`/`l`).
