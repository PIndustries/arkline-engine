# Existing electrical model: connected accounting

This local checkpoint exposes the authority's existing capacitor and circuit model. It does not add power consumption, change recharge/action rates, rebalance mining, infer kilogram mass, or simulate previously unmetered machinery.

## Contract

A shared pure budget supplies the existing Pioneer room integrator and response-only accounting projections. Stable component IDs identify the controller, room light, ventilation, reserve recharge and enabled exterior reservations. Projection is made on the response clone after integration; it is not a second energy writer or persisted telemetry stream.

The UI distinguishes nominal reactor rating, modeled main-bus reservations, net available charging and actual modeled capacitor storage rate at the sample. A full capacitor has zero storage rate without claiming the nominal reactor is off. Watts describe rates per advancing simulation second, not measured consumption or a historical energy interval. Paused, disconnected and stale presentations are labeled.

Emergency-reserve output is independent from the main bus. Its recharge reservation is already included in that bus exactly once. Instantaneous warp/mining/weapon costs remain separate joule tariffs and are never added again as watts. Enabled exterior reservations remain accounted during visually dark strobe intervals.

Unknown loads are explicitly unmetered, not zero: bay mechanisms, displays and other unmodeled ship systems receive no invented ratings. No physical mass, thermal or comprehensive life-support accounting is claimed.

## Binding and presentation

Public accounting follows the actually viewed pilot and actual owned vessel. The client checks supported schema, exact authority sample, capacitor values and every available physical vessel binding. It never borrows controller-private stock for a remote public view. Parked accounting remains authenticated-owner inventory. The parked hull format has no second independent vessel-ID field: its actual identity binding is established by the server's Account mapping, not redundant client verification.

Presentation uses the existing Fleet and physical engineering workspaces. No new physical screen, camera position, FOV or gameplay command is introduced.

## Evidence and limits

The server compares the shared-budget integration with the frozen previous formula across 512 state combinations and ten time boundaries, including fractional carry, full and empty capacitors. Native evidence replays genuine command Responses from the current registration-12 preview. These are sampled command states, not a continuous energy-measurement recording or performance benchmark. A separately labeled legacy parked-hull case does not imply hull switching is available in the current flagship preview.

Final gate receipts and renderer/authority provenance are recorded with the delivery artifact. Existing physical-bay registration, finite stock, handling interlocks and timed unloading remain unchanged.

## Accepted local verification

- Native renderer `aed08aa47b693e4205e186154d7d598afd1c49e9`, authority producer
  `8708b444d6ed05e15487ac833d4188d1b9b51b18`:16 sampled views passed; all image and
  Response hashes independently verified. Software EGL rendering is not a live
  hardware performance measurement.
- Final test-only source `5616bb12473fe859ebfba1f3fb155ac48aaeeb42` corrects two
  observational snapshot comparisons. Original failures remain preserved. Each
  sample and accounting projection is validated first; only advancing derived
  fields are normalized, retaining exact invariant fields and prior energy guards.
- Accepted standard suites:1359pass/0fail/66ignored, combining the unchanged
  workspace suites with the full workspace-feature journey replacement4/0.
  Initial workspace exit101 is not falsely relabeled as an exit0 invocation.
- All-target check, strict Clippy and formatting pass on the corrected source.
  Real five-miner physical recovery/docking/unload, rollback and restart tests pass.
- Registration12, actual Blender/GLB, bay lifecycle, docking and cargo authority
  are byte-unchanged from the preserved physical-bay base.
