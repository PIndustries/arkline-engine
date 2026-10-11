# Models and community artwork

## What runs today

The client creates its geometry in Rust. `apps/client/src/flight3d.rs` builds vertex
positions and normals from primitive meshes, assembles ship parts, and fits their
measured bounds to hull dimensions from the protocol. Station and cockpit geometry
also come from the renderer. Current geometry is not loaded from GLB, FBX or OBJ files.

There is no glTF/GLB parser, model browser or import command in this release. A GLB
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
