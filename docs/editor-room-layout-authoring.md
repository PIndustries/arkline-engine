# Editor room layout authoring

The **Room layout** inspector edits the Pioneer room blueprint in two places with distinct purposes: a project-local draft and, after an explicit live apply, the current shared development-world layout. Every connected client renders the authority's `RoomLayoutState`; server collision uses obstacles derived from that same document. A draft save alone does not change the running world.

All Pioneer interiors in this development universe use the same shared blueprint. Layouts are not customized per Pioneer hull.

## Draft and live state

The project source file is `Assets/Pioneer.rooms.json`. It is local to that editor project and suitable for source control. The runtime authority persists its current revision and document at `<authority-dir>/room_layout.json`—for the editor's normal project authority, `<project>/.arkline-studio/room_layout.json`. A new authority starts at revision 0 with the current default Pioneer room document when that runtime file does not exist. Joining another editor shares its live state; it does not overwrite the joining project's database or copy the host's project files.

**Save draft** writes only `Assets/Pioneer.rooms.json`. **Save + apply live** first saves the project draft, then submits it to the authority as a background, expected-revision-checked change. A failed file save does not submit a live change. If the live revision has changed, the authority rejects the stale apply; the saved local draft remains available for reconciliation. A successful apply increments the live revision, persists it, updates simulation collision, and publishes the new state to connected clients. Renderer geometry caches are keyed by revision and document content, so each client rebuilds for accepted changes and cannot keep a previous session's layout.

Only schema-valid, runtime-valid layouts can be applied. The host and editor participants may apply; viewers are read-only. A valid layout may be applied while simulation is playing if the authority confirms that existing avatars remain valid in the new collision geometry. The authority does not relocate a pilot to make an invalid layout fit.

The inspector follows incoming live updates while its draft has no local edits. If a remote apply arrives after local edits, the editor keeps that draft and reports a conflict against its old base revision; it cannot overwrite the newer live revision with a stale submission. **Load live layout** explicitly replaces the current draft with the authority's current document. Review or copy any local changes before using it.

For two-editor authoring, have both editors join the same host and load the current live layout before editing. One editor can make a change and use **Save + apply live**. The other editor sees the new authoritative geometry automatically if its draft is clean. If both editors have local changes, the later stale apply is rejected; load the live document and reconcile the drafts before applying again. Project files remain separate and must be coordinated through source control.

## Geometry and live limits

The document uses schema version 1 and supports at most 24 rooms across two independent 32 × 14 m deck plans. X is forward and Z is starboard, in deck-local meters. Each room stores a clear origin, length, depth, height, a door center and clear opening, an editable name, and optional source/runtime references.

General draft validation checks finite, positive dimensions, plan bounds, room overlap, and that the door fits the room and lies near a room edge. Runtime validation is stricter: live rooms must be at least 3 m long, 2.4 m deep, and 2 m high; live room height is capped at 3.2 m; door openings must be at least 0.8 × 1.9 m. The reserved access core, corridor, room-side wall clearance, and connected deck-one airlock geometry remain fixed. The connected airlock's name can change; its position, dimensions, runtime binding, and door contract cannot.

Accepted layouts drive both the client room shell projection and server obstacle generation. Door walls and trim share the normalized door plane. Existing fixed access-core geometry remains in place. Prototype storage fixtures are positioned from their source rooms. This projection does not implement room-specific utilities, equipment simulation, microcontrollers, or lighting firmware. Service paths and engineering documents remain separate local authoring data.

The inspector's deck plan and side view are authoring previews. They do not certify hull fit or engineering safety. Draft validation and live collision validation are separate checks; a document that can be saved may still be rejected for live use.
