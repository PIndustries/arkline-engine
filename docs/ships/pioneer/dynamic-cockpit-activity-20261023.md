# Pioneer activity-driven cockpit mechanisms: evolving next slice

Tracking: issue 62. Base: `49c17e9804c06427c423136303b16ea052506d23`.
Branch/worktree: `design/pioneer-whole-space-20261023`, isolated dot prototype.
Owner: dot. Client implementation: integrated cockpit runtime worker; coordination,
documentation and checkpoint publication: cockpit redesign coordinator.


## Superseding requirement (23 October, physical deployment)

Current implementation contract: [physical cockpit mechanisms, registration 6](physical-cockpit-mechanisms-20261023.md).

The subsequent explicit user request replaces the fixed-bay implementation target
below: **five physical drone screens lower from the ceiling and two physical
turret screens rise from the center console**, with all seven individual views
available simultaneously at full mining. Confirmed warp state retracts the five
upward and the two downward; primary instruments enter warp mode. Failed or
cancelled warp must reconcile to actual authority, not command intent.

The earlier fixed-glass controller/feed work remains uncommitted and reusable;
it is not acceptance of this new mechanism. The four-texture limit is now an
explicit capacity constraint to redesign and measure, not a reason to substitute
paging for the requested seven views. Stable hardware slots and stable entity
assignments are separate identities. Physical cases, glass, input/depth and
collision must consume one deployment pose, including intermediate states and
swept-volume obstruction. Motors, mass and power are not accounted until real
simulation supplies those quantities; no decorative operational claim is made.

A larger ship/reactor warp-configuration request is being clarified separately.
No scaling of the entire ship, reactor core or occupied interior follows from
this screen mechanism scope.

**Everything below records the earlier fixed-bay plan and reusable findings,
not the current physical-deployment acceptance target.**

The user accepted the spatial direction and requested more connected behavior:
drone launch should reveal individual camera views, and turret activity should
reveal two turret views. This slice animates **display content inside fixed
physical instrument bays**. It does not move solid screen hardware and must not
be described as physical screen deployment. A later mechanical version would
require shared render/collision/picking transforms and swept-volume authority.

## Required behavior

- Observe accepted authoritative snapshots, not launch/fire button intent.
- Track stable owned drone IDs. Every deployed drone has an individually
  identified view, with explicit paging/pinning when all cannot fit at once.
- Reconcile initial/reconnect snapshots without replaying fictitious launch
  events. Distinct states can share timestamps; do not deduplicate only by time.
  Duplicate or older observations must not repeatedly restart an animation.
- Existing mining modules supply activity, not an authoritative shot event.
  Show two turret panes with individually truthful active/standby/stopped states.
  Do not label a generic ore event or a camera pose as proof that a turret fired.
- Returning drones remain present until authority removes their IDs. Removal
  alone does not distinguish docking from destruction.
- Preserve central command focus/semantics and view-only boundaries, including
  on-foot use. Changes in visible content must not turn a prior press into a
  command on a newly exposed target.
- Preserve per-feed signal, capture age and stale/disconnected/paused states.
  An animated reveal must not conceal unavailable or old telemetry.

## Capacity and source planning

The existing ship can expose up to fifteen drones plus two turret views, while
its renderer caches at most four views with a longest dimension of 384 pixels.
This slice does not alter that cache budget. The existing manually selected
camera bay must be included in the same unique-key admission/scheduling plan;
adding four wing views on top of it would exceed the real budget.

Use a mixed source planner rather than mutually exclusive Drones/Turrets mode.
Every visible consumer must register its stable feed key with one global
scheduler; repeated views of one source should reuse its capture. Explicit
paging/admission is preferable to silently omitting the fifth source or claiming
seventeen simultaneous live views. Exact bay allocation is an implementation
review item before code changes.

Feed renders remain nonrecursive (`cockpit=false`, `drone_camera=true`). They
are locally rendered views of received state, not network video streams. No
interior-camera protocol, mobile gateway, new shader/material/backlight or
uniform-cache performance optimization is in this slice.

## Acceptance

1. Pure controller tests: confirmed multi-drone additions, duplicate/equal-time/
   stale snapshots, reconnect reconciliation, returning/removal, module activity
   edges, and interrupted/reversed presentation.
