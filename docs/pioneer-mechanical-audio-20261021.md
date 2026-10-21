# Pioneer mechanical audio — 2026-10-21

Confirmed authoritative snapshot edges now drive restrained original procedural cues:

- Hatch motor starts/reversals, stops, closed endpoint latch, and newly reported obstruction.
- Lift motor starts/reversals and stops. A stop is not represented as a successful landing.
- Seat servo for a newly accepted Leaving or Entering transition. No completion sound is inferred from avatar disappearance.

Implementation stays in sound_scene.rs and sound_engine.rs. No traversal, input, protocol, collision, world layout, camera, renderer or console changes. Hatch/lift comparisons require the same canonical traversal and actuator binding. Missing legacy motion data stays silent. Reconnect, stale gaps and first observations establish a silent baseline; repeated snapshots do not retrigger. Paused client updates continue consuming the scene baseline while suppressing cue playback, through the existing update_sound integration.

All cues use existing saved Effects/Master, mute and voice handling. Voices are short bounded envelopes in the existing fixed tone pool, not persistent motor loops that could survive lost state.

Focused validation: four mechanical audio checks cover confirmed/repeated/rejected/blocked state, rebinding/staleness/reconnect, explicit seat transitions, finite bounded envelopes, decay, mute and zero Master/Effects. Independent read-only transition review completed. No full suite repeated.

Limitations: no acoustic pass is claimed. Offline PCM and callback delivery do not prove the user hears sound. Seated snapshots contain no canonical actuator clone, so hatch/lift transitions unavailable to the controlled pilot remain silent. Seat completion, continuous motor hum and legacy boolean-only motion are intentionally outside this bounded milestone. User world actions will not be manufactured merely to demonstrate the sound.
