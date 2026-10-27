# Pioneer physical drone bay — local implementation checkpoint

This is an integrated flagship-preview workstream, not a release or normal-world
migration. Existing worlds without the explicit initialization stamp stay legacy.
The raised bridge and its nine moving camera displays remain in place.

## Authored source and verified geometry

Registration 12 binds GLB SHA-256
`7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd`.
The retained Blender master contains fifteen supported cradles (three stations,
five tiers), trays, three retracting forks, lift, carriage, split rear doors and
pockets. One handling route is serialized; fifteen cradles are not fifteen
simultaneous launch throats. The 5 × 6 × 1.8 metre drone envelope is checked along
all accepted routes against source triangles and the other hardware/stock.
Source authoring proofs are geometry checks, not strength, pressure or motor simulation.

The runtime partitions exact source ranges into nine fixed material batches and
43 moving group/material batches, retaining all original vertices and triangles.
Moving groups use their authored reference roots and confirmed authority poses.
Color and depth use the same stock/active transforms. The extra draw batches must
be measured; the original nine-batch performance claim no longer applies.

## Finite authority contract

Fresh opted-in preview vessels receive an explicit test fleet exactly once:
five Recon, five Mining and five Attack drones. This is not an acquisition economy
or an unlimited replacement mechanism. Each identity owns a reserved home cradle.
Stored state owns HP; an active DroneView owns HP while away; destroyed identities
remain terminal tombstones. Capture transfers the same ID and current HP back to
storage. It does not grant ore, repair, money or a replacement drone.

Commands carry existing authenticated session authority, actual vessel identity,
source registration and retained monotonic operation IDs. Launch/recovery are
stationary-carrier operations. Holds, interruptions and restart preserve confirmed
poses rather than advancing offline. Destruction cannot race capture into a revived
stored craft; empty machinery cleanup remains a separate carrier interlock.

Existing incoming range/signal combat rules remain. The bay does not claim new
armor, invulnerability or hull line-of-sight protection. Constrained drones cannot
mine or fire. Crew traversal to this new aft room is not yet implemented.

## Verified visual checkpoint and remaining integration

The continuous authority sequence was produced by
`de940f55b480647508ab5e82dcbbd2f2f9afde64`; the production native renderer was
`8dd6de6b84322651f48aa1dbd697352ebe05f474`. These are distinct from later safety
fixes. All 1,171 actual Responses at 100 ms cadence and 2,342 matching native PNGs
have independently verified hashes. The original fifteen IDs remain conserved;
drone ID 8 leaves and returns to its original zero-based cradle 7 (displayed as
cradle 8), retaining its HP. Both fixed observation views show the actual hardware
and drone route. The close tray pass naturally occludes the bay lens; the full
paired video retains this rather than substituting another pose.

Four view-only observation cameras show the overview and each class rack. The
rack lens fits all five tiers without changing the pilot eye or FOV. Camera
selection stays on the selected identity: an active body uses its actual full
attitude, while a stored selection explicitly shows its home rack. It never
silently changes to another deployed drone. Five sticky cockpit cassettes remain
five active feeds, not fifteen simultaneous streams.

Checkpoint `b7205fee09abe11a74bab20483716749d0e9a45e` has 592 passing client tests,
zero failures and 49 explicit ignored native tests; all-target check, strict
Clippy and formatting pass. Its seven native stock/UI images and five sampled
same-ID camera images pass. The exact source and actual Blender/GLB are preserved
in a verified offline-restorable private backup.

The separately accepted four-file client interlock delta has 600 passing client
tests, zero failures and 49 ignored, plus check/strict Clippy/formatting passes.
It retains STOP and bay Hold/Resume/Cancel while blocking unsafe carrier motion,
and uses live command freshness rather than a frozen presentation clock. Actual
Close pointer and painted-stroke tests pass at 1280, 900 and 800 widths; an apparent
native glyph crop did not justify changing the window layout.

The integrated interlock renderer `2fa0e8f0c84a85c2350baddfacf7748f66cdcf4a`
has 600 passing client tests, zero failures and 50 ignored native tests, strict
client all-target Clippy and full formatting passes. Its eight actual-response
900px native images show stored → handling → free → stored: STOP remains usable,
the complete handling explanation is visible, and Dock restores only after
recovery. These sampled views retain the historical `de940f55` producer identity.

## Final integrated acceptance

Exact implementation checkpoint `e7d3413d1465daf0eb589b14ccf401faacc32b6b`
passes the full workspace test command, all-target check, strict all-target Clippy
and full formatting check. Client: **600 passed / 0 failed / 50 ignored**;
editor: **297 / 0 / 1**; protocol: **99 / 0 / 0**; server library: **144 / 0 / 9**.
All integration binaries passed, including seamless docking **11 / 0** and timed
unloading **14 / 0 / 2 ignored exporters**. Explicit native/export ignores are not
presented as automatic-suite passes.

The accepted authority is `0d10a83d38cd22bd919cf8287b1a6844a9fba24d`, whose
production is byte-identical to `a68bd5879a0cd3c6c29807f9e11e92f247a38560`.
Its separate 48-binary gate passed **434 / 0 / 12 ignored**. The final test-only
follow-up extends an obsolete 300-second routine observation ceiling to 1200
simulation seconds, retaining 50 ms ticks and every departure/capture/deposit/
ledger assertion. No production action rate or timeout was changed.

Final-source native acceptance passes sixteen sampled bay/aft images and eight
900px physical/navigation images. Independent review verified all image and raw
Response hashes, same fifteen identities, ID8/HP100, proper home vacancy/refill,
readable restrictions and restored Dock readiness. These are sampled views, not
another continuous movie. The full historical movie retains its original source
labels. Final versus historical raw sequences match all launch frames, registered
bay machinery/door poses, phase timing, full attitudes, IDs and HP. Thirty exterior
return frames differ by at most **7.493 mm** after correcting the conservative
collision sphere; no whole-route exact-parity claim is made.

The complete finite-stock mining → physical recovery → docking → transactional
unloading path and independent economic verifier pass. A test fleet is initialized
once only in fresh opted-in previews. Legacy saves and their available historical
asset references remain unchanged. Candidate/failure receipts are retained beside
final receipts rather than hidden. The final documentation-only checkpoint does
not change the validated runtime or assets.

Captured normal-cadence footage is an offline software-renderer replay, not a
live frame-rate benchmark. The extra source batches still need release-hardware
performance measurement. Incoming combat keeps its prior range/signal rules;
bounded bay obstruction admission is not global collision response or hull LOS.
No new acquisition economy, repair, energy/battery model or aft-bay crew access
is claimed. Registration 12 and the bay feature capability require matching
client/server support; existing unstamped worlds are not silently migrated.
