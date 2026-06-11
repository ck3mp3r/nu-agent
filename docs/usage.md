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

## Agent personas

Load instructions and configuration from persona files using `--agent`:

```nu
"implement the feature" | agent --agent coder
```

Persona file resolution (first match wins):
- `.agents/<name>.md` (project-local)
- `$XDG_CONFIG_HOME/nu-agent/agents/<name>.md` (global, usually `~/.config/nu-agent/agents/`)

### Front matter keys (optional YAML)

```yaml
---
name: coder
description: Development agent focused on implementing features
model: anthropic/claude-sonnet-4-20250514
permissions:
  "*": "ask"
  "read": "allow"
  "c5t_get*": "allow"
---

# Your agent instructions here
```

All front matter keys are optional:
- `name` - Agent identity (overridden by `--name` flag)
- `description` - Persona summary
- `model` - Default model (overridden by `--model` flag)
- `permissions` - Authorization overlay (overridden by `--permissions` flag)

**Precedence:** CLI flags > front matter > plugin config

### Examples

```nu
# Use persona as-is
agent --agent researcher

# Override persona's model
agent --agent coder --model openai/gpt-4o

# Override persona's name
agent --agent coder --name "bob"

```

## Tool authorization (permissions DSL)

Authorization uses a **map-style** `permissions` DSL (not a rules array).

Only CLI surface for policy override is `--permissions` (structured record/object).

```nu
# Build per-run overlay in Nu and pass as a record
let permissions = {
  "read": "deny"
  "nu__run": {
    "command": {
      "kubectl delete *": "deny"
      "*": "ask"
    }
  }
}

"review this command" | agent --permissions $permissions
```

Canonical shape:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  permissions: {
    "*": "ask"
    "read": "allow"
    "c5t_get*": "allow"
    "nu__run": {
      "command": {
        "kubectl delete *": "deny"
        "*": "ask"
      }
    }
  }
}
```

### Supported values

- global baseline: `"*": "allow|ask|deny"`
- tool pattern actions: `"read": "allow"`, `"c5t_get*": "allow"`
- nested `nu__run` map with explicit `command` key only:
  - `"nu__run": { "command": { "<pattern>": "allow|ask|deny" } }`

Unknown nested fields under `nu__run` are rejected with deterministic diagnostics.

### Deterministic precedence

Decision order is fixed:

1. global baseline
2. tool override
3. nested `nu__run.command` override

For `nu__run.command` matching, runtime normalizes commands by:

- trimming leading/trailing whitespace
- collapsing internal whitespace runs to one space

If `nu__run.command` is missing/unreadable, behavior deterministically falls back
to inherited tool/global decision with diagnostic metadata.

If nested `"*"` equals inherited decision, it is a valid no-op and reported via
deterministic diagnostics metadata.

### CLI overlay merge semantics

Effective policy is built once at startup using additive overlay:

1. base: `config.agent.permissions`
2. overlay: CLI `--permissions`

Merge rules:

- overlapping leaf/action keys: CLI wins
- non-overlapping config keys: retained
- nested maps (for example `nu__run.command`): deterministic key merge

Malformed CLI overlay fails fast with explicit key-path diagnostics.

Startup emits a compact policy diagnostic summary:

- `permissions policy: overlay_active=true|false global=<action> tool_rules=<n> nu__run.command_rules=<n>`

### Ask flow and session grants

Ask hook choices are:

- `allow_once`
- `allow_always` (session-only; reset on restart)
- `deny`

`allow_always` grants are **session-only** and scoped to the approving tool
context (tool/source/mode scope), not global across unrelated tools.

Within a running session, later calls in the same scoped tool context can reuse
that grant. Restarting the plugin/session clears all `allow_always` grants.

### Interactive permission prompt behavior (TUI)

When a tool call resolves to `ask` in TUI mode, execution pauses at the
authorization boundary until a decision is submitted. The tool handler is not
invoked while waiting.

The permission ask UI is rendered as a compact **inline permission card**
attached to the active tool call display in transcript space (not a large
blocking modal).

The card includes request context (`tool`, `source`, optional `mode`), matched
rule metadata, summary, and `request_id`.

Exact keybindings:

- `a` => `allow_once`
- `A` => `allow_always` (session-only)
- `d` => `deny`
- `Esc` => `deny`

Safety semantics:

- Timeout while waiting => deterministic deny
- Stale/unknown decision submissions => ignored
- Rule-identity mismatch submissions => ignored
- Prompt handling remains deterministic and non-blocking for overall UI loop

TUI rendering guardrails:

- Prompt content is rendered as an **inline transcript card** anchored to the related tool row (not a blocking modal).
- Permission decision controls are rendered in a **sticky footer row** so controls remain visible independent of transcript window clipping.
- Viewport fitting preserves a required permission-context row while ask is active; manual user scroll override disables auto-recentering jitter but keeps required-row preservation state.

Lifecycle events emitted by runtime/UI path:

- `PermissionRequested`
- `PermissionDecisionSubmitted`
- `PermissionDecisionTimedOut`
- `PermissionDecisionIgnored`

### Non-interactive ask fallback

In non-interactive mode (`stderr` mode), `ask` defaults to secure deny.

Optional override in plugin config:

```nu
$env.config.plugins.agent = {
  # ...
  non_interactive_ask: "allow"  # or "deny" (default)
}
```

- default (missing): `deny`
- supported values: `deny`, `allow`
- invalid values fail fast with deterministic config error

## Built-in filesystem tools (CAS-safe)

The agent exposes exactly three built-in filesystem tools:

- `read`
- `edit`
- `patch`

These names are unprefixed and exact. There are no builtin aliases like
`fs__read` or `tool__edit`.

### Filesystem tool permission gating

`read` is non-mutating and **bypasses** the permission system entirely — it is always allowed.

`edit` and `patch` are filesystem-mutating and **require permission approval**, exactly like MCP or closure tools. Use the permissions DSL to control their behavior per persona:

```nu
# Developer persona — allow edit/patch without prompting
permissions:
  "*": "ask"
  "edit": "allow"
  "patch": "allow"
