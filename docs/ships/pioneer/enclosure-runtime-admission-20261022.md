# Runtime admission: occupied-save counterexample

The `ed76b75` enclosure cannot safely become registration-4 runtime geometry
while preserving all existing occupied-save semantics. This is a concrete
collision/save compatibility issue, beyond the earlier study coordinate mismatch.
No runtime recipe, capability, registration, migration, client render path or
user save is changed by this assessment.

## Counterexample

Body position **[32.1, -0.32, 4.84] metres** is clear and supported in the existing
registered cockpit, including private, linear-fleet and circular-fleet placement.
The proposed `candidate/starboard-wall` occupies X=31.6..34.9,
Y=-0.32..2.40, Z=4.80..4.88. That existing legal avatar intersects it.

The example regression checks this directly with the original and proposed
collision models in memory. Passing a few circulation routes does not prove
compatibility with every legal persisted position. Original serialized bindings
identify registration 4 and layout hash, not an enclosure-study variant.

A server regression creates a new disposable SQLite world, connects a pilot,
leaves the seat through the actual authority transition, and walks through:

```
[33.36, -0.32, 3.0]
[33.36, -0.32, 4.84]
[32.10, -0.32, 4.84]
```

No avatar or persisted row is patched to manufacture this state. The test stops,
validates, closes and reopens the database, then checks retained feet and binding,
jobs, cargo, credits and ledger rows. The reopened capsule intersects the exact
candidate wall read from the checked-in manifest. Its fixture directory is new
and named `/tmp/arkline-enclosure-save-admission-*`; no user world is opened.

## Registration and render mapping

`DockedTraversalBinding` stores host vessel, registration revision, live-layout
hash and placement. `docked_binding_pose` rejects a different revision.
The negotiated capability also carries a source hash; the client requires exact
capability equality before enabling docked traversal. There is no existing
candidate-enclosure feature/configuration path shared by authority and client.

`registered_geometry` is consumed by both server collision/support validation
and `pioneer_docked_scene::SceneCache`, which builds canonical runtime render
parts. Therefore adding the study members there would give matching drawing and
collision, but also change the meaning of a registration-4 saved position.
The server's occupied-world validation rejects overlapping traversal capsules.

Appending visual walls only would leave a passable wall. Appending solid walls
under the unchanged capability would permit old and new builds to advertise
matching identities with different collision. A client-only flag, or an
unrecorded environment switch, would not solve either problem. Neither is used.

The existing migration helper is specifically v3 to v4, restricted to empty,
closed/stowed vessels. It does not authorize or implement this new geometry
change, and it was not broadened.

## Smallest design decision

To preserve all current registration-4 save semantics, redesign the proposed
solid enclosure members to avoid every existing legal occupied volume, using
the current solid boundary envelopes. The present inward wall is incompatible
with that constraint.

To keep this narrower bay, use an explicitly versioned authority geometry and
validated save-admission policy. At minimum, migration must refuse an occupied
or moving incompatible world, preserve its data without relocating feet, and
require a matching build. A quiescent/seat-only migration would need independent
validation analogous to the existing v3-to-v4 path. This is an authority-level
identity/save decision and is intentionally not implemented silently.

The next step is that decision, not another isolated geometry pass. No actual
runtime adoption or runtime-render success is claimed. Earlier geometry captures
remain inspection evidence only; the empty forward field and instrument quality
remain unresolved.

## Executed evidence

The current-source protocol example reports **3 passed, 0 failed, 0 ignored**,
including the occupied-position counterexample in three placements. The targeted
SQLite fixture reports **1 passed, 0 failed, 0 ignored**. The relevant complete
server traversal module then reports **18 passed, 0 failed, 1 ignored**, including
the fixture, seat/exit/reseat, physical hatches/lift, full boarding roundtrip,
stairs, restart, stale-registration rejection and the existing guarded v3-to-v4
migration tests. The ignored case requires a separately supplied copied user
world and was not executed. No full-client suite or new client runtime render
was run: adoption is blocked before changing the live recipe.

The [retained receipt](enclosure-runtime-admission-20261022.json) records loaded
capability version **2**, registration **4**, source SHA-256
`8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123`, and unchanged
persisted station feet **[32.1, 8.68, 4.84] m**. It binds the intersecting candidate
to GLB SHA-256
`07e8b762fadaed0f8fdb4d7c31ea69dd9e3b28dcb11d51d25e61d3441be41b50`.
These are actual server-load/save results for the disposable fixture, not live
user telemetry or evidence that the candidate is installed.

Logs are retained at `/tmp/canonical-enclosure-admission-{protocol,server,server-suite}.log`.
No remote CI lookup or previously denied operation was attempted.

Reproduce the admission evidence:

```sh
cargo test --locked -p arkline-protocol --example pioneer_canonical_enclosure
cargo test --locked -p arkline-server --lib docked_traversal_tests::canonical_enclosure_candidate_intersects_a_reachable_v4_sqlite_save -- --exact --nocapture
```
