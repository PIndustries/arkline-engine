# Models and community artwork

## What runs today

Pioneer uses an original embedded GLB hull with an editable Blender source and a reproducible generator. The native and browser clients use its actual triangles for rendering, shadows, camera feeds and selection. Moving mining turrets remain separate engine components. Other ships, stations and cockpit geometry retain their existing Rust-generated geometry.

The first Pioneer asset has seven material batches and 32,284 triangles. Its body measures 72 meters long, 38 meters wide and 18 meters high. This is a detailed exterior foundation, not final concept parity or a completed fleet. The walkable interior remains a separate scene; exterior openings, bridge/cockpit registration and physical systems need further work.

- [Embedded model runtime](pioneer-model-runtime.md): supported GLB subset, loading limits, caching and rendering behavior.
- [Pioneer editable model](../assets/ships/pioneer/README.md): source files, generation instructions, dimensions, hardpoints and known gaps.
- [Ship art contribution checklist](art-contribution-checklist.md): rights, previews, intended scale and offline inspection.

This is not a general model browser or arbitrary import command. A user cannot drop any GLB into the running client. The current embedded loader supports static indexed triangle geometry and opaque, untextured metallic/roughness materials with emissive color. It rejects unsupported textures, skins, animations and extensions instead of silently substituting them. It uses no runtime network or external resource loading.

## Contributing a model

Use glTF 2.0 / GLB as the interchange format and include the editable source project. Discuss a substantial asset in an issue before building a large pack. Supply preview images, author and license information, dimensions in meters, triangle/material counts and any texture sizes. Keep turrets, drives, doors and other moving parts distinct. Identify attachment points and collision shapes.

Use original work or material you have the right to contribute, with applicable third-party licenses and attribution. Do not submit extracted assets from other games.

For the current embedded path, follow the runtime contract rather than assuming every glTF feature is supported. Pioneer is limited to sixteen material batches by a regression check; the broader decoder limits are safety ceilings, not recommended art budgets. Texture, LOD, animation, collision and wider fleet conventions still require implementation and measured desktop/browser performance validation.

The [ship design archive](ships/README.md) preserves the visual direction and proposed plans. A concept sheet is not evidence that its rooms, utilities or structural properties already exist in the game. Contributions should state which parts they implement and which gaps remain.
