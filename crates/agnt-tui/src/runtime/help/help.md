# Getting started

- Type your request in the input box.
- Press `Enter` to send.
- While a turn is busy, typing still edits the draft and `Enter` queues the prompt.
- Press `Ctrl-C` twice to quit and restore your terminal.

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
- Use it to open Help, Status, MCPs, Skills, or Models.
- Press `Esc` to close the palette.

# Inline slash suggestions

- Inline slash suggestions open when input starts with `/`.
- Filtering is deterministic and filters incrementally as input grows (`/c`, `/co`, ...).
- Suggestions are non-modal and independent of the command palette popup/table/state.
- If the slash prefix is removed, suggestions close cleanly without falling back to command palette.
- Up/Down selects a suggestion; `Enter` submits the selected slash command.
- Supported commands: `/compact`, `/uncompact`, `/mcp`, `/help`, `/stats`, `/status`, `/models`, `/agent`, `/new`, `/session`, `/theme`, `/skills`.
- `/help`, `/status`, and `/mcp` map to the same handlers as Ctrl-P actions.
- `/models` and Ctrl-P `Models` use the same shared model-picker action path.
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
- `/uncompact` restores what the most recent compaction replaced; with nothing to undo it warns and nothing changes.
- `/stats` toggles the render-stats band at the bottom of the screen: a data row of counters plus a legend row decoding them; the same command turns both on and off, and Esc takes no part.
- Unknown slash commands show a warning and continue the loop.
- Slash command text is handled by the runtime and is not sent to the LLM.
- Slash command text is not echoed into the transcript; only resulting artifacts (for example compaction summary output) may appear.

# Model switching

- `/models` opens the inline model picker.
- Ctrl-P `Models` launches the same picker path as `/models`.
- Model switching uses cached startup `PluginConfig` metadata (no per-switch plugin config re-read).
- Applying a model switch updates active identity immediately in the footer/status lane.
- Model switches apply full resolved model config (provider/provider_kind/model/limits/settings) for subsequent turns.

# Modal presentation

- Modal surfaces use a consolidated layout policy by panel kind.
- Help uses larger readable layout; Status uses compact layout; MCP/Skills/Models use balanced defaults.
- Modal frames use rounded borders.
- Modal-open states apply dimmed backdrop treatment for non-modal regions.

# Compaction

Auto compaction fires when the next request would exceed the model's input limit less `reserve_tokens`: 16384 tokens (unset) or the count you set.

Each key is optional; a key outside the three fails the load as unknown, and unset = the built-in default:

- `enabled` — whether a conversation is compacted on its own: near the window, and after a request refused as too long (unset = the built-in default, true).
- `reserve_tokens` — tokens of the model's input limit kept free below the trigger (unset = the built-in default, 16384 tokens).
- `keep_recent_tokens` — about how many tokens of the newest messages stay as they are (unset = the built-in default, 20000 tokens).

Use `/compact` to trigger compaction immediately; it bypasses threshold gating. `/compact` args are reserved for a future focus option.

# Auto compaction behavior

- Auto compaction fires once the next request would exceed the model's input limit less `reserve_tokens`.
- Manual and auto compaction share one execution path.
- The newest messages stay as they are within the keep budget, and the older ones become the summary.
- The splice is written to the session's saved memory, and the next run loads the compacted set.

# Session memory persistence

- Compaction updates are persisted to the session's saved memory.
- The persisted update is the compacted message set; the next run loads it.
- Compaction emits a transcript-visible summary artifact including source and summarized/kept counts.
- The `/uncompact` artifact (`Uncompacted`) carries the source of the command that asked for it — `manual`, the only uncompact source.

# Troubleshooting

- If keys feel wrong, press `Esc` once to return to normal mode.
- If the assistant is stuck, press `Esc` twice to request abort.
- `Ctrl-C` closes the topmost popup, then clears the draft, then asks for a second press to quit.
- If MCP tools are missing, open `Ctrl-P` -> MCPs and check `failed` servers.
- If text looks stale after resize, resize once more or switch focus (`h`/`l`).
