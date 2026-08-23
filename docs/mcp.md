# MCP

MCP server configuration is optional and lives in config.toml under `[mcp]`.

## Example

```toml
[mcp.c5t]
transport = "sse"
url = "http://0.0.0.0:3737/mcp"

[mcp.nu]
transport = "stdio"
command = "nu-mcp"
cwd = "/path/to/workspace"
args = ["--add-path", "/tmp"]

[mcp.nu.env]
GIT_PAGER = ""

[models.default]
model = "github-copilot/claude-opus-4.6"

[providers.github-copilot]
provider = "openai"
api_key = "store:github-copilot"
base_url = "https://api.individual.githubcopilot.com"
```

## Behavior

- If `mcp` is missing or empty, agent runs without MCP.
- Tools are discovered from connected MCP servers at runtime.
- Exposed/callable MCP tool names are namespaced as `<server_key>__<raw_tool_name>`.
  - `server_key` is the key under `[mcp.<server_key>]` in config.toml.

## Transport Rules

- `transport: "stdio"` requires `command`
- `transport: "sse" | "http"` requires `url`
- optional fields: `args`, `env`, `headers`, `cwd` (`stdio` only), `auth`

## Stdio working directory behavior

For `transport: "stdio"` servers:

- Child process `current_dir` is resolved deterministically from caller context:
  - absolute `mcp.<server>.cwd`: used as-is, then canonicalized/validated
  - relative `mcp.<server>.cwd`: resolved against the caller cwd, then canonicalized/validated
  - no `mcp.<server>.cwd`: caller cwd is used
- `PWD` is explicitly set to the effective child cwd for compatibility.
- Caller context is preserved in env variables:
  - `NU_AGENT_CALLER_CWD` = canonical caller cwd
  - `NU_AGENT_CALLER_PWD` = canonical caller cwd (compat alias)
- Invalid/missing cwd is an explicit configuration/runtime error (no silent fallback).

For `sse`/`http` transports, cwd behavior is unchanged.

## Tool failure recovery behavior

Tool-call failures are non-fatal for the current agent turn. Instead of aborting, the agent appends
a structured tool result payload that the LLM can consume for retry/replanning.

Failure payload contract:

- `tool_name`
- `tool_call_id`
- `source` (`closure` | `mcp` | `unknown`)
- `error_kind` (`timeout` | `validation` | `runtime` | `transport` | `unknown`)
- `message`
- optional `details`

Typical recovery flow:

1. LLM calls tool with invalid args.
2. Tool returns structured failure payload above.
3. LLM sees payload in tool-result stream and issues corrected tool call.

Fatal errors still remain for unrecoverable command-level failures (for example: invalid top-level
agent config or LLM provider initialization failures).

## Collision prevention

If two MCP servers expose the same raw tool name (for example both expose `list_prs`),
the exposed names remain unique via server namespacing:

- `gh__list_prs`
- `altgh__list_prs`

This avoids cross-server collisions in discovery, filtering, and tool execution.

## Tool precedence

If a closure tool and an MCP tool share the same exposed name, closure tools take precedence during execution.

- precedence order: closure tool, then MCP tool
- use distinct names to avoid accidental shadowing

## MCP Authentication

MCP servers can require authentication. The agent supports three auth types: `none`, `bearer`, and `oauth`.

Configure auth under `[mcp.<server>.auth]` in config.toml. See [configuration.md](./configuration.md#mcp-servers) for config examples.

### OAuth flow (step-by-step)

When you run `agent mcp auth login <name>`, the following happens:

1. **Load config** — reads the MCP server configuration from config.toml.
2. **Start callback server** — binds a local HTTP server to `127.0.0.1` on a random port to receive the OAuth redirect.
3. **Discover OAuth metadata** — fetches `/.well-known/oauth-authorization-server` from the server URL to find authorization, token, and registration endpoints.
4. **Register client** — if no `client-id` is configured, performs dynamic client registration via the discovered registration endpoint. If `client-id` is configured, uses it directly.
5. **Generate PKCE challenge** — creates a cryptographically random `code_verifier` and `code_challenge` (SHA-256 hashed) for the authorization request.
6. **Build authorization URL** — constructs the URL with `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` (CSRF token), and `code_challenge`.
7. **Open browser** — launches the system browser to the authorization URL.
8. **User authenticates** — the user logs in and grants consent in the browser.
9. **Browser redirects** — the authorization server redirects to the callback server at `http://127.0.0.1:<random-port>/mcp/oauth/callback?code=...&state=...`.
10. **Validate state** — the callback server verifies the `state` parameter matches the one sent (CSRF protection).
11. **Exchange code for tokens** — sends the authorization code, `code_verifier`, and `redirect_uri` to the token endpoint. Receives `access_token`, `refresh_token`, and `expires_in`.
12. **Save credentials** — stores tokens to `$XDG_DATA_HOME/nu-agent/mcp-auth.json` with `0600` permissions.
13. **Stop callback server** — shuts down the local HTTP server.

On subsequent agent runs, the stored access token is used automatically. If expired, the refresh token is used to obtain a new access token without user interaction.

### Security notes

| Measure | Implementation |
|---------|---------------|
| **File permissions** | Credentials stored at `$XDG_DATA_HOME/nu-agent/mcp-auth.json` with `0600` permissions (owner read/write only). |
| **Loopback-only callback** | The OAuth callback server binds exclusively to `127.0.0.1` — never exposed to the network. |
| **CSRF protection** | The `state` parameter is a cryptographically random token. The callback validates it matches the sent value before exchanging the code. |
| **SSRF blocking** | URL validation in the HTTP client blocks requests to cloud metadata endpoints (`169.254.169.254`) and link-local addresses (`169.254.0.0/16`). |
| **PKCE** | Proof Key for Code Exchange ensures the authorization code can only be exchanged by the client that initiated the flow, protecting against interception. |

## Migration note

Previous behavior exposed raw MCP tool names directly (e.g. `list_prs`).

Current behavior requires namespaced names (e.g. `gh__list_prs`) for:

- permissions DSL patterns
- LLM tool-call names routed through the tool handler

Update any existing filters/prompts that referenced raw MCP tool names.

## Reserved delimiter

`__` is reserved as MCP tool namespace delimiter.

- `mcp.<server_key>` must not include `__`
- MCP raw tool names containing `__` are rejected at discovery time
