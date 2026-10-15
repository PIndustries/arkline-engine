# Project GLB preview

The native editor’s **Assets** workspace imports one static GLB into a local project preview. It
renders the imported triangles and scalar material bindings using an independent
renderer. It does not replace the live ship or create world entities.

1. Place a GLB inside the project directory, then enter its project-relative path
   (for example `Source/pioneer.glb`) and choose **Inspect GLB**.
2. Read the staged geometry report. Bounds and XYZ dimensions are measured from
   transformed vertices in meters; the original origin is preserved. The preview
   interprets +X as forward, +Y as up and +Z as starboard. Export into that frame
   before importing. It does not rotate or rescale an asset to fit a ship class.
3. Drag to orbit and scroll to move the camera. The white X ruler projects a real
   meter distance with the same camera. The preview uses simple lit scalar
   materials, not the full game lighting or a physical material certification.
4. Enter source/license references if known. Blank references mean unknown.
   **Accept preview** makes the staged bytes the local accepted revision.
   This is a content operation, not a command to the simulation.
5. **Save assets** persists the accepted snapshot. **Reload assets** reloads the
   saved reference and verifies/decodes its bytes again. Reopening does the same.
   Later accepted edits remain unsaved if an older submitted Save finishes.

The initial asset slot keeps the same asset ID across replacements and increments
its revision/hash. To replace it, inspect another GLB, review its changed geometry
and accept/save it. **Cancel staged** discards a candidate without touching the
accepted or saved revision. Saved source/license references belong to that accepted
revision; editing candidate references alone does not change the saved record.

## Supported content

The bounded shared decoder accepts one embedded buffer, static indexed triangles,
positions/normals and opaque untextured scalar materials. Limits remain 16 MiB,
4,096 nodes/visits, 128 meshes/primitives, depth 32 and 600,000 triangle indices.
Material metadata is capped at 512 records and 1,024 bytes per name. Only the
default scene is previewed; other scenes and unreferenced geometry are reported
as ignored. Textures, images, animation, skins, morphs, cameras, lights, external buffers and
unsupported extensions are rejected. Additional UV/color/tangent/weight attributes
are explicitly reported as ignored; they do not influence the preview. Positions
must stay within ten million meters per axis and have a total span of at least
1 mm. Missing density, mass, collision and engineering ratings stay unknown.

The report names actual material batches. The checked-in Pioneer source contains
524 meshes with no authored parent hierarchy; its runtime GLB joins objects by
material. Importing that hull does not recover 524 editable construction parts.
PR54's source inventory is separate inspection evidence. Persistent source-part
identity, authored hierarchy, source-to-runtime correspondence, section/exploded
editing and a fitted interior remain later assembly work.

## Persistence and recovery

The versioned index is `Assets/ImportedAssets.json`. Verified bytes are stored as
`Assets/Imported/<sha256>.glb` before an atomic index publication. The index records
asset ID/revision, hash, explicit frame, source/license references and report
version. Reopen verifies the hash and derives the report from the bytes again.
Paths are project-relative, ordinary files/directories; traversal and symlinks are
rejected. These operations assume a trusted local project directory and do not
protect against an adversarial process swapping directories during a file action.

A failed import/replacement keeps the previous verified preview and saved record.
A fresh reopen with missing, corrupt or hash-mismatched bytes shows unavailable;
there is no silent embedded-Pioneer fallback. Reimporting and accepting the original
verified bytes allows an explicit Save to atomically repair a damaged ordinary blob
at its matching hash address. An unreadable index locks Save.
Repair it and Reload, or explicitly choose **Allow replacing unreadable asset
index**; that choice itself writes nothing. A lost worker result is not replayed.
A submitted Save may finish in its original project after the workspace closes.
Unused content blobs and interrupted temporary files are not loaded as a revision.

## Validation

Two original asymmetric fixtures live in `apps/editor/tests/fixtures/import`.
The green wedge is translated; the magenta spire is translated and rotated. Tests
cover measured bounds, import/accept/save/full reopen/replacement, provenance,
failed content publication, corrupt/missing bytes and index, cancellation, delayed
results, project isolation and an actual local authority with preview input.

Use `python3 tools/build.py test -p arkline-editor --locked imports::tests` and the
normal workspace checks. The Linux GPU test is opt-in:

```
ARKLINE_IMPORT_GPU_EVIDENCE=/tmp/import-gpu \
  python3 tools/build.py test -p arkline-client --locked \
  linux_gpu_import_replacement_materials_and_resource_cleanup -- --ignored
```

It creates a private surfaceless EGL context, renders both fixtures and the actual
Pioneer, checks color/geometry differences and obsolete GPU resource deletion,
and optionally writes PPM captures. The pure geometry decoder also compiles for the browser client; this change does
not add a browser editor/import interface. Native Mac and visible-desktop acceptance are
separate observed checks. No merge, release or deployment follows from importing.
