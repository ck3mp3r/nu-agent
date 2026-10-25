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
temperature: 0.2
max_tokens: 8192
max_tool_turns: 20
max_tool_calls_per_subturn: 3
max_tool_result_bytes: 50000
additional_params:
  thinking: disabled
permissions:
  "*": "ask"
  "read": "allow"
  "c5t_get*": "allow"
---

# Your agent instructions here
```

All front matter keys are optional:
- `name` — agent identity (overridden by `--name` flag)
- `description` — persona summary shown in `agent list`
- `model` — model role name or literal `provider/model` (resolved via `models` map; overridden by `--model` flag)
- `temperature` — response creativity, 0.0–2.0 (overridden by `--temperature` flag)
- `max_tokens` — maximum tokens to generate
- `max_tool_turns` — maximum tool execution turns per conversation turn
- `max_tool_calls_per_subturn` — maximum tool calls per LLM sub-turn
- `max_tool_result_bytes` — maximum bytes per tool result before truncation; 0 = unlimited
- `additional_params` — provider-specific parameters (e.g. `thinking: disabled` for Anthropic)
- `permissions` — authorization overlay (overridden by `--permissions` flag)

**Precedence:** CLI flags > front matter > config.toml

### Examples

```nu
# Use persona as-is
agent --agent researcher

# Override persona's model
agent --agent coder --model openai/gpt-4o

# Override persona's name
agent --agent coder --name "bob"

```

## Model roles

Configure named model roles in config.toml:

```toml
[models.default]
model = "openai/gpt-4o"

[models.heavy]
model = "anthropic/claude-sonnet-4-6"

[models.light]
model = "ollama/gemma4:31b"
```

Then reference roles by name in agent persona front matter:

```yaml
---
model: light
---
```

When a persona has `model: light`, it resolves to `models.light` from config.toml. If your persona needs a specific model that isn't in the map, use the full `provider/model` identifier instead:

```yaml
---
model: openai/gpt-4o-mini
---
```

CLI `--model provider/model` overrides all resolution and is used as-is.

Resolution rules:
1. **Role name** (e.g. `model: heavy`, no slash, matches a key in `models`) → resolves to that key's value
2. **Literal** (e.g. `model: provider/model`, contains slash) → used as-is
3. **Unknown role** (no slash, not in the map) → error at startup
4. **No `model:` field** → falls back to `models.default`

## Model pricing

Every model the runtime can serve resolves a price through one ladder. The
ladder runs once, when the model binding is constructed, and the resolved
prices ride the model's wire facts. The cost hook reads them; it holds no
rung logic of its own.

Outcomes, highest precedence first:

1. **Endpoint-reported** — the provider's own reply states the cost. This is
   authoritative and always wins.
2. **User-configured pricing** — a `pricing` block on the model's config
   record. The user stated what they pay, so it wins over the shipped rate.
   See [Per-model pricing](configuration.md#per-model-pricing).
3. **Own cache row** — the model's own row in the local models cache
   (`agent models sync`) lists a cost.
4. **Residual** — nothing prices the model. The call stays tokens-only.

The cache row reads the same cache the model picker renders, so the price a
session computes and the price the picker shows come from one source.

There is no cross-vendor borrowing: a model id priced only under another
section stays tokens-only unless the block declares that section as its
`catalog`.

Pricing is per **model id**, not per wire. The vendor whose wire serves the
model scopes only the base spec's shape — the modalities and compatibility
rig's built-in catalog lists for that vendor. The price comes from the ladder
over the models cache, keyed by the model id and the block's declared cache
section. A provider block named for a reseller therefore prices from the
section it declares: a block named `ollama-cloud` that serves through the
openai wire (`provider = "openai"`, `base_url = "https://ollama.com/v1"`)
prices its models from the `ollama-cloud` section, even though the wire
reports `openai`.

### Provider names and the cache

The cache stores models.dev provider keys. A config block reaches its section
through `catalog`; without it, the block name is its own section key. There is
no name mapping, so a block named `ollama` reaches no section unless it
declares `catalog = "ollama-cloud"`. See
[The models cache](configuration.md#the-models-cache).

### The Ollama caveat

`ollama` reaches no cache section, so its models stay tokens-only. Declare
`catalog = "ollama-cloud"` and the block prices from that section, whose
rates are Ollama Cloud list prices for those model ids. A local Ollama server
is not billed those rates, so the session cost line then reads as if the
tokens were billed at cloud list prices.

### The residual

A model no rung prices stays tokens-only. The existing signals say so
explicitly: the model picker leaves its price column blank, and the session
cost line shows tokens alone, or a figure ending in `+` while any call was
unpriced. The residual classes are:

- a model id the cache lists with no cost — a local Ollama classic whose id
  no hosted vendor uses is the common case;
- a model the cache does not list at all, or one whose block declares no
  `catalog` and whose name is not a cache section key.

A blank price is never invented. If a model you use lands in the residual,
run `agent models sync` first — a newer cache may price it — or set a
`pricing` block for it in config.toml.

A reply that reports no input or output token count also stays tokens-only:
the rates apply to those counters. A missing `total_tokens` never blocks a
price — some endpoints omit it, and the total is not what the rates
multiply.

A session whose calls are only partly priced shows the summed price with a
`+` suffix. That happens when a call reports cache tokens the model's row
lists no rate for: the known parts are summed, and the `+` says the figure is
a lower bound.

## Tool authorization (permissions DSL)

Authorization uses a **map-style** `permissions` DSL (not a rules array).

Only CLI surface for policy override is `--permissions` (structured record/object).

```nu
# Build per-run overlay in Nu and pass as a record
let permissions = {
  "read": "deny"
  "nu": {
    "command": {
      "kubectl delete *": "deny"
      "*": "ask"
    }
  }
}

