# Official Pioneer assembly study

Status: staged internal design/compatibility work, not a player ship builder.
Baseline: `e9ef19e550c89f131c2ab5d73d07ede7e7030cf2`.

## Product boundary

Ships remain officially authored and supplied by the team. The purpose of modular
definitions is to let the team design a broader official fleet without duplicating
lifecycle code or reducing ships to arbitrary blocks. No player geometry editor,
runtime coordinate input, capacity slider, public publishing, or save migration is
part of this milestone. Existing Pioneer assets and worlds remain available.

## First executable gate: unchanged Pioneer

Introduce a bounded catalog keyed by the exact immutable bay registration. Its
first entry describes the existing 15 slots, 22 mechanism groups, serialized rear
gantry topology, registered source, host compatibility, anchors and envelopes.
Legacy APIs may wrap this entry during adoption. Unknown definitions fail closed.
Compare every old launch/recovery route, phase, duration and sampled pose, including
all slots and reverse/cancellation paths. Reopen old saves without rebinding,
reseeding stock or changing inventory, cargo, ledger, physics or timing.

This first gate does not demonstrate arbitrary capacity or topology. Those remain
separate contracts rather than a change to a global maximum.

## Official alternative: geometry before adoption

The proposed visual language is a raised command spine with paired working pods,
clearer large armor surfaces, restrained structural color and readable machinery.
Retain the raised bridge and protected crew/core interfaces. Compare whole-ship
front/side/three-quarter native views at matching exposure and distance; bay and
receiver closeups supplement, not replace, the overall form study.

Two geometry options must be distinguished:

* A bounded additive dorsal-pod and explicitly replaced fixed-rack-support study
  can retain the current hull and 15 home anchors. This changes assembly structure
  and visibility, not capacity or the arrangement of stored drones.
* A genuinely rearranged bay may require an expanded official hull. The old
  envelope is not a product restriction, but new hull bounds must reach collision,
  berth/docking admission, occlusion and every dependent route before gameplay
  adoption. It is not safe to stretch the mesh while keeping old collision bounds.

Small anchor offsets may test parameterization but are not a meaningful ship
redesign. Current cavity measurements make alternative 15-drone packing tight;
source-based clearance decides feasibility, not a desired screenshot.

### Decision record — 2026-10-25

The delivered code milestone is unchanged-reg12 definition compatibility only.
Study A was rejected as an exterior redesign after matched native review: its
added gray rails retained the old overall box silhouette. It remains an isolated,
unregistered provenance/open-rack comparison, not an installed ship or proof of
stocked-bay visibility, new topology or structural strength.

Study B was also rejected by the user after viewing the bounded refinement.
It remains an isolated authored massing record, not an approved direction or a
playable registered hull. The user prefers the existing bulky industrial
construction/mining character, not the streamlined broad-carrier concept.
Its broader aft industrial volumes taper toward the command bow; the raised
bridge remains an exact source reference joined visually to a central spine.
Two recessed working/drive volumes and distinct aft propulsion, portal and
receiver zones explore functional organization, not implemented mechanisms.
The first blank-shell study and one bounded refinement are retained separately.
Matched native views use identical cameras and lighting; no AI image is evidence.

The proposed bilateral bay has three longitudinal stations with three port and
two starboard tiers (15 total), near Z ±6.8 m, requiring about 20.2 m clear bay
width. The current massing is approximately 72.12 m long and 42 m wide. These are
design dimensions, not accepted moving-route, docking, strength or mass results.
Protected-volume and padded parked-envelope checks do not establish dynamic
clearance. New handling topology, portal/doors, full collision bounds, receiver
traffic, berth admission, cameras and persistence require separate validation
before any fresh-world adoption. The current Pioneer remains unchanged.

Combat readiness is a separate design question: protected deep storage, near-hull
Ready cradles with independent departure lanes, and exposed emergency attachment
must not be conflated. No such states or hardware are implemented here. The ore
receiver remains a separate service resource. No deployment-speed change or
physical kilogram accounting is claimed by these studies.

## Source composition contract

Prefer explicit composition where appropriate: preserved registered host asset,
exact replacement source-ID ranges and separately hashed additive assemblies.
Every removed visible range must have an identified reason/replacement; no blanket
hull hiding or old/new rack double drawing. The same resolved composition feeds
color, depth, rays and mounted cameras. Cache keys include the actual assembly.

An assembly key must distinguish the composition digest from its compatible host
registration. Traversal may remain on the exact old host only if its geometry and
interfaces actually remain unchanged. Receiver compatibility is an explicit tuple
with its source hash and new cross-clearance proof, never generic hash acceptance.

An alternate enters only an explicitly selected fresh official preview after its
asset hashes, routes, runtime consumers and negotiated capability are validated.
Old defaults and old saved bindings remain unchanged. No mixed-version admission
may silently discard the new assembly.

## Acceptance and rollback

1. Old definition equivalence and save/replay tests before enabling an alternative.
2. Source geometry and static/all-pose moving clearance, contact and camera checks.
3. Native matched whole-ship and functional bay/receiver views; no pilot FOV trick.
4. Actual launch/recall, Hold/Resume/Cancel, destruction, restart and transaction
   rollback; retained identity/HP and ore conservation for each supported variant.
5. Full applicable tests/check/strict formatting gates and independently verified
   source/assets/receipts backup; old checkpoint remains the rollback path.

No structural strength, kilogram mass, full inertia, thermal model or newly metered
motor power is inferred from shape/material names. Existing accounting preserves
unknown/unmetered distinctions. Render, collision and simulation budgets are
separate; visual detail does not imply an entity or solver per part.

## Subsequent functional layout question — design only

Before further hull artwork, compare ten versus fifteen independent bays while
retaining the bulky industrial identity. Each candidate bay needs its own door,
cradle, positive capture/clamp state, approach/departure corridor and resource
ownership. Parallel launches are possible only for independently clear lanes,
not by ignoring intersections or shortening existing certified motion.

The proposed per-bay ore mount may extend at the mouth for home-preferred quick
unload without full storage; full Recall proceeds through physical cradle
capture. The sequence must distinguish opening, extension, alignment, capture,
transfer, release, retraction and closure. A damaged/blocked door, clamp or mount
would affect that bay, not silently disable the whole fleet. A fallback must use
an available compatible service berth without taking another drone's occupied
home, or hold safely. Coupled failure, full ship hold, loss of power, obstruction
and aborted recall need explicit recoverable states and visible availability.
These are proposed mechanics, including component damage—not claims of existing
repair, power-loss or per-bay hardware simulation. The implemented single bay
and ore receiver remain unchanged until a separately approved variant exists.
