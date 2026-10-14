# Cockpit panels

The **Cockpit** workspace authors a small local control panel for a ship. This first
slice supports a room-pressure gauge and a room-light switch. It does not create
3D cockpit meshes, seats, cameras, arbitrary scripts or new simulated equipment.

1. Choose a reported ship, select a template, then **Assign template to ship**.
   Vessel IDs come from the authority's room or hangar data. Saved assignments can
   be inspected when their ship is unavailable; their controls remain unavailable.
2. Choose **Edit shared template** to change every inheriting ship, or **Customize
   this ship** to create a complete panel override. An override no longer inherits
   template edits. **Use shared template** removes that override.
3. Select a control in the list or canvas. Edit its label, supported binding and
   rectangle in the inspector, or drag it on the canvas. Undo/Redo affect this
   cockpit document only. Preview clicks never operate equipment.
4. **Apply panel** installs a validated local content snapshot for this session.
   **Operate applied** tests that snapshot, while the inspector shows its binding,
   vessel, measurement time and reported state. The applied panel can also appear
   over the real **Game** viewport. Edits need another Apply to affect that panel.
5. **Save cockpit** writes the draft to `Assets/Cockpit.panels.json`. Save and Apply
   are separate. Reload changes the draft; reopening does not automatically Apply.
   Ordinary labels, placement and supported bindings need no engine rebuild.

Pressure comes from `pioneer.deck1.airlock.pressure`, in kPa converted from reported
Pa. Measurement timestamps are simulation milliseconds, not wall-clock freshness.
The switch targets `pioneer.deck1.airlock.light`; commanded and actual output
are displayed separately. The reported room interlock is shared room state, not
a light-specific fault or a fabricated fault model.
Other hulls currently have no compatible room telemetry. Unsupported or missing
bindings cannot be applied as working controls.

To test the switch, Play the simulation, use the active Pioneer from its cockpit
or deck one, and commission room power through **Game → Equipment** first. The
switch emits the existing typed light request. The client transport gate and
server authority/interlocks still decide whether it can run. There is no bypass
for a paused, stopped, disconnected or uncommissioned ship. Snapshot updates must
also be recent before the panel enables commands. The panel reads the existing
transport/request gate directly; disconnection or another pending client request
disables operation. The inspector includes existing session feedback, which may
describe another client operation and is not a correlated cockpit-command receipt.

The panel never predicts the result of a request. It waits for a newer room report
whose commanded state matches, and continues showing actual output independently.
That report is not a correlated acknowledgement of a particular request. After an
unconfirmed outcome, inspect the reported state; dismissing the warning sends
nothing. There is no automatic command retry. Changing sessions clears the applied
panel and pending command; Apply again in the new session.

File reads, Save and Reload use one bounded background worker. Save records the
submitted snapshot, so later edits stay unsaved. Late loads preserve edits,
Undo/Redo and active drags. Failed reads lock Save until Reload succeeds or **Allow
replacing unreadable cockpit file** is explicitly chosen; that choice itself
writes nothing. Invalid versions/bindings, oversized files and symbolic-link
destinations are rejected. The atomic writer is intended for trusted local project
directories, not adversarial directory replacement. A submitted write may finish
in its original project after closing the panel. Lost results are not replayed.

Automated checks cover the document, visible panel editing and a real local
authority light round trip. Native macOS protected-folder recovery and the visible
desktop workflow need separate observed acceptance. Documents are local project
content; Host/Join does not synchronize drafts or shared templates.