"review this command" | agent --permissions $permissions
```

Canonical shape:

```toml
[permissions]
"*" = "ask"
read = "allow"
"c5t_get*" = "allow"

[permissions.nu.command]
"kubectl delete *" = "deny"
"*" = "ask"
```

### Supported values

- global baseline: `"*": "allow|ask|deny"`
- tool pattern actions: `"read": "allow"`, `"c5t_get*": "allow"`
- nested `nu` map with explicit `command` key only:
  - `"nu": { "command": { "<pattern>": "allow|ask|deny" } }`

Unknown nested fields under `nu` are rejected with deterministic diagnostics.

### Deterministic precedence

Decision order is fixed:

1. global baseline
2. tool override
3. nested `nu.command` override

For `nu.command` matching, runtime normalizes commands by:

- trimming leading/trailing whitespace
- collapsing internal whitespace runs to one space

If `nu.command` is missing/unreadable, behavior deterministically falls back
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
- nested maps (for example `nu.command`): deterministic key merge

Malformed CLI overlay fails fast with explicit key-path diagnostics.

Startup emits a compact policy diagnostic summary:

- `permissions policy: overlay_active=true|false global=<action> tool_rules=<n> nu.command_rules=<n>`

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

Nu command rendering:

- Each tool call row starts with the tool name (for example `edit`, `read`, `nu`) followed by the call summary. The name carries the tool lane's emphasis style; the summary carries the muted style. The `nu` call line carries the applied timeout as `⏱ {timeout}s` (120s when the call omits `timeout_seconds`) — the command renders only in the preview block below it.
- When the agent calls the `nu` tool, the transcript shows the command as a syntax-highlighted code block under the tool status row, on a full-width background block (with one blank margin row above and below) that separates it from the transcript. The command appears at `ToolEvent::Started` — before the permission decision and before completion.
- At the permission gate, the `nu` command also renders as a code preview block (language `nu`) directly under the tool row, so you see the exact command before you approve. The preview is built from the tool-call arguments alone — nothing executes before you approve.
- The edit tool's diff display renders in the same full-width background block style (with margin rows). Each diff body line carries a muted line-number gutter (old and new columns) followed by the syntax-highlighted code body, on a line-level tint: added lines get a dim green background, removed lines a dim red one, and context lines stay untinted. Hunk headers (`@@`), file headers (`---`/`+++`), and the no-newline marker render as plain meta rows.
- Consecutive tool blocks get one spacer line between them when either renders a background block.
- Other tools keep their existing previews (edit shows a diff, nu shows the command) or none.
- See `./contribution-guardrails.md` for the pre-authorize zero-write rules behind the edit preview.

Lifecycle events emitted by runtime/UI path (all delivered on the single `ui_event` bus channel):

- `PermissionRequested`
- `PermissionDecisionSubmitted`
- `PermissionDecisionTimedOut`
- `PermissionDecisionIgnored`

### Non-interactive ask fallback

In non-interactive mode (`stderr` mode), `ask` defaults to secure deny.

Optional override in config.toml:

```toml
[permissions]
non_interactive_ask = "allow"
```

- default (missing): `deny`
- supported values: `deny`, `allow`
- invalid values fail fast with deterministic config error

## Built-in tools

The agent exposes the following built-in tools (no MCP server required):

**Filesystem (CAS-safe):**
- `read` — read file content with optional line windowing
- `edit` — search/replace or create files with CAS guard

**Search:**
- `grep` — recursive regex search (ripgrep-style)
- `glob` — file glob matching

**HTTP:**
- `http` — fetch URLs, markdown extraction from HTML

**Skills:**
- `skill` — load skill content by name from local or home skill roots

**Shell execution:**
- `nu` — stateless Nushell command execution (NOT bash/sh/zsh)

**Code analysis:**
- `ast_query` — run an S-expression tree-sitter query against source code
- `ast_nodes` — list AST nodes of a given type in source code
- `ast_refs` — find references to a named symbol in source code
- `ast_tree` — dump the full S-expression parse tree of source code

  All four tools require `path` and `language` parameters. `language` must match an installed tree-sitter grammar (e.g. `rust`, `python`, `go`). See [docs/configuration.md](../configuration.md) for full setup instructions.

  ```json
  {"path": "src/main.rs", "language": "rust", "query": "(function_item name: (identifier) @name)"}
  {"path": "src/main.rs", "language": "rust", "node_type": "function_item"}
  {"path": "src/main.rs", "language": "rust", "name": "Config"}
  {"path": "src/main.rs", "language": "rust", "max_depth": 3}
  ```

**tmux session management:**
- `tmux_session` — list, info, create, kill sessions
- `tmux_window` — create, kill windows
- `tmux_pane` — list, find, capture, send, split, kill panes
- `tmux_layout` — select layout

Panes are addressed as `session:window.pane`. Prefer the `target` field returned by
`tmux_pane` list/find; `pane: "2"` means pane index 2 of the given (or current)
window, not window 2.

These names are unprefixed and exact. There are no builtin aliases like
`fs__read` or `tool__edit`.

### Filesystem tool permission gating

`read` is non-mutating and **bypasses** the permission system entirely — it is always allowed.

`edit` is filesystem-mutating and **requires permission approval**, exactly like MCP or closure tools. Use the permissions DSL to control its behavior per persona:

```nu
# Developer persona — allow edit without prompting
permissions:
  "*": "ask"
  "edit": "allow"
