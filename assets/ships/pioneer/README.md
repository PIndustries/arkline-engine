# Pioneer authored hull

## Local physical drone-bay registration 12 — 24 October 2026

The current local master adds an aft unmanned bay with15 supported home cradles,
individual trays, a shared handling gantry/lift/forks and a split rear door. The
raised bridge, two-deck crew route, port boarding aperture and turret mounts remain
unchanged. All709 old non-Pressure-hull parts retain exact exported positions and
normals. Only the Pressure hull receives the new cavity/aperture/pockets;243 parts
are added. This is source adoption, not completed stock/launch/runtime acceptance.

The exact export has131,438 triangles, nine original materials and953 source parts.
The source-bound moving partition has22 groups,71 parts and7,668 triangles. A naive
per-group/material runtime split is43 moving batches plus up to9 static batches,
not nine draw calls. Body bounds and72×20.35×38m extent remain unchanged.

`drone-bay-authoring.json` defines the real geometry and handling sequence;
`drone-bay-runtime-binding.json` maps each moving part to exact exported triangles.
Apply current body-space group pose times inverse reference pose exactly once.
Stored drone meshes are not baked into the hull: persistent authority entities
must be rendered separately, retaining ID/health/home cradle through release and
recovery. No acquisition, replenishment, crew-access or physical power/force model
is supplied by this asset.

267 continuous static sweeps and426,000 relative-motion checks pass, with closed
door/missing-release negative controls. The original4,971-pose crew/canopy/port
aperture proof also passes on this exact master with minimum0.42m source clearance.
Receipts are listed in `pioneer-adoption.json`; dynamic native camera/depth/picking,
stock conservation and runtime performance remain separate integration gates.
Registration12 requires its matching fresh preview; older bound saves are preserved
and rejected, never rewritten or silently initialized.

Reproduce with `tools/art/build_pioneer_drone_bay.py` from the exact preserved
revision11 master under official bpy5.0.1, then strict `export_pioneer.py` and
`bind_pioneer_drone_bay.py`. New output paths are required. The prior
`raised-bridge-assembly.json`, `raised-bridge-authoring.json` and
`raised-bridge-source-clearance.json` remain historical revision11 evidence;
`drone-bay-bridge-source-clearance.json` is the refreshed registration12 crew proof.
No public source publication, merge or deployment occurred.

`pioneer-hull.png` and the following revision11/Study05 sections retain historical
authoring context; that old preview is not a registration12 runtime screenshot.

## Local raised-bridge revision 11 — 24 October 2026

The revision11 local source was a raised centerline bridge refit of the preserved Study05 master. The complete cabin is registered 8 m higher, with a compact front canopy, larger side apertures, structural shoulders and a tapered aft access tower. Canonical hull cavities now connect both decks, the lower boarding/service route and the three-flight bridge stair. The former low glazing is closed by opaque service panels; there is no second low bridge. Turret sockets move to Z ±5.2 m on supported pads to clear the raised cabin.

The body origin is unchanged. Bounds are `[-36,-9,-19]` to `[36,11.35,19]` metres: **72 × 20.35 × 38 m**, not a vertically centered 18 m envelope. The export contains **105,344 triangles**, nine material batches and 710 source parts. New source/asset hashes and explicit registration11 reject incompatible bindings; no saved-world migration is performed. Source and neutral Blender checks are not combined native acceptance or deployment. See `pioneer-adoption.json` for current evidence and preserved previous adoption.

Reproduce from the preserved original `studies/study05/pioneer-study05.blend` using `tools/art/build_pioneer_raised_bridge.py --source ... --output /new/path/pioneer-raised-bridge.blend` under official bpy5.0.1, then export the new source with `tools/art/export_pioneer.py`. The authoring script verifies the original source hash and refuses output overwrite. `tools/art/validate_raised_bridge_source.py --source ... --output ...` audits source-triangle clearance along the registered bridge access path.

The following Study05 text records the previous authoring baseline; its old dimensions and acceptance do not describe revision11.

