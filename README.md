# nu-agent

Nushell plugin for running an LLM agent from pipelines.

## Quick Start

```bash
cargo build --release
plugin add target/release/nu_plugin_agent
plugin use nu_plugin_agent
```

Set plugin config in Nushell:

```nu
$env.config.plugins.agent = {
  model: "ollama/gemma4:31b"
  providers: {
    ollama: {
      base_url: "http://127.0.0.1:11434/v1"
      models: {
        "gemma4:31b": {}
      }
    }
  }
}
```

Use it:

```nu
"explain this repo" | agent
```

## Documentation

- `docs/configuration.md` - config structure, env vars, precedence
- `docs/mcp.md` - MCP servers, discovery, filtering
- `docs/usage.md` - commands and examples
- `docs/development.md` - build, test, lint

For development commands, see `docs/development.md`.
