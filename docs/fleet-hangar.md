# Personal fleet hangar foundation

This change provides real personal vessel berths and server-authorized walking-to-board. It does not create corporation-owned ships, ship purchases, shared public station avatars, or a physical exterior docking transition.

## Implemented contract

A client opts in through `Connect.supports_fleet_hangar`; missing means false. Its private `AssetsView.hangar` contains only the account's vessels at the current station, their persistent opaque IDs, actual class dimensions through the shared class specifications, berth transforms, boarding markers and active state. Vessels elsewhere are absent. This view is not a public inventory of other pilots.

Existing preview accounts still own one vessel per class. IDs are persisted once in the account and validated against the immutable account/class identity (`v1:<pilot>:<class>`). They remain stable when boarding, travelling or restarting. This intentionally retains the existing catalog limitation; a future multiple-vessels-of-one-class migration must introduce independent allocated identities without changing existing IDs.

The fixed catalog berth layout does not move when a ship leaves or the active class changes. Ships have their true dimensions. A human-scale concourse runs beside them; the server restricts walking to that lane and its furnished quarters. Large vessels therefore occupy genuinely large berths; convenient travel between distant berths is not yet implemented. These are station-local coordinates, not an exterior station geometry fit or seamless airlock model.

`BoardShip { id }` requires an authenticated pilot, enabled fleet hangar, a docked owned vessel at that station, an avatar on foot outside the interior, grounded and standing, within the server's boarding range of the requested berth. It uses the existing transactional `ShipBoarded` cargo/power exchange, then clears the avatar/walking input. Another pilot's ID, a forged ID, a distant marker, or a repeated stale boarding request fails. Membership and remote-control delegation confer no boarding permission. The remote gameplay whitelist excludes this command.

Legacy `SelectShip` works only in legacy private-hangar mode while aboard and docked. It remains forbidden while walking. `EnterShip` boards the already-active ship at its own berth, while the new command selects a different vessel by identity.

## Compatibility and persistence

An authenticated older client recovers a fleet-hangar avatar to the existing private hangar boarding point. Incorrect credentials cannot relocate it or affect capability state. If any incompatible session has connected, fleet mode stays unavailable until all sessions for that pilot disconnect; reconnecting one newer session cannot strand the older session in coordinates it cannot render. The latch is process-local because a server restart ends all old transport sessions. New-only reconnects preserve the fleet scene; existing interior compatibility behavior remains separate.

Back up the world before upgrading. Older server binaries do not understand the fleet coordinate frame; rolling back requires evacuating fleet walkers through a compatible server first or restoring a consistent pre-upgrade world/ledger backup. Do not restore only one half of the economic state. Additional JSON fields default safely when loading older saves into this version; that does not prove old binaries safely read newer saves.

## Corporation custody follow-up — not implemented

Corporations currently hold ore storage only. Members' personal vessels must not be presented as corporation property. Real corporation berths require:

- Explicit vessel transfer into corporation ownership, with an auditable owner transition.
- Owner-managed boarding grants, distinct from membership, storage withdrawal rights and remote pilot delegation.
- Exactly one active custodian per vessel, atomic boarding/return, and defined revocation while occupied. A disconnect must not duplicate or silently reassign a vessel.
- A vessel cargo/power record independent of the pilot, with verifier replay counting stored and occupied cargo exactly once.
- Stable station/berth placement and migration of the existing one-of-each-class ownership invariant.

Required tests include forbidden donation of another pilot's ship, unauthorized boarding, membership without a grant, revoked access, simultaneous claims, ship and cargo persistence after disconnect/restart, and conservation across donation/boarding/return. This foundation deliberately supplies no fabricated corporation fleet.

## Validation

`cargo test -p arkline-server --test fleet_hangar` exercises actual timed walking, range and identity rejection, boarding, economic replay, restart, and mixed-client recovery. Existing station/interior, storage/power and mining suites remain regression gates. Native/browser scene acceptance is a separate client integration gate; server tests alone do not establish visual usability.
