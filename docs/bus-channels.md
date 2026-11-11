# Bus Channels

The delivery contract of every channel the kernel `Bus` carries. Read this before publishing on a channel or subscribing to one.

A channel is chosen by **what happens when it runs ahead of its consumers**, not by what the payload looks like. Two payloads that both describe a turn belong on different channels if one of them cannot be lost and the other can.

Everything here lives in `agnt-kernel/src/bus/`. The kernel holds the channels and their vocabularies; no surface vocabulary appears at the kernel edge.

---

## The six concerns

| Channel | Payload | Mechanism | Capacity | Drop policy | Ordering | Late subscriber |
|---|---|---|---|---|---|---|
| `stream` | `SessionRecord` | bounded broadcast | 1024 | loses the oldest under lag | publication order while the consumer keeps up; re-syncs at the tail | sees nothing already published |
| `cancel` | `CancelEvent` | bounded broadcast | 64 | never dropped | the single request | must subscribe **before** the request |
| `control` | `ControlCommand` | unbounded fan-out | uncapped | **never dropped** | publication order | must subscribe **before** the publish; control is consumed, not replayed |
| `lifecycle` | `LifecycleEvent` | unbounded fan-out | uncapped | **never dropped** | publication order | must subscribe **before** the publish |
| `narration` | `NarrationEvent` | bounded broadcast | 1024 | drops the **oldest** tail; the consumer skips and resumes | receipt order holds after the skip | sees nothing already published |
| `facts` | `FactRecord` | bounded broadcast | 256 | drops intermediate snapshots | publication order while the consumer keeps up | sees nothing already published; the next record is a whole value |

Capacities are declared beside their concerns (`stream.rs`, `cancel.rs`, `narration.rs`, `facts.rs`), never at a call site.

---

## `stream` — the session record log

`SessionRecord`: inference, tool, permission, compaction, notice, session, turn. The ordered log of what happened in a session.

- **Drop policy:** bounded and lossy. A consumer that stalls past 1024 records receives `Lagged` and re-syncs at the tail.
- **Why bounded:** the stream is a delivery surface, not a log. Durable history belongs to the session store.
- **Consumers** fold whichever record families they need; a record is never a render instruction.

## `cancel` — the steering signal

`CancelEvent::Requested`.

- **Drop policy:** never dropped. Its ring is independent of the record stream, so a stream flood cannot delay a cancellation.
- **Ordering is the contract:** a consumer that must observe cancellation subscribes first. The resolver relies on this; the 64-slot ring can never lag because one request is the whole vocabulary.

## `control` — commands to the loop owner

`ControlCommand`: `EnqueueExternalPrompt`, `PermissionDecided`, `CompactionRequested`, `SharedUiAction`.

- **Drop policy:** never dropped. The fan-out has no ceiling: each subscriber gets its own unbounded queue, and a queue whose subscriber has dropped is pruned.
- **Why:** a command that is lost is a command that never ran. Dropping an `EnqueueExternalPrompt` silently loses a task; dropping a `PermissionDecided` blocks the gate until its timeout.
- **The enum grows by adding variants.** Control never carries state to be shown and never carries a value a caller waits on.
- **Control carries commands that cross a boundary.** A row exists when a publisher outside the receiving loop asks it to act: the router handing an accepted task to the loop, the surface answering an open card, the conversation asking for a sweep. An intake the loop performs for itself — a completion event the loop receives and dispatches on the spot — is an internal action, not a row. Publishing such an action onto `control` would make the loop consume its own command and act twice; a test observes it on the worker command channel, not on `control`.

### Ownership

Control has one consumer per command, so each command has one owner. Control is **consumer-side ignoring**: every subscriber matches the enum exhaustively and documents its ignore arms in a comment. There is no sub-class split and no per-consumer channel — a consumer that does not own a command ignores it by name.

| command | owner |
|---|---|
| `EnqueueExternalPrompt` | loop |
| `PermissionDecided` | loop |
| `CompactionRequested` | loop |
| `SharedUiAction` | TUI |

### Subscriptions

A consumer subscribes to the concerns it folds, and nothing else. `control` is subscribed only where the consumer owns a command. A concern a consumer only **publishes** on is not a subscription: it holds the transmit hand, never a receiver, and the cell says so.

| consumer | stream | cancel | control | lifecycle | narration | facts |
|---|---|---|---|---|---|---|
| TUI (`agnt-tui/src/interactive.rs`) | yes | no — the `CancelController` `AtomicBool` latch; the CLI publishes `CancelEvent` | yes (`SharedUiAction`) | no — `RenderInput::Lifecycle` is an ignore arm; only the TTY loop feeds it | yes | yes |
| TTY (`agnt-cli/src/mode/stderr.rs`) | yes | publishes — `CancelEvent::Requested` | no | yes | yes | no |
| ACP (`agnt-acp/src/session.rs`) | yes | publishes — `request_cancel`, behind the session's `AtomicBool` latch | yes (`CompactionRequested`) | publishes — `ModelSwitched` | yes | no |
| hooks runner (`agnt-runtime/src/hooks/runner.rs`) | yes | no | no | yes | no | no |

