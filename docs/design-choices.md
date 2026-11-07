# Design Choices

Patterns this workspace follows. Read this before adding a crate, a layer, an event, or a rendering path.

Each rule states what it prevents, so you can recognize the mistake instead of memorizing the rule. **Status** says whether the pattern is enforced in the tree today or is the direction the migration is moving in. Do not cite an unimplemented pattern as if it exists; do not cite an implemented one as if it is still open.

This document is per-change guidance. The properties the **target** must satisfy, the numbered checks that enforce them, and the rule by which a slice is accepted are in `docs/target-architecture.md`.

---

## 1. A crate boundary exists to forbid an edge

**Status:** enforced.

A crate split is justified only by a dependency that must be impossible to express. "Different concern" is not a reason to split — that is a module.

Forbidden edges:

| Crate | Must not reach |
|---|---|
| `agnt-kernel` | rig, orchestration, any surface, the test kit |
| `agnt-storage` | the runtime |
| `agnt-render` | any terminal library |
| `agnt-tui`, `agnt-tty`, `agnt-acp` | each other |
| production crates | the test kit |

**Completion test for the whole migration:** `cargo tree -p agnt-cli --edges normal` lists no `agnt-core`.

- ✅ A new crate when the wall makes an illegal call a compile error.
- ❌ A crate "to keep things tidy". `agnt-runtime` is a monolith by design — that is what a composition root is.

---

## 2. The kernel is a data plane, not a processing stage

**Status:** enforced for records and display types. The transcript fold lives in `agnt-storage` (see §8).

`agnt-kernel` carries records, events, the bus, rings, cancel state, and the methods that keep that data coherent. It may contain moving parts.

What it may not contain is a **transformation stage**: a reshape or mapping whose beneficiary is another layer. The test is not "does it move" but **"who does the reshape serve?"** If only a renderer benefits, or the work exists to put data into another layer's preferred shape, it belongs to that layer.

Corollaries:

- The kernel prints nothing and supplies no text mapping. A layer that needs text declares its own trait and implements it for the kernel type.
- No `as_str`-style table on a kernel enum. Parsing text **into** a variant at the boundary is correct and belongs there (`impl FromStr for BuiltinKind`); rendering a variant back to text does not.
- **Identity contract.** An entity derives its identifier however it likes — content hash, counter, uuid — provided it keeps the contract:
  1. **Derived, never supplied.** The id comes from the entity's own content, or from a monotonic source inside the owning layer. A caller never hands one in.
  2. **Deterministic where replay matters.** If the entity crosses a persistence boundary, the same content must yield the same id in any process. A random value there is a defect, not a choice.
  3. **Unique where replay does not matter.** An allocated id (a permission request, a session) need only be unique within its stated scope.
  4. **Typed.** One newtype per entity. No bare `String` or `u64` in a signature.
  5. **Immutable and unreachable.** Fixed at construction, no public field, no setter. A record that can be built with an id disagreeing with its content is the defect.
  6. **Scoped.** The collision domain is stated where the id is defined — per process, per session, per store — so a short hash is a decision, not a hope.
  7. **Identity is not display.** An id may be rendered; a rendered value is never an id.

  Value objects are exempt: no identity, equality by value. No shared `Identifiable` trait exists until a consumer must be generic over identity; when one appears it carries the associated id type, never a string.
- No *display* derived from a record is stored on it (see §3). An identifier is not a display.

---

## 3. A producer ships data; the drawer derives the display

**Status:** enforced (`agnt-render::tool_view`).

A display payload built by the producer is the defect. Relocating it to a third crate is not a fix — there is no crate both sides may see, and that is the design saying the payload is wrong.

- ✅ The drawer owns its vocabulary and implements its trait **for the data type it receives**:
  ```rust
  // agnt-render — local trait, foreign type, orphan-legal
  pub trait ToolView {
      fn call_line(&self) -> String;
      fn result_body(&self, _payload: Option<&str>) -> Option<Display> { None }
      fn preview_body(&self, _content: Option<&str>) -> Option<Display> { None }
  }
  impl ToolView for ToolCall { … }
  ```
