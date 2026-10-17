# Measured planet and station scene

This development change gives newly created editor worlds a measured orbital
scene and circular personal-fleet berths. It preserves the geometry of existing
worlds. Implementation and validation are tracked in the [scene workstream
claim](https://github.com/PIndustries/arkline-engine/issues/41#issuecomment-6237056463),
with berth and scale acceptance in [#40](https://github.com/PIndustries/arkline-engine/issues/40).
The dimensions below describe the current source definitions and layout plan;
they are not a completed native visual or performance acceptance report.

## World versions and compatibility

`LegacyV1` retains the existing planet centre, approach point and linear fleet
berths. Missing layout data in an older saved world means `LegacyV1`; opening it
does not opt it into the new scene or relocate its pilots, surface props or
navigation targets. A saved revision remains authoritative on reopen.

Fresh `EditorWorld` creation explicitly supplies `OrbitalV1` when inserting a new
world. The creation choice is ignored for an existing world row. Ordinary
server/local-world creation retains its legacy default. No automatic migration
or legacy-world conversion is provided.

Open **Session** in the editor toolbar, choose **Local** or **Host**, then select
**Orbital station preview** and use **Use local world** or **Host shared world**.
This opens a separate persistent world under the original project authority
directory at `realms/orbital-preview-v1`; it does not move or overwrite the project
world. Select **Project world** to return to that original directory. Each realm
keeps its own world data and pilot resume credentials. Existing files or symlinks
in place of the preview folders are rejected.

The dialog's **Selected world** identifies the selected storage, and **Current
layout** comes from the accepted authority snapshot. Selecting a directory never converts
an existing world there: a previously saved legacy preview also stays legacy.
The **Project world** label describes storage, not a promise that its layout is
legacy. **Join** uses the remote host's world layout and offers no local realm
choice. A validation rejection or busy command queue leaves the current session
in place. Once a switch is accepted, the old worker stops before the selected
world opens. A corrupt database, unreadable credentials or occupied host port can
therefore leave the selected world disconnected. The dialog shows the worker's
error and the return path: choose **Local**, **Project world**, then **Use local
world** to reopen the preserved project. This handoff preserves world data; it
does not guarantee an uninterrupted live session when the new target fails.

The authority sends an immutable `WorldLayoutDefinition` in snapshots: schema
version **1**, the scene revision, planet definitions and fleet-layout revision.
`Connect` and `AccountConnect` add `world_layout_version`; an omitted value means
**0**, for legacy-only clients. An orbital realm requires capability **1**.
Unsupported capabilities are rejected before admission. A client validates the
declared descriptor; unknown or altered geometry is an error, not a reason to
guess replacement coordinates. An absent snapshot descriptor retains the exact
legacy definition.

Shared-editor viewers negotiate the same capability on their join message even
though they do not create a pilot through `Connect`. The command-line client
advertises legacy-only capability because it does not validate the new layout.

This is a separately versioned layout contract. The global protocol version and
economic ruleset remain **1**, and the ledger event format is unchanged. A layout
descriptor is not an arbitrary scene-editing API or an economic migration.
New orbital editor databases use the marker `local-development-orbital-v1`;
older server/editor readers reject that marker before reading world state.
Existing legacy markers stay unchanged. Marker/layout disagreement is also an
error before migration or clock writes. The regression preserves the old reader's
source behavior; it is not a test of a launched older binary. No conversion for
opening an orbital world in an older build is provided.

## Verdant at a physical orbital distance

Verdant keeps its **6,000,000 m radius** and **60,000 m atmosphere height**. In
`OrbitalV1`, its centre is **23,182,219.83094 m from the station docking port**,
along the negative Y axis. This follows `D = R / sin(15°)`, so its geometric angular
diameter at that port is **30°**. Actual screen coverage also depends on the
camera's position, direction, projection and viewport.

With the unchanged port at `(-4500, 0, 0) m`, the new planet centre is
`(-4500, -23182219.83094, 0) m`. Legacy worlds retain centre
`(0, -6120000, 0) m` and approach point `(0, -115000, 0) m`.

The approach point is **5,000 m above the surface**, on the station-facing side.
Rendering, selection/range displays, navigation and authoritative surface checks
must consume the same world descriptor; the planet is physically farther away,
rather than drawn at a different cosmetic offset.

The straight distance from port to approach is approximately **17,177,220 m**.
At a steady **600 m/s**, that is about **7.95 hours**. This is a scale calculation,
not an arrival-time promise: acceleration, braking and existing travel rules still
apply. This scene change adds no fast-travel mechanic.

## Full-size fleet and circular berths

Ships keep their current body dimensions. Length is X, height is Y and width is Z;
the local frame points +X toward the bow, +Y up and +Z starboard. Articulated
turrets and other separate runtime appendages are not part of the body-height
measurement. Canonical Study05 source/assets and their hashes are unchanged;
Study06 is not adopted by this work.

Each circular platform reserves a disk of radius
`rho = hypot(length, width) / 2 + 12 m`. Its berth centre is `680 m + rho` from the
concourse centre. The table gives source-derived proposal values, rounded here
for reading; geometry calculations use the full precision values.

| Ship | Body length × width × height, m | Platform radius, m | Berth centre Y, m |
| --- | --- | ---: | ---: |
| Pioneer | 72 × 38 × 18 | 52.706 | 13 |
| Prospector | 180 × 92 × 64 | 113.074 | 36 |
| Hauler | 420 × 180 × 140 | 240.473 | 74 |
| Vanguard | 1,100 × 380 × 260 | 593.893 | 134 |
| Borealis | 640 × 260 × 180 | 357.398 | 94 |
| Stratum | 1,800 × 800 × 480 | 996.886 | 244 |
| Monolith | 5,200 × 2,200 × 1,400 | 2,835.119 | 704 |

The central concourse has a **680 m radius**. Stable slots follow the complete
class catalog, with tangent hull headings and class-specific radial distances;
removing a vessel from inventory does not repack the remaining berths. Angular
wedges reserve each platform's full disk with a **4° gap** between slots. Hull
footprints remain restricted even when a vessel is away.

The layout is translated so Pioneer's X/Z centre stays at **(0, 25) m** and its
boarding marker stays **(0, 0, 0) m**. Circular hulls hover **4 m above the walking
floor**, making Pioneer's centre **(0, 13, 25) m**. Boarding markers remain **6 m
from the inward body face**. An **8 m wide** radial bridge connects each platform
to the concourse; authoritative walking uses the same poses and routes as the
scene. Boarding remains an owned-vessel, nearby-marker action. The hover gap is
not a fitted physical airlock, animated ramp or new boarding mechanism.

The enclosure formula currently gives an outer radius of approximately
**6,390.238 m**, including **40 m wall clearance**, and a ceiling at **Y = 1,464 m**.
The ceiling allows **60 m above the highest body plus its hover gap** for runtime
turret articulation; body dimensions alone do not prove appendage clearance.
These remain provisional until the all-class rotated-hull/platform, route,
camera and enclosure checks are complete. The largest hull drives the envelope;
smaller ships are not enlarged to fill it. Scene population comes from actual
owned vessels, not decorative ships or invented occupants.

The circular concourse and platforms have walking surfaces at **Y = 0**. Their
rendered floor edges and the shared bridge endpoints must cover every legal
avatar centre. The decorative pit floor at **Y = -18 m** is not walkable. Real
boarding worklights supplement emissive windows and strips; their presence alone
does not establish GPU lighting or frame-time acceptance.

## Evidence still required

The shared definitions are in `crates/arkline-protocol/src/world_layout.rs` and
`fleet_layout.rs`; authority creation, surface and station paths consume those
definitions. Client views must agree with the authority on planet geometry,
berths, boarding and walking. Review legacy/new-world reopen, old-client refusal,
unchanged economic replay, all-class separation, path reachability and camera
clearance independently of visual appearance.

Native before/after observation is currently blocked by the locked Mac and needs
manual unlock. No final native pixels, walking/boarding journey, matched camera
comparison, screenshot delivery or measured frame-time result is claimed here.
Use a numbered build and disposable worlds for those checks. No PR publication,
merge, release or deployment has been performed by this scene workstream.
