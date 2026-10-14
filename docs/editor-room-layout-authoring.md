# Editor room layout authoring

`apps/editor/src/rooms.rs` defines a project-local candidate document for the
Pioneer two-deck room plan. The panel appears in the editor's
right inspector as **Room layout**. It does not edit or reload
`arkline_protocol::pioneer_interior`, `PioneerRoomDefinition`, server collision,
the live room controller, or ship meshes. Live rooms and controls continue to
use their current compiled source until a separately reviewed runtime
projection/import is implemented.

## Source relationship

The first document is seeded from `arkline_protocol::pioneer_rooms(1|2)` and
`PIONEER_INTERIOR_SIZE`. Its twelve `source_room_id` values use the proposed
room-plan identifiers from `docs/ships/pioneer/rooms/rooms.json`: D1-P1,
D1-S1, D1-P2, D1-S2, D1-P3, D1-S3, then the matching D2 IDs. Each row also
retains the exact current runtime role string in `source_role`; the editable
`name` can change without losing that source reference. The first room also
retains the one current protocol room ID (`pioneer.deck1.airlock`); other
source rooms are explicitly marked as having no runtime ID. Current source room
origins, room dimensions, and door positions are shown as the initial candidate
values. New rooms receive local numeric IDs and have no runtime/source ID.

The editor writes `Assets/Pioneer.rooms.json`. The room plan is not linked to
`Pioneer.engineering.json`, equipment catalog connections, service paths, or
live telemetry. IDs such as D1-P1 are references to the room design source,
not promises that a runtime object with that identifier can be mutated.

## Coordinates and bounds

Each deck is an independent 32 × 14 m local plan. X is forward and Z is
starboard, matching the current interior source. Room origin is its plan
corner; clear length/depth/height are meters. Decks are discrete: the document
does not assign absolute vertical separation between them. Door centers use
deck-plan X/Z coordinates. Door clear width and height are dimensions only.

Validation currently permits at most 24 rooms across the two decks. Each room
has a positive, finite clear box that fits within the deck plan and a maximum
height of 12 m; clear boxes on the same deck may not overlap. Door openings
must fit the room width and height and lie within 25 cm of a front or rear room
edge. This is a document consistency check, not structural, egress, collision,
access-control, or engineering certification.

The preview draws candidate plan rectangles and openings plus the selected
room's X/Y side view. It is a 2D schematic: it does not display hull fit,
thickness, materials, equipment footprints, pipes, cables, service routes,
doors that open, or generated 3D geometry.

## Persistence

The module loads and saves asynchronously through a one-request bounded
worker. It uses `Assets/Pioneer.rooms.json`, caps the document at 64 KiB,
validates before save/load, rejects symlinked project/Assets/destination paths,
and writes through a same-directory temporary file followed by rename. A
failed initial load locks Save until Reload succeeds or the user explicitly
resets to the current compiled room source. Save completion records the
submitted snapshot while preserving edits made during the write; Reload
completion preserves edits made after the reload began.

## Editor integration and limits

The panel is integrated in the editor's right inspector under **Room layout**.
It remains a project-local candidate document: edits do not enter
`EmbeddedClient`, `PioneerRoomCommand`, `PioneerRoomState`, the server obstacle
list, or the live ship renderer. The existing Engineering tab and runtime room
source remain independent.

A future runtime projection must explicitly map candidate stable IDs to the
runtime's room identifiers and validate openings against collision/door
contracts. That projection, runtime generation, and shared wires/equipment
authoring are not implemented by this panel.
