# Game sound

Arkline's game sound is generated procedurally from confirmed simulation state.
It is machinery and event feedback, not random music or a recording of an unrelated
world. No external sound library or music asset is introduced by this implementation.

The audio scene derives engine intensity from measured acceleration, mining loops from
active confirmed modules/drone cycles, and travel sound from authoritative warp
phases. Connection, docking, lock, extraction and other supported transitions can
produce short cues. A confirmed nearby warp arrival within 200 km in the same
system also produces a cue; stale arrivals do not replay. Repeated snapshots must not repeat rewards or their sounds;
stale/disconnected state silences continuous machinery rather than implying that
unobserved work continues. Sounds do not submit gameplay commands or award ore.

## Event signatures

The reactor bed stays quiet under other events. Engine harmonics follow measured
thrust; coasting does not produce a full-power engine loop. Mining has a spin-up,
a restrained resonant loop, a short confirmed extraction cue and a spin-down.
Continuous cycles do not replay start/stop cues each time a cycle rolls over.
Warp uses related low-frequency layers for charge, entry, travel and arrival.
Entry is emitted once when the confirmed trajectory crosses its warmup boundary;
repeated frames, reconnects and stale samples do not replay it. Docking, drone
operations, target lock, low capacitor and full cargo retain distinct signatures.

All layers are synthesized from original oscillator/envelope definitions. Filtered
mechanical texture stays behind the tonal layers. A fixed voice pool, gain smoothing
and limiter retain callback bounds and leave headroom for overlapping events.

## Controls and output

Sound preferences provide Master, Ambience and Effects levels, a game-sound mute,
Test sound and Retry audio. Defaults are 45%, 35% and 65%, with mute off. The
settings are stored locally in `sound.json` in the client's data directory, using
an atomic file replacement. Invalid, incomplete, oversized or out-of-range files
fall back to defaults; invalid values are never saved. These preferences do not
change server state.

The engine uses the system's default output device. Output initialization occurs
on an audio worker; an unavailable device reports a status instead of making game
connection depend on audio. Retry audio attempts to open the current default
device again. Select speakers or headphones in the operating system. Explicit
in-game output-device selection is not included.

Test sound is for checking the current output and gain path. Mute and zero levels
must still be honored; it is not a bypass of saved settings. Normal game audio
uses bounded cue storage, a fixed voice pool, smoothing and an output limiter.
Those implementation limits do not establish that any particular speaker volume
is comfortable; start with a low system output level.

## Voice and remote ships

Voice input and spoken replies have separate controls. The mixer supports lowering
game audio during spoken replies (22% of its normal gain). During microphone
capture, the client mutes game audio to keep it out of the recognition
input; ending capture retains the user's saved mute and volume choices.
Speech recognition/output behavior and permissions remain separate from game-sound
availability. No microphone is needed for game audio.

The scene planner accepts a single source pilot. The client uses
the ship currently under confirmed control: normally the authenticated pilot's
ship, or the authorized remote ship while its manual-control lease is active.
Merely watching a remote camera does not switch the entire cockpit's
sound source. The planner resets transition history when the source changes so
another ship's old extraction or arrival does not replay as a new event. Returning
to the owner's ship restores its sound source. Packaged validation should exercise
these transitions; separate spatial feeds or remote audio streams are not claimed.

## Validation boundary

Preference tests cover round-trip storage, invalid-file fallback and failed-save
preservation. Scene/mixer tests can validate state transitions, bounded samples and
mix rules without opening an audio device. They cannot prove audible output,
latency, sound quality, voice ducking or device switching. Record those separately
with a native playback test on the packaged build. No microphone capture is part
of the settings tests.

Linux source builds need ALSA development headers (`libasound2-dev` on the CI
Ubuntu image), alongside the existing window/OpenGL dependencies. Adding this CI
prerequisite does not by itself establish that the hosted runner has usable audio
hardware or that Linux playback was heard.
