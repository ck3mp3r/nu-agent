# Configuration

`nu-agent` reads config from Nushell plugin config:

```nu
$env.config.plugins.agent = {
  model: "provider/model"
  small_model: "provider/model" # optional
  providers: {
    provider_name: {
      api_key: "..."            # optional
      base_url: "https://..."   # optional
      provider_impl: "openai"   # optional
      models: {
        "model-name": {}
      }
    }
  }
}
```

Required top-level fields:

- `model`
- `providers`

Optional top-level fields:

- `small_model`
- `mcp`

## Model Format

All models use `provider/model` format:

- `ollama/gemma4:31b`
- `openai/gpt-4o`
- `anthropic/claude-sonnet-4-20250514`
- `github-copilot/claude-opus-4.6`

## Precedence

Highest to lowest:

1. CLI flags
2. `$env.config.plugins.agent`
3. environment variables
4. built-in defaults

## Environment Variables

- `AGENT_BASE_URL`
- `AGENT_TEMPERATURE`
- `AGENT_MAX_TOKENS`
- `AGENT_MAX_CONTEXT_TOKENS`
- `AGENT_MAX_OUTPUT_TOKENS`
- `AGENT_MAX_TOOL_TURNS`
- `{PROVIDER}_API_KEY` (for providers with direct env naming, e.g. `OPENAI_API_KEY`)

There is no `AGENT_MODEL`. Set the default model in plugin config.

## Provider examples

```nu
$env.config.plugins.agent = {
  model: "openai/gpt-4o"
  providers: {
    openai: {
      api_key: $env.OPENAI_API_KEY
      models: {
        "gpt-4o": {}
      }
    }
  }
}
```

```nu
$env.config.plugins.agent = {
  model: "anthropic/claude-3-5-sonnet-20241022"
  providers: {
    anthropic: {
      api_key: $env.ANTHROPIC_API_KEY
      models: {
        "claude-3-5-sonnet-20241022": {}
      }
    }
  }
}
```

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
