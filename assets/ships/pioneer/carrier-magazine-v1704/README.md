# Physical Pioneer source 1704 — controlled inspection only

Exact measured source assets for the authenticated, fixed-host, fixed-target,
single-bay physical cargo client. This does not admit the source into normal
carrier inventory, fleets, warp, or public gameplay.

| File | SHA-256 |
| --- | --- |
| `pioneer.glb` | `08241a77914f7b909b666cde14c4f7efd6f4ed8364e3fe25e8926dee75a2e011` |
| `mining-drone.glb` | `9363b725e154f7219533461766c2dee2d1a6aaed7d662db38ad1e7c25071cf14` |
| `mining-drone-source.json` | `04ef513ff5e0fc89f4909bb3078b6ed899fcc6a3ee9637b5d5de9837c8168237` |
| `runtime-binding.json` | `a76a0ac0b108a7905c7caa2a5cc76621c6682a21a72a3b15cc95792f54355534` |

The ship binding preserves 199,632 exported triangles: 62,220 moving and
137,412 fixed, driven by 500 complete source roots. Drone source preserves
2,064 triangles, including the two explicit inlet panels. Its non-indexed
GLB loader is restricted to this exact asset hash; generic model admission
is unchanged. Body geometry is expressed in metres, with the existing
renderer conversion applied once at the world mount.

The client checks the source configuration, owner, actual actor and power
projection, complete roots, and exact route-state frame/inlet sample before
installing the scene. It does not advance machinery on observation. The
mining beam uses the fixed aperture and target ray in the immutable selected
bay certificate; there is no speculative articulated head or moving target.
The same source scene is retained by camera feeds and used for colour,
depth, and picking. Source validation failures clear the old physical scene.

Geometric capacities are 214,121,238 cm³ for the ship hold and 893,833 cm³
for the drone. They are not mass, structural-load, pressure, or flow ratings.
Legacy prototype ore-unit fields are not converted into these volumes.
