# Pioneer approved concept fit — 22 October 2026

The approved cockpit six-view, twelve-room study and two-deck schematic are visual targets, not dimensionally certified construction plans. This pass keeps the registered world layout, original authored GLB, entrances, chair and authoritative interaction surfaces. It does not complete the whole-ship redesign.

## Canonical fit

Body coordinates are +X bow, +Y up, +Z starboard. Each deck core is 32 × 14 m; floor heights are −4.12 and −0.32 m. Rooms are approximately 8.933 × 5.4 × 3.2 m with 1.4 × 2.4 m doors. The central corridor has approximately 2.3 m clear width. Aft internal stairs connect the decks.

The current cockpit bay is shallow and wide: X 31.6–34.9 m, Z −6.8–5.8 m, floor −0.32 m and ceiling underside 2.88 m. The pilot eye is [33.22, 1.35, 3] m; the rear connector enters near Z 0 rather than directly behind the chair. The actual boarding connection is upper/bow/port, through the angled vestibule and external lift. The concept schematic's aft boarding access on both decks does not match this model. Room D2-P1 retains its existing Bridge/Nav function; conceptual Nav/Planning is not an approved room reassignment.

## Bounded model work

Match the concepts through connected low console construction, cream structural skins, graphite recesses, stepped housings and metallic edge details around the existing functional instruments. Preserve instrument input planes, live data and command gates. Construction seams are passive geometry; additional knobs, switches, warning lights or holograms must not imply unsupported functions. The denied lighting/renderer work is excluded.

Keep the existing 0.6 m diameter, 1.8 m high traversal capsule and rear-chair dogleg. Changes to visual skins must not change collision, registration, door actuators or saved poses. Actual forward-view visibility and physical control projection remain acceptance checks. Compare pilot-forward, both quarter views, aft-facing and rear-chair views using the same native engine capture route before and after; isolated captures are not desktop acceptance or latency measurements.

## Structural differences that remain

The concept's deep, narrow, centered cockpit and straight rear aisle cannot be reproduced inside the present shallow bay by console modeling. Approaching that silhouette requires a separate plan: either a deeper cockpit with revised adjoining hull/connector clearances, or a broader bridge concept explicitly designed around the current offset pilot and entrance. Both require hull-fit, collision, saved-world and circulation assessment before implementation. This pass does not move the entrance, chair or room boundaries to conceal the mismatch.

The central holographic table, new life-support machinery and room-specific equipment in the sheets are proposals. Only existing authoritative telemetry and working commands may be presented as operational. The mouse-lag diagnosis is separate and remains unresolved; frame cadence alone does not measure input-to-display latency.

## Next model decision: instrument architecture versus hull change

The retained five display mounts also constrain the visible silhouette. A clean housing pass cannot turn those fixed rectangular apertures into the reference's low, continuous wraparound pilot station. The next coherent model phase should redesign the instrument layout and its corresponding physical input frames together: a low central multipurpose display/control shelf, independently recessed navigation and energy instruments, and angled peripheral task modules. It must preserve existing supported commands and telemetry, remap pointer projection from the same canonical definitions, and assess the resulting collision volumes against a fresh saved-world copy before delivery. Merely narrowing a visual bezel while leaving invisible old collision and click planes is not a finished reconstruction.

Two structural directions require comparison before changing the registered model:

- Retain the current 3.3 × 12.6 m bay and offset entrance, designing an intentionally broad bridge with an integrated low station. This preserves the major hull/connector arrangement but cannot match the concept's deep centered pod.
- Assess a deeper pilot enclosure extending aft into the current connector region, with a revised doorway/circulation design and actual hull-section clearance. A candidate study envelope is roughly 6–8 m deep and 4–5 m wide around the pilot station; these are proposed design ranges, not measured free space or approved construction dimensions. The present rear portal at X31.6/Z0 and boarding path must be explicitly reconciled. No hull, doorway, room or saved-avatar transformation is applied by this study.

Both choices need matched views with the seat and human-scale routes visible. The generated top-down sheet cannot be treated as a certified hull fit. Lighting fidelity and input-lag work remain separate open dependencies.

## Construction checkpoint result

The bounded coaming/cabinet changes compiled and the five-view native review passed on isolated authority. The existing strict mining composition check also passed at a 772 px editor aperture, including forward visibility and physical workspace/Undock interactions. Original collision/input/registration recipes outside the visual replacement helpers remain byte-identical to checkpoint 56196df. The preserved untracked console draft is unchanged.

This checkpoint is WIP, not a polished handoff. Pale breakup remains on horizontal skins after eliminating known coplanar plate overlap and increasing shell clearance; the remaining shading cause is unproven. The five fixed mounts still determine the overall silhouette. No new app package or restart is justified as concept completion from these results. Current delivered build remains 26.10.21.29. The separately rejected new enamel material was not integrated or retried; existing materials remain.

Retained evidence: /tmp/pioneer-concept-fit-before-20261022, /tmp/pioneer-concept-fit-corrected-20261022, and /tmp/pioneer-concept-fit-composition-20261022. Native captures are actual engine images, not desktop acceptance, acoustic confirmation, FPS benchmarks or input-latency measurements. The seated aft capture is chair-occluded and must not be presented as a useful concept-equivalent rear-enclosure view.

## Mount reconstruction recovery checkpoint

A subsequent source pass replaces the upright sockets with true inclined cases and a low chassis. Runtime rendering and pointer mapping already consume the shared canonical screen frames; the central 560 × 380 canvas remains. The native opaque-visibility test now removes the central instrument's own rotation before projecting body-space case geometry, preserving its original protected region.

The first refit compiled and rendered, but failed seated visual acceptance: instruments were below/outside the fixed 55-degree camera. Its isolated saved-copy capsule/support validation passed, as did leg-well and rear-route checks. These results apply to that earlier candidate, not every later coordinate change.

The currently staged correction is still unfit: NAV/POWER centers [34.80, .73, 2.25/3.75], task centers [34.80, .90, 2.25/3.75], and central MFD [34.40, .95, 3]. Analytical projection identifies narrow-aspect side clipping and wide-aspect case intrusion into the unchanged protected ROI. Do not package this candidate.

A further inward/down correction was rejected before execution by automatic approval review, citing the historical read-only diagnostic restriction. No approval request ID was returned. The proposed correction remains unapplied: side Z centers 2.34/3.66, NAV Y .715, task Y .875, with corresponding supports. Pure pinhole checks pass glass margins and case ROI for those proposed values, but they are not rendered or accepted geometry. No alternate writer or equivalent patch was used.

The rejected enamel variant and earlier renderer/camera/cache/power/backlight operations remain untouched. No app restart, user-avatar movement or live authority operation occurred. The last verified running package remains 26.10.21.29. This is a recovery checkpoint, not delivery or completion.
