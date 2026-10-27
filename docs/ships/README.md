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

The latest review images introduce top/side views, interior zoning, human-scale rooms, power reserves and holographic displays. The [design reference](design-reference.md) records source-backed prototype values and proposed dimensions. The [gap register](gaps.md) records station walking and the original shipboard-interior proposals. The [systems reference](systems-reference.md) expands power, data, RF, water, air and cooling connections as proposed designs.

## Numerical authority

Source definitions win for current gameplay. Per-ship JSON files capture those values and explicitly proposed coordinate reservations. Vector drawings visualize the JSON; raster illustrations are art direction only. A drawing labeled with meters is not proof that its painted proportions match those meters. No delivered plan has passed hull-mesh fit, navmesh, egress or in-game collision acceptance.

Pioneer has two walkable docked decks in merged source. The development branch adds connected airlock equipment and native editor tools. Other ships' interior layouts and distributed utilities remain design proposals. See [Pioneer](pioneer/README.md) for implementation status and remaining hull-registration work.

## Storage and contribution workflow

The original concept-image collection uses compressed, full-resolution JPEG documentation previews; original PNGs remain in the downloadable review archive. Later model and engine evidence may use other image formats. [The image manifest](image-manifest.json) records every original and preview hash.

Keep future revisions under the appropriate ship's `images/` directory, preserve previous versions and record what changed. Update the matching JSON, SVG and gap register when a design changes. Prefer pull requests with one coherent design change and link relevant issues. Publish the reviewed index to the official Wiki so artists and players can browse it without needing a checkout.

Tracking: [GLB and artwork pipeline #10](https://github.com/PIndustries/arkline-engine/issues/10), [walkable ship interiors #23](https://github.com/PIndustries/arkline-engine/issues/23), [client stability #7](https://github.com/PIndustries/arkline-engine/issues/7).

[Room-by-room fixture plans and connection schedules](room-services.md).

[How the ship subsystems work together](subsystem-behavior.md).

See the [vessel engineering contract](engineering-contract.md) for proposed lifecycle states, energy/mass/heat accounting and structural load requirements.
