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
      provider: "openai"   # optional
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
- `compaction`
- `read_timeout_secs` — HTTP read timeout in seconds (default: 30, set to 0 to disable)

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
      base_url: "http://127.0.0.1:11434"
      models: {
        "gemma4:31b": {}
      }
    }
  }
}
```

Multi-instance example (same provider, different hosts):

```nu
$env.config.plugins.agent = {
  model: "ollama-remote/gemma4:31b"
  providers: {
    ollama: {
      models: {
        "gemma4:31b": {}
      }
    }
    ollama-remote: {
      provider: "ollama"
      base_url: "http://gpu-server:11434"
      models: {
        "gemma4:31b": {}
      }
    }
  }
}
```

### OpenAI-compatible providers

Any provider that implements the OpenAI Chat Completions API (`POST /chat/completions`) works with `provider: "openai"` and a custom `base_url`. Setting `base_url` automatically routes through the Chat Completions API — no additional config needed.

```nu
$env.config.plugins.agent = {
  model: "together/meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo"
  providers: {
    together: {
      provider: "openai"
      api_key: $env.TOGETHER_API_KEY
      base_url: "https://api.together.xyz/v1"
      models: {
        "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo": {}
      }
    }
  }
}
```

This works for Together AI, Groq, OpenRouter, vLLM, LiteLLM, and any other OpenAI-compatible endpoint.

### HTTP timeout

By default the HTTP client uses a 30-second read timeout (fires only when no bytes are received — safe for long but active streaming responses). Override at the top level:

```nu
$env.config.plugins.agent = {
  read_timeout_secs: 60   # increase for slow providers
  # read_timeout_secs: 0  # disable entirely
  model: "..."
  providers: { ... }
}
```

## Compaction

Configure conversation compaction via the optional `compaction` block:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  compaction: {
    strategy: "sliding_summary"              # or sliding_window, token_truncate
    keep_recent: 10                          # messages to keep (default: 10)
    token_budget: 4000                       # for token_truncate only
    proactive_threshold_pct: 0.80            # 0.0-1.0, proactive trigger (default: 0.80)
  }
}
```

All fields are optional — defaults are used when omitted.

### Strategies

- `sliding_summary` (default) — LLM summarizes old messages, keeps recent verbatim window.
- `sliding_window` — drops old messages, keeps only the last N. No LLM call.
- `token_truncate` — keeps newest messages within a token budget (chars/4 estimate). No LLM call.

### Validation rules

- `proactive_threshold_pct` must be between 0.0 and 1.0.
- `keep_recent` must be greater than 0.

### Precedence

CLI flags override plugin config, which overrides built-in defaults.