```

```nu
# Researcher persona — deny all writes
permissions:
  "*": "ask"
  "edit": "deny"
```

```nu
# Default (safe) — prompt before every edit
permissions:
  "*": "ask"
```

Via CLI:

```nu
# Allow edit for this run
let perms = { "*": "ask", "edit": "allow" }
"refactor the code" | agent --permissions $perms
```

The default for unmatched tools is `ask` (interactive prompt in TUI, deny in non-interactive mode).

### Contracts

- `read` is non-mutating and returns file content plus metadata, including
  `version` (content hash token).
- `edit` is a mutating operation and **requires**
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

`edit` uses a single stable contract with explicit mode:

- `mode: "preview"` computes validation + planning + diff without writing.
- `mode: "apply"` uses the exact same validation + planning semantics, then writes if allowed.
- The `operation` block is required and supports types `search_replace` or `create`.

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

No-op and conflict results:

- An apply-mode `edit` that would not change the file fails the tool call. The tool returns an error whose message names the reason (`no change` for an unmatched search or an identical replacement, `conflict` for a stale `expected_version` or an existing create target) and whose details carry the same contract envelope (`applied: false`, `would_change: false`, `noop` or `conflict: true`, `diff: ""`).
- The permission gate shows no diff block for these plans — there is nothing to approve, so the prompt renders bare.
- The failed disposition marks the tool row as failed in the TUI instead of done.

### `edit` with `operation.type: "create"` (new file creation)

To create a new file, use `operation.type: "create"` with a `content` field:

```json
{
  "tool": "edit",
  "arguments": {
    "path": "src/new_module.rs",
    "operation": {
      "type": "create",
      "content": "// New module\npub fn hello() {\n    println!(\"hello\");\n}\n"
    }
  }
}
```

Notes:

- `expected_version` is not needed for create operations.
- If the file already exists, the call fails with an `already exists` conflict error. Use `search_replace` instead.
- Parent directory must exist. The tool does not create intermediate directories.

### CAS conflict recovery flow (required)

When `edit` detects a version mismatch, do not retry with stale args.
Use this flow:

1. `read` the file again to get latest `content` and `version`
2. recompute your intended change against that latest content
3. retry `edit` with the new `expected_version`

Short form: **read -> recompute change -> retry with latest version**.

This is the required conflict recovery pattern for all mutating filesystem
built-ins.

## Sessions

```nu
# Continue session
"continue" | agent --session "session-id"
```

### Session storage

A session's conversation lives in a rig-memory store. The `--store` flag picks
the backend:

| `--store` | Where the session lives |
|-----------|--------------------------|
| `sqlite` (default) | `{cache}/{APP_DIR_NAME}/rig-sessions.db` — one database holds every session |
| `jsonl` | `{cache}/{APP_DIR_NAME}/sessions/{session_id}/` — a `DirStore` directory per session, holding `logs/`, `files/` (with `files/blobs/`), and the `name` file |
| `memory` | An in-memory database. Nothing survives the process, so it is for tests and one-shot runs |

`APP_DIR_NAME` is `nu-agent-dev` in debug builds and `nu-agent` in release
builds. The rig database is separate from the old `sessions.db`: the rig store
refuses a database that holds the pre-rig-memory schema.

**Legacy sessions are refused, not migrated.** A session saved by a version of
nu-agent before rig-memory makes the store report *"this session was made by
another version of nu-agent; start a new session"*. The old files are left as
they are. Start a new session.

**A stopped or failed turn keeps its completed work.** rig saves a turn only
when it succeeds, so the host appends the interrupted turn's delta to the
session log itself: the prompt, the tool exchanges that finished (with their
real results), and the assistant text the turn streamed. A call killed
mid-execution is closed as `[interrupted]`, so the log stays valid. The next
turn continues from that log. A call the stop denied before it ran renders as a
not-run (⊘) tool block with the args preview of what would have run under it,
live and after a reload. No reason text renders. The batch's later calls the
stop skipped without dispatching render the same ⊘ shape.

**A warning appears when the turn's work could not be saved.** If another
writer moved the session log while the turn ran, the host leaves the log alone
and shows *"Turn stopped: the session log moved mid-turn; this turn's work was
not saved"* instead of writing a delta it cannot attribute.

**Stopping may take up to ~2 s.** After a cancel, the turn keeps draining the
model stream for a bounded grace so in-flight tools finish and their results
reach the session log.

**Re-login after upgrading from a pre-0.45 build.** The rig 0.45 upgrade
changed how provider credentials are read. A Copilot login made by an older
build is not picked up; run `agent provider auth login` again.

### Session metadata

The host writes the session's state records itself, because rig 0.45's save path
writes only the header and change records:

- `rig::model` — the `provider/model` string the turn ran, taken from the
  resolved config rather than the wire descriptor (a custom provider on an
  OpenAI-shaped wire would otherwise be mislabelled).
- `rig::title` — the start of the session's **first** user message, so a resumed
  session keeps its original title.
- `meta.json` — the listing: title, labels, `updated`, model, message count, and
  plugins' listed values.

A model switch rewrites the model record and relists; it passes no title, so the
title is preserved.

`agent session list` and `agent session inspect` read these records. The plugin
command shows all six listing fields (`id`, `title`, `model`, `messages`,
`updated`, `labels`, `listed` — `listed` as JSON text). The CLI's `agent session
list` table shows five of them (`ID`, `TITLE`, `MODEL`, `MESSAGES`, `UPDATED`):
`labels` and `listed` are plugin-owned metadata, so the CLI table leaves them
out. Use the plugin command when you need them.

A session whose store cannot be opened is skipped with a warning, so one
unreadable log does not hide the others.

### Open-store bound

A session's store is opened on first use and kept. At most **32** stores stay
open at once; the oldest is dropped when the bound is reached. A turn that still
holds the store keeps it alive through its `Arc`, and the next access reopens it.

On the `memory` backend nothing persists across opens, so an evicted session
disappears from `agent session list` entirely — its conversation lived only in
the store that was dropped.

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
  - the splice is written to the session's rig-memory log through rig's own save path
  - the compacted message set is what the next run loads
  - transcript-visible compaction summary artifact includes source + summarized counts
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

CLI flags override config.toml, which overrides built-in defaults.

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
  - `/skills`
  - `/models`
  - `/agent`
  - `/new`
  - `/session`
  - `/theme`
- `/help`, `/status`, `/mcp`, and `/skills` route to the same action handlers as Ctrl-P entries.
- `/models` and Ctrl-P `Models` route to the same shared model-picker action handler.
- `/new` clears the transcript, starts a new session, and shows the startup logo.
- Unknown slash commands emit deterministic warning text and the interactive loop continues.
- Immediate slash command text is not echoed into the transcript and is not persisted as a session turn message.
- Compaction result artifacts remain transcript-visible (for example, compaction summary/source/count rows).
- Input soft-wraps at word boundaries; the input box grows up to 6 rows, then scrolls internally keeping the caret visible.

### Prompt queue and cancel-on-empty-Enter

- Submitting a prompt while a turn is busy queues it; the queued prompts run after the active turn completes.
- Pressing Enter on empty input while a turn is busy and at least one prompt is queued cancels the active turn. The queued prompts then run through the normal drain path.
- Pressing Enter on empty input while idle, or while busy with no queued prompts, does nothing.
- Stopping a turn shows `Turn stopped` in the status bar until the next turn starts, and keeps the turn's completed work in the session.

### Ctrl-C quit guard

- `Ctrl-C` is intercepted globally, before any popup or mode routing. Each press does exactly one thing.
- A permission prompt open: `Ctrl-C` arms the quit confirmation and leaves the prompt untouched. `Esc` keeps sole ownership of `deny`.
- Any other popup open (picker, info panel, inline slash): `Ctrl-C` closes it without arming and without changing the draft.
- No popup and a non-empty draft: `Ctrl-C` clears the draft without arming. The turn state is unchanged, so a busy turn keeps running.
- No popup and an empty draft: `Ctrl-C` arms the quit confirmation and shows `Ctrl-C again to quit`.
- A second `Ctrl-C` within 5 seconds quits. While a turn is busy, quitting also requests cancel.
- Any other key or a paste disarms the confirmation. A resize leaves it armed.
- The confirmation expires after 5 seconds; the status message expires through its own 5 second TTL.

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

## Hooks

A hook is a Nushell script that runs when the agent publishes a lifecycle event. Hooks observe the agent. They never change a turn, a tool result, or the agent output.

Configure hooks in `config.toml`. See [Configuration § Hooks](configuration.md#hooks) for the fields.

```toml
[[hooks]]
name = "herdr"
events = ["turn_started", "turn_completed"]
script = "hooks/herdr.nu"
```

### Handler convention

The agent runs one handler per event. The handler is a function named after the event. It takes one argument: the envelope as a record.

```nu
def turn_started [event: record] {
    print $"turn started in session ($event.session_id)"
}
```

The agent invokes the handler with this command:

```nu
nu --stdin -c "source '<script>'; <event> ($in | from json)"
```

The agent writes the JSON envelope to the child process stdin. `--stdin` binds that input to `$in`, and `from json` turns it into the record the handler receives. The handler runs in the session working directory and inherits the environment.

### Envelope

Every invocation receives one JSON envelope:

```json
{
  "version": 1,
  "event": "turn_started",
  "timestamp": "2026-10-25T14:32:07.481920+00:00",
  "session_id": "9dad1e4e08b0b11c-main",
  "payload": {
    "prompt": "list the rust files",
    "task_id": null
  }
}
```

- `version` — envelope schema version. Current value is `1`.
- `event` — the event name.
- `timestamp` — RFC 3339 UTC time of the invocation.
- `session_id` — the session the event belongs to. See [Session id](#session-id).
- `payload` — event-specific fields.

Payload fields per event:

| Event | Payload fields |
|-------|----------------|
| `turn_started` | `prompt` (string), `task_id` (string or null) |
| `turn_completed` | `tool_calls` (integer) |
| `tool_started` | `name`, `source`, `arguments` (raw JSON string) |
| `tool_completed` | `name`, `source`, `arguments`, `success` (bool), `error_kind` (string or null), `message` (string or null) |
| `session_started` | `session_id` (string), `hydrated` (bool) |
| `session_ended` | `session_id` (string) |
| `agent_started` | `mode` (`"tui"`, `"stderr"`, or `"acp"`) |
| `agent_shutdown` | `reason` (`"completed"`, `"shutdown"`, or `"error"`) |

`task_id` is set only when an A2A task drives the turn. The TUI single-turn path, the stderr path, and the ACP path publish no A2A attribution, so `task_id` is null there.

### Session id

The runner tracks the session id from `SessionEvent::Started` and `SessionEvent::Switched` on the session bus. The envelope carries the tracked id.

The id is an empty string in two cases:

- The runtime published no session event. Stderr mode and TUI single-turn publish none, so their turn and tool envelopes carry `""`.
- The event is agent-class. Agent events are process-scoped, so `agent_started` and `agent_shutdown` always carry `""`. This is by design.

### Ordering

The agent guarantees order in two scopes:

- Within one `[[hooks]]` entry on one runner. The entry has one subscriber task per runner. It runs one handler to completion before it takes the next event, so a hook never overlaps itself.
- Within one event channel. Events on one channel arrive in publish order. `turn_started` and `turn_completed` share the turn channel, so `turn_started` always precedes `turn_completed`.

Order across channels is best-effort. A `tool_started` and a `turn_completed` that are both ready can arrive in either order. A hook that declares both session-class and agent-class events runs on two runners, so its `agent_started` and `turn_started` have no relative order.

### Failure behavior

A hook failure never affects the agent run.

- A handler that exits non-zero, fails to spawn, or exceeds `timeout_ms` produces one warning log line: `hook '<name>' failed on <event>: <error>`.
- The warning goes to the agent log file at `$XDG_STATE_HOME/nu-agent/logs/agent.log`. The agent never writes log output to the terminal.
- A timeout kills the subprocess.
- The agent never retries a failed hook.
- The agent never publishes a hook result.
- The turn continues.

At exit the agent gives a running handler limited grace:

- Agent-class runners: up to 250 ms each.
- Session-class runner in stderr mode: one 250 ms pause in total, after the turn.
- Session-class runner in TUI and ACP: no exit wait. A handler still running when the process exits is abandoned.

### Mode coverage

| Event | TUI interactive | TUI single-turn | stderr | ACP |
|-------|-----------------|-----------------|--------|-----|
| `turn_started`, `turn_completed` | yes | yes | yes | yes |
| `tool_started`, `tool_completed` | yes | yes | yes | yes |
| `session_started`, `session_ended` | yes | no | no | yes |
| `agent_started`, `agent_shutdown` | yes | yes | yes | yes |

`session_started` and `session_ended` are per-session events. They fire where a session lifecycle exists: the TUI interactive loop and ACP sessions. Stderr mode and TUI single-turn run one turn and publish no session lifecycle event.

`agent_started` and `agent_shutdown` are per-process events. They fire exactly once per process in all three modes. The pair is best-effort on signals: no SIGTERM handler exists, so SIGTERM and SIGKILL produce no `agent_shutdown`.

The plugin `agent` command runs turns but starts no hook runner. Hooks never fire there. The `agent` binary does start the runners. See [Example: signal an external app](#example-signal-an-external-app).

### Example: signal an external app

This script posts each turn boundary to a local HTTP endpoint. Save it as `$XDG_CONFIG_HOME/nu-agent/hooks/herdr.nu`.

```nu
# ~/.config/nu-agent/hooks/herdr.nu
const HERDR_URL = "http://127.0.0.1:7777/events"

