# Architecture

> **Tiny Agent Runtime (TAR)** — a portable, memory-safe AI agent runtime for the edge.
> One Rust core runs on a $5 microcontroller *or* a Linux camera SoC, talks to a cloud
> LLM for intelligence, and lets any IoT chip register itself as a tool the agent controls.

---

## 1. Goals & Constraints

| Constraint | Decision |
|---|---|
| **Where intelligence runs** | Cloud LLM over HTTPS (Anthropic Messages API; OpenAI-compatible fallback). The device runs the *agent*, not the model. |
| **Hardware tiers** | Two tiers, **one protocol**: a Linux edge SoC (the "brain", e.g. a security camera) and bare-metal MCUs (ESP32-class) acting as standalone agents or remote tool-nodes. |
| **Primary priority** | Lowest cost / tiny footprint. The MCU build stays lean; the linking protocol carries no broker tax. |
| **Language** | **Rust.** Memory safety on network-exposed, actuator-controlling devices; one language and one shared protocol crate across both tiers. |

### Why Rust

The runtime is internet-exposed (it calls a cloud LLM), parses untrusted JSON/TLS, and can
drive **physical actuators** (relays, locks). A memory-safety bug there is not a crash — it is
remote code execution on something physical. Rust removes that bug class. It also compiles to
both `no_std` MCU targets (`esp-rs`) and `std` Linux, so the protocol and agent logic are
written **once** and shared, instead of drifting between a C struct and a hand-parsed JSON blob.

---

## 2. System Overview

```
                    ┌─────────────────────────┐
                    │   Cloud LLM (HTTPS)     │   intelligence
                    └────────────▲────────────┘
                                 │ Messages API + tool_use
              ┌──────────────────┴───────────────────┐
              │      PORTABLE AGENT CORE (tar-core)    │   the "brain"
              │  agent loop · ReAct · context · memory │
              │  session · message bus · tool dispatch │
              └───┬───────────────┬────────────────┬──┘
        HAL traits (tar-hal) — the only thing that differs per target
          ┌───────┘               │                └────────┐
   ┌──────▼──────┐        ┌────────▼────────┐        ┌────────▼────────┐
   │ ESP32 (std- │        │ Linux edge SoC   │        │ Remote tool-node │
   │ alone agent)│        │ = camera "brain" │◀──TAR──▶│ (cheap MCU)      │
   │ GPIO = tools│        │ snapshot+vision  │  node   │ GPIO/sensors as  │
   │             │        │ via cloud        │  proto  │ remote tools     │
   └─────────────┘        └─────────────────┘         └──────────────────┘
```

**Three deployment shapes, one codebase:**

1. **Standalone MCU** — the agent loop and tools run on a single ESP32 (brain + body).
2. **Camera brain** — the agent runs on the camera's Linux SoC; perception is a JPEG
   snapshot pulled from the RTSP/V4L2 stream and sent to cloud vision **only on a trigger**
   (motion or schedule), keeping token cost bounded.
3. **Brain + tool-nodes** — one brain node commands a fleet of cheap MCUs whose GPIO and
   sensors are exposed as **remote tools** over one LAN protocol.

---

## 3. Workspace Layout

The runtime is a Cargo workspace. The brain is pure logic; platform crates are thin and are
the **only** place I/O lives.

```
crates/
  tar-core     no_std + alloc, ZERO I/O. Agent loop, ReAct, context builder,
               message bus, session — depends only on HAL traits.
  tar-hal      trait definitions: Storage, Clock, HttpClient, Transport,
               Actuator, Camera. The portability seam.
  tar-proto    serde node-protocol types + tool schemas. Shared by every node.
  tar-llm      Provider trait + Anthropic impl (OpenAI-compatible fallback),
               built on the HttpClient HAL trait.
  tar-tools    built-in tools, each behind a feature flag so an MCU compiles
               only what fits: gpio, camera_snapshot, fs, memory, web_search.
  tar-policy   capability + rate-limit + human-confirm engine.

platforms/
  tar-linux    std / Tokio HAL impls  → the camera "brain" binary
  tar-esp32    esp-idf-svc HAL impls  → standalone or brain on ESP32
  tar-node     minimal registry + transport + Actuator HAL → the $5 controller chip
```

