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
- Exposed/callable MCP tool names are namespaced as `<server_key>::<raw_tool_name>`.
  - `server_key` is the key under `mcp.<server_key>` in plugin config.
- `--mcp-tools` filters discovered tools for that single run.

## Transport Rules

- `transport: "stdio"` requires `command`
- `transport: "sse" | "http" | "streamable-http"` requires `url`
- optional fields: `args`, `env`, `headers`

## `--mcp-tools`

Use glob patterns to restrict exposed MCP tools:

```nu
"check open prs" | agent --mcp-tools ["gh::*"]
"cluster + prs" | agent --mcp-tools ["k8s::*" "gh::list_*"]
```

If omitted, all discovered MCP tools are exposed.

## Collision prevention

If two MCP servers expose the same raw tool name (for example both expose `list_prs`),
the exposed names remain unique via server namespacing:

- `gh::list_prs`
- `altgh::list_prs`

This avoids cross-server collisions in discovery, filtering, and tool execution.

## Tool precedence

If a closure tool and an MCP tool share the same exposed name, closure tools take precedence during execution.

- precedence order: closure tool, then MCP tool
- use distinct names to avoid accidental shadowing

## Migration note

Previous behavior exposed raw MCP tool names directly (e.g. `list_prs`).

Current behavior requires namespaced names (e.g. `gh::list_prs`) for:

- `--mcp-tools` filters
- LLM tool-call names routed through the tool handler

Update any existing filters/prompts that referenced raw MCP tool names.

## Reserved delimiter

`::` is reserved as MCP tool namespace delimiter.

- `mcp.<server_key>` must not include `::`
- MCP raw tool names containing `::` are rejected at discovery time
