# Editor authoring and automation roadmap

Tracking: [Editor workspace #45](https://github.com/PIndustries/arkline-engine/issues/45). This is a staged design, not a claim that the editor can already create and import arbitrary ships. Native acceptance for a packaged build must be recorded separately.

## Current source boundary

The integrated workspace embeds the shared game client and a local server worker. Play/Pause/Step/Stop control persisted simulation time; Stop preserves the world instead of rewinding. The outliner and systems inspector expose actual state. Local identities retain normal ownership checks. See [editor operation](editor.md).

The first material editor provides bounded hull/trim color and authored-emission overrides, atomic project saving and bounded undo/redo in the embedded hangar. This is not yet a reviewed asset revision, arbitrary shader editing, or evidence of visual acceptance. It must not change a shared mesh cache, packaged asset, multiplayer state or production world.

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
