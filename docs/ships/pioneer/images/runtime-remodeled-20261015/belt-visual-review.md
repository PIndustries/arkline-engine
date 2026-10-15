# Pioneer actual-engine visual review — 2026-10-15

**Readable intermediate checkpoint accepted; v05 visual completion remains open.** Build 26.10.15.7032, source `fb50b8ffb274c1b5a7ee537faccb9475edf03ecd`, actual hull `ed5e29cdfc5f21027f251847def5319421aaf1af1eedd81f06b7a7cd4f69ec1a`. Viewed native Belt front-quarter/front/side/top/rear/bottom, powered OFF/IDLE, strobe and same-time accepted-thrust images alongside v05 and the frozen Study04 quarter.

Green collars, graphite brow/roof plates, rails, radiators and side service equipment are now readable under the existing sunlight after real accepted Belt warp/yaw. The source winding and shading audit found no reason to flip normals; nearby station geometry explained the earlier dark checkpoint.

Both existing gameplay gun/barrel assemblies are visibly present on the fore-roof pad in top/front/quarter views; receipt count is two. Static images prove presence, not articulation or mining operation. All 13 fixture positions match source-backed provenance and the exact current hull binding.

Independent decoded pixel checks reproduce the receipt: OFF→IDLE changes 7,698 pixels; dorsal on→off changes 100,648; paused, +50 ms and next-cycle-on images equal the initial on pixels. Same-time idle→accepted thrust changes 3,536 pixels at the two aft drive fixtures. The dorsal pulse is 100 ms on per 1,000 ms, phase zero; these are gameplay settings.

| Area | Visible remaining gap against v05 |
| --- | --- |
| Bow / bridge | Broad smooth forehead and heavy continuous chin; powered glazing is flat white and hides tinted bridge detail. |
| Roof / work deck | Repeated paired tiles and mirrored radiator/pipe layout; missing distinct work bays, crane, reels and varied equipment. Existing gameplay turrets remain the implementation to develop. |
| Collars / side bays | Green shoulders read clearly, but layered seams, recesses and machinery remain simple and repetitive. |
| Underside | Straight slab lower silhouette remains. The bottom view is unlit at this attitude, limiting fine-detail review; it does not establish the reference's stepped underside. |
| Aft drive | Flat opaque caps remain. Round cyan fixtures brighten with accepted thrust, but do not show or create nozzle apertures. |
| Fixtures | Source-backed positions are established. Physical lamp housings, flush angled wrap lenses and per-part emissive graph binding remain separate work. |

This is actual native EGL engine output with authoritative `EditorWorld` state on Mesa llvmpipe. It is not a new OS-window/TCP journey. v05's proposed interiors and engineering values remain proposals. No repo/model/code edits or builds were made by this review.

Evidence and exact image hashes: [belt-visual-review.json](belt-visual-review.json). Capture receipt: `/workspace/arkline-runtime/pioneer-authority-belt/authority-receipt.json`.
