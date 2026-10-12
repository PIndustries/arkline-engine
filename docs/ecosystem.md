# Arkline ecosystem milestones

Arkline should grow through complete, testable journeys. A panel is only a working
feature when its action reaches an authoritative service and its result survives
restart where appropriate. The public engine is a place to build, learn, test and
contribute together; a source release is not a claim that every planned feature
or official-network service is finished.

## Community projects and the official network

Our goal is one community building Arkline together: a shared engine and a
lasting official universe shaped by the people who play and contribute.
P.Industries focuses its development and operations on that official network.

For contributors, we recommend self-hosting as a place to test changes, experiment
with mechanics, reproduce bugs, and develop new features without affecting the
live universe. Bring useful discoveries back as issues and pull requests with
reproduction steps and relevant validation. Reviewed improvements can then become
part of an official release; running an experiment does not change the official
world automatically.

Independent community servers and forks are welcome too. You can explore a
different direction under the repository's license, and we are excited to see
what the community makes. This recommendation is not a hosting restriction or a
requirement to submit changes upstream. A separate world can still be part of the
broader Arkline community through shared knowledge, tools, and contributions.

Independent worlds do not have to use official account infrastructure. Official
network admission, authority and any future operator programme remain separate
from the ability to run the open-source code; operating a server does not currently
confer official status or rewards. P.Industries operates the official network;
independent operators are responsible for their own worlds.

Keep attribution and license obligations, and distinguish independent projects
from the official service under the [branding policy](../TRADEMARKS.md). See
[licensing](licensing.md) for the software boundary. The GitHub Wiki is the
published reader documentation; versioned documents remain in the source
repository so documentation changes can be reviewed alongside code.

## Current development priority: local Arkline Editor

The immediate focus is the integrated local editor: one place to run the real
simulation, edit ship definitions, inspect authoritative results and prepare
reviewable contributions. Follow the [community project](https://github.com/orgs/PIndustries/projects/14)
and [local editor milestone](https://github.com/PIndustries/arkline-engine/milestone/5).

The active integration work in [PR #47](https://github.com/PIndustries/arkline-engine/pull/47)
includes shared live fleet materials, local simulation controls, native contributor
sign-in and the first typed event graph. These are integration-branch capabilities,
not a claim that the public release or official services have been updated.

| Workstream | Current boundary | Acceptance still needed |
| --- | --- | --- |
| [Editor workflow #45](https://github.com/PIndustries/arkline-engine/issues/45) | Embedded client/authority, saved project documents and bounded background work | Complete native edit, apply, save, restart and recovery journeys |
| [Event flow #49](https://github.com/PIndustries/arkline-engine/issues/49) | Editable typed nodes, revision checks, breakpoints, causal traces and independent event/tick stepping | Native device round trips, deeper subsystem hooks, entity bindings and replay design |
| [Contributor identity #48](https://github.com/PIndustries/arkline-engine/issues/48) | Native GitHub device flow and direct API; public reading works signed out | Register app, verify approved login/credential lifecycle and enforce a wiki clone disk bound |
| [Rendering #36](https://github.com/PIndustries/arkline-engine/issues/36) | Shared materials, actual wireframe/draw counters and fewer redundant paused snapshots | Measured frame-time baseline and sustained native performance |
| [Ship fidelity #41](https://github.com/PIndustries/arkline-engine/issues/41) | Existing ship assets and shared material controls | Geometry/interior fidelity against the ship specification |

Graph documents and material edits can change without recompiling the editor;
new engine primitives still need reviewed Rust changes. The server remains the
authority for permissions, conservation and device interlocks. A graph trace is
not yet a recording of every internal subsystem or a replayable universe history.

The larger ecosystem journeys below remain future work. Local authoring and
stability take priority over expanding hosting, governance or reward systems.

## Current foundation

- Pilot: saved identity, station departure, timed travel, finite-resource mining,
  cargo, escrow listings, partial purchases and cancellation.
- Station: owned preview hulls with separate cargo, per-station personal storage,
  corporation invitations and shared ore storage with owner/member permissions.
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
production, node payouts or creator publishing
through decorative controls. Each milestone should become a usable part of this
shell once its complete journey passes validation.
