# nu-agent

A Nushell plugin providing AI agent capabilities via rig-rs, with support for multiple LLM providers including GitHub Copilot, OpenAI, Anthropic, and Ollama.

## Features

- **Multiple LLM Providers**: GitHub Copilot, OpenAI, Anthropic, Ollama
- **Full rig.rs Integration**: Agents, tools, and streaming support (where available)
- **GitHub Copilot Support**: Complete rig.rs provider with proper API headers
- **Flexible Configuration**: Per-provider settings with model-specific options
- **Pipeline Integration**: Works seamlessly with Nushell pipelines

## Installation

```bash
# Build the plugin
cargo build --release

# Register with Nushell
plugin add target/release/nu_plugin_agent

# Restart Nushell or run:
# plugin use nu_plugin_agent
```

## Configuration

Configure the plugin in your Nushell config (`$nu.config-path`):

### GitHub Copilot (requires Copilot subscription)

```nu
$env.config.plugins.agent = {
  model: "github-copilot/claude-sonnet-4.5"
  providers: {
    github-copilot: {
      api_key: $env.GITHUB_TOKEN  # Uses gh CLI OAuth token
      base_url: "https://api.individual.githubcopilot.com"  # Required for personal accounts
      models: {
        "claude-sonnet-4.5": {}
        "gpt-4o": {}
        "gpt-4o-mini": {}
      }
    }
  }
}
```

**For GitHub Actions workflows:**
```nu
# Default endpoint works in Actions (no base_url override needed)
$env.config.plugins.agent = {
  model: "github-copilot/claude-sonnet-4.5"
  providers: {
    github-copilot: {
      api_key: $env.GITHUB_TOKEN  # Actions GITHUB_TOKEN
      models: {
        "claude-sonnet-4.5": {}
      }
    }
  }
}
```

**Requirements:**
1. Active GitHub Copilot subscription
2. GitHub CLI (`gh`) authenticated with `copilot` scope:
   ```bash
   gh auth login --scopes "repo,read:org,gist,workflow,copilot"
   ```
3. `GITHUB_TOKEN` environment variable set to `gh auth token` output

**Finding your endpoint:**
```bash
# Query your Copilot endpoint (for personal accounts)
gh api graphql -f query='{ viewer { copilotEndpoints { api } } }'
# Use the returned API URL as base_url in config
```

### OpenAI

```nu
$env.config.plugins.agent = {
  model: "openai/gpt-4"
  providers: {
    openai: {
      api_key: $env.OPENAI_API_KEY
      models: {
        "gpt-4": {}
        "gpt-4-turbo": {}
        "gpt-3.5-turbo": {}
      }
    }
  }
}
```

### Anthropic

```nu
$env.config.plugins.agent = {
  model: "anthropic/claude-3-5-sonnet-20241022"
  providers: {
    anthropic: {
      api_key: $env.ANTHROPIC_API_KEY
      models: {
        "claude-3-5-sonnet-20241022": {}
        "claude-3-opus-20240229": {}
      }
    }
  }
}
```

### Ollama (local)

```nu
$env.config.plugins.agent = {
  model: "ollama/llama3.2"
  providers: {
    ollama: {
      base_url: "http://localhost:11434"
      models: {
        "llama3.2": {}
        "codellama": {}
      }
    }
  }
}
```

## Usage

```nu
# Simple prompt
"What is Rust?" | agent

# Output structure:
# {
#   response: "...",
#   model: "claude-sonnet-4.5",
#   provider: "github-copilot",
#   timestamp: "2026-04-24T22:29:28Z"
# }

# Use with pipelines
ls | to json | $"Summarize this file list: ($in)" | agent
```

## Troubleshooting

### GitHub Copilot: "Access to this endpoint is forbidden"

**Solution:** Re-authenticate GitHub CLI with `copilot` scope:
```bash
gh auth login --scopes "repo,read:org,gist,workflow,copilot"
```

Verify the scope is present:
```bash
gh auth status
# Should show: Token scopes: '...copilot...'
```

### GitHub Copilot: "Personal Access Tokens are not supported"

GitHub Copilot requires OAuth tokens from `gh auth`, NOT Personal Access Tokens (PATs). Use `gh auth token` for authentication.

## Development

See [AGENTS.md](./AGENTS.md) for development rules and TDD workflow.

### Running tests

```bash
cargo test
```

### Development build

```bash
cargo build
plugin add target/debug/nu_plugin_agent
```
