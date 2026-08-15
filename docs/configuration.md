# Configuration

`nu-agent` uses a **TOML config file** as its sole source of configuration. The Nushell plugin record is no longer supported — all configuration lives in `config.toml`.

Configuration is organised around three files, all located under the XDG base directories:

| File | Location | Purpose |
|------|----------|---------|
| `config.toml` | `$XDG_CONFIG_HOME/nu-agent/config.toml` | Main configuration: models, providers, MCP, compaction, agents, session store |
| `secrets.json` | `$XDG_DATA_HOME/nu-agent/secrets.json` | Secret store for API keys and OAuth tokens |
| `models.json` | `$XDG_DATA_HOME/nu-agent/models.json` | Local cache of the `models.dev` database |

> On macOS/Linux the XDG defaults are `~/.config`, `~/.local/share`, and `~/.cache`. If `XDG_CONFIG_HOME`/`XDG_DATA_HOME` are set they take precedence.

## 1. config.toml

### Location

`$XDG_CONFIG_HOME/nu-agent/config.toml`

If the file does not exist, `nu-agent` uses built-in defaults (plus environment-variable fallbacks — see [Environment Variable Fallback](#environment-variable-fallback)). It is **not** an error for the file to be absent.

Generate a starter config from your current environment with:

```sh
agent config init          # creates config.toml (errors if it already exists)
agent config init --force  # overwrite an existing config.toml
```

`agent config init` scans for `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` and any `AGENT_*` overrides and writes a `config.toml` reflecting them.

### Top-level structure

```toml
[models.default]
model = "openai/gpt-4o"

[providers.openai]
api_key = "store:openai"

[providers.openai.models.gpt-4o]

[compaction]
strategy = "sliding_summary"

[agents]
planner_enabled = true
maker_enabled = true
default = "planner"
```

Top-level sections:

- `[models.<role>]` — per-role model configuration (at least `models.default`)
- `[providers.<name>]` — provider definitions
- `[compaction]` — conversation compaction settings (optional)
- `[agents]` — built-in persona availability (optional)
- `[session_store]` — session store backend (optional)
- `a2a_enabled` — enable A2A (agent-to-agent) protocol (default: `false`)

### Model roles

The `[models.<role>]` map defines named model roles. Each role is a set of per-role overrides for model selection and generation parameters. At minimum a `default` role must be present.

```toml
[models.default]
model = "openai/gpt-4o"
temperature = 0.3
max_tokens = 8192
max_context_tokens = 128000
max_tool_turns = 20
max_tool_result_bytes = 20000
max_tool_calls_per_subturn = 10
read_timeout_secs = 30
max_retries = 3
retry_base_delay_ms = 1000

[models.heavy]
model = "openai/gpt-4o"
temperature = 0.7
max_tokens = 16384
max_context_tokens = 200000

[models.light]
model = "openai/gpt-4o-mini"
temperature = 0.1
max_tokens = 4096
max_context_tokens = 32768
```

#### ModelRoleConfig field reference

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `model` | String (required) | Provider/model identifier (e.g. `"openai/gpt-4o"`) | — |
| `temperature` | float | Response randomness, 0.0–2.0 | env / provider / model default |
| `max_tokens` | int | Maximum tokens to generate | env / provider / model default |
| `max_context_tokens` | int | Context window size in tokens; drives compaction threshold | from `models.json` / 128,000 |
| `max_output_tokens` | int | Maximum output tokens | from `models.json` / env / provider default |
| `max_tool_turns` | int | Maximum tool execution turns per conversation turn | env / provider default |
| `max_tool_result_bytes` | int | Truncation limit for tool results in bytes; 0 = unlimited | 20,000 |
| `max_tool_calls_per_subturn` | int | Maximum tool calls in a single LLM response; 0 = unlimited | 25 |
| `model_context_tokens` | int | Approximate context window for in-session token warnings | from `models.json` / none |
| `context_warning_threshold` | float | Fraction of `model_context_tokens` at which to warn, 0.0–1.0 | 0.6 |
| `additional_params` | table | Provider-specific parameters forwarded verbatim to the completion request body (see [Additional Parameters](#additional-parameters)) | none |
| `read_timeout_secs` | int | HTTP read timeout in seconds; 0 = disable | 30 |
| `max_retries` | int | Retry attempts for transient errors | 3 |
| `retry_base_delay_ms` | int | Base backoff in ms, doubles each attempt, capped at 30s | 1,000 |

All fields except `model` are optional. When omitted, the value is inherited from the next level in the [resolution priority](#resolution-priority).

### Providers

```toml
[providers.openai]
api_key = "store:openai"       # or "sk-..." literal, or omit to use env var
base_url = "https://api.openai.com/v1"   # optional override
provider = "openai"            # provider implementation (default: the key itself)
name = "OpenAI"                # optional display name
preamble = "You are helpful." # optional provider preamble

[providers.openai.models.gpt-4o]
# per-model overrides (temperature, limits, name, tool_call, preamble)
```

#### ProviderConfig field reference

| Field | Type | Description |
|-------|------|-------------|
| `api_key` | String | API key or a `store:` reference (see [secrets.json](#2-secretsjson)) |
| `base_url` | String | Custom API endpoint URL |
| `provider` | String | Provider implementation to use (e.g. `"openai"` for a `github-copilot` provider or any OpenAI-compatible endpoint) |
| `name` | String | Provider display name |
| `preamble` | String | Optional system preamble |
| `models` | table | Per-model configuration keyed by model id |

> **Note:** The `[providers.<name>]` block is **optional**. If omitted, the model is resolved from `models.default.model` with environment variables as the fallback for API keys and base URLs. Providers that don't require authentication (like Ollama) can be used with just `models.default.model = "ollama-cloud/glm-5.2"` and no provider block at all.

#### Model format

All models use `provider/model` format:

- `ollama/gemma4:31b`
- `openai/gpt-4o`
- `anthropic/claude-sonnet-4-6`
- `github-copilot/claude-sonnet-4.6`

### OpenAI-compatible providers

Any provider that implements the OpenAI Chat Completions API (`POST /chat/completions`) works with `provider = "openai"` and a custom `base_url`.

```toml
[models.default]
model = "together/meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo"

[providers.together]
provider = "openai"
api_key = "store:together"
base_url = "https://api.together.xyz/v1"

[providers.together.models."meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo"]
```

This works for Together AI, Groq, OpenRouter, vLLM, LiteLLM, and any other OpenAI-compatible endpoint.

### Session store

```toml
[session_store]
store_type = "sqlite"    # "sqlite" (default), "jsonl", or "memory"
path = "/custom/path"   # optional custom path
```

- `store_type` — `"sqlite"` (default), `"jsonl"`, or `"memory"`
- `path` — optional custom path (JSONL defaults to `$XDG_CACHE_HOME/nu-agent/sessions/`)

`"memory"` uses an in-memory SQLite database — nothing persists after the process exits. It is useful for ephemeral sessions and testing.

Precedence for store type: CLI `--store` flag > `AGENT_SESSION_STORE_TYPE` env var > config > built-in default (SQLite).

### Compaction

```toml
[compaction]
strategy = "sliding_summary"    # or "sliding_window", "token_truncate"
keep_recent = 10                # minimum-message guard for sliding_summary; last N kept for sliding_window
# token_budget = 4000          # required for token_truncate
proactive_threshold_pct = 0.80  # 0.0-1.0 fraction of max_context_tokens before compaction fires
```

- `sliding_summary` (default) — LLM summarizes all messages into a single system message. `keep_recent` acts only as a minimum-message guard.
- `sliding_window` — drops old messages, keeps the last `keep_recent` verbatim. No LLM call.
- `token_truncate` — keeps newest messages within a token budget (chars/4 estimate). Requires `token_budget`. No LLM call.

`max_context_tokens` lives on the active model role, not in the `[compaction]` block, but it directly controls when compaction fires. Set it to match your model's actual context window.

### Agents

```toml
[agents]
planner_enabled = true
maker_enabled = true
default = "planner"       # default persona at startup
fallback = "coder"       # optional .agents/<name>.md persona when default built-in is disabled
```

Defaults: planner enabled, maker enabled, `default` = `"planner"`, no fallback. `fallback` is used only when `default` names a disabled built-in persona and must reference a file under `.agents/` or `$XDG_CONFIG_HOME/nu-agent/agents/`.

### A2A

```toml
a2a_enabled = true
```

Enable A2A (agent-to-agent) JSON-RPC 2.0 over HTTP. Default is `false`.

## Permissions

The `[permissions]` table controls tool execution policy.

```toml
[permissions]
"*" = "ask"           # global default
read = "allow"
glob = "allow"
grep = "allow"
edit = "ask"
patch = "ask"
http = "ask"
skill = "ask"
```

### Actions

- `"allow"` — execute without prompting
- `"deny"` — block execution silently
- `"ask"` — prompt the user for approval (TUI mode); deny in non-interactive mode

### Global policy

`"*"` sets the default action for any tool not explicitly listed. Defaults to `"ask"` (TUI) or `"deny"` (pipeline) if omitted.

### Nested field rules

```toml
[permissions.nu.command]
"rm*" = "deny"
"git push*" = "deny"
"*" = "ask"
```

### Precedence

1. Base config (`[permissions]` in `config.toml`)
2. Agent persona overlay (from persona front matter)
3. CLI `--permissions` flag (highest priority)

### Safe defaults

When no `[permissions]` section exists: `read`, `glob`, `grep` → `allow`; `c5t_get*`, `c5t_list*` → `allow`; everything else → `ask` (TUI) or `deny` (pipeline).

## Tree-sitter tools (code analysis)

The tree-sitter tools perform structural code analysis using tree-sitter grammars. They are language-generic — they work with any tree-sitter grammar you have installed. They are read-only and auto-allowed by default.

### Prerequisites

The tree-sitter tools require the `tree-sitter` CLI to install and compile grammars. Install it with:

```sh
cargo install tree-sitter-cli
```

Or via your package manager:

```sh
# macOS (Homebrew)
brew install tree-sitter

# Debian/Ubuntu
apt install tree-sitter

# Arch
pacman -S tree-sitter

# Windows (scoop)
scoop install tree-sitter
```

### Setup steps

1. **Create the config file** (once):

   ```sh
   tree-sitter init-config
   ```

   This creates `config.json` at the platform config location (see below).

2. **Clone grammar repos** into a directory you will list in `parser-directories`:

   ```sh
   git clone https://github.com/tree-sitter/tree-sitter-rust ~/code/tree-sitter-rust
   git clone https://github.com/tree-sitter/tree-sitter-python ~/code/tree-sitter-python
   ```

3. **Add the directory to the config** — edit `config.json` and add:

   ```json
   {
     "parser-directories": ["/Users/yourname/code"]
   }
   ```

4. **Build each grammar**:

   ```sh
   cd ~/code/tree-sitter-rust
   tree-sitter build
   ```

   This creates `build/rust.so` (or `.dylib` on macOS, `.dll` on Windows).

### Config file location

The config file is resolved in this order:

| OS | Path |
|----|------|
| Any | `$TREE_SITTER_DIR/config.json` (env var override) |
| Linux | `$XDG_CONFIG_HOME/tree-sitter/config.json` or `~/.config/tree-sitter/config.json` |
| macOS | `~/Library/Application Support/tree-sitter/config.json` |
| Windows | `%APPDATA%/tree-sitter/config.json` |

### Config format

The config is a JSON file with a `parser-directories` array:

```json
{
  "parser-directories": ["/Users/yourname/code"]
}
```

Any subdirectory of a listed directory whose name starts with `tree-sitter-` is recognized as a grammar repo. The language name is the suffix — `tree-sitter-rust` → language `rust`, `tree-sitter-python` → `python`.

### Grammar discovery

The tools build a grammar cache on first use:

1. Reads `parser-directories` from the config file.
2. Scans each directory for `tree-sitter-*` subdirectories.
3. For each grammar, looks for a compiled shared library in this order:
   1. `<grammar_dir>/build/<language>.<ext>` — if built in the `build/` subdir
   2. `<grammar_dir>/<language>.<ext>` — grammar dir root (default `tree-sitter build` output)
   3. `<cache_dir>/lib/<language>.<ext>` — XDG cache `lib/` subdirectory
4. Loads the symbol `tree_sitter_<language>` from the shared library.

The cache is built once per process. If you install a new grammar, restart the agent.

### Cache path

Compiled grammars are cached in the XDG cache directory:

| All platforms | `$XDG_CACHE_HOME/tree-sitter/lib/` or `~/.cache/tree-sitter/lib/` |

### Supported languages

ANY language with a tree-sitter grammar. The tools are language-generic — Rust, Go, Python, TypeScript, Nix, TOML, JSON, YAML, and more. Search for grammars at <https://github.com/tree-sitter> or clone the grammar repo and run `tree-sitter build`.

### Tools

There are four separate tools. Each is called directly with its own parameters — there is no shared `action` field.

**`ast_query`** — run an S-expression tree-sitter query against source code.

- Required: `path`, `language`, `query`
- Optional: `captures` (array of strings), `max_matches` (int, default 100), `include_text` (bool, default true)

```json
{"path": "src/main.rs", "language": "rust", "query": "(function_item name: (identifier) @name)"}
{"path": "src/main.rs", "language": "rust", "query": "(struct_item name: (type_identifier) @name)", "captures": ["name"], "include_text": false}
```

**`ast_nodes`** — list AST nodes of a given type in source code.

- Required: `path`, `language`, `node_type`
- Optional: `max_matches` (int, default 200), `include_text` (bool, default false)

```json
{"path": "src/main.rs", "language": "rust", "node_type": "match_arm"}
{"path": "src/main.rs", "language": "rust", "node_type": "function_item", "include_text": true}
```

**`ast_refs`** — find references to a named symbol in source code.

- Required: `path`, `language`, `name`
- Optional: `max_matches` (int, default 100)

```json
{"path": "src/main.rs", "language": "rust", "name": "Config"}
{"path": "src/lib.rs", "language": "rust", "name": "parse", "max_matches": 50}
```

**`ast_tree`** — dump the full S-expression parse tree of source code.

- Required: `path`, `language`
- Optional: `max_depth` (int, default unlimited)

```json
{"path": "src/main.rs", "language": "rust", "max_depth": 3}
{"path": "main.py", "language": "python", "max_depth": 5}
```

Other languages:

```json
{"path": "main.py", "language": "python", "query": "(function_definition name: (identifier) @name)"}
{"path": "main.go", "language": "go", "node_type": "function_declaration"}
{"path": "default.nix", "language": "nix", "name": "buildInputs"}
```

### Error messages

| Message | Cause | Fix |
|---------|-------|-----|
| `No tree-sitter config found. Run tree-sitter init-config to create one.` | No config file exists at any platform location. | Run `tree-sitter init-config`. |
| `Failed to load tree-sitter config: ...` | Config file exists but is unreadable (permissions, corrupt). | Check file permissions; delete and re-run `tree-sitter init-config`. |
| `Failed to parse tree-sitter config: ...` | Config JSON is invalid or missing `parser-directories`. | Open the config and fix the JSON; ensure `parser-directories` is an array. |
| `No tree-sitter grammar found for language 'rust'. File: '...'. Install the rust grammar: clone the tree-sitter-rust repo and run tree-sitter build in it.` | No grammar dir for the requested language was found in any `parser-directories`. | Clone the `tree-sitter-rust` repo and run `tree-sitter build` in it. |
| `Grammar for rust found at ... but not compiled. Run tree-sitter build in the grammar directory.` | Grammar repo exists but has no compiled shared library in `build/`, the grammar root, or the cache `lib/` subdir. | `cd <grammar-dir> && tree-sitter build`. |
| `Failed to load grammar for rust: ...` | Shared library found but could not be loaded (wrong architecture, missing deps). | Rebuild: `tree-sitter build` in the grammar dir. |
| `Failed to load grammar for rust: symbol 'tree_sitter_rust' not found: ...` | Shared library loaded but does not export the expected symbol. | Delete and re-clone the grammar repo, then rebuild. |
| `File not found: ...` | The `path` argument points to a non-existent file. | Verify the path relative to the working directory. |
| `Invalid tree-sitter query: ...` | The `query` S-expression is malformed or uses an unknown node name. | Check node names via the `ast_tree` tool; fix the query. |
| `Invalid ast_query arguments: ...` | Argument JSON does not match the `ast_query` schema. | Check required fields (`path`, `language`, `query`) and their types. |
| `Invalid ast_nodes arguments: ...` | Argument JSON does not match the `ast_nodes` schema. | Check required fields (`path`, `language`, `node_type`) and their types. |
| `Invalid ast_refs arguments: ...` | Argument JSON does not match the `ast_refs` schema. | Check required fields (`path`, `language`, `name`) and their types. |
| `Invalid ast_tree arguments: ...` | Argument JSON does not match the `ast_tree` schema. | Check required fields (`path`, `language`) and their types. |

### Permission

The tree-sitter tools (`ast_query`, `ast_nodes`, `ast_refs`, `ast_tree`) are read-only and auto-allowed in `safe_defaults`. They do not modify files.

## MCP Servers

The `[mcp]` table configures MCP servers. Each server is a sub-table keyed by name.

```toml
[mcp.context7]
transport = "http"
url = "https://mcp.context7.com/mcp"

[mcp.c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"
enabled = true

[mcp.nu]
transport = "stdio"
command = "nu-mcp"
args = ["--add-path", "/tmp"]
enabled = false

[mcp.nu.env]
GIT_PAGER = ""
```

### Transports

- `"stdio"` — spawns a local process, requires `command`
- `"sse"` — Server-Sent Events, requires `url`
- `"http"` — HTTP streaming, requires `url`

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `transport` | String | **yes** | `stdio`, `sse`, or `http` |
| `url` | String | for `sse`/`http` | Server URL |
| `command` | String | for `stdio` | Command to launch |
| `args` | Array | no | Command arguments |
| `cwd` | String | no | Working directory for stdio |
| `env` | Table | no | Extra environment variables |
| `headers` | Table | no | Extra HTTP headers (sse/http) |
| `enabled` | Bool | no | Default `true` |

### Auth (optional)

```toml
[mcp.api]
transport = "http"
url = "https://api.example.com/mcp"

[mcp.api.auth]
type = "bearer"
token = "your-token"
```

Auth types: `none` (default), `bearer` (requires `token`), `oauth` (requires http/sse transport; supports `client-id`, `client-secret`, `scope`, `redirect-uri`).

### Auth commands

- `agent mcp auth login <server>` — start OAuth flow
- `agent mcp auth logout <server>` — clear credentials
- `agent mcp auth status` — show auth status for all servers

## 2. secrets.json

The secret store persists API keys and OAuth tokens to `$XDG_DATA_HOME/nu-agent/secrets.json` with `0600` permissions. It is the recommended way to keep credentials out of `config.toml`.

### `store:` reference syntax

Reference a stored credential from `config.toml` with a `store:` prefix:

```toml
[providers.openai]
api_key = "store:openai"
```

At runtime, `store:openai` is resolved by looking up `openai` in the secret store and using the stored credential (API key or OAuth access token). If the key is not present, `nu-agent` falls back to the corresponding environment variable.

### Managing credentials

Use the `agent provider auth` commands:

```sh
# Store an API key for a provider (interactive prompt, or via --api-key)
agent provider auth login openai
agent provider auth login openai --api-key sk-...

# GitHub Copilot uses the OAuth device-code flow
agent provider auth login github-copilot

# Clear stored credentials for a provider
agent provider auth logout openai

# Show stored credentials (provider, type, expiry)
agent provider auth status
```

`agent provider auth status` outputs one row per stored provider credential, showing the credential type (`api_key` or `oauth`) and, for OAuth, the token expiry timestamp.

## 3. models.json

`nu-agent` can cache the `models.dev` database locally to power model discovery, context-window metadata, and the model picker. The cache lives at `$XDG_DATA_HOME/nu-agent/models.json`.

### Syncing

```sh
agent models sync
```

Fetches the latest specs from `models.dev` and writes them to the local cache. Prints a summary such as `Synced 120 providers, 3000 models`.

### Listing

```sh
agent models list                       # all models in the cache
agent models list --provider openai     # only openai models
```

Outputs one row per model with `provider`, `model`, `name`, `context`, `output`, and `tool_call`. If the cache does not exist, it instructs you to run `agent models sync` first.

When a models cache is present, the model picker and `resolve_model()` use its context/output limits to fill missing `max_context_tokens` / `max_output_tokens` values (see [resolution priority](#resolution-priority)).

## Resolution priority

When resolving the runtime config for a model, values are filled from the highest-priority source that sets them, falling through to the next. Highest to lowest:

1. **CLI flags** — `--model`, `--temperature`, `--max-tokens`, etc. override all config
2. **Persona front matter** — a persona's `model:` field selects a role or literal model
3. **Role-level config** — the `[models.<role>]` record for the selected role (e.g. `models.heavy`)
4. **Provider/model-level config** — `[providers.<name>]` and `[providers.<name>.models.<model>]`
5. **models.json cache** — fills `max_context_tokens`, `max_output_tokens`, and `model_context_tokens` when not already set by the levels above
6. **Secret store** — resolves `store:` references in `api_key`
7. **Environment variables** — `AGENT_*` vars and `{PROVIDER}_API_KEY` (lowest priority fallback)
8. **Built-in defaults** — hardcoded fallbacks

A value set at a higher priority is never overridden by a lower one. For example, if a role sets `temperature`, provider-level or env values for `temperature` are ignored.

### Persona `model:` resolution rules

1. `model: heavy` (no slash, matches a role in `[models]`) → resolves to that role's `ModelRoleConfig`
2. `model: provider/model` (contains a slash) → used as-is (literal model identifier); other parameters inherit from `models.default`
3. `model: foo` (no slash, not a role) → **error** at startup
4. No `model:` field → falls back to `models.default`
5. CLI `--model provider/model` → overrides everything

## Environment variable fallback

Environment variables still work as a lowest-priority fallback when `config.toml` / `secrets.json` don't set a value:

- `AGENT_BASE_URL`
- `AGENT_TEMPERATURE`
- `AGENT_MAX_TOKENS`
- `AGENT_MAX_CONTEXT_TOKENS`
- `AGENT_MAX_OUTPUT_TOKENS`
- `AGENT_MAX_TOOL_TURNS`
- `AGENT_MAX_TOOL_RESULT_BYTES` — max bytes per tool call before truncation (default 20000; `0` = unlimited)
- `AGENT_MAX_TOOL_CALLS_PER_SUBTURN` — max tool calls in a single LLM response (default 10; `0` = unlimited)
- `AGENT_MODEL_CONTEXT_TOKENS` — approximate context window for token warnings
- `AGENT_CONTEXT_WARNING_THRESHOLD` — fraction at which to warn, `0.0`–`1.0` (default `0.6`)
- `AGENT_MAX_RETRIES` — retry attempts (default 3)
- `AGENT_RETRY_BASE_DELAY_MS` — base backoff in ms, doubles each attempt, capped at 30s (default 1000)
- `AGENT_READ_TIMEOUT_SECS` — HTTP read timeout in seconds (default 30). 0 disables.
- `AGENT_A2A_ENABLED` — enable A2A (default `false`)
- `AGENT_A2A_PORT` — A2A port (0 = random, >0 = fixed)
- `AGENT_SESSION_STORE_TYPE` — `sqlite`, `jsonl`, or `memory`
- `{PROVIDER}_API_KEY` — e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` (for providers with direct env naming)

There is no `AGENT_MODEL` env override at runtime; set the default model in `config.toml`. (`agent config init` reads `AGENT_PROVIDER`/`AGENT_MODEL` when generating a starter config.)

## CLI commands reference

```sh
# Configuration
agent config init [--force]                     # generate starter config.toml from env vars

# Model discovery
agent models sync                                # fetch models.dev into the local cache
agent models list [--provider <name>]            # list models from the cache

# Provider auth (secrets.json)
agent provider auth login <name> [--api-key <key>]
agent provider auth logout <name>
agent provider auth status

# MCP auth
agent mcp auth login <server>                    # OAuth device flow
agent mcp auth logout <server>                   # clear stored credentials
agent mcp auth status                            # show auth status for all MCP servers
```

## Examples

### Single provider (OpenAI)

```toml
[models.default]
model = "openai/gpt-4o"
temperature = 0.3
max_context_tokens = 128000

[providers.openai]
api_key = "store:openai"

[providers.openai.models.gpt-4o]
```

### Single provider (Anthropic)

```toml
[models.default]
model = "anthropic/claude-sonnet-4-6"

[providers.anthropic]
api_key = "store:anthropic"

[providers.anthropic.models.claude-sonnet-4-6]
```

### Local Ollama

```toml
[models.default]
model = "ollama/gemma4:31b"

[providers.ollama]
base_url = "http://127.0.0.1:11434"

[providers.ollama.models.gemma4:31b]
```

### Multi-provider

```toml
[models.default]
model = "openai/gpt-4o"

[models.heavy]
model = "anthropic/claude-sonnet-4-6"
max_context_tokens = 200000

[models.light]
model = "ollama/gemma4:31b"

[providers.openai]
api_key = "store:openai"

[providers.openai.models.gpt-4o]

[providers.anthropic]
api_key = "store:anthropic"

[providers.anthropic.models.claude-sonnet-4-6]

[providers.ollama]
base_url = "http://127.0.0.1:11434"

[providers.ollama.models.gemma4:31b]
```

### GitHub Copilot

```toml
[models.default]
model = "github-copilot/claude-sonnet-4.6"

[providers.github-copilot]
# No api_key needed — Copilot uses the OAuth device-code flow
# (run: agent provider auth login github-copilot)

[providers.github-copilot.models.claude-sonnet-4.6]
```

## Additional parameters

`additional_params` forwards a table of provider-specific keys verbatim into the top-level HTTP request body. Set it per role in the model config:

```toml
[models.default]
model = "anthropic/claude-sonnet-4-6"

[models.default.additional_params.output_config]
effort = "medium"
```

### Gotchas

- **Must be a table** — arrays, strings, and scalars are rejected at parse time.
- **Do not shadow typed fields** — keys like `model`, `max_tokens`, or `temperature` already have typed fields. Duplicating them via `additional_params` produces duplicate JSON keys with undefined behavior.
- **Applied to every turn** — the value is included in every completion request.

### Controlling thinking depth on Anthropic Sonnet 4.6

Sonnet 4.6 uses **adaptive thinking** — the model decides when to think. Control thinking depth via `output_config.effort`:

```toml
[models.default]
model = "anthropic/claude-sonnet-4-6"

[models.default.additional_params.output_config]
effort = "medium"
```

Effort levels: `low` (fastest, fewest tokens) · `medium` (recommended for agents) · `high` (default if omitted) · `max` (highest capability)
