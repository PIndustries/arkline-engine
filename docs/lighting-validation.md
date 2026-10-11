# Lighting validation

The renderer combines explicit rock, ore, painted hull, metal and opaque-backed glass materials with bounded raster sun shadows and nearby mining/thruster lights. Indirect and specular environment light samples the same procedural sky as the background. This is an approximation of distant illumination, not reflections of nearby objects or ray tracing. Geometry and sky use one linear-light, filmic, sRGB output path.

## Repeatable automated fixtures

Run from the repository root on macOS with an OpenGL context available:

```sh
cargo test --locked -p arkline-client
cargo test --locked -p arkline-client native_camera_cache_composites_and_restores_state -- --ignored
cargo test --locked -p arkline-client native_local_light_has_bounded_illumination -- --ignored
cargo test --locked -p arkline-client native_hangar_renders_real_geometry -- --ignored
cargo clippy --locked -p arkline-client --all-targets -- -D warnings
cargo clippy --locked -p arkline-client --target wasm32-unknown-unknown -- -D warnings
```

These fixtures create an offscreen context without launching the game. They compile the integrated scene shader, compare equal-color material pixels, check view-dependent sky reflection, exercise the actual material BRDF with a local light, verify asteroid depth occlusion, bounded local-light falloff, cached feed compositing and GL state restoration. They do not measure gameplay FPS or substitute for WebGL runtime validation.

## Hands-on acceptance before release

Use an isolated local server and client built from the same revision; keep production saves untouched. Record the revision, platform, viewport, GPU and measured frame interval with each capture.

1. Inspect a ship at the station in External view, then Cockpit and Tactical. Check readable hull facets and cockpit instruments, dark metal versus painted surfaces, and stable lighting during orbit/headlook.
2. Travel to the belt and inspect rock/ore at several view angles. Place a ship or rock between another surface and the sun: nearby shadows should move coherently without a hard camera-following square. Shadow coverage is local (2.4 km half-width), not system-wide.
3. Lock a rock and activate one, then both mining modules. Check localized impact illumination only while an authoritative active cycle and valid aligned target exist. Stop mining and disconnect: stale snapshots must not keep event illumination alive.
4. Thrust and brake near geometry, then coast. Plume illumination must follow confirmed acceleration, not velocity alone.
5. Switch Low/Balanced/High and compare measured frame interval. Main shadow maps are disabled/1024/2048 pixels respectively; local lights are capped at 2/4/8. Drone/remote feeds use at most 512-pixel shadows and two lights; Low also disables their shadows. Open one and four feeds and verify selection/camera controls remain responsive.
6. Enter and leave the hangar; a previous flight shadow map or local light must not leak into the separate interior coordinate space. Resize and repeat mode/feed transitions.
7. Repeat in the browser, check WebGL errors and compare screenshots against native. Validate reduced GPU budgets on a modest device before choosing any performance claims.

At integration time the client unit suite, native offscreen GPU fixtures and native/WASM strict compilation passed. Interactive native/browser visual quality, sustained frame pacing and hardware coverage remain release gates; nothing in this change deploys a release.

## Station planet appearance

The station is about 120 km above the planet's north pole. The visible globe cap has `abs(sphere.y)` approximately 0.98–1.0, above the existing polar-ice mask's 0.94 upper threshold. A broad pale disk from the station is therefore expected polar ice, not evidence of clipped exposure. The planet branch and final `ark_display` transfer function are unchanged from `e590a068`. Geography has not been rotated or altered for the lighting update. Station metal can appear dark blue because it reflects the procedural sky rather than the previous artificial diffuse fill.

## Browser acceptance on the integrated renderer

Interactive Mac browser QA at 1280×720 on `40961f2` confirmed docked hangar, undocking, cockpit/external/tactical transitions, a two-AU warp, timed asteroid lock, sustained mining, mining drones and four camera feeds. The Balanced-quality retake confirmed the sloped-asteroid shadow stripes were gone, with no WebGL warnings or errors observed. The displayed frame counter was around 119–122 FPS after settling. This is one-device observation, not a benchmark or a performance guarantee. The fixed renderer matches the earlier gameplay run; the retake specifically rechecked Balanced shadows.

## 26.10.11.9 release preparation

`BUILD_VERSION` is prepared for 26.10.11.9. No release tag, artifact publication or deployment is implied. Required sequence:

1. Finish PR review and the latest Linux/macOS/browser validation run, then merge the exact approved revision. Do not substitute a previously validated head for the final revision.
2. Dispatch `release.yml` from main with `platform=all`. Existing self-hosted runners build Linux, Windows, macOS and browser artifacts; wait for all jobs and verify the receipts, source revision, version, licenses and SHA256 manifests.
3. Smoke-test the packaged native clients and browser output. Native offscreen GPU coverage does not prove all packaged device/driver combinations. The existing 1280×720 overlay overlap remains a separate usability issue.
4. Only after acceptance, run `publish.yml` with that successful release run ID, `version=26.10.11.9` and `upgrade_servers=false`. This client-renderer release does not need a server restart or data migration.
5. Verify public assets against manifest hashes, browser startup/rendering, native connection, website links and current source/license links. Record exact image/artifact identity and rollback references. Keep the previous release available until the new journey is verified.
