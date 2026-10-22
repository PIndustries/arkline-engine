# Isolated authored forward-bay proof: preparation, not visual acceptance

The proposed scope is one original window frame, recessed central instrument and adjoining sculpted side console. No live cockpit, world, avatar or package was changed in this pass. The model and authoring script were not created because automatic review rejected the geometry worker operation, including its one retry with the explicit implementation delegation. No approval request ID was returned. The rejected action was stopped without another writer or equivalent implementation route.

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

The native preview fixture compiles but has not run because its required authored asset does not exist. No new runtime image is available. Required next proof criteria remain unverified:

- Clearly different coherent frame/console silhouette in matched seated and standing views, with bevels, authored normals and material separation.
- Readable actual glyphs at delivered size, no label/value overlap or bezel clipping.
- Open forward view and clear instrument aperture; matching rendered and picked geometry.
- Stable canonical component identities, fixture collision clearances and occlusion results.
- Working command mapping and production integration, which are outside a static recorded-value preview.

The fixture records visibility and pick results rather than declaring the whole proof passed. Broader renderer/material/cache denials remain untouched. Do not launch or replace the user cockpit on the strength of this preflight.
