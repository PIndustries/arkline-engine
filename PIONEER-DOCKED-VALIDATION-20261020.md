# Physical Pioneer: locally accepted native build

Build **26.10.20.7** runs from clean runtime source **cbf48c5cd09a97389d0260c79a09fbac63089f48**. Local required checks, all **14 explicit native GPU tests** and the actual **macOS component viewer and full docked physical round trip pass**. PID **64207** remains paused and seated on a disposable `/tmp` project. [Native OS acceptance](PIONEER-NATIVE-OS-ACCEPTANCE-20261020.md) records the exact scope, screenshots and authority observations.

[Prepared app](<.build/packages/pioneer-docked-fixed-20261020/Arkline Editor Reconstructed.app>) · [Commands, receipts and limits](PIONEER-DOCKED-VALIDATION-20261020.json) · [Source recovery receipt](.build/packages/pioneer-docked-fixed-20261020/source-checkpoint.json)

## Implemented journey

The registered Pioneer now has a fitted physical cockpit, folding pilot seat, two decks with real stairwell openings and risers, a vestibule, two interlocked hatches, a boarding lift and a station-floor approach. Seat exit, movement, lift travel and return share metre transforms and collision geometry between authority and rendering. The existing 72×38×18 m hull remains the source reference; this fit is based on the verified local hull image, plan and specification, not new CAD or engineering approval.

Owned docked captain requests use negotiated, vessel/layout-bound state. Tests walk down and up both real stair flights, operate actual hatches, ride the lift to station and back, and approach/reseat continuously. Finite command bounds, movement leases, power hold, occupied-door and lift interlocks, layout replacement safety, and old-client projection/recovery remain enforced. Existing legacy crew movement in flight is preserved; docked-only applies to this captain exit/station milestone.

The client uses one registered physical scene along that journey. Headlook is independent of authority heading. Actual computer/instrument pixels use physical display faces, matching triangle depth and pointer blocking. Visible LeaveSeat input emits the exact bound request; a held computer click is cancelled after the display disappears. Real first/third-person choices persist, and aboard Cockpit/External/Tactical modes retain their corresponding renderers.

The original GLB, 686 source-object identities and nine inspection GPU buffers remain unchanged. Three SHA/range-guarded derived buffers provide the fitted doorway/hatch cuts. The previous component browser, shared model previews, imported GLBs, materials, drones, physical feed and live instruments are included in this single build and covered by regressions.

Each camera independently selects a conservative frustum set. Timings report selection/submission and optional delayed GPU observations. Native full-versus-selected image comparisons are pixel-identical at two tested poses, reducing submitted mesh draws from 405 to 19 seated and 405 to 70 on the upper deck. No room-portal occlusion, LOD, simulation culling or measured FPS improvement is claimed.

## Exact-head local validation

| Check | Result |
| --- | --- |
| Full workspace suite | 964 passed, zero failed, 35 explicitly ignored across 55 top-level suites |
| Formatting | Passed |
| Native workspace/all-target Clippy, warnings denied | Passed |
| Exact CI Wasm client Clippy, warnings denied | Passed |
| Explicit native GPU regressions | 14 passed |
| Packaging/publication-boundary Python tests | 8 packaging and 15 publication checks passed |
| Native speech bridge compile, warnings denied | Passed |

[Required-check receipt](.build/packages/pioneer-docked-fixed-20261020/final-head-checks.json) · [Native test batch](.build/packages/pioneer-docked-fixed-20261020/native-final-batch.json) · [Physical scene](.build/packages/pioneer-docked-fixed-20261020/physical-scene/receipt.json) · [Actual composed cockpit/input](.build/packages/pioneer-docked-fixed-20261020/physical-composite/receipt.json)

[Supplemental corridor/stair proof](.build/packages/pioneer-docked-fixed-20261020/supplemental-route-evidence.json) · [Lower corridor](.build/packages/pioneer-docked-fixed-20261020/physical-scene-route-views/lower-deck-corridor.png) · [Actual stairs](.build/packages/pioneer-docked-fixed-20261020/physical-scene-route-views/lower-core-stairs.png)

The original scene test records 18 camera checkpoints plus four full/frustum comparison images, visibly distinct inner/outer hatch states, actual instrument pixels and original buffer invariance. The composed test renders the actual Cockpit app and secondary computer, captures real UI requests into an isolated channel, exercises typing/focus/headlook, stale input cancellation and view selection. Its on-foot state is a deterministic received snapshot; continuous authority walking is tested separately. All GPU contexts are private macOS CGL on Apple M4 Max/OpenGL 4.1; they do not establish OS input acceptance or a packaged app launch. The full logs retain the existing Apple zero-texture sampler warning, while image/state assertions and explicit GL error checks pass.

Visual review identified that the retained lower-deck image faces its legitimate forward bulkhead only 1.85 m away. Exact ray inspection confirmed the shared wall and the open upper-deck portal; no render/collision mismatch was found. A later test-only checkpoint adds grounded, capsule-clear corridor and stair views, spatial depth/color checks and a third full/frustum comparison. Its separate receipt and native lint/focused GPU logs are recorded in the JSON at test checkpoint 2efe7de. Root inspected all four new corridor/stair captures. All three floor/tread landmarks match in every new view; the largest GPU-depth world error is 5.31 cm overall and 1.14 cm on stairs. The supplemental corridor comparison also has identical pixels with 405→205 mesh draws. All 28 supplemental PNG hashes/dimensions were independently checked; authority journeys remain separate. The packaged runtime is unchanged from cbf48c5; the full workspace and 14-test batch above ran at that exact clean runtime checkpoint.

Candidate 26.10.20.6 at 4297c13 is retained as unaccepted: its full run exposed the camera regression. The replacement fixes that contract and the independently reviewed 13 ms lift-reversal defect. Both focused regressions pass before this final-head run. The original five hanging interaction tests were stopped/reaped earlier; their input-lock repair is retained and all later runs are bounded. There is no current hung process or execution-tool blocker.

## Package, recovery and acceptance

The app uses a verified deep/strict local ad-hoc signature, CFBundleVersion **2485.0.7**, debug profile with incremental compilation/debug information disabled. Packaged executable SHA-256:

`e850b42158fae7c67c6d0884ee4c5f4d5ef5329a6f57ba981a75abafa25d5ec3`

Incremental source checkpoints preserve the physical WIP 170de45, threshold repair 9c1e92d, native query repair ee9c903, composed/authority proof 371e64e, lift repair f196e2b and camera/input repair 98b95dc. A complete seven-ref bundle at cbf48c5 was verified before final tests; the source recovery receipt records the later documentation checkpoint and its complete verified bundle. The validated 26.10.20.5 component app/bundle and earlier recovery/shared-runner sources remain retained.

This is a reconstruction over retained inputs, with no remotes, rather than exact recovery of missing combined 46/47 source. Results are local; no published exact-head CI, browser GPU run or Linux CI is implied. Following explicit user authorization, the old editor was ordinarily closed and this verified candidate was launched. The native journey covers one fresh circular-fleet Pioneer at yaw0: seat exit, both decks/stairs down and up, hatches, lift to station, station walk, return and reseating. OS physical computer typing/focus/headlook also passed. Detailed sampling and scope limits are in the linked native acceptance report.

Automatic approval review rejected a persistent whole-Documents permission grant as broader than close/reopen authorization. No grant executed; Don't Allow and a fresh disposable `/tmp` project allowed the native work to finish. No saved-work handoff answer remains pending.

No push, PR/issue/project mutation, merge, publication, deployment, external-drive operation or user-project modification was performed.
