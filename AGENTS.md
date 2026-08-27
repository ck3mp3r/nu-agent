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
- When unit tests in a source file become large, split them out:
  ```rust
  // region:    --- Tests
  #[cfg(test)]
  #[path = "applier_tests.rs"]
  mod tests;
  // endregion: --- Tests
  ```

### No test-only code in production

**`#[cfg(test)]` on `fn` in production files is banned.**

- ❌ NO: `#[cfg(test)]` on any `fn` in production code (`.rs` files that are not `*_test.rs` / `test.rs`)
- ✅ YES: `#[cfg(test)]` on `mod` declarations for test modules (e.g. `#[cfg(test)] mod foo_test;`)
- ❌ NO: Test-only accessor methods that expose private fields solely for tests
- ✅ YES: Design public API so tests use the same methods as production code
- ✅ YES: Test through behavior — if the public API doesn't expose enough to verify a behavior, the API is missing a method, not the test missing a backdoor
- ❌ NO: Making a field `pub(crate)` just so tests can peek at internal state — this is the same code smell as `#[cfg(test)]` accessors

**If a test needs to inspect private state, one of these is wrong:**
1. The test is testing implementation details — rewrite it to assert outcomes through the public API
2. The design is wrong — the internal state shouldn't matter, only its observable effects

**Pre-existing violations are still violations.** If you encounter `#[cfg(test)]` accessor methods while working on an unrelated task, do NOT fix them inline — create a separate task for the cleanup. But they ARE a code smell that must be addressed.

### Test Through the Public Boundary

- Tests verify behavior through the actual public API (e.g., `BuiltinTool::execute()` for tool handlers), not through internal types or methods exposed solely for testing
- No private helper functions made public just for tests — shared logic is a private implementation detail, tested indirectly through observable output
- If a test needs to inspect internal state, the test is testing implementation details — rewrite it to assert outcomes through the public API, not add a backdoor
- `pub(crate)` on a field or method just so tests can peek at internal state is the same code smell as `#[cfg(test)]` accessors

### What counts as production code

Any `.rs` file that is NOT a test file (`*_test.rs`, `test.rs`) is production code. This includes `mod.rs`, `input.rs`, `lifecycle.rs`, `dispatch.rs`, etc. The `#[cfg(test)]` attribute on `fn` in these files is the violation.

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

### Constructor Patterns

Follow the rust10x fluid API conventions:

- `Default` for sync empty constructors — do not use `new() -> Self` with empty arguments
- `new(...)` for the primary constructor with obvious common arguments
- `from_...(...)` for secondary constructors
- `with_...(self, ...) -> Self` for fluent chainable setters (consuming pattern, not `&mut self`)
- `append_...(self, ...)` for adding single items, `append_...s(self, ...)` for `IntoIterator`
- `set_...(&mut self, ...)` for property setters taking `&mut self`
- Use `impl Into<...>` for constructor and setter parameters where appropriate
- Separate `Builder` struct with `.build()` only when construction is complex, requires validation, or is fallible (returning `Result`)

### Code Structure (rust10x regions)

Organize source files in this order:

1. Primary public types
2. Supporting public types
3. Public functions
4. Public inherent implementations (grouped by purpose: Constructors, Chainable setters, Accessors)
5. Trait implementations (in separate `impl` blocks)
6. Private support functions/types (`// region: --- Support`)
7. Unit tests (`// region: --- Tests`)

Use code regions for meaningful sections:

```rust
// region:    --- Types
// endregion: --- Types

// region:    --- Froms
// endregion: --- Froms

// region:    --- Support
// endregion: --- Support

// region:    --- Tests
// endregion: --- Tests
```

Group `From` implementations together in a `Froms` region. Place private helpers in a `Support` region. Do not create regions merely to label every `impl` block.

### Error Handling

- Use `derive_more::{Display, From}` for error enums (not `thiserror`)
- Define `pub type Result<T> = core::result::Result<T, Error>` in `error.rs`, re-export from `lib.rs`/`main.rs`
- `Custom` variant with `#[from(String, &String, &str)]` for easy `.ok_or("message")?`
- External error types below `// -- Externals` comment
- Application-specific variants above
- `Custom` variant always on top

### Modern Rust (Edition 2024)

- Use if-let chains: `if let Some(x) = foo && x > 0 { ... }` — do NOT nest `if` inside `if`
- Avoid `ref` bindings — use match ergonomics
- Inline format args: `println!("{name}")` not `println!("{}", name)` for simple variables
- `async || {}` closures supported (use `AsyncFn`, `AsyncFnMut`, `AsyncFnOnce` traits)
- Avoid manual pattern match when possible: `line.trim_start_matches([' ', '\t'])` not `|c: char| c == ' ' || c == '\t'`

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

### `unwrap()` / `expect()` Policy

- ❌ NO: `unwrap()` or `expect("...")` in production code
- ❌ NO: `unwrap()` or `expect("...")` in test or example code
- ✅ YES: `unwrap_or`, `unwrap_or_else`, `if let`, `?`, `.ok_or("should have ...")?`
- ✅ YES: `mutex.lock().unwrap()` — mutex poison is a fatal internal
  inconsistency and panicking is correct (the ONLY exception)
- For tests, use `.ok_or("should be ...")?` with `type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>`

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

### Module Files

Use `mod.rs` primarily for module declarations and intentional reexports. Put those declarations and reexports in a `Modules` region after any module-level documentation:

```rust
//! Module-level documentation.

// region:    --- Modules

mod event_base;
mod support;

pub use event_base::*;

// endregion: --- Modules
```

