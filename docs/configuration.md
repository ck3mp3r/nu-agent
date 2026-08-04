# Configuration

`nu-agent` reads config from Nushell plugin config:

```nu
$env.config.plugins.agent = {
  models: {
    default: {
      model: "ollama-cloud/glm-5.2"
      temperature: 0.3
      max_tokens: 8192
      max_context_tokens: 128000
      max_tool_turns: 20
      max_tool_result_bytes: 20000
      max_tool_calls_per_subturn: 10
      read_timeout_secs: 30
      max_retries: 3
      retry_base_delay_ms: 1000
    }
    heavy: {
      model: "ollama-cloud/claude-opus-4"
      temperature: 0.7
      max_tokens: 16384
      max_context_tokens: 200000
      max_tool_turns: 30
    }
    light: {
      model: "ollama-cloud/qwen3:8b"
      temperature: 0.1
      max_tokens: 4096
      max_context_tokens: 32768
    }
  }
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

- `models.heavy` — model role for heavy/expensive tasks
- `models.light` — model role for lightweight tasks (e.g. compaction summarization)
- `mcp` — MCP server configuration
- `compaction` — conversation compaction settings
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

The `models` map defines named model roles. Each role is a **`ModelRoleConfig`** record — a set of per-role overrides for model selection and generation parameters:

```nu
models: {
  default: {
    model: "ollama-cloud/glm-5.2"
    temperature: 0.3
    max_tokens: 8192
    max_context_tokens: 128000
  }
  heavy: {
    model: "ollama-cloud/claude-opus-4"
    temperature: 0.7
    max_tokens: 16384
    max_context_tokens: 200000
    max_tool_turns: 30
  }
  light: {
    model: "ollama-cloud/qwen3:8b"
    temperature: 0.1
    max_tokens: 4096
    max_context_tokens: 32768
  }
}
```

### ModelRoleConfig field reference

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `model` | String (required) | Provider/model identifier (e.g. `"ollama-cloud/qwen3:8b"`) | — |
| `temperature` | Option\<f64\> | Response randomness, 0.0–2.0 | env / provider / model default |
| `max_tokens` | Option\<u32\> | Maximum tokens to generate | env / provider / model default |
| `max_context_tokens` | Option\<u32\> | Context window size in tokens; drives compaction threshold | 128,000 |
| `max_output_tokens` | Option\<u32\> | Maximum output tokens | env / provider / model default |
| `max_tool_turns` | Option\<u32\> | Maximum tool execution turns per conversation turn | env / provider / model default |
| `max_tool_result_bytes` | Option\<usize\> | Truncation limit for tool results in bytes; 0 = unlimited | 20,000 |
| `max_tool_calls_per_subturn` | Option\<usize\> | Maximum tool calls in a single LLM response; 0 = unlimited | 25 |
| `model_context_tokens` | Option\<usize\> | Approximate context window for in-session token warnings | env / provider / model default |
| `context_warning_threshold` | Option\<f32\> | Fraction of `model_context_tokens` at which to warn, 0.0–1.0 | 0.6 |
| `additional_params` | Option\<json\> | Provider-specific parameters forwarded verbatim to the completion request body (see [Additional Parameters](#additional-parameters)) | env / provider / model default |
| `read_timeout_secs` | Option\<u64\> | HTTP read timeout in seconds for inference API and MCP connections; 0 = disable | 120 |
| `max_retries` | Option\<u8\> | Retry attempts for transient errors | 3 |
| `retry_base_delay_ms` | Option\<u64\> | Base backoff in ms, doubles each attempt, capped at 30s | 1,000 |

All fields except `model` are optional. When omitted, the value is inherited from the next level in the [precedence chain](#precedence).

### Per-role settings example

Different roles can have completely different models and parameters. Here the `heavy` role uses a powerful model with high token limits and more tool turns, while `light` uses a small local model with conservative settings:

```nu
$env.config.plugins.agent = {
  models: {
    default: {
      model: "ollama-cloud/glm-5.2"
      temperature: 0.3
      max_tokens: 8192
      max_context_tokens: 128000
      max_tool_turns: 20
    }
    heavy: {
      model: "ollama-cloud/claude-opus-4"
      temperature: 0.7
      max_tokens: 16384
      max_context_tokens: 200000
      max_tool_turns: 30
      read_timeout_secs: 60
    }
    light: {
      model: "ollama-cloud/qwen3:8b"
      temperature: 0.1
      max_tokens: 4096
      max_context_tokens: 32768
      max_tool_turns: 5
    }
  }
  providers: { ... }
}
```

When a persona sets `model: heavy`, it gets the Claude Opus config with 200k context and 30 tool turns. When a persona sets `model: light`, it gets the Qwen config with 32k context and only 5 tool turns. The `default` role is the fallback when no `model:` field is set.

### Resolution rules

When an agent persona has a `model:` front matter field, it is resolved as follows:

1. **`model: heavy`** (no slash, matches a key in `models`) → resolves to the `ModelRoleConfig` at `models.heavy`
2. **`model: provider/model`** (contains a slash) → used as-is (literal model identifier); all other parameters inherit from `models.default`
3. **`model: foo`** (no slash, not a key in `models`) → **error** at startup
4. **No `model:` field** → falls back to `models.default`
5. **CLI `--model provider/model`** → overrides everything, used as-is

### Precedence

Highest to lowest:

1. **CLI flags** — `--model`, `--temperature`, `--max-tokens`, etc. override all config
2. **Persona front matter** — `model:` field selects a role or literal model
3. **Role-level config** — the `ModelRoleConfig` record for the selected role (e.g. `models.heavy`)
4. **Model-level config** — `providers.<name>.models.<name>` fields, applied in `PluginConfig::resolve_model()`
5. **Environment variables** — `AGENT_TEMPERATURE`, `AGENT_MAX_TOKENS`, etc.
6. **Built-in defaults** — hardcoded fallbacks in `ModelRoleConfig` and runtime `Config`

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
- `AGENT_A2A_PORT` — A2A agent port (0 = random, >0 = fixed, default: not set)
- `{PROVIDER}_API_KEY` (for providers with direct env naming, e.g. `OPENAI_API_KEY`)

There is no `AGENT_MODEL`. Set the default model in plugin config.

## Provider examples

```nu
$env.config.plugins.agent = {
  models: { default: { model: "openai/gpt-4o" } }
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
  models: { default: { model: "anthropic/claude-sonnet-4-6" } }
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
  models: { default: { model: "ollama/gemma4:31b" } }
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
  models: { default: { model: "ollama-remote/gemma4:31b" } }
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
  models: { default: { model: "together/meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo" } }
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

By default the HTTP client uses a 30-second read timeout (fires only when no bytes are received — safe for long but active streaming responses). Override per role in the model config:

```nu
$env.config.plugins.agent = {
  models: {
    default: {
      model: "..."
      read_timeout_secs: 60   # increase for slow providers
      # read_timeout_secs: 0  # disable entirely
    }
  }
  providers: { ... }
}
```

## Compaction

Configure conversation compaction via the optional `compaction` block:

```nu
$env.config.plugins.agent = {
  # ...existing config...
  models: {
    default: {
      model: "..."
      max_context_tokens: 200000    # MUST match your model's actual context window
    }
  }
  compaction: {
    strategy: "sliding_summary"              # or sliding_window, token_truncate
    keep_recent: 10                          # minimum-message guard for sliding_summary; last N kept for sliding_window
    token_budget: 4000                       # for token_truncate only
    proactive_threshold_pct: 0.80            # 0.0-1.0, fraction of max_context_tokens before compaction fires (default: 0.80)
  }
}
```

All compaction fields are optional — defaults are used when omitted. **`max_context_tokens` is not part of the `compaction` block** — it is a `ModelRoleConfig` field on the active role, but it directly controls when compaction fires. The default is `128_000`. If your model has a larger context window (e.g. 200k), set this explicitly or compaction will trigger too early.

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

## MCP Authentication

MCP servers can require authentication. Configure it per-server under the `auth` field inside each `mcp.<server>` block.

### Auth types

Three auth types are supported:

| Type | Description |
|------|-------------|
| `none` | No authentication (default) |
| `bearer` | Static bearer token from config |
| `oauth` | OAuth 2.0 authorization-code flow with PKCE |

### McpAuthConfig field reference

| Field | Type | Required | Applies to | Description |
|-------|------|----------|------------|-------------|
| `type` | string | **yes** | all | One of `none`, `bearer`, `oauth` |
| `token` | string | **yes** | `bearer` | Static bearer token value |
| `client-id` | string | no | `oauth` | OAuth client ID. Omit for dynamic client registration |
| `client-secret` | string | no | `oauth` | OAuth client secret (optional, for confidential clients) |
| `scope` | string | no | `oauth` | Space-separated OAuth scopes |
| `redirect-uri` | string | no | `oauth` | Custom redirect URI (default: `http://127.0.0.1:<random-port>/mcp/oauth/callback`) |

### Examples

**No auth (default):**

```nu
$env.config.plugins.agent = {
  mcp: {
    my-server: {
      transport: "sse"
      url: "http://localhost:8080/mcp"
      auth: { type: "none" }
    }
  }
  models: { default: { model: "..." } }
  providers: { ... }
}
```

Omitting the `auth` field entirely is equivalent to `auth: { type: "none" }`.

**Bearer token:**

```nu
$env.config.plugins.agent = {
  mcp: {
    my-server: {
      transport: "sse"
      url: "http://localhost:8080/mcp"
      auth: {
        type: "bearer"
        token: $env.MY_MCP_TOKEN
      }
    }
  }
  models: { default: { model: "..." } }
  providers: { ... }
}
```

**OAuth with static client ID:**

```nu
$env.config.plugins.agent = {
  mcp: {
    my-server: {
      transport: "sse"
      url: "http://localhost:8080/mcp"
      auth: {
        type: "oauth"
        client-id: "my-client"
        client-secret: $env.MCP_CLIENT_SECRET
        scope: "read write"
        redirect-uri: "http://127.0.0.1:19876/mcp/oauth/callback"
      }
    }
  }
  models: { default: { model: "..." } }
  providers: { ... }
}
```

**OAuth with dynamic client registration:**

```nu
$env.config.plugins.agent = {
  mcp: {
    my-server: {
      transport: "sse"
      url: "http://localhost:8080/mcp"
      auth: {
        type: "oauth"
        scope: "read write"
      }
    }
  }
  models: { default: { model: "..." } }
  providers: { ... }
}
```

When `client-id` is omitted, the agent performs dynamic client registration with the MCP server's OAuth endpoint. The redirect URI defaults to `http://127.0.0.1:<random-port>/mcp/oauth/callback`.

### Backwards compatibility

The legacy `headers` field still works for setting `Authorization` headers:

```nu
$env.config.plugins.agent = {
  mcp: {
    my-server: {
      transport: "sse"
      url: "http://localhost:8080/mcp"
      headers: {
        Authorization: $"Bearer ($env.MY_MCP_TOKEN)"
      }
    }
  }
}
```

If both `auth` and `headers.Authorization` are set, the `auth` field takes precedence and a warning is logged.

### Validation rules

- `oauth` requires HTTP or SSE transport (not stdio)
- `bearer` token must not be empty
- Unknown auth types are rejected at parse time

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

Set it per role in the `ModelRoleConfig`:

```nu
$env.config.plugins.agent = {
  models: {
    default: {
      model: "anthropic/claude-sonnet-4-6"
      additional_params: {
        output_config: { effort: "medium" }
      }
    }
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
  models: {
    default: {
      model: "anthropic/claude-sonnet-4-6"
      additional_params: {
        output_config: { effort: "medium" }
      }
    }
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
