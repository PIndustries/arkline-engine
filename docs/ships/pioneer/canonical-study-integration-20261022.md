# Forward-bay study: canonical integration decision

The enclosed study at `c198ee0` is not a drop-in replacement for the registered
Pioneer cockpit. Keep it isolated until it is rebuilt around the existing deck,
seat, access route and physical input frames. No chair or rear-wall geometry is
added by this audit: those additions would compound measured access conflicts.
This is an integration decision, not a request to alter saved ship coordinates.

Reproduce the read-only measurements from the repository root:

```sh
cargo run --locked -p arkline-protocol --example pioneer_study_fit
```

The example reads the study manifest and current authoritative protocol geometry;
it writes JSON to stdout only. The accompanying JSON records this audit's result
for registration revision 4, source SHA-256
`8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123`.
It does not validate arbitrary rotations/scales, triangle collision, registration
admission or visual quality.

## Measured conflicts

Aligning the study's seated eye with the canonical eye requires translation
`[33.22, 0, 3]` metres. Its floor then lies at Y=0, **320 mm above** the registered
deck at Y=-0.32. Aligning the floor instead lowers the seated eye by 320 mm.
A translation preserving up and scale cannot satisfy both constraints.

The registered rear portal maps to study coordinates `[-1.62, 0.88, -3]`.
Its Z interval `[-3.7, -2.3]` lies entirely outside the study deck's `[-1.6, 1.6]`
width. The study port wall crosses the existing seat-to-port circulation route.
Closing the study rear would not connect this offset entrance. Importing visual
walls alone would also disagree with the authoritative collision/support model.

The study has one vertical instrument, approximately 600 by 350 mm, rendered on
a 400 by 240 canvas. The game has five physical slots and a 560 by 380 canvas.
The mapped study instrument center is **248 mm aft and 50 mm above** the central
canonical MFD; the latter is pitched -50 degrees. Its substrate would sit in
front of the existing display. Keeping either input planes or visible geometry
unchanged while appending the study is therefore insufficient.

The existing chair already has authoritative wallplate, backrest, hinges,
deck support and an actuated pan with a continuous stow recess. Its root is
`[33.54, -0.32, 3]`; the egress anchor is `[33.36, -0.32, 3]`. A static study chair
would not reproduce that lifecycle or establish collision clearance.

## Smallest safe integration path

Regenerate the enclosure using revision-4 deck and seat coordinates, with an
explicit port-side connection to the registered rear portal and boarding route.
Preserve the existing folding-chair sweep. Fit the console around the five
canonical display frames, or deliberately update visible faces and input
projection together with readability and pointer regression coverage.

The actual game draws registered `canonical_pioneer_parts` through `flight3d`
and `pioneer_docked_scene`; the study currently uses only `imported_asset`.
Integration must map structural identities into that registered model and
validate matching collision/support, private and fleet placement, seat/egress,
interlocked boarding and persisted binding compatibility before adoption.
This audit changes none of those runtime paths, registration values or saves.

## Blank seated window

The imported-asset preview clears to RGB `(0.025, 0.04, 0.05)` and draws only the
imported meshes. It has no exterior world or sky pass. In the retained seated
render, aperture samples `(750,120)`, `(750,250)`, `(450,250)` and `(1050,250)`
are exactly `(6,10,13,255)`, the quantized clear color. The blank field is empty
preview background through the opening. It does not justify filling the window
or moving the camera. No renderer, material or lighting change is included.

Existing canonical circulation and boarding-support tests report **2 passed,
0 failed, 0 ignored**. The continuous chair test reports **1 passed, 0 failed,
0 ignored** (1,001 fold samples). Reproduce with:

```sh
cargo test --locked -p arkline-protocol --lib fitted_ -- --test-threads=2
cargo test --locked -p arkline-protocol --lib pilot_joints_and_pan_have_fixed_roots_and_a_real_continuous_stow_recess -- --test-threads=1
```

These results establish the existing model's constraints,
not compatibility of the isolated study. The eleven separately evidenced
baseline client failures remain outside this audit. CI lookup remains unavailable
after the earlier Forbidden response and was not retried.
