# Development Rules for nu-agent

## Commands

```bash
cargo build
cargo test
cargo clippy --workspace --tests -- -D warnings   # warnings are errors
cargo fmt -- --check
```

## Testing Philosophy

### Test-Driven Development (TDD)

**Always follow the RED → GREEN → REFACTOR cycle:**

1. **RED** - Write a failing test first
2. **GREEN** - Write minimal code to make the test pass
3. **REFACTOR** - Clean up and improve the code

Never write production code without a failing test first.

### Test Organization

**No inline tests** - Tests must be in separate files in `src/`:

```
src/
  lib.rs
  lib_test.rs
  plugin.rs
  plugin_test.rs
  commands/
    info.rs
    info_test.rs
```

- All tests live in `src/` directory alongside the code they test
- Use the module-aware naming convention:
  - Single-file module: `foo.rs` with sibling `foo_test.rs`
  - Multi-file module: `foo/mod.rs` with `foo/test.rs`
  - Forbidden: mixed `foo.rs` + `foo/test.rs`
- Keep test files focused and organized by module

### Mocking

**Use mocks wherever available:**

- Mock external dependencies (LLM APIs, file system, network)
- Mock Nushell's `EngineInterface` when testing commands
- Use dependency injection to make code testable
- Prefer trait-based abstractions for mockable interfaces

## Code Quality

- Write tests before implementation
- Keep functions small and focused
- Use meaningful names for tests (describe what they verify)
- Each test should verify one behavior
- Refactor only when tests are green
- No hidden global mutable state — use explicit state via structs

### No Parallel Developer Agents

**NEVER run two developer agents concurrently on the same repository.**

Compiled projects share a build cache, lock files, and the working tree.
Parallel agents will corrupt each other's builds and produce interleaved file edits.

- Delegate one developer task at a time
- Wait for it to complete before delegating the next
- Researcher and reviewer agents may run in parallel with each other, but never alongside a developer

### Review Before Commit

**Always run a review before `git commit`. No exceptions.**

```bash
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

A reviewer subagent must sign off before committing. Clippy warnings are
build failures. `#[allow(...)]` is never acceptable — fix the code instead.

### `unwrap()` Policy

- ❌ NO: `unwrap()` in production code
- ✅ YES: `unwrap_or`, `unwrap_or_else`, `if let`, `?`, `expect("reason")`
- ✅ EXCEPTION: `mutex.lock().unwrap()` — mutex poison is a fatal internal
  inconsistency and panicking is correct

### Nested `if` Statements

Flatten nested conditions. Never nest `if` inside `if` when they can be combined.

```rust
// ❌ WRONG
if let Some(x) = foo {
    if x > 0 {
        do_thing();
    }
}

// ✅ CORRECT
if let Some(x) = foo && x > 0 {
    do_thing();
}

// ✅ ALSO CORRECT — early return
let Some(x) = foo else { return; };
if x > 0 {
    do_thing();
}
```

### Scope Discipline

Do NOT refactor adjacent code "while you're at it." Every change must be
scoped to the task at hand. If something else needs fixing, create a new task.

### `#[allow(...)]` Is Never a Fix

Suppressing a clippy warning is not fixing it. If clippy flags something,
fix the underlying code. The only exception is `#[allow(dead_code)]` on
test helpers that are intentionally unused — and even then, prefer deleting
the dead code.

## SOLID Principles

**No dynamic dispatch! Use static dispatch with generics.**

- ❌ NO: `Box<dyn Trait>`, `&dyn Trait`, trait objects in internal code
- ✅ YES: Generics with trait bounds `T: Trait`
- ✅ EXCEPTION: nu-plugin API boundary can use dynamic dispatch (it's required by the framework)
- Follow SOLID principles throughout:
  - **S**ingle Responsibility: One reason to change
  - **O**pen/Closed: Open for extension, closed for modification
  - **L**iskov Substitution: Subtypes must be substitutable
  - **I**nterface Segregation: Many specific interfaces over one general
  - **D**ependency Inversion: Depend on abstractions, not concretions

### Trait Design

Every required trait method must be callable by at least one production code
path. If a method only exists to satisfy the compiler, the trait is designed
wrong — make it optional (provide a default implementation) or redesign the
interface.

```rust
// ❌ WRONG — choose() is required but never called in production
pub trait AskApprovalHook {
    fn choose(&mut self, ...) -> AskChoice;           // required, never called
    fn choose_with_sink(&mut self, ...) -> AskChoice { // optional, always called
        self.choose(...)
    }
}

// ✅ CORRECT — single method, covers all call sites
pub trait AskApprovalHook {
    fn choose<S: Sink>(&mut self, ..., sink: Option<&mut S>) -> AskChoice;
}
```

## Streaming HTTP

When working with LLM streaming responses:

- ✅ Use `read_timeout()` — fires only when no bytes received; resets on each chunk
- ❌ Never use `timeout()` — kills the entire request after a fixed deadline,
  breaking long but active streaming responses

## Configuration System

See `docs/configuration.md` for the full field reference and provider examples.

### Two-path parsing

`PluginConfig::from_plugin_config()` (`crates/nu-agent-core/src/config/mod.rs`) is tried first. The legacy flat `Config::from_plugin_config()` is the fallback for configs without a `providers` key. **All new features go in the new path only** — never add fields to the legacy parser.

### Precedence chain (highest → lowest)

1. CLI flags — applied in `resolve_with_new_config()` (`crates/nu-agent/src/command/agent/runtime_build.rs`)
2. Model-level config — `providers.<name>.models.<name>` fields, applied in `PluginConfig::resolve_model()`
3. Environment variables — `Config::from_env()` in `config/mod.rs`
4. Top-level `PluginConfig` fields — fallback block at end of `resolve_model()`
5. Built-in defaults — `unwrap_or(N)` at usage sites in executor/runtime

### The `None` rule

Every `Option` field on the runtime `Config` struct must have at least one real input path (env var, plugin config key, or CLI flag). **Hardcoding `None` in a parsing function is a bug.** Intentional exceptions — fields resolved separately at runtime and not user-configurable:
- `preamble` — resolved via `resolve_preamble()` from provider/model preamble config
- `provider_impl` — resolved from the `provider` key inside the provider config block

### Adding a new config field

```
1. Add field to `PluginConfig` struct + parse in `from_plugin_config()`
2. Forward in `resolve_model()` fallback block (`if config.field.is_none()`)
3. Add env var `AGENT_<FIELD_UPPER>` to `Config::from_env()`
4. Add CLI flag in `crates/nu-agent/src/command/agent/mod.rs` + apply in `resolve_with_new_config()`
5. Add tests in the sibling `test.rs` / `*_test.rs` file — NO inline tests
6. Update `docs/configuration.md`
```

## Examples

### Good Test Structure

```rust
// src/commands/info_test.rs
use super::*;

#[test]
fn returns_plugin_version() {
    // RED: Write this first, watch it fail
    // GREEN: Implement minimal code
    // REFACTOR: Clean up when green
}

#[test]
fn handles_empty_input() {
    // Another focused test
}
```

### Bad Test Structure (Don't do this)

```rust
// src/lib.rs
pub fn some_function() -> String {
    "result".to_string()
}

#[cfg(test)]  // ❌ NO inline tests
mod tests {
    #[test]
    fn it_works() {
        // ...
    }
}
```

## Docs Guardrails (Tool/Authz/TUI changes)

When changing tool handler modules, permission UX, or TUI transcript rendering:

- Follow `docs/contribution-guardrails.md` checklist (inline permission card, viewport invariants, sticky controls)
- Keep `docs/usage.md` aligned with user-visible behavior changes
