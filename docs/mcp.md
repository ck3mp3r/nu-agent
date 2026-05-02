# MCP

MCP server configuration is optional and lives in plugin config under `mcp`.

## Example

```nu
$env.config.plugins.agent = {
  mcp: {
    c5t: {
      transport: "sse"
      url: "http://0.0.0.0:3737/mcp"
    }
    nu: {
      transport: "stdio"
      command: "nu-mcp"
      cwd: "/path/to/workspace" # optional stdio cwd override
      args: ["--add-path" "/tmp"]
      env: { GIT_PAGER: "" }
    }
  }
  model: "github-copilot/openai/gpt-5.3-codex"
  providers: {
    "github-copilot": {
      provider_impl: "openai"
      api_key: $env.GITHUB_TOKEN
      base_url: "https://api.individual.githubcopilot.com"
      models: {
        "openai/gpt-5.3-codex": {}
      }
    }
  }
}
```

## Behavior

- If `mcp` is missing or empty, agent runs without MCP.
- Tools are discovered from connected MCP servers at runtime.
- Exposed/callable MCP tool names are namespaced as `<server_key>__<raw_tool_name>`.
  - `server_key` is the key under `mcp.<server_key>` in plugin config.
- `--mcp-tools` filters discovered tools for that single run.

## Transport Rules

- `transport: "stdio"` requires `command`
- `transport: "sse" | "http"` requires `url`
- optional fields: `args`, `env`, `headers`, `cwd` (`stdio` only)

## Stdio working directory behavior

For `transport: "stdio"` servers:

- Child process `current_dir` is resolved deterministically from caller context:
  - absolute `mcp.<server>.cwd`: used as-is, then canonicalized/validated
  - relative `mcp.<server>.cwd`: resolved against the caller cwd, then canonicalized/validated
  - no `mcp.<server>.cwd`: caller cwd is used
- `PWD` is explicitly set to the effective child cwd for compatibility.
- Caller context is preserved in env variables:
  - `NU_AGENT_CALLER_CWD` = canonical caller cwd
  - `NU_AGENT_CALLER_PWD` = canonical caller cwd (compat alias)
- Invalid/missing cwd is an explicit configuration/runtime error (no silent fallback).

For `sse`/`http` transports, cwd behavior is unchanged.

## Tool failure recovery behavior

Tool-call failures are non-fatal for the current agent turn. Instead of aborting, the agent appends
a structured tool result payload that the LLM can consume for retry/replanning.

Failure payload contract:

- `tool_name`
- `tool_call_id`
- `source` (`closure` | `mcp` | `unknown`)
- `error_kind` (`timeout` | `validation` | `runtime` | `transport` | `unknown`)
- `message`
- optional `details`

Typical recovery flow:

1. LLM calls tool with invalid args.
2. Tool returns structured failure payload above.
3. LLM sees payload in tool-result stream and issues corrected tool call.

Fatal errors still remain for unrecoverable command-level failures (for example: invalid top-level
agent config or LLM provider initialization failures).

## `--mcp-tools`

Use glob patterns to restrict exposed MCP tools:

```nu
"check open prs" | agent --mcp-tools ["gh__*"]
"cluster + prs" | agent --mcp-tools ["k8s__*" "gh__list_*"]
```

If omitted, all discovered MCP tools are exposed.

## Collision prevention

If two MCP servers expose the same raw tool name (for example both expose `list_prs`),
the exposed names remain unique via server namespacing:

- `gh__list_prs`
- `altgh__list_prs`

This avoids cross-server collisions in discovery, filtering, and tool execution.

## Tool precedence

If a closure tool and an MCP tool share the same exposed name, closure tools take precedence during execution.

- precedence order: closure tool, then MCP tool
- use distinct names to avoid accidental shadowing

## Migration note

Previous behavior exposed raw MCP tool names directly (e.g. `list_prs`).

Current behavior requires namespaced names (e.g. `gh__list_prs`) for:

- `--mcp-tools` filters
- LLM tool-call names routed through the tool handler

Update any existing filters/prompts that referenced raw MCP tool names.

## Reserved delimiter

`__` is reserved as MCP tool namespace delimiter.

- `mcp.<server_key>` must not include `__`
- MCP raw tool names containing `__` are rejected at discovery time
