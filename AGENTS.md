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
