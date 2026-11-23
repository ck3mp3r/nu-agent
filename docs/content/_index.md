+++
title = "nu-agent documentation"
description = "Documentation for the nu-agent Nushell plugin."
sort_by = "weight"
template = "index.html"

[extra]
lead = "Run an LLM agent from a Nushell pipeline: pipe a prompt in, get structured records out, with tools, sessions, and MCP wiring under your control."
url = "/docs/getting-started/quick-start/"
url_button = "Read the docs"
repo_url = "https://github.com/ck3mp3r/nu-agent"
repo_license = "MIT"
repo_version = "GitHub v0.1.0"

[[extra.list]]
title = "Plugin pipeline input"
content = "A Nushell plugin, so a prompt is pipeline data: a plain string, or a record carrying a prompt and optional context."

[[extra.list]]
title = "Session model"
content = "Every run is a session you can continue; sessions, usage counters, and transcripts persist in one local store."

[[extra.list]]
title = "MCP tooling"
content = "Built-in filesystem, search, HTTP, shell, and AST tools out of the box; add MCP servers for everything else."

[[extra.list]]
title = "Nix build"
content = "The plugin and the CLI come from one flake, and the devshell carries the toolchain, the test runner, and zola."
+++

nu-agent runs an LLM agent from a Nushell pipeline. The pages below cover
using it, wiring it into other systems, the architecture it follows, and the
reference material behind both.

## Getting started

- [Quick Start](@/docs/getting-started/quick-start.md) — build the plugin, install it, configure a model, and make your first run.

## User guide

- [Usage](@/docs/user/usage.md) — commands, examples, sessions, hooks, and TUI transcript rendering behavior.
- [Configuration](@/docs/user/configuration.md) — config.toml structure, secret storage, the models cache, and resolution precedence.

## Integration

- [MCP](@/docs/integration/mcp.md) — MCP servers, transports, discovery, filtering, and authentication.

## Architecture

- [Design Choices](@/docs/architecture/design-choices.md) — the patterns this workspace follows, each marked as enforced or as direction.
- [Target Architecture](@/docs/architecture/target-architecture.md) — the acceptance criteria the crates must satisfy, and the checks that enforce them.
- [Bus Channels](@/docs/architecture/bus-channels.md) — the delivery contract of every kernel bus channel.
- [Contribution guardrails](@/docs/architecture/contribution-guardrails.md) — invariants for changes to tool handling, permission UX, and transcript rendering.

## Reference

- [Code Coverage](@/docs/reference/coverage.md) — measuring coverage with cargo-llvm-cov, and the workspace's test profiles.

The TUI's `/help` covers the in-terminal help: keybindings, slash commands, and the panels the current build exposes.