The TUI's four hands are the render loop's channels: `stream`, `narration`, `control`, `facts`. Cancellation never reaches it as a record — a keypress sets the `CancelController` latch and pushes `OrchestratorEvent::CancelRequested`, and the CLI publishes `CancelEvent::Requested` on `cancel` itself. The fold's `RenderInput::Lifecycle` arm exists so the surface has one intake vocabulary; it folds to `false`, and its only producer is the TTY loop.

The session-class hook runner takes `HookSources` — `stream` and `lifecycle` — and gives each hook only the concerns it declares events on.

### The hook layer's narrow hands

A hook function takes only the hands its own body publishes on, never the bus. The table records what each site in the hook layer carries.

| site | hands |
|---|---|
| `hook/chain/compaction.rs::decide_compaction` | `control` + `stream` + `lifecycle` |
| `hook/chain/compaction.rs::current_conversation` | `stream` + `lifecycle` |
| `hook/chain/compaction.rs::emit_store_dead` | `stream` + `lifecycle` |
| `hook/chain/compaction.rs::emit_compaction_failed` | `stream` + `lifecycle` |
| `hook/chain/compaction.rs::run_compaction` | `stream` + `lifecycle` |
| `hook/permission_resolver.rs::publish_auto_resolved` | `stream` + `facts` |
| `hook/permission_resolver.rs::resolve` (policy) | `stream` + `facts` |
| `hook/permission_resolver.rs::resolve` (interactive) | `stream` + `facts` + `cancel` |

The permission sites reach `facts` and `stream` because a gate is one value on two concerns: the record every consumer folds and the snapshot a surface draws. The policy resolver draws no card, so it ignores the `cancel` hand; the interactive resolver subscribes to it before it publishes the request, which is the ordering `cancel` requires.

## `lifecycle` — the turn spine

`LifecycleEvent`: the turn spine (`PromptSubmitted` → `TurnStarted` → `TurnCompleted`), the session sequence, the compaction sequence, and the external task outcomes.

- **Drop policy:** never dropped. `Lifecycle` is the same unbounded fan-out as `Control`.
- **Why:** the spine is the attribution map's input. A skipped step leaves a turn unattributed rather than merely unseen, and a missing compaction outcome cannot be re-derived from anything else.
- **Every spine step carries the task id it belongs to.** The per-task subsequence is ordered and complete; interleaving between tasks is expected and permitted.
- **Subscriptions are opened before the first publish**, because a late subscriber is not handed the records it missed.

## `narration` — text for a human to read

`NarrationEvent`: inference start, chunk, completion, stop; compaction summary chunks; system messages.

- **Drop policy:** order-critical with an **oldest-droppable tail**. A consumer that falls behind receives `Lagged(n)`, re-syncs at the tail, and from there stays in receipt order. The surviving tail is contiguous — a skip costs a prefix, never a hole.
- **Why oldest:** the skipped records are the ones a reader has already stopped caring about.
- **Chunk records carry `aggregated`**, so a skipped intermediate delta costs no content: the next chunk carries the whole text so far.

## `facts` — latest-wins state

`FactRecord`: worker phase, permission-gate state, session usage, active model, active agent, active persona icon, context-window ceiling, MCP server usability, MCP visible tool count, MCP visible tool names.

- **Drop policy:** droppable, and nothing is lost by dropping. Every record is a **whole value for its field family**, never a delta, so a consumer that lags loses intermediate snapshots and never a state — folding the surviving suffix reaches the latest state for every field.
- **Facts carry state only:** no commands, no prompt content, and nothing a caller waits on.
- **The permission card is derived from the gate snapshot.** The resolver's wait therefore tolerates a dropped `Prompting` snapshot: a later snapshot replaces it, and the wait is bounded by its own timeout rather than by the channel.

---

## Choosing a channel

| The payload is… | Channel |
|---|---|
| Something the loop must **do** | `control` |
| A step in a sequence that must stay **complete** | `lifecycle` |
| Text a human reads, order-sensitive, tolerant of a skipped tail | `narration` |
| The **current value** of some state | `facts` |
| A record of what happened, folded by whoever needs it | `stream` |
| A stop signal that must arrive | `cancel` |

A payload that fits none of these is a missing channel, not a reason to overload one: the drop policy **is** the channel's identity.

---

## Retired carriers

`turn`, `session`, `external`, `compaction`, `ui_state`, and `ui_event` are **gone**. Their payloads are on the six concerns above:

- `ui_state`/`ui_event` carried runtime-crate vocabularies that could not cross the kernel edge. Their payloads landed on `control`, `facts`, `narration`, and `lifecycle`; the TUI-internal rows (picker options, transcript clear, startup logo, shared UI actions, transcript hydration) are surface-internal and left the bus entirely.
- `turn`, `session`, and `external` were kernel-typed already. Their publications are `LifecycleEvent` steps.

The retirement criterion is a guard, not history: no production or test source names `ui_state()`, `ui_event()`, `UiStateEvent`, `SystemEvent`, or `ExternalEvent`, and the grep that checks it returns nothing.
