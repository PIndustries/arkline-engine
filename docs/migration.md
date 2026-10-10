# Relationship to earlier Arkline work

This is a new Rust workspace, preserving the earlier repositories unchanged.
The source review used local checkouts; it did not establish current deployments.

| Earlier project | Design carried forward |
|---|---|
| Elixir arkline-core | Pilot journey, command boundary, persistent ships, mining, escrow market, audit events |
| arkline/arkline_app | Event-oriented universe observation and generation concepts |
| Celestial/resource/sector Rust generators | Future source material for procedural generation |
| Apple ArklineEngine | Navigation, warp, fitting, damage and cockpit reference |

The first acceptance scenario comes from the Elixir game's pilot-loop testing:
undock, travel, mine, dock, sell, recover. This preview covers that economic journey;
combat, insurance, manufacturing, scripting, corporations and editor tools are not
ported yet. No legacy assets, account data, secrets, or deployment credentials were
copied into this repository.

Important deliberate changes:

- Economic changes and audit events commit atomically; the inspected legacy market
  fill performed several separate updates before publishing its audit event.
- The independent verifier is a separate executable with its own replay logic.
- Issuance is explicit rather than implied by ambient NPC orders.
- One Cargo workspace keeps the local development journey accessible. Each logical
  component does not need its own network service or repository.
- The older code's gameplay federation means corporate alliances. It is not reused
  as evidence of community-operated infrastructure.

The existing Rust procedural generators used seeded generation alongside random
UUIDs. They require stable identities and reproducibility tests before serving as
verifiable universe generation. They have not been copied into this release.

## Spatial migration in build 26.10.9.5

World spatial version 2 moves the belt to 2 AU and gates to multi-AU coordinates.
Existing ships retain their offset from the nearest old station, belt or gate;
asteroid IDs and remaining ore are retained. Manual navigation stops during the
one-time migration and an active old warp is replanned from its migrated position.
Cargo, credits, orders and ledger events are unchanged. Make a SQLite-consistent
backup before upgrading. Older clients/servers must not reopen an upgraded world;
restore the pre-upgrade backup to roll back.
