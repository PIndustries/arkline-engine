# Pioneer — life of the ship

Pioneer is Arkline's 72 × 38 × 18 m industrial starter cutter. **v05-interior-review is the current visual reference**; older drawings remain below as design history. The running model still needs substantial shape work to match that reference. Importing the existing GLB does not remodel the hull.

The merged source includes an editable Blender/embedded GLB exterior, two docked walkable decks and the existing flight, mining, drone, cargo and scalar power systems. A separate, unmerged development stack adds fleet berths, one connected airlock room and the native editor. Its later material, inspection, cockpit and importer PRs are also unmerged. Source implementation, automated checks, native acceptance and release are recorded separately here.

**Record checked 2026-10-15 (UTC).** The merged baseline is [`78fdc5d`](https://github.com/PIndustries/arkline-engine/commit/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175); the combined editor candidate is draft [#47](https://github.com/PIndustries/arkline-engine/pull/47) at `c1170b1`. Its [exact-head CI](https://github.com/PIndustries/arkline-engine/actions/runs/38698656233) passed macOS, Linux and browser; build-specific native observations and remaining gates are below. The stack remains unmerged and unreleased.

[Design direction](#design-direction) · [Dimensions and frames](#dimensions-and-frames) · [Current runtime](#current-runtime) · [Editor and asset work](#editor-and-asset-work) · [Editable vessel](#editable-vessel-animations-and-lights) · [Evidence](#evidence-and-acceptance) · [Gaps](#remaining-gaps) · [Milestones](#milestone-log) · [Concept history](#concept-history)

[Versioned page source](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/README.md) · [Fleet archive](https://github.com/PIndustries/arkline-engine/wiki/Ship-Design-Archive) · [Project 14](https://github.com/orgs/PIndustries/projects/14)

## Design direction

![Pioneer current v05 concept](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v05-interior-review.jpg)

v05 calls for a broad, enclosed black/emerald cutter: faceted bow, integrated shoulders, heavy collars, a stepped underside and articulated laser mining turrets. The separated outriggers in v03 and earlier drill-like equipment are historical. Generated labels, checkboxes and painted proportions are not acceptance evidence.

The merged [authored model](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/README.md) is the first exterior foundation: 32,284 runtime triangles in seven material batches, with separate engine-controlled turrets. Draft [#44](https://github.com/PIndustries/arkline-engine/pull/44) adds an industrial detail pass and changes the GLB to 65,866 triangles in nine batches while retaining the body envelope and gameplay hardpoints. Neither version is a finished reconstruction of v05.

The current narrow center, projecting square nose, long side blocks and roof glazing need a deliberate shape pass. The proposed order is whole-hull blockout, bow/bridge/shoulders, roof bays and machinery, assembled turret clearances, then materials and lighting. See the [modeling and capture plan](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6183334728). It is proposed work with no new ownership claim or implementation recorded.

## Dimensions and frames

[JSON/source values](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/specification.json) · [Dimensioned SVG](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/plan.svg) · [Design reference](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/docs/ships/design-reference.md)

Current gameplay definitions in the target revision's [protocol](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/crates/arkline-protocol/src/lib.rs) govern behavior. The dossier JSON pins an older source snapshot and separates implemented values from proposed reservations; it is not a live query. SVGs visualize those numbers. Concept-image pixels are not measurements.

| Reference | Meaning |
| --- | --- |
| Body envelope | 72 m length × 38 m width × 18 m hull height. Articulated turrets extend above the hull; this is not an 18 m overall-height claim. |
| Runtime model | Metres, centered hull-envelope origin; +X bow, +Y up, +Z starboard. XYZ extents are 72 × 18 × 38 m. The loader does not resize the GLB. |
| Dossier plan | X forward from the aft bounding-box face, Y up, Z from the port face; `bounds_xzy_lwh_m` arrays use X,Z,Y ordering. This differs from model XYZ and its centered origin. |
| Interior instance | Two 32 × 14 m local plans; each deck has floor Y=0, X=plan U, Z=plan V. A tested body-to-deck registration remains missing. |
| Human reference | 1.8 m avatar, 2.4 m corridor and 1.4 m doorway width. Proposed clear door/room heights are 2.4/3.2 m, with 3.8 m deck pitch. |
| Gameplay quantities | 1,000 cargo units; 240,000 J charge capacity; 1,200 W regeneration; 30,000 J warp admission; 4,000 J turret mining cycle. These are game tuning, not reactor or structural ratings. |

Cargo units have no implemented kilogram or cubic-metre conversion. Mass, material density, pressure rating, structural strength and engineering adequacy must remain unknown until supported by declared inputs and validation. Matching bounds alone proves neither interior fit nor safe clearances.

## Current runtime

**Merged on main:** [#29](https://github.com/PIndustries/arkline-engine/pull/29) implements two private, docked Pioneer decks with twelve rooms, authoritative walking/collision, first/third-person cameras, an access-core lift transition and return through the airlock to boarding. The lift changes to a validated fixed landing; it is not a moving lift. [#34](https://github.com/PIndustries/arkline-engine/pull/34) adds compatible-client recovery. Ship operation is gated while on foot. See [interior behavior and rollback limits](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Interior-Preview).

[#39](https://github.com/PIndustries/arkline-engine/pull/39) replaces the procedural hull with the embedded GLB and editable Blender source. Its actual cached triangles serve the main view, hangar, camera feeds, shadows and selection. The bounded loader supports static indexed geometry and opaque, untextured scalar PBR materials; textures, skins, animation and morph targets are rejected. See [model runtime](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Model-Runtime).

Existing cockpit flight, finite-ore mining, drones, cargo/storage, resource jobs and scalar reactor/capacitor behavior are engine systems. Decorative pipes, radiator shapes, hatch geometry and lamps do not create a conserved electrical, atmosphere, water, heat or structural model. The interior still occupies its own scene; the exterior glazing does not establish the bridge room or pilot-eye location.

**Unmerged stack:** [#42](https://github.com/PIndustries/arkline-engine/pull/42) adds physical personal-fleet berths and boarding; [#44](https://github.com/PIndustries/arkline-engine/pull/44), based on #42, connects the D1-P1 airlock controller. That room has typed breaker/light/vent/door operations, actual versus commanded outputs, an emergency reserve, pressure interlocks and a door obstacle shared with authoritative collision. Its loads reduce the existing reactor recharge budget. Pressure is a bounded single-room scalar model against a fixed corridor reference, not conserved whole-vessel atmosphere. [Room controls](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Room-Controls) and the [equipment catalog](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Equipment-Catalog) describe this development slice, not merged main behavior.

[Room fixture plans](https://github.com/PIndustries/arkline-engine/blob/cb90a340eb2f92497c5136ffddc13414d479f0c9/docs/ships/pioneer/rooms/README.md) · [Systems SVG](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/systems.svg) · [Systems JSON](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/systems.json) · [Proposed vessel engineering contract](https://github.com/PIndustries/arkline-engine/wiki/Vessel-Engineering-Contract)

## Editor and asset work

All PRs in this table were **draft, open and unmerged** when checked. The dependency chain is #42 → #44 → combined #47. The original #52–#56 branches remain open and unchanged; their content is now included through an owner-reviewed append to #47, preserving their separate histories. One combined native build exists, with the observed journeys and remaining acceptance below.

| Work | Implementation checkpoint | What it adds; remaining boundary |
| --- | --- | --- |
| [#47 Combined editor](https://github.com/PIndustries/arkline-engine/pull/47) | `c1170b1` | Embedded local game/authority, typed material graph, scene destinations, host-accepted room geometry and loopback shared sessions. The new bounded spatial hull graph and searchable project GLB selection have native .5 evidence; per-file thumbnails and editable assemblies remain future work. Candidate routes/engineering/material files remain local; they do not install utilities or become shared project files. |
| [#52 Hangar scale](https://github.com/PIndustries/arkline-engine/pull/52) | `c6b057b` | Calibrated CPU projections and tests of actual render-list transforms. Geometry is unchanged; comparison with native rendered pixels remains open. |
| [#53 Material recovery](https://github.com/PIndustries/arkline-engine/pull/53) | `1b681b5` | Bounded file workers and recovery protection. Native protected-folder/recovery acceptance remains open. |
| [#54 Source inspection](https://github.com/PIndustries/arkline-engine/pull/54) | `7a35a6c` | Reproducible source-object/material/frame/bounds manifest. Snapshot IDs are not durable semantic part IDs; this is not a CAD editor or runtime bake. |
| [#55 Cockpit panels](https://github.com/PIndustries/arkline-engine/pull/55) | `f53af26` | Local pressure-gauge/light-switch panels, per-vessel assignment, applied/draft separation and existing typed room commands. It does not build a fitted 3D bridge. |
| [#56 Project GLB import](https://github.com/PIndustries/arkline-engine/pull/56) | `a305b8a` | Project-local verified static GLB bytes, actual-triangle preview, accept/save/reopen and same-ID replacement. One asset slot; no live-world adoption or editable source-part hierarchy. |

The #54 inspection found 524 source meshes, ten material datablocks and no parent hierarchy in the inspected development asset. The runtime GLB merges these into nine material batches. Source-object counts, evaluated source polygons and runtime triangles describe different representations. Importing those batches does not recover construction-part ownership, physical properties or fitted rooms.

## Editable vessel, animations and lights

**Proposed follow-on; no new feature implementation or ownership claim.**

Pioneer's v05 shape pass must preserve an editable ship: identifiable construction parts, meaningful material assignments, attachment points, moving mechanisms and lights tied to actual equipment state. The detailed source should produce the runtime asset used by both the editor and game. That shared source-to-runtime relationship is part of acceptance for the remodeled hull.

The measured current body is **72 × 18 × 38 m in XYZ**, or **72 × 38 × 18 m in length/width/height**, centered on the body origin with +X forward, +Y up and +Z starboard. The inspected `.blend` and current GLB retain those bounds; articulated turrets extend above the hull. v05 guides the silhouette, glazing, shoulders and machinery arrangement. Exact dimensions of those illustrated details remain design choices to author and measure. The existing deck scenes still need a tested body-to-deck transform, clearances and openings. No CAD fit, pressure integrity or structural rating follows from visual similarity.

### What exists and what must change

The saved Blender source contains 524 named meshes and no authored parent hierarchy. The generator saves that source before joining objects by material for export. It also clears the scene when rerun: exporting later user edits needs a separate derived-export path that preserves the edited master. The current GLB has nine root meshes/material batches, no parent-child assembly, animation clips or punctual lights. The runtime decoder evaluates node transforms into vertices; it retains source material indices, not editable parts, joints or pivots. PR54's hash/name IDs identify a source snapshot and change after renaming or a source-byte change. They are not durable design IDs.

The editor can preview verified static project GLBs, and the current hull surface graph binds one explicit embedded-asset hash and material slot. Neither feature supplies a shared editable hull assembly. The surface graph adds spatial procedural material parameters; it does not add imported textures, UVs, normal maps, glass or model animation. Re-exporting a remodeled hull changes its hash and requires an explicit reviewed material-binding update.

Engine-controlled mining turrets already articulate. The connected airlock has real commanded/actual outputs, power demand, pressure interlocks and saved state. Its door currently uses a boolean: a closed box is present or absent, with corresponding fixed collision. It does not yet travel through an authored opening. Room lamps follow actual output and already illuminate nearby surfaces through a bounded emissive-cube heuristic; they lack authored fixture IDs and optical settings. Navigation strobes are procedural presentation-clock geometry, with no authored vessel-wide strobe/sequence system. Imported glTF animation and light extensions remain unsupported by Arkline.

### Dependency order

1. **Keep the validated editor baseline fixed.** PR47 `c1170b1` has passed exact-head CI and the recorded native .5 material/chooser journeys. Preserve its remaining native gates separately, and coordinate the first bounded component slice through [#45](https://github.com/PIndustries/arkline-engine/issues/45) before broad model/runtime changes.
2. **Agree the canonical component contract in [#10](https://github.com/PIndustries/arkline-engine/issues/10).** Give each part a durable ID, parent-relative rest transform, declared body frame, material identity, attachment references and optional constrained motion/light definition. Specify joint axis or pivot, travel limits, animation timing and the existing typed device binding. Light definitions need a fixture/emitter transform, type, color/intensity, range or beam cone, shadow policy and saved sequence. Power and switch bindings reuse actual effective equipment state; damage-dependent behavior needs an authoritative damage contract before it can be claimed. Preserve IDs through rename, reorder, undo and reopen. Keep source hierarchy and part-to-render mapping even when static surfaces are batched. Raw glTF indices and material display names are not durable identities across asset versions. A versioned manifest maps these stable IDs to each export’s nodes, primitives and material indices. Editor and runtime share the same schema; render, picking, shadows, collision and controls resolve the same accepted asset/component revision. Preserve local vertices and explicit rest-body coordinates so moving a part does not silently change its material coordinate semantics. Unsupported replacement retains the previous valid revision.
3. **Prove one working assembly in [#43](https://github.com/PIndustries/arkline-engine/issues/43).** Extend the existing D1-P1 airlock proposal with one horizontally sliding authored door and one powered sequenced warning light. Reuse its room/equipment IDs and pressure, ownership and occupied-door protections. Author the door frame, leaf, travel pocket, lamp and attachment coordinates. A ramp follows later: current walking collision ignores Y, and exterior/deck registration is unfinished. This slice does not require a general glTF clip player.
4. **Carry that contract into the v05 model pass and [#41](https://github.com/PIndustries/arkline-engine/issues/41) visual review.** Establish the whole silhouette and bridge/shoulder forms before small detail. Preserve moving assemblies and their clearance envelopes while deriving bounded render batches. Review source-to-render mappings and material assignments with each export. Expand lights, landing gear, hatches, crane and other mechanisms only after their individual state and collision contracts exist.

### First moving-door and light acceptance

The proposed door needs authoritative motion state, a confirmed travel fraction and declared timing; the existing open boolean cannot establish intermediate geometry. Rendering, picking and collision must use that same pose and account for the swept opening. Pressure rejection keeps the door closed; occupancy prevents unsafe closing. Keep traversal conservatively blocked until the moving leaf is fully clear, and test its swept path rather than removing today’s collider at animation start. Define interruption, manual operation and power-loss behavior before implementation. Travel speed and any motor load are explicit gameplay assumptions until supported by engineering inputs.

The warning light needs a saved pattern, device binding and phase derived from simulation time. Verify its actual illumination and declared shadow/occlusion behavior; an emissive mesh alone does not establish a beam or a working shadow. Its visible emitter and bounded local-light contribution must follow effective power and confirmed state. Breaker loss must disable the normal output; emergency behavior remains separately defined by the existing reserve. A glowing material or wall-clock blink is not proof of powered equipment. Choose and label pattern periods, intensities and load policy, accounting for transition intervals if load changes with sequence phase. The existing 24 W lamp tuning is a gameplay assumption, not a measured fixture rating.

The first acceptance journey selects the actual door/lamp, edits an allowed parameter, applies a validated revision and operates the existing typed control. It observes intermediate door poses, interlocks, actual light sequence and power loss/restore. Pause freezes motion and sequence phase; fixed stepping advances them deterministically. Save/reopen and authority restart retain component IDs, accepted geometry, bindings and the declared state/phase policy without replaying commands. Stale applies, failed saves and incompatible clients must leave an explicit safe/last-valid result. Record automated authority/collision tests, actual GPU captures and packaged native interaction separately. Front/top/side views and human-scale references still prove only their measured clearances, not a fitted whole ship.

This extends the [authored-airlock proposal](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6183327461) and [v05 modeling plan](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6183334728). Full CAD solids, a structural solver, conserved vessel-wide utilities and unrestricted animation remain separate work.

Ownership for the next claim: [#10](https://github.com/PIndustries/arkline-engine/issues/10) owns the component/export/decoder contract; [#43](https://github.com/PIndustries/arkline-engine/issues/43) owns authoritative door/light state and collision; [#41](https://github.com/PIndustries/arkline-engine/issues/41) owns renderer/fixture quality; [#45](https://github.com/PIndustries/arkline-engine/issues/45) owns the integrated editor journey, with [#36](https://github.com/PIndustries/arkline-engine/issues/36) measuring costs. One integrator must own the shared schema and hooks. The retained browser/decoder handoff covers only its named scope; hierarchy, motion and light support require a new bounded claim and coordination before source edits. No implementation worker is dispatched by this plan.

Source basis: [export generator](https://github.com/PIndustries/arkline-engine/blob/c1170b1/tools/art/build_pioneer.py), [source registration](https://github.com/PIndustries/arkline-engine/blob/c1170b1/docs/ships/pioneer-assembly-registration.md), [static decoder](https://github.com/PIndustries/arkline-engine/blob/c1170b1/apps/client/src/ship_asset.rs), [room rendering](https://github.com/PIndustries/arkline-engine/blob/c1170b1/apps/client/src/pioneer_interior.rs) and [authority](https://github.com/PIndustries/arkline-engine/blob/c1170b1/apps/server/src/room_controls.rs). The [glTF node/animation specification](https://github.com/KhronosGroup/glTF/blob/main/specification/2.0/Specification.adoc) and [punctual-light extension](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_lights_punctual/README.md) provide interchange conventions; Arkline requires explicit bounded support and an editable source master before using those features.


## Evidence and acceptance

- **Merged foundations:** #29 records a full application-input journey through all twelve rooms/lifts/airlock/boarding/undocking plus server and native GPU fixtures. Its partial native Mac smoke is not a complete OS held-key walkthrough. #39 records geometry review, native GPU capture and native/WASM static checks; source merge does not prove a deployed build.
- **Development Mac evidence:** #44 records an isolated room commissioning → breaker loss/emergency → restore → exhaust/interlock → equalization/safe opening → exit/reboard journey. #47 records package **26.10.14.49 / `196ef4f`** material save/reload, two native editors sharing clock and accepted room geometry, and Host-to-Local behavior. The later **.56 / `3827de9`** package and green CI do not repeat those native journeys.
- **Later contributions:** #52–#56 link successful published-head CI and separate focused receipts. #54's 30 local Python/Blender checks include four real-source Blender tests; existing CI does not run that opt-in Blender extraction. CPU scale evidence does not resolve the reported native visual scale defect.
- **Linux visual receipts:** [cockpit recheck](https://github.com/PIndustries/arkline-engine/pull/55#issuecomment-6181539800) and [importer receipt](https://github.com/PIndustries/arkline-engine/pull/56#issuecomment-6182883524) establish their stated visible Linux journeys, locally composed with a separate startup patch. They do not establish native Mac acceptance or a published combined build. The importer also retains a [stale candidate-status message finding](https://github.com/PIndustries/arkline-engine/pull/56#issuecomment-6182670358).

- **Initial combined local Mac checkpoint:** the [validation receipt](https://github.com/PIndustries/arkline-engine/issues/45#issuecomment-6184733715) records isolated `d86a9bc` combining #47 code with #52–#56: **590 passed / 0 failed / 20 ignored** on test build **26.10.14.59**, strict desktop/browser checks and an optimized ad-hoc signed Mac package **.60**. At that initial checkpoint native UI journeys and calibrated hangar pixels were pending; the linked receipt now also records its later cockpit operation and paused quit/reopen observations. The four opt-in real-source Blender checks are blocked before Blender starts by Mac `RLIMIT_AS` setup; 26 non-Blender checks pass. That initial local composition had no published exact-head CI; the later carrier below does.

- **Published combined material/browser candidate:** [PR47 `c1170b1`](https://github.com/PIndustries/arkline-engine/commit/c1170b11e5f703be54a4f753026387a60e4c2a2e) passed [macOS/Linux/browser CI](https://github.com/PIndustries/arkline-engine/actions/runs/38698656233): each native job passed640 tests with zero failures (22 Mac/17 Linux ignores). Signed build **26.10.15.5**, implementation110cd06, has actual graph/Pioneer preview, uniform-only seed edits, Undo/Redo, Save/Reload, both lighting modes, invalid-graph last-good recovery, project GLB search/select, native chooser cancellation and same-ID revision replacement/Save observations. The c117 follow-up changes only a test helper platform gate. [Current receipt](https://github.com/PIndustries/arkline-engine/issues/36#issuecomment-6186664159) keeps full application reopen, native file drop, actual WebGL2 execution and profiling open; user activity in the new running world was preserved.

No feature is marked complete from a screenshot, test count or green workflow alone. Record future combined-native results with exact source composition, build version, disposable fixture, observed journey and remaining failures; keep published-head CI separate from local integration tests.

## Remaining gaps

The immediate goal is one usable combined native build and a reconciled review stack. Merge and release decisions remain pending. Model and assembly work below is deferred until that consolidation is accepted.

1. Finish native acceptance for material recovery, cockpit/import workflows, lifecycle/restart/reconnect and changed-room walking. Broader multiplayer interiors, physical cargo handling, operable exterior doors, fault/repair and conserved power/fluids/heat remain open design and implementation work.
2. Rework the large hull forms against v05, then verify front/top/side and three-quarter engine captures with metric bounds and the dynamic turrets present. Detailed textures/glass/LOD support and measured performance need their own evidence.
3. Register exterior, bridge/cockpit camera, deck plans, openings, collision and mounts in one tested frame. No fitted bridge, continuous external hatch traversal, pressure hull, flight interior or full-ship CAD fit is claimed.
4. Preserve durable part/assembly IDs and explicit parent/source-to-render transforms before editable pieces join the engine. Exporting all 524 meshes individually would exceed the current 128-primitive/mesh decoder ceiling.
5. The [authored airlock-section proposal](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6183327461) is deferred next feature work: identified fixtures, constrained placement, one installed lamp-feed connection, accepted authority revision and save/reopen. It must reuse the existing equipment IDs/interlocks and distinguish installed state from draft topology. No implementation has started under that proposal.

Tracking: [model/asset pipeline #10](https://github.com/PIndustries/arkline-engine/issues/10), [interiors #23](https://github.com/PIndustries/arkline-engine/issues/23), [vessel engineering #32](https://github.com/PIndustries/arkline-engine/issues/32), [hangar #40](https://github.com/PIndustries/arkline-engine/issues/40), [visual quality #41](https://github.com/PIndustries/arkline-engine/issues/41), [connected equipment #43](https://github.com/PIndustries/arkline-engine/issues/43), [editor #45](https://github.com/PIndustries/arkline-engine/issues/45), [room authoring #50](https://github.com/PIndustries/arkline-engine/issues/50), [shared development sessions #51](https://github.com/PIndustries/arkline-engine/issues/51). These issues cover broader work than the current slices; this record does not close them.

## Milestone log

Dates are UTC. A merged source milestone is not a release receipt.

| Date | Milestone and evidence | State |
| --- | --- | --- |
| 2026-10-11 | [#24](https://github.com/PIndustries/arkline-engine/pull/24), [#25](https://github.com/PIndustries/arkline-engine/pull/25), [#28](https://github.com/PIndustries/arkline-engine/pull/28): preserve fleet concepts, numerical reservations and room-service plans. | Merged documentation; v05 remains a visual proposal. |
| 2026-10-11 | [#27](https://github.com/PIndustries/arkline-engine/pull/27), [#33](https://github.com/PIndustries/arkline-engine/pull/33): layout viewer, industrial fleet geometry and enclosed cutter silhouette. | Merged; Pioneer procedural exterior later superseded by #39. |
| 2026-10-11 | [#29](https://github.com/PIndustries/arkline-engine/pull/29), [#34](https://github.com/PIndustries/arkline-engine/pull/34): two docked decks/lift and mixed-client recovery. | Merged; separate-scene interior and incomplete native/browser acceptance retained. |
| 2026-10-11 | [#30](https://github.com/PIndustries/arkline-engine/pull/30): repeated mining deliveries and recovery; [local protocol receipt](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/docs/mining-loop-validation.md) records 1,050 manual ore, 1,200 AI ore and 138 verified events. | Merged gameplay acceptance; not visual or load acceptance. |
| 2026-10-12 | [#39](https://github.com/PIndustries/arkline-engine/pull/39): editable Blender hull and embedded GLB runtime. | Merged at `78fdc5d`; final concept fidelity and registration remain open. |
| 2026-10-12–15 | [#42](https://github.com/PIndustries/arkline-engine/pull/42), [#44](https://github.com/PIndustries/arkline-engine/pull/44), [#47](https://github.com/PIndustries/arkline-engine/pull/47): fleet dock, connected airlock and editor/shared-room work. | Draft stack, unmerged. Build-specific native evidence above. |
| 2026-10-14–15 | [#52](https://github.com/PIndustries/arkline-engine/pull/52)–[#56](https://github.com/PIndustries/arkline-engine/pull/56): scale evidence, material recovery, source inspection, cockpit panels and project imports. | Separate draft PRs; review and combined native acceptance pending. |
| 2026-10-15 | [Hull modeling plan](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6183334728) and [authored airlock plan](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6183327461); lifecycle record consolidated. | Proposals deferred while the existing stack is reconciled. |
| 2026-10-15 | Combined PR47 `c1170b1`, signed native26.10.15.5 and exact-head CI; editable-parts/animated-door/powered-sequence dependency plan recorded. | Candidate validated at its stated scope; full native gates and the proposed moving assembly remain open. |

## Historical exterior capture

This actual renderer capture from [#33](https://github.com/PIndustries/arkline-engine/pull/33) preserves the procedural cutter step before the authored GLB. It is historical evidence, not the current hull or concept art.

![Pioneer historical procedural exterior](https://raw.githubusercontent.com/PIndustries/arkline-engine/80eafbb/docs/ships/pioneer/images/engine-surface-preview.jpg)

## Concept history

Superseded images are retained for review. Early drafts include rejected pale hulls, drill-like equipment, invented brand marks, misleading checkboxes or inconsistent dimensions. Their annotations are not authoritative. The accepted direction is black/emerald industrial construction styling with articulated laser mining turrets.

### v00-initial-study

![Pioneer v00-initial-study](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v00-initial-study.jpg)

### v01-drill-draft-superseded

![Pioneer v01-drill-draft-superseded](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v01-drill-draft-superseded.jpg)

### v02-laser-correction

![Pioneer v02-laser-correction](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v02-laser-correction.jpg)

### v03-black-green-construction

![Pioneer v03-black-green-construction](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v03-black-green-construction.jpg)

### v04-interior-draft

![Pioneer v04-interior-draft](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v04-interior-draft.jpg)


## Provenance and upkeep

The concept JPEGs are compressed documentation previews; original PNG hashes and preview hashes are recorded in the [image manifest](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/image-manifest.json). The original concept archive supplies no executable game assets. The Blender/GLB assets were added separately by #39 and refined in the unmerged #44 stack.

Keep this as Pioneer's lifecycle index. Append dated decisions and evidence, retain superseded designs, and update each work item's implementation/review/native/release state explicitly. The [repository Markdown workflow](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/README.md) keeps the versioned source reviewable, then synchronizes reviewed content to this wiki; avoid a second independent narrative.