def turn_started [event: record] {
    let body = {
        kind: "turn_started"
        session: $event.session_id
        prompt: $event.payload.prompt
    }
    http post --content-type application/json $HERDR_URL ($body | to json)
}

def turn_completed [event: record] {
    let body = {
        kind: "turn_completed"
        session: $event.session_id
        tool_calls: $event.payload.tool_calls
    }
    http post --content-type application/json $HERDR_URL ($body | to json)
}
```

Register it in `config.toml`:

```toml
[[hooks]]
name = "herdr"
events = ["turn_started", "turn_completed"]
script = "hooks/herdr.nu"
timeout_ms = 5000
```

Run one turn with the `agent` binary:

```nu
^agent "list the rust files"
```

The caret forces the external `agent` binary. With the plugin registered, `agent` alone calls the plugin command, which starts no hook runner. The binary runs one turn in stderr mode, where the runners are wired.

The endpoint receives one `turn_started` post and one `turn_completed` post. If the endpoint is down, `http post` fails, the handler exits non-zero, and the agent logs one warning. The turn still completes.

### Deferred

Hooks cannot veto or modify an event. Hooks run without a sandbox. There is no `/hooks` panel. The agent spawns one process per event; a persistent-stdio mode is not implemented.

## Permission-based tool visibility

Permissions control both **visibility** and **authorization**. Denied tools are
hidden from the LLM entirely — they never appear in the tool list.

| Action | Visible to LLM | Behavior |
|--------|----------------|----------|
| `allow` | yes | Runs without prompting |
| `ask` | yes | User prompted before each use |
| `deny` (tool-level) | **no** | Hidden from LLM tool list |
| `deny` (granular, e.g. `nu.command`) | yes | Tool visible; specific commands blocked at runtime |

### Defaults

| Mode | Default when no `permissions` configured |
|------|------------------------------------------|
| TUI (interactive) | `"*": "ask"` — all tools visible, user prompted |
| TTY (non-interactive) | `"*": "deny"` — all tools hidden; must allowlist |

### TTY allowlist example

In non-interactive (TTY/pipe) mode, deny everything by default and allow only
the tools you need:

```toml
[permissions]
"*" = "deny"
read = "allow"
grep = "allow"
glob = "allow"
"c5t_get*" = "allow"
"c5t_list*" = "allow"
```

### TUI denylist example

In interactive (TUI) mode, ask for everything by default and deny tools that
should never be available:

```toml
[permissions]
"*" = "ask"
"nu" = "deny"
edit = "deny"
```

## Flag reference

`agent` (TUI mode when no input is piped):

- `--model <provider/model>` / `-m` — model to use in `provider/model` format
- `--api-key <string>` — API key override for the provider
- `--base-url <string>` — custom API endpoint URL
- `--temperature <number>` — sampling temperature (0.0 to 2.0)
- `--max-context-tokens <int>` — maximum context window size (input + output)
- `--max-output-tokens <int>` — maximum output tokens
- `--max-turns <int>` — maximum tool calling turns
- `--tools <record>` — record of tool closures `{name: closure, ...}`
- `--permissions <record>` — structured permissions overlay for this run
- `--tool-timeout <duration>` / `-t` — tool execution timeout (default: 30s)
- `--session <id>` — session ID to resume (auto-creates if missing)
- `--a2a-port <int>` — A2A agent port. `0` = random, `>0` = fixed; implies A2A mode
- `--mesh-key <string>` — mesh isolation key; agents only discover peers with the same key (default: SHA-256 of cwd)
- `--verbose` / `-v` — increase UX detail; repeat (`-v`, `-vv`, `-vvv`)
- `--quiet` / `-q` — suppress non-essential UX progress output
- `--log-level <string>` — file log level (`error|warn|info|debug|trace`); writes to `$XDG_STATE_HOME/nu-agent/logs/agent.log`
- `--agent <name>` — load an agent persona from `.agents/<name>.md`
- `--name <string>` — agent instance identity for multi-agent messaging
- `--parent-name <string>` — parent agent name for sub-agent reporting (internal)
- `--compaction-strategy <string>` — `sliding_summary`, `sliding_window`, `token_truncate`
- `--keep-recent <int>` — recent messages to keep during compaction (default: 10)
- `--token-budget <int>` — token budget for `token_truncate` strategy
- `--proactive-threshold-pct <number>` — proactive compaction threshold 0.0–1.0 (default: 0.80)
- `--store <string>` — session store backend: `sqlite|jsonl|memory`

## Subcommands

```nu
# Configuration
agent config init [--force]

