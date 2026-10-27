# Continuous docking checkpoint

Local implementation based on `c358f7b49d321a786a9f6a14bea866a600549f22`.
Scope claim: <https://github.com/PIndustries/arkline-engine/issues/62#issuecomment-6380550924>.
This is a fresh-world, Pioneer-only acceptance checkpoint, not a finished flagship or a production rollout.

## Try it without converting a world

In the editor Session panel, choose **Continuous docking preview**, then Local or Host.
This creates `realms/docking-preview-v1` under the project directory. Project and orbital preview worlds retain their saved revision and positions. Joining uses the host's world and does not initialize a local authority. DockingV1 requires world-layout capability 2 and the existing Pioneer interior/traversal capabilities; authentication and remote editor permissions are unchanged.

The preview has eight permanent, vessel-owned exterior berths. They are open station aprons, not entrances to the existing private fleet hangar. Departure and return use the same world-space ship pose and the same flight renderer. The seated view is not replaced with a hangar scene at capture.

## Implemented contract

- `crates/arkline-protocol/src/docking.rs`: persistent vessel/berth/operation identity; Hermite trajectory; shared platform geometry; phase and clamp state.
- `apps/server/src/docking.rs`: the authority accepts and checks a route, advances approach, alignment, berth translation and clamps, and grants docked access only after capture. Departure releases clamps before backing to the gate. No command instantly relocates an existing ship.
- An approach starts on the space-facing side, within 6 km of its assigned gate and below 10 m/s, with drones recovered. Approach is at least 6 seconds, alignment 3 seconds, berth translation 15 seconds and clamp capture 2 seconds. Departure is 2 seconds of release and 15 seconds of translation. Longer approach distances extend the approach time.
- The server checks a conservative hull envelope, fixed platform solids, other confirmed ships and their reserved remaining routes. Free-flight movement is stopped before entering the platform envelope. This is bounded kinematic collision avoidance, not general rigid-body collision physics.
- Cancellation identifies the current operation and brakes continuously to a held state. Duplicate cancellation is harmless. Resume must identify the same held operation. Disconnect and restart preserve the last confirmed position and operation and require explicit resume; they do not complete a journey offline.
- `apps/server/src/resource_jobs.rs` waits for confirmed departure/capture rather than assuming one-tick docking. Cancelling the routine brakes an active docking operation.
- `apps/client/src/motion.rs` and `flight3d.rs` sample only the confirmed segment, capped at 150 ms beyond its last update. The client never advances a phase, enables access or replays delayed movement at the free-flight handoff.
- Registered onboard traversal keeps its rigid ship-local frame. Existing hatch/lift interlocks remain required. Legacy instant station-entry requests are rejected in this preview.
- Docking has no economic ledger effect. Deposit/withdraw and corporate transfers are still existing instantaneous SQLite transactions; there is no animated fake cargo count.

## Acceptance evidence

`apps/server/tests/seamless_docking.rs` exercises real commands, fixed ticks and disposable saved worlds. The nine-test run in `docking-acceptance-final.log` passed: continuous round trip, no early storage access, cancellation and retry, unclean mid-flight restart, unique owned berths/peer observations/version guard, platform collision stop, occupied-route hold/blocked-resume/safe-clearance recovery, full berth-capacity rollback, old-world preservation, and a complete real owned mining/deposit routine that waits for both docking transitions. The consolidated protocol/server command (`cargo test -p arkline-protocol -p arkline-server --locked`) passed **352 tests, 0 failed, 7 intentionally ignored** across 46 suites. The editor check passed. The final full client binary run reports **537 passed, 11 known baseline failures, 39 ignored**; the additional version-literal transport fixture mismatch was corrected. Server/protocol Clippy completed successfully with four pre-existing warnings outside the new docking code; the two new cosmetic docking lints were corrected. These results do not claim green workspace CI or completed walking acceptance.

The opt-in `native_authoritative_docking_motion_capture` test uses real `EditorWorld` snapshots and production `Renderer::paint` with Mesa EGL. It saves 10 fps exterior/cockpit PNG sequences plus simulation time, phase, operation, ship pose and camera receipts. The seated sequence uses the complete real Cockpit UI and GPU callbacks, not blank instrument surfaces. Its before sequence runs the retained unchanged OrbitalV1 instant command path in the modified binary and is labeled accordingly; it is not represented as an exact baseline binary. Smoke runs are explicitly incomplete.

