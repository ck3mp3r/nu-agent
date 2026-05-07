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

### `edit` example (search/replace)

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
