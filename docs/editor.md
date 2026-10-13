# Arkline Editor

Current work is focused on the local developer workspace: design, inspect, edit and test in one window. Game-account login, deployment and public-server changes are outside this authoring milestone. Optional GitHub contributor sign-in is part of the local workspace. Source functionality and packaged native acceptance are tracked separately in [issue #45](https://github.com/PIndustries/arkline-engine/issues/45).

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

`ARKLINE_EDITOR_PROJECT` can select the default project for a native launcher or test environment; blank values are ignored and an explicit `--project` takes precedence. This lets isolated editor runs use separate local worlds without changing the normal project.

## Shared ship finish and Fleet materials — 26.10.12.4

**Materials** edits a shared local preview finish: hull and trim colors, authored-emission multiplier and optional roughness/metallic overrides. Matching paint/trim surfaces across all seven ship classes use the same policy in the embedded hangar, undocked 3D views and rendered camera feeds. The 2D tactical map is schematic rather than a shaded material preview. Cockpit interiors, station geometry and unrelated surfaces are not repainted. Changing a preview invalidates cached feeds so their next render can reflect the edit; this does not guarantee an instantaneous frame while a feed is hidden.

Roughness and metallic controls accept 0–1; the shader retains its minimum roughness of 0.08. Unchecking an override preserves each surface's authored factor. **Industrial satin** and **Weathered coating** change those two factors while preserving colors/emission. **Inspection finish** changes the inspection palette; **Authored defaults** restores document defaults. These controls adjust the existing shading response, not texture maps, mesh detail, ship geometry or authoritative engineering properties.

Undo/redo retain up to 100 material edits. The preview checkbox compares against packaged materials without editing the document. **Save material** atomically saves `Assets/Pioneer.material.json`; this legacy filename now stores the common preview finish, not a Pioneer-only material override. Existing color/emission-only documents retain authored roughness/metallic. Saved materials load on startup; **Reload saved material** is undoable. RGB values are linear 0–1; emission is an authored multiplier 0–4, not reactor power. Invalid or oversized documents are rejected. No packaged assets or public servers are overwritten.

**Fleet materials** pauses the gameplay workspace and opens a render-only asset comparison with the same Materials document/panel. Choose any of the seven classes to see two matching instances of its real cached model, its source dimensions, and whether it uses the embedded Pioneer GLB or a procedural class model. Both instances receive the same finish. Drag to orbit, scroll to zoom, or reset the camera.

The comparison is explicitly **not world scale**: each selected class is normalized for display while preserving aspect ratio. Instances are asset samples, not owned ships or spawned entities; no gameplay state or credentials are fabricated. One renderer and two instances avoid seven independent camera renders. GPU allocation happens on first opening the workspace.

Automated checks verify all seven classes use the same cached geometry/material roles for both display instances. An actual offscreen native GPU test confirms both Pioneer instances change under the shared color/PBR preview and reset to the original pixels. Native checks for this build confirmed the same saved magenta finish on both gallery Pioneers and the undocked exterior. While Play advanced simulation time, applying Inspection finish immediately made the exterior hull white; undo restored the saved document with no unsaved changes. The finish saved in the earlier build persisted across the restart into this build. Native checks in 26.10.12.5 verified the repaired gallery header and all seven class buttons: each displayed two matching models with the saved magenta hull/teal trim. Industrial satin changed both Prospector instances visibly while setting roughness 0.38 and metallic 0.75; Undo restored the saved state with no unsaved material changes. Remote/camera-feed native propagation remains pending. These observations are not complete realism or full workflow acceptance.

## Event flow — local graph authoring and stepping

Build 26.10.12.6 adds **Event flow** beneath the live 3D/game viewport in the same editor window. It is a first bounded graph interpreter, not a complete replacement for engine code or an instrumentation view of every internal subsystem. Connections, operation parameters and breakpoints are editable project data; changing them does not require compiling Rust. New primitive operations still require engine implementation and review.

The first catalog contains Manual and committed-Tick triggers, a simulation-time Delay, an If docked condition, Pioneer room light/breaker/door commands, Stop ship and a trace marker. Commands execute for the connected local pilot through existing server ownership, location, power and pressure checks. A graph cannot create credentials, write arbitrary memory, execute shell code, overwrite cargo, bypass door interlocks or administer a remote network. Room commands still require the normal room context; a rejection is a real result, not a simulated success.

- **Draft:** add/remove/drag nodes, edit parameters and before-node breakpoints, and connect typed Next/True/False outputs. Node IDs do not rewind on undo/removal within the session. Undo/redo retains up to 64 graph edits; dragging is grouped into one edit.
- **Validate:** checks schema, references, typed outputs, finite positions, bounded labels/delays and cycles. This version allows one edge per output port and rejects cycles. Dynamic control flow means editable data within this supported catalog, not unrestricted scripts.
- **Save draft / Reload saved:** use `Events/Pioneer.events.json`, independently of the running graph. Saves are bounded, atomic background writes; edits made during a save remain dirty. Startup loads a draft but does not execute it.
- **Apply to runtime:** requires a paused, connected local world, matching graph revision and an empty pending queue. The worker acknowledges success or rejection. A rejected draft leaves the previous runtime graph intact. **Load runtime draft** explicitly copies the acknowledged graph back for editing.
- **Trigger applied Manual node:** queues its installed runtime node; editing or selecting a draft does not execute it.
- **Step one node:** executes at most one ready queued node while paused, without advancing the simulation clock. A Delay schedules continuation at simulation time; it does not sleep on the render thread or jump time.
- **Step 50 ms:** commits one complete authoritative simulation tick. It can make delayed work ready and enqueue Tick triggers, but paused graph work is still stepped explicitly. It does not stop halfway through a database transaction.
- **Play / Pause:** the authority runs bounded graph work and fixed ticks. A before-node breakpoint pauses the actual world before that node executes. Resume crosses the current breakpoint once. **Clear pending events** is a separate paused operation; Stop disconnects and clears pending execution.

The runtime permits at most 128 nodes, 256 edges and 256 pending events. Each graph drain executes at most 64 nodes. Scheduling is deterministic by due time then insertion order; Tick roots start in node-ID order. The retained trace is capped at 256 entries and carries graph revision, sequence, causal parent, node, simulation time, result and host execution duration in microseconds. Host duration is measured wall-clock work, not simulated signal latency or GPU time. Snapshot polling is omitted from the trace. Commands, tick transaction boundaries and graph nodes are traced; internal physics/mining/system sub-events are not all instrumented yet.

Graph queue saturation and automatic scheduler faults pause the world with a recorded fault while preserving the session for inspection, clearing and correction. Failed authoritative database ticks remain distinct errors and never receive a successful tick record. Graph state, queue and trace are session diagnostics; restart preserves the normal saved world clock/state and reopens paused, but requires applying the saved graph again. This is not event-sourced world replay, time travel or a saved in-flight continuation.

The worker bridge uses bounded command/debug queues and acknowledges graph commands off the rendering thread. Unchanged paused worlds no longer emit duplicate 20 Hz snapshots. A stale tick-step request received after Play is rejected without terminating the local authority.

Source tests cover typed validation, cycle/revision rejection, causal order, breakpoints and node/clock separation, simulation-only delays, bounded history, queue saturation/recovery, normal Pioneer power/pressure interlocks, restart semantics, actual embedded worker acknowledgements, stale clock controls, draft persistence and native egui node dragging. Native packaged walkthrough and performance acceptance are recorded separately after testing. Follow [#49](https://github.com/PIndustries/arkline-engine/issues/49) for richer subsystem hooks, typed data pins, general entity bindings, event graph debugging and validated runtime patches.

## Current scope

This is the first integrated play-and-inspect workspace. It is not yet a general mesh/room/shader authoring tool, a general PLC graph editor, a general revision/undo editor or a replacement for an external modeling application. Project switching currently happens through launch arguments; no project browser is implemented. The outliner lists objects but does not yet support drag placement, gizmos or arbitrary property editing. Official account login and multiplayer network acceptance remain separate from this local workspace.

Validation covers actual movement/mining pause, bounded stepping, persisted clock, atomic tick rollback, local session Stop/Play, client input/window ownership and native/WASM compilation. Native visual acceptance must be recorded for each packaged build rather than inferred from automated checks.

See the [authoring and automation roadmap](editor-authoring.md) for geometry, ship assembly, connected systems and REST/MCP work.

## Contributor workspace and candidate engineering

Build 26.10.12.5 replaces the GitHub CLI integration with **Sign in with GitHub** and direct HTTPS API requests. **Community · GitHub** shows a selectable `owner/repository`, recent issues and pull requests, milestones, project items and an official wiki text reader. Public repository reading works without signing in. Projects require suitable authenticated access; missing permissions, network failures and stale results are reported explicitly. Refresh is manual, not a live event stream.

For contributor sign-in, the editor requests a GitHub device code, displays its expiry and opens GitHub for approval. It polls at the server's required interval and handles slowdown, denial, cancellation and expiry. The app verifies the returned identity before displaying a signed-in account. Credentials live in the OS credential store (macOS Keychain, Windows Credential Manager, or Linux Secret Service), never in project documents. When saving a new credential fails after safely clearing any prior credential, the UI explicitly reports session-only access. Sign-out clears in-memory access immediately and reports any failure to remove the stored credential. Cancelling and changing accounts discard pending results and account-specific data.

An operator must register an OAuth app and enable **Device Flow**, then supply its public Client ID. In **GitHub app setup**, enter that ID and click **Apply app settings**. The public ID is saved to `editor-github-client-id.txt`; alternatively set `ARKLINE_GITHUB_CLIENT_ID` at runtime or build time. Environment/build configuration takes precedence on the next launch. No client secret belongs in a distributed editor. Requested scopes are `read:user read:project`, without repository write access. Organization policies may still require approval. See [GitHub's device authorization documentation](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow).

The public app registration and an actual approved GitHub sign-in have not yet been verified for this build. Protocol, cancellation and request-boundary tests are separate from that pending end-to-end check. Anonymous live API validation loaded issues, pull requests, milestones and the official wiki with no GitHub CLI. Native 26.10.12.5 checks confirmed the unconfigured-app state, 26 loaded issues and 50 wiki pages, with Projects access explicitly unavailable while signed out. The editor suite passed 34 tests with one live test separately exercised; strict Clippy passed. The official wiki reader now retrieves anonymous Markdown directly from the fixed official raw-content host, with no Git subprocess, clone or disk writes. It follows links from Home, sidebar and footer, up to 80 discovered pages, 256 KiB per page and 2 MiB total within a 25-second request budget. Access tokens are never sent to raw content; redirects and off-wiki paths are rejected. Unlinked pages are not enumerated. A live anonymous check retained all 50 current wiki pages; unit checks cover link boundaries, content bounds and cancellation.

This integration remains read-only: it does not create a code checkout or fork, commit, submit PRs, or publish changes. Repository selection is a preference, not a Git checkout. Local simulation needs no GitHub account. A contributor GitHub token is never a game credential. Optional GitHub game login must link through the separate Arkline identity service to the same Arkline account and pilot; that federation is tracked separately in [issue #9](https://github.com/PIndustries/arkline-engine/issues/9). Native contributor authentication is tracked in [issue #48](https://github.com/PIndustries/arkline-engine/issues/48).

The **Engineering** tab edits candidate component boxes, material labels, mass and placement, with a top-down map and mass/center-of-mass/diagonal-inertia estimates. Save/reload uses `Assets/Pioneer.engineering.json`. Component ID allocation does not rewind on undo or reload within the session. It does not regenerate the ship mesh, collision shapes or authoritative physics.

The candidate engineering work describes entered component masses and box envelopes, not measured or CAD-derived physical ratings. Its center and diagonal-inertia estimates must not be presented as authoritative flight data. Detailed source assemblies should eventually produce reduced render/collision/mass/system representations through a validated bake, rather than rendering every construction fastener. See the [design-to-runtime contract](ships/design-runtime-contract.md).

## Saved source history

Materials and Engineering include manual **Inspect saved source history**. For a project inside a Git working tree, this runs bounded read-only Git inspection against the fixed saved asset file. Component selection maps to its persisted ID line; otherwise attribution uses the document's first line. The report distinguishes untracked files, working-tree edits and committed attribution. It is not per-property semantic history, model-surface picking or automated Rust code editing. The default standalone FirstProject is not a Git checkout, and the panel reports that fact. Launch with `--project /path/to/checkout` to author in an existing repository.

## Context-aware view debugging — 26.10.12.4

The right-side **Render Debug** panel provides explicit view buttons alongside the active scene and viewed pilot. Available camera modes follow the actual client context: cockpit, exterior, 2D tactical and 3D tactical in flight; Hangar while aboard a docked ship; first-person and third-person while on foot. Offline/stopped or incompatible selections are rejected. Changing the camera does not issue an authority request, teleport the pilot or enter/leave a ship.

The panel retains the latest 32 observed client-mode transitions. Entries contain a sequence, previous/new mode, reason, monotonic host elapsed milliseconds since client creation, and optional world simulation time. This records observed camera-mode changes, not every server event, asset load, visibility decision or rendered frame. Host elapsed time and simulation time are distinct clocks; neither is a measured GPU latency.

Two API tests cover 40 view changes without gameplay requests or snapshot mutation, bounded/ordered history, on-foot/deck context, docked availability and offline/incompatible rejection. Native interaction confirmed the paused Cockpit → 3D External button changed the actual viewport and recorded history entry #2 while world simulation time stayed unchanged. This checks that particular transition, not the complete view/scene transition matrix. Existing last-scene draw counters retain the scope described below.

## Physics and render experiments

**Physics lab** is isolated from the saved gameplay world. It provides fixed 20 ms stepping, Play/Pause, time speed and explicit parameter Apply/Reset. The thrust example is one-dimensional constant acceleration `F/m - g` under uniform gravity. Its seeded point cloud uses linear radial scaling; it is not a cosmological, orbital or particle-interaction simulation. Neither replaces Pioneer runtime physics. Explanations and numerical bounds are visible in the lab.

**Render debug → Mesh wireframe** overlays actual cached mesh triangles in native hangar and flight scenes. It reports submitted triangles, mesh draws and added wireframe draws from the last GPU scene pass; the last pass can be a camera feed. It excludes sky/shadow passes and is not a total frame profiler. Debug rendering adds work. Automatic transition captures, collision/interest overlays and GPU timestamps remain pending. Changing workspace pauses gameplay, and hidden viewport time is excluded from subsequent frame samples.

Native checks for 26.10.12.3 confirmed real GitHub issues/wiki reading, wireframe and draw counters, one 20 ms physics step, engineering add/remove/save and the not-a-Git-project history message. Automated checks cover deterministic physics, source attribution in disposable Git repositories, bounded workers, component IDs and material persistence. These checks do not establish full ship-building or performance acceptance.

## Package a local macOS editor

```sh
tools/package-editor-macos.sh work/editor-local-check debug
# Omit the second argument, or use release, for an optimized local build.
```

The script requires macOS, Cargo, Python 3 and codesign. It reads `BUILD_VERSION`, builds the locked editor target, takes the executable path from Cargo's artifact report, and creates `Arkline Editor.app` with the official icon and build identity. Use a fresh output directory: an existing app is never overwritten. Verify the app's shown build before recording acceptance so a running older copy is not mistaken for the new artifact.

The bundle uses a local ad-hoc signature verified by codesign. It is **not Developer ID signed or notarized distribution**, and creating it does not upload, release or deploy anything. The application identifier is `industries.p.arkline.editor.embedded`; the Arkline build version is stored separately from the bundle's product version. Local developer packaging is distinct from public release publication.
