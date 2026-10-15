# Pioneer model study 03

**2026-10-15 · Superseded study: bow-corner fan rejected; use study 04 for further review · Not the installed game hull**

This snapshot preserves a real editable Blender master, its static GLB export and four matched Blender views. It derives from the original Pioneer source at `3e415204747cd9adaa146a9777831a7acc82e7cb` and is prepared for review through [existing draft PR 47](https://github.com/PIndustries/arkline-engine/pull/47).

- Extended visible green cladding over the evaluated outboard nacelles and fore/aft collars.
- Connected frontal and side bridge glazing; lowered existing bow lamps below the glass.
- Restored the upper centerline RCS ray surface to approximately Y=8.3 m with a transverse roof frame.
- Added a corner brow fan that was rejected during visual review and superseded by study 04.

## Files and history

- [Editable master](pioneer-study.blend) — 511,253 bytes.
- [Static runtime-format export](pioneer-study.glb) — 7,756,576 bytes.
- [Hashes, dimensions, material slots, camera settings and validation scope](study.json).
- Matched views: [front](../../../../docs/ships/pioneer/images/model-study/front.png), [side](../../../../docs/ships/pioneer/images/model-study/side.png), [top](../../../../docs/ships/pioneer/images/model-study/top.png), [three-quarter](../../../../docs/ships/pioneer/images/model-study/quarter.png).

These stable paths change in separate study commits. Use the full Git commit SHA in download and image links to retrieve an earlier revision; do not treat a moving branch URL as an immutable historical snapshot. The dated timeline belongs in the [Pioneer design archive](../../../../docs/ships/pioneer/README.md). Original canonical assets and the seven historical images are preserved.

## Editing and coordinates

The Blender 5.0.1 master contains 541 individually named mesh objects and was saved **before** applying export modifiers, triangulating and joining parts by material. Open and edit that master, then export a derived copy. The GLB has nine material batches and cannot recover the source object granularity. The original `tools/art/build_pioneer.py` clears/rebuilds its scene; rerunning it is not a way to preserve edits in this derivative master. Portable derivative workers are not included in this snapshot; their hashes are provenance only, not a reproducible published build recipe.

The runtime body uses metres, **+X forward, +Y up, +Z starboard**. Body `(x, y, z)` maps to Blender `(x, -z, y)`; Blender `(x, y, z)` maps back to body `(x, z, -y)`. glTF export uses Y-up. The shared origin and evaluated body envelope remain `(-36, -9, -19)` to `(36, 9, 19)` m: **72 × 18 × 38 m** (length × height × width). Indexed GLB extrema agree within the 1 mm inspection tolerance; that tolerance is not a manufacturing specification.

The existing articulated turret centers are `(10.8, 9.864, ±2.66)` m, above the hull's Y=9 support. Aft drive centers remain `(-35.9, 0, ±12.5)` m. Turrets are separate runtime entities and extend above the 18 m hull envelope. Their absence from these static renders does not mean duplicate turrets should be added to this GLB. Coordinate retention does not establish interior registration or mechanical clearance.

## Export checks

Read-only packaging checks measured **79,434 triangles, nine primitives/material batches**, finite positions/normals, zero triangles at the runtime degeneracy threshold and indexed bounds within the declared envelope. Each source/export file is below 16 MiB. The recorded budget checks are static inspections, not execution of the Rust decoder or a GPU acceptance journey.

Native decoder/staging acceptance is not recorded for study 03.

The GLB preserves this explicit array order; a primitive's traversal position is not its material index. Graphite remains slot 0, but this study has a different asset hash and the existing exact-hash default binding remains unchanged.

| GLB slot | Material |
| --- | --- |
| 0 | Graphite ceramic armor |
| 1 | Titanium machinery |
| 2 | Recessed carbon |
| 3 | Emerald structural frames |
| 4 | Drive emission |
| 5 | Working lights |
| 6 | Navigation glass |
| 7 | Safety amber |
| 8 | Hull identification ivory |

The export is static, indexed and opaque, with scalar materials and no textures, skins, morph targets, animation, embedded cameras or punctual lights. `Navigation glass` is an opaque material name. Emissive geometry is not proof of illumination.

## Remaining limits

- Rejected visible bow/glazing corner fan creates crossing/intersecting facets; superseded by study 04.
- Broad gray brow/chin and repeated roof/service equipment still differ from v05.
- Study only: canonical game hull and exact-hash material default remain unchanged.
- Static Blender renders omit the separate articulated gameplay mining turrets.
- No working door, exterior light sequence, power connection, collision/interlock, interior registration or runtime animation is demonstrated.
- Named editable mesh objects are preserved; no durable component IDs, semantic hierarchy or operational pivots are established.
- Decorative lamps/emissive material do not establish actual illumination or a physical electrical/thermal simulation.

The four images use the same fixed body-space cameras, 1200 × 800 resolution, Cycles CPU lighting and 16 samples as the original-model comparison. They show this editable source under Blender lighting. In-engine appearance, working systems and performance need their own evidence tied to the exact model revision and engine build.
