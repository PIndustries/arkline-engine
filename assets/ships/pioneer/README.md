# Pioneer authored hull

`pioneer.blend` is the editable source model. `pioneer.glb` is the material-batched runtime export. `tools/art/build_pioneer.py` recreates both with Blender 5.0. The model and generator are original project contributions under the repository license; no downloaded artwork or textures are included.

Run from the repository root:

```sh
blender --background --python tools/art/build_pioneer.py
```

The model uses meters with +X toward the bow, +Y upward and +Z starboard. The hull envelope is 72 × 18 × 38 meters (length × height × width), centered on the origin. The editable source contains named armor panels, structural ribs, radiator blades, pipes, cargo hatches, drive bells, running lights, landing gear and turret sockets. Export joins components by material to limit draw calls.

The engine supplies the moving mining turrets. Their mount centers are `(10.8, 9.864, ±2.66)` meters; the hull mounting surface is at Y=9. Aft drive centers are `(-35.9, 0, ±12.5)` meters. Decorative green paint and lamps are not thrust emitters.

## Relationship to the ship plans

This is the first authored external hull, not a finished one-to-one reconstruction of the concept sheets. It preserves the documented hull envelope and reserves the declared two-deck inhabited core. The construction palette, armored side machinery and paired roof mining mounts follow the visual direction in the Pioneer concept collection.

The following remain explicit work, not implemented features of this asset:

- The rendered interior is a separate gameplay scene. It is not yet cut into this exterior mesh with functioning exterior apertures.
- Concept images place bridge glazing differently from the current room plan. The decorative forward glazing does not establish the physical bridge location or cockpit camera position.
- The 18-meter height describes the hull. Articulated turrets extend above it; the concept label describing 18 meters overall needs reconciliation.
- Pipes, radiators, lights, gear and hatches are visual geometry. They do not establish conserved thermal, fluid, electrical, pressure or structural simulation.
- There is no new collision mesh, LOD chain, skeletal rig, texture set or physically rated structural model in this change.
- Other ship classes still use their existing models. They must be authored and reviewed individually.

`pioneer-hull.png` is a Blender render of the source hull, without the engine's articulated mining turrets. In-game screenshots are the evidence for runtime appearance; the Blender lighting is not the game's lighting.

## Industrial detail pass

The hull now separates the inhabited core from the stepped outboard pods. Fore and aft collars, recessed side service equipment, pressure couplings, harnesses, radiator banks, handrails, access ladders and permanent identification markings add scale without changing the published envelope or moving gameplay hardpoints. Text is exported as geometry, not a texture. Export triangulates evaluated surfaces and removes zero-area faces before material batching.

These exterior utility details remain visual assemblies. The connected room implementation is separately defined and validated; an external pipe or radiator must not be described as operational merely because its model is visible. The runtime still uses scalar materials, with no normal, roughness or occlusion textures.