- ✅ Call sites are ordinary methods: `call.call_line()`, `call.result_body(payload)`.
- ✅ The discriminator is **derived on use**, never stored (`ToolCall::kind()`).
- ❌ A free function that takes a name or a kind and matches to pick behaviour (`render_for`, `tool_display_from_result`) — every call site re-decides what the type already knew.
- ❌ A `&dyn` table, a `static VIEWS: [...]`, or a wrapper whose `new()` resolves nothing.

**Adding a tool that draws something** is one arm in `call_line` and, if it draws a result or a preview, one arm in `result_body` / `preview_body`. `tool_view.rs` is the only file that changes.

---

## 4. Text is parsed once, at the boundary

**Status:** enforced.

- ✅ `impl FromStr for BuiltinKind` is the single place a name becomes a variant; everything downstream carries the variant.
- ✅ A consumer that acts on a distinction matches an enum, so adding a variant fails to compile until its arm exists.
- ✅ User-visible strings are frozen: whoever owns the text reproduces it byte-for-byte.
- ❌ Branching behaviour on a name string, a message prefix, or a serialized token.

**A layer that needs text declares its own trait on the kernel type.** The session-grant cache keys on the `ToolSource` variant, not on its label. `ToolSourceText` in `agnt-runtime` writes the hook envelope's `source`; `ToolSourceLabel` in `agnt-tty` writes the tool lines. Neither trait crosses to the other layer, and the kernel supplies no mapping.

---

## 5. The composition root is builder-gated

**Status:** enforced — `AgentRuntime` declares no public field; `compose` is the one way in, and ARCH-9.a's baseline is empty.

Completeness is a compile-time property: every required part is a constructor parameter, so omitting one does not compile.

```rust
AgentRuntime::compose(provider, tools, session, persona, multi, engine, bus)
    .with_decision_source(resolver)   // default: pre-determined policy
```

- ✅ Fields are private; surfaces and tests drive the runtime through its traits.
- ✅ Derived internal state (circuit breaker, doom state, usage counters, store latch, hook context, compaction config) initialises inside `compose` from config — it is not a composition parameter.
- ❌ A constructor that takes one argument with the rest reachable by mutation — a half-built config with no signal for what is missing.
- ❌ A configuration type whose `Option` fields cannot all be set from a real input path (env var, `config.toml` key, or CLI flag).

---

## 6. The loop has an owner

**Status:** direction — the free-function family still exists.

A free function that constructs its own collaborators cannot be injected, whatever bounds its parameters carry. Dependency inversion fails above the point where the dependency is constructed, not at the signature.

```rust
pub struct Loop<R, F = NoEventSink, H = NoAgentSwitchHook> {
    runtime: R,
    config: LoopConfig<F, H>,
    bus: Bus,
    sources: SourceChannels,   // owned receivers
    ctx: LoopContext,          // owns the state, not borrows into locals
}

impl<R, F, H> Loop<R, F, H> {
    pub fn compose(runtime: R, config: LoopConfig<F, H>) -> Self;
    pub async fn run<S: StageSet>(mut self, mut stages: S) -> (R, Result<Value, LabeledError>);
}

// one public entry
pub async fn run(runtime: R, config: LoopConfig<F, H>) -> Result<Value, LabeledError>
```

- ✅ The body is named phases: `hydrate`, `announce_facts`, `spawn_worker`, `select`, `teardown`. Each is a method with one job and is reachable from a test without spinning the select loop.
- ✅ Stages arrive as **one composed value** through a `StageSet` trait; the existing four-handler struct is its production impl. A test supplies one mock.
- ✅ The loop owns its receivers and its state; a `parts()`-style disjoint borrow view replaces `&mut` handles into locals.
- ✅ Async trait methods are declared `-> impl Future<Output = …> + Send` (as `AskApprovalHook::choose` does) so dispatch stays static.
- ❌ Function names that encode a parameter or a variant: `run_interactive_loop_with_external_prompts`, `run_hydrated_…`, `*_impl`. Parameterisation is the type system's and the builder's job.
- ❌ Two public functions whose bodies are identical and whose names differ by which config field the caller intends to set.

