# Cargo departure readiness — 24 October 2026

This local follow-through makes the existing authoritative cargo interlock visible.
It does not add a new server policy, loading simulation, crane or asset change.
Base: verified formatting checkpoint 5170c5b. Reviewed three-file worker 2150083 is
integrated as 554b38d with one additional test-only native capture fixture.

## Behavior

- All six Undock surfaces share a departure-only readiness reason. Known supported
  cargo reservations show “Cargo reserved · finish or cancel unloading in Cargo &
  storage before undocking.” The physical authority cartridge says CARGO RESERVED;
  controls retain their positions and identifiers.
- Unloading, held and resumed operations block departure while units are reserved.
  Completed and cancelled operations release it. Cancelling does not pretend that
  remaining ore left the ship; the real fixture retains 32 units after cancellation.
- Match supported schema, authenticated controller, pilot, class, system, berth
  and nonempty authoritative vessel identity. Do not apply private owner receipts
  to a remotely controlled worker or manufacture an identity from a name.
- A narrow Undock send guard explains the known reservation before relinquishing
  inputs, cancelling AI or marking a request pending. Snapshots are never edited.
  Existing server validation remains authoritative for races and unknown data.
- Generic navigation readiness is unchanged. STOP, docking HOLD/RESUME, walking,
  seat/traversal controls and cargo Resume/Cancel remain available under their
  previous independent conditions. No auto-cancellation or new wire fields.

## Evidence

Worker focused tests cover 60 phase/width/surface combinations at 600×650 and
900×650, using actual pointer clicks and full text-rectangle clipping checks.
Disabled controls emit nothing and do not merely rely on the send guard. Terminal
states restore exactly one request. Additional real clicks preserve STOP,
HOLD/RESUME, LeaveSeat and cargo Resume/Cancel. Identity/schema/readiness and
before-mutation state guards are checked. Worker client 570/0/45 and strict/check/
fmt gates passed before integration.

Native integrated proof passes 20 actual engine images at 900×650 and 1280×800:
unloading, held, resumed, completed and cancelled, in physical cockpit and
Navigation workspace. Response producer is exact e443229; renderer is 554b38d.
Every PNG and input response hash is recorded and independently checked. The
reserved explanation is fully visible; terminal states remove it and enable
Undock. No private authority state is fabricated by the renderer.

The accompanying GIF is five genuine Navigation stills held 1.8 seconds each for
readability, explicitly a sampled status sequence—not continuous simulation
cadence, new cargo motion or a frame-rate benchmark. Software Mesa rendering was
used. Existing 900px connection-bar/window-title overlap is visible and remains a
separate polish item; the proof does not hide it.

Final exact integrated workspace tests pass: 1,267 passed, 0 failed, 57 ignored
(three nested subprocess summaries excluded from the total). Client 570/0/46 and
editor 297/0/1 pass; one new explicitly ignored native fixture was separately run
and passed above. All-target check, strict all-target Clippy and formatter check
all exit 0. No publication,
merge, deployment, world migration or public tracking was performed.
