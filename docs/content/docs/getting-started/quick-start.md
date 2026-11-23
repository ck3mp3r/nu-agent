+++
title = "Quick Start"
description = "Build nu-agent, install the Nushell plugin, configure a model, and make your first run."
weight = 1
draft = false
template = "docs/page.html"

[extra]
toc = true
+++

## Build and install

nu-agent is a Nushell plugin, so it ships as a separate binary. Build it in
release mode and register it with Nushell:

```bash
cargo build --release
plugin add target/release/nu_plugin_agent
plugin use agent
```

`plugin add` records the binary path in Nushell's plugin registry, and
`plugin use` loads it. The command name is `agent`: the `nu_plugin_` prefix is
what makes the binary a plugin.

## Configure a model

Generate a starter config from your environment:

```bash
agent config init          # creates config.toml (errors if it already exists)
agent config init --force  # overwrite an existing config.toml
```

`agent config init` scans for `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` and any
`AGENT_*` overrides, and writes a `config.toml` reflecting them. The file lives
at `{config base}/nu-agent/config.toml` — `~/.config/nu-agent/config.toml` by
default; a debug build reads `nu-agent-dev/` instead.

Then edit the file and set a model. Two blocks are enough to run against a
local Ollama server:

```toml
[models.default]
model = "ollama/gemma4:31b"

[providers.ollama]
base_url = "http://127.0.0.1:11434"
```

## First run

Pipe a prompt into the agent:

```nu
"explain this repo" | agent
```

The input can be a plain string, or a record with `prompt` and an optional
`context`:

```nu
{ prompt: "Summarize", context: "short bullets" } | agent
```

## Common flags

```nu
# Override the model for one call
"quick answer" | agent --model "ollama/gemma4:31b"

# Quiet mode (suppress non-essential progress UX)
"debug this" | agent --quiet

# Progressive verbosity on the stderr UX
"debug this" | agent -v
"debug this" | agent -vv
"debug this" | agent -vvv

# Load a persona for instructions and configuration
"implement the feature" | agent --agent coder
```

## What's next?

- [Usage](@/docs/user/usage.md) — the full command surface: sessions, hooks, permissions, and the TUI.
- [Configuration](@/docs/user/configuration.md) — config.toml, secret storage, the models cache, and precedence.
- [MCP](@/docs/integration/mcp.md) — wire nu-agent to MCP servers for extra tools.
