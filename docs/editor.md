# Arkline Editor

Current work is focused on the local developer workspace: design, inspect, edit and test in one window. Login, deployment and public-server changes are outside this authoring milestone. Source functionality and packaged native acceptance are tracked separately in [issue #45](https://github.com/PIndustries/arkline-engine/issues/45).

Arkline Editor embeds the real game client and a local authoritative world in one native Rust window. The viewport uses the same renderer, ship assets, game controls and protocol commands as the standalone client. No second game window or server process is launched.

```sh
cargo run --locked -p arkline-editor -- --project work/my-world --pilot EditorPilot
```

The window contains a Play/Pause/Step/Stop toolbar, live world outliner, central game viewport, read-only systems inspector and project/console panels. The world opens paused. Press **Play** to operate the ship using normal game controls inside the viewport.

## Simulation controls

- **Play:** resumes the local simulation. After Stop, the editor reconnects its saved test pilot before resuming.
- **Pause:** freezes authoritative simulation time. Movement, mining and timed systems do not advance because the interface is repainting or wall time is passing. Pending flight input is cleared.
- **Step 50 ms:** advances one fixed simulation tick while paused. It does not turn Play on.
- **Stop:** pauses and disconnects the test session while preserving the world. It does not rewind to the start of the play session or delete earned resources. Play reconnects in the same window.

A worker owns the simulation and database, so SQLite writes do not run on the rendering thread. Time advances in 50 ms ticks with a maximum 250 ms advance per worker update. Long stalls do not cause unbounded catch-up. Closing the editor requests worker shutdown; the UI does not wait for database writes.

## Projects and identities

Project data lives in `<project>/.arkline-studio`, separately from the former external-process launcher's `.arkline-editor` directory. This avoids treating an old wall-clock world as a paused editor save. The editor database includes its simulation clock, persisted atomically with ticks. Reopening starts paused at that time. Standard startup cleanup still stops active mining/jobs; this is not an exact save-state debugger.

The editor uses isolated local identities and normal pilot ownership checks. Credentials stay in the project's client directory. Official OAuth credentials, public relay connections and server administration are not exposed in this workspace. Network server modes refuse editor-clock databases, and editor mode refuses existing unmarked/wall-clock databases. See [local identity boundaries](local-development-identity.md).

## Viewport and inspection

The central viewport runs normal gameplay. Editor toolbar and side panels reserve their own space and input. Native full-screen/session/new-window actions are disabled in embedded mode. Select a pilot in the outliner to inspect its public snapshot. Private inventory/room data appears only for the active test pilot.

The inspector reports actual simulation values: ship class and dimensions, cargo, position or deck-local avatar coordinates, velocity, power, drones, mining modules and the connected Pioneer room catalog. It does not invent sensor values or imply that every catalog connection is independently simulated.

## Live Pioneer materials

Select **Materials** in the right panel to preview hull and trim colors and authored emission in the hangar. Parameters apply immediately, including while paused. **Inspection finish** provides a bright inspection preset; **Authored defaults** restores the source palette. Undo/redo retain up to 100 edits. The preview checkbox lets you compare against packaged materials without changing the document.

**Save material** atomically saves `Assets/Pioneer.material.json` in the project. Saved materials load on startup. **Reload saved material** is undoable. Colors use linear RGB in the 0–1 range; emission is an authored multiplier from 0–4, not a reactor-power setting. Invalid or oversized documents are rejected. Preview changes only Pioneer instances in the embedded hangar; it does not overwrite packaged assets or update public servers. Material color editing is not yet a physically complete shader/material authoring system.

## Current scope

This is the first integrated play-and-inspect workspace. It is not yet a general mesh/room/shader authoring tool, a general PLC graph editor, a general revision/undo editor or a replacement for an external modeling application. Project switching currently happens through launch arguments; no project browser is implemented. The outliner lists objects but does not yet support drag placement, gizmos or arbitrary property editing. Official account login and multiplayer network acceptance remain separate from this local workspace.

Validation covers actual movement/mining pause, bounded stepping, persisted clock, atomic tick rollback, local session Stop/Play, client input/window ownership and native/WASM compilation. Native visual acceptance must be recorded for each packaged build rather than inferred from automated checks.

See the [authoring and automation roadmap](editor-authoring.md) for geometry, ship assembly, connected systems and REST/MCP work.

## Contributor workspace and candidate engineering

Build 26.10.12.3 adds **Community · GitHub** with a selectable `owner/repository`, recent issues and pull requests, milestones, project items and an official wiki text reader. It uses the existing GitHub CLI login, runs bounded background requests, and reports freshness or partial failures. No tokens are saved in the project. This first integration is read-only; it does not clone code, create forks, commit, submit PRs, or stream live GitHub events. Repository selection is a preference, not a Git checkout. Refresh explicitly to update the results.

The **Engineering** tab edits candidate component boxes, material labels, mass and placement, with a top-down map and mass/center-of-mass/diagonal-inertia estimates. Save/reload uses `Assets/Pioneer.engineering.json`. Component ID allocation does not rewind on undo or reload within the session. It does not regenerate the ship mesh, collision shapes or authoritative physics.

The candidate engineering work describes entered component masses and box envelopes, not measured or CAD-derived physical ratings. Its center and diagonal-inertia estimates must not be presented as authoritative flight data. Detailed source assemblies should eventually produce reduced render/collision/mass/system representations through a validated bake, rather than rendering every construction fastener. See the [design-to-runtime contract](ships/design-runtime-contract.md).

## Saved source history

Materials and Engineering include manual **Inspect saved source history**. For a project inside a Git working tree, this runs bounded read-only Git inspection against the fixed saved asset file. Component selection maps to its persisted ID line; otherwise attribution uses the document's first line. The report distinguishes untracked files, working-tree edits and committed attribution. It is not per-property semantic history, model-surface picking or automated Rust code editing. The default standalone FirstProject is not a Git checkout, and the panel reports that fact. Launch with `--project /path/to/checkout` to author in an existing repository.

## Physics and render experiments

**Physics lab** is isolated from the saved gameplay world. It provides fixed 20 ms stepping, Play/Pause, time speed and explicit parameter Apply/Reset. The thrust example is one-dimensional constant acceleration `F/m - g` under uniform gravity. Its seeded point cloud uses linear radial scaling; it is not a cosmological, orbital or particle-interaction simulation. Neither replaces Pioneer runtime physics. Explanations and numerical bounds are visible in the lab.

**Render debug → Mesh wireframe** overlays actual cached mesh triangles in native hangar and flight scenes. It reports submitted triangles, mesh draws and added wireframe draws from the last GPU scene pass; the last pass can be a camera feed. It excludes sky/shadow passes and is not a total frame profiler. Debug rendering adds work. Automatic transition captures, collision/interest overlays and GPU timestamps remain pending. Changing workspace pauses gameplay, and hidden viewport time is excluded from subsequent frame samples.

Native checks for 26.10.12.3 confirmed real GitHub issues/wiki reading, wireframe and draw counters, one 20 ms physics step, engineering add/remove/save and the not-a-Git-project history message. Automated checks cover deterministic physics, source attribution in disposable Git repositories, bounded workers, component IDs and material persistence. These checks do not establish full ship-building or performance acceptance.
