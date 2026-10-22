# Canonical-fit enclosure candidate — 22 October 2026

This separate study corrects the coordinate mismatch measured at `0ce03f3`.
The `c198ee0` isolated forward-bay asset remains unchanged as a reference.
Neither study is adopted by the game. No canonical registration, saved identity,
seat, door, floor, screen, interlock, renderer or material definition is changed.

## Geometry

The new asset uses body coordinates directly. It exports registered cockpit
boxes with their exact oriented frames, including existing floor, rear bulkhead
and portal opening, all five instrument assemblies, static chair and deployed
actuated pan. The three existing study materials are reused verbatim as an
opaque inspection palette. Registered glass is omitted from this opaque preview;
no exterior scene is invented. Colors do not reproduce canonical game materials.
New member boundaries are derived from the registered portal, floor,
boarding walkway, existing header and seat root, with explicit authored wall
thicknesses and clearance margins.

Five proposed solid members make a connected local bay: an overhead underside
at Y=2.40 m, a starboard wall at Z=4.80 m, and a port wall at Z=-0.80 m split around
the existing X=33.7..34.5 m boarding walkway. Its header underside stays at
Y=2.08 m, matching the canonical port opening. The local walls run from the
registered rear plane X=31.6 m to the existing forward boundary X=34.9 m.
The bay remains connected to the offset rear portal at Z=0, with the original
port corridor outside the local partition still accessible.

The starboard circulation leg at Z=4.2 retains 300 mm beyond the full capsule's
radius before the proposed wall. This candidate is wider and asymmetric compared
with the old study because the actual circulation and entrance require it.
The registered deck remains at Y=-0.32 m; no translating gameplay to fit artwork.
The asset has **13 meshes / 5,244 triangles / 450,928 bytes**, GLB SHA-256
`07e8b762fadaed0f8fdb4d7c31ea69dd9e3b28dcb11d51d25e61d3441be41b50`.
Five meshes are proposed members. The unchanged reference vertices are grouped
by palette and chair/floor identity to respect the existing import object budget.
All 450 source panel identities and exact frames remain in the manifest; 13 glass
panels are omitted from the opaque mesh. The initial ungrouped export was rejected
with `Ship object budget exceeded`; no importer limit was relaxed.

## Reproduction and checks

From the repository root:

```sh
cargo run --locked -p arkline-protocol --example pioneer_canonical_enclosure > /tmp/canonical-enclosure.json
python3 tools/art/canonical_enclosure_proof.py /tmp/canonical-enclosure.json --output assets/studies/pioneer-canonical-enclosure-proof
cargo run --locked -p arkline-protocol --example pioneer_study_fit -- assets/studies/pioneer-canonical-enclosure-proof/manifest.json
cargo test --locked -p arkline-protocol --example pioneer_canonical_enclosure
python3 tools/art/test_canonical_enclosure_proof.py
python3 tools/art/test_forward_bay_proof.py
```

The fit report now has **zero deck offset**, rear portal within the exported
deck width, and **zero central instrument translation delta**. All five exported
instrument centers, half extents and axes come directly from `cockpit_screen`
and `cockpit_screen_axes`; the original 560×380 canvas is retained.

The candidate is combined with the complete registered collision model in memory.
At 25 mm maximum route spacing, **2,940 full-height capsule samples** retain
clearance and coincident support across private, linear-fleet and circular-fleet
placements. The routes include the rear portal, both chair-side circulation legs
and the port boarding walkway to the inner hatch. Original panels, supports and
portals are asserted unchanged. Existing canonical tests separately cover the
interlocked route beyond that hatch; no interlock changes are needed here.

For each placement, **1,001 actuated pan positions** have conservative bounding
separation from every new member. Fifty segments from the canonical seated and
standing eyes to the center and four inset corners of all five instruments miss
the new members. This proves the overlay adds no sampled instrument occlusion;
it does not prove inherited canonical display readability or visibility.

The example tests report **2 passed, 0 failed, 0 ignored**, including negative
controls that close the real port opening and intersect the folding pan.
Portable candidate export tests report **3 passed** (determinism, unchanged
palette, oriented bounds/floor placement, finite normals and outward triangles).
The original study tests remain **3 passed**, including byte-for-byte asset
reproduction. The candidate checks are added to the existing Linux CI script;
remote CI lookup remains unavailable and is not retried.

The ignored `flight3d::imported_asset::mesh_proof_tests::native_canonical_enclosure_geometry`
test accepts `ARKLINE_CANONICAL_ENCLOSURE_GLB`,
`ARKLINE_CANONICAL_ENCLOSURE_MANIFEST` and a new
`ARKLINE_CANONICAL_ENCLOSURE_EVIDENCE` directory. It renders geometry-only views
using the unchanged imported-asset renderer: canonical seated/standing eyes and
diagnostic rear-entry/port-route positions, all at 60 degrees vertical FOV.
It produces PNGs and a hash-bound receipt, with no live UI or telemetry claim.

The explicit current-source native test reports **1 passed, 0 failed, 0 ignored**
for the candidate and separately **1 passed, 0 failed, 0 ignored** for the canonical
reference without the five new members. Actual matched images and receipts are
retained locally at:

- `/workspace/scratch/pioneer-canonical-enclosure-candidate2`
- `/workspace/scratch/pioneer-canonical-enclosure-before-captures`

All four view positions, targets, projection and viewport match between them.
Locally inspected rear-entry and port-route images show a lower continuous
overhead and local bay walls around the existing chair and console, replacing
the distant side boundary/ceiling gap. The existing walkway remains traversable
in collision checks. The image alone does not prove clearance.

The forward images still show the same coarse five-instrument assembly and
large empty preview background. Standing pixels are **identical** before/after;
only **6,427 of 1,350,000 seated pixels** differ. Rear-entry and port-route have
207,057 and 189,118 differing pixels respectively. These counts locate visible
change; they do not establish aesthetic improvement. This fit checkpoint does
not solve the forward composition or concept-fidelity problem. There is no live
text in these geometry-only captures, so they establish no new readability result.

Existing canonical circulation/boarding checks report **2 passed, 0 failed,
0 ignored** and the existing chair check **1 passed, 0 failed, 0 ignored**.
Existing physical-input checks report **13 passed, 0 failed, 1 ignored**, including
the unchanged reaped-process deadline and actual command-routing assertions.
The eleven previously documented full-client baseline failures remain; that full
suite was not rerun for these isolated geometry and capture-test changes.

## Adoption boundary

The new member collision boxes and exported visible boxes agree in this study.
They are not yet installed in the protocol's registered geometry recipe or live
render path. Actual adoption needs a reviewed identity/source-revision update,
matching visual and server geometry, saved-binding compatibility and actual-game
UI visibility checks. This checkpoint does not certify complete enclosure,
pressure sealing, hull fit, human readability or concept fidelity.
