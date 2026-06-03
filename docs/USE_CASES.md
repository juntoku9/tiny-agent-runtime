# Use Cases

What you can build on the Tiny Agent Runtime. Every case below maps to the
pieces that already exist: a cloud LLM **brain**, cheap MCU **tool-nodes**
(GPIO / sensors / actuators), **cost-aware vision** (snapshot on trigger, not
streaming), a **policy gate** on physical actions, plain-Markdown **memory**,
and a **dashboard** to watch it all.

The pattern is always the same:

```
sensors → the agent perceives → it reasons (cloud) → it acts through tools → policy gates the act
```

What makes these different from a normal "smart home" rule is that the agent
**reasons in language** about messy, open-ended situations instead of firing
fixed if-this-then-that rules — and it can explain *why* it did something.

---

## Home & everyday

### 1. The camera that *explains*, not just detects
A doorbell/porch camera that says **"a delivery courier just left a package by
the door"** instead of "motion detected." On a real event it can open a parcel
locker (a relay tool-node) — but only after the policy gate confirms, so a
spoofed frame can't pop your lock.
- **Nodes/tools:** `camera_snapshot` (vision), `gpio_write` (locker relay)
- **Why TAR:** language-level scene understanding + a hard safety gate on the
  physical act. Vision only fires on motion, so it's cheap.

### 2. Plant / greenhouse keeper
A soil-moisture sensor node and a pump relay. The agent checks the forecast
(`web_search`), looks at the plant on camera, and waters **only when the soil is
dry and rain isn't coming** — then writes "watered the basil, soil was 18%" to
its daily Markdown notes.
- **Why TAR:** combines a live sensor, outside knowledge, and vision into one
  judgement no fixed timer can make.

### 3. "Talk to your house"
Message the runtime in plain language — *"make the garage a bit warmer and tell
me if I left the workshop light on."* The agent routes each intent to the right
tool-node (heater relay, light sensor, camera) and reports back.
- **Why TAR:** one brain, many cheap nodes, **one protocol** — new hardware
  shows up as new tools with zero rebuild.

### 4. Pet feeder that knows your pets
A camera identifies *which* animal is at the bowl and a servo dispenses the
right portion — **rate-limited by policy** so a confused model can never
overfeed. Ask it over chat: "did the cat actually eat today?"
- **Why TAR:** vision-conditioned actuation + abuse-proof rate limiting.

---

## Safety & security

### 5. Workshop / kitchen safety guardian
A camera plus a smoke/gas sensor node and a buzzer + power-cutoff relay. If the
agent **sees flames and reads rising smoke**, it sounds the alarm and cuts power
to the bench — fire-safety actions are pre-authorized in policy, everything else
needs a human.
- **Why TAR:** multi-modal corroboration (vision *and* sensor) before a
  high-stakes action, with per-action policy.

### 6. Elderly-care wellness watcher
Learns a person's normal rhythm into memory ("usually active in the kitchen by
8am"). If the pattern breaks — **no movement since 9am** — it sends a gentle
check-in to family. Privacy-first: it reasons over snapshots and presence, not a
streamed feed.
- **Why TAR:** durable Markdown memory of routines + snapshot-only vision keeps
  it private and cheap.

### 7. Smart-lock concierge
"The plumber's coming at 2pm — let them in." The agent schedules a one-time
access window (cron), verifies the visitor on camera at the door, opens the lock
once under policy, and logs the entry to memory.
- **Why TAR:** scheduling + vision verification + a logged, gated actuation.

### 8. Server-room / site sentry
Door sensor + temperature node + camera. **"Door opened at 2am and the person
isn't recognized"** → alert + auto-lock the rack. Correlates events the way a
guard would, not as isolated alarms.
- **Why TAR:** the agent fuses several weak signals into one judgement.

---

## Industrial & fleet

### 9. Predictive-maintenance fleet
Vibration/temperature sensor nodes bolted to machines; **one brain** baselines
"normal," flags drift, reasons about the likely cause ("bearing wear signature
on conveyor 3"), and opens a ticket — escalating only what matters.
- **Why TAR:** one agent orchestrating **many heterogeneous nodes** over the LAN
  protocol; adding a machine = adding a node.

### 10. Retail shelf / inventory eye
Scheduled camera snapshots; vision estimates stock and raises **"milk shelf is
nearly empty"** — no video streaming, just a frame every few minutes.
- **Why TAR:** cost-aware perception turns an expensive vision problem into a
  few cents of tokens a day.

### 11. Air-quality + smart ventilation
A CO₂ sensor node and a fan relay. The agent ventilates when CO₂ climbs,
cross-checks occupancy on camera ("nobody's home — don't bother"), and explains
each decision.
- **Why TAR:** sensor + context reasoning beats a dumb threshold.

---

## Novel / delightful

### 12. Backyard naturalist
A battery PIR-triggered trail cam. On each wake it snapshots, vision IDs the
species, and logs sightings: **"barred owl, 3:14am, by the feeder."** Builds a
little wildlife journal in Markdown.
- **Why TAR:** wake-on-motion + snapshot vision = months on a battery.

### 13. Aquarium / reef autopilot
Temp and pH sensor nodes with heater/pump/dosing relays. The agent holds
parameters in range, **explains** adjustments in plain language, and alerts on
drift — with a hard policy cap on the heater so it can never cook the tank.
- **Why TAR:** continuous control with an explainable narrator and a safety
  ceiling.

### 14. Gate / parking attendant
A camera reads a plate; the gate relay opens **only for allowlisted plates**
(policy), and every entry is logged. Tell it in chat: "add a guest plate for
tonight."
- **Why TAR:** vision + allowlist policy + natural-language administration.

---

## The throughline

Three properties make these practical rather than science-fiction:

1. **Cheap bodies, smart brain.** The intelligence is in the cloud; the things
   on the wall are \$5 chips. You can blanket a space in sensors without a
   GPU on every one.
2. **Cost-aware senses.** Vision fires on a trigger, not a stream — so "an AI
   watching your cameras" costs cents, not dollars, per day.
3. **It can't run amok.** Every physical action goes through a policy gate
   (allowlist + rate limit + human-confirm), so a hallucination or a prompt
   injection can't throw a relay it shouldn't.

If you can wire a sensor and a relay to a cheap chip, you can give it a brain.
