# Living-world preview and follow-on interfaces

## Station background traffic

The server supplies nine background service actors: one shuttle, freight tender
and patrol in each of the three test systems. Each has a stable ID, role, hull,
position, velocity, heading and arriving/departing/holding phase. Routes repeat
with a deterministic seven-minute schedule offset by actor/system. Smooth legs
last three minutes and end in a thirty-second turn/hold. Hulls remain outside the
station's front surface and away from the player berth.

These are explicitly **background traffic**, not invented connected players.
They do not mine, consume inventories, trade, dock into hangars, earn credits or
appear in online presence. The schedule creates visible local activity; it is not
an economic agent simulation or a logistics system. Traffic positions are generated
by the server from its timestamp, so two observers receive the same schedule.
There is no persistent individual route state or collision avoidance. A future
interactive traffic simulation would need authority, collision and ownership rules
before these actors could affect players or assets.

Acceptance coverage checks stable unique identities, three actors per system,
position continuity, bounded velocity, hull clearance and unchanged player/ledger/
asteroid state. Rendering acceptance must separately confirm visibility, labels,
system filtering and frame pacing in the packaged application.

## FTL relay fiction and actual authority

The intended fiction is a **P.Industries relay connecting the pilot to the unknown
universe of Arkline**. The startup sequence is the pilot's passage through that
relay into a persistent place to explore and build. It is not merely a story about
station navigation data synchronizing between the three current test systems.

Today the visible relay sequence represents the existing TCP connection and saved
pilot session. A successful session response and a snapshot containing that pilot
are required before the introduction can complete. Animation does not establish
an independent authority, certify an operator or turn a disconnected client into
a connected one. The three preview systems still run inside one server and one
saved world. “FTL relay” is fiction, not a claim about real networking technology.

The longer-term direction includes community-operated **separate universes**, with
communities helping develop the engine, content and operating rules. A future
relay interface could identify the selected universe, its operator and rules, and
show the real connection state before entering it. Separate universes must not be
presented as an already shared economy or as automatically interoperable worlds.
Cross-universe identity, assets, travel and trust would need explicit designs.

That is a future direction. The repository remains private pending owner validation,
and no open-source license has been selected. Community node registration, official
node certification, uptime rewards, distributed authority/consensus and universe
federation are **not implemented**. The existing independent verifier checks supplied
economic history; it does not establish any of those network guarantees. Choosing a
license and defining operator governance are still separate decisions before a
public release. No future node or licensing proposal changes the current server's
authority or the meaning of today's economic audit.

## Future MIDI device controls

A configurable MIDI control surface could map knobs/faders to bounded thrust,
rotation or camera axes and buttons to explicit actions such as brake, docking or
module activation. Prefer a calibration/mapping screen with a live input monitor,
axis dead zones, inversion and a clear device-disconnected state. Device identity
and bindings should remain local preferences.

All inputs would use the existing command boundary and server validation. Focus
loss, device loss and stuck controls need neutral input plus the existing server
lease; reconnect must not replay stale movement or economic actions. Reserve
one-shot destructive or costly actions for explicit presses, not continuous knob
movement. This is a design proposal: no MIDI backend, device support matrix or
hardware acceptance is claimed.

## Future event-driven audio

Audio should communicate confirmed state: lock acquisition/completion/loss,
module start/stop, completed extraction, low capacitor, warp phases and connection
loss. Continuous engine and module loops can follow authoritative/interpolated
speed or activity, with bounded volume and voice counts. Distinguish request
acknowledgement from actual completion. Deduplicate one-shot sounds across repeated
snapshots and reset their history on reconnect so old rewards do not replay.

Use separate controls for ambience, machinery and alerts; retain visual equivalents
for accessibility and muted play. Inventory original/licensed sound sources and
redistribution rights before packaging. This document proposes an event/audio
mapping, not an implemented mixer or audio asset library.