---

## 7. One way in; behavior lives on its owner

**Status:** enforced for new code.

- ✅ Behavior is a method on the type or trait that owns the data. A rename is a re-homing.
- ✅ A second entry point is a trait seam, never another function.
- ❌ A free function that hugs another type's properties — reads its fields, combines them, or forwards to its method.
- ❌ A variant-function family, a companion helper module beside a trait, or a one-off wrapper that adds a layer without adding behavior.

---

## 8. The store owns the transcript ETL

**Status:** direction.

What goes into a transcript for posterity ends up in `agnt-storage`, and that is where the ETL layer lives. The same store feeds re-hydration after a session reload, so live and replayed data converge because both derive from one transcript.

- ✅ `agnt-storage` owns the stream→transcript fold.
- ✅ Surfaces keep projection caches over what the store hands them; a cache is not a source of truth.
- ❌ A canonical fold in the kernel.
- ❌ A surface-owned fold as the canonical source for other surfaces.
- ❌ Rendering a derived line into a persisted row: the row holds the data, the drawer derives the line.

---

## 9. Permissions are a pause point, not a decision engine

**Status:** enforced.

- ✅ The gate parks the call, publishes a request, waits on a oneshot, and subscribes to cancellation **before** publishing, so a cancel cannot be missed.
- ✅ A timeout resolves to **deny**.
- ✅ The decision source is a per-surface capability: realtime for the interactive surface, pre-determined policy elsewhere (ask ⇒ deny).
- ✅ The permission card is derived by the drawer from the call; the event carries at most content only the producer can supply (an edit's proposed diff). A replayed row publishes the request with no preview content, because that content was never in the transcript.

---

## 10. Testing: through the public boundary, mocks at the edges

**Status:** enforced.

- ✅ Drive behaviour through the real public API. Substitute only at genuine external edges: the LLM wire, the tool environment, the MCP transport, the clock.
- ✅ Inject collaborators through traits; the mock lives in the test tree, never in production code.
- ✅ `#[tokio::test(start_paused = true)]` plus time advance for timeout paths — a clock trait is not needed.
- ❌ `#[cfg(test)]` on a function in production, or a `pub(crate)` field added so a test can peek. If the API cannot express the assertion, the API is missing a method.
- ❌ Tests that assert compiler-enforced properties (`Send`, `Sync`, trait bounds, `Clone`).
- ❌ A fixture that claims a source or a line the producer cannot produce.

Test layout: `crates/<crate>/test/` mirroring `src/`, declared from the production module with `#[cfg(test)] #[path = "../test/…"] mod …;`. Unit tests complete in under a second; anything that binds a port, opens a database, or builds a real HTTP client is an integration test and sits behind the `integration` feature.

```
cargo nextest run                                  # unit only
cargo nextest run --features integration           # unit + integration
cargo clippy --workspace --all-features --tests -- -D warnings
```

---

## 11. Where a new thing goes

| You are adding | It goes in | Constraint |
|---|---|---|
| A record, an event, a bus channel | `agnt-kernel` | data only; no text mapping, no rendering |
| A drawing of something | `agnt-render` | a trait impl on the record, static dispatch |
| Orchestration, a tool, config resolution | `agnt-runtime` | composition root |
| Persistence, a transcript shape | `agnt-storage` | owns the ETL; never reaches the runtime |
| Input or output for a client | its surface crate | translate and project only; no core semantics |
| A shared test fixture | `agnt-testkit` | dev-dependency only |

If a change seems to need a new crate, name the edge it must forbid first. If you cannot name one, it is a module.
