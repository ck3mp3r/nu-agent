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

# Verbose diagnostics to stderr
"debug this" | agent --verbose
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
- `--verbose` / `-v`
