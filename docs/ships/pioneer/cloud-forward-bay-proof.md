# Forward-bay proof in the cloud

The migrated development checkpoint is `3d1129e543824a7e1c515aa03f2af54c2c1f4be7`.
It contains `b5f090a` in its ancestry, including the current isolated study and
the unintegrated editor console draft. All 110 recovery archive files matched
their manifest hashes in the cloud. `main` at `78fdc5d` is an ancestor; no merge
or replacement of newer source was needed. The migration branch remains a checkpoint.

The migration checkpoint's study was 16 meshes and 3,256 triangles, with GLB
SHA-256 `4196f82270e1b8fd773cd84d94c62d9792dc6eb4e67f7cbe88abbe8506e00b13`.
The [cloud enclosure milestone](forward-bay-enclosure-20261022.md) now has
22 meshes and 2,872 triangles, GLB SHA-256
`7a2dbb0c78fe734a368eb2b697ab3c6e6dbad934245df77a87e3119baf79cd46`.
Earlier 8-mesh/1,120-triangle descriptions do not describe either asset.

The same imported-mesh test now runs on Linux using the existing private Mesa
EGL helper, or on macOS using its existing CGL helper. Neither opens an app window
or a saved world. Linux requires Rust 1.89, the repository's native build libraries,
`libEGL.so.1`, and a working OpenGL Mesa driver. Software llvmpipe is sufficient;
cloud context preparation verified EGL 1.5 and OpenGL 4.5.

Physical DETAILS now uses a compact navigation row. The full operations search
and navigation previously pushed content below the display's 322 px content
limit, leaving nothing usable to scroll. Navigation, Modules, Drones, Mining and
Cargo remain directly available on the physical display; the existing Expand
panel button retains the full operations workspace. Input journeys enter through
DETAILS, scroll inside the current content area, and open collapsed text orders
with a real click. Their hit, command, focus, clipboard and timeout assertions
remain in place. Synthetic fixture freshness is held independently of CPU render
duration; production freshness checks are unchanged.

Supply a JSON file containing previously confirmed `charge_j`, `ore`, and
`location_kind`, retaining its provenance. The values are recorded input, never
live telemetry. The runner validates the input and refuses existing output paths.

```sh
LIBGL_ALWAYS_SOFTWARE=1 EGL_PLATFORM=surfaceless MESA_SHADER_CACHE_DISABLE=true \
  python3 tools/run-forward-bay-proof.py \
  --telemetry /absolute/path/to/recorded-values.json \
  --output /absolute/path/to/new-evidence-directory
```

The test renders seated and standing views through the existing import preview
and cockpit display code. It verifies the study hash, named component identities,
triangle count, authored normals, component bounds, and conservative capsule
clearance samples. Actual egui text bounds must fit the display without pairwise
overlap. Five display points must round-trip through mesh-aware picking, and
projected label/value em sizes must remain at least 14/24 pixels in both views.
These last two criteria previously appeared only as receipt booleans; they now
fail the test when false. The receipt is retained for inspecting those failures.

Each successful run retains `seated.png`, `standing.png`, `receipt.json` and
`run.json` locally. The receipt identifies renderer/GL version and input hashes;
the run record identifies the source commit and whether the source was dirty.
No files are uploaded. The Mac is unnecessary for this workflow.

Passing these checks does not establish human readability, approved concept
fidelity, production enclosure/collision, live command mapping, or visual
acceptance. The canonical cockpit geometry, renderer materials/backlight, user saves,
avatars, jobs and ledgers are unchanged. Separate denied operations remain outside
this workflow.

## Cloud validation, 2026-10-22

This aggregate result predates the isolated enclosure revision; that milestone
records its focused validation separately.

The normal Rust 1.89 client library suite reports **465 passed, 11 failed,
23 ignored**. All physical console input journeys pass, including text focus,
IME rejection, clipboard forwarding, real fleet command dispatch and the
unchanged 20-second child-process deadline. The focused integration module
reports **5 passed, 0 failed, 1 ignored**; its helper is executed by the parent
deadline test. An experimental unoptimized test build exceeded that deadline;
no override or increased timeout is retained.

The 11 unresolved aggregate failures are outside this change's input journeys:

- Two Pioneer interior geometry assertions: room cutaway part count and removed
  room prototype-fixture appearance.
- On-foot camera selection and station gathering-readiness assertions.
- Seven legacy UI assertions: target-lock confirmation, drone control layout,
  fleet selection, hangar capacitor text, on-foot mining readiness, full-hold
  status text and overview dragging.

They remain failures; this milestone does not claim a green full suite. Broad
formatting also reports inherited differences in the migrated source, and the
client emits five existing deprecation/dead-code warnings. Unrelated source is
left intact.

Portable Python checks report **59 passed, 3 skipped** across the study,
build-version, build-guard, packaging, publication and export-contract checks.
The skipped cases require the official Blender integration runtime. The new
study regression is included in the existing Linux CI test script.

The explicit Linux EGL proof passes: minimum seated label/value em sizes are
**32.25/56.51 px** and standing sizes **14.84/25.95 px**, with all five sampled
display points clear in both views. Locally inspected images show legible
recorded values without clipping, but a sparse surround, heavy central housing
and flat preview shading remain. No approved concept image was supplied to this
executor, so no pixel-match or completed visual fidelity is claimed.
