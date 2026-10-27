# Study 05 independent source/export review — 2026-10-15

**CLEAR for exact source/export data and the selected source geometry.** Actual engine appearance and explicit adoption remain separate root gates; this is not v05 visual completion.

- Source `9cc3ac5d…c11b4`; GLB `df8490f4…3a2c9`; receipt `0576abdd…3287e7`. All bytes remained unchanged.
- 670 exact source-part identities, material slots and complete retained spans; nine canonical materials; exact 72 × 18 × 38 m bounds. 100,208 evaluated triangles minus 1,350 explicit float32-gate drops gives 98,858 retained triangles.
- Unchanged strict normal method: 98,850 triangles / 296,550 corners, zero mismatch bins or unique reversed pairs at 10/20/100 µm. Eight 0.48 µm thin-face corner ambiguities remain excluded from these strict counts. A separate position-only float32 supplement identifies each uniquely in its mapped source part, same winding, with all 24 normal dots exactly 1.0.
- All 197 changed/new selected source parts are closed and consistently oriented by indexed edges, with every raw/evaluated component volume positive. Evaluated zero-area geometry is reported separately from runtime gate drops.
- Both rear centerlines now first reach emissive drive throats about 2.8 m behind the aft rim. 106 independent direct/BVH ray checks agree within 0.1 mm. There are real recessed source openings; no physical propulsion adequacy or open-through tube is claimed.

Snapshot identities are not durable cross-revision IDs. The changed source/export hash requires explicit binding; old graphs must remain bound to their old hash. Readable native images, powered state, mount acceptance and publication are outside this source/data review. No model edits, builds or Mac access occurred.

Exact hashes, methods, count limits and supporting artifact manifest: [summary.json](summary.json).
