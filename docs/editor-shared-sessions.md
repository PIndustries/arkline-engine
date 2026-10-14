# Shared development sessions

**Multiplayer session** switches the editor between its local development world and a shared host. This is a local-development feature, separate from the official account service and contributor GitHub sign-in.

## Host a world

Choose **Host**, enter a pilot, and select **Host shared world**. The default address is `127.0.0.1:7779`. Hosting closes the current local worker before opening the same project's preserved `.arkline-studio` world as the shared authority. The world reopens paused; its resources and pilot credentials are preserved. The host controls Play, Pause, and simulation stepping.

Each host start generates fresh editor and viewer admission secrets. The host UI masks the editor secret until Reveal or Copy is selected. Keep secrets out of source control, issue reports, and screenshots. Share the editor secret with a teammate joining as an editor. A returning pilot also needs its saved resume credential.

## Join from another editor

Use a separate project folder for each editor process. Choose **Join**, enter the host's loopback address, a distinct pilot name, and the editor secret, then select **Join shared world**. Joining preserves that editor's local world database; it sends gameplay requests to the host authority and renders its snapshots. The host controls the shared clock. Stopping your local controls does not reset the shared world; resuming returns you to its current state.

The transport accepts loopback bind and connect addresses only. For a teammate on another machine, establish an authenticated SSH tunnel to the host:

```sh
ssh -N -L 17779:127.0.0.1:7779 developer@host-machine
```

Then join `127.0.0.1:17779` from that machine's editor. Configure SSH access separately. The session transport's local JSON traffic is not TLS-encrypted; SSH encrypts the connection between machines. The editor does not open a public listener or change firewall rules automatically.

## Shared simulation and room-layout workflow

Participants use one authoritative world and clock. Pilot movement and game requests, snapshots, event-graph revisions, and the live Pioneer room-layout revision come from that authority. Event-graph writes use expected revisions, so conflicting writes are rejected rather than overwriting another editor's changes. Graph execution remains bound to the host's actor.

Room-layout project drafts remain local files at `Assets/Pioneer.rooms.json`; project files are not synchronized. The host authority keeps the accepted live document at `<authority-dir>/room_layout.json`. After an editor applies a valid layout, the authority persists the revision and distributes its `RoomLayoutState`. All clients render from that state, and the server derives collision obstacles from the same document. A draft save alone affects only that editor's project file.

For a coordinated edit, both editors join the same host and load the current live layout before editing. **Save + apply live** saves the local draft first and then sends a revision-checked apply request. A stale request is rejected if another editor has already changed the live revision. A clean inspector follows incoming edits automatically; an inspector with local draft edits retains them and reports the conflict. **Load live layout** explicitly replaces that draft with the current authority document. Reconcile local drafts and use source control for project files. Host and editor participants can apply a valid layout; viewer access is read-only.

The runtime projection currently covers shared room bounds, walls, door openings, and the existing prototype storage fixtures. It does not simulate room utilities, microcontrollers, or lighting firmware. Engineering documents, service routes, material preview data, and other authoring files remain local.

## Lifecycle and limits

Returning to **Local** shuts down the owned host or leaves the joined session before reopening the preserved local world. Closing a host ends the shared session; other clients see a connection error. Rehosting creates a new secret, and clients must join again. Automatic reconnect and replay are not implemented. Viewers are supported by the development transport as read-only peers; the current session panel provides Local, Host, and Join editor choices.

The transport bounds peer counts, frame sizes, idle deadlines, request rates, and actor queues. Network work and database shutdown run on workers rather than the rendering thread. Gameplay mutations are rejected while the world is paused.

Automated transport and authority checks do not establish the native multi-editor or cross-machine journeys. Track those acceptance results separately; this guide makes no claim that they have passed.
