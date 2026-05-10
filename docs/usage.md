# Usage

## Basic

```nu
"What is Rust?" | agent
```

Input can be:

- plain string
- record with `prompt` (and optional `context`)

```nu
{ prompt: "Summarize", context: "short bullets" } | agent
```

## Common flags

```nu
# Override model for one call
"quick answer" | agent --model "ollama/gemma4:31b"

# Use configured small model
"quick answer" | agent --small

# Quiet mode (suppress non-essential progress UX)
"debug this" | agent --quiet

# Progressive verbosity on stderr UX
"debug this" | agent -v
"debug this" | agent -vv
"debug this" | agent -vvv
```

## Tools

```nu
# Closure tools
let tools = {
  now: {|| date now | format date "%Y-%m-%d %H:%M:%S" }
}
"what time is it" | agent --tools $tools
```

## Built-in filesystem tools (CAS-safe)

The agent exposes exactly three built-in filesystem tools:

- `read`
- `edit`
- `patch`

These names are unprefixed and exact. There are no builtin aliases like
`fs__read` or `tool__edit`.

### Contracts

- `read` is non-mutating and returns file content plus metadata, including
  `version` (content hash token).
- `edit` and `patch` are mutating operations and **require**
  `expected_version`.
- `expected_version` is compared against the current file version (CAS guard)
  to prevent blind overwrites.

### `read` example

```json
{
  "tool": "read",
  "arguments": {
    "path": "src/lib.rs",
    "offset": 0,
    "limit": 120
  }
}
```

Typical response fields:

- `content`
- `total_lines`
- `offset`
- `limit`
- `version`

### `edit` canonical contract (preview/apply)

`edit` now uses a single stable contract with explicit mode:

- `mode: "preview"` computes validation + planning + diff without writing.
- `mode: "apply"` uses the exact same validation + planning semantics, then writes if allowed.
- Legacy top-level `search`/`replacement`/`match_mode`/`occurrence` are still accepted for compatibility.

Canonical request shape:

```json
{
  "tool": "edit",
  "arguments": {
    "path": "src/lib.rs",
    "mode": "preview",
    "expected_version": "<version-from-read>",
    "operation": {
      "type": "search_replace",
      "search": "old_name",
      "replacement": "new_name",
      "match_mode": "literal",
      "occurrence": "first"
    }
  }
}
```

Stable response envelope fields:

- `proposal_id` (currently `null` in this slice)
- `applied` (bool)
- `would_change` (bool)
- `diff` (deterministic diff text for this plan)
- `stats` (deterministic counters)
- `diagnostics` (deterministic class/message entries)

Diff/newline semantics:

- Diff rendering is deterministic for identical input snapshots and operation plans.
- Paths in diff headers are stable (`--- a/file`, `+++ b/file`) for edit contract parity between preview/apply.
- Newline model preserves source line endings (LF/CRLF) in emitted line payloads.
- EOF newline transitions use unified-diff marker `\\ No newline at end of file`.
- Large diff output is bounded and can include a truncation marker with omitted-count metadata while preserving full summary stats.

Deterministic diagnostic classes used by the edit contract:

- `validation`
- `stale`
- `permission`
- `conflict`
- `internal`

### `edit` legacy-compatible example (search/replace)

```json
{
  "tool": "edit",
  "arguments": {
    "path": "src/lib.rs",
    "search": "old_name",
    "replacement": "new_name",
    "expected_version": "<version-from-read>",
    "match_mode": "literal",
    "occurrence": "first"
  }
}
```

Notes:

- `match_mode`: `literal` (default) or `regex`
- `occurrence`: `first` (default) or `all`
- If `mode` is omitted, behavior defaults to `apply` for backward compatibility.

### `patch` example (line-range batch)

```json
{
  "tool": "patch",
  "arguments": {
    "path": "src/lib.rs",
    "expected_version": "<version-from-read>",
    "operations": [
      {
        "range": { "start": 10, "end": 12 },
        "replacement": "new block\n"
      }
    ]
  }
}
```

### CAS conflict recovery flow (required)

When `edit`/`patch` detect a version mismatch, do not retry with stale args.
Use this flow:

1. `read` the file again to get latest `content` and `version`
2. recompute your intended change against that latest content
3. retry `edit`/`patch` with the new `expected_version`

Short form: **read -> recompute change -> retry with latest version**.

This is the required conflict recovery pattern for all mutating filesystem
built-ins.

## Sessions

```nu
# New session
"start" | agent --new-session

# Continue session
"continue" | agent --session "session-id"
```

### Compaction triggers and persistence

- Auto compaction trigger evaluation uses threshold tolerance and hysteresis:
  - fire bound: `threshold - tolerance`
  - rearm bound: `threshold - (tolerance + hysteresis_margin)`
