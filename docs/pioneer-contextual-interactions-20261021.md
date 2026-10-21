# Pioneer contextual interaction milestone — 2026-10-21

Research direction is an original interface over Arkline authority, not a promise to copy competing games or invent systems.

| Feature group | Current evidence / remaining scope |
|---|---|
| Cockpit/flight instruments | Shipped snapshot-backed physical display painters in `pioneer_docked_scene.rs`, `flight3d.rs`; complete flagship composition remains partial. A context-sensitive ownership/reference panel now uses the shared sampled flight bindings; shared walking-key integration remains blocked by worker approval. |
| Doors, seats, lift | Shipped bound authoritative commands/interlocks in `station_ui.rs`, server `docked_traversal.rs`; this milestone adds consistent seat readiness/reasons, real lift landing/target and nearby range guidance. F nearby-action coverage shipped in 26.10.21.4 for seat, same-side hatches and unambiguous lift controls; arbitrary authored doors/consoles remain missing. |
| Map/target management | Shipped persistent view selection and real target/drone controls (`target_controls.rs`, `flight_workspace.rs` where applicable); fleet/map breadth remains partial. |
| System workspaces | Shipped grouped destinations in `operations_workspace.rs`; destination text search shipped in 26.10.21.3; query preserves selected content. |
| Transitions/input | Shipped held-input release guards; this milestone adds focus-aware Escape back to Flight without camera/body/clock commands. The separately denied scene-init camera correction remains unresolved. |
| Logs/console/reconnect | Existing activity/audit and reconnect uncertainty handling in `lib.rs`; console draft remains unintegrated under its denial; persistent-server reconnect policy is not changed. |
| Audio | Output device and confirmed-world cues exist; Sound feedback now separates effective mute, pause, test queue/callback receipt and unconfirmed hearing. Mechanical hatch/lift/seat cues and automatic device-change recovery remain missing. |
| Lighting | Existing raster/PBR settings in `lighting_settings.rs`; denied renderer changes remain untouched. No hardware ray tracing claim. |

## New behavior

Both pilot-seat panels explain the same real blocker and use the server's <=0.015 m boarding-lift stow tolerance. Distance, landing, open hatches, moving lift and seat transitions are explicit. Service-power loss alone does not invent a seat restriction.

Boarding lift controls show actual landing/moving target/stopped-between-landings state and nearby control distance; authority gates and request bindings are unchanged. Existing on-foot movement keys are now visible in the primary interaction strip. E remains strafe.

Escape closes only the current management workspace when the viewport owns keyboard input. Prior-frame text focus is retained because egui can surrender focus at begin-pass on Escape: editing gets the first Escape, navigation a subsequent one. Return releases walking/flight input and requires held keys to be released before world controls resume. It does not change camera, avatar, simulation mode or issue ship commands.

## Preservation and validation

No server/protocol, collision, layout, registration, authored asset or world migration changes. Use the known PioneerFinish11QA project; inspect fresh state and back up before normal save/shutdown. Preserve FirstProject independently. Focused contextual checks and one guarded editor build; no full-suite rerun. The earlier separately blocked camera test is not claimed passed.
