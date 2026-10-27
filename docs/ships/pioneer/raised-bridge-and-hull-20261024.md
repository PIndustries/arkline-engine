# Pioneer raised bridge and hull refit — local development

This is a continuation of the whole-ship workstream in issue 62, based on the
preserved integrated checkpoint `28817c23ee0d5b6d84ffe207fe60317cff3afe13`.
The bridge was already on the lateral centerline. This refit raises the complete
inhabited assembly rather than moving the camera or claiming new recentering.

## Shared placement and access

The bridge uses a single body-space translation `[0, 8, 0]` metres. Its floor is
7.68 m, canonical seated eye 9.35 m, and interior roof 10.98 m. The seat, joints,
fixed consoles, reactor/cargo displays, nine camera mechanisms, cassette metrics,
input planes, collision bodies and upper walking supports retain their relative
placement. The pilot field of view remains unchanged.

Both main decks stay at their existing heights. Reactor machinery and cargo room
reservations do not rise with their bridge displays. Boarding lift and interlocked
hatches stay at their original body-space anchors. The lower forward service
foyer connects these to the main-deck corridor.

Three switchback flights provide a real walking route to the raised bridge.
Each has 15 treads, 0.32 m run, approximately 0.178 m rise and 1.2 m clear width.
Landings connect the flights; there is no instant teleport or new powered-lift
claim. The existing main-deck stairs and boarding lift remain functional.

## Exterior and equipment boundary

The matching canonical model replaces the old low bridge glazing with a lower
service shell, integrates a compact raised canopy and supporting deckhouse, and
physically opens the access tower. Openings are baked into the verified model;
obsolete view-specific triangle cuts must not be retained against new indices.
Actual exporter hashes and bounds are checked by the runtime/catalog binding.

Forward gun bores at the old hardpoints would intersect the raised bridge.
The refit therefore moves both physical mounting sockets and runtime turret,
muzzle and camera anchors outboard together, keeping existing equipment identity.
Visibility must be checked for the bores separately from their elevated cameras.
A clear static forward ray is not a new authoritative firing-collision system.

## Compatibility and retained systems

Registration 11 requires matching client/server geometry and fresh development
fixtures. Earlier registered bindings are rejected rather than silently moved.
No original user world is opened, rewritten or migrated by this work.

The integrated nine-camera order, five drone metric bands, cargo overview,
reactor mechanical configuration and AUTO / OPEN / STOWED captain policies are
retained. Monitor movement does not invent equipment power, heat, fuel or mass.
Camera views remain local rendered views, not mobile network streams.

## Model and clearance contract

The final canonical GLB is SHA-256
`26eb88de6e05ecc2b1f5e019642ef7a475186d0a0978f0b82e12296d0bd82ca7`;
the editable Blender source is
`c5a55fe2c7e37c501e188249ae7eec90e292f35fb251b5b21e361013df8633de`.
The verified model contains 710 parts and 105,344 triangles. Its actual vertical
bounds are approximately -9 to +11.35 m. ShipSpec uses a conservative 22.8 m
origin-centered envelope, not an invented exact modeled height. Berth and surface
placement use that envelope consistently. Both real turret sockets are at
[10.8, 9.864, ±5.2] m; cameras remain tied to those physical mounts.

A strict repeat export is byte-identical. Source clearance checks cover 4,971
route poses, 25 capsule-axis samples per pose, and separate seated/standing eye
and forward turret-bore rays. The retired low bridge recess is physically sealed:
40 closure probes hit hull, while the intended boarding aperture remains clear.
Side glazing has matching source panes, opaque frames and collision surfaces;
only duplicate procedural pane visuals are omitted, not opaque hull occluders.

## Acceptance record

Final combined validation uses the exact canonical source above. Protocol: 79 passed. Verifier: 14 passed. Server: 263 passed, zero failed,
seven opt-in ignored. Native workspace check and Clippy complete with warnings.
Client: 535 passed and 11 unchanged baseline failures; this is not a green
full-client-suite claim. All 240 exported authority payloads report registration
11, despite legacy revision names in some test-only directory/receipt labels.

The actual raised access route uses all 45 treads in both directions, both main
decks, boarding hatches/lift, seat entry/exit and a saved intermediate landing.
Private, fleet and rotated placements are covered. The obstruction regression
uses an explicitly constructed avatar intrusion; it is not a claim that a player
can walk into the hardware sweep from valid grounded supports.

This is a substantial functional model/blockout checkpoint, not finished flagship
art. Actual engine captures and server-response replay establish the tested
geometry and retained screen/control behavior, not desktop frame-rate performance
or new physical simulation. Older user worlds are preserved unchanged. Public
source review, merge, release and deployment are outside this local checkpoint.

Eight native-rendered suites pass: whole hull/bridge (12 captures), supported
access (7), nine-camera/state replay (38), captain controls (18), cargo (5),
reactor (38), full-face drone data (11), and telemetry/freshness (13): 142 receipt-verified
captures in total. These are actual engine images;
recorded input hashes refer to unchanged authoritative test responses. Dedicated
telemetry reconnect evidence passes. Reconnect replaces
the world with the connection UI; it must not be presented as fresh cockpit data.
