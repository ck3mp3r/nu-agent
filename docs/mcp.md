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
- `--mcp-tools` filters discovered tools for that single run.

## Transport Rules

- `transport: "stdio"` requires `command`
- `transport: "sse" | "http" | "streamable-http"` requires `url`
- optional fields: `args`, `env`, `headers`

## `--mcp-tools`

Use glob patterns to restrict exposed MCP tools:

```nu
"check open prs" | agent --mcp-tools ["gh/*"]
"cluster + prs" | agent --mcp-tools ["k8s/*" "gh/list_*"]
```

If omitted, all discovered MCP tools are exposed.
