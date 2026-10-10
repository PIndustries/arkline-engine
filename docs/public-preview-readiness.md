# Public developer preview readiness

Assessment: 2026-10-10, during development of 26.10.10.10–11. This is a release gate
inventory, not publication approval or a claim that the current checkout passed
all checks. Keep the repository private until the owner validates the model and
explicitly authorizes its publication and license. A public source preview and a
publicly reachable game server have different requirements; the latter is not
ready merely because the former builds.

## Evidence already present

| Area | Repository evidence | What it establishes |
|---|---|---|
| Source startup | README.md, Cargo.lock, rust-toolchain.toml | Documented server/client commands; Rust 1.89.0 and locked dependency resolution |
| CI definition | .github/workflows/ci.yml | macOS/Linux formatting, strict Clippy, tests and build configured; latest successful execution must be checked separately |
| Economic checks | apps/verifier, apps/server/tests/journey.rs, flight_tcp.rs, station_assets.rs | Independent replay and integration coverage, including persistence and authorization; individual test names/results need checking on the release revision |
| Target/drone checks | apps/server/tests/drone_targeting_acceptance.rs | Three independent acceptance tests passed during 26.10.10.9: five-lock limits, separate mining targets, drone motion/recall and invalid commands |
| Security boundary | SECURITY.md, server main.rs | Plaintext loopback default, bearer credentials, finite request sizes; not an Internet deployment design |
| Contribution rules | CONTRIBUTING.md | Atomic economy/ledger updates, independent verifier, protocol version decisions and regression expectations |
| Runtime evidence | docs/validation.md | Dated results and explicit unknowns; earlier build results do not certify later changes |

## Publication gates

All rows below need a recorded outcome against the exact release commit. Preserve
failed/partial results and their limitations instead of replacing them with a
blanket “verified.”

| Gate | Current status | Acceptance evidence needed |
|---|---|---|
| Owner acceptance | Pending | Owner completes the intended local journey and explicitly approves publication |
| License and rights | Unselected | Owner chooses a license; actual LICENSE text, package metadata and contribution terms agree; dependency and asset inventories reviewed |
| Clean source startup | Commands present; fresh-machine reproduction not established here | Clean macOS and Linux checkout builds with `--locked`, starts two identities, saves/resumes them, and audits the resulting ledger; document Linux window packages from CI |
| Release CI | Workflow present | Successful checks for the exact release commit, including new fleet features when merged; record URLs/revision |
| Upgrade and restore | Partial migration coverage | Copy an existing supported world with consistent SQLite backup, upgrade, verify cargo/escrow/storage/identities, restore original backup and open with original binary |
| Wire compatibility | Explicit decision still needed | Supported client/server matrix, mismatch rejection or tested additive compatibility; explain version policy before release |
| Contributor access | Private guidance present | Public reporting channel and working private vulnerability contact, supported platforms, PR checks and ownership of contributions documented |
| Asset/dependency notices | No completed inventory in this assessment | Exact resolved packages, license files/notices, distributed fonts/native components and asset provenance reviewed; required notices included in source/bundles |
| Gameplay acceptance | Prior dated results only | Current packaged build: two pilots, manual flight/roll/brake, all views, travel, five locks, separate miners, cargo/storage/trade, restart, audit and new fleet behavior |
| Rendering/performance | Partial dated macOS measurements | Current focused/fullscreen frame-time samples in station, dense belt, warp, active miners and drone feed; hardware/resolution/window focus recorded; visual checks on macOS and Linux |
| Release packaging | Local ad-hoc macOS packaging exists | Document unsigned/ad-hoc status, bundle contents and build instructions; verify launched package matches source revision; signing/notarization needed only if claiming that distribution status |
| Repository hygiene | Ignore rules exist | Review tracked files and history for credentials, private saves, proprietary imports and unrelated private material before changing visibility |

## License and asset inventory boundary

The current LICENSE explicitly reserves rights; it does not grant open-source
rights. Workspace packages also lack a selected license declaration. Do not call
the project open source until the owner resolves that gate. No legal compatibility
or third-party ownership conclusion follows from this document.

Known asset categories to inventory:

- `assets/Arkline-1024.png`, `Arkline-256.png`, `Arkline.icns`: icon generator
  `tools/render-icon.swift` identifies the orbital monogram as original. Rebuild
  and record provenance/license with the distributed icon files.
- Procedural hull, station, asteroid, cockpit, shader and visual-effect source:
  inventory source authorship and the actual source license; procedural generation
  does not itself settle ownership or contribution rights.
- Rust dependencies and their enabled/transitive native components, including the
  eframe UI/render stack and its bundled font data: inspect their license files
  and redistribution notices rather than assuming the root project license covers
  them. macOS system frameworks are platform prerequisites, not repository assets.
