# Ship design archive

This archive documents how Arkline's ships are being designed. It is a review space, not a catalogue of finished gameplay features. The fleet direction is industrial construction machinery in P.Industries black and emerald green, with articulated laser mining turrets rather than drills.

| Ship | Current hull bounds (length × width × height) | Design history |
|---|---|---|
| Pioneer | 72 × 38 × 18 m | [Images and plans](pioneer/README.md) |
| Prospector | 180 × 92 × 64 m | [Images and plans](prospector/README.md) |
| Hauler | 420 × 180 × 140 m | [Images and plans](hauler/README.md) |
| Vanguard | 1,100 × 380 × 260 m | [Images and plans](vanguard/README.md) |
| Borealis | 640 × 260 × 180 m | [Images and plans](borealis/README.md) |
| Stratum | 1,800 × 800 × 480 m | [Images and plans](stratum/README.md) |
| Monolith | 5,200 × 2,200 × 1,400 m | [Images and plans](monolith/README.md) |

## What to review

The archive includes every ship concept produced for this design discussion: 31 unique images across seven classes. Each ship page separates the latest interior review from superseded exterior and holographic studies. Earlier versions intentionally remain visible, including rejected drills, colors, brand marks and inaccurate status annotations. They are historical proposals, not specifications.

The latest review images introduce top/side views, interior zoning, human-scale rooms, power reserves and holographic displays. The [design reference](design-reference.md) records source-backed prototype values and proposed dimensions. The [gap register](gaps.md) distinguishes current station walking from unimplemented shipboard interiors. The [systems reference](systems-reference.md) expands power, data, RF, water, air and cooling connections as proposed designs.

## Numerical authority

Source definitions win for current gameplay. Per-ship JSON files capture those values and explicitly proposed coordinate reservations. Vector drawings visualize the JSON; raster illustrations are art direction only. A drawing labeled with meters is not proof that its painted proportions match those meters. No delivered plan has passed hull-mesh fit, navmesh, egress or in-game collision acceptance.

Current station avatars are 1.8 m tall and are blocked from entering the ship hull. Full interior navigation, doors/lifts, distributed power, fluid systems and model import remain implementation work. Large ships' human cores are detailed initial reservations, not an exhaustive design of every industrial compartment.

## Storage and contribution workflow

Repository images are compressed, full-resolution JPEG documentation previews; original PNGs remain in the downloadable review archive. This avoids adding production model packs or Git LFS to the code repository. [The image manifest](image-manifest.json) records every original and preview hash. No Tripo gallery asset was downloaded or incorporated.

Keep future revisions under the appropriate ship's `images/` directory, preserve previous versions and record what changed. Update the matching JSON, SVG and gap register when a design changes. Prefer pull requests with one coherent design change and link relevant issues. Publish the reviewed index to the official Wiki so artists and players can browse it without needing a checkout.

Tracking: [GLB and artwork pipeline #10](https://github.com/PIndustries/arkline-engine/issues/10), [walkable ship interiors #23](https://github.com/PIndustries/arkline-engine/issues/23), [client stability #7](https://github.com/PIndustries/arkline-engine/issues/7).

[Room-by-room fixture plans and connection schedules](room-services.md).