# Model discovery
agent models sync
agent models list [--provider <name>]

# Provider auth (OS keychain)
agent provider auth login <name> [--api-key <key>]
agent provider auth logout <name>
agent provider auth status

# MCP auth
agent mcp auth login <server>
agent mcp auth logout <server>
agent mcp auth status

# Sessions
agent session list [--store <type>]
agent session clear <id> [--store <type>]
agent session inspect <id> [--store <type>]
```

## MCP Authentication Commands

Manage OAuth credentials for MCP servers. These commands operate on the credential store (not config.toml).

### `agent mcp auth login`

Triggers the OAuth authorization-code flow with PKCE for a configured MCP server. Opens a browser to the server's authorization URL and waits for the callback.

```nu
agent mcp auth login my-server
```

The MCP server must be configured with `auth.type = "oauth"` in config.toml. On success, tokens are saved to the credential store and automatically refreshed on expiry.

Example output (stderr):

```
Opening browser to authenticate with MCP server 'my-server'...
If the browser doesn't open, visit:
  http://auth-server.example.com/authorize?response_type=code&client_id=...
Successfully authenticated with MCP server 'my-server'
```

#### What happens during login

The OAuth authorization-code flow with PKCE proceeds as follows:

1. **Load config** — reads the MCP server configuration from config.toml.
2. **Start callback server** — binds a local HTTP server to `127.0.0.1` on a random port to receive the OAuth redirect.
3. **Discover OAuth metadata** — fetches `/.well-known/oauth-authorization-server` from the server URL to find authorization, token, and registration endpoints.
4. **Register client** — if no `client-id` is configured, performs dynamic client registration via the discovered registration endpoint. If `client-id` is configured, uses it directly.
5. **Generate PKCE challenge** — creates a cryptographically random `code_verifier` and `code_challenge` (SHA-256 hashed) for the authorization request.
6. **Build authorization URL** — constructs the URL with `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` (CSRF token), and `code_challenge`.
7. **Open browser** — launches the system browser to the authorization URL.
8. **User authenticates** — the user logs in and grants consent in the browser.
9. **Browser redirects** — the authorization server redirects to the callback server at `http://127.0.0.1:<random-port>/mcp/oauth/callback?code=...&state=...`.
10. **Validate state** — the callback server verifies the `state` parameter matches the one sent (CSRF protection).
11. **Exchange code for tokens** — sends the authorization code, `code_verifier`, and `redirect_uri` to the token endpoint. Receives `access_token`, `refresh_token`, and `expires_in`.
12. **Save credentials** — stores the tokens in the OS keychain. When no keychain is available, the agent uses its file store instead.
13. **Stop callback server** — shuts down the local HTTP server.

