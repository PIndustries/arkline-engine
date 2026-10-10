# Ship computer and resource jobs

Open **AI** in the cockpit console or the service rail. The computer operates an
existing authenticated ship, or an existing worker that explicitly delegated fleet
control to your pilot. It does not create pilots, spawn worker ships, or grant access
to another player's inventory.

## One confirmed haul

Select a ship, choose an ore goal, and press **Start gathering**. No model is needed
for this button. The ship must be docked, have empty active cargo and no deployed
drones. The goal must fit that hull's cargo capacity.

The server undocks the ship, travels to its current system's belt, acquires an
asteroid and mines using both turret slots and up to five mining drones. It recalls
the drones before returning, docks at the starting system's station, and deposits
ore in that pilot's personal station storage. This is one haul in one system; it
does not traverse gates, repeat indefinitely, trade, or transfer the proceeds to
the controller. Fixed extraction cycles can exceed the requested minimum, within
cargo capacity.

The panel shows the latest job for the selected ship, its phase and message, and
separate **confirmed mined** and **stored** totals. Progress reflects confirmed
storage, not an animation or model prediction. A blocked job displays its reason;
low power can pause work before a bounded wait ends in a blocked result. The panel
retains last-known state when disconnected. Starting jobs and accepting model
instructions require a fresh server snapshot.

**Cancel job** stops automation. Successful manual actions take control and cancel
the job; they do not roll back ore already extracted or deposited. Server restart
cancels jobs, and reconnect never silently restarts one.

## Explicit worker delegation

1. Open a second cockpit and sign in as the existing worker pilot.
2. Open **AI → Fleet permission · this pilot**. Enter an existing controller pilot
   and press **Grant fleet control**.
3. In the controller's cockpit, select that worker in the AI ship selector. The
   same readiness checks and Start/Cancel controls apply.
4. The worker can use **Revoke control** to withdraw permission.

A delegated worker can log out while the controller stays connected. The server
continues its job and keeps its working drones available for that job. The
controller's last connection closing, the worker reconnecting or issuing manual
commands, permission revocation, or server restart cancels the job. A self-directed
job is cancelled when its pilot's last connection closes. These permissions apply
to this bounded job interface, not unrestricted remote flight or general account
access.

**Watch ship** switches to Cockpit and places the selected ship's live exterior
camera on the left physical screen. Your main cockpit and flight controls remain
attached to your own ship. The feed checks current delegation and snapshot
freshness; it does not require a delegated worker to have an open cockpit. Use
**Restore drone feed** to return that screen to your selected drone. Only one
remote/drone camera feed is rendered at a time.

## Typed Spark instructions

Type an instruction such as “Gather 100 ore and bring it back” and press
**Submit instruction**. Supported gathering and cancellation intents run directly
through the normal validated server requests. Status answers do not mutate the
world. Unsupported or invalid output cannot become an arbitrary game command.

The optional adapter contacts the configured model through SSH, using the default
`spark-wg` alias and its local Ollama service. The UI identifies the endpoint/model
and shows readiness, replies and errors. Operators can configure
`ARKLINE_AI_SSH_HOST` and `ARKLINE_AI_MODEL` before launching the client. Existing
SSH access and the model service must already be available; the game does not
provision them or open a model service port. The deterministic Start button remains
usable without Spark.

A pending instruction is bound to the selected pilot and connection session.
Changing worker, disconnecting, losing permission or fresh state, cancelling the
request, or issuing manual commands invalidates it. A delayed reply cannot start
work on a different ship or a replacement session. Only limited ship telemetry is
sent to the adapter; game credentials are excluded. MIDI and general autonomous planning are not implemented.


## Talk to the ship (macOS preview)

In the AI console, select the worker first, then press **Talk to ship**. macOS asks
for microphone and speech-recognition permission on first use. Speak a supported
order, such as “Gather 100 ore and bring it back.” **Finish and send** ends capture;
a final recognized transcript is submitted automatically through the same Spark
interpreter and server validation as typed input. The recognizer also ends capture
after 20 seconds. **Cancel recording** discards the pending utterance. Partial
transcripts appear as **Heard** but never execute commands. Changing ships,
losing the connection or permission, stale state and manual takeover invalidate
pending voice orders.

Talk enables **Spoken replies** for this session. The computer reads interpretation
results and confirmed job phase/storage updates. Uncheck it to mute, or use
**Stop speech** to interrupt playback. Starting a new recording stops playback to
avoid transcribing the computer's own voice. The transcript and text replies remain
visible. There is no always-listening wake word.

Input/output follows the Mac's default audio devices, including an external audio
interface if selected in macOS Sound settings. Device selection and MIDI bindings
inside Arkline are not included yet. The native helper requires installed on-device
recognition support for the locale (default en-US); it reports unavailable speech
or denied permission rather than falling back to a cloud recognition service. Raw
audio is not saved or sent to Spark. The resulting order text and limited selected
ship telemetry go to the configured Spark model. The Rust client/server remain
Rust; a small Swift helper bridges Apple's Speech and AVFoundation frameworks.
Other platforms retain typed input; the native speech helper is macOS-only.
