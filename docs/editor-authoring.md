# Editor authoring and automation roadmap

Tracking: [Editor workspace #45](https://github.com/PIndustries/arkline-engine/issues/45). This is a staged design, not a claim that the editor can already create and import arbitrary ships. Native acceptance for a packaged build must be recorded separately.

## Current source boundary

The integrated workspace embeds the shared game client and a local server worker. Play/Pause/Step/Stop control persisted simulation time; Stop preserves the world instead of rewinding. The outliner and systems inspector expose actual state. Local identities retain normal ownership checks. See [editor operation](editor.md).

The current material source provides a common seven-class hull/trim finish, authored-emission multiplier, optional roughness/metallic overrides, atomic project saving and material-specific undo/redo. The legacy `Assets/Pioneer.material.json` filename remains for compatibility. A render-only Fleet materials gallery shows two matching selected-class assets at normalized display scale, without creating owned ships or changing gameplay. This is not general revisioned authoring or arbitrary shader editing. Packaged acceptance is separate; shared caches, packaged assets and public worlds are not rewritten.

The existing Pioneer equipment catalog provides stable device/port IDs, positions and typed connections. Only the documented first-room behaviors are simulated. Fleet design diagrams and proposed engineering systems are not all executable systems. See [equipment catalog](pioneer-equipment-catalog.md) and [engineering contract](ships/engineering-contract.md).

## One command model for UI and automation — proposed

Use an editor project document separate from the runtime SQLite world. Each authoring command carries `project_id`, `expected_revision`, `command_id`, target stable IDs and a typed payload. UI buttons, REST and MCP call the same validator and dispatcher; adapters must not write files or game state directly.

1. Stage an edit against the expected revision and validate types, units, ranges, references, geometry and budgets.
2. Show a diff and preview the staged result in the isolated viewport. Failed validation keeps the last valid preview.
3. Apply an explicit transaction, increment the revision and record the inverse operation for undo. Repeated command IDs return the original result rather than replaying the edit. Conflicting revisions fail with the current revision.
4. Save an atomic, versioned project manifest and referenced asset hashes. Undo/redo are new validated revisions, not unchecked writes. Crash recovery must not lose a previously acknowledged save.
5. Export a reproducible asset package for review. Official content still goes through normal source review and release validation; editor Apply does not publish to a live universe.

Runtime events are a different stream. Mining, cargo, movement, damage and timed room outputs are observations of the local authority; they must not silently become authoring edits. A future “capture current transform as design” operation must be explicit. Rewinding design revisions must not replay or reverse the economic ledger.

## Ordered delivery slices — proposed

| Slice | Deliverable | Acceptance gate |
| --- | --- | --- |
| 1. Project revisions and material inspector | Stable project/object/material IDs; bounded color, roughness, metallic and emission edits; stage/apply/save/undo | UI and command adapter produce identical revisions; NaN/out-of-range/stale writes fail; reopen reproduces materials; other instances stay unchanged |
| 2. Props and geometry | Asset browser, explicit import report, transform gizmos, primitive props, mesh replacement and collision preview | Finite meter-based transforms; invalid geometry rejected; render, picking and shadows use the same mesh; save/reopen keeps IDs and placement; frame time measured |
| 3. Ship assembly | Hull dimensions, interior/deck registration, hardpoints, turret mounts and engine/boarding locations | Exact class bounds; clear corridors; mounts intersect hull; turret axes and muzzle positions tested; no silent mesh rescaling; collider fit reviewed |
| 4. Room/system graph | Inspector and graph editor based on shared typed equipment/ports/connections | Unique IDs, valid port directions/types and explicit supply/control links; disconnected supply changes actual implemented outputs; commanded vs actual/interlocks remain visible |
| 5. Automation adapters | Local authenticated REST plus MCP over the same command model; read-only inspection first | Automated stage/preview/apply/undo round trip; unauthorized/cross-project/stale/replayed commands rejected; no production endpoint or shell execution |
| 6. Material/shader authoring | Reusable material definitions first; optional shader source/graph editing later | Compile/validate off the frame path; atomic last-good program replacement; bounded compile resources; native GL and WebGL validation; no unsupported effect advertised |

A single ship and prop should complete import → edit → preview → save → reopen → automated inspection before expanding the asset catalog. Turret visuals and graph diagrams alone do not establish working gameplay or a complete engineering simulation.

## Actual GLB runtime limits

`apps/client/src/ship_asset.rs` currently decodes trusted embedded assets, not a user-facing general importer. Its constraints must appear in an import report before preview:

- Maximum 16 MiB; one embedded binary buffer; no filesystem/network resource resolution.
- At most 4,096 nodes/visits, 128 meshes and 128 output primitives; hierarchy depth is bounded at 32.
- Static indexed triangles with positions and normals; no skins, animations, morph targets or sparse accessors. Total expanded index/vertex budget is 600,000 (at most 200,000 triangles); each accessor is also bounded.
- Opaque untextured materials only; no image textures, normal maps or alpha blending. The only accepted extension is `KHR_materials_emissive_strength`, with bounded intensity.
- Finite nonsingular transforms and normals; invalid indices and degenerate triangles are rejected. Negative-determinant node transforms reverse winding. The decoder does not repair a badly authored hull or certify a watertight structure.
- Authored meters become renderer units by dividing by 100. Ship-specific dimensions, interior fit and hardpoints need additional validation; successful GLB decoding is not proof of those constraints.

Do not present Blender/glTF feature support as complete. Texture workflows, arbitrary imported scenes, collision generation, editable topology and shader graphs require separate implementations and measured validation.

## Local automation boundary — proposed

REST should be disabled by default, explicitly enabled per editor project and bound to numeric loopback. Require a random per-session secret in an authorization header, keep it out of URLs/logs/project exports, and reject unexpected Host/Origin values to prevent browser-origin or DNS-rebinding abuse. MCP should preferably use a local stdio adapter tied to the same project/session, with narrowly named tools and the same authorization checks.

Expose capabilities and bounded read-only inspection before mutation. Proposed operations include inspecting the selected object/catalog, staging typed edits, validating a revision, requesting a preview, applying a revision, undoing and exporting. Use bounded request sizes, timeouts, rate limits, audit records without secrets, canonical project-relative paths and symlink/path-traversal checks. No arbitrary code execution, filesystem access outside the project, remote network binding, production credentials or hidden game-administrator commands.

Claude, Codex, Grok and other clients can integrate through these documented adapters if they support the selected transport. None is currently verified as an editor automation integration. A protocol name or successful connection is not acceptance: each client needs a real inspect → stage → validate → preview → apply → undo test against an isolated project.

## Local contribution and detailed design priority

This milestone stays local: contributor workspace, candidate engineering, asset editing and reproducible preview. In-editor GitHub browsing and the candidate engineering panel began in 26.10.12.3. Build 26.10.12.5 adds native device authorization and direct GitHub API access; real account approval remains pending app registration. See editor.md for precise validation and remaining workflow boundaries. Read-only issue/PR/wiki status should cite its repository and freshness. Subsequent contribution actions should expose the exact branch/diff/target before publishing and leave account tokens outside project documents.

The detailed vessel source should resemble a reviewable construction assembly; the runtime should use reduced render, collision, mass-region and system representations, not one draw call/entity per bolt. The [design-to-runtime contract](ships/design-runtime-contract.md) defines mass provenance, coordinate transforms, full inertia, conservation through reduction and reproducible bake evidence. Unknown densities/strengths stay unresolved, and candidate engineering values do not silently replace server balance parameters.

Tracking remains in [#45](https://github.com/PIndustries/arkline-engine/issues/45) for the local workspace, [#10](https://github.com/PIndustries/arkline-engine/issues/10) for asset import/bake, and [#32](https://github.com/PIndustries/arkline-engine/issues/32) for authoritative engineering. These are complementary gates, not a new promise of a complete CAD or physics package.

## Local experimental physics and provenance

The implemented local Physics Lab is separate from authoritative gameplay: one-dimensional thrust under uniform gravity (`a = F/m - g`) and a seeded kinematic point-cloud visualization. The source labels the limits explicitly: constant mass, no drag/fuel/ground collision/orbital gravity, and no cosmology, particle-force/collision or gravity coupling in the point cloud. Build 26.10.12.3 native checks confirmed a 20 ms step and point-cloud extent change; this does not validate a broader physics model. These local experiments must not modify a ship's live flight model or mint runtime resources.

Future deterministic natural-phenomena jobs need seeded/versioned inputs, finite material provenance, bounded work and replay-safe saved results. They must preserve existing ore conservation; visual particles and depleted deposits do not authorize material creation or respawn. The [design-to-runtime contract](ships/design-runtime-contract.md) describes this boundary and separates render, collision and simulation budgets.

Asset history should map stable IDs to saved project fields, source revisions and Git history, with typed semantic design diffs and local export reports. Read-only GitHub issue/PR/wiki inspection is the first integration step. Creating commits or submitting PRs from the editor is not currently implemented or implied by connecting a repository; those future actions require explicit reviewable targets and changes.

## Runtime transition and visibility debugger — partial

Track this work in [#45](https://github.com/PIndustries/arkline-engine/issues/45). Native 26.10.12.3 checks demonstrated actual triangle-edge wireframe and last-scene submitted draw counters. Source for 26.10.12.4 adds context-aware camera buttons, active scene/viewed pilot and a bounded 32-entry observed client-mode history. API tests establish camera-only changes without requests or snapshot mutation; native checks confirmed the paused Cockpit → 3D External viewport change and history entry without advancing world time. Other transition cases remain pending. See [editor operation](editor.md) for exact modes and clock meanings.

This partial history is not a full event trace, GPU latency measurement, asset lifecycle record or interest/visibility debugger. Remaining scope:

- Trigger and observe exterior ↔ interior ↔ cockpit, remote/drone camera-feed and surface transitions through real supported gameplay/camera actions. Identify the active scene, camera, pilot/entity, source system and transition state. Do not teleport or bypass ownership to make a demonstration succeed.
- Extend the existing mesh wireframe with actual collision shapes, equipment/port anchors, selection bounds and interest/visibility diagnostics. Distinguish authored geometry, loaded resources, submitted draws, culled objects and authoritative entities. A debug box is not proof of a collider unless it comes from the collision source.
- Inspect real loaded objects/assets with stable IDs, source/hash, instance counts, transforms, material/program bindings and lifecycle state. Show why an object was loaded, retained, culled or unloaded and which view/feed references it.
- Report simulation/update, CPU frame and GPU timing separately, plus draw calls, triangles, resource memory where measurable, network update rate/latency/bytes and snapshot age. Embedded play must say **in-process** rather than fabricate network metrics. Unsupported GPU timers must show unavailable; frame interval/FPS is not GPU execution time.
- Capture a bounded diagnostic record around a transition, including before/after state, resource counts and timing samples. Avoid credentials/player-private data, unbounded per-frame logging and synchronous GPU readback stalls.

Acceptance: execute the real transition matrix on a stated local fixture; verify the overlay uses the same transforms, visibility and collision/asset sources as the runtime; inspect main and camera-feed views; confirm stopping/reopening returns resource counts to a documented baseline; compare timing with debugging off/on. Pending state, failed transitions and unavailable metrics must remain visible. Do not claim correctness from a drawn overlay or a single FPS sample.

### Shared material slice acceptance boundary

The 26.10.12.4 source extends the common finish to matching hull/trim surfaces in the hangar, undocked 3D rendering and camera-feed rendering. The schematic 2D view is not a PBR surface preview. Roughness/metallic alter the existing shader response; no new meshes, textures or physical vessel simulation are implied. Native GPU readback verifies two gallery Pioneer instances change/reset, while all-class geometry parity is unit-tested. Native checks confirmed the saved finish on both gallery Pioneers and an undocked exterior, immediate Inspection finish while Play advanced time, and undo back to the saved document. Native 26.10.12.5 checks verified the repaired gallery controls and all seven classes, plus Industrial satin on both Prospector instances and undo to the saved state. Native camera-feed propagation remains pending. General editable material graphs, mesh generation and physically validated engineering are still separate roadmap work.


### Connected candidate component selection

The Engineering tab uses a dedicated 3D candidate-design view. Select a component in the Pioneer design outliner or the view to inspect the same stable component ID in the Engineering panel. Position and extent edits update the design preview; undo/redo and saved engineering documents retain their existing behavior. The local simulation can continue independently while this authoring view is open.

These boxes visualize the supplied engineering document. They are not imported hull triangles, commissioned room devices, collision geometry or authoritative physics. An empty document remains empty until a contributor adds components. Live-equipment selection and event-node bindings still require explicit integration with the equipment catalog.

### Playing and inspecting live equipment

Play returns to the Ship workspace and game viewport from any authoring workspace. F5 plays/resumes from an authoring view and pauses an active game viewport when a text field is not focused. A paused or stopped world shows a Resume game action in the outliner; gameplay commands remain disabled until the authority runs.

In the embedded editor, station actions occupy a compact bottom strip outside the 3D viewport. On-foot actions use a bounded scrolling strip. The embedded HUD omits the standalone client branding and duplicated pilot block.

The Live Pioneer room equipment outliner opens the Equipment inspector. It shows server-confirmed output separately from commanded state, declared connections, and supported breaker/light/vent/door actions. Play is required for direct operation; ownership and server interlocks still apply. Breaker, light, and door commands can also be added to an event draft while paused. This only creates an unconnected node: wiring, validation, Apply, and Trigger remain explicit. Vent graph operations and general entity bindings are not implemented.
