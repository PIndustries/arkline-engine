# Pioneer — life of the ship

Pioneer is Arkline's 72 x 38 x 18 m industrial starter cutter. It has two mining turrets and two docked interior decks. The v05 interior review remains its visual design reference; earlier drawings are kept below as design history.

As of 15 October 2026, merged source includes the authored Blender/GLB hull, walkable decks, flight, mining, drones, cargo and scalar power systems. The unmerged development branch uses the Study05 hull, connected airlock equipment, powered exterior fixtures and native editor tools. Study05 refines the bow, glazing and service areas; overall v05 fidelity and exterior/interior alignment remain unfinished.



[Design direction](#design-direction) · [3D model history](#dated-3d-model-history) · [Dimensions and frames](#dimensions-and-frames) · [Current runtime](#current-runtime) · [Editor and asset work](#editor-and-asset-work) · [Editable vessel](#editable-vessel-animations-and-lights) · [Evidence](#evidence-and-acceptance) · [Gaps](#remaining-gaps) · [Milestones](#milestone-log) · [Concept history](#concept-history)

[Versioned page source](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/README.md) · [Fleet archive](https://github.com/PIndustries/arkline-engine/wiki/Ship-Design-Archive) · [Project 14](https://github.com/orgs/PIndustries/projects/14)

## Design direction

![Pioneer current v05 concept](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v05-interior-review.jpg)

v05 calls for a broad, enclosed black/emerald cutter: faceted bow, integrated shoulders, heavy collars, a stepped underside and articulated laser mining turrets. The separated outriggers in v03 and earlier drill-like equipment are historical. Generated labels, checkboxes and painted proportions are not acceptance evidence.

The merged [authored model](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/README.md) is the first exterior foundation: 32,284 runtime triangles in seven material batches, with separate engine-controlled turrets. Draft [#44](https://github.com/PIndustries/arkline-engine/pull/44) adds an industrial detail pass and changes the GLB to 65,866 triangles in nine batches while retaining the body envelope and gameplay hardpoints. Neither version is a finished reconstruction of v05.

The earlier industrial baseline had a narrow center, projecting square nose and long side blocks. Studies 01–04 broadened and enclosed the hull, moved glazing forward and integrated the shoulders. Study05 now corrects the bow/glazing, roof architecture and actual drive openings in the engine. Broader side/collar, underside and overall proportion/detail matching remain deliberate v05 work. See the [modeling plan](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6183334728), [Study05 development record](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6199911507) and open checklist below.


## Study05 · inhabited and manufactured scale · 2026-10-15

The two-area pass follows the published v05 architecture: a more compact **bow/bridge**, and purposeful **service/work architecture**, including the existing drive exits. Thick separated armor panels, connected braces, continuous gaskets, actual pane recesses, fixed service covers/handles and human-scale rails provide construction cues. The roof now has distinct access, exchanger and folded handling areas. The arm, hatches and service detail remain static inferred authoring choices; no operating equipment or fitted crew interior is claimed.

| Study04 before · actual7036 | Study05 refined · actual7040 |
| --- | --- |
| ![Before same body camera](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/before/pioneer-authority-idle-front-quarter.png) | ![Refined Study05 same body camera](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front-quarter.png) |

| Bridge before | Bridge refined |
| --- | --- |
| ![Before white planar bridge output](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/before/pioneer-authority-idle-bridge-quarter.png) | ![Refined recessed bridge and armor joints](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-bridge-quarter.png) |

| Recessed work area | Existing drive in a real opening |
| --- | --- |
| ![Raised paired handling links in recessed service area](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-aft-roof.png) | ![Actual recessed existing bell throat and seated rim](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-port-nozzle.png) |

[View the model comparisons](#matched-view-gallery).

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#study05--inhabited-and-manufactured-scale--2026-10-15).


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


## Remaining gaps

Overall v05 shape/material completion, moving doors and broader component behavior remain unfinished.

1. Finish native acceptance for material recovery, cockpit/import workflows, lifecycle/restart/reconnect and changed-room walking. Broader multiplayer interiors, physical cargo handling, operable exterior doors, fault/repair and conserved power/fluids/heat remain open design and implementation work.
2. Rework the large hull forms against v05, then verify front/top/side and three-quarter engine captures with metric bounds and the dynamic turrets present. Detailed textures/glass/LOD support and measured performance need their own evidence.
3. Register exterior, bridge/cockpit camera, deck plans, openings, collision and mounts in one tested frame. No fitted bridge, continuous external hatch traversal, pressure hull, flight interior or full-ship CAD fit is claimed.
4. Preserve durable part/assembly IDs and explicit parent/source-to-render transforms before editable pieces join the engine. Exporting all 686 current source meshes individually would exceed the current 128-primitive/mesh decoder ceiling.
5. The [authored airlock-section proposal](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6183327461) is deferred next feature work: identified fixtures, constrained placement, one installed lamp-feed connection, accepted authority revision and save/reopen. It must reuse the existing equipment IDs/interlocks and distinguish installed state from draft topology. No implementation has started under that proposal.

Tracking: [model/asset pipeline #10](https://github.com/PIndustries/arkline-engine/issues/10), [interiors #23](https://github.com/PIndustries/arkline-engine/issues/23), [vessel engineering #32](https://github.com/PIndustries/arkline-engine/issues/32), [hangar #40](https://github.com/PIndustries/arkline-engine/issues/40), [visual quality #41](https://github.com/PIndustries/arkline-engine/issues/41), [connected equipment #43](https://github.com/PIndustries/arkline-engine/issues/43), [editor #45](https://github.com/PIndustries/arkline-engine/issues/45), [room authoring #50](https://github.com/PIndustries/arkline-engine/issues/50), [shared development sessions #51](https://github.com/PIndustries/arkline-engine/issues/51). These issues cover broader work than the current slices; this record does not close them.


## Dated 3D model history

Each row preserves the actual editable Blender master and its GLB, rather than a generated illustration. Download links pin full Git commits; later replacements at the stable study paths retain every earlier version in Git history. All listed hulls use the same centered **72 × 18 × 38 m XYZ** envelope. Moving gameplay turrets are separate and omitted from the static Blender study images; they appear in the later actual-engine captures. None of these rows establishes a fitted interior or a released build.

| Date (UTC) / revision | Source and downloads | Change and evidence status |
| --- | --- | --- |
| 2026-10-12 · First authored hull | [Merged #39](https://github.com/PIndustries/arkline-engine/pull/39), [`78fdc5d`](https://github.com/PIndustries/arkline-engine/commit/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/pioneer.blend) (227,320 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/pioneer.glb) (1,689,712 B). | 32,284 triangles / seven batches; first embedded GLB foundation. Installed in merged source; release not asserted. |
| 2026-10-12–15 · Industrial baseline | Draft [#44](https://github.com/PIndustries/arkline-engine/pull/44), combined [#47](https://github.com/PIndustries/arkline-engine/pull/47), [`3e41520`](https://github.com/PIndustries/arkline-engine/commit/3e415204747cd9adaa146a9777831a7acc82e7cb); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e415204747cd9adaa146a9777831a7acc82e7cb/assets/ships/pioneer/pioneer.blend) (353,015 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e415204747cd9adaa146a9777831a7acc82e7cb/assets/ships/pioneer/pioneer.glb) (6,736,012 B). | 65,866 triangles / nine batches. Actual packaged .5 game hull at this checkpoint; draft stack remains unmerged. |
| 2026-10-15 · Study 01 | [`e4a80b2`](https://github.com/PIndustries/arkline-engine/commit/e4a80b299dd7258efb021a4da6e0b367ce18ded8) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/pioneer-study.blend) (342,348 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/pioneer-study.glb) (6,531,236 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/study.json). | Widened connected pressure hull and frontal glazing; shoulder overlay intersections remain. **Superseded study; no engine adoption.** |
| 2026-10-15 · Study 02 | [`f099e43`](https://github.com/PIndustries/arkline-engine/commit/f099e4372908530393e374710e685018462e3cc4) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/pioneer-study.blend) (405,669 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/pioneer-study.glb) (6,917,496 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/study.json). | Closed armor volumes and recessed service deck; old gray covers hide much of the collars. **Superseded study; native .5 decoder/staging passed, no game adoption.** |
| 2026-10-15 · Study 03 | [`ebf6fa2`](https://github.com/PIndustries/arkline-engine/commit/ebf6fa2746bd01bd3e27be93810a19de0091fb16) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/pioneer-study.blend) (511,253 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/pioneer-study.glb) (7,756,576 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/study.json). | Exterior green pod collars, connected corner glass and restored upper RCS surface; crossing corner fan rejected. **Superseded after actual visual review; no engine adoption.** |
| 2026-10-15 · Study 04 | [`30bd538`](https://github.com/PIndustries/arkline-engine/commit/30bd5381ea3c2ef553f4235a5a0c026184dada18) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/pioneer-study.blend) (497,325 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/pioneer-study.glb) (7,664,728 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/study.json). | Clean closed brow facets replace the crossing fan; green collars and recessed deck retained. **Reviewed development candidate; cloud 7102 decoder/staging passed, no game adoption.** |
| 2026-10-15 · Cloud saved-master export and engine adoption | [`7747079`](https://github.com/PIndustries/arkline-engine/commit/7747079c7f43017928fed41dc2699c76e7469cc0); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer.blend) (497,325 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer.glb) (6,582,300 B); [export receipt](https://github.com/PIndustries/arkline-engine/blob/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer-export.json), [adoption record](https://github.com/PIndustries/arkline-engine/blob/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer-adoption.json). | Unchanged Study 04 source `772bf76a…`; fresh normal-preserving `ed5e29cd…` export, 78,244 triangles / nine batches. **Installed in cloud native7031 and actual engine7032 with powered exterior and existing turrets. Intermediate fidelity; unmerged.** |
| 2026-10-15 · Study05 first engine preview | [`a3928f2`](https://github.com/PIndustries/arkline-engine/commit/a3928f23eefc171c12830e496fbd60ccf773dea7); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/a3928f23eefc171c12830e496fbd60ccf773dea7/assets/ships/pioneer/studies/study05/pioneer-study05.blend), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/a3928f23eefc171c12830e496fbd60ccf773dea7/assets/ships/pioneer/studies/study05/pioneer-study05.glb); [actual 7038 views](https://github.com/PIndustries/arkline-engine/tree/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/first). | Source 9cc3ac5d… / runtime df8490f4…, 670 parts, 98,858 triangles. Recessed bridge/workdeck and real aft openings; actual review found floating braces, weak arm silhouette and coplanar rim overlap. **Superseded by the refinement below; preserved.** |
| 2026-10-15 · Study05 refined | [Editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/pioneer-study05.blend) (592,067 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/pioneer-study05.glb) (8,700,948 B); [source-authoring record](https://github.com/PIndustries/arkline-engine/blob/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/study05.json), [export receipt](https://github.com/PIndustries/arkline-engine/blob/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/export.json), [actual 7040 views](https://github.com/PIndustries/arkline-engine/tree/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined). | Source 61eb70e1… / runtime 8125644f…, 686 parts, 103,466 triangles, nine primitives. Thick separated brow plates, connected gaskets/braces, raised paired handling links and seated nozzle rims. **Selected bow/service scope visually reviewed in actual engine; overall v05 unfinished.** |

The original baseline source/export SHA-256 values are `8ec754fc7afe6b67dcab6cbd6694bb04bdc03cc12e91ee96e0bca9e08bb24f6f` / `f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402`. Study receipts record each later master/export hash and its checks. The frozen Study 04 export has 543 named source meshes before derived batching and 78,246 runtime-format triangles in nine batches. The historical Study04 cloud export from that same master has 78,244 retained triangles after the explicit engine float32 degeneracy policy; the frozen file is preserved. Its independent static checks preserve both turret supports and all six RCS intersections, with upper hit approximately Y8.3 m. Native staging uses the real importer; static checks and Blender renders remain separate from GPU acceptance.

### Matched view gallery

The baseline and Study01–04 rows are source renders with the same cameras, lighting, orthographic scale and resolution. The cloud-engine rows uses the separately recorded body-relative cameras and actual sunlight; it is not the same render setup. The supplied concept has a different perspective and provides visual direction, not measured CAD. Select each linked view for its full image.

| Revision | Matched views |
| --- | --- |
| Before remodel | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/quarter.png) |
| Study 01 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 02 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 03 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 04 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/quarter.png) |
| Cloud installed · engine7032 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png) · [rear](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-rear.png) · [bottom](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-bottom.png) |
| Study05 refined · engine7040 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front-quarter.png) · [rear](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-rear.png) · [bridge close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-bridge-quarter.png) · [roof close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-aft-roof.png) · [nozzle close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-port-nozzle.png) |

| Before remodel | Latest reviewed study 04 |
| --- | --- |
| ![Before Pioneer remodel](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/quarter.png) | ![Pioneer study04 actual editable source](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/quarter.png) |

| Superseded study 01 | Superseded study 02 |
| --- | --- |
| ![Pioneer study01](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/quarter.png) | ![Pioneer study02](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/quarter.png) |

[Download supplied full-resolution reference](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/supplied-reference.png) · [Baseline capture provenance](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/capture.json). The seven earlier concept/runtime images remain below. This page supplies rendered views and actual model downloads; no embedded interactive 3D viewer is supplied.

Study04 remains preserved as a historical intermediate. Study05 changes the actual source geometry and the declared Window presentation parameters, while retaining the existing systems/hardpoints. Prior-hash graph and accepted-world mismatch protections remain active; no automatic migration is supplied.

The visual acceptance checklist remains open. Study05 resolves its bounded bow/service architecture corrections; remaining whole-ship work is tracked separately:

| Area | Remaining visible gap |
| --- | --- |
| Bow / bridge | Study05 has a shallower chin, three thick faceted brow plates, actual recesses/gaskets and connected frames. Opaque scalar-PBR glazing remains an approximation; no fitted bridge is shown. |
| Roof / work area | Study05 exposes recessed access/exchanger/handling areas with a fixed folded arm and reels. The arm is less prominent than the raised reference crane, and no operation is implemented. |
| Underside | A mostly continuous lower beam; stepped armor, supports and service volumes need actual bottom/side review. |
| Side bays / collars | Equal repeated panels and broad green slabs; the concept has more varied depths, layered structure and fittings. |
| Aft drives | Study05 opens the actual caps and seats flanges in counterbores around existing bells/throats. Deeper coincident graphite floors remain behind the flanges; no overlap-free CAD or propulsion claim. |

Existing dynamic mining turrets belong to the engine and appear in the new engine comparison; the static study images do not justify adding duplicate turrets to the hull. Fixture colors, wattages and pulse timing remain gameplay assumptions. Model adoption is an intermediate implementation checkpoint, not completion of this checklist.


## Earlier combined Study04 checkpoint · 2026-10-15

![Installed Pioneer in the actual engine](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png)

| Exterior circuit OFF · drive IDLE | Exterior ON · drive IDLE |
| --- | --- |
| ![Exterior OFF at the same paused time](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-exterior-off.png) | ![Exterior ON at the same paused time](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png) |

| Dorsal strobe ON | After real +50 ms · still ON | After real +100 ms · OFF |
| --- | --- | --- |
| ![Actual dorsal strobe ON close view](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-on.png) | ![Actual dorsal strobe at plus50ms](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-step-50ms.png) | ![Actual dorsal strobe OFF close view](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-off.png) |

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#earlier-combined-study04-checkpoint--2026-10-15).


## Earlier powered exterior checkpoint · industrial baseline · 2026-10-15

| Exterior OFF / drive OFF | Exterior ON / drive IDLE | Exterior ON / drive ACTIVE |
| --- | --- | --- |
| ![Actual engine exterior OFF](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-off.png) | ![Actual engine exterior IDLE](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-idle.png) | ![Actual engine exterior ACTIVE](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-active.png) |

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#earlier-powered-exterior-checkpoint--industrial-baseline--2026-10-15).


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



## Historical exterior capture

This actual renderer capture from [#33](https://github.com/PIndustries/arkline-engine/pull/33) preserves the procedural cutter step before the authored GLB. It is historical evidence, not the current hull or concept art.

![Pioneer historical procedural exterior](https://raw.githubusercontent.com/PIndustries/arkline-engine/80eafbb/docs/ships/pioneer/images/engine-surface-preview.jpg)


## Technical references

### Source-to-model and live-system checklist

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#source-to-model-and-live-system-checklist).


### Editor and asset work

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#editor-and-asset-work).


### Editable vessel, animations and lights

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#editable-vessel-animations-and-lights).

#### What exists and what must change

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#what-exists-and-what-must-change).

#### Dependency order

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#dependency-order).

#### First moving-door and light acceptance

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#first-moving-door-and-light-acceptance).


### Evidence and acceptance

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#evidence-and-acceptance).


### Milestone log

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#milestone-log).


## Provenance and upkeep

The concept JPEGs are compressed documentation previews; original PNG hashes and preview hashes are recorded in the [image manifest](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/image-manifest.json). The original concept archive supplies no executable game assets. The Blender/GLB assets were added separately by #39 and refined in the unmerged #44 stack.

Keep this as Pioneer's lifecycle index. Append dated decisions and evidence, retain superseded designs, and update each work item's implementation/review/native/release state explicitly. The [repository Markdown workflow](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/README.md) keeps the versioned source reviewable, then synchronizes reviewed content to this wiki; avoid a second independent narrative.