The completed capture at runtime source `30618f0d204d6db817d972fd4dbc83fc134ac6bd` passed its native test. All **984 PNGs** decoded and were hash-verified: 461 seated and 461 exterior after frames, plus 31 per camera for the retained legacy path. Every after seated frame stayed on the world-space route with no services popup. At 100 ms cadence, maximum displacement was **2.49985 m** after versus a **1,021.147 m** one-step legacy jump. Request boundaries preserve position; the final berth position equals the initial position. Actual manual input turns the ship 45 degrees before its return, so the alignment phase is exercised. The fixed exterior camera's largest view-matrix roundoff is 2.16e-7. The later commits add tests, wording and equivalent lint cleanup only; they do not change captured runtime behavior.

The private evidence directory is `docking-motion-20261024`, with `receipt.json` and `validation.json`. The sequence represents simulation-time sampling rendered offscreen with Mesa EGL, not real-time frame-rate evidence. Start, departure, alignment, return and terminal frames were inspected. The distant exterior finish matches the existing baseline; it is not a finished-art claim.

## Remaining flagship punch list

1. **Visual and interaction acceptance:** inspect uninterrupted approach, attitude alignment, clamp motion and departure from seated, walking and external views; verify useful instrument access and stale-network behavior. Add latency/packet-loss and multi-client end-to-end tests. The existing bottom workspace strip still says “Holding” for `NavigationMode::Stop` during docking; the center console already shows the actual docking phase and the integration owner is correcting that remaining label.
2. **Shared station integration:** replace the bounded external apron with an authored public station route, a shared spatial frame, traffic arbitration, collision geometry and ownership/access policy. Current private traversal coordinates are rigidly mapped to an assigned apron; that is not a complete shared station interior. The new knee rail currently extends 0.2 m inside the old avatar-clearance extent; the integration owner must add a DockingV1-only capsule clearance test/fix before walking acceptance. Initial seat exit during manual motion remains rejected by the existing traversal contract; no permission was loosened. Support additional vessel classes and safe berth release/reassignment before general use.
3. **Services and airlock:** implement physical service connection, pressure/equalization and access interlocks as real authority states. Clamp completion currently grants the existing docked services; it does not pretend a pressure simulation exists.
4. **Timed unloading:** introduce a durable transfer operation with source/destination ownership, reserved amount, progress and committed batch receipts. Inventory movement and its receipt/ledger event must share one SQLite transaction. Retry IDs must return the same committed result; cancellation may release only uncommitted reservations. Reconnect/restart cannot duplicate inventory, credit or job progress. Add crash-boundary, duplicate request, concurrent ownership/capacity and ledger verification tests before animating a real loader.
5. **Collision and recovery expansion:** test crossing approaches, blocked routes, all cancellation phases, full berth capacity, long-lived multiplayer reconnects and near-boundary manual rotation. Replace conservative whole station half-space exclusion with complete authored station geometry before unrestricted exploration.
6. **Normal experience:** integrate the accepted path into new normal flagship worlds after review; keep explicit legacy compatibility/migration policy. Do not leave the feature permanently as a detached demonstration, and do not silently migrate existing worlds.
7. **Release acceptance:** integrate the independently owned cabin polish and baseline test fixes, run complete client/server/protocol and lint checks, inspect final actual-engine motion media, then obtain separate runtime publication approval. No runtime publication, push or merge is part of this checkpoint.

## Local cabin integration and walking acceptance

The accepted docking work is integrated locally with the lower cassette HP glance
band, neutral cabin palette and passive inset working-wing covers. The canonical
Blender/GLB source, nine moving screens, captain controls, reactor/cargo display,
pilot eye/FOV and registration 11 are preserved. Source publication remains separate.

The DockingV1 apron rail projects 0.2 m inside the original private walking extent.
The integrated authority applies that inward allowance to capsule clearance only
in this preview realm, including actual walk integration and occupied-layout checks.
Legacy realms retain their previous bounds. No stored feet are relocated: a former
preview save occupying that newly guarded strip is incompatible and must be preserved
and opened with its matching earlier build to move the pilot safely inward before
retrying. This is not a silent world migration.

Manual docking does not grant permission to leave the seat during motion. Walking
at a captured berth uses the existing registered v2 path; routine-owned in-flight
traversal retains its existing conditions. The bottom status now reports the confirmed
docking phase instead of the underlying navigation Stop mode. It does not advance
phases or bypass stale-network command guards.

New tests cover rail inner-face contact/legacy bounds, actual walk-integrator stop,
real captured-seat/walk/reseat and docking return commands, incompatible saved-edge
preservation, and client rigid-body/FOV sampling. The optional integrated native
proof records eight actual-authority state views; this supplements, not replaces,
the earlier continuous-motion sequence. Terminal results are recorded with the
checkpoint receipt. Neither proof claims full station traversal or pressure simulation.
