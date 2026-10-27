# Pioneer recovery and fitted cockpit checkpoint — 20 October 2026

Recovery began in the existing `/Users/thepherm/Documents/Codex/arkline-editor-reconstructed-20261020` repository at source `5d1a399`. No repository was recreated. No AGENTS.md was found in the repository or its inspected parent chain. The previous root and six observed workers were idle/completed; process inspection found no surviving Cargo or guarded-build worker. Free space was 138.47 GiB, above the guarded 50 GiB start and 30 GiB stop thresholds.

## References actually checked

- Correct project wiki: https://github.com/PIndustries/arkline-engine/wiki/Ship-Pioneer
- Interior scope: https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Interior-Preview
- Retained detailed audit: `docs/ships/pioneer/pioneer-flagship-reference-assessment-20261020.md`
- Original local v05 image visually inspected: `docs/ships/pioneer/images/v05-interior-review.jpg`

The wiki still identifies v05 as visual direction and distinguishes concept reservations from fitted/implemented geometry. Visually, its corridor calls for light wall panels, dark grip flooring, metallic framing, a service ceiling and restrained trim. Its bridge suggests rear-chair circulation; those illustrations do not author measured cockpit dimensions. The retained audit measures the former front pocket at 1.505 m depth. The actual upper boarding vestibule/lift differs from the proposed lower AIRLOCK/EVA room; current room identities and layouts have not been relabelled or moved.

## Preserved and corrected source

- `0f7f6d7`: explicitly unvalidated inherited WIP checkpoint, including whole workstation shift −1 m in body X, rear bay X31.6, typed surface material transport, on-foot Shift sprint, confirmed-displacement gait and existing guidance.
- `827fd13`: follow-up WIP checkpoint with input routing corrections, inspection material parity, fitted authoritative route waypoints, a complete rear-chair loop regression, geometry registration revision 2, previous-registration saved-world preservation regression, and central display mount raised 5 cm to Y0.82.
- Verified complete source bundles: `/tmp/pioneer-recovery-inherited-20261020.bundle` and `/tmp/pioneer-fit-display-wip-20261020.bundle`.
- Original inherited patch and untracked material file also retained in `/tmp/pioneer-recovery-inherited-20261020.patch` and `/tmp/pioneer-recovery-surface_roles-20261020.rs`.

The input regression exposed the test helper forcing legacy avatars into the registered seated path. The helper now uses actual navigation admission; the registered view additionally requires no station avatar for seated Shift-look. All original input/focus and movement assertions remain. The chair translation required route targets to follow the new chair rather than run through its console. No capsule, stair, movement-distance or seat-return assertions were weakened.

Registration 2 prevents changed geometry being represented as registration 1. An old bound saved world fails admission without rewriting its durable world JSON. No old user or QA world was migrated. Such worlds require their matching earlier build until explicit migration is designed and validated. Existing v2 boolean admission is unchanged; client exact capability and bound-command revision checks remain.

## Validation and actual application state

All operations used the existing single guarded target, offline dependencies and retained disposable QA data.

- Library suites: 383 client + 54 protocol + 71 server = 508 passed, 34 explicitly ignored client tests. Log: `.build/logs/20261020T143555385207Z.log`.
- Rear circulation walks around both sides and behind the folded chair, checks capsule/floor collision every 25 mm, and returns to the same seat root in private and circular fleet poses.
- Old-save regression uses real SQLite close/reopen and confirms durable JSON is identical after rejection.
- Initial fitted native GPU route matrix passed in 90.09 s, evidence `/tmp/pioneer-recovery-fit-route-20261020`. This first render retained the known lower-screen-corner clipping and is NOT screen-fit acceptance.
- Screen correction adds actual projectability and a 2% viewport margin for all four inset interactive corners; the final screen-fit route run is recorded separately below.
- `cargo fmt --all` and `git diff --check` completed before `827fd13`.

Packaged `.8` launch is now verified: PID19652, actual window title `Arkline Editor — 26.10.20.8`, disposable project `/tmp/arkline-canonical8-native-osqa-20261020`, pilot `Continuity8QA`. Its saved app-swap receipt and desktop shortcut captures exist under `.build/packages/pioneer-continuity-20261020/native-matrix-os`. A live attempted Cockpit shortcut correctly refused while boarding hatches/lift were not stowed; a Deck1 shortcut confirmed registered body Y−4.12. Those observations do NOT establish a full actual-desktop .8 journey. Native screenshot capture through the current UI tool yielded a small skewed thumbnail, so it was not treated as useful visual acceptance. Saved full-size desktop evidence and new offscreen native GPU captures were inspected locally.

## Still open

This is a recovery/fit milestone, not a finished flagship or release. The full .8 desktop route is incomplete. No new package was installed or launched for registration2. Surface response transport is implemented, but visual inspection still shows a blockout-like corridor with blank slabs and saturated frames: panel construction, grip finish, service ceiling and purposeful lighting/equipment remain substantial work. Existing hatches still disappear instantly; the measured split-leaf sweep proposal is retained in `/tmp/pioneer-hatch-geometry-contract-20261020.md` and its JSON/audit script, but animated authority/render/collision has not been implemented. Broader room purpose and exterior v05 completion remain open. No whole-Documents permission was requested, and no source/screenshots were uploaded, pushed, published or moved.

## Final native screen-fit result

The strict native GPU route matrix on `827fd13` passed in 90.73 seconds. Log: `.build/logs/20261020T144314651496Z.log`. Full retained captures, per-view projection JSON and authority fixture receipt: `/tmp/pioneer-recovery-screen-fit-route-20261020/`. Both docked and undocked/returned central screen corners report `[false,false,false,false]` for clipping. Lower raw normalized Y is approximately0.93933 versus1.00346 before the physical mount correction. Docked and space screenshots were visually inspected. This is offscreen native macOS CGL acceptance, not an actual OS-window journey or a packaged release. The driver printed its existing GLD_TEXTURE_INDEX_2D warning; render/depth/input/authority assertions completed successfully.

No action in this recovery was denied or left with uncertain execution. Broader flagship work remains pending as listed above. The packaged `.8` process remains the old matching build; new source registration2 has not been substituted into that running session.