2. Mixed planner tests: all IDs accessible, stable selection/page reconciliation,
   no more than four unique admitted cache keys across every visible bay,
   scheduling fairness, truthful availability and no recursive feeds.
3. Physical display tests: clipping remains inside fixed glass, actual scene
   depth/picking agrees, no moving-target click-through, central commands remain
   unchanged, and low/narrow viewports retain safe readable presentation.
4. Actual App/EGL transition evidence: before launch, confirmed launch/reveal,
   multiple drones/pages, two turret panes, return/removal, stale/disconnect and
   reconnect. Controlled test snapshots must be labelled as fixtures, not a live
   production session. Inspect images or video pixels before claiming success.
5. Repeat the spatial checkpoint's applicable protocol/server/client baseline
   comparisons. Commit and publish only the tested source tree; report CI,
   merge and release separately.


## Reviewed allocation and bounded implementation

Port bay 6 contains the drone-card drawer with stable view-only page/pin actions.
Starboard bay 7 contains two turret cards and compact retained ship/job context.
Existing manual bay 4 retains priority and its user's selection. The planner
admits its actually visible sources first, then turret views, then the selected
wing drone page, with at most four unique capture identities in total.

Identity includes camera mode/aspect and the fixed capture width/height; raised
and POV views of the same drone are
not interchangeable. This slice keeps the four-texture budget and capture dimensions bounded. When the exact
capture is already visibly displayed on the primary bay, the side card says
“Viewing on primary” instead of issuing a differently sized duplicate callback.
When capacity is exhausted it says why the card is not live and exposes a
view-only way to choose another page or free primary capacity. A merely selected
but hidden primary source cannot justify “Viewing on primary.” No manual choice
or pin is silently changed by an activity event.

Drawers use a bounded presentation transition of roughly 400 ms. Video rectangles
stay fixed and become active at completed reveal, preventing animated capture
sizes and moving pointer targets. Deferred/non-live cards still reveal honestly;
they do not pretend to display cached live video.

The demonstration must include one manual primary view plus two turret views
plus one wing drone view, then selecting the next drone. A separate four-manual-
view scenario must explicitly show side capacity limits. Equal-timestamp distinct
authority updates, initial/reconnect reconciliation and view-only interaction
remain required acceptance cases.

## Snapshot-ordering boundary

Current native/browser/embedded workers exchange and enqueue accepted responses
serially. The activity observer therefore uses current-session FIFO receipt
order for distinct equal-timestamp snapshots, and resets on reconnect/session/
ship changes. The protocol has no general snapshot revision; economic ledger
sequence is not a launch or display-state version. Lower timestamps may be
rejected and identical current relevant state deduplicated, but arbitrary older
same-time payloads cannot be detected from content/time alone. A global seen-
fingerprint set would incorrectly reject legitimate same-tick A→B→A changes.
No stronger network ordering guarantee is claimed by this presentation slice.

## Mixed-size scheduling finding

The current renderer's candidate-age lookup filters each candidate using the
current callback's width/height. That is insufficient for simultaneous differently
sized bays. A paused example can starve a cold feed: warm key 20 at size B, then
capture key 10 at size A, then add cold key 30 at size C. The key-30 callback can
misreport warm 20 as cold and select it; key 20 itself is not due and never claims
the capture. The sequence can repeat each presentation frame.

Pure activity-state work can proceed independently. Mixed-bay live-feed
integration must not claim fairness until that sequence is tested and resolved.
The relevant target is `Renderer::paint_cached_feed_on_surface` candidate age
selection. A correction would need each candidate's own capture specification
rather than the current callback's dimensions; raising the texture limit does
not solve this. The scoped correction uses source-specific candidate metadata directly, with a
paused mixed-size regression. It does not introduce the unrelated uniform-cache
performance helper, increase texture capacity or use a surface adapter to hide
the scheduling error.

A same-aspect resize also needs an explicit cold identity: changing a manual
single feed to a 2×2 page can halve both dimensions without changing aspect.
Including exact capture width/height in the key permits a narrow key-specific
age lookup and avoids stale-size warm metadata. Both different-aspect and
same-aspect/different-size paused transitions require regression tests.
