# Pioneer cockpit ergonomics and cassette telemetry — 2026-10-23

Local review branch: `design/pioneer-ergonomics-20261023`, based on published
`f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068`. Experimental registration **8**.
No source publication, merge, deployment, old-world migration, or reactor-branch
integration is implied by this review checkpoint.

## User-directed changes

The annotated reference marks six empty areas: upright side bays (1/4), sloped
wing surfaces (2/5), and lower inner service fascia (3/6). The change uses the
upright bays for the existing permanent drone/mining displays and reworks the
supporting wings, rather than treating the seven moving camera screens as the
only monitors needing attention.

* Permanent side faces retain their full 0.84 × 0.44 m glass size. Their centers
  move from `[32.35, 1.03, ±1.50]` to `[33.70, 1.25, ±1.80]` metres, with yaw
  ±39° and the existing −15° pitch. They move **forward and up**, not physically
  inward: seated center bearing improves from about 60° to 39°.
* The pilot eye remains `[31.48, 1.35, 0]`; canonical 55° vertical FOV is unchanged.
  Full-glass pointer/depth checks use a 15° glance on desktop aspect ratios and
  25° on the narrow 800×650 case. Forward view does not contain every outer edge.
* Real pedestals join the raised faces to shallower, shorter working wedges.
  Long unused pale aft countertops become narrow dark service returns. Existing
  floor benches, rear entrance, seat motion and circulation boundaries remain.
* Both turret screens move forward from X=33.45 to 33.90 m and deploy at Y=1.55 m
  instead of 1.35 m. This clears NAV/RADAR at full deployment without hiding the
  center drone feed. The forward chassis has real open cassette wells rather
  than a solid countertop intersecting their travel.
* Seven independently driven camera mechanisms retain their identities,
  stow/deploy authority, dimensions and vertical motion. A moving turret can
  briefly cross another instrument during transit; stable endpoints are clear.

## Five fixed cassette metric bands

Each overhead cassette front carries a depth-tested read-only status band tied
to the same sticky drone ID as its camera. The lower portion prioritizes:

1. Class/ID and HP;
2. Actual velocity magnitude and task/target;
3. Authority, signal and mining-cycle progress, with stale/offline age replacing
   nominal status when appropriate.

There is no per-drone name or cargo capacity/stored-cargo field in this protocol.
The implementation does not invent these values or mislabel ship cargo as drone
storage. The compact band omits unsupported storage. Stale, paused, disconnected,
missing-assignment and missing-telemetry states are explicit. On-foot labels use
the same freshness policy as the other physical instruments.

Metrics add no camera captures. Camera sources remain capped at 11 and new
captures at one per presented frame. All five metric bands can be inspected
across ordinary upward/left/right glances; they are **not all fully readable in
one canonical forward frame**, and the connection overlay can cover a front-view
band. Capture tests warm the real camera cache after changing the viewing angle,
respecting the normal scheduler rather than fabricating live image content.

## Integration boundary

Surface **15 is reserved for the separately recovered reactor display**. This
isolated branch returns no display there. Metrics use named slots **16–20**, total
21. Reactor state, reactor room geometry and its dedicated screen are not present
in this f2-based branch and require deliberate integration with the separately
validated registration-7 recovery checkpoint. Registration 8 rejects old bindings
instead of silently rewriting saved worlds.

## Validation

* Protocol: 66 passed, including all seven moving-screen glass rays, 101-fraction
  mechanism collision/sweep checks, seat/circulation journeys, and new permanent
  face endpoint/side-face travel visibility checks.
* Client: 495 passed, 11 retained baseline failures, 30 ignored; no new failure.
  Includes edge-inclusive 25-point physical-glass projection/picking checks for
  all seven permanent faces at four viewport aspects with 8 px margins; fixed
  cassette placement, pointer mapping, stable identity and freshness tests.
* Server: 91 passed, 4 ignored. Fresh actual-request mining/warp and standing
  routine fixtures use registration 8; the isolated-old-world test is updated to
  assert revision 8 while still proving no migration.
* Verifier: 14 passed.
* Native GPU journey and workspace check pass. Screenshots are genuine engine
  captures, not concept renders. Inspection found and fixed a published-base bug:
  all camera callbacks were gated on visibility of legacy NAV surface 4. At a
  right/up glance that unrelated face was culled, suppressing the visible drone
  and turret feeds. Camera callbacks now run independently; only legacy primary
  unavailable overlays retain the legacy-face guard. No cache scheduling policy
  changed. The regression pose explicitly culls surface 4 and proves every
  remaining visible camera key refreshes using fresh accepted authority states.
  Right/up images were visually inspected after the fix. Software-rendering
  timings are not a desktop performance benchmark.

Not implemented here: captain manual screen stow controls, drone cargo storage,
network/mobile video streaming, reactor integration, seamless docking/unloading,
or new mechanical power/heat/mass simulation.
