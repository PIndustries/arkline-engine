# Runtime drone models

The three existing drone classes have original procedural exterior geometry in
[`drone_models.rs`](../apps/client/src/drone_models.rs). Recon has a low segmented
body, a forward optical assembly, small outboard sensor faces and a receiver mast.
Mining has a broader protected work housing, exposed braces and amber markings.
Attack has paired forward weapon housings, stepped armor and restrained rust-red
identification. All three retain a common emerald frame, dark recessed machinery,
graphite armor and small green optical/light surfaces.

These are static visual distinctions for the existing Recon, Mining and Attack
roles. The mining head follows the project's laser equipment direction; it is not
a drill. Mode, movement, targeting, effects and gameplay specifications remain
owned by the existing protocol and simulation. Visible receivers, braces and
housings do not introduce sensors, joints, armor ratings or new capacities.

## Coordinates and visible size

Models use the flight renderer's scene units: one scene unit is 100 meters, +X is
forward, +Y is up and +Z is starboard. Their vertex bounds retain the old body/pod
model envelope, from `(-0.025, -0.009, -0.030)` to
`(0.025, 0.009, 0.030)` scene units. This measures **5 meters long, 6 meters wide
and 1.8 meters high** in the rendered scene. It is a presentation measurement,
not a new protocol dimension, collision envelope or physical rating.

Static components are transformed once, including correctly transformed face
normals, then merged by color and emission into five batches per class. The
models are cached together and borrowed on subsequent requests. Each class has
at most 1,800 triangles. No per-frame model construction, external assets,
textures or new material system is required. The five batches use the existing
painted material response; their color roles do not claim distinct simulated
metal, glass or armor behavior.

The flight view, cockpit occlusion and editor model viewer must consume these
same cached triangle batches. World transforms continue to use the authoritative
interpolated drone position and heading. The existing camera-drone body exclusion
also remains in effect. The viewer may center and scale the shared model for
inspection; its displayed size must come from these measured bounds.

## References and validation

The construction palette and layered industrial forms follow the existing
[Pioneer asset record](../assets/ships/pioneer/README.md) and
[fleet design reference](ships/design-reference.md). The fleet reference explicitly
uses articulated mining lasers rather than drills. These references supply visual
language, not drone dimensions or newly implemented systems. The
[model contribution guidance](models.md) and
[Pioneer runtime contract](pioneer-model-runtime.md) explain the existing distinction
between visible geometry, actual runtime triangles and gameplay functionality.

Module tests check finite nondegenerate triangles, transformed unit normals,
retained visible bounds, batch/triangle budgets, distinct class geometry,
deterministic construction and reuse of the cached data. Native appearance and
viewer interaction still require inspection in the combined build, including
selection, orbit, zoom and reset for every class. Cockpit occlusion must be
validated against the same model data after integration.

## Editor inspection

Open **Model > Drone models** in the native editor. Choose Recon, Mining or Attack,
drag the canvas to orbit, scroll over it to zoom, and use **Reset framing** to
restore the fitted view. The same cached runtime meshes and color/emission values
are displayed with only a centered uniform inspection transform. Switching back
to Play view retains the selected model and leaves flight input released while
the viewer is open.