- Manual slash trigger:
  - `/compact` triggers force compaction immediately (bypasses threshold gate)
  - `/compact <args>` is treated as unknown for now
  - unknown slash commands emit a warning and processing continues
- Slash command text is runtime control input and is not forwarded as prompt text to the LLM.
- Manual and auto trigger sources share one execution path, and compaction persistence is durable:
  - session JSONL content is rewritten through session APIs
  - compacted message set and `compaction_count` metadata are persisted
  - transcript-visible compaction summary artifact includes source + summarized/kept counts
  - compaction summary artifact includes summary preview/body from the produced summary text

### Compaction strategy contract

- Canonical strategy name is `sliding_summary`.
- Current runtime exposes a single active compaction mode only (`sliding_summary`).
- Legacy stored strategy values `truncate`, `sliding`, and `summarize` normalize to `sliding_summary` on deserialize.

### Inline slash suggestions and slash commands

- Inline slash suggestions open immediately when input starts with `/`.
- Suggestions are non-modal and independent of command palette popup/table/state.
- Filtering is deterministic and incremental as input grows (`/c`, `/co`, ...).
- If slash prefix is removed, inline suggestions close cleanly and never fallback to command palette.
- Supported slash commands:
  - `/compact`
  - `/mcp`
  - `/help`
  - `/status`
  - `/models`
- `/help`, `/status`, and `/mcp` route to the same action handlers as Ctrl-P entries.
- `/models` and Ctrl-P `Models` route to the same shared model-picker action handler.
- Unknown slash commands emit deterministic warning text and the interactive loop continues.
- Immediate slash command text is not echoed into the transcript and is not persisted as a session turn message.
- Compaction result artifacts remain transcript-visible (for example, compaction summary/source/count rows).

### Model picker and switch semantics

- `/models` opens an inline model picker in TUI.
- Ctrl-P `Models` opens the same picker path (launcher parity with slash).
- Model switch resolution uses cached startup `PluginConfig` for session lifetime (no plugin-config re-read per switch).
- Successful switch applies full resolved model configuration for next turn execution.
- Footer/status active model identity updates immediately after successful switch.

### Consolidated modal system

- Modal layout uses one policy source keyed by panel kind.
- Help modal uses larger readable viewport.
- Status modal uses compact content-fit viewport.
- MCP/Skills/Models use balanced default modal layout.
- Modal containers use rounded borders with dimmed backdrop while modal/picker is open.

## MCP tool filtering

```nu
"what tools do you have" | agent --mcp-tools ["c5t/*" "nu/*"]
```

## Flag reference

- `--model <provider/model>`
- `--small`
- `--api-key <string>`
- `--base-url <string>`
- `--temperature <number>`
- `--max-tokens <int>`
- `--max-context-tokens <int>`
- `--max-output-tokens <int>`
- `--max-turns <int>`
- `--tools <record>`
- `--mcp-tools <list<string>>`
- `--tool-timeout <duration>`
- `--session <id>`
- `--new-session`
- `--quiet` / `-q`
- `--verbose` / `-v` (progressive: `-v`, `-vv`, `-vvv+`)

## Output contract

- `stdout`: final machine-readable Nushell record output only.
- `stderr`: interactive UX/progress output (spinner while busy, tool progress, warnings, completion).

This keeps pipelines stable while preserving interactive feedback.

## TUI transcript markdown and fenced code

When running in TUI mode, assistant markdown is projected into transcript-friendly
lines.

- Supported markdown constructs (for example headings, emphasis, lists,
  blockquotes, and code fences) are rendered with deterministic formatting.
- Fenced code blocks with recognized languages use token-level syntax styles in
  the transcript.
- Unknown or unsupported fence languages fall back to plain code rendering
  (stable, readable, and non-panicking).

The fallback behavior keeps transcript output predictable even when model output
contains malformed markdown or uncommon language identifiers.

### Examples

Pipeline-safe capture of final output:

```nu
let result = ("Summarize repo" | agent --quiet)
$result._meta.usage.total_tokens
```

Interactive debugging with detailed stderr UX:

```nu
"Investigate failures" | agent -vv
```

### Busy indicator behavior

- Busy state is shown via spinner on interactive TTY stderr.
- Default UX does **not** print redundant persistent busy lines like "thinking" or "response ready".
- Default tool lifecycle UX is concise and singular:
  - busy: `[spinner] tool <tool_name> args=<truncated_args>` while running
  - completion: `✓ tool <tool_name> args=<truncated_args>` or `✗ tool <tool_name> args=<truncated_args>` exactly once
  - result payload follows on next line(s) as a separate block when meaningful
- Concise levels print non-empty payloads verbatim (including `null`, `[]`, and `{}`).
- In non-interactive stderr (non-TTY), spinner is disabled and only policy-driven persistent lines are shown.