- Packaging outputs: identify all copied executables, icons and notices from
  `tools/package-macos.sh`; do not ship credentials or a private world.

An attempted `cargo metadata --locked --offline --format-version 1` on the
assessment machine could not resolve all-platform metadata because
`accesskit_atspi_common v0.10.1` was absent from the cache. Therefore a complete
transitive license inventory is **not** established. Resolve metadata in a clean
network-enabled dependency audit, record target/feature selections and the lockfile
revision, then inspect license expressions/files and any missing declarations.
A successful Cargo build for one target is not this inventory.

## Persistence and compatibility

Use a stopped server for a straightforward backup: stop the hosting app/server,
confirm the process exited, then copy the complete database and retain the matching
binary/version. For live backup use SQLite's supported backup mechanism; copying
only the main file while WAL transactions are active is not a consistent-backup
procedure. Keep pilot credential files separately and protect them as bearer
secrets. Audit checkpoints should also be retained: creating a new pin loses the
ability to compare with an earlier trusted head.

Document the backup/restore steps with actual filenames for both source and
packaged storage. Run a restore drill before release. Do not reopen a migrated
world with an older server; restore the pre-upgrade world with its matching binary.
`docs/migration.md` covers the spatial upgrade but is not a complete versioned save
compatibility matrix. Its introductory list of unported features is historical
and must be reconciled with subsequently implemented corporations and storage.

Protocol and ruleset constants currently remain `1` despite several additive
preview fields and requests. Defaults help old saved data deserialize; they do not
prove older clients can understand new enum variants or gameplay semantics. Before
publication, record a deliberate supported-version policy and test both sides of
each promised pairing. Fleets must be included once their schema is finalized.

## Trust and operational limits

The server owns movement, locks, module cycles, ownership and balances. Credentials
identify sessions; the client is not trusted to issue ore or credits. Independent
ledger replay checks economic consistency against declared rules and a pinned
history, not whether the operator honestly simulated extraction. Snapshots and
ledger exports expose economic information; private asset UI filtering does not
make the underlying economic history private.

Keep the developer preview on loopback or an explicitly trusted encrypted tunnel.
Direct Internet hosting needs a separate transport, abuse/connection limits,
account recovery/rotation and operational review. Node rewards, independent
consensus, trusted community validators and distributed authoritative simulation
are not established by the current single-server prototype.

Drone recall travels physically. In the new fleet implementation, control-range
loss forces autonomous return and rejects steering until signal recovers; warp,
docking, last disconnect and restart still clear deployed drones as prototype
policy. Mining drones issue real economic extraction events. Attack drones can
damage and destroy another pilot's drones without a corporation or team permission
requirement; this is a narrow prototype combat rule, not a complete PvP policy.
Five drones of each class (15 total) are supported. Each pilot controls only their
own fleet; general fleet command delegation and shared ship authority are absent.

## Validation record to attach to a preview

Record commit, toolchain, dependency lockfile, OS/hardware and package version.
Run formatting, strict workspace Clippy, workspace tests and locked build; preserve
CI/local result distinctions. Run an isolated TCP economic journey and independent
verifier, followed by a native packaged journey in a disposable world. Include a
restart/upgrade/restore drill and disconnected/timeout recovery. Never use a real
pilot save as a destructive test fixture.

For frame timing, record focus state, resolution, camera/view, visible workload,
mean and tail frame times. Prior focused macOS measurements of roughly 116–120 FPS
and a synthetic CPU preparation benchmark are scoped to their dated builds. They
do not establish Linux behavior, multi-pilot load, the current drone feed or a
stress-tested network. Screen-capture failures leave visual acceptance pending
when the remaining evidence cannot establish the appearance.

Current known gaps include load/distributed-failure testing, interrupted active
SQLite commit testing, full sleep/wake recovery, final-build drone-feed visual/FPS
sign-off (intermediate .10 onboard feed observed), orbit/weak-link visual checks and
cross-platform native render acceptance. See docs/validation.md for
historical evidence rather than treating this list as a completed checklist.

## Independent fleet acceptance during this assessment

`apps/server/tests/fleet_acceptance.rs` passed four integration tests against the
in-progress 26.10.10.10 backend: 15-drone/per-class caps and globally distinct IDs;
cross-pilot control rejection; approach followed by two mining cycles with exact
asteroid depletion and independent ledger replay; assigned foreign-drone damage
and destruction with an innocent drone untouched; signal decay, steering rejection
and physical autonomous return after range loss. Non-mining operations leave the
economic head unchanged. These are in-process fake-clock tests, not native visual
acceptance, network load testing or a release-commit CI result.