On subsequent agent runs, the stored access token is used automatically. If expired, the refresh token is used to obtain a new access token without user interaction.

### `agent mcp auth logout`

Clears stored OAuth tokens and client registration data for a server. Does not modify config.toml.

```nu
agent mcp auth logout my-server
```

Example output:

```
Cleared credentials for 'my-server'
```

If no credentials are stored for the server:

```
No stored credentials for 'my-server'
```

### `agent mcp auth status`

Shows authentication status for all configured MCP servers as a table.

```nu
agent mcp auth status
```

Example output:

```
╭───────┬───────────┬──────────────────────────────────────────╮
│ server│ auth_type │ status                                   │
├───────┼───────────┼──────────────────────────────────────────┤
│ c5t   │ none      │ no auth required                         │
│ nu    │ bearer    │ static token (from config)                │
│ gh    │ oauth     │ authenticated (token valid)               │
│ gitlab│ oauth     │ authenticated (token expired — will       │
│       │           │ refresh)                                  │
│ jira  │ oauth     │ not authenticated (run: agent mcp auth    │
│       │           │ login jira)                                │
╰───────┴───────────┴──────────────────────────────────────────╯
```

Status values:

| Status | Meaning |
|--------|---------|
| `no auth required` | Server uses `auth: { type: "none" }` |
| `static token (from config)` | Server uses `auth: { type: "bearer" }` |
| `authenticated (token valid)` | OAuth token is stored and not expired |
| `authenticated (token expired — will refresh)` | OAuth token is expired; auto-refresh on next use |
| `not authenticated (run: ...)` | OAuth server with no stored credentials |

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 Unauthorized` | Missing or invalid credentials | Run `agent mcp auth login <name>` |
| `403 Forbidden` | Insufficient OAuth scopes | Re-login with additional scopes in the `auth.scope` config field |
| `Callback port in use` | Another process is using the callback port. Stop it or configure a different `redirect-uri`. |
| `Token expired` | Refresh token also expired | Run `agent mcp auth login <name>` to re-authenticate |
| `OAuth requires HTTP or SSE transport` | OAuth configured on stdio server | Change transport to `sse` or `http` in config.toml |

## Output contract

- `stdout`: final machine-readable Nushell record output only.
- `stderr`: interactive UX/progress output (spinner while busy, tool progress, warnings, completion).

This keeps pipelines stable while preserving interactive feedback.

## TUI transcript markdown and fenced code

When running in TUI mode, assistant markdown is projected into transcript-friendly
lines. Each markdown element maps to a dedicated style. Colors are named; the
concrete RGB values come from the active theme (`--theme`, default
`catppuccin-mocha`).

| Element | Style |
|---------|-------|
| Heading level 1-6 | bold, one color per level: red, peach, yellow, green, sapphire, lavender |
| Table header cells | bold blue (the generic heading style) |
| Table data cells | default text color |
| Bold / italic / bold italic | red, with the matching bold/italic modifier |
| Block quote text and `│` prefix | pink |
| Link label | lavender |
| Link URL suffix | blue, italic, underline |
| Inline code | green, no dim |
| List markers (`•`, `1.`) | teal |
| Code keyword / type / function | mauve / yellow / blue |
| Code variable / constant | text / peach |
| Code string / number / operator | green / peach / sky |
| Code punctuation / comment | overlay2 / overlay2 italic |

- Supported markdown constructs (for example headings, emphasis, lists,
  blockquotes, and code fences) are rendered with deterministic formatting.
- Fenced code blocks with recognized languages use token-level syntax styles in
  the transcript.
- Unknown or unsupported fence languages fall back to plain code rendering
  (stable, readable, and non-panicking).

The fallback behavior keeps transcript output predictable even when model output
contains malformed markdown or uncommon language identifiers.

In TTY mode the same elements are approximated with ANSI-16 SGR codes, because
truecolor is not guaranteed. The codes are:

| Element | SGR |
|---------|-----|
| Heading level 1-6 | `1;31`, `1;33`, `1;33`, `1;32`, `1;36`, `1;95` |
| Table header cells | `1;34` |
| Bold / italic / bold italic | `1;31` / `3;31` / `1;3;31` |
| Block quote | `35` |
| Link label / URL suffix | `95` / `3;4;34` |
| Inline code | `32` |
| List markers | `36` |
| Code keyword / type / function | `35` / `33` / `34` |
| Code variable / constant | `39` / `33` |
| Code string / number / operator | `32` / `33` / `36` |
| Code punctuation / comment | `90` / `3;90` |

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
