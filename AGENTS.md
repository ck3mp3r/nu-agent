# Development Rules for nu-agent

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
- Use `*_test.rs` naming convention for test files
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
