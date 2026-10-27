# Native desktop acceptance: Pioneer 26.10.20.7

The signed local build **26.10.20.7** passed the component viewer and complete docked physical journey in the actual macOS editor. Runtime source is **cbf48c5cd09a97389d0260c79a09fbac63089f48**. PID **64207** remains open, paused and seated, on the disposable `/tmp/arkline-physical-native-osqa-20261020` project. The source tree was clean at ef53ed6 before these documentation updates; no runtime source changed during desktop acceptance.

[Detailed receipt](PIONEER-NATIVE-OS-ACCEPTANCE-20261020.json) · [Image/action audit](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/images-final-audit.json) · [Running identity](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/final-running-identity.json)

## Completed in the running app

- Component viewer: source-parts filter 686/797, clear filters 797/797; exact pressure hull, glazing and fastener selections; actual orbit, pan, wheel zoom, Fit, Reset, isolate/context, Focus piece and Fit ship. Paused authority world and clock were unchanged.
- Leave pilot seat, walk the upper corridor, descend both actual stair flights from 12.68 through 11.16 to 8.88 m, then climb both flights back to 12.68 m. Sampled feet remained grounded with matching registered stair surface heights.
- Walk through the inner hatch into the vestibule, close it, open/cross/close the outer hatch, board the lift and use the actual Lift to station button. Walk off the stopped platform onto station support and beyond the hull overhang; return and ride back to ship level.
- Return through both hatches in sequence, secure them, walk back across the cockpit and use Take pilot seat. Final authority confirms docked Pioneer with no walking avatar.
- Actual First/Third person switching and independent head look. On the physical cockpit computer, Corp navigation and `NativeFocusQA` typing succeeded without submission. With pointer away, text focus kept the camera geometry pixel-identical under Shift+left; after blur the same key moved the view. The authority world payload remained unchanged across these UI checks.

[Upper deck](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-upper-deck-stairs-approach.png) · [Actual stairs](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-lower-stair-entry-headlook.png) · [Lower deck](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-lower-deck-corridor.png) · [Station endpoint](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-station-floor-after-walk.png) · [Reseated](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-roundtrip-reseated.png) · [Typed physical computer](.build/packages/pioneer-docked-fixed-20261020/native-os-acceptance/os-physical-computer-typed.png)

## Evidence and limits

All **67** desktop PNGs decode at 3000×1984. All **67** top-level OS driver actions and **344** walking driver calls returned zero. There are **14 completed walking segments** and **three retained bounded helper stops** caused by short sampled keys or convenience-waypoint precision; the journey resumed from actual observed feet. No runtime change or authority bypass was needed. Input used ordinary mouse/keyboard events; authority reads used SQLite read-only mode, never movement writes or direct protocol commands.

The two lift rides contain **135/133 observations**, including **122/121 moving samples**. At every sampled moving/stop state, the pilot remained grounded on BoardingLift with unchanged binding and feet matching the platform within 8.4e-16 m. These 10 Hz observations and saved screenshots do not expose every rendered or 25 ms authority frame. Seat animation was checked at endpoints. The distant scenery's small lift parallax was reviewed against the actual station-feet camera path; no stale-camera route was found.

The text-focus comparison retains **0/550,000 changed geometry pixels** while focused, then **549,572/550,000 changed pixels** after blur and positive head look. Computer input is intentionally disabled when paused, so its successful interaction checks ran while playing. The app is left paused afterward.

This run covers one owned circular-fleet Pioneer at yaw zero. Rotated multi-vessel OS journeys, window resizing/focus-loss, middle-button-specific pan, import/material save/export and selecting every source object individually are outside this desktop scope. Existing explicit native GPU import/material, hatch-state, source-identity, focus/layer and frustum evidence remains separate. Two vestibule-look captures had the pointer over UI buttons and do not establish door-facing visual states.

Automatic approval review rejected granting the app persistent access to the entire Documents folder because close/reopen authorization did not cover that permission. No grant executed. The exact Don't Allow action and a fresh disposable `/tmp` project allowed acceptance to continue without changing that security boundary. No further user handoff answer is pending for this app run.

No publication, push, PR/issue/project mutation, merge, deployment, file moving/deletion, external-drive write or user-project modification occurred. Local required checks and private GPU results are in [the build validation](PIONEER-DOCKED-VALIDATION-20261020.md); this acceptance does not imply published exact-head CI.
