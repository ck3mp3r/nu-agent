# Code Coverage

Code coverage is measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov), which wraps rustc's native LLVM source-based instrumentation (`-C instrument-coverage`). It runs on stable Rust, needs no separate coverage binary, and reports line, region, and function coverage.

Coverage is a second quality dimension next to `cargo clippy --workspace --tests -- -D warnings`: clippy finds suspicious code, coverage finds code no test ever executes.

## Installation

```sh
cargo install cargo-llvm-cov --locked
```

Installing requires rustc 1.87 or newer. Running requires Cargo 1.60 or newer.

`cargo-llvm-cov` also needs the LLVM tools that match the rustc in use:

```sh
rustup component add llvm-tools-preview
```

Verify the installation:

```sh
cargo llvm-cov --version
```

## Basic usage

Run the workspace test suite and print a coverage summary to stdout:

```sh
cargo llvm-cov --workspace
```

`cargo llvm-cov` runs `cargo test` by default, so it needs the same environment as the test suite. Doc tests are excluded by default because coverage for them requires nightly.

## Output formats

### HTML

```sh
cargo llvm-cov --workspace --html --output-dir coverage/
```

Open `coverage/index.html` in a browser. The HTML report shows per-file line hit counts and highlights uncovered regions. Without `--output-dir` the report is written to `target/llvm-cov/html`.

Use `--open` to generate the HTML report and open it in the default browser in one step.

### LCOV

