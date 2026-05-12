# Reviewer Checklist: Agent CLI Flag Surface Cleanup v1

Scope: task `6e574bcd` and subtasks `4a8e7a21`, `429e7321`, `403ca0e1`, `409b84f1`, `41165ec1`, `44c29089`.

## Final verification commands

- `cargo test --workspace`
- `cargo clippy --all-targets --all-features -- -D warnings`

Expected: both commands exit `0`.

## Checklist

- [ ] Signature/help no longer includes removed flags:
  - [ ] `--provider`
  - [ ] `--max-tokens`
- [ ] Parser clearly rejects removed flags as unknown options (via Nushell/plugin parser behavior).
- [ ] Runtime argument extraction no longer reads removed flags.
- [ ] Authoritative paths remain:
  - [ ] provider/model selected through `--model` (provider/model format)
  - [ ] token limits configured via `--max-output-tokens` and `--max-context-tokens`
- [ ] Existing non-removed flags preserve behavior.
- [ ] Documentation/examples contain no references to removed flags.

## Evidence pointers

- Command signature and resolution logic:
  - `src/agent/application/command/mod.rs`
  - `src/agent/application/command/runtime_build.rs`
- Regression and surface tests:
  - `src/agent/application/command/test.rs`
- Usage docs:
  - `docs/usage.md`
