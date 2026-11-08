# ADR 0004: The AI sets targets; a deterministic controller executes them

## Status
Accepted (2026-11-08)

## Context
The goal for 1.0 is an autopilot with guardrails. A language model takes
seconds to respond, can be interrupted, and is not deterministic. A roast needs
adjustments on a timescale of seconds, consistently, and safely when the AI is
gone.

## Decision
- A **controller** inside the daemon follows a target profile on every sample
  (feed-forward from the digital twin plus PI correction on rate-of-rise error)
  and issues commands with source `Autopilot`.
- The **AI** plans and adjusts the target profile, explains, and proposes
  critical steps. It never sits in the inner control loop.
- Critical steps (start, DROP, large changes) need a human confirmation.
- AI control is **leased**: without AI activity for the lease time, the
  autopilot stops and the human is alerted. While the AI or autopilot is in
  control, the human must confirm presence at regular intervals
  ([safety](../safety.md)).

## Consequences
- Roasts are reproducible and testable in the simulator with the same
  controller.
- The AI's job is planning and judgement, which suits it; the controller's job
  is tracking, which suits it.
- The controller and the twin need careful tuning per machine.
