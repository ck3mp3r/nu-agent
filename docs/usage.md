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
