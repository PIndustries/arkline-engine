# Editor workspace map

This page describes the bounded editor workspace, its authoring previews, and the boundary between editor tools and the running Arkline world. The consolidated workspaces, initial typed material-preview graph, and editor-only scene destinations are implemented in source and covered by automated checks. Packaged native interaction remains a separate acceptance step; this page does not claim that native workflows have been accepted end to end.

## Workspace flow

```text
+-----------------------------------------------------------------------+
| Game                 Materials              Physics          Community|
+-----------------------------------------------------------------------+
| Game workspace:                                                       |
| Outliner / selection | Game viewport / scene | Inspector              |
| World entities       | Cockpit / Station     | Live state, equipment,|
| Pioneer rooms        | Hangar / Interiors    | room, design, debug   |
+-----------------------------------------------------------------------+
| Materials workspace:                                                  |
| Node library +       | Material graph canvas  | Selected node         |
| compact ship preview |                        | properties + output   |
+-----------------------------------------------------------------------+
```

The four primary areas organize the work. Game and Materials use contextual content: the Game inspector exposes live-state and project tools, while Materials uses a node library and compact ship preview beside the graph, with selected-node properties on the right. The material preview reuses the fleet renderer; exact panel sizing and native input acceptance still need to be recorded against a packaged build.

## What the workspace does today

- **Game** embeds the real client and local authority in one window. Play, Pause, fixed-step and Stop operate the development world. A shared development host owns its world clock; joining does not synchronize local project files.
- **Outliner and properties** expose the current world selection and project tools. The outliner is not a general scene-placement system and properties are not arbitrary object editing.
- **Room layout** authors a Pioneer room document. An explicit, revision-checked host acceptance can publish the accepted layout to the shared protocol projection used for room rendering and collision. A saved local candidate is not implicitly applied.
- **Service paths and Engineering** remain local design proposals and estimates. They do not install conduits, change authoritative physics, or create synchronized project documents.
- **Event flow** is a bounded runtime graph with its own supported nodes, validation and simulation-clock stepping. It is not a general script or shader editor.
- **Materials** already edits the shared local finish document and previews it through the renderer's existing material path. This includes color, authored emission and optional roughness/metallic controls described in [editor.md](editor.md).
- **Physics** contains an isolated experiment, not a simulation of Pioneer or the online universe.
- **Community** reads public GitHub information and supports optional contributor authentication. It is not game-account login or the private official account service.

## Material graph scope in this change

The initial graph is a small typed material-preview graph, not a GPU shader language. Its supported path is:

```text
[Color] ----+                       +----------------+
            +--> [MixColor] -------->|                |
[Color] ----+        ^               |                |
                     |               | Fleet Material |
[Scalar] --+         |               |                |
           +--> [MultiplyScalar] --->| Roughness      |
[Scalar] --+                         | / PBR Output --+--> Existing ship preview
                                     +----------------+
```

`Color` supplies a color value, `Scalar` supplies a number, `MixColor` combines two colors using a scalar factor, and `MultiplyScalar` combines scalar inputs. The Fleet Material/PBR output maps the supported graph values to hull and trim color, authored emission strength, and optional roughness/metallic overrides. Evaluation feeds the compact ship preview and existing fleet finish renderer; it does not compile or install a new shader. This first graph does not promise textures, image sampling, normal maps, arbitrary node types, custom shader source, or a material assignment pipeline for every asset.

Graph editing and preview are project authoring. They do not alter the authoritative world, equipment behavior, or server state. Save/reload, graph validation, malformed-value handling, and native visual results must be recorded for the exact build before claiming those workflows accepted.

## Editor-only scene destinations

The scene controls place the active test pilot in the editor sandbox at Cockpit, Station Hangar, Interior Deck 1, or Interior Deck 2 while the simulation is paused or playing. This is an explicit authority command that changes only the pilot's avatar placement: Cockpit clears the avatar, Hangar places it at the ship's berth, and the two interior choices place it at the entry or lift point. It bypasses the normal walking and boarding route for editor inspection. The ship must already be docked; interior placement requires a Pioneer, and remote control or incompatible interior clients are rejected. This does not move or undock the ship, alter room doors, or advance simulation time/systems. The host's paused/running state is preserved, and the UI reports the command acknowledgement.

This control is separate from normal gameplay camera buttons and from the existing free camera. Acceptance must verify each destination from both paused and playing states, the visible scene and pilot context, unchanged clock across the jump, and unchanged pause/play state. The control is editor-only and is not evidence that a player-facing transition, boarding path, or production travel system exists.

## Explicit future work

- Texture import and authoring, including reviewed base-color, normal, roughness/metallic and occlusion support.
- Custom shader source or a general shader graph, beyond the initial typed material-preview nodes.
- General 3D object placement, transform gizmos, arbitrary property editing and full mesh/room authoring.
- Installed, authoritative power/data/water networks and utility simulation. Candidate routes and engineering estimates do not provide these systems.
- Broader material assignment and asset workflows, graphics-quality controls, and native/browser performance acceptance.

## Tracking and acceptance boundaries

- [#45](https://github.com/PIndustries/arkline-engine/issues/45) owns the packaged editor lifecycle and bounded local workspace. Extend its acceptance to cover the consolidated panels, the material graph preview path, and scene jumps in paused and playing states with clock preservation. Keep Host-to-Local, two-native-editor, route save/reopen and failed-load/world-lock recovery open until each is observed.
- [#50](https://github.com/PIndustries/arkline-engine/issues/50) owns the room-layout and service-path workflow. Preserve the distinction between an explicit host-accepted room revision and local candidate route/engineering/material documents. Keep route authoring/save/reopen, equipment placement, endpoint binding and 3D gizmos open; authoritative utilities remain with #32.
- [#51](https://github.com/PIndustries/arkline-engine/issues/51) owns loopback shared-development session acceptance. The independent-process revision/clock test is not native two-window, reconnect/restart, cross-machine tunnel, membership UI or shared project-file acceptance. Material graphs and local project drafts must not be described as synchronized by Host/Join.
- [#7](https://github.com/PIndustries/arkline-engine/issues/7) owns player-facing movement and camera acceptance. Editor scene jumps do not close its on-foot, cockpit/tactical, camera-follow, landing or representative performance criteria.
- [#41](https://github.com/PIndustries/arkline-engine/issues/41) owns visual-quality acceptance and remains **In Progress** while the material work is being implemented. The initial graph reuses the existing render preview and does not complete texture support or the docked, undocked, tactical and visible remote/drone consistency pass. After the material pass is accepted, move the issue to **Todo** for its remaining visual-quality work; do not close the broader issue. Keep geometry/art direction, station/interior material preservation and performance criteria open.

Source changes, automated tests, packaging, native interaction, exact-head CI, merge and release are distinct evidence. Report each separately and leave criteria open when their corresponding result has not been observed.
