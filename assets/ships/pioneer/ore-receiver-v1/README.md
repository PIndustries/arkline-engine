# Additive ore receiver — source candidate revision 1

This original Arkline derivative module attaches to the unchanged registered
Pioneer revision12 master. The original hull, fifteen-cradle machinery and old
world compatibility remain unchanged. It is not globally adopted for old worlds.
New preview capability must independently pin BOTH the base hull hash and this
module hash. AGPL-3.0-only, same project authorship/rights as the original source;
no third-party geometry, textures or simulation values are introduced.

## Real authored pieces

The sealed aft-starboard face is at body[-36,-1,5] metres. Anchored mounting shoes,
gussets, a backed gasket, nested hollow sleeves, an open square head and two
locking shoes form separate editable meshes. The pressure hull is NOT cut open.
This models a sealed transfer interface, not an accessible cargo-room tunnel.

The Mining drone holds at[-40,-1,5] with quaternion XYZW[0,1,0,0]. Its tail mouth
is[-37.5,-1,5]; the0.44m square head enters the0.50m square socket by0.08m.
Three sleeves travel1/3m,2/3m,1m in−X; each0.40m sleeve retains66.7mm overlap.
The head is staggered ahead of them to prevent stow interference. Two genuine
side pockets allow locking shoes to move3cm onto the socket's inner sidewalls.
No jaw cuts through the flange or old solid armor; the corresponding procedural
Mining socket is required. The source part named `head_seal` is a protective
elastomer head band, not a pressure-tight mating seal: its0.225m outer half-width
leaves25mm radial clearance to the0.25m socket walls. Only the two deployed
locking shoes reach those walls. No internal mating seal lip is modeled. Existing laser/engine equipment is not reused.

Stowed hardware reachesX−36.58; deployed hardware reachesX−37.58. Operating
bounds MUST include this change. Export float32 tolerances require outward-rounded
bounds: working[-37.59,-1.81,4.25]..[-35.97,-0.19,5.75]. Do not reuse the old hull
bounds alone. Base ship bounds remain separate and unchanged.

## Source and loader contract

- `ore-receiver.blend`: editable official bpy5.0.1 source.
- `ore-receiver.glb`: one identity node/mesh, body-metre POSITION/NORMAL indexed
  triangles, five canonical-named PBR materials and 4488 retained triangles.
- `receipt.json`: exact source/export hashes, evaluated part ranges, retained and
  explicitly dropped runtime-degenerate triangle counts, mechanism reference roots.
- `authoring.json`: part conservative boxes, kinematic groups and contact dimensions.
- `clearance.json`: continuous conservative source/relative-box CPU proof.

Every source part appears exactly once in the receipt's primitive/index ranges.
Keep static ranges fixed. For dynamic groups `stage/1`, `stage/2`, `stage/3`,
`latch/port`, `latch/starboard`, apply currentRoot × inverse(referenceRoot) once.
All reference roots are[-36.5,-1,5] with identity quaternion. Then apply the full
carrier pose once, not a second heading/yaw approximation. Do not create geometry
by drawing complete primitives again beneath the moving ranges.

## Validation and limits

The CPU proof checks exact base hull triangles, actual additive source bounds,
continuous extension/lock sweeps, named intentional mounting contacts only,
full-size drone approach, exact socket remainder, and all fifteen old bay paths
against every possible receiver position. Closed-bay and closed-socket negative
controls must collide. Existing bay source/proof hashes remain intact. Independent
byte-level tests verify source hashes, normal lengths, primitive/part partition,
mesh indices, mechanism membership and non-empty negative controls.

No GPU/native capture was run during the preserved full-film job. These source
checks do not prove runtime collision admission, interrupted transitions, traffic
clearance, cargo ledger behavior, performance, visual readability or gameplay.
No kg, force, power, airtightness, charging, stored capacity or throughput is
inferred from appearance. Server acceptance and the100-unit/25-per500ms preview
policy are separate. The receiver and drone remain stationary during coupling.

Recipes are `tools/art/build_pioneer_ore_receiver.py`,
`export_pioneer_ore_receiver.py`, `validate_pioneer_ore_receiver.py` and
`test_pioneer_ore_receiver.py`. They neither weaken nor replace the full hull's
strict exporter/bounds contract. No external resources or animations are embedded.

## Exact accepted source receipt

Base hull SHA256: `7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd`.
Receiver GLB SHA256: `7101f1cfaf7ca4aa6bcae7ed8de966eb97dc47d00c087c644efc242be0f171ff`.
Receiver Blender SHA256: `3b4a1ba43c0904063574118d7a42e9c246b58aab19eb0a6c015590a0086f84c1`.
46 editable source parts; 4488 retained triangles; 2544 moving triangles; five material primitives.

## Independent review follow-up: full route envelope coverage

The original474720 pair checks enumerate eight outbound stages with doors open;
they are NOT by themselves complete launch/recovery/empty-return evidence.
`clearance.json` now also records73 conservative all-pose domains (71 moving
hardware parts plus internal/external full-drone domains), each disjoint from
the union of every receiver pose by at least2.84m on a separating axis. Those
Cartesian products include both door endpoints and every intermediate fraction,
every tier/station/fork position, the empty-tray return axes, and the exact
reversed recovery route. They deliberately contain more poses than a legal
choreography. The route source is pinned to its exact hash; source changes
require re-review of those bounds. This completes geometric separation coverage,
not runtime collision admission or peer/world obstacle acceptance.

The reported three protocol tests used the exact standalone geometry module with
a temporary `pub mod drone_dropoff_geometry` line in the protocol root, then
restored that root byte-for-byte. The source checkpoint intentionally does not
own protocol exports. The integrator must register the module and rerun combined
source tests; isolated tests do not mean canonical runtime adoption.
