# Pioneer exterior effects

This document describes the bounded powered-exterior slice for issue [#43](https://github.com/PIndustries/arkline-engine/issues/43), under the [cloud implementation claim](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6197382437). Source changes, automated verification, runtime captures, and publication are separate evidence. Engine acceptance captures and exact-SHA test results belong in the claim / draft carrier once produced.

The [Pioneer design wiki](https://github.com/PIndustries/arkline-engine/wiki/Ship-Pioneer) supplies the canonical 72 m X × 18 m Y × 38 m Z hull envelope. Fixture coordinates use ship-local meters with +X forward, +Y up, and Z across the beam. The fixture mounts, colors, pulse timings and wattages in this slice are editable gameplay assumptions. They are not verified engineering specifications from the wiki and do not claim a new hull-model match.

## Definition and identity

The reviewable default document is [exterior-v1.json](../assets/ships/pioneer/exterior-v1.json). It matches `arkline_protocol::default_pioneer_exterior()`; an editor test checks equality. It contains 13 fixtures with stable IDs: three navigation lights, two strobes, four windows, two panels, and two drive-glow emitters. Its electrical load is 88 W. Drive glow has zero electrical load because it displays existing drive telemetry.

`hull_asset_sha256` binds the definition to `PIONEER_MATERIAL_SHA256`. Validation rejects another hull hash. A changed hull therefore requires an explicit reviewed definition/rebinding decision; loading it must not silently retarget either this definition or an existing material graph. This slice changes neither the canonical hull, editable Blender source, material graphs, nor dynamic turret assembly.

Definitions support at most 32 unique bounded fixture IDs. Mounts, colors, emission, local illumination, pulse timing, and summed electrical load are validated by the shared protocol. Exterior load is capped at 1066 W, reserving the room's maximum 134 W inside the existing 1200 W gameplay reactor budget. An enabled strobe reserves its load during the dark interval too.

## Editor workflow

Open **Game → Exterior** in the inspector. Select an existing fixture to edit its mount, RGB color, emission, light range/intensity, electrical load and, for strobes, pulse period / on interval / phase. Fixture IDs, kinds and hull binding remain stable in this bounded panel. Invalid drafts show the authority's shared validation message and cannot be saved or applied.

- **Save draft** writes a snapshot to the current project's `Assets/Pioneer.exterior.json`. It does not change live state.
- **Reload saved** reads that file into the draft. If editing continues while the read is pending, those newer edits are retained.
- **Load live (replace draft)** explicitly copies the confirmed authority definition into the draft and establishes its revision base. It does not save the project file.
- **Rebase draft (keep edits)** explicitly accepts the current confirmed revision as the retained draft's base. It permits an intentionally different saved candidate to be applied without discarding its edits; it changes neither file nor live state.
- **Apply to authority** sends the current draft with its expected live definition revision. This updates the shared Pioneer class definition only after the authority accepts it. A stale base must be reconciled with Load live, or explicitly rebased while retaining edits, before retrying.
- **Turn exterior ON/OFF** affects only the authenticated pilot's supported vessel. The panel displays confirmed electrical enable/supply state and independent drive OFF / IDLE / ACTIVE state.

Live commands appear only when `snapshot.exterior` advertises support. Project authoring remains usable with an older authority. An unsupported own-pilot vessel has no power command.

Project I/O runs on a dedicated worker with one bounded pending request and a bounded result channel. Reads are limited to 64 KiB. Saves validate and serialize a submitted snapshot, write/sync a uniquely created same-directory temporary file, and atomically rename it over an ordinary file. Symlink destinations are rejected. An unreadable saved file locks Save until Reload succeeds or the user explicitly chooses **Reset to runtime defaults; allow file replacement**. Reset alone does not write anything.

A save or live rejection preserves the local draft and accepted live definition. Save completion records the submitted snapshot while keeping edits made during the write. A successful authority acknowledgement does not install a local render-only definition: the panel waits for its confirmed snapshot. Session changes cancel outstanding UI mutation tracking; neither file completion nor reconnect automatically replays Apply or power commands.

## Simulation and rendering contract

The authority supplies exterior power and drive state. The client uses fixture output from the shared simulation clock for both emitted fixture pixels and nearby illuminated hull pixels. Navigation, windows and panels require exterior power. Strobed output uses period / on interval / phase in simulation milliseconds. Pause freezes that time; a Step advances 50 ms.

Drive glow follows existing drive state separately: OFF emits zero, IDLE emits 20% only while exterior supply is powered, and ACTIVE emits full output independently of the exterior electrical switch. Fresh Stop / manual braking also counts as ACTIVE while the existing flight state reports current acceleration; ACTIVE does not mean forward thrust alone. It does not add a propulsion model or attach turrets to the hull mesh.

The installed GLB batches existing source emitters by material. For each of EngineGlow, Window and Panel, the static material batch receives the componentwise maximum of `color × emission × output` across all fixtures of that kind. Editing one fixture therefore does not independently recolor its bound source part inside that shared batch. Setting every fixture's emission to zero for a kind suppresses that kind's static batch emission. Added fixture lenses and local illumination retain independent per-fixture parameters. Source-part-bound independent emitters remain future work and are not claimed by this slice.

Local illumination has a shared uniform capacity of eight lights in the main view and two in a feed. Flight lighting quality further limits the main view to two / four / eight lights for Low / Balanced / High; feeds remain limited to two. Light selection shares these budgets with other local lights, so independent fixture definitions do not guarantee that every fixture illuminates a receiving surface in the same view. The installed renderer's actual emitted and illuminated pixels still require integrated runtime proof; these implementation limits and source tests are not that proof.

Automated editor coverage checks default/source equality, project round trips and identity preservation, invalid/oversized/symlink file handling, explicit reset protection, edits retained through pending and failed operations, confirmed revision reconciliation, and cancellation across session changes. Local lights use the existing bounded, unshadowed BRDF collector; this does not establish fixture occlusion or physical photometry. Actual emitted pixels, hull illumination, timed Pause/Step behavior, persisted power, and two-client agreement require integrated engine acceptance evidence; unit tests alone do not establish those outcomes.

For software graphics environments that cannot provide the default four-sample OpenGL configuration, `ARKLINE_EDITOR_MSAA=0` selects no multisampling. Supported overrides are 0, 2, 4 and 8; unsupported or malformed values exit with a clear error. The default remains 4. This changes graphics context selection only.