`pioneer.blend` is the editable source model. `pioneer.glb` is the material-batched runtime export. The original construction recipe, `tools/art/build_pioneer.py`, refuses to run when a source, export or evidence render already exists. The model and generator are original project contributions under the repository license; no downloaded artwork or textures are included.

The construction recipe is only for an empty asset destination. To export an existing saved master, use the workflow below.

```sh
blender --background --python tools/art/build_pioneer.py
```

The model uses meters with +X toward the bow, +Y upward and +Z starboard. The hull envelope is 72 × 18 × 38 meters (length × height × width), centered on the origin. The editable source contains named armor panels, structural ribs, radiator blades, pipes, cargo hatches, drive bells, running lights, landing gear and turret sockets. Runtime export batches triangles by material to limit draw calls.

## Exporting a saved master

Select the saved `.blend` explicitly and provide a new output directory outside the repository. The bounded worker requires Blender **5.0.1** and disables file auto-execution. It reads the saved master rather than rerunning the construction recipe:

```sh
python3 tools/art/export_pioneer.py \
  --source assets/ships/pioneer/pioneer.blend \
  --output-dir /absolute/new/pioneer-review \
  --degenerate-policy drop \
  --blender /path/to/blender-5.0.1
```

An official Blender Python module installation can use `--bpy-python /path/to/python-with-bpy` instead of `--blender`. The cloud installation uses `/workspace/toolchains/bpy501`.

The directory must not exist. An existing directory, file or symlink is refused. Successful output contains an exact editable source copy with its original filename, `pioneer.glb`, `receipt.json` and `worker.log`. A failed attempt can leave its private source copy and log for inspection; use a different new directory for a retry. The selected source and exporter recipes are hashed before and after successful or failed work. No source is saved, overwritten, joined, rebuilt or installed by this workflow.

The exporter reads evaluated loop triangles and their corner normals, including weighted and custom normals. It bakes positive invertible object transforms into body metres and applies their inverse transpose to normals. Cyclic corner rotations and face sorting make the output deterministic while retaining winding and each normal's corner. It does not recalculate normals or decide which surfaces should face outward. The source must retain the centered 72 × 18 × 38 m envelope within 1 mm.

Degenerate handling is an explicit choice. `--degenerate-policy reject` fails on any triangle that fails the engine's scaled float32 predicate. `drop` omits those triangles and records their source part, evaluated triangle index and measured value. The predicate uses float32 body positions divided by 100, then float32 edges, cross product and squared length `<= 1e-20` in renderer scene units. It does not use the unrelated `1e-20` body-metre analysis cutoff. The actual Rust decoder and native runtime remain independent acceptance checks.

The supported source is deliberately small: one scene/view layer, visible unparented meshes, one material slot per part, live Bevel/Weighted Normal modifiers, and constant Principled BSDF → Material Output materials. Animation, drivers, constraints, instancing, shape keys, external resources, other shader nodes or authored unsupported Principled inputs, disabled/other modifiers, and singular or negative-determinant transforms are rejected. The internal empty Render Result image is allowed; it supplies no texture or external file.

The nine exact material names use the canonical GLB slot order: Graphite ceramic armor, Titanium machinery, Recessed carbon, Emerald structural frames, Drive emission, Working lights, Navigation glass, Safety amber, Hull identification ivory. Canonicalization changes JSON material references and ordering while preserving BIN bytes. Each receipt maps source-hash-and-name part identities to material primitive/index ranges; these are snapshot identities, not durable IDs across edited source revisions. The untouched editable copy retains the original object and mesh names.

The generated GLB is a review candidate. Its hash does not retarget existing material graphs or install it as the game hull. Adoption, native appearance and powered-exterior checks belong to a separate reviewed engine change.

Run the standard-library checks and optional Blender fixtures:

```sh
python3 tools/art/test_pioneer_export_contract.py
PIONEER_BPY_PYTHON=/path/to/python-with-bpy python3 tools/art/test_pioneer_export_contract.py
```

The engine supplies the moving mining turrets. Their mount centers are `(10.8, 9.864, ±2.66)` meters; the hull mounting surface is at Y=9. Aft drive centers are `(-35.9, 0, ±12.5)` meters. Decorative green paint and lamps are not thrust emitters.

