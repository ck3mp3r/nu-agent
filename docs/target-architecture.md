# Target Architecture — Acceptance Criteria

What the surviving crates must be, and how we prove it. This is not a description of today's tree and it is not a task list. It is the definition of **done** for the core strangler: a set of properties that hold for the crates that remain, and a set of checks that fail the build when one stops holding.

Read `docs/design-choices.md` for the patterns to follow in a single change. This document answers a different question: *is the system we are building toward structurally sound, and how would we know?*

**The rule this document exists to enforce:** a green test suite certifies behaviour, not structure. A suite written against the current seams certifies the current seams. Every slice is therefore accepted against the assertions below, not against "tests pass".

---

## 1. The crates that survive

One responsibility each, stated as the reason it would change.

| Crate | Holds | Changes when | May depend on |
|---|---|---|---|
| `agnt-kernel` | records, domain events, the bus, rings, record identity | the wire vocabulary or record vocabulary changes | nothing |
| `agnt-storage` | schema, migrations, services, the stream→transcript fold | the persisted shape or the fold changes | kernel |
| `agnt-render` | transcript vocabulary, call lines, drawer traits | how something is drawn changes | kernel |
| `agnt-runtime` | config, content, orchestration, tools, hook, permissions, session | how the system is assembled, driven, or configured changes | kernel, storage |
| `agnt-tui`, `agnt-tty`, `agnt-acp` | input translation and projection caches | how a client sees or says something changes | render, kernel, runtime |
| `agnt-a2a` | transport | the wire protocol changes | kernel |
| `nu_plugin_agent` | the Nushell boundary | the plugin protocol or its command surface changes | runtime |
| `agnt-cli` | argument parsing and composition | the CLI surface changes | runtime, surfaces, a2a |
| `agnt-testkit` | the test fabric | test infrastructure changes | dev-dependencies only |

`agnt-core` is absent by design. Its deletion is the migration's completion, and the test is `cargo tree -p agnt-cli --edges normal` naming no `agnt-core`.

---

## 2. Properties, expressed as checks

Each property has one assertion and one observable failure. Assertions are numbered so a task can require that a specific one flips from failing to passing.

### SRP — one reason to change per crate

**ARCH-1.** A change to how something is displayed must not touch a crate outside `agnt-render` and the surface that displays it.
*Measured by:* the file set of a representative change; review against the crate table in §1.

**ARCH-2.** `agnt-runtime` must not contain rendering vocabulary and `agnt-render` must not contain orchestration vocabulary.
*Measured by:* a symbol scan for a maintained term list over each crate's public API.

### OCP — one crate owns each extensible axis

**ARCH-3.** Adding a builtin tool changes `agnt-runtime` and, only if the tool draws something, `agnt-render`. Adding a surface changes that surface crate and `agnt-cli`. Adding an event variant changes `agnt-kernel` plus the consumers that act on it.
*Measured by:* the file set of the change, asserted in review against the axis table.

### LSP — one concept, one type

**ARCH-4.** A distinction has exactly one type across the workspace. Two enums for one distinction, or an enum that must mirror a foreign library's string names, is a violation.
*Measured by:* a census of enums whose variant names or string forms overlap. Current known violation: the two tool-error kinds.

**ARCH-5.** The end-to-end fabric runs the same code path with mocks and with real adapters. If a mock path exists that production cannot take, the abstraction is wrong.
*Measured by:* the E2E suite; mocks are permitted only at the LLM wire, the tool environment, the MCP transport, and the clock.

### ISP — a consumer imports only what it uses

**ARCH-6.** Every `pub` item has at least one production consumer outside its own crate, or is a documented entry point with a named caller.
*Measured by:* a public-surface census. Current known violations: the `UiRenderer` trait, whose consumer set is a single surface, and orphaned items left by earlier moves.

**ARCH-7.** No consumer is required to satisfy a capability it never calls. A fake implements only what its test exercises.
*Measured by:* for each trait a fake implements, at least one test must reach each method, or the trait is split.

**ARCH-8.** No function outside the composition root takes the whole bus. A component takes the channel it publishes to or subscribes to.
*Measured by:* a signature scan. Direction, not yet true.

### DIP — dependencies point at abstractions

