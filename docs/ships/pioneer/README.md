# Pioneer — life of the ship

Pioneer is Arkline's 72 x 38 x 18 m industrial starter cutter. It has two mining turrets and two docked interior decks. The v05 interior review remains its visual design reference; earlier drawings are kept below as design history.

Historical baseline as of 15 October 2026: merged source includes the authored Blender/GLB hull, walkable decks, flight, mining, drones, cargo and scalar power systems. The unmerged development branch uses the Study05 hull, connected airlock equipment, powered exterior fixtures and native editor tools. Study05 refines the bow, glazing and service areas; overall v05 fidelity and exterior/interior alignment remain unfinished.



[Latest progress and vision](#progress-and-ship-vision--23-october-2026) · [Design direction](#design-direction) · [3D model history](#dated-3d-model-history) · [Dimensions and frames](#dimensions-and-frames) · [Current runtime](#current-runtime) · [Editor and asset work](#editor-and-asset-work) · [Editable vessel](#editable-vessel-animations-and-lights) · [Evidence](#evidence-and-acceptance) · [Gaps](#remaining-gaps) · [Milestones](#milestone-log) · [Concept history](#concept-history)

[Versioned page source](https://github.com/PIndustries/arkline-engine/blob/docs/pioneer-progress-20261023/docs/ships/pioneer/README.md) · [Fleet archive](https://github.com/PIndustries/arkline-engine/wiki/Ship-Design-Archive) · [Project 14](https://github.com/orgs/PIndustries/projects/14)

<a id="progress-and-ship-vision--23-october-2026"></a>

## Progress and ship vision · updated 25 October 2026

### Visual development timeline — newest first

The newest checkpoint is first; earlier revisions continue below. **These are actual engine captures unless explicitly labeled concept art.** Local prototypes are not published runtime builds or releases; integration status is stated for each checkpoint. GIF playback uses edited sample timing and endpoint holds, not measured desktop frame rate. Select a still for its full 1280 × 720 image.

#### 25 October · Latest: physical mining drones and a working ore-return loop

Pioneer's mining workflow now connects the drone bay, the asteroid, each drone's cargo and the ship's external ore receiver. The ship and its equipment are official Arkline designs. Player ship customization is not part of the current offering or this milestone.

**Status:** validated development preview, not yet a released build.

[![Actual receiver capture — select to play the ore-transfer GIF](https://arkline.network/images/arkline-journal/progress-20261025/ore-receiver-poster-ed1847f009e4.png)](https://arkline.network/images/arkline-journal/progress-20261025/ore-receiver-excerpt-d8c8244459db.gif)

*Select the receiver image to open the GIF. Actual engine capture: a 10.2-second normal-time excerpt of the receiver approach and transfer, looping from the beginning. It is not the full mining trip, a captain-view recording or a live frame-rate benchmark.*

[Watch the full 4:52.5 mining, drop-off and recall journey at normal simulation cadence](https://arkline.network/images/arkline-journal/progress-20261025/ore-journey-full-normal-time-18df47b114b1.mp4).

**What works now**
- Mining drones have a real 100-unit cargo limit and extract 10 ore per completed eight-second cycle.
- They travel to an external receiver and transfer up to 25 units per 500 ms step. Reserved ship headroom and conservation checks keep cargo ownership explicit through the transfer.
- A quick drop-off returns the drone to mining; Recall All brings it back, unloads its remaining ore and then stows it. These are different orders, not interchangeable screen animations.
- The captured 4-minute-52.5-second journey follows the same drone: mine 40, transfer 40, resume for another 10, then recall, unload and stow. Five completed extraction events are verified; a reserved but uncompleted cycle is not counted as mined ore.

![Drone and cargo controls showing confirmed ore, reserved headroom and receiver status](https://arkline.network/images/arkline-journal/progress-20261025/ore-transfer-controls-1a8383248097.png)

*Current coupled drone/cargo controls in the actual preview. Displayed state comes from the ship authority.*

**Still ahead:** final ship art and animation polish, representative hardware performance and feed-freshness acceptance, broader multiplayer/interruption testing, and the remaining connected ship systems. Development visuals remain in use. This milestone does not claim completed life support, vessel mass/heat simulation or finished station services.

<details>
<summary>Technical validation and capture provenance</summary>

The final local `e9ef19e` workspace validation recorded 1,471 passing tests, zero failures and 76 ignored tests. Check, Clippy and formatting gates passed. Runtime source for this milestone is not published.

**Capture provenance:** the ore footage uses renderer `c6f03b0` and authority `11325b7`. The coupled-controls still is a separate capture using renderer `a73caac` and authority `11325b7` (source index 1141). The later `e9ef19e` source validation is recorded separately; these images are not relabeled as footage captured from that final commit. The full journey was checked against 2,926 authority responses at 100 ms intervals, with ore conservation and unchanged credits verified.

[Public capture provenance and media hashes](https://arkline.network/images/arkline-journal/progress-20261025/drone-ore-media-manifest-a94a71009bd3.json).

</details>

Next, we are tightening the complete operational loop and its presentation together: clear orders, physical movement, truthful cargo/progress readings, recovery behavior and repeatable acceptance evidence. Older checkpoints remain below with their original status and limitations.

#### 25 October · Earlier: five-drone deployment and recovery through one physical bay

The Mining class can deploy five persistent drones and recall those same five through the shared bay. The recorded full-cycle check completed five launches and five recoveries, returning to 15 stored drones and none active. Drone identity, home position and health remained tracked throughout.

![Five-drone deployment and recall controls — sampled states with edited holds](https://arkline.network/images/arkline-journal/progress-20261025/deploy-recall-sampled-controls-ed199284df32.gif)

*Sampled actual controls and bay-camera states with edited holds, not continuous launch/recovery footage. This earlier capture uses renderer `d9fd266` and authority `4f1eb584`; the full continuous cycle was 630.2 seconds. The camera selector changes the bay monitor, not the captain's viewpoint. This is separate from the later ore-return journey above.*


<a id="24-october--latest-continuous-departure-and-return-without-a-scene-cut"></a>

#### 24 October · Earlier: continuous departure and return, without a scene cut

Pioneer now has a local, authority-driven docking path: the release phase begins, the ship backs out, and its return proceeds through approach, alignment, berth translation and the secured phase. The seated camera stays in the same world-space cockpit rather than switching to a hangar scene. This is real engine motion, not an animation laid over an instant teleport.

**Checkpoint:** footage captured from local `30618f0`; the later docking milestone `d1e86f8` adds acceptance tests, documentation and equivalent cleanup without changing the captured behavior. This is not the later combined cabin-polish build. Runtime source remains unpublished.

![Retained legacy immediate path versus continuous docking motion](https://raw.githubusercontent.com/PIndustries/arkline-engine/d197234676cd6a883cc741f0942c1012f5c5a2eb/docs/ships/pioneer/images/progress-20261024/pioneer-docking-before-after-motion.gif)

*Comparison: the retained legacy immediate path runs in the same modified binary at 1× simulation cadence, followed by a hard cut to the complete new sequence at labeled 3× speed. It is not an exact old-binary or same-world benchmark.*

![Continuous departure from the actual engine cockpit](https://raw.githubusercontent.com/PIndustries/arkline-engine/d197234676cd6a883cc741f0942c1012f5c5a2eb/docs/ships/pioneer/images/progress-20261024/pioneer-undocking-cockpit-continuous.gif)

*Seated departure, at 1× simulation cadence, with the actual engine cockpit and instruments. These offscreen captures are not a live frame-rate measurement.*

![Continuous approach, alignment and return to berth](https://raw.githubusercontent.com/PIndustries/arkline-engine/d197234676cd6a883cc741f0942c1012f5c5a2eb/docs/ships/pioneer/images/progress-20261024/pioneer-docking-return-continuous.gif)

*Return at 1× simulation cadence: the ship approaches after a manual 45° yaw, aligns, translates into its berth and reaches the secured phase. No generated intermediate frames or crossfades.*

**Connected behavior:** persistent operation and berth identities, vessel-owned berths, occupied-route checks, cancellation that brakes to a held state, and explicit resume after disconnect/restart. Docked access is granted only after capture. Collision handling remains conservative and kinematic, not general rigid-body physics.

**Evidence:** all 984 source PNGs decoded and matched their recorded hashes in producer validation. The actual engine renderer sampled authority states every 100 ms. Request boundaries preserve position and the completed journey returns to its starting berth; this validates the recorded route, not unrestricted multiplayer or live FPS. Consolidated protocol/server checks passed 352 tests with seven intentionally ignored; the final client run passed 537 tests with 11 known baseline failures and 39 ignored. No all-green workspace CI is claimed. [Media provenance and timing](https://github.com/PIndustries/arkline-engine/blob/d197234676cd6a883cc741f0942c1012f5c5a2eb/docs/ships/pioneer/images/progress-20261024/pioneer-docking-media-manifest.json).

**Still unfinished at this checkpoint:** this operates in a fresh **Continuous docking preview / DockingV1** realm with exterior Pioneer aprons. Existing worlds are not migrated, and the apron is not a finished shared station interior. Cargo transfer is still instantaneous; timed unloading is not implemented. Physical service hookup and pressure equalization are not implemented. Rail/walking clearance, combined cabin-polish integration, adverse-network/multi-client acceptance and normal-world adoption remain gates before calling docking finished.

Next: combine this path with the current cabin finish, validate walking and rail clearances and truthful phase labels, then build transactionally safe timed unloading. The older raised-bridge and cockpit studies remain below for comparison.

<a id="24-october--latest-raised-bridge-matching-hull-and-working-access-stairs"></a>

#### 24 October · Earlier: raised bridge, matching hull and working access stairs

Pioneer's bridge now sits eight metres higher as a complete cabin. The seat, nine camera monitors, consoles, controls and collision moved together. The exterior canopy and side windows follow that raised space, and the lower recess is closed around the intended boarding opening. This gives the ship a clearer bridge silhouette and makes the cockpit feel connected to the hull around it.

**Checkpoint:** locally validated prototype `c358f7b`. Runtime source is not published; this journal publishes documentation and capture media only. The model is still a functional industrial blockout, with further readability and finish work underway separately.

**Hull and glazing.** The new exterior and interior agree on the raised cabin. Both mining turrets sit farther outboard at ±5.2 m, with their rendered pivots, firing origins and camera positions kept consistent. The exterior GIF is a sequence of captured viewpoints, not a continuous orbit or flight recording.

![Raised bridge exterior — held-viewpoint study](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-exterior-study.gif)
![Raised bridge and matching canopy](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-exterior.png)

**Getting to the bridge.** Three real stair flights provide 45 treads, landings and guards between the lower access area and bridge. Route tests cover the stairs, both decks, boarding lift/hatches and seat entry/exit. The stairs GIF shows held inspection viewpoints; it does not claim continuous recorded walking.

![Three-flight access — held viewpoints, not continuous walking](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-stair-access-study.gif)
![Real stair treads, railings and landings](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-stairs.png)

**Working from the seat.** The nine-screen arrangement, full-face drone metrics, permanent cargo overview, reactor display and captain AUTO / OPEN / STOWED choices remain together in the raised cabin. Stowing a monitor changes its presentation, not the ship's mining job. The camera views retain visible cache ages and obstruction labels. A straight-ahead glance still crops some overhead metric tops; looking up and turning covers them, so readability and framing remain on the punchlist.

![Nine-screen mining controls — sampled stow and reopen replay](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-nine-screen-mining-controls.gif)
![Current raised-bridge mining cockpit](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-mining-cockpit.png)

**Warp and reactor.** The warp/arrival GIF shows sampled authority states: arrival remains stowed, followed by an explicit manual reopen with unassigned drone feeds. The cut omits transit time; arrival alone is not shown deploying all nine monitors. The reactor GIF uses captured 100 ms replay samples of rigid collars moving around a fixed core. Neither establishes live frame rate, a complete warp safety interlock or reactor physics.

![Warp override, arrival stowed and explicit manual reopen — sampled states](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-warp-arrival-states.gif)
![Fixed-core reactor collar movement — 100 ms replay samples](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-reactor-mechanism.gif)

**Checked locally:** 79 protocol tests, 14 asset-verifier tests and 263 server tests passed. Client results were 535 passing with the same 11 baseline failures. Eight GPU evidence suites passed with 142 receipt-verified captures; workspace checks and Clippy passed with warnings. Repeated art export was byte-identical. Registration 11 uses fresh fixtures and refuses incompatible old bindings; no user saves were migrated. These checks support the checkpoint, not a finished flagship or release.

[Capture provenance and timing](https://github.com/PIndustries/arkline-engine/blob/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/pioneer-raised-bridge-media-manifest.json). GIF and PNG files are published here; MP4 encoding records in the manifest describe companion exports, not additional repository files.

### Flagship finish line · 24 October checkpoint punchlist

The goal is a ship whose spaces, controls and systems work together. The following are acceptance gates, not promises that every simulation already exists.

| Workstream | Current position | What must be demonstrated before calling it finished |
|---|---|---|
| Cockpit readability and control finish | Active next pass | Clear seated/standing views, useful head turns, consistent labels, readable telemetry and dependable physical input without overlapping panels |
| Exterior and interior art | Functional blockout | Coherent finished surfaces, construction details, lighting, glazing and room equipment while retaining verified clearances and collision |
| Walkable decks and access | Raised route locally tested | Repeatable boarding, stairs, hatches, lift and seat journeys, including obstruction, restart and safe escape paths |
| Mass, power, heat, life support and damage | Partial foundations; broader simulation remains | Shared component accounting, meaningful power/fault behavior, conserved compartment systems and repair consequences; no decorative readings presented as a solver |
| Drones, mining and cargo | Working mining/control foundation | Safe recovery, real storage and energy boundaries, firing clearances, truthful metrics and conserved cargo handling |
| Flight, warp and reactor | Local configuration/control prototype | Equipment securing, interruption/recovery and closure interlocks driven by authority; actual load and thermal accounting remain separate work |
| Seamless docking and unloading | Docking workstream scoped; unloading follows | Continuous authoritative approach/alignment/capture/departure, safe reservations and restart; then timed transactional unloading without instant teleport or duplicate credit |
| Performance, saves and builds | Local checks with known failures | Representative native/browser frame pacing and feed freshness, resolved existing test failures, explicit migration policy and exact-head release checks |
| Public development journal | Updated with checkpoint evidence | Newest-first entries, matching GIFs/stills, exact provenance, and clear boundaries between local prototype, published source and release |

[Current workstream scope and ownership](https://github.com/PIndustries/arkline-engine/issues/62#issuecomment-6380550924). Next work prioritizes cockpit readability/panel finish and authoritative docking. Full life support, vessel mass/heat, general weapon interlocks and mobile video streaming are not delivered by this checkpoint. We will keep the older studies below so changes remain easy to compare.


#### 23 October · Earlier: integrated nine-camera cockpit

**Status:** locally validated prototype `28817c`; runtime source is not published. The published source checkpoint remains [`f2f3bb7`](https://github.com/PIndustries/arkline-engine/commit/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068), not a release or deployment.

The overhead order is **D1 · Port · D2 · D3 · D4 · Starboard · D5**, plus two rising turret views: nine dynamic camera monitors. Full-face drone instruments show actual HP, signal, heading/pitch, cycle and task data. The permanent cargo overview reports ore units, and the fixed-core reactor configuration display is now in the same local prototype. Exterior camera views are local scene captures, not network streams.

Captain **AUTO / OPEN / STOWED** controls change physical presentation without stopping drones, turrets or mining. Warp takes priority while retaining the captain's preference. This supersedes the earlier “manual control planned” and “not integrated” statuses below, which remain historical checkpoint descriptions.

![Integrated local cockpit: port-side exterior view, drone full-face metrics and turret monitors](https://raw.githubusercontent.com/PIndustries/arkline-engine/7d7bb8e639e46a59f0642d142ee6f5e22f5cf09e/docs/ships/pioneer/images/progress-20261023/integrated-28817c/deployed-port.png)

![Integrated local cockpit: permanent onboard cargo overview in ore units](https://raw.githubusercontent.com/PIndustries/arkline-engine/7d7bb8e639e46a59f0642d142ee6f5e22f5cf09e/docs/ships/pioneer/images/progress-20261023/integrated-28817c/deployed-cargo.png)

![Integrated local cockpit: fixed-core reactor configuration display](https://raw.githubusercontent.com/PIndustries/arkline-engine/7d7bb8e639e46a59f0642d142ee6f5e22f5cf09e/docs/ships/pioneer/images/progress-20261023/integrated-28817c/deployed-reactor.png)

**Evidence:** these stills come from controlled server-state replay, not a live-session FPS benchmark. Cached image ages remain explicit. Local checks recorded server 262 passing, client 535 passing with 11 inherited failures, verifier 14 passing, plus workspace and serialized Rustdoc checks. This uses fresh registration 10; no old-world migration or all-green suite is claimed. [Media provenance](https://github.com/PIndustries/arkline-engine/blob/7d7bb8e639e46a59f0642d142ee6f5e22f5cf09e/docs/ships/pioneer/images/progress-20261023/integrated-28817c/media-manifest.json).

![Historical integrated 28817c cockpit — held port, cargo and reactor viewpoints](https://raw.githubusercontent.com/PIndustries/arkline-engine/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/integrated-28817c-held-gallery.gif)

**Animation boundary:** this historical 23 October gallery holds three captured stills for comparison; it is not recorded motion and predates the raised bridge. [Gallery provenance](https://github.com/PIndustries/arkline-engine/blob/7e82a0690aab21a9d10cde906140f75393931031/docs/ships/pioneer/images/progress-20261024/integrated-28817c-gallery-manifest.json).

**Next:** representative native frame pacing and feed-freshness acceptance, followed by warp preparation and safety interlocks.

#### 23 October · Earlier: closer side consoles and per-drone metrics

**Status:** locally validated prototype `4819300`; runtime source not published and not integrated with the reactor replacement. Full-size side displays now use the empty side-console space and a 15° glance. Five cassette-front bands show real drone ID, HP, velocity-derived speed, mode, signal and cycle data. Viewing all five still requires head turns; storage is not modeled or shown as a number.

![Latest actual engine view: closer full-size side console at a 15-degree glance](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/ergonomics-side-console-15deg.png)

![Latest actual engine view: starboard drone feeds and cassette-front metrics](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/ergonomics-metrics-starboard.png)

![Animated latest local prototype: physical monitor deployment and accepted-warp stow](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/ergonomics-deploy-warp.gif)

*Actual captured deployment/warp samples from the same local prototype; GIF is 960 × 540 with edited endpoint holds. This is not the reactor-integrated build.*

**Next:** captain-controlled monitor stow/reopen without stopping equipment; integrate the separately validated prototypes, then check native readability, collision and sustained feed freshness. [Source/provenance manifest](https://github.com/PIndustries/arkline-engine/blob/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/media-manifest.json).

#### 23 October · Earlier: rebuilt reactor and dedicated configuration display

**Status:** locally rebuilt and revalidated replacement `a154a3c`; runtime publication pending. The original tested commit was not recovered. Four rigid collars slide around a fixed core; the reactor does not shrink. This is separate from the latest side-console prototype.

![Actual rebuilt reactor expanded configuration, diagnostic inspection camera](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/reactor-expanded.png)

![Actual cockpit view of the rebuilt reactor configuration screen in compact mode](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/reactor-cockpit-compact.png)

![Animated rebuilt reactor: collar compression and reopening from current replacement captures](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/reactor-compress-open.gif)

*New footage from the replacement checkpoint—not the older pre-rebuild GIF. Room views use a diagnostic inspection camera, not a player walking route; motion playback is edited, not a performance benchmark.*

**Next:** publish/integrate only through the separate source workflow, then add warp preparation/interlocks and real mechanism power/heat accounting. [Capture provenance](https://github.com/PIndustries/arkline-engine/blob/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/media-manifest.json).

#### 23 October · Earlier: seven physical camera monitors

**Status:** [published development checkpoint `f2f3bb7`](https://github.com/PIndustries/arkline-engine/commit/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068), not a merged release. Five drone monitors lower from ceiling cassettes; two turret monitors rise from the center console. Accepted warp stows them.

![Historical actual-engine animation: seven monitors deploy and stow for warp before the latest ergonomics refinement](https://raw.githubusercontent.com/PIndustries/arkline-engine/832d77704a43ac1d9d05a7b12f357fd4f3aa1da5/docs/ships/pioneer/images/progress-20261023/seven-screen-deploy-warp-historical.gif)

*Historical 1280 × 720 / 7.5-second edited feature capture. It predates closer side monitors and cassette-front metrics; it is not footage of the latest local prototype.*

**Next:** safe drone recovery and mechanism-closure interlocks. Current warp does not wait for mechanisms to close. [Mechanism report](https://github.com/PIndustries/arkline-engine/blob/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068/docs/ships/pioneer/physical-cockpit-mechanisms-20261023.md) · [failed exact-head CI](https://github.com/PIndustries/arkline-engine/actions/runs/39913590864).

#### 15 October and earlier · Exterior studies and concept history

The older [Study05 actual-engine comparison](#study05--inhabited-and-manufactured-scale--2026-10-15), [dated model history](#dated-3d-model-history) and [concept archive](#concept-history) remain below. Model-history rows and concept revisions are ordered newest first. **Concept images are art direction, not implemented-feature evidence.**

---

Pioneer is the flagship development ship: a coherent, inhabited industrial vessel, not a fixed layout with more screens added. The current design remains open to substantial improvement. Its centered cockpit should give the captain a clear view of flight, mining and the ship's condition, with an integrated wraparound console, usable side aisles, rear access and a readable hierarchy rather than a wall of unrelated tablets.

### What is implemented, and where

> Historical summary of the earlier 23 October checkpoints. The integrated nine-camera entry at the top supersedes the earlier planned/manual-control and separate-prototype statuses here.

These are **development checkpoints, not a merged release or deployed feature announcement**. The [active workstream and acceptance record](https://github.com/PIndustries/arkline-engine/issues/62) distinguishes implementation, local evidence and exact-head CI.

| Area | Status on 23 October | Boundary |
|---|---|---|
| Whole-ship spatial redesign | [Published checkpoint 49c17e9](https://github.com/PIndustries/arkline-engine/commit/49c17e9804c06427c423136303b16ea052506d23): deeper centered cockpit, wraparound console construction, rear access, framed canopy and two-deck room-program blockouts | Coarse spatial/mechanical foundation, not finished concept fidelity or a fully operational room fit-out |
| Dynamic mining cockpit | [Published checkpoint f2f3bb7](https://github.com/PIndustries/arkline-engine/commit/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068): five drone monitors lower from ceiling cassettes; two turret monitors rise from the center console | Seven distinct camera views at full mining activity; outer views require head turns. Displaying seven feeds does not mean seven full-rate video streams |
| Mechanism behavior | Confirmed vessel/activity state drives physical deployment and warp stow; shared sampled poses drive rendering, collision and input, with obstruction and persistence handling | Warp currently **does not wait for mechanisms to finish closing**. Motor mass, electrical demand and heat are not modeled |
| Reactor configuration | **Rebuilt and revalidated locally; source publication pending.** A fixed core envelope and four rigid segmented collars move along rails between expanded and compact configurations, with a dedicated cockpit configuration screen | It is mechanical compression, not the reactor shrinking. This is a newly validated replacement; the original tested commit was not recovered. This is not a thermodynamic reactor simulation or a released feature |
| Side-console ergonomics and drone metrics | **Validated local prototype; not published or integrated with the reactor replacement.** Full-size side monitors use closer placement and a 15° desktop glance; fully deployed turret screens clear NAV. Five cassette-front strips show actual drone ID, HP, velocity-derived speed, mode, signal and cycle data | Blockout remains unfinished; viewing all five metric bands and feeds requires head turns. Storage is not modeled or shown as a number. Representative desktop performance remains unverified |
| Captain's manual monitor control | **Requested next feature, not implemented**: manually stow or reopen the camera monitors without stopping their associated equipment | Closing a display must not stop drones, turrets or mining. Presentation preference and equipment activity need separate state; warp safety takes priority |

The [physical-mechanism report](https://github.com/PIndustries/arkline-engine/blob/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068/docs/ships/pioneer/physical-cockpit-mechanisms-20261023.md) supersedes earlier fixed-screen/paging proposals for these seven views. Each drone has a stable assigned view; the turret views show their real rendered perspective. Supports, housings and stowed covers are physical geometry, not just a sliding overlay. Unavailable, stale and obstructed views must remain truthful; a camera blocked by the ship's own hull must not be made clear by hiding that hull.

The published mechanism checkpoint uses experimental registration 6 and disposable fresh fixtures. It does not automatically migrate older saved layouts. Existing concept images and earlier studies below remain historical references, not captures of this new checkpoint.

### Drone metrics and side-console ergonomics (local prototype)

The locally validated ergonomics prototype adds five cassette-front metrics strips tied to stable drone identities and corrects the right/up camera-feed visibility gate independently of the legacy four-view NAV display. It reports actual authoritative fields; unsupported storage must not become an invented capacity, and stale/disconnected data must remain identifiable. Local validation recorded protocol 66, server 91 and verifier 14 passing; client 495 passed, 11 inherited failures and 30 ignored. Actual GPU evidence and workspace checks passed. These are local prototype results, not published source, reactor integration, a green full suite or a released feature.

### A connected, living ship

The target is one vessel model joining physical components and their actual behavior:

- **Structure and mass:** hull, installed equipment, cargo and consumables contribute to vessel mass, balance, inertia and load limits. Present cargo-dependent acceleration is not a complete mass ledger.
- **Power and heat:** generation, storage, distribution, breakers, essential/emergency loads, mechanism demand and cooling must affect what equipment can operate. Existing scalar capacitor and bounded airlock loads are foundations, not a whole-ship electrical or thermal solver.
- **Habitability:** compartments, doors, pressure boundaries, air circulation, oxygen/CO₂, temperature, water and emergency response need conserved state and meaningful consequences. Current scalar pressure behavior is not complete life support.
- **Mining and logistics:** drone bays, recovery, turret clearances, cargo capacity and physical handling should connect to the same authoritative state rather than independent visual effects.
- **Ship awareness:** the centerline cockpit needs a ship-wide overview with component/room condition, cause of faults and data freshness. Interior cameras should show real room activity with power, authorization and stale/offline states.
- **Future mobile viewing:** interior feeds need an authenticated network streaming architecture. Current camera views are local scene renders, not remotely accessible video streams; mobile streaming is planned.
- **Continuous docking and unloading:** approach, alignment, capture, clamps, service connection, safe access and cargo transfer should occur in-world without teleporting or a cutscene. Current dock/undock and deposit operations remain instant; cosmetic animation alone would not deliver this requirement.

These are design goals and engineering work, not claims that all systems now exist. The [whole-space report](https://github.com/PIndustries/arkline-engine/blob/f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068/docs/ships/pioneer/whole-space-redesign-20261023.md) records the underlying gaps and proposed compartment-first integration.

### Next implementation and acceptance priorities

> Earlier planning baseline, retained for history. Manual monitor controls and local prototype integration have since been implemented in the newest entry; publication and native acceptance remain separate.

1. **Manual monitor control:** separate captain visibility preference from drone/turret operation. Test stow/reopen during ongoing mining, safe reversal, obstruction, reconnect/restart, and warp precedence without issuing equipment-stop commands.
2. **Safe warp preparation:** recover drones rather than deleting their deployed state; retract equipment and close mechanisms before warp acceptance, with explicit blocked/recovery states. The current warp path clears deployed drones and does not supply that recovery sequence.
3. **Connected docking and cargo:** establish one station/hangar/berth coordinate contract and a persistent approach-to-services operation. Follow with timed, interruptible unloading using conserved inventory and idempotent transfer receipts.
4. **One real compartment end to end:** connect a feeder, lamp, vent, door controller, emergency reserve and interior camera through stable component IDs. A breaker trip must change actual behavior and the cockpit overview; restart must preserve the state. Expand only after this slice works.
5. **Visual and performance acceptance:** integrate and review the locally validated closer side-monitor layout and cassette-front metrics against the approved concepts; full native acceptance and finished construction remain outstanding. Refine integrated instrument bays, chair, canopy joins, lighting/readability and room construction against the approved concepts. Measure sustained frame pacing and feed freshness on representative desktop/browser hardware, not only screenshots.

Formatting, runner prerequisites and inherited failing tests remain a cross-cutting acceptance gate, not something to defer until final art. [CI for f2f3bb7](https://github.com/PIndustries/arkline-engine/actions/runs/39913590864) **failed**: the issue's verification record reports Linux/macOS formatting stops and an unavailable browser Clippy command. Local checkpoint results recorded 65 protocol tests, 91 server tests and 14 verifier tests passing; the client recorded 489 passing, **11 inherited failures**, and 30 ignored. Those results are not an all-green suite. Software-rendered capture evidence establishes behavior, not adequate interactive desktop performance; representative desktop benchmarking remains outstanding.

This dated summary takes precedence for the new development work. The sections below preserve the earlier model, runtime and concept history.

## Design direction

![Pioneer current v05 concept](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v05-interior-review.jpg)

v05 calls for a broad, enclosed black/emerald cutter: faceted bow, integrated shoulders, heavy collars, a stepped underside and articulated laser mining turrets. The separated outriggers in v03 and earlier drill-like equipment are historical. Generated labels, checkboxes and painted proportions are not acceptance evidence.

The merged [authored model](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/README.md) is the first exterior foundation: 32,284 runtime triangles in seven material batches, with separate engine-controlled turrets. Draft [#44](https://github.com/PIndustries/arkline-engine/pull/44) adds an industrial detail pass and changes the GLB to 65,866 triangles in nine batches while retaining the body envelope and gameplay hardpoints. Neither version is a finished reconstruction of v05.

The earlier industrial baseline had a narrow center, projecting square nose and long side blocks. Studies 01–04 broadened and enclosed the hull, moved glazing forward and integrated the shoulders. Study05 now corrects the bow/glazing, roof architecture and actual drive openings in the engine. Broader side/collar, underside and overall proportion/detail matching remain deliberate v05 work. See the [modeling plan](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6183334728), [Study05 development record](https://github.com/PIndustries/arkline-engine/issues/10#issuecomment-6199911507) and open checklist below.


## Study05 · inhabited and manufactured scale · 2026-10-15

The two-area pass follows the published v05 architecture: a more compact **bow/bridge**, and purposeful **service/work architecture**, including the existing drive exits. Thick separated armor panels, connected braces, continuous gaskets, actual pane recesses, fixed service covers/handles and human-scale rails provide construction cues. The roof now has distinct access, exchanger and folded handling areas. The arm, hatches and service detail remain static inferred authoring choices; no operating equipment or fitted crew interior is claimed.

| Study04 before · actual7036 | Study05 refined · actual7040 |
| --- | --- |
| ![Before same body camera](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/before/pioneer-authority-idle-front-quarter.png) | ![Refined Study05 same body camera](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front-quarter.png) |

| Bridge before | Bridge refined |
| --- | --- |
| ![Before white planar bridge output](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/before/pioneer-authority-idle-bridge-quarter.png) | ![Refined recessed bridge and armor joints](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-bridge-quarter.png) |

| Recessed work area | Existing drive in a real opening |
| --- | --- |
| ![Raised paired handling links in recessed service area](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-aft-roof.png) | ![Actual recessed existing bell throat and seated rim](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-port-nozzle.png) |

[View the model comparisons](#matched-view-gallery).

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#study05--inhabited-and-manufactured-scale--2026-10-15).


## Dimensions and frames

[JSON/source values](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/specification.json) · [Dimensioned SVG](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/plan.svg) · [Design reference](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/docs/ships/design-reference.md)

Current gameplay definitions in the target revision's [protocol](https://github.com/PIndustries/arkline-engine/blob/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/crates/arkline-protocol/src/lib.rs) govern behavior. The dossier JSON pins an older source snapshot and separates implemented values from proposed reservations; it is not a live query. SVGs visualize those numbers. Concept-image pixels are not measurements.

| Reference | Meaning |
| --- | --- |
| Body envelope | 72 m length × 38 m width × 18 m hull height. Articulated turrets extend above the hull; this is not an 18 m overall-height claim. |
| Runtime model | Metres, centered hull-envelope origin; +X bow, +Y up, +Z starboard. XYZ extents are 72 × 18 × 38 m. The loader does not resize the GLB. |
| Dossier plan | X forward from the aft bounding-box face, Y up, Z from the port face; `bounds_xzy_lwh_m` arrays use X,Z,Y ordering. This differs from model XYZ and its centered origin. |
| Interior instance | Two 32 × 14 m local plans; each deck has floor Y=0, X=plan U, Z=plan V. A tested body-to-deck registration remains missing. |
| Human reference | 1.8 m avatar, 2.4 m corridor and 1.4 m doorway width. Proposed clear door/room heights are 2.4/3.2 m, with 3.8 m deck pitch. |
| Gameplay quantities | 1,000 cargo units; 240,000 J charge capacity; 1,200 W regeneration; 30,000 J warp admission; 4,000 J turret mining cycle. These are game tuning, not reactor or structural ratings. |

Cargo units have no implemented kilogram or cubic-metre conversion. Mass, material density, pressure rating, structural strength and engineering adequacy must remain unknown until supported by declared inputs and validation. Matching bounds alone proves neither interior fit nor safe clearances.


## Current runtime

> Historical baseline recorded on 15 October 2026. See the dated 23 October progress section above for newer development checkpoints; this section does not describe the latest experimental layout.

**Merged on main:** [#29](https://github.com/PIndustries/arkline-engine/pull/29) implements two private, docked Pioneer decks with twelve rooms, authoritative walking/collision, first/third-person cameras, an access-core lift transition and return through the airlock to boarding. The lift changes to a validated fixed landing; it is not a moving lift. [#34](https://github.com/PIndustries/arkline-engine/pull/34) adds compatible-client recovery. Ship operation is gated while on foot. See [interior behavior and rollback limits](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Interior-Preview).

[#39](https://github.com/PIndustries/arkline-engine/pull/39) replaces the procedural hull with the embedded GLB and editable Blender source. Its actual cached triangles serve the main view, hangar, camera feeds, shadows and selection. The bounded loader supports static indexed geometry and opaque, untextured scalar PBR materials; textures, skins, animation and morph targets are rejected. See [model runtime](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Model-Runtime).

Existing cockpit flight, finite-ore mining, drones, cargo/storage, resource jobs and scalar reactor/capacitor behavior are engine systems. Decorative pipes, radiator shapes, hatch geometry and lamps do not create a conserved electrical, atmosphere, water, heat or structural model. The interior still occupies its own scene; the exterior glazing does not establish the bridge room or pilot-eye location.

**Unmerged stack:** [#42](https://github.com/PIndustries/arkline-engine/pull/42) adds physical personal-fleet berths and boarding; [#44](https://github.com/PIndustries/arkline-engine/pull/44), based on #42, connects the D1-P1 airlock controller. That room has typed breaker/light/vent/door operations, actual versus commanded outputs, an emergency reserve, pressure interlocks and a door obstacle shared with authoritative collision. Its loads reduce the existing reactor recharge budget. Pressure is a bounded single-room scalar model against a fixed corridor reference, not conserved whole-vessel atmosphere. [Room controls](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Room-Controls) and the [equipment catalog](https://github.com/PIndustries/arkline-engine/wiki/Pioneer-Equipment-Catalog) describe this development slice, not merged main behavior.

[Room fixture plans](https://github.com/PIndustries/arkline-engine/blob/cb90a340eb2f92497c5136ffddc13414d479f0c9/docs/ships/pioneer/rooms/README.md) · [Systems SVG](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/systems.svg) · [Systems JSON](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/systems.json) · [Proposed vessel engineering contract](https://github.com/PIndustries/arkline-engine/wiki/Vessel-Engineering-Contract)


## Remaining gaps

> Historical baseline recorded on 15 October 2026. The dated 23 October progress section above supersedes this list where newer development is explicitly documented.

Overall v05 shape/material completion, moving doors and broader component behavior remain unfinished.

1. Finish native acceptance for material recovery, cockpit/import workflows, lifecycle/restart/reconnect and changed-room walking. Broader multiplayer interiors, physical cargo handling, operable exterior doors, fault/repair and conserved power/fluids/heat remain open design and implementation work.
2. Rework the large hull forms against v05, then verify front/top/side and three-quarter engine captures with metric bounds and the dynamic turrets present. Detailed textures/glass/LOD support and measured performance need their own evidence.
3. Register exterior, bridge/cockpit camera, deck plans, openings, collision and mounts in one tested frame. No fitted bridge, continuous external hatch traversal, pressure hull, flight interior or full-ship CAD fit is claimed.
4. Preserve durable part/assembly IDs and explicit parent/source-to-render transforms before editable pieces join the engine. Exporting all 686 current source meshes individually would exceed the current 128-primitive/mesh decoder ceiling.
5. The [authored airlock-section proposal](https://github.com/PIndustries/arkline-engine/issues/43#issuecomment-6183327461) is deferred next feature work: identified fixtures, constrained placement, one installed lamp-feed connection, accepted authority revision and save/reopen. It must reuse the existing equipment IDs/interlocks and distinguish installed state from draft topology. No implementation has started under that proposal.

Tracking: [model/asset pipeline #10](https://github.com/PIndustries/arkline-engine/issues/10), [interiors #23](https://github.com/PIndustries/arkline-engine/issues/23), [vessel engineering #32](https://github.com/PIndustries/arkline-engine/issues/32), [hangar #40](https://github.com/PIndustries/arkline-engine/issues/40), [visual quality #41](https://github.com/PIndustries/arkline-engine/issues/41), [connected equipment #43](https://github.com/PIndustries/arkline-engine/issues/43), [editor #45](https://github.com/PIndustries/arkline-engine/issues/45), [room authoring #50](https://github.com/PIndustries/arkline-engine/issues/50), [shared development sessions #51](https://github.com/PIndustries/arkline-engine/issues/51). These issues cover broader work than the current slices; this record does not close them.


## Dated 3D model history

Each row preserves the actual editable Blender master and its GLB, rather than a generated illustration. Download links pin full Git commits; later replacements at the stable study paths retain every earlier version in Git history. All listed hulls use the same centered **72 × 18 × 38 m XYZ** envelope. Moving gameplay turrets are separate and omitted from the static Blender study images; they appear in the later actual-engine captures. None of these rows establishes a fitted interior or a released build.

| Date (UTC) / revision | Source and downloads | Change and evidence status |
| --- | --- | --- |
| 2026-10-15 · Study05 refined | [Editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/pioneer-study05.blend) (592,067 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/pioneer-study05.glb) (8,700,948 B); [source-authoring record](https://github.com/PIndustries/arkline-engine/blob/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/study05.json), [export receipt](https://github.com/PIndustries/arkline-engine/blob/3e0be4df586e70a152ce10397702d43193c35130/assets/ships/pioneer/studies/study05/export.json), [actual 7040 views](https://github.com/PIndustries/arkline-engine/tree/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined). | Source 61eb70e1… / runtime 8125644f…, 686 parts, 103,466 triangles, nine primitives. Thick separated brow plates, connected gaskets/braces, raised paired handling links and seated nozzle rims. **Selected bow/service scope visually reviewed in actual engine; overall v05 unfinished.** |
| 2026-10-15 · Study05 first engine preview | [`a3928f2`](https://github.com/PIndustries/arkline-engine/commit/a3928f23eefc171c12830e496fbd60ccf773dea7); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/a3928f23eefc171c12830e496fbd60ccf773dea7/assets/ships/pioneer/studies/study05/pioneer-study05.blend), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/a3928f23eefc171c12830e496fbd60ccf773dea7/assets/ships/pioneer/studies/study05/pioneer-study05.glb); [actual 7038 views](https://github.com/PIndustries/arkline-engine/tree/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/first). | Source 9cc3ac5d… / runtime df8490f4…, 670 parts, 98,858 triangles. Recessed bridge/workdeck and real aft openings; actual review found floating braces, weak arm silhouette and coplanar rim overlap. **Superseded by the Study05 refinement above; preserved.** |
| 2026-10-15 · Cloud saved-master export and engine adoption | [`7747079`](https://github.com/PIndustries/arkline-engine/commit/7747079c7f43017928fed41dc2699c76e7469cc0); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer.blend) (497,325 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer.glb) (6,582,300 B); [export receipt](https://github.com/PIndustries/arkline-engine/blob/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer-export.json), [adoption record](https://github.com/PIndustries/arkline-engine/blob/7747079c7f43017928fed41dc2699c76e7469cc0/assets/ships/pioneer/pioneer-adoption.json). | Unchanged Study 04 source `772bf76a…`; fresh normal-preserving `ed5e29cd…` export, 78,244 triangles / nine batches. **Installed in cloud native7031 and actual engine7032 with powered exterior and existing turrets. Intermediate fidelity; unmerged.** |
| 2026-10-15 · Study 04 | [`30bd538`](https://github.com/PIndustries/arkline-engine/commit/30bd5381ea3c2ef553f4235a5a0c026184dada18) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/pioneer-study.blend) (497,325 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/pioneer-study.glb) (7,664,728 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/30bd5381ea3c2ef553f4235a5a0c026184dada18/assets/ships/pioneer/studies/study.json). | Clean closed brow facets replace the crossing fan; green collars and recessed deck retained. **Reviewed development candidate; cloud 7102 decoder/staging passed, no game adoption.** |
| 2026-10-15 · Study 03 | [`ebf6fa2`](https://github.com/PIndustries/arkline-engine/commit/ebf6fa2746bd01bd3e27be93810a19de0091fb16) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/pioneer-study.blend) (511,253 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/pioneer-study.glb) (7,756,576 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/ebf6fa2746bd01bd3e27be93810a19de0091fb16/assets/ships/pioneer/studies/study.json). | Exterior green pod collars, connected corner glass and restored upper RCS surface; crossing corner fan rejected. **Superseded after actual visual review; no engine adoption.** |
| 2026-10-15 · Study 02 | [`f099e43`](https://github.com/PIndustries/arkline-engine/commit/f099e4372908530393e374710e685018462e3cc4) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/pioneer-study.blend) (405,669 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/pioneer-study.glb) (6,917,496 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/f099e4372908530393e374710e685018462e3cc4/assets/ships/pioneer/studies/study.json). | Closed armor volumes and recessed service deck; old gray covers hide much of the collars. **Superseded study; native .5 decoder/staging passed, no game adoption.** |
| 2026-10-15 · Study 01 | [`e4a80b2`](https://github.com/PIndustries/arkline-engine/commit/e4a80b299dd7258efb021a4da6e0b367ce18ded8) → draft #47; [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/pioneer-study.blend) (342,348 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/pioneer-study.glb) (6,531,236 B); [hashes/checks](https://github.com/PIndustries/arkline-engine/blob/e4a80b299dd7258efb021a4da6e0b367ce18ded8/assets/ships/pioneer/studies/study.json). | Widened connected pressure hull and frontal glazing; shoulder overlay intersections remain. **Superseded study; no engine adoption.** |
| 2026-10-12–15 · Industrial baseline | Draft [#44](https://github.com/PIndustries/arkline-engine/pull/44), combined [#47](https://github.com/PIndustries/arkline-engine/pull/47), [`3e41520`](https://github.com/PIndustries/arkline-engine/commit/3e415204747cd9adaa146a9777831a7acc82e7cb); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e415204747cd9adaa146a9777831a7acc82e7cb/assets/ships/pioneer/pioneer.blend) (353,015 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e415204747cd9adaa146a9777831a7acc82e7cb/assets/ships/pioneer/pioneer.glb) (6,736,012 B). | 65,866 triangles / nine batches. Actual packaged .5 game hull at this checkpoint; draft stack remains unmerged. |
| 2026-10-12 · First authored hull | [Merged #39](https://github.com/PIndustries/arkline-engine/pull/39), [`78fdc5d`](https://github.com/PIndustries/arkline-engine/commit/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175); [editable .blend](https://raw.githubusercontent.com/PIndustries/arkline-engine/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/pioneer.blend) (227,320 B), [GLB](https://raw.githubusercontent.com/PIndustries/arkline-engine/78fdc5db83d195ce1909c0b302c0bfe3dbe7b175/assets/ships/pioneer/pioneer.glb) (1,689,712 B). | 32,284 triangles / seven batches; first embedded GLB foundation. Installed in merged source; release not asserted. |

The original baseline source/export SHA-256 values are `8ec754fc7afe6b67dcab6cbd6694bb04bdc03cc12e91ee96e0bca9e08bb24f6f` / `f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402`. Study receipts record each later master/export hash and its checks. The frozen Study 04 export has 543 named source meshes before derived batching and 78,246 runtime-format triangles in nine batches. The historical Study04 cloud export from that same master has 78,244 retained triangles after the explicit engine float32 degeneracy policy; the frozen file is preserved. Its independent static checks preserve both turret supports and all six RCS intersections, with upper hit approximately Y8.3 m. Native staging uses the real importer; static checks and Blender renders remain separate from GPU acceptance.

### Matched view gallery

The baseline and Study01–04 rows are source renders with the same cameras, lighting, orthographic scale and resolution. The cloud-engine rows uses the separately recorded body-relative cameras and actual sunlight; it is not the same render setup. The supplied concept has a different perspective and provides visual direction, not measured CAD. Select each linked view for its full image.

| Revision | Matched views |
| --- | --- |
| Study05 refined · engine7040 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-front-quarter.png) · [rear](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-rear.png) · [bridge close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-bridge-quarter.png) · [roof close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-aft-roof.png) · [nozzle close](https://raw.githubusercontent.com/PIndustries/arkline-engine/3e0be4df586e70a152ce10397702d43193c35130/docs/ships/pioneer/images/runtime-study05-20261015/refined/pioneer-authority-idle-port-nozzle.png) |
| Cloud installed · engine7032 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png) · [rear](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-rear.png) · [bottom](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-bottom.png) |
| Study 04 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 03 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/ebf6fa2746bd01bd3e27be93810a19de0091fb16/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 02 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/quarter.png) |
| Study 01 | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/quarter.png) |
| Before remodel | [front](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/front.png) · [side](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/side.png) · [top](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/top.png) · [quarter](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/quarter.png) |

| Before remodel | Latest reviewed study 04 |
| --- | --- |
| ![Before Pioneer remodel](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/quarter.png) | ![Pioneer study04 actual editable source](https://raw.githubusercontent.com/PIndustries/arkline-engine/30bd5381ea3c2ef553f4235a5a0c026184dada18/docs/ships/pioneer/images/model-study/quarter.png) |

| Superseded study 01 | Superseded study 02 |
| --- | --- |
| ![Pioneer study01](https://raw.githubusercontent.com/PIndustries/arkline-engine/e4a80b299dd7258efb021a4da6e0b367ce18ded8/docs/ships/pioneer/images/model-study/quarter.png) | ![Pioneer study02](https://raw.githubusercontent.com/PIndustries/arkline-engine/f099e4372908530393e374710e685018462e3cc4/docs/ships/pioneer/images/model-study/quarter.png) |

[Download supplied full-resolution reference](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/supplied-reference.png) · [Baseline capture provenance](https://raw.githubusercontent.com/PIndustries/arkline-engine/6549a3800173deb8ee3730503073f912c5b271df/docs/ships/pioneer/images/model-baseline/capture.json). The seven earlier concept/runtime images remain below. This page supplies rendered views and actual model downloads; no embedded interactive 3D viewer is supplied.

Study04 remains preserved as a historical intermediate. Study05 changes the actual source geometry and the declared Window presentation parameters, while retaining the existing systems/hardpoints. Prior-hash graph and accepted-world mismatch protections remain active; no automatic migration is supplied.

The visual acceptance checklist remains open. Study05 resolves its bounded bow/service architecture corrections; remaining whole-ship work is tracked separately:

| Area | Remaining visible gap |
| --- | --- |
| Bow / bridge | Study05 has a shallower chin, three thick faceted brow plates, actual recesses/gaskets and connected frames. Opaque scalar-PBR glazing remains an approximation; no fitted bridge is shown. |
| Roof / work area | Study05 exposes recessed access/exchanger/handling areas with a fixed folded arm and reels. The arm is less prominent than the raised reference crane, and no operation is implemented. |
| Underside | A mostly continuous lower beam; stepped armor, supports and service volumes need actual bottom/side review. |
| Side bays / collars | Equal repeated panels and broad green slabs; the concept has more varied depths, layered structure and fittings. |
| Aft drives | Study05 opens the actual caps and seats flanges in counterbores around existing bells/throats. Deeper coincident graphite floors remain behind the flanges; no overlap-free CAD or propulsion claim. |

Existing dynamic mining turrets belong to the engine and appear in the new engine comparison; the static study images do not justify adding duplicate turrets to the hull. Fixture colors, wattages and pulse timing remain gameplay assumptions. Model adoption is an intermediate implementation checkpoint, not completion of this checklist.


## Earlier combined Study04 checkpoint · 2026-10-15

![Installed Pioneer in the actual engine](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png)

| Exterior circuit OFF · drive IDLE | Exterior ON · drive IDLE |
| --- | --- |
| ![Exterior OFF at the same paused time](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-exterior-off.png) | ![Exterior ON at the same paused time](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-idle-front-quarter.png) |

| Dorsal strobe ON | After real +50 ms · still ON | After real +100 ms · OFF |
| --- | --- | --- |
| ![Actual dorsal strobe ON close view](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-on.png) | ![Actual dorsal strobe at plus50ms](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-step-50ms.png) | ![Actual dorsal strobe OFF close view](https://raw.githubusercontent.com/PIndustries/arkline-engine/7747079c7f43017928fed41dc2699c76e7469cc0/docs/ships/pioneer/images/runtime-remodeled-20261015/pioneer-authority-strobe-off.png) |

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#earlier-combined-study04-checkpoint--2026-10-15).


## Earlier powered exterior checkpoint · industrial baseline · 2026-10-15

| Exterior OFF / drive OFF | Exterior ON / drive IDLE | Exterior ON / drive ACTIVE |
| --- | --- | --- |
| ![Actual engine exterior OFF](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-off.png) | ![Actual engine exterior IDLE](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-idle.png) | ![Actual engine exterior ACTIVE](https://raw.githubusercontent.com/PIndustries/arkline-engine/e7cc2313246cd5d04ce60c45eee19e5e18fabe90/docs/ships/pioneer/images/runtime-exterior-20261015/gpu-active.png) |

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#earlier-powered-exterior-checkpoint--industrial-baseline--2026-10-15).


## Concept history

Superseded images are retained for review. Early drafts include rejected pale hulls, drill-like equipment, invented brand marks, misleading checkboxes or inconsistent dimensions. Their annotations are not authoritative. The accepted direction is black/emerald industrial construction styling with articulated laser mining turrets.

### v04-interior-draft

![Pioneer v04-interior-draft](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v04-interior-draft.jpg)

### v03-black-green-construction

![Pioneer v03-black-green-construction](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v03-black-green-construction.jpg)

### v02-laser-correction

![Pioneer v02-laser-correction](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v02-laser-correction.jpg)

### v01-drill-draft-superseded

![Pioneer v01-drill-draft-superseded](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v01-drill-draft-superseded.jpg)

### v00-initial-study

![Pioneer v00-initial-study](https://raw.githubusercontent.com/PIndustries/arkline-engine/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/pioneer/images/v00-initial-study.jpg)

## Historical exterior capture

This actual renderer capture from [#33](https://github.com/PIndustries/arkline-engine/pull/33) preserves the procedural cutter step before the authored GLB. It is historical evidence, not the current hull or concept art.

![Pioneer historical procedural exterior](https://raw.githubusercontent.com/PIndustries/arkline-engine/80eafbb/docs/ships/pioneer/images/engine-surface-preview.jpg)


## Technical references

### Source-to-model and live-system checklist

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#source-to-model-and-live-system-checklist).


### Editor and asset work

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#editor-and-asset-work).


### Editable vessel, animations and lights

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#editable-vessel-animations-and-lights).

#### What exists and what must change

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#what-exists-and-what-must-change).

#### Dependency order

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#dependency-order).

#### First moving-door and light acceptance

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#first-moving-door-and-light-acceptance).


### Evidence and acceptance

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#evidence-and-acceptance).


### Milestone log

[Technical record](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/ships/pioneer/technical-history.md#milestone-log).


## Provenance and upkeep

The concept JPEGs are compressed documentation previews; original PNG hashes and preview hashes are recorded in the [image manifest](https://github.com/PIndustries/arkline-engine/blob/214348845109dbc9cbe8f09b742a4c91228cef43/docs/ships/image-manifest.json). The original concept archive supplies no executable game assets. The Blender/GLB assets were added separately by #39 and refined in the unmerged #44 stack.

Keep this as Pioneer's lifecycle index. Append dated decisions and evidence, retain superseded designs, and update each work item's implementation/review/native/release state explicitly. The [repository Markdown workflow](https://github.com/PIndustries/arkline-engine/blob/feat/account-editor-foundation/docs/README.md) keeps the versioned source reviewable, then synchronizes reviewed content to this wiki; avoid a second independent narrative.
