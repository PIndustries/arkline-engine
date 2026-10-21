# Pioneer discrete instrument suite — 2026-10-21

This replaces the lower rectangular monitor faces with actual separate circular housings and a range module in the canonical cockpit. It is an original Arkline implementation, not borrowed reference artwork, and not a completed whole-ship overhaul.

| Instrument | Authoritative source | Meaning and limits |
| --- | --- | --- |
| Capacitor dial | PilotView.power charge_j/capacity_j | kJ display, capacity-calibrated scale; unavailable has no needle. Warp mark is the specified energy cost, not permission to warp. |
| Ore-load dial | PilotView.ore and cargo_capacity(ship_class) | Explicit legacy ore units, not kg/m³ or a physical-volume model. Existing cargo semantics are preserved. |
| Heading dial | FlightState.heading or confirmed warp trajectory sampler | System X/Z azimuth; +X=0°, +Z=90°. Docked and unavailable azimuth are explicit; no invented Earth compass. |
| Range module | Existing selected_position and pilot-motion sampler | Selected center distance, known speed and system/contact readings; missing values remain unavailable. |
| Mining status | Confirmed mining_modules and cycle timestamps | Actual assignment/countdown/stop reason, not proof of beam output or fabricated progress. |
| Berth / flight controls | Existing Dock, Undock, Navigate(Stop) requests | Local live freshness, pause, pending and seating gates; achieved state comes from snapshots, not button press. |

The old complete lower rectangular glass/backing/rims are removed. Three circular Carbon backings, separate segmented Titanium rims and one bounded range housing share normalized layout constants with content. Existing upper map/radar displays and central multipurpose computer remain. Camera mode owns the upper-left multipurpose display, replacing its map; dedicated lower navigation remains available. Turning camera mode off restores the map.

Visible console divider/inner-rail service strips now have recessed covers, perimeter pieces and passive captive fasteners within their previous solid envelopes. These are construction details, not invented cooling controls or fake status lights.

The default central face emphasizes actual speed, navigation mode and the current owned routine instead of the previous six-row prose table. Detailed power and management remain accessible. Reactor regeneration is labeled a rating. The Undock completion label says UNDOCKED only when the received state is already Location::Station.

## Undocking diagnosis

The current server has no continuous undocking phase: accepted Undock atomically changes Docked to Station and assigns its normal-space berth. A fresh copy of the preserved Pioneer state passed acceptance, normal navigation, movement and reopen with the source backup byte-identical and completed job/cargo/avatar/full ledger preserved. The physical-pointer reproduction exposed a separate client failure: a floating hover tooltip's sizing layer captured the next press, so the enabled Undock button never received a click. The physical berth/flight control bank now uses persistent visible help instead of hover popups. The reproduced press/release sends exactly one Undock, receives Location::Station, clears pending and changes to the free-flight renderer route. The extended native journey also checks the normal mining/view composition. This does not establish a continuous physical departure sequence: the current authority still performs an atomic berth release. Paused simulation is never silently resumed for testing.

## Preservation and acceptance

No camera, authored source asset, room registration, saved unit meanings or live avatar placement is changed. New physical modules remain within the prior lower case envelopes. Full-face metadata visibility/protected-forward-area checks remain strict even though the visible lower faces are now smaller separate instruments. Circular backing/center/edge rays and actual text containment provide additional module checks. Native renders are separate from live OS capture; no GPU-performance or acoustic-audibility claim follows from these visual checks.

This document records implemented source. Validation, package identity, actual launch and remaining visual gaps are recorded separately as delivery proceeds.

## Verified source checkpoint and open work

The native 1500×638 scene journey passed the physical first-click Undock, one request, confirmed free-flight route and cleared pending, followed by five-drone mining, display-corner/opaque protected-area assertions and external/3D/cockpit return. The ten focused physical-computer input checks, compact-canvas fit, command readiness gates and shared dial-rim alignment also passed. These are isolated native checks, not live desktop acceptance.

The existing five-bank silhouette and small actual-size dials remain visual limitations. A connected faceted dashboard pod/spine/brow and a power-footer/backlit-marking refinement were rejected before execution under the historical read-only interpretation. Neither new operation was applied or routed through another worker. Current markings are screen emission, not proof of nearby surface illumination. Bridge task lighting needs a real power/state contract.

Next product slices remain incomplete: physical command bodies with mapped actions; richer continuous dashboard construction; context-only dismissible information panels and stable all-drone/single-drone views with honest camera scheduling; continuous physical docking and transfer. None is represented as shipped by this checkpoint.

## Dashboard and performance follow-up

The geometry retry received previously missing owner removal/full-rebuild evidence and succeeded: a hollow tapered center pod, sloped cover, descending structural spine and low connecting brow now use the existing canonical frame. Eight new passive solids and the revised base preserve the glass, eye, seat, registration and source asset. The expanded leg-motion clearance check and native physical-pointer/mining/view/opaque-protected-area journey passed. This is structural progress; the full control suite and reference-level silhouette are still unfinished.

A persistent Performance toggle opens a dismissible, movable, scrollable diagnostics window. Its 120 observed client update intervals yield cadence FPS, average/p95/worst interval and measured hitch history. These include idle scheduling and readback in the native test, and are not GPU benchmarks. Snapshot receipt age uses a separate timestamp updated on every accepted snapshot, including remote sessions. Embedded authority tick work uses the actual cached graph trace host-duration and simulation timestamp; RTT and whole-frame GPU time are explicitly unavailable. Opening the overlay releases/gates cockpit, walking and management shortcuts while the authoritative simulation continues.

The separate power-footer/backlit-markings retry was rejected again before execution; no alternate writer or route was used. That refinement is unapplied. No bridge illumination or raytracing claim is made.