**ARCH-9.** Only the composition root constructs infrastructure: HTTP clients, database handles, TLS providers, subsystem tasks. Every other crate receives what it needs.
*Measured by:* a construction scan over non-root crates for the relevant constructor calls.

**ARCH-10.** No consumer names the aggregate runtime type. Consumers take the narrow trait they use; the aggregate exists only where it is assembled and in the production alias.
*Measured by:* a reference count per crate. Direction, not yet true.

### The kernel's own boundary

**ARCH-11.** No field on a kernel record is a rendering: no pre-rendered string, no display section, no label, no kind-as-text. Identity fields are permitted and expected, and they obey the identity contract in `docs/design-choices.md` §2.
*Measured by:* a field census over kernel records, classified by the consumer test in `docs/design-choices.md` §2: a consumer that *finds* may store; a consumer that *shows* may not. The mechanical parts of the identity contract are checked too — an identity field must be a typed newtype, must not be publicly settable, and on any record that crosses a persistence boundary must be derivable deterministically from the record's own content.

**ARCH-12.** `agnt-kernel` has zero non-test workspace dependencies.
*Measured by:* `cargo tree`.

---

## 3. Corrections this document makes to our own target design

These were defects in the first draft of the target, not in today's code. They are listed because a check written against the first draft would enforce the wrong thing.

1. **A single `StageSet` trait is an ISP violation.** One trait with four methods forces every test mock to implement all four. Fix: four narrow capabilities with default no-op methods, composed by a blanket impl; a test implements only what it exercises. ARCH-7 enforces this.

2. **`&Bus` is a god-parameter.** Eight accessors means every component can publish anywhere. Fix: components take the specific channel. ARCH-8 enforces this.

3. **The aggregate runtime may implement many traits; consumers must not name it.** Five trait impls on a composition root is a façade, not a god object — provided nothing takes `&AgentRuntime`. ARCH-10 enforces this.

4. **"Derived, never stored" needs its identity carve-out, stated precisely.** Identity is a function of the entity's own content — `to_hash`, not an id handed in by a caller — so the capability belongs on the entity. It may be kept, because recomputation is how a live record and a replayed one drift apart; a display is derived everywhere. And the invariant has to be *enforced*: if the identifier sits in a public field or the record is publicly constructible, a record can exist whose id disagrees with its content. ARCH-11 checks both the field's nature and its reachability.

5. **A display mapping must never be load-bearing for behaviour.** The current grant cache keys on a label; it must key on the enum and the call's own data. ARCH-4 and ARCH-11 together catch the general form.

---

## 4. How the checks are enforced

An architecture test suite, run in the existing CI profile, that reads the workspace rather than running it: `cargo metadata` for the graph, AST queries for definitions, references, signatures and field types.

- It lives in the test kit, so every crate can assert against it without introducing a new dependency direction.
- Ten gate checkers are installed — the `BASELINES` array is `[(&str, &[&str]); 10]`, one row per checker (`ARCH_2`, `ARCH_4`, `ARCH_5`, `ARCH_8`, `ARCH_9.a`, `ARCH_9.b`, `ARCH_10`, `ARCH_11.a`, `ARCH_11.b`, `ARCH_12`); `crates/agnt-testkit/src/architecture/mod.rs`'s `REVIEW_CHECKS` records the review side of the gate: the assertions with no checker (`ARCH-1`, `ARCH-3`, `ARCH-7`), and the non-mechanical halves of installed assertions (the `ARCH-8` 17-site ratchet — 11 struct fields, 5 parameters, 1 return — that only ever shrinks, with its two exempt assembly files; the `ARCH-9` state rows and `ARCH-10`'s three structurally invisible doc lines; and `ARCH-11`'s enum-variant half).
- Each assertion reports its identifier with the offending site, so a failing build names the rule and the line.
- Assertions that cannot be mechanical yet are listed as review checks, not silently dropped.

A precedent already exists in the tree: a TUI test that pins a structural invariant by reading source. The suite generalises that pattern.

---

## 5. How a slice is accepted

1. The slice names the assertion it flips, or states that it preserves all assertions.
2. The full suite runs; the named assertion goes from failing to passing; no other assertion regresses.
3. The behavioural suite remains green — necessary, not sufficient.
4. "Tests pass" alone is not acceptance. If a slice cannot name an assertion, it is a refactor without a structural claim, and that is a reason to question it.

