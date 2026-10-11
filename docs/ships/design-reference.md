# Arkline construction fleet — design dossiers

Seven original concept images, one per current class, using black/graphite industrial hulls and emerald green identification. All mining equipment is articulated laser turrets, not drills. These are proposals for discussion and modeling, not shipped game graphics or production blueprints.

## Files and authority

- `Pioneer.png` through `Monolith.png`: visual design dossiers, including physical hull, exterior elevations, interior zoning, human-scale rooms and holographic displays.
- `Blueprints/<Ship>.svg`: editable, dimensioned vector planning sheets. Top and side envelopes use the same meter scale; front views and local deck plans have their own scale. Rectangles are bounding reservations, not hull profiles. Zoom these in a browser or vector editor.
- `Blueprints/<Ship>.json`: exact meter coordinates, deck elevations, room rectangles, door centers, current gameplay values and proposal status.
- `Fleet-Specifications.json`: all seven specifications together.
- `GAPS.md`: implementation gaps and acceptance work.

The JSON is the numerical source of truth. The SVG is its planning visualization. Generated art is an appearance study and can disagree in drawn proportions, labeling, component count or room placement; do not measure pixels in the PNGs or model directly from their annotations. Modelers must reconcile the chosen hull mesh with the vector/JSON reservations before implementation. No CAD fit or walkability acceptance is claimed.

## Existing game reference

Source snapshot: PIndustries/arkline-engine `cb76d975421937d4cb4ae5da57cb5488843b108b`. Hull, power, avatar and station-layout definitions are in `crates/arkline-protocol/src/lib.rs`; station movement/collision is in `apps/server/src/station.rs`. These values were also present in local checkout `e590a0683fc07558069c4f975b9c6e1ed6e6964e`.

| Ship | Length × width × height, m | Proposed occupied decks | Current cargo, game units | Charge, J | Regen, W | Warp, J | Mining cycle, J |
|---|---|---:|---:|---:|---:|---:|---:|
| Pioneer | 72 × 38 × 18 | 2 | 1,000 | 240,000 | 1,200 | 30,000 | 4,000 |
| Prospector | 180 × 92 × 64 | 3 | 2,500 | 360,000 | 1,600 | 45,000 | 6,000 |
| Hauler | 420 × 180 × 140 | 4 | 16,000 | 600,000 | 2,200 | 70,000 | 5,000 |
| Vanguard | 1,100 × 380 × 260 | 6 | 4,000 | 1,200,000 | 4,000 | 120,000 | 8,000 |
| Borealis | 640 × 260 × 180 | 5 | 8,000 | 900,000 | 3,000 | 90,000 | 9,000 |
| Stratum | 1,800 × 800 × 480 | 8 | 24,000 | 2,400,000 | 7,000 | 240,000 | 16,000 |
| Monolith | 5,200 × 2,200 × 1,400 | 12 | 80,000 | 8,000,000 | 16,000 | 800,000 | 24,000 |

Mining tiers 1–5 are Pioneer, Prospector, Borealis, Stratum and Monolith. Hauler and Vanguard are separate classes. All currently have power/mining definitions; two mining module IDs exist. Drone mining costs 1,500 J/cycle and drone attack costs 2,000 J/event in this snapshot. This is a partial gameplay energy budget, not a physically engineered plant design. In particular, do not interpret a 1,200 W prototype value as a credible 72 m spacecraft reactor rating or size a reactor from it.

## Human-scale planning standard

Implemented references: avatar height 1.8 m, station collision radius 0.3 m, walking speed 4 m/s. Proposed interiors: clear corridors 2.4 m wide; clear doors 1.4 m wide × 2.4 m high; room clear height 3.2 m; floor-to-floor pitch 3.8 m; planning wall thickness 0.2 m. Vertical access core is a reservation only: lifts, ladders, stairs, landings and emergency routes still need detailed geometry. The existing station quarters' 4 m opening is not a ship door specification.

The detailed plans reserve a compact human core (32 × 14 m for Pioneer, 48 × 18 m for others), while cargo and machinery remain separate large zones. Deck counts describe this occupied core only, not every possible industrial service level. Repeated support-room uses are proposals, not complete furnishings or a finalized room program. Large ships will need distributed additional human cores and transport routes; this package intentionally does not fabricate thousands of finished rooms.

## Hangar proposal

Small classes reserve drone servicing or cargo access, not an internal full-size ship hangar. Optional larger ship bays are proposed: Borealis 100 × 60 × 30 m (Pioneer bounding box fits), Stratum 240 × 140 × 100 m (Prospector bounds fit), Monolith 800 × 420 × 240 m (Borealis bounds fit). These are static bounding-box comparisons only. Doors, approach sweep, landing gear, collision safety, ownership, docking and ship-within-ship physics remain undefined. Current station hangars are external to ships.

## Modeling handoff

Build hull and interiors as separate GLB nodes using meters, documented origin and axes. Separate yaw/elevation pivots for the two mining turrets, optical emitters, thruster sockets, airlock leaves, cockpit origin, cargo interfaces and drone launch points. GLB loading, texture support and hologram pass are still tracked work. Do not bake a scan effect into hull textures; the same geometry should serve physical and hologram passes. Provide provenance and asset license for every model/texture. Include collision proxies, low-detail meshes, named sockets and external asset manifests once that pipeline exists. Never treat this PNG bundle as a GLB model pack.
