# Refined Study 05 source/export review — 2026-10-15

**CLEAR for exact source-v3/export-v3 data and selected source geometry.** Actual engine appearance remains a separate acceptance gate; no overall v05 completion is claimed.

- Source `61eb70e1…e57c9`; GLB `8125644f…cb123`; receipt `fadece50…d5429`. Source copy and before/after hashes match.
- 686 exact named-part/material/matrix identities and complete spans, nine canonical materials, exact72×18×38m. 104,884 evaluated minus1,418 explicit float32-gate drops gives103,466 retained triangles. Exporter code unchanged.
- Strict normals:103,458triangles/310,374corners, zero mismatch bins or unique reversed pairs at10/20/100µm. Eight thin aft-cover faces remain excluded by this tolerance method; the separate exact-float32 position supplement uniquely matches them, same winding, with all24normaldots1.0.
- All213selected changed/new source parts are closed and consistently oriented by indexed edges; every raw/evaluated component volume is positive. All23handling-arm parts have valid nonzero corner normals, including all triangles.
- True nozzle depth remains.128sampled flange rays show20mmrear clearance from counterbore floors and no cap inside flange thickness;266global direct/BVH first-hit depths agree within0.1mm. Prior rim/cap front overlap is resolved.

**Residual bound:** pressure-hull/nacelle counterbore floors remain mutually coincident behind the flange in64samples. This is distinct from the corrected front overlap and should be disclosed if discussing source surface cleanup; actual close pixels determine remaining visible finish. No whole-model overlap-free or engineering claim is made.

Exact hashes, methods, count limits and artifact manifest: [summary.json](summary.json). No model edits, builds, publication or Mac access occurred.
