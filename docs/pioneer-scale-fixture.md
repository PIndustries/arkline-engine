# Pioneer hangar scale evidence

Issue #40, supporting #41 and #45. This bounded fixture starts from PR #47 at
`196ef4f8d43505b203b102a02af02b06729c4511` (package 26.10.14.49).
The user-reported hangar scale defect remains open pending calibrated native
observation. Do not rescale the 72 × 38 × 18 m body to fit a screenshot.

## Automated fixture

Run from the repository root with Rust 1.89 and the Linux dependencies listed in
`docs/player-guide.md` (or the supported native platform toolchain):

```sh
python3 tools/build.py test -p arkline-client --lib hangar_scale --locked
ARKLINE_SCALE_EVIDENCE_DIR=/tmp/arkline-scale-new \
  python3 tools/build.py test -p arkline-client --lib \
  hangar_scale_export_cpu_evidence --locked -- --ignored --nocapture
```

The output directory must not exist. The ignored export test writes `scale.json`
and three SVGs with private-bay CPU vertex projections. These are measurement
artifacts, not screenshots, shaded renders, collision validation or native
acceptance. Orthographic axes are explicitly labeled; each grid cell is 1 m at
10 pixels/m. The green 1.8 m reference and 1 m square are calibration glyphs,
not injected gameplay objects. The top view's reference remains a legend glyph,
not the top projection of an avatar. JSON records build, asset hash, actual final
instance matrices, body bounds, boarding anchors, fleet contents, viewport and
projection parameters. Keep these files with the source commit/test receipt.

Tests extract only embedded GLB mesh instances from the actual private-bay and
fleet render lists, then transform the CPU mesh vertices used by the renderer.
They require the embedded asset to decode, so procedural fallback cannot pass as
the Pioneer model. Hull composition and perspective projection helpers are used
by real rendering and by the tests. Dynamic turrets, fittings and collision
proxies are separate envelopes and deliberately excluded from body measurements.

Expected XYZ bounds in meters (XYZ means length/height/width):

| Scene | Minimum | Maximum | Body extent |
| --- | --- | --- | --- |
| Private bay, yaw 0 | −36, 0, −19 | 36, 18, 19 | 72, 18, 38 |
| Fleet berth, yaw 0 | −36, 0, 6 | 36, 18, 44 | 72, 18, 38 |

The private boarding anchor is (0, 0, 22), 3 m beyond the body. The fleet anchor
is (0, 0, 0), 6 m from the nearest body face. Both place the body bottom at floor
Y=0. Separate tests check yawed actual fleet instances in the intrinsic body frame
and verify that 90° yaw exchanges world X/Z extents. The mixed-hall case contains
Pioneer and Monolith; enlarging the hall must not resize Pioneer.

The neutral standing avatar is measured from its actual model at approximately
1.8 m high. The shared hangar perspective helper uses vertical FOV 55°, near
0.08 m and far 30,000 m. At a 1280 × 720 viewport, eye (0,9,100), target (0,9,0),
a vertical 1 m segment in the target plane projects to approximately 6.916 px.
That landmark is checked against the analytic perspective formula. This is a
fixed diagnostic pose; it does not describe an unknown running camera.

## Remaining P0-H0 native gate

Add an editor-only diagnostic overlay and locked measurement cameras using these
same render instances. Show the actual scene type, owned berths, camera pose,
projection/viewport, body versus articulated/collision envelopes, build and asset
hash. Include meter rulers on all three axes, an actual 1.8 m reference, 1 m cube,
floor grid, boarding and camera anchors. Capture Pioneer-only and mixed-fleet
scenes, front/side/top orthographic views and measured perspective views.

Compare observed pixels with projected landmarks from the same matrices. If
instance dimensions are correct but anchors, camera clipping or hall context are
misleading, fix that cause with measured evidence. Passing CPU bounds alone does
not establish that the user's visible scale is correct. Linux without a display
or GPU cannot establish native macOS acceptance. No authority, clock, room,
boarding permission, material, history or published hull dimension changes are
part of this fixture.