Implementation details should normally live in dedicated source files rather than in `mod.rs`.

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
- ✅ EXCEPTION: nu-plugin API boundary and rig's `ModelHandle`/`ConversationMemory` can use dynamic dispatch (required by the framework)
- Follow SOLID principles throughout:
  - **S**ingle Responsibility: One reason to change
  - **O**pen/Closed: Open for extension, closed for modification
  - **L**iskov Substitution: Subtypes must be substitutable
  - **I**nterface Segregation: Many specific interfaces over one general
  - **D**ependency Inversion: Depend on abstractions, not concretions

### Event/Channel Design (rust10x 3-level architecture)

When designing event/channel systems, follow the 3-level pattern:

- **Level 1** (Event Base): Channel backend normalization — `MpscTx<M>`/`MpscRx<M>`, etc. Backend-agnostic facade with application-owned errors.
- **Level 2** (Use-Case Aliases): Type aliases over Level 1 for specific use cases — `type TuiTx = MpscTx<TuiEvent>`. Topology choice lives in one place.
- **Level 3** (Domain Types): Domain-specific endpoint types with domain operations — `AiJobTx::exec_request()` instead of raw `.send()`. Owns error translation from Level 1.

Domain types should not leak Level 1 error types. Translate at the boundary.

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

### No Delegation Chains

A delegation chain is a type that implements a trait by forwarding every method to
an inner field — `self.inner.method()` for each trait method. This is a SOLID
violation: the wrapper type has no responsibility of its own, it just adds a layer.

Delegation chains indicate the trait is implemented on the wrong type. If type B
owns the data and type A wraps B just to implement a trait, either:
1. Implement the trait on B directly (B owns the data, B owns the behavior)
2. Or eliminate the trait — if the caller can call B directly, the trait and the
   wrapper are unnecessary indirection

- ❌ NO: `TuiInteractiveUi` implements `DisplayStateUi` by calling `self.renderer.set_*()`
       which calls `self.state.set_*()` — 3 layers, zero behavior added
- ✅ YES: The render loop calls `AppState::reduce_ui_state_event()` directly — no wrapper

If you find yourself writing `fn method(&mut self) { self.inner.method() }`, stop.
Either implement the trait on the inner type or eliminate the indirection.

```

## Streaming HTTP

When working with LLM streaming responses:

- ✅ Use `read_timeout()` — fires only when no bytes received; resets on each chunk
- ❌ Never use `timeout()` — kills the entire request after a fixed deadline,
  breaking long but active streaming responses

## Configuration System

See `docs/configuration.md` for the full field reference and provider examples.

### Loading from config.toml

`PluginConfig` is loaded via `toml_config::load()` (`crates/nu-agent-core/src/config/toml_config.rs`) from `config.toml`. All new features go through this single path.

The `PluginConfig` struct holds top-level blocks: `models`, `providers`, `compaction`, `mcp`, `permissions`, `agents`, `preamble`, `a2a_enabled`, `session_store`.

### Precedence chain (highest → lowest)

1. CLI flags — applied in `resolve_with_new_config()` (`crates/nu-agent/src/command/agent/runtime_build.rs`)
2. Persona front matter — `model:` field selects a role or literal model
3. Role-level config — `ModelRoleConfig` record for the selected role (e.g. `models.heavy`), applied in `PluginConfig::resolve_model()`
4. Model-level config — `providers.<name>.models.<name>` fields, applied in `PluginConfig::resolve_model()`
5. Environment variables — `Config::from_env()` in `config/mod.rs`
6. Built-in defaults — `unwrap_or(N)` at usage sites in executor/runtime

### The `None` rule

Every `Option` field on the runtime `Config` struct must have at least one real input path (env var, config.toml key, or CLI flag). **Hardcoding `None` in a parsing function is a bug.** This applies to `ModelRoleConfig` fields too — every optional field must be settable via the role config record, a model-level override, an env var, or a CLI flag. Intentional exceptions — fields resolved separately at runtime and not user-configurable:
- `preamble` — resolved via `resolve_preamble()` from provider/model preamble config
- `provider_impl` — resolved from the `provider` key inside the provider config block

### Adding a new config field

```
1. Add field to `ModelRoleConfig` struct + parse in `toml_config::load()` models sub-record parsing
2. Apply in `resolve_model()` role-level block (`if config.field.is_none()`)
3. Add env var `AGENT_<FIELD_UPPER>` to `Config::from_env()`
4. Add CLI flag in `crates/nu-agent/src/command/agent/mod.rs` + apply in `apply_cli_flags()`
5. Add tests in the sibling `test.rs` / `*_test.rs` file — NO inline tests
6. Update `docs/configuration.md`
```

### Test Structure (rust10x conventions)

Every test function follows this layout:

```rust
#[test]
fn test_module_function_variant() -> Result<()> {
    // -- Setup & Fixtures
    // ... code that preps/sets the context for the test

    // -- Exec
    // ... code that executes the function to be tested

    // -- Check
    // ... assertions and verification
    Ok(())
}
```

- `type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>` at the top of every test module
- Test naming: `test_[module_path_name]_[function_name]_[variant]`
- Section comments in every test: `// -- Setup & Fixtures`, `// -- Exec`, `// -- Check`
- Use `// -- Exec & Check` when exec and checks are in a `for` loop
- No `unwrap()` — use `.ok_or("should be ...")?`
- Test support functions at the end under `// -- Test Support` (inline) or `// region: --- Test Support` (dedicated file)
- For temp data files, use paths like `tests-data/.tmp/test_function_name/`

## Docs Guardrails (Tool/Authz/TUI changes)

When changing tool handler modules, permission UX, or TUI transcript rendering:

- Follow `docs/contribution-guardrails.md` checklist (inline permission card, viewport invariants, sticky controls)
- Keep `docs/usage.md` aligned with user-visible behavior changes