# Reviewer Checklist: CLI `--permissions` overlay (v1)

## Scope confirmation

- [ ] CLI override surface is only `--permissions` (record/object)
- [ ] No repeated legacy `--permission` parsing path introduced
- [ ] Existing ask/session-grant semantics remain unchanged

## P1 Parser / validation

- [ ] Reject non-record CLI value
- [ ] Reject malformed nested maps (`nu__run` supports only `command`)
- [ ] Fail-fast errors include explicit key path

## P2 Merge determinism

- [ ] Effective policy uses additive merge: base config + CLI overlay
- [ ] CLI wins on overlapping leaf/action keys
- [ ] Non-overlapping config keys remain
- [ ] Nested `nu__run.command` keys merge deterministically

## P3 Runtime wiring

- [ ] Effective merged policy built once at startup
- [ ] Dispatch decisions use merged policy for all tool calls

## P4 Startup diagnostics

- [ ] Startup summary indicates overlay active state
- [ ] Summary includes concise policy counts and baseline action

## P5 Security regressions

- [ ] Overlay does not erase unrelated config permissions
- [ ] Overlapping keys override as intended
- [ ] Malformed input fails fast (no permissive fallback)
- [ ] Runtime decisions match merged policy

## P6 Docs/examples

- [ ] `docs/usage.md` includes Nu examples for `--permissions`
- [ ] Docs describe additive merge + precedence
- [ ] Docs include troubleshooting/malformed payload guidance

## P7 Verification evidence

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
