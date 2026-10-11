# Models and community artwork

Browse the [ship design archive](ships/README.md) for the current seven-ship fleet, concept history, dimensioned planning reservations and proposed onboard systems. These are review documents; they do not imply that GLB importing or walkable ship interiors have shipped.

## What runs today

The client creates its geometry in Rust. `apps/client/src/flight3d.rs` builds vertex
positions and normals from primitive meshes, assembles ship parts, and fits their
measured bounds to hull dimensions from the protocol. Station and cockpit geometry
also come from the renderer. Current geometry is not loaded from GLB, FBX or OBJ files.

The offline `arkline-asset-tool inspect` command can validate a bounded GLB submission and report its geometry, scene bounds and named nodes. It does not install a model. A GLB
file cannot yet be dropped into the game and rendered. Current animation and
interactive cockpit behavior are implemented in code; imported animation, materials,
collision and attachment points need their own integration.

## Proposed contribution format

We intend to use glTF 2.0 / GLB as the model interchange format. This is a planned
pipeline, not an implemented compatibility promise. Artists can prepare original
models and discuss them in an issue before building a large asset pack.

Include the editable source project, preview images, author and license information,
intended dimensions in meters, triangle counts and texture sizes. Keep turrets,
drives, doors and other moving parts distinct. Identify intended attachment points
and collision shapes. Use original work or material you have the right to contribute;
include third-party licenses and attribution. Do not submit extracted assets from
other games.

Before model imports ship, we need bounded and validated file loading, texture and
material support, coordinate/scale conversion, mesh selection and collision handling,
attachment/animation conventions, and performance checks on desktop and browser.
Triangle and texture budgets will be defined from measured performance; there is no
published numeric budget yet.

## Inspect a contribution

```sh
cargo run --locked -p arkline-asset-tool -- inspect assets/examples/calibration-cube.glb
cargo run --locked -p arkline-asset-tool -- inspect /path/to/your-ship.glb > inspection.json
```

The original calibration cube is one meter along each axis; regenerate it with `python3 tools/make-example-glb.py`. The tool reports SHA-256, counts, transformed scene bounds, named nodes, materials, embedded image count, animation and skin count. It uses the validated [gltf parser](https://docs.rs/gltf/1.4.1/gltf/struct.Gltf.html). It never fetches external buffers, images or URLs.

The inspection profile requires a self-contained GLB with an explicit scene and triangle primitives. It caps file size at 32 MiB, decoded vertices at one million, total triangles at one million, and scene traversal depth/work. Sparse accessors must be expanded before inspection; empty, overlapping-stride, misaligned or out-of-range accessor layouts are rejected before decoding. These are tool resource limits, not a promise that an asset performs well in-game. Textures/animations are counted, not decoded or simulated.

See the [contribution checklist](art-contribution-checklist.md) for the required source, ownership/license, scale, attachment and review information. Runtime loading, textures/materials, animation, attachments and collision remain separate implementation work.
