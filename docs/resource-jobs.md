# Mining routine and ship computer

Open **Mining** in the combined editor header or cockpit service launcher. The panel remains readable on foot and while paused; board the selected ship and press Play before starting. The computer operates an
existing authenticated ship, or an existing worker that explicitly delegated fleet
control to your pilot. It does not create pilots, spawn worker ships, or grant access
to another player's inventory.

## Confirmed multi-trip delivery

Select a ship, choose an ore goal, and press **Start mining routine**. No model is needed
for this button. Start docked or in space; orders during warp wait for arrival.
Deployed drones are recalled before departure. Gate transit must finish before
assignment. Existing cargo counts toward the total goal, which may be up to 1,000,000 ore.
The hold limits each trip rather than the whole job: a 4,000-ore order with a
1,000-ore hold makes approximately four trips.

The server undocks when needed, travels to its current system's belt, acquires an
asteroid and mines using both turret slots and up to five mining drones. It recalls
the drones before returning, docks at the starting system's station, and deposits
ore in that pilot's personal station storage. If storage is below the total goal, it departs again and repeats. The job stays
in one system; it does not traverse gates, repeat indefinitely, trade, or transfer
the proceeds to the controller. Fixed extraction cycles can exceed the requested minimum, within
cargo capacity.

The panel shows the latest job for the selected ship, its phase and message, and
separate cumulative **confirmed mined** and **stored** totals across all trips.
Carried-in ore counts toward storage but is not newly mined ore. Progress reflects confirmed
storage, not an animation or model prediction. A blocked job displays its reason;
low power can pause work before a bounded wait ends in a blocked result. The panel
retains last-known state when disconnected. Starting jobs and accepting model
instructions require a fresh server snapshot.

**Cancel routine** stops automation. Successful manual actions take control and cancel
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
job is cancelled when its pilot's last connection closes. The worker grant permits bounded jobs and explicit manual piloting leases.
It never delegates account login, storage, trading, or corporation access.

**Watch ship** switches to Cockpit and places the selected ship's live exterior
camera on the left physical screen. Your main cockpit and flight controls remain
attached to your own ship. The feed checks current delegation and snapshot
freshness; it does not require a delegated worker to have an open cockpit. Use
**Close remote camera** to restore the drone screen, or **Return to my ship**
to clear the remote selection. Only one
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


## Watching, manual control and returning

Your login/player identity never changes when selecting a worker. The persistent
header distinguishes **YOUR SHIP**, **WATCHING … CAMERA ONLY**, and **REMOTE
CONTROL**. Watch keeps flight input attached to your player ship. **Take manual
control** requests a server-confirmed lease for that explicitly permitted worker,
stops its AI job and places its cockpit/ship state in the main view. Flight,
navigation, locks, mining modules and drones then address that worker. Account,
market, inventory and AI-job actions require returning to your player ship.

**Return to my ship** is always visible in the header while watching or controlling
a remote ship; it does not require reopening the AI panel. It clears queued input,
releases the piloting lease, closes the camera link and selects your own ship.
It does not remove the worker or automatically resume a cancelled mining job.
Release clears manual thrust/rotation and brakes manual flight; previously issued
navigation autopilot orders can continue. Worker manual takeover/reconnect,
permission revocation, controller logout or server restart also release the lease.
Queued remote commands pin the exact worker, so a delayed command cannot steer a
different ship after switching. The server validates every remote command against
that lease and a gameplay-only allowlist.

All cockpit service destinations are now visible as a wrapping button grid. The
current page is highlighted; none are hidden in a More dropdown. Long page content
scrolls within the physical screen while the service grid and ship actions stay
visible.

## Conversation and drone instructions

The computer retains up to eight completed question/reply exchanges, bounded to
16,000 characters, for the selected pilot. For example, “I want a mining
expedition” can lead to a brief quantity question; answering “Fill the hold” uses
that ship's current cargo capacity. An explicit larger goal such as “Mine and
store 4,000 ore” schedules repeated deliveries without rejecting it at hold size.
The **Conversation** disclosure shows retained
exchanges. Changing pilot or connection session clears context. Cancelled or failed
interpretations do not become completed conversation turns, and previous tool
interpretations are explicitly not proof that the server accepted or finished work.
Current telemetry and job state remain the source of truth.

The computer can also request **orbit an asteroid** or **hold position** for an
existing deployed drone. Supply the drone ID and an asteroid ID, or use the current
selected asteroid when unambiguous. If a needed argument is missing, the computer
asks one short clarification. It checks the drone belongs to the selected ship and
the asteroid exists in that ship's current system; the server still validates the
command against the latest state.

An own-ship drone instruction requires control of your own ship. For a delegated
worker's drone instruction, first use **Take manual control** and wait for the
matching server-confirmed lease. Viewing a worker's camera or granting resource-job
permission alone does not route drone commands to it. If control does not match the
selected ship, the instruction is not sent and the panel explains the next step.
