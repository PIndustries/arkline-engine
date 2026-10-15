# Pioneer hull surface authoring

The hull surface workspace authors a spatial graph for one explicit Pioneer
material slot. Its document is separate from the existing fleet finish:
`Assets/Pioneer.surface.json`. The saved binding contains an asset ID, its exact
SHA-256 content revision and a material slot. A graph cannot silently follow an
unrelated replacement asset.

Opening or editing the graph does not apply it to the running scene. **Apply hull
surface** is an explicit preview choice, initially off on each project open. The
choice is not persisted in the graph file. While enabled, the graph owns its bound
material slot; disabling it restores the renderer's existing finish path. The
Authored Surface node uses source material values before fleet tint or grain.

The starter graph takes object position in body metres, transforms the coordinate
scale and offset, evaluates seeded 3D noise, remaps it and produces a smooth mask.
That mask blends two surface bundles containing linear base color, roughness,
metallic and linear emission color. A separate body-space box mask confines the
finish to one hull section, blending inward from its faces. Outside that box the
graph retains the original authored surface. Min, Max and positive Feather are
named, typed controls; the graph validates their bounds. This is spatial parameter
blending within the engine's existing lighting model. It does not simulate
engineering materials.

Select any node to edit its input values or choose connected nodes in the
inspector. Named inputs can be reused by multiple Parameter nodes. The node
library creates real graph operations; newly added operations begin with
unconnected inputs. Click an output followed by a matching input, or use the
inspector's input menus. Right-click an input to disconnect it. Deleting a node
disconnects its consumers. The single Surface Output remains in the graph.

Incomplete graphs stay editable and undoable. Typed port mismatches and cycles
are rejected before changing a connection. Shared validation checks the full
document and resource budgets before lowering. The editor only sends immutable
validated revisions to the renderer. An incomplete draft keeps the last valid
surface visible and cannot be saved. GPU compilation is a separate context-local
step; its diagnostics and requested/applied revisions come from the actual
viewport. A failed candidate retains that viewport's last linked program.

Dragging a node changes the saved canvas layout and undo history without emitting
a new shader revision. Scalar, color, vector and seed edits emit new immutable
parameter revisions. Lowering and renderer program reuse determine whether those
revisions need compilation. The active viewport reports actual compile attempts;
the editor does not infer GPU acceptance from CPU validation.

Open, Save and Reload run on a bounded background worker tied to the panel's
project. Panels opened with the same project path share FIFO operation ordering,
established before a request returns to the UI. Reopening waits for an already
submitted save from the closed panel before reading. Save atomically replaces
the file using a new sibling, after validating
the complete submitted snapshot. Files larger than 128 KiB, unsupported documents,
invalid graphs, symbolic links and unreadable directories are rejected. A failed
load locks Save. Repair the source and Reload, or explicitly allow replacement of
the unreadable surface and then Save. Allowing replacement does not itself write
or reset the draft.

A late save receipt updates the saved baseline while preserving newer edits.
Reload is undoable and replaces the draft only when no edit, undo/redo movement
or node drag occurred during the read. Switching projects creates a separate
panel, generation and result channel. A submitted write may finish in its original
project after the window closes; it is never redirected to the new project. If
the worker stops before a save receipt, the outcome is reported as unknown and
further writes stay locked until the project is reopened and read.

This ordering protects quick close/reopen within this editor process. It does not
resolve independently edited drafts in simultaneously active panels or external
programs, and project path aliases are not a cross-process file lock. Reload before
saving if another writer changed the file.

This first slice has no image sampling, UV authoring, normal maps, decals or
arbitrary shader text. Those controls should appear only when the asset pipeline
and runtime renderer implement them. The target is authored material slot 0, with
a spatially bounded section inside that slot; finer part assignments still need
their own source asset and acceptance evidence. A passing graph or persistence
test does not establish native GPU appearance, frame latency or released status.
