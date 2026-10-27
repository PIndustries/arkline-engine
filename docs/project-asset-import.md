# Project GLB preview

The native editor’s **Assets** workspace imports one static GLB into a local project preview. It
renders the imported triangles and scalar material bindings using an independent
renderer. It does not replace the live ship or create world entities.

1. Choose a GLB from the searchable **Project assets** list, then choose
   **Inspect selected** (or double-click the file). **Refresh files** checks the
   project again. The list shows only the currently supported model type, GLB;
   it is not a general catalog of textures, materials or other asset types.
   **Import GLB…** opens the native file chooser, and dropping one GLB into this
   workspace also stages a file. These two actions can select a file outside the
   project; its original is read, never changed. A cancelled chooser leaves the
   previous preview and saved revision unchanged. **Advanced: relative path**
   retains the manual project-relative path field as a fallback.
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
5. **Save assets** copies verified bytes into this project and persists the
   accepted snapshot. The external original is not needed afterward.
   **Reload assets** reloads the
   saved reference and verifies/decodes its bytes again. Reopening does the same.
   Later accepted edits remain unsaved if an older submitted Save finishes.

The initial asset slot keeps the same asset ID across replacements and increments
its revision/hash. To replace it, inspect another GLB, review its changed geometry
and accept/save it. **Cancel staged** discards a candidate without touching the
accepted or saved revision. Selecting a different project file or editing the
advanced path also discards the old candidate and
invalidates its pending inspection result. Source/license text entered during a
Reload survives that completion, while the saved geometry still loads normally.
Saved source/license references belong to that accepted
revision; editing candidate references alone does not change the saved record.
The accepted slot is also shown as **Accepted asset #… · revision …** in the
browser. Its binding is the stable asset ID, revision and content hash, not an
absolute source path. This is one accepted preview slot with several selectable
source files, not a multi-asset assignment system. The central 3D view renders
actual selected geometry; the file list does not claim generated thumbnails.
Cancelling a pending native picker invalidates its eventual result; close the
dialog itself if it is still open. Nothing is automatically retried.

Project discovery runs off the UI thread and reads metadata only, with limits of
2,048 entries, depth 8 and 256 GLB choices. Hidden/runtime directories, Cargo's
`target` directory, symlinks and the private `Assets/Imported` content store are
excluded. Oversized GLBs can be listed but fail the normal inspection byte budget.
The list reports partial scans and skipped unreadable/symlink entries; an incomplete
scan is not proof that no other source files exist. Inspection rechecks the actual
file and its project containment before decoding. External chooser/drop input must
be an ordinary `.glb` file, not a file symlink or directory. Native dialogs and
decode/read work run on the asset worker. On Linux the chooser uses RFD's XDG
portal backend (or its Zenity fallback), which requires desktop runtime support;
project choices and file drop remain independent of the picker backend.

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
rejected for project choices. Explicit native chooser/drop input may come from
outside the project, and only verified bytes plus the versioned record are saved.
No external filesystem path is added to the index. These operations assume a
trusted local project directory and do not
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
Additional checks cover the visible choice/search workflow, bounded discovery and
excluded paths, picker cancellation, late picker-result rejection, drop staging,
and reopening an external import after removing its original source file.

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