**Dependency rule:** `tar-core` and the trait/proto crates never depend on a platform. I/O
flows *inward* only through HAL traits. This is what lets the identical agent sources run on
an MCU and a camera.

---

## 4. Core Concepts

### 4.1 The agent loop (ReAct)

The core runs a Reason-Act loop: build context → call the cloud LLM with the available tools →
if the model returns `tool_use`, execute the tools and feed results back → repeat until the
model ends its turn. The loop is bounded by a **token + iteration budget** with graceful
summarization rather than a hard truncation.

### 4.2 Tools are a trait — local and remote are identical

```
trait Tool {
    fn manifest(&self) -> ToolManifest;          // typed schema, generated from types
    async fn call(&self, input: ToolInput) -> ToolResult;
}
```

A local GPIO tool and a remote tool-node implement the **same** trait. "Remote" is just a
`Transport` implementation that forwards the call over the LAN. **Adding an IoT chip as a
controller** therefore means: implement one trait and advertise a capability — nothing in the
agent loop changes.

### 4.3 Capability manifests

A tool-node advertises its tools when it connects. The brain builds the LLM `tools` array
**dynamically** from the live manifest set, instead of hardcoding tools at build time. New
hardware appears as new tools with no brain rebuild.

### 4.4 The node protocol (`tar-proto`)

Given the lowest-cost priority, the linking protocol is **line-delimited JSON-RPC over plain
TCP/WebSocket on the LAN** — no broker, no gRPC tax. The message types are defined once with
`serde` and reused by brain, tool-node, and any future cloud component:

```
{ "type": "hello",       "node_id": "...", "capabilities": [ ... ] }
{ "type": "tool_call",   "id": "...", "tool": "gpio_write", "input": { ... }, "cap": "<token>" }
{ "type": "tool_result", "id": "...", "ok": true, "content": "..." }
```

A remote tool-node is, in effect, a `tar-tools` registry whose `Transport` is the socket.

---

## 5. Security & Safety

Edge devices that face the internet and drive physical actuators cannot treat security as a
later phase. The minimum bar before any real deployment:

- **Actuator policy layer** — every actuator tool call passes a policy engine: allowlist +
  rate limit + optional human-confirm before a physical state change. A prompt-injected model
  cannot freely toggle a relay or a lock.
- **Capability tokens** — tool calls carry a capability; a node rejects calls it was not
  granted. Tool-nodes authenticate with a per-node pre-shared key. LAN-only by default; the
  cloud LLM is the only outbound connection.
- **Secrets off the binary** — credentials are provisioned to a keystore (NVS on MCU,
  filesystem keystore on Linux) at setup, never compiled into firmware.
- **Memory safety** — the network/parser/actuator path is Rust, removing the buffer-overflow
  class outright.

---

## 6. Perception (camera tier)

Vision runs in the cloud, so the cost lever is **how often we send a frame**. The camera brain
runs a cheap on-device pre-filter (motion / threshold) and spends cloud-vision tokens only on a
real trigger. Snapshot cadence is a first-class config knob, not an afterthought. Continuous
understanding is possible but explicitly opt-in because it dominates running cost.

---

## 7. Resilience

Cloud-dependent edge devices lose connectivity. The brain persists inbound events and pending
tool results to flash and replays them on reconnect (store-and-forward), so a camera does not
silently drop a "person detected" event during a Wi-Fi blip. Firmware updates use A/B OTA slots
so a bad update can roll back.

---

## 8. Memory

Long-term memory is human-readable Markdown behind a typed `Storage` trait (flash files on MCU,
filesystem on Linux), so it is OTA-friendly and inspectable. The agent can write its own memory
through a `memory` tool, and the loop flushes durable memories before the context budget is
exhausted.

---

## 9. Status

This document describes the target architecture. Implementation proceeds in phases — see
[ROADMAP](docs/ROADMAP.md). Each phase ships a verifiable milestone before the next begins.
