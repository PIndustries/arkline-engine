# Arkline ecosystem milestones

Arkline should grow through complete, testable journeys. A panel is only a working
feature when its action reaches an authoritative service and its result survives
restart where appropriate. Keep the repository private until owner validation and
explicit approval to publish.

## Current foundation

- Pilot: saved identity, station departure, timed travel, finite-resource mining,
  cargo, escrow listings, partial purchases and cancellation.
- Cockpit: alternate tactical and perspective views of the same server state.
- Operator: one authoritative Rust process and a durable SQLite world.
- Observer: independent economic replay and locally retained history checkpoints.
- Contributor: Rust workspace, repeatable checks and a real TCP restart journey.

## Next complete journeys

| Milestone | Deliverable | Acceptance evidence |
| --- | --- | --- |
| Reliable commands | Persisted command IDs and receipts; reconnect can resolve an uncertain purchase without buying twice | Drop a response after commit, reconnect, retrieve the original receipt, verify one debit |
| Production loop | Versioned recipes, input escrow, timed jobs and explicit output events | Mine, manufacture, restart mid-job, collect once and audit all resource transformations |
| Creator workflow | Documented content format, validator, local preview and operator approval | A community-authored asset enters a test region through validation and retains attribution |
| Community witnesses | Operator identity, signed checkpoints and authenticated witness comparison | Detect a rollback and conflicting signed histories across independently operated observers |
| Regional hosting | Admitted regional operators, durable authority transfer and failure recovery | Transfer a pilot between two real regions without duplicated inventory or authority |
| Operator programme | Measured service, admission rules, dispute process and explicit reward policy | Independently reproduce service measurements and review disputed credit before any payout |

Production jobs need new economic rules and independent verifier support in the
same change. Content tools need asset provenance, compatibility and resource limits.
Regional hosting needs failure testing before opening admission. Reward rules must
not treat self-reported uptime as proof of useful service.

## Interface direction

Use a persistent cockpit shell: pilot and ship status, selected-object details,
region overview, space viewport and station services. Cargo, escrow, credits and
history must describe the same authoritative snapshot. Distinguish saved pilots
from live connections, and make stale state visible after a disconnect.

Show controls only when their underlying behavior exists. Do not imply combat,
free-flight physics, corporations, production, node payouts or creator publishing
through decorative controls. Each milestone should become a usable part of this
shell once its complete journey passes validation.
