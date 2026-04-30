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

## Notes

- Follow TDD (RED -> GREEN -> REFACTOR)
- Keep tests in separate files in `src/` (`*_test.rs`)
- Avoid inline `#[cfg(test)] mod tests` inside production files
