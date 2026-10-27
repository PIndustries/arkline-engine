# Isolated forward-bay enclosure milestone — 22 October 2026

This is an original geometry study rendered through the existing imported-asset
preview and physical display code. It changes no canonical ship asset, room,
saved pose, command mapping, material definition, shader, lighting/backlight or
cache behavior. No gameplay integration, deployment or image upload is included.

## Geometry and scale

The previous section was an asymmetric cabinet and window surround suspended in
an otherwise empty preview. The revision adds a connected deck, raked overhead,
sidewalls and upper/lower structural haunches, with an apron below the forward
window. The rear remains open for study access. The local deck footprint is
**3.23 × 3.20 m**; the overhead underside rises from **2.09 m forward to 2.40 m
aft**. These are study dimensions, not certified free space in the canonical hull.

A shallow continuous desk now returns into both sides of the enclosure, with
three deck webs and visible space underneath. Its central underside rises from
**250 to 560 mm**. The decorative access-cover, seam and fasteners were removed;
the change concerns architecture and workstation proportions.

| Instrument case envelope | Before | After |
| --- | ---: | ---: |
| Depth | 270 mm | 180 mm |
| Height | 551 mm | 436 mm |
| Width | 850 mm | 676 mm |

The glass stays at the identical 400×240 canvas, approximately 600×350 mm face,
and X=0.900 m input plane. The case front moves to X=0.875 m, retaining a 25 mm
recess while reducing the deep front-lip parallax. The large rear flare and high
asymmetric shoulder are replaced by a narrower tapered case and low side decks.
The forward aperture top is deliberately lowered by 240 mm to meet the raked
roof; its lower sill and width stay fixed. Forward field of view is therefore
not claimed unchanged, although the display plane and cameras are unchanged.

The asset changes from **16 meshes / 3,256 triangles / 286,684 bytes** to
**22 meshes / 2,872 triangles / 258,456 bytes**. Current GLB SHA-256:
`7a2dbb0c78fe734a368eb2b697ab3c6e6dbad934245df77a87e3119baf79cd46`.
All three existing material definitions compare equal in the before/after GLB
JSON; existing components retain their palette assignments.

## Actual matched-view inspection

Before evidence is retained under
`/workspace/scratch/forward-bay-cloud-verified-20261022`; the first after render
is under `/workspace/scratch/forward-bay-enclosure-candidate1-20261022`.
Both contain actual engine images and hash-bound receipts. Seated and standing
eye/target coordinates, viewport, projection code and glass/input-plane definition agree
with the before fixture. Values are explicitly **recorded, not live**.

The standing image now shows floor, sidewalls and a bilateral workstation with
an open under-desk volume, rather than an isolated drum. The case reads slimmer
from both cameras. The seated image retains a legible primary instrument and a
visible top brow, but the forward window remains a large blank field in this
isolated preview. Broad flat sidewalls, coarse structural transitions and flat
preview shading remain visible deficits. The model is not a polished cockpit.

The repository's supplied Pioneer overview has a small proposed bridge inset;
it supports the general enclosed/wraparound direction. An approved cockpit
six-view sheet was not available here. No pixel-match or concept-completion
claim follows from this pass.

## Checks and limits

The explicit EGL proof passes with authored normals, exact component identities,
manifest bounds and triangle counts. All five sampled display points remain
clear in each view; projected label/value em minima remain **32.25/56.51 px
seated** and **14.84/25.95 px standing**. Actual egui text layouts fit without
pairwise overlap or clipping. This is sampled visibility and a font-size floor,
not proof of every pixel's accessibility or human reading performance.

The unchanged 17-pose standing-path capsule check passes against conservative
component AABBs, including tangent deck contact. Explicit console-intersection
and deck-penetration controls are rejected. Portable checks cover floor footprint
support at 33 path samples, support-web envelope contacts and the slim case
budget. Conservative path-envelope margins are at least **290 mm overhead**,
**350 mm to the side haunch**, **400 mm at the rear deck edge**, and **470 mm
behind the desk front** for the existing 1.8 m × 0.6 m capsule. These are
geometry-envelope measurements, not registered support, exact triangle collision
or continuous traversal admission. There is no fitted chair, rear door, server
collider or authoritative seat lifecycle in this isolated study.

Portable study checks: **3 passed**. Export-contract checks: **15 passed,
3 skipped** (official Blender runtime integration unavailable).
Physical-input checks: **13 passed, 0 failed, 1 ignored**. The manifest-bound
projection preflight reports **1 passed, 0 failed, 1 ignored**, with the ignored
native proof explicitly executed separately and passing.

One initial two-thread cockpit batch reported **44 passed, 1 failed, 1 ignored**
because the existing child input test exceeded its unchanged 20-second deadline.
The failure log is retained at `/tmp/arkline-enclosure-cockpit-tests.log`.
Isolated deadline comparisons then passed at **1.73 s candidate / 1.58 s
pre-geometry**; both two-thread comparison batches passed **45 passed,
0 failed, 1 ignored**. The earlier timeout's cause is unproven; no timeout,
assertion or production input path was changed to hide it. This does not
establish desktop input latency or a general performance improvement.

The eleven full-suite failures already reproduced at the migration checkpoint
remain documented in [the baseline diagnosis](cloud-baseline-failures-20261022.md).
CI remains unavailable; the Forbidden Actions API was not retried.

The next integration step is a geometric fit/connection plan against the actual
Pioneer bay and boarding route, including chair, rear access, collision/support
and canonical instrument registration. Keep this study isolated until that work
has independent evidence. The current geometry milestone does not replace the
live cockpit or change the user's world.
