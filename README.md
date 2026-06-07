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
      base_url: "http://127.0.0.1:11434"
      models: {
        "gemma4:31b": {}
      }
    }
  }
  compaction: {               # optional
    strategy: "sliding_summary"
    threshold: 100
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
- `docs/usage.md` - commands, examples, and TUI transcript rendering behavior
- `docs/development.md` - build, test, lint, architecture references
- `docs/event-architecture.md` - typed event-driven harness enforcement (checkpoints, subscribers, policy modes)
- `docs/handler-decomposition-contract.md` - handler module boundaries and migration plan (R1)
- `docs/contribution-guardrails.md` - R7/R8/R9 contributor guardrails for permission/TUI/tool-handler changes

## Module/Test Layout Convention

- Single-file module: `foo.rs` + `foo_test.rs`
- Multi-file module: `foo/mod.rs` + `foo/test.rs`
- Forbidden mixed pattern: `foo.rs` + `foo/test.rs`

## TUI transcript rendering

In TUI mode, assistant markdown is projected into readable transcript lines.
Fenced code blocks use syntax highlighting when the fence language is recognized.
If the language is unknown (or unsupported by the highlighter adapter), rendering
falls back to stable plain code text so transcript readability is preserved.

For development commands, see `docs/development.md`.
