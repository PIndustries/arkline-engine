# Shared development sessions

Open **Multiplayer session** from the editor toolbar. This is a local-development feature, independent of the official account service and contributor GitHub sign-in.

## Host the saved world

Choose **Host**, choose a pilot, and select **Host shared world**. The default address is `127.0.0.1:7779`. The editor closes its previous local worker before opening that same project's `.arkline-studio` world as a shared authority. The world reopens paused; resources and pilot credentials are preserved. The host alone controls Play, Pause and simulation stepping.

The host creates fresh editor and viewer admission secrets each time it starts. The interface exposes only the editor secret, behind explicit Reveal/Copy controls. Do not put secrets in source control, issue reports or screenshots. Another editor needs a different pilot name and the editor secret to join. An existing pilot also needs the saved resume credential from its previous session.

## Join from another editor

Use a separate project folder for each editor process. Choose **Join**, enter the host's loopback address, your pilot and the editor secret, then select **Join shared world**. Your local world's database is preserved; the joined session renders and sends commands to the host's authority. The host controls the shared clock. Stop suspends your local controls without resetting the shared world; Play returns your controls to the host's current state.

For another machine, first establish an authenticated SSH tunnel, for example:

```sh
ssh -N -L 17779:127.0.0.1:7779 developer@host-machine
```

Then join `127.0.0.1:17779` from that machine's editor. Configure SSH access separately. The transport deliberately rejects non-loopback bind/connect addresses: its local JSON traffic is not itself TLS-encrypted. SSH supplies the encrypted connection between machines. No public listener or automatic firewall change is made.

## Shared and local state

All participants use one server authority and clock. Pilot movement, game requests, snapshots and event-graph revisions come from that authority. Event graph edits use an expected revision; conflicting writes are rejected rather than overwriting another editor's changes. Graph execution remains bound to the host's actor; a guest editing a graph does not silently change that actor identity.

Room layouts, service routes, engineering documents, materials and other project files are still local authoring drafts. Joining does not synchronize those files or transmit material previews. Coordinate them through source control until document collaboration is implemented. This is shared simulation, not yet a simultaneous shared-document editor.

## Lifecycle and limits

Returning to **Local** shuts down the current owned host or leaves the joined session before reopening the preserved local world. Closing the host ends the shared session; clients show a connection error. Join again explicitly after a restart with the new secret. Automatic reconnect/replay is not implemented.

The listener bounds peers, frame sizes, idle deadlines, request rate and actor queues. Viewer access is read-only and does not create a pilot; the current editor UI joins as an editor, while viewer support is available in the development transport API. Game mutations are rejected while paused. Network work and database shutdown happen on workers rather than the rendering thread.

Acceptance must separately record native Host/Join operation and a cross-machine SSH test. Passing loopback transport tests alone does not prove those journeys.
