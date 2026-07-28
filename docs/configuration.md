# Configuration

`nu-agent` reads config from Nushell plugin config:

```nu
$env.config.plugins.agent = {
  models: {
    default: "provider/model"             # required
    heavy: "provider/model"               # optional
    light: "provider/model"               # optional
  }
  temperature: 0.7                        # optional, 0.0–2.0
  max_tokens: 4096                        # optional
  max_context_tokens: 128000              # optional
  max_output_tokens: 4096                 # optional
  max_tool_turns: 20                      # optional
  max_tool_result_bytes: 20000            # optional, 0 = unlimited
  model_context_tokens: 200000            # optional, enables context warnings
  context_warning_threshold: 0.8         # optional, 0.0–1.0
  max_retries: 3                          # optional
  retry_base_delay_ms: 1000              # optional
  read_timeout_secs: 30                   # optional
  max_tool_calls_per_subturn: 10          # optional
  providers: {
    provider_name: {
      api_key: "..."                      # optional
      base_url: "https://..."            # optional
      provider: "openai"                 # optional
      models: {
        "model-name": {}
      }
    }
  }
}
```

Required top-level fields:

- `models` (with at least `models.default`)
- `providers`

Optional top-level fields:

- `models.heavy` — model used for heavy/expensive tasks
- `models.light` — model used for lightweight tasks (e.g. compaction summarization)
- `mcp` — MCP server configuration
- `compaction` — conversation compaction settings
- `additional_params` — provider-specific parameters forwarded verbatim to the completion request body (see [Additional Parameters](#additional-parameters))
- `temperature` — response randomness (0.0–2.0)
- `max_tokens` — maximum tokens to generate
- `max_context_tokens` — context window size in tokens (default: 128_000 — set this to match your model)
- `max_output_tokens` — maximum output tokens
- `max_tool_turns` — maximum tool execution turns per conversation turn
- `max_tool_calls_per_subturn` — maximum tool calls in a single LLM response (default: 10)
- `max_tool_result_bytes` — truncation limit for tool results in bytes (default: 20_000, 0 = disable)
- `read_timeout_secs` — HTTP read timeout in seconds for inference API and MCP HTTP connections (default: 120). Set to 0 to disable.
- `max_retries` — retry attempts for transient errors (default: 3)
- `retry_base_delay_ms` — base backoff in ms, doubles each attempt, capped at 30s (default: 1000)
- `model_context_tokens` — approximate context window for in-session token warnings (no auto-detection)
- `context_warning_threshold` — fraction of `model_context_tokens` at which to warn (default: 0.6)
- `preamble` — system preamble prepended before prompt/context
- `a2a_enabled` — enable A2A (agent-to-agent) communication via JSON-RPC 2.0 over HTTP (default: `false`)
- `session_store` — session store backend configuration (optional, defaults to SQLite)

## Session Store

Configure the session store backend:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  session_store: {
    type: "sqlite"        # "sqlite" or "jsonl"
    path: "/custom/path"  # optional custom path
  }
}
```

- `type` — backend type: `"sqlite"` (default) or `"jsonl"`
- `path` — optional custom path for the store (defaults to XDG cache directory)

### Environment Variable

- `AGENT_SESSION_STORE_TYPE` — set to `sqlite` or `jsonl` to override the default

### CLI Flag

- `--store sqlite|jsonl` — available on `agent`, `agent session list`, `agent session inspect`, and `agent session clear`

### Precedence

CLI flag > environment variable > config file > built-in default (SQLite)

### Notes

- SQLite is the default backend
- JSONL path defaults to `~/.cache/nu-agent/sessions/`

## Model Format

All models use `provider/model` format:

- `ollama/gemma4:31b`
- `openai/gpt-4o`
- `anthropic/claude-sonnet-4-6`
- `github-copilot/claude-sonnet-4.6`

## Model Roles

The `models` map defines named model roles that agents can reference by name:

```nu
models: {
  default: "openai/gpt-4o"          # required — fallback when no model: field
  heavy: "anthropic/claude-sonnet-4-6"  # optional — for complex reasoning
  light: "ollama/gemma4:31b"        # optional — for quick/cheap tasks
}
```

### Resolution rules

When an agent persona has a `model:` front matter field, it is resolved as follows:

1. **`model: heavy`** (no slash, matches a key in `models`) → resolves to the value of `models.heavy`
2. **`model: provider/model`** (contains a slash) → used as-is (literal model identifier)
3. **`model: foo`** (no slash, not a key in `models`) → **error** at startup
4. **No `model:` field** → falls back to `models.default`
5. **CLI `--model provider/model`** → overrides everything, used as-is

### Precedence

Highest to lowest:

1. CLI `--model` flag — overrides all model resolution
2. Persona front matter `model:` field — role name or literal
3. `models.default` — fallback when no `model:` field is set
4. Model-level config (`providers.<name>.models.<name>` fields)
5. Environment variables
6. Top-level `$env.config.plugins.agent` fields
7. Built-in defaults

## Environment Variables

- `AGENT_BASE_URL`
- `AGENT_TEMPERATURE`
- `AGENT_MAX_TOKENS`
- `AGENT_MAX_CONTEXT_TOKENS`
- `AGENT_MAX_OUTPUT_TOKENS`
- `AGENT_MAX_TOOL_TURNS`
- `AGENT_MAX_TOOL_RESULT_BYTES` — max bytes returned per tool call before truncation; LLM is told to use `read` with offset/limit for the rest (default: 20000; `0` = unlimited)
- `AGENT_MAX_TOOL_CALLS_PER_SUBTURN` — max tool calls allowed in a single LLM response; guards against models ignoring `parallel_tool_calls: false` (default: 10; `0` = unlimited)
- `AGENT_MODEL_CONTEXT_TOKENS` — approximate context window size in tokens for the configured model; enables context-usage warnings (default: none; no warning emitted until set)
- `AGENT_CONTEXT_WARNING_THRESHOLD` — fraction of `AGENT_MODEL_CONTEXT_TOKENS` at which to emit a context-usage warning, `0.0`–`1.0` (default: `0.6`)
- `AGENT_MAX_RETRIES` — retry attempts for transient errors (default: 3)
- `AGENT_RETRY_BASE_DELAY_MS` — base backoff in ms, doubles each attempt, capped at 30s (default: 1000)
- `AGENT_READ_TIMEOUT_SECS` — HTTP read timeout in seconds for inference API and MCP connections (default: 120). Set to 0 to disable.
- `AGENT_A2A_ENABLED` — enable A2A agent-to-agent communication (default: `false`)
- `{PROVIDER}_API_KEY` (for providers with direct env naming, e.g. `OPENAI_API_KEY`)

There is no `AGENT_MODEL`. Set the default model in plugin config.

## Provider examples

```nu
$env.config.plugins.agent = {
  models: { default: "openai/gpt-4o" }
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
  models: { default: "anthropic/claude-sonnet-4-6" }
  providers: {
    anthropic: {
      api_key: $env.ANTHROPIC_API_KEY
      models: {
        "claude-sonnet-4-6": {}
      }
    }
  }
}
```

```nu
$env.config.plugins.agent = {
  models: { default: "ollama/gemma4:31b" }
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
  models: { default: "ollama-remote/gemma4:31b" }
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
  models: { default: "together/meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo" }
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
  models: { default: "..." }
  providers: { ... }
}
```

## Compaction

Configure conversation compaction via the optional `compaction` block:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  max_context_tokens: 200000               # MUST match your model's actual context window
  compaction: {
    strategy: "sliding_summary"              # or sliding_window, token_truncate
    keep_recent: 10                          # minimum-message guard for sliding_summary; last N kept for sliding_window
    token_budget: 4000                       # for token_truncate only
    proactive_threshold_pct: 0.80            # 0.0-1.0, fraction of max_context_tokens before compaction fires (default: 0.80)
  }
}
```

All compaction fields are optional — defaults are used when omitted. **`max_context_tokens` is not part of the `compaction` block** — it is a top-level `Config` field, but it directly controls when compaction fires. The default is `128_000`. If your model has a larger context window (e.g. 200k), set this explicitly or compaction will trigger too early.

### Strategies

- `sliding_summary` (default) — LLM summarizes **all** messages into a single system message. Nothing is kept verbatim. `keep_recent` acts only as a minimum-message guard: compaction is skipped if total messages ≤ `keep_recent`.
- `sliding_window` — drops old messages, keeps only the last `keep_recent` verbatim. No LLM call.
- `token_truncate` — keeps newest messages within a token budget (chars/4 estimate). Requires `token_budget`. No LLM call.

### Validation rules

- `proactive_threshold_pct` must be between 0.0 and 1.0.
- `keep_recent` must be greater than 0.
- `token_budget` is required when using `token_truncate`.

### Precedence

CLI flags override plugin config, which overrides built-in defaults.

## Agents

Configure built-in persona availability via the optional `agents` block:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  agents: {
    planner: "disabled"   # set to "disabled" to disable; omit or any other value = enabled
    maker: "disabled"     # same
    default: "planner"    # default persona at startup (default: "planner")
    fallback: "coder"     # optional: .agents/<name>.md persona when default built-in is disabled
  }
}
```

All `agents` fields are optional. Defaults: `planner` enabled, `maker` enabled, `default` = `"planner"`, no `fallback`.

`fallback` is only used when `default` names a disabled built-in persona. It must be the name of a file under `.agents/` or `$XDG_CONFIG_HOME/nu-agent/agents/`.

## Additional Parameters

`additional_params` forwards a record of provider-specific keys verbatim into the top-level HTTP request body. The record is flattened — each key becomes a top-level field alongside `model`, `messages`, etc.

```nu
$env.config.plugins.agent = {
  models: { default: "anthropic/claude-sonnet-4-6" }
  additional_params: {
    output_config: { effort: "medium" }
  }
  providers: { ... }
}
```

This works with all providers (Anthropic, OpenAI, Copilot, Ollama, OpenAI-compatible).

### Gotchas

- **Must be a record** — arrays, strings, and scalars are rejected at parse time.
- **Do not shadow typed fields** — keys like `model`, `max_tokens`, or `temperature` already have typed fields. Duplicating them via `additional_params` produces duplicate JSON keys with undefined behavior.
- **Applied to every turn** — the value is included in every completion request for the agent's lifetime.

### Controlling thinking depth on Anthropic Sonnet 4.6

Sonnet 4.6 uses **adaptive thinking** — the model decides when to think based on the request. The correct way to control thinking depth is the `effort` parameter inside `output_config`, not `thinking: {type: "disabled"}` (which has no effect on this model).

Anthropic recommends `medium` effort as the default for agentic workloads — it gives a good balance of speed, cost, and quality. Use `low` for latency-sensitive tasks or `high`/`max` when quality is the priority.

```nu
$env.config.plugins.agent = {
  models: { default: "anthropic/claude-sonnet-4-6" }
  additional_params: {
    output_config: { effort: "medium" }
  }
  providers: {
    anthropic: {
      api_key: $env.ANTHROPIC_API_KEY
      models: {
        "claude-sonnet-4-6": {}
      }
    }
  }
}
```

Effort levels: `low` (fastest, fewest tokens) · `medium` (recommended for agents) · `high` (default if omitted) · `max` (highest capability)
