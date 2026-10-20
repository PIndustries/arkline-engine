# Airlock interaction repair — 2026-10-20

## Observed failure

The live version 24 window was PLAYING and Connected, at a grounded registered Pioneer airlock threshold with closed hatches, service power available, the boarding lift stopped exactly at ship level and no active routine. Accessibility listed enabled Open inner/outer hatch buttons, but the actual full-size screenshot showed neither button: the expanded Nearby controls body was clipped below its bottom panel. The registered strip capped height at112 logical pixels; its details scroller derived its height from already exhausted remaining space and collapsed to1 pixel.

At the observed saved pose, range and both complete opening sweeps pass the existing authority checks. No server opening defect or pressure-state fault was established. The registered hatch interlock currently requires the other hatch closed, an available stopped lift for the outer hatch, power, valid embodiment/binding and a clear physical leaf sweep. No unimplemented pressure simulation is represented as live telemetry.

## Bounded correction

Primary nearby hatch controls and expanded details share the real bound SetHatch request and readiness calculation. They show cabin/vestibule/lift purpose and actual closed/opening/closing/held fraction/target state. Reasons explain clock/connection/freshness, routine authority, power, lift alignment, other-hatch and occupied doorway/sweep conditions. Lift alignment matches the server tolerance. The primary controls sit outside the optional details scroller; its panel reserves sufficient intrinsic height and the scroller no longer depends on exhausted space.

Existing authored door/chamber geometry and animated mechanical motion are retained. This milestone improves reachable operation and truthful mechanical status; it does not claim a new airlock model or new renderer features.

## Coordination and limits

Authority investigation, airlock UI implementation and independent navigation/performance review ran in parallel with one build owner. No measured runtime bottleneck was established; the user's faster-progress request is addressed with coordinated work and one affected build, not speculative engine changes. The independently blocked renderer work remains untouched. Final acceptance requires an actual populated world viewport and visible primary controls, then a supported live hatch action where user activity and current authority permit it. User avatar, job, ledger and authored geometry must remain preserved.
