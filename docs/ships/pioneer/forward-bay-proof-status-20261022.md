# Isolated authored forward-bay proof: technical evidence, visual work in progress

The scope is one original window frame, recessed central instrument and adjoining sculpted side console. No live cockpit, world, avatar or package was changed in this pass.

The previous geometry-authoring rejection was resolved on the same worker's one reviewed retry with new explicit user authorization on 2026-10-22 at 16:50–16:51 UTC. The normal reviewed action succeeded; no alternate writer or route was used. Existing separate material, lighting and cache denials remain untouched.

Checkpoint `d0d793d` contains a reproducible standard-library authoring script and original GLB/manifest. The second geometry revision has nine meshes, 2,588 triangles and 225,124 bytes. Its exact SHA256 is `8ca7618e6a8b60897a675929fccc4bb8dc20425da47ac9566967fd4e73a52646`. It is an isolated study, not production registration.

## Independent preparation saved

`apps/client/src/pioneer_mesh_proof_gpu_tests.rs` is a macOS test-only imported-asset fixture. It requires an explicit original GLB, instrument manifest, recorded telemetry file and new evidence directory. It uses the existing strict mesh importer, imported-asset preview renderer, cockpit surface renderer and the exact imported triangles for pointer occlusion. It does not open EditorWorld, dispatch commands or register geometry into a live world. Preview lighting is not the live cockpit lighting pipeline.

The proposed instrument canvas is 400 by 240, covering approximately 0.60 by 0.35 metres. Primary values are confirmed location, capacitor energy in kJ and ore in existing legacy units (`u`); no conversion to invented physical mass is implied. Captures must say RECORDED / NOT LIVE. The recorded snapshot used for preparation has simulation time 1792575627512, docked location, capacitor 240000 J and ore 0. It is not a live binding.

## Evidence and criteria

At a fixed 1500 by 900 viewport and 60-degree vertical field of view, the actual CockpitSurface projection preflight measured:

| Proposed view | Minimum label em | Minimum value em | Proposed 14 / 24 px floor |
| --- | ---: | ---: | --- |
| Seated | 32.25 px | 56.51 px | Pass |
| Standing | 14.84 px | 25.95 px | Pass |

Focused check: `proposed_forward_bay_font_budget`, guarded build log `.build/logs/20261022T064623262887Z.log`. An earlier smaller-font analytical proposal failed the standing target and was retained separately at `/tmp/pioneer-forward-bay-proposed-readability-20261022.json`. The revised measurement is a plane projection only; it does not certify glyph readability, text fit, physical enclosure or an authored model.

The native fixture ran successfully against that mesh. Exact GLB hash, byte length, named node/mesh identities, decoded component bounds and counts matched the manifest. All five sampled display points (four corners and center) were visible and round-tripped through the existing mesh-aware pointer mapping in both matched views. Seventeen sampled upright capsule poses cleared conservative per-component AABBs; a console-intersecting control pose was rejected. This is a clearance proxy, not exact triangle collision or full traversal admission.

Actual engine images and receipt: `/tmp/pioneer-authored-forward-bay-proof-20261022-identity/`. The receipt binds the exact geometry, manifest, recorded data and image hashes. Guarded native test log: `.build/logs/20261022T165812797938Z.log`. Library image transfer failed with `hosted apps tools/list request failed: DNS`; no Library attachment was created.

First visual review rejected the isolated-monitor-on-stalk silhouette. The second revision replaces that with one continuous curved dashboard, but the central housing remains heavy and imported-preview material separation is flat. Technical importer/picking success is not a visual acceptance pass. A further refinement is limited to this same section; no whole-ship expansion or live replacement is warranted yet.

Required next proof criteria remain unverified:

- Clearly different coherent frame/console silhouette in matched seated and standing views, with bevels, authored normals and material separation.
- Readable actual glyphs at delivered size, no label/value overlap or bezel clipping.
- Open forward view and clear instrument aperture; matching rendered and picked geometry.
- Production canonical component registration and collision geometry. Study identities and conservative clearance samples are verified, but the asset has no floor, chair, doorway or support geometry.
- Working command mapping and production integration, which are outside a static recorded-value preview.

The fixture records visibility and pick results rather than declaring the whole proof passed. There are no command controls in this recorded-value instrument, so no server command dispatch or acknowledgement is claimed. Broader renderer/material/cache denials remain untouched. Do not launch or replace the user cockpit on the strength of this isolated study.

## Third revision: retained matched views

The final revision of this bounded pass has 16 meshes, 3,256 triangles and 286,684 bytes, SHA256 `4196f82270e1b8fd773cd84d94c62d9792dc6eb4e67f7cbe88abbe8506e00b13`. It slims the case border, sweeps its rear shoulder into the dashboard, and adds an original passive access cover and four fasteners. These are construction details, not simulated controls.

The same compiled native fixture ran against this exact revision: `.build/logs/20261022T170211703308Z.log`. Captures and hash-bound receipt are retained at `/tmp/pioneer-authored-forward-bay-proof-20261022-v3/`. No broad suite was rerun. Both views retain the same 14/24 px projected font floor and all five pointer samples clear. Identity and conservative clearance checks pass.

| Proposed proof criterion | Outcome |
| --- | --- |
| Original triangle mesh, authored normals and stable study identities | Pass |
| Actual-size primary recorded values, no visible overlap in matched views | Pass for these two fixed views; no dynamic-length coverage |
| Recessed instrument and adjoining connected body | Basic construction pass; shape still needs refinement |
| Pointer visibility and projection/unprojection agreement | Pass at the five sampled points in each view |
| Conservative rear pose/path clearance, blocked control pose | Pass; component-envelope proxy only |
| Concept-quality materials, enclosure and detailed silhouette | Not achieved; import-preview lighting is limited and section remains sparse |
| Exact mesh collision, grounded circulation, live control commands | Not implemented by this isolated proof |
| Production world registration and live build handoff | Not attempted |

Decision: continue authored-mesh development as a viable technical path, but do not expand this section into the whole ship or replace the live cockpit as an accepted visual result. Next evidence needed is a better resolved section under the existing production material/light path, plus real command integration and canonical collision/support planning. Any operation overlapping a separately denied renderer/material feature remains stopped; this study is not authorization to route around it.

No user database was opened for writing, no avatar/job/ledger was changed, and no app launch/restart was performed. The current supported app inventory did not list Arkline; the old running-version observation was not treated as current evidence.
