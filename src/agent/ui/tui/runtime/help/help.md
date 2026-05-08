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
- Use it to open Help, Status, or MCPs.
- Press `Esc` to close the palette.

# MCP basics (enabled/disabled/failed + where to toggle)

- Open MCP controls from the command palette: `Ctrl-P` -> MCPs.
- `enabled`: server is available for tool calls.
- `disabled`: configured, but off for this session.
- `failed`: startup/runtime check failed.
- In MCPs panel, use `j`/`k` or arrows, then `Enter` to toggle.
- Toggles are session-only (not written to config).

# Troubleshooting

- If keys feel wrong, press `Esc` once to return to normal mode.
- If the assistant is stuck, press `Esc` twice to request abort.
- If MCP tools are missing, open `Ctrl-P` -> MCPs and check `failed` servers.
- If text looks stale after resize, resize once more or switch focus (`h`/`l`).
