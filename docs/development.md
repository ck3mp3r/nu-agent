# Development

## Build

```bash
cargo build
```

## Test

```bash
cargo test
```

## Lint

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## Format

```bash
cargo fmt -- --check
```

## Architecture

- **[Event Architecture](./event-architecture.md)**: Typed event-driven harness enforcement checkpoints, subscribers, and policy modes.
- **[Handler Decomposition Contract](./handler-decomposition-contract.md)**: Tool handler module structure and dependency rules.
- **[Contribution Guardrails](./contribution-guardrails.md)**: Requirements for tool/authz/TUI changes.

## Notes

- Follow TDD (RED -> GREEN -> REFACTOR)
- Keep tests in separate files in `src/` following module layout rules:
  - Single-file module: `foo.rs` + `foo_test.rs`
  - Multi-file module: `foo/mod.rs` + optional `foo/<submodule>.rs` + `foo/test.rs`
  - Forbidden mixed pattern: `foo.rs` + `foo/test.rs`
- Avoid inline `#[cfg(test)] mod tests` inside production files
- Guardrail: `module_test_layout_rejects_mixed_root_and_nested_test_pattern` fails CI/tests when mixed layout is introduced
- Use static dispatch (generics) over dynamic dispatch (`Box<dyn Trait>`) in internal code
- No hidden global mutable state—explicit state via structs or `RefCell`