## Relationship to the ship plans

The current canonical source is Study05, a cloud fidelity pass derived from the preserved Study04 master. It adds a shallower layered bow, actual recessed glazing, a varied recessed service/work deck and openings around the existing drive bells. Overall v05 matching remains unfinished. It preserves the documented hull envelope and reserves the declared two-deck inhabited core. The construction palette, armored side machinery and paired roof mining mounts follow the visual direction in the Pioneer concept collection.

The following remain explicit work, not implemented features of this asset:

- The rendered interior is a separate gameplay scene. It is not yet cut into this exterior mesh with functioning exterior apertures.
- Concept images place bridge glazing differently from the current room plan. The decorative forward glazing does not establish the physical bridge location or cockpit camera position.
- The 18-meter height describes the hull. Articulated turrets extend above it; the concept label describing 18 meters overall needs reconciliation.
- Pipes, radiators, lights, gear and hatches are visual geometry. They do not establish conserved thermal, fluid, electrical, pressure or structural simulation.
- There is no new collision mesh, LOD chain, skeletal rig, texture set or physically rated structural model in this change.
- Other ship classes still use their existing models. They must be authored and reviewed individually.

`pioneer-hull.png` is an actual flight-engine capture on cloud build26.10.15.7040, including both existing gameplay turrets and accepted powered exterior. [Matched before/after engine evidence](../../../docs/ships/pioneer/images/runtime-study05-20261015) and [refined source/export proof](../../../docs/ships/pioneer/images/runtime-study05-20261015/audit-refined/summary.md) record its scope.

## Industrial detail pass

The hull now separates the inhabited core from the stepped outboard pods. Fore and aft collars, recessed side service equipment, pressure couplings, harnesses, radiator banks, handrails, access ladders and permanent identification markings add scale without changing the published envelope or moving gameplay hardpoints. Text is exported as geometry, not a texture. Export triangulates evaluated surfaces and removes zero-area faces before material batching.

These exterior utility details remain visual assemblies. The connected room implementation is separately defined and validated; an external pipe or radiator must not be described as operational merely because its model is visible. The runtime still uses scalar materials, with no normal, roughness or occlusion textures.

## Canonical cloud adoption · Study05 · 2026-10-15

Study05 preserves the 72×18×38 m XYZ envelope, existing dynamic turret/drive registrations and nine canonical material identities. The editable source SHA is `61eb70e1…`, with 686 named parts. The refined runtime GLB is `8125644f…`, 103,466 triangles. [Export receipt](pioneer-export.json), [adoption record](pioneer-adoption.json), [source-authoring record](studies/study05/study05.json) and [fixture provenance](exterior-mounts.json) retain exact source/recipe/part spans and assumptions. The canonical master is byte-identical to receipt-selected `pioneer-study05.blend`. Study04 and both first/refined Study05 sources remain in Git history.

Three separate beveled brow plates have actual joints and a recessed maintenance cover; cockpit panes sit in cut hull recesses with continuous gaskets and connected braces. The service roof has distinct access/exchanger/stowed-arm areas. Drive caps are actually opened, with counterbored flanges revealing the existing bells/throats. These are inferred static visual architecture: the service hatches and handling arm are not operable, and no pressure, structural, propulsion or fitted-interior claim follows.

Glazing remains the supported opaque scalar-PBR approximation. Its metallic0.60/roughness0.12 and restrained acceptedWindow emission0.025 reduce the screenlike white appearance; fixture IDs/kinds/12Wloads, actual switch/clock behavior, total88W load and pulse/drive semantics are preserved. No transparent or reflected fitted cockpit exists. Visible Window surface comes from the authored glazing rather than raised procedural cuboids.

Existing old-hash material documents retain their binding and are rejected until explicitly replaced/rebound. Worlds with accepted prior-hash exteriors fail without rewriting their world, clock or project files. Use a new preview project or retain the matching old build; no automatic migration is supplied. Broader side/collar/underside fidelity, richer materials, moving assemblies and independent source-part emitters remain separate work.
