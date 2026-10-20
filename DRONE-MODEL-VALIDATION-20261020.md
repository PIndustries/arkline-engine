# Native drone models and viewer

Build **26.10.20.2** is running as **PID9337** on the preserved disposable
ReconstructionQA project. Open **Model > Drone models** to select Recon, Mining or
Attack, drag to orbit, scroll to zoom, and Reset framing. The final window is left
on Attack with its fitted camera.

[Open the app](<.build/packages/drone-model-viewer-fixed-20261020/Arkline Editor Reconstructed.app>) ·
[Exact evidence and hashes](DRONE-MODEL-VALIDATION-20261020.json)

## Source and behavior

The feature is saved at `c26128d8dabca3872d7232647facd2c2298e5910`. Native
inspection found coplanar Attack trim; `c785de0c8479835f216908dd04347d921fd48fa4`
recesses two surfaces to remove the visible flicker. The packaged source is this
commit with BUILD_VERSION set to26.10.20.2. The final delivery checkpoint adds the
version stamp and this evidence.

All three models share cached CPU vertices, GPU catalog mesh slots and
color/emission values across flight, the viewer, cockpit depth and pointer
occlusion. Inspection adds only a centered uniform display transform. Existing
ship/floor/station indices stay unchanged. Protocol, gameplay, drone speed/range,
position interpolation and camera-body exclusion remain unchanged.

| Model | Triangles | Draw batches | Actual Mac journey |
| --- | ---: | ---: | --- |
| Recon | 900 | 5 | Selection, orbit, zoom and Reset passed |
| Mining | 956 | 5 | Selection, orbit, zoom and Reset passed |
| Attack | 1000 | 5 | Selection, orbit, zoom and Reset passed |

The original visible5×6×1.8meter envelope is preserved as a presentation bound,
not a new physical specification. Design references and usage are in
[drone-models.md](docs/drone-models.md).

## Validation

The full bounded workspace run passed **854 tests, zero failures,30 ignored**
before the two-position trim correction. After that correction, all16 applicable
drone tests passed (two explicit external/native cases ignored), native/all-target
and browser-target Clippy passed with warnings denied, and formatting passed.
The complete workspace was not repeated for the two presentation coordinates.

All **six named Mac CGL checks passed for26.10.20.2**: the real viewer callback
journey for every drone, depth-query safety, physical display perspective/depth
state, canonical enclosure/apertures, physical feed orientation, and whole-App
Shift/live instrument/video composition. These use Apple M4 Max, GL4.1 Metal91.7.
The viewer test checks distinct silhouettes, exact shared geometry/material tuples,
source immutability and retained GPU buffer identities; it does not claim GPU
buffer payload readback.

Separate **actual OS mouse/scroll journeys** selected every class and captured
default, orbit, zoom and Reset in the running app. Decoded3000×1984 owned-window
captures show substantial orbit/zoom changes and **zero changed canvas pixels
after Reset for all three classes**. These are real window captures, not mockups.
No keyboard text was injected during this slice.

[Recon capture](.build/packages/drone-model-viewer-fixed-20261020/os-ui-acceptance-20261020/os-recon-default.png) ·
[Mining capture](.build/packages/drone-model-viewer-fixed-20261020/os-ui-acceptance-20261020/os-mining-default.png) ·
[Attack capture](.build/packages/drone-model-viewer-fixed-20261020/os-ui-acceptance-20261020/os-attack-reset.png)

## Package and preservation

The app has a verified deep/strict local ad-hoc signature and CFBundleVersion
**2485.0.2**. Executable SHA256 is
`fdece742b6603ef0c5eb08bb542184e3b6718fda79215f8cc7f26413d9baa344`.
Previous owned editor processes26743 and5329 were closed through their normal
window close controls. Ordinary `open -a` cold opening succeeded for both new
bundles. No process was forced closed.

One internal `.build/target` remains guarded against path/filesystem changes,
low disk space and per-operation deadlines. Original recovery sources, the shared
runner, previous packages and fixtures are retained. The final source bundle and
verification receipt are saved beside the app as `source-bundle-receipt.json`.
No push, PR, merge, deployment or external-drive operation occurred.

## Remaining acceptance scope

These are local checks; no published exact-head CI is claimed. Browser GPU runtime
and the older reconstruction's controlled OS text-focus/continuous-feed/depth
probe remain unverified; the separate native GPU depth and Shift checks passed.
The source remains a new reconstruction over retained inputs, not exact recovery
of the missing combined46/47 source. Initial26.10.20.1 screenshots named
`os-recon-*` actually show Attack selected; those historical filenames supply no
Recon acceptance. The final explicit per-class captures above supersede them.