---

## 6. Sequence forced by the assertions

The order is a dependency claim, to be verified against the assertions rather than assumed. It has largely landed: the status after each row states what is installed today, taken from the test kit's `BASELINES` array.

1. **ARCH-4 and ARCH-11** — one tool-error type, no display on a record. Mechanical, small blast radius, and they remove ambiguity the later slices would otherwise have to navigate. *Status: landed for the mechanical halves — `ARCH_4` ratchets twelve declarations over six names (each declared twice), two of them the ruled parallel `RenderInput` drawer types, `ARCH_11.a` holds an empty baseline, and so does `ARCH_11.b`; `ARCH-11`'s enum-variant half stays a review row.*
2. **ARCH-8** — bus capability parameters. Mechanical, and it shrinks what the loop slice has to carry. *Status: landed and covered — the checker matches every shape (`&Bus` in any spelling, an owned `Bus`, and an `Arc<Bus>`/`Box<Bus>` wrapper) at all three positions (parameter, return type, struct field); `ARCH_8`'s baseline is the 17-site ratchet, which only ever shrinks, and the two assembly files (`conversation/runtime.rs`, `conversation/runtime_parts.rs`) are exempt as the composition root. The nine hook-layer sites that took `&Bus` were re-cut to the narrow hands their bodies publish on, so no hook-layer signature carries the whole bus. The measured list and its exemptions are the `ARCH-8` block of the test kit's `REVIEW_CHECKS`.*
3. **ARCH-9 and ARCH-10** — the composition-root gate and consumer naming. *Status: landed as ratchets — `ARCH_9.a` is empty, `ARCH_9.b` locks ten construction sites (seven `agnt-runtime` rows — six for the shared-client factory and one installer body —, two `agnt-cli` rows — the A2A transport's client construction and the binary's own provider install —, and one `agnt-plugin` row for the plugin binary's own composition root; `agnt-a2a` declares none, since its browse thread fetches through the transport its composition root hands down), and `ARCH_10` locks the nine recorded acp sites with the CLI drained. The rows are re-recorded from an executed run, so their line numbers follow the files they point at.*
4. **The loop owner** — phases and one entry; depends on 8 and 10 being settled. *Status: landed — `crates/agnt-runtime/src/orchestrator/loop/mod.rs` holds the `Loop`, its named phases, and the single `run` entry.*
5. **ARCH-6** — public-surface census, applied continuously as crates lift. *Status: landed for the census half — the checker pins every declared `pub` item no other crate reaches, ratcheted per crate under `test/architecture/baselines_arch6/`, and the baseline only ever shrinks; the judgement half ("documented entry point with a named caller") stays a review row until the drain ruling.*
6. **`agnt-core` deletion** — last, gated by ARCH-1 through ARCH-12 together. *Status: landed — no `agnt-core` member remains and `cargo tree -p agnt-cli --edges normal` names none.*

---

## 7. Open decisions that block an assertion

| Decision | Blocks |
|---|---|
| **Settled.** The four class names stand — `facts` included — because the per-channel contract now exists (`docs/bus-channels.md`): each channel is named after the guarantee it provides (drop policy, ordering, late-subscriber rule), and `ui_state`/`ui_event` are retired | ARCH-2's term list, and the channel split — settled by the D4 landing: `stream`, `cancel`, `control`, `lifecycle`, `narration`, `facts` |
| Where allocated identifiers are minted — kernel or composition root | ARCH-11's field census |
| The transcript record's home and shape (the fold belongs to storage) — **Settled** by the transcript-record landing: storage-owned `TranscriptRecord`/`TranscriptRow`/`TranscriptStamp` + `TranscriptId` (FNV-1a identity with explicit constants), the rehydration read path in the storage service (`Entries::record_transcript`); M3 to M4 | ARCH-6 — no longer blocked by this row (its blocker is now the allowance-row question, per the landing's ARCH-6 note; the ARCH-6 checker slice owns that census); the storage-surface workstream keeps its half |
| Whether the aggregate runtime keeps five trait impls | ARCH-10's exact form |

---

## 8. What this document is not

- Not an inventory of today's defects. Today is inventory for the move, not the criterion.
- Not a task list. Tasks reference assertions; this document defines them.
- Not a claim that the current tree is sound. It is the definition by which the target will be judged.
