# Embedded Pioneer model runtime

The original Pioneer GLB is embedded in the client. It needs no file picker, network asset service, texture downloads or runtime Blender installation. Other ship classes retain their current procedural models.

## Supported authoring contract

- Path: `assets/ships/pioneer/pioneer.glb`.
- Metres, hull-envelope origin, +X bow, +Y up, +Z starboard. Body envelope is 72 m long, 38 m wide and 18 m high. The loader does not normalize or resize the asset.
- One embedded binary buffer, an explicit default scene, static triangle primitives with positions, normals and indices. Node transforms are applied once; mirrored transforms reverse triangle winding.
- Opaque, untextured metallic/roughness materials. Base color, roughness, metallic fraction and RGB emissive factors are supported. Textures, skins, animation and morph targets are rejected rather than silently dropped.
- A 16 MiB file limit, 128 decoded primitive limit, 200,000 triangle limit and 32-level hierarchy limit bound decoding. The Pioneer draw-call regression has a tighter 16-material-batch budget.
- The dynamic mining turrets remain engine geometry at their existing hardpoints. Static roof geometry must support the pedestal rays at body x=10.8 m, z=±2.66 m. The main hull must intersect the six principal-axis reaction-nozzle rays.

The decoder runs once. GPU buffers are uploaded once per renderer, and the same triangles are used by the main view, hangar, camera feeds, sun-shadow pass and pointer picking. Per-frame ship drawing borrows cached model parts. Invalid embedded data produces a renderer initialization error. It does not fetch a replacement asset.

This is an original model implementation, not a claim that every concept-art surface or proposed utility has been built. Ship interiors, gameplay systems, external hull registration and final visual fidelity require their respective acceptance checks. Model loading does not enable water, life support or electrical simulation.
