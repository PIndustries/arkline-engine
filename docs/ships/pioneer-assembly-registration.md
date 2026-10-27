# Pioneer source-assembly inspection baseline

**Status: derived inspection evidence, not an authoritative assembly or engineering model.**
This implements the source-registration portion of the [design/runtime contract](design-runtime-contract.md).
The saved [Pioneer source](../../assets/ships/pioneer/README.md) remains authoritative for
visual geometry. The inspector reads that file; it does not run the generator, change
assets, invent a hierarchy, modify runtime behavior, or establish physical ratings.

## Reproduce

Use Python 3.10+ and a compatible Blender 5.x executable. The checked-in source uses
the Blender 5 file format; Blender 4.3.2 cannot read it. Verification below used the
[official Blender 5.0.1 Linux x64 build](https://download.blender.org/release/Blender5.0/blender-5.0.1-linux-x64.tar.xz),
build `a3db93c5b259`. Its archive SHA-256 is
`8019580ee1b7262e505f4196a00237ccf743c88d205b38d34201510676e60b09`, matching the
[official checksum file](https://download.blender.org/release/Blender5.0/blender-5.0.1.sha256).
No system installation is required.

From the repository root, with `BLENDER` set to a compatible executable:

```sh
export BLENDER=/path/to/blender
PYTHONDONTWRITEBYTECODE=1 python3 tools/art/export_pioneer_assembly.py \
  --blender "$BLENDER" --output /tmp/pioneer-inspection-a.json
PYTHONDONTWRITEBYTECODE=1 python3 tools/art/export_pioneer_assembly.py \
  --blender "$BLENDER" --output /tmp/pioneer-inspection-b.json
cmp /tmp/pioneer-inspection-a.json /tmp/pioneer-inspection-b.json
python3 tools/art/pioneer_assembly.py /tmp/pioneer-inspection-a.json
```

Output files must be new files outside the repository. Existing files, symlinks and
repository destinations are rejected rather than overwritten. `--output -` emits
canonical JSON on stdout; Blender logs stay separate. Do not commit manifests as a
second source model. Re-extract after changing any registered source or recipe.

The launcher starts a separate, background Blender process with `--factory-startup`,
`--disable-autoexec`, two threads and an isolated configuration directory. It never
saves or overwrites the blend. Embedded Blender scripts are not executed. The user's
open editor and desktop are not touched. Use the ordinary Python launcher, rather
than the internal `--worker` entry point, to apply its subprocess limits.

## Registered evidence

Schema: `arkline.pioneer-assembly-inspection/v1`.

- SHA-256, project-relative path and byte count for the saved `.blend`, its generator,
  asset README, design/runtime contract, extractor and validator. Fresh hashes are
  checked before and after extraction and again during standalone validation.
- Blender version, build hash, source-file version, saved frame/subframe, scene,
  view layer, unit settings and evaluation policy. No timestamps or host paths are
  included, so identical inputs/settings on the same Blender build produce the
  same canonical JSON bytes. Cross-version mesh evaluation is not assumed equal.
- Every `bpy.data.objects` object, in exact source-name order, with its source name,
  type and data-block name. Source object count is not inferred from the generator.
- Parent reference, source basis, parent inverse, parent-relative local matrix,
  source world matrix, evaluated world matrix and evaluated object-local-to-body
  matrix. Full 4×4 affine matrices retain scale/shear; no Euler decomposition is used.
- Every actual material data block, including unused ones, with source name,
  viewport RGBA and node-use flag. Object and evaluated material slots reference
  those records. Polygon counts retain the evaluated slot assignment. Colors and
  material names are visual evidence, not density or engineering specifications.
- Per-object evaluated vertex/polygon counts and local, world and body-meter AABBs.
  Bounds scan actual evaluated vertices after enabled viewport modifiers; they are
  not transformed corners of an unevaluated local box. The inspection does not
  perform runtime triangulation, degeneracy removal or material batching, so these
  are not runtime GLB triangle counts. Render-only modifier differences are visible
  in each recorded modifier's flags, but this is viewport evaluation.
- Aggregate measured bounds, independent of the declared hull envelope. Validation
  reports each min/max disagreement at a fixed 1 mm comparison tolerance. A mismatch
  remains visible; it does not cause fitting or rescaling. This tolerance is an
  inspection parameter, not manufacturing accuracy.

### Identity and completeness limits

Object/material IDs are `kind:sha256(kind + NUL + blend_sha256 + NUL + exact_name)`.
They identify a source snapshot, not durable design components. Renaming an object
changes its ID. Any saved source-byte change changes all snapshot IDs, even for
unchanged objects. No UUIDs are written into the source, and these IDs do not satisfy
the contract's future rename/undo/branch-stable component identity requirement.

The standalone stdlib validator checks fresh source hashes and consistency of the
recorded inventory, transforms, materials, counts and bounds. A valid report is not
cryptographic proof that recorded geometry or object coverage was faithfully
extracted from Blender. Reproducible re-extraction and the independent real-source
inventory test provide that evidence. A maliciously edited, internally consistent
manifest is not authenticated by a matching source hash.

The supported source is one static, local, fully visible scene with one view layer.
All source objects must be in that evaluated scene. Linked libraries, detached or
excluded objects, hidden viewport geometry, animation/drivers, constraints,
non-object parenting, instancing and modifiers other than the current bevel and
weighted-normal types fail closed. These require an explicit future schema/evaluation
policy; they are never silently skipped. Non-geometry objects are registered with
null geometry. A zero-vertex mesh has null bounds. Singular object scales are retained
as legal affine data; no physical validity is inferred from them.

## Frame registration

The authoring recipe maps body points `(x, y, z)` to Blender `(x, -z, y)`.
The inverse maps Blender world points `(x, y, z)` to body meters `(x, z, -y)`.
Both frames are right-handed and share the saved world origin. One source unit is
one meter, explicitly supported by the generator/README and checked against the
scene's unit scale. No conversion based merely on field ordering is permitted.

- Blender world: +X forward, +Y port, +Z up
- Body meters: +X forward, +Y up, +Z starboard
- Declared body XYZ bounds: min `(-36, -9, -19)`, max `(36, 9, 19)` meters
- Declared XYZ dimensions: `(72, 18, 38)` meters; dossier LWH ordering is `(72, 38, 18)`

The object's local basis is deliberately unchanged Blender object-local coordinates.
If `C` is the Blender-world-to-body conversion and `W` is evaluated Blender world,
`body_m_from_object_local = C × W`. This is not `C × W × inverse(C)`, which would
also change the local basis. For ordinary object parenting,
`local = parent_inverse × basis` and `world = parent_world × local`; an unparented
object uses `local = basis`. The validator tests those relationships and rejects
cycles or missing parents.

Runtime rendering's separate meters-to-render-units division by 100 is **not**
applied. Tests cover frame round trips, both actual source turret-socket centers,
and documented body points. The source socket cylinders are centered at
`(10.8, 8.85, ±2.66)` m and reach the Y=9 hull mount surface. The runtime articulated
mount centers at Y=9.864 are distinct; this inspector does not invent them as source
objects. Boarding/door socket registration and runtime socket reconciliation remain
future work because the present exterior source does not establish those semantics.

## Verified source snapshot

Source `.blend` SHA-256:
`8ec754fc7afe6b67dcab6cbd6694bb04bdc03cc12e91ee96e0bca9e08bb24f6f`.

With Blender 5.0.1:

- 524 source objects; all 524 are meshes
- 0 parented objects: the saved visual source has no assembly hierarchy to preserve
- 10 material data blocks, including the unused default `Material` data block
- 34,790 evaluated vertices and 32,276 evaluated polygons
- Measured body bounds `(-36, -9, -19)` to `(36, 9, 19)` m
- Measured XYZ dimensions `72 × 18 × 38` m; declared envelope matches without rescale
- Repeated independent extraction produces byte-identical canonical manifests

These are inspection results for the named saved snapshot, not claims that the ship
is concept-complete, fitted internally, watertight, structurally valid, collidable,
or operational. A useful next gate is persistent semantic part IDs and an explicitly
authored hierarchy, under a separately coordinated source-editing scope.

## Physical properties remain unresolved

Mass, density, pressure, strength and thermal ratings are all explicit JSON `null`
with status `unresolved`, both globally and per object. Zero is not used as a
placeholder. Neither a box nor a mesh surface is material volume. Visual titanium,
lights, coolant vessels and pipes do not establish composition, wall thickness,
heat rejection, luminosity, power demand, pressure capacity or connected systems.
The inspector performs no mass integration and grants no physics authority to the
existing candidate engineering document or runtime flight model.

## Resource and input safety

The launcher limits its Blender subprocess to 180 seconds wall time, 120 seconds
CPU time, 6 GiB address space, 256 open files, no core dumps and 32 MiB per output
file. It limits threads to two. The extractor additionally caps 10,000 objects,
512 materials, 2 million vertices per object, 10 million total vertices and
10 million total polygons. Evaluated mesh allocations happen inside the bounded
subprocess before their counts can be checked. Extraction currently requires a
POSIX host for these limits; validation is Python-stdlib-only and portable.

The validator caps input at 32 MiB, each registered source at 128 MiB, JSON depth at
64 and visited JSON values at 2 million. It rejects duplicate JSON keys,
nonfinite numbers, malformed/non-affine matrices, missing or duplicate IDs,
cycles, stale hashes, unknown material references, inconsistent counts and bounds,
and altered fixed frame/limit declarations. Source paths must match the registered
project-relative paths; absolute paths, traversal, backslashes, drive-like paths
and resolved symlink escapes are rejected. These guards do not make Blender a
sandbox for arbitrary hostile files. Use the reviewed project source.

## Versioned acceptance plan: inspection-v1

```sh
# 26 stdlib unit cases; 4 Blender integration cases are explicitly skipped.
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s tools/tests -p test_pioneer_assembly.py -v

# All 30 cases, including the actual checked-in source:
PYTHONDONTWRITEBYTECODE=1 ARKLINE_BLENDER="$BLENDER" python3 -m unittest discover \
  -s tools/tests -p test_pioneer_assembly.py -v

git diff --check
```

The real-source tests run extraction twice, compare exact bytes, independently
enumerate Blender object/material names and parents, verify both source turret
socket centers and frame conversion, and check source/recipe hashes stayed
unchanged. They also test output overwrite/source-destination refusal. Unit cases
cover hierarchy/basis/parent-inverse relations, affine/frame round trips,
snapshot identity semantics, stale blend/recipe hashes, duplicate IDs/JSON keys,
cycles, missing parents/materials, nonfinite values, path/symlink escapes, resource
limits, unknown physics, scene-unit conflicts and mismatch reporting without rescale.

This is a focused tooling check. Rust workspace tests, UI behavior, runtime loading,
collision, fitted interior and physics validation are separate checks and are not
claimed by this inspection baseline.
