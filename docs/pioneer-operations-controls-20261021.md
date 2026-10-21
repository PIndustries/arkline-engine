# Pioneer operations controls — 21 October 2026

This bounded group applies the researched management/interaction principles to existing Arkline simulation rather than copying another game's artwork or adding fictional systems.

## Concrete corrections

- Explicit drone selection survives a missing drone report as a missing selection. It no longer silently commands the first remaining drone. The panel requests deliberate reselection; selecting a current drone still routes real ID-bound commands.
- Mining Modules and Drones share a control-ownership summary identifying the currently controlled ship. Active owned routines state precisely that manual orders cancel them; viewing telemetry or selecting a drone does not take control. Existing pending/stale/paused/on-foot restrictions remain visible.
- Manual mining activation, stop and unlock use fresh received state consistently with the target rack and drone controls.
- Mining-drone assignment is disabled when the actual shared ship hold is full, with a specific reason. No independent drone cargo, batteries or bay inventory are invented.
- The authority's actual routine-stage message appears directly below the stage badge, including ordinary waits rather than only Blocked messages. Goal, delivered ore, current haul and real capacity remain separate.

Server commands, cancellation semantics, permissions, protocol, geometry, renderer and user worlds are unchanged. Denied console integration, scene-initialization camera correction and renderer work were not retried or included. The saved console draft remains unintegrated.

## Validation and limits

Four focused checks passed, including real pointer requests, disappearance/reselection, stale-order suppression, full-hold gating and unchanged received snapshots. One isolated native authority fixture rendered Mining, Modules and Drones at 1500x960; all three actual images were viewed. The fixture's real routine was travelling to the belt, so its stage report and zero deployed drones are genuine fixture state. No live user routine was started or changed.

Evidence: `.build/logs/20261021T052842142136Z.{json,log}` and `/tmp/pioneer-operations-workspace-20261021`. This is not live desktop acceptance, a performance claim or completion of the whole flagship interface. The earlier camera-initialization test remains an explicit known failure pending its separately blocked correction. The .27 launch-target question remains unanswered; no project/window switch or live app input is authorized by this group alone.