```

```nu
# Researcher persona — deny all writes
permissions:
  "*": "ask"
  "edit": "deny"
  "patch": "deny"
```

```nu
# Default (safe) — prompt before every edit/patch
permissions:
  "*": "ask"
```

Via CLI:

```nu
# Allow edit/patch for this run
let perms = { "*": "ask", "edit": "allow", "patch": "allow" }
"refactor the code" | agent --permissions $perms
```

The default for unmatched tools is `ask` (interactive prompt in TUI, deny in non-interactive mode).

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

### Compaction strategies

Three compaction strategies are available:

- `sliding_summary` (default) — LLM summarizes old messages, keeps recent verbatim window.
- `sliding_window` — drops old messages, keeps only the last N. No LLM call.
- `token_truncate` — keeps newest messages within a token budget (chars/4 estimate). No LLM call.

Legacy stored strategy values `truncate`, `sliding`, and `summarize` normalize to `sliding_summary` on deserialize.

### Two-tier compaction policy

Compaction uses a two-tier policy:

1. **Proactive compaction** fires when context usage reaches `proactive_threshold_pct` (default: 0.80 = 80%) of the context window. Uses the configured primary strategy.
2. **Fallback compaction** fires at 95% of the context window using the ordered `fallback_strategies` list (default: `["sliding_window"]`). This is a safety net if the primary strategy is unavailable or insufficient.

CLI flags override plugin config, which overrides built-in defaults.

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

### MCP panel error presentation

- MCP table stays compact: `Status`, `Name`, `Visible tools`.
- `Status` is iconized with UTF-8 emoji for quick scan: `🟢` enabled, `⚪` disabled, `🔴` failed.
- `Visible tools` is derived from live LLM-visible MCP tool mapping per server (not gated by row state text).
- Full error text is shown below the table for the currently selected MCP row.
- Selected-row details include compact `Tools: a, b, c` formatting with sorted visible tool names; fallback is `Tools: None`.
- MCP layout keeps table as primary pane in typical modal heights; details stay compact.
- When tool names exceed the details line budget, tools remain comma-separated and end with deterministic truncation cue: `+N more`.
- If selected row has no error reason, details pane shows `Error: None`.
- Keyboard selection behavior is unchanged; moving selection updates the details pane.
- Top legend line is removed; controls remain compact (`Session-only toggles | Enter/Space toggle | Esc close`).

## Permission-based tool visibility

Permissions control both **visibility** and **authorization**. Denied tools are
hidden from the LLM entirely — they never appear in the tool list.

| Action | Visible to LLM | Behavior |
|--------|----------------|----------|
| `allow` | yes | Runs without prompting |
| `ask` | yes | User prompted before each use |
| `deny` (tool-level) | **no** | Hidden from LLM tool list |
| `deny` (granular, e.g. `nu__run.command`) | yes | Tool visible; specific commands blocked at runtime |

### Defaults

| Mode | Default when no `permissions` configured |
|------|------------------------------------------|
| TUI (interactive) | `"*": "ask"` — all tools visible, user prompted |
| TTY (non-interactive) | `"*": "deny"` — all tools hidden; must allowlist |

### TTY allowlist example

In non-interactive (TTY/pipe) mode, deny everything by default and allow only
the tools you need:

```nu
$env.config.plugins.agent = {
  permissions: {
    "*": "deny"
    "read": "allow"
    "grep": "allow"
    "glob": "allow"
    "c5t_get*": "allow"
    "c5t_list*": "allow"
  }
}
```

### TUI denylist example

In interactive (TUI) mode, ask for everything by default and deny tools that
should never be available:

```nu
$env.config.plugins.agent = {
  permissions: {
    "*": "ask"
    "nu__run": "deny"
    "edit": "deny"
    "patch": "deny"
  }
}
```

## Flag reference

- `--model <provider/model>`
- `--small`
- `--api-key <string>`
- `--base-url <string>`
- `--temperature <number>`
- `--max-context-tokens <int>`
- `--max-output-tokens <int>`
- `--max-turns <int>`
- `--tools <record>`
- `--permissions <record>`
- `--tool-timeout <duration>`
- `--session <id>`
- `--quiet` / `-q`
- `--verbose` / `-v` (progressive: `-v`, `-vv`, `-vvv+`)
- `--compaction-strategy <string>` — `sliding_summary`, `sliding_window`, `token_truncate`
- `--compaction-threshold <int>` — message count threshold for auto-compaction (default: 100)
- `--keep-recent <int>` — recent messages to keep during compaction (default: 10)
- `--token-budget <int>` — token budget for `token_truncate` strategy
- `--proactive-threshold-pct <number>` — proactive compaction threshold 0.0–1.0 (default: 0.80)

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

See also: [Contribution guardrails](./contribution-guardrails.md) for contributor-facing invariants and test mapping.

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
