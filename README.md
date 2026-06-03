# Tiny Agent Runtime (TAR)

**A portable, memory-safe AI agent runtime for the edge.**

One Rust core runs on a **$5 microcontroller** *or* a **Linux camera SoC**. It talks to a
cloud LLM for intelligence, and lets **any IoT chip register itself as a tool the agent
controls** — a relay, a sensor, a camera, a lock. The IoT chip is the body; the cloud model is
the intelligence; the runtime is the nervous system that connects them safely.

```
   Cloud LLM  ◀──HTTPS──  Agent core (Rust)  ──LAN──▶  IoT tool-nodes
 (intelligence)            (runs on MCU or                (relays, sensors,
                            Linux camera SoC)              GPIO, cameras)
```

## What makes it different

- **One core, two tiers, one protocol.** The same agent logic compiles to bare-metal ESP32
  (`no_std`) and to a Linux edge SoC (`std`). The wire protocol is defined once and shared.
- **IoT chips as first-class tools.** Local and remote tools implement the same trait. Adding
  hardware means implementing one trait and advertising a capability — no change to the agent.
- **Cloud intelligence, edge cost discipline.** Vision runs in the cloud, triggered by a cheap
  on-device pre-filter, so token cost stays bounded.
- **Safe by construction.** Memory-safe Rust on the network and actuator paths, plus a policy
  engine that rate-limits and gates physical actions — a prompt-injected model can't fire a
  relay at will.

## Target deployments

- **Security cameras** — the agent runs on the camera's Linux SoC, sends snapshots to cloud
  vision on motion, and raises grounded alerts ("person at the door").
- **IoT sensor/actuator fleets** — a brain node orchestrates many cheap MCUs whose GPIO and
  sensors are exposed as remote tools.
- **Standalone MCU assistant** — a single ESP32 acting as a self-contained agent.

## Status

Early development. The architecture is defined; implementation proceeds in verifiable phases.

- [Architecture](ARCHITECTURE.md) — system design, workspace layout, protocol, security model
- [Roadmap](docs/ROADMAP.md) — phased plan with per-phase success criteria
- [CLAUDE.md](CLAUDE.md) — engineering guidelines for contributors and AI assistants

## License

TBD.