```sh
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

LCOV is the format consumed by Codecov, Coveralls, and most editor coverage extensions. Without `--output-path` the report is printed to stdout.

### JSON

```sh
cargo llvm-cov --workspace --json --output-path cov.json
```

JSON carries the full per-file, per-function data and is the format to use for custom tooling. Add `--summary-only` to emit only per-file totals.

### Text

```sh
cargo llvm-cov --workspace --text
```

Plain-text report, printed to stdout when `--output-path` is omitted.

### Cobertura

```sh
cargo llvm-cov --workspace --cobertura --output-path cobertura.xml
```

Cobertura XML, for CI systems that expect that format.

## Per-crate coverage

Restrict the run and the report to one crate with `-p`:

```sh
cargo llvm-cov -p nu-agent-core --html
cargo llvm-cov -p nu-agent-tui --lcov --output-path lcov-tui.info
```

`--exclude` removes a crate from both the test run and the report. `--exclude-from-test` and `--exclude-from-report` remove it from only one of the two.

## Coverage profile

The workspace defines a `coverage` profile in `.cargo/config.toml`:

```toml
[profile.coverage]
inherits = "test"
debug = "line-tables-only"
opt-level = 0
```

Select it with `--profile`:

```sh
cargo llvm-cov --workspace --profile coverage
```

The profile inherits from `test`, keeps debug info at `line-tables-only` to reduce artifact size, and disables optimization so coverage counters map cleanly to source lines.

## Interpreting results

| Metric | Meaning |
|--------|---------|
| Line coverage | Percentage of executable source lines hit by at least one test |
| Region coverage | Percentage of LLVM coverage regions hit; finer-grained than lines |
| Function coverage | Percentage of functions called by at least one test |

A line that is never executed is either dead code or untested code. Both are worth a look: delete the dead code, or add a test that exercises the path.

### Test files in the report

`cargo-llvm-cov` excludes these paths by default:

- any `tests/`, `examples/`, or `benches/` directory under the workspace root
- files named `tests.rs`, `<name>_tests.rs`, or `<name>-tests.rs`
- `target/`, the cargo registry, and rustup toolchains

This repository keeps test files in a `test/` directory (singular) with no `_test.rs` suffix, so those files are **not** excluded by the default patterns. Exclude them explicitly:

```sh
cargo llvm-cov --workspace --html --ignore-filename-regex '(/test/|/test$)'
```

`--no-default-ignore-filename-regex` disables the built-in exclusions entirely.

## Thresholds for CI

`--fail-under-lines` exits with status 1 when total line coverage falls below the given percentage:

```sh
cargo llvm-cov --workspace --fail-under-lines 80
```

Related flags:

| Flag | Fails when |
|------|-----------|
| `--fail-under-lines <MIN>` | Total line coverage is below `MIN` percent |
| `--fail-under-functions <MIN>` | Total function coverage is below `MIN` percent |
| `--fail-under-regions <MIN>` | Total region coverage is below `MIN` percent |
| `--fail-under-file-lines <MIN>` | Any single file's line coverage is below `MIN` percent |
| `--fail-uncovered-lines <MAX>` | Uncovered lines exceed `MAX` |
| `--fail-uncovered-functions <MAX>` | Uncovered functions exceed `MAX` |

No threshold is enforced yet. Pick a number after the first full run establishes a baseline, then add the flag to CI.

## Test profiles

Tests run under [`cargo-nextest`](https://nexte.st). Profiles are defined in `.config/nextest.toml`:

| Profile | Command | Purpose |
|---------|---------|---------|
| `default` | `cargo nextest run` | Unit tests only — fast feedback, high parallelism |
| `integration` | `cargo nextest run --features integration --profile integration` | All tests — lower parallelism, one retry for flaky server tests |
| `ci` | `cargo nextest run --features integration --profile ci` | Full suite for CI — no retries, JUnit XML output |

Integration test modules are gated behind the `integration` cargo feature, so the default profile runs unit tests only. The `integration` feature is defined in `nu-agent-a2a`, `nu-agent-core`, `nu-agent-tui`, and `nu-agent`; enabling it on `nu-agent` or `nu-agent-tui` propagates to the other two crates.

The Nix dev shell wraps the profiles as `tests` (unit only), `tests-all` (unit + integration), and `tests-integration` (integration only, with retries).

Coverage runs against the same profiles. `cargo-llvm-cov` accepts a nextest subcommand:

```sh
cargo llvm-cov nextest --workspace --features integration --html --ignore-filename-regex '(/test/|/test$)'
```

Without `nextest`, `cargo llvm-cov` runs `cargo test`, which covers unit tests only unless `--features integration` is passed.

## CI integration

No CI workflow exists yet. When one is added, the LCOV path is the one to use:

```sh
cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 80
```

Upload `lcov.info` to Codecov or Coveralls. `--codecov` emits Codecov's custom JSON format instead, which carries region coverage that the LCOV path drops.

The test job runs `cargo nextest run --features integration --profile ci`, which writes `junit.xml` for the CI test report. The lint job runs `cargo clippy --all-features --tests -- -D warnings` so integration test code is linted too.

## Merging runs

Coverage from several runs under different conditions can be merged. Use `--no-report` to collect data without generating a report, then `report` to combine:

```sh
cargo llvm-cov clean --workspace
cargo llvm-cov --no-report --features a
cargo llvm-cov --no-report --features b
cargo llvm-cov report --lcov --output-path lcov.info
```

`cargo llvm-cov` cleans stale build artifacts before a run to avoid false results. `--no-clean`, `--no-report`, and `--no-run` disable that cleaning, so run `cargo llvm-cov clean --workspace` first when using them.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `error: no such command: llvm-cov` | `cargo-llvm-cov` is not installed | `cargo install cargo-llvm-cov --locked` |
| `llvm-tools-preview` component missing | rustup component not installed | `rustup component add llvm-tools-preview` |
| Coverage numbers change between runs | Stale build artifacts | `cargo llvm-cov clean --workspace` |
| Test files appear in the report | Default exclusions cover `tests/`, not `test/` | Pass `--ignore-filename-regex '(/test/|/test$)'` |

## Known limitations

- Branch coverage (`--branch`) and doc-test coverage (`--doctests`) are unstable and require nightly.
- Coverage measures execution, not correctness. A line can be covered by a test that asserts nothing.
