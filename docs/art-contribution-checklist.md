# Contributing ship models

Begin with one hull and open a design issue before creating a large asset pack. The fleet [concept archive](ships/README.md) and [subsystem behavior guide](ships/subsystem-behavior.md) explain the intended appearance and functions.

1. **Choose a specific deliverable.** For example, a Pioneer hull revision, one articulated mining turret, a door/frame, a ceiling sensor or a drone berth. Avoid a monolithic model with every moving component fused together.
2. **Provide editable source and a self-contained GLB.** Use meters. Keep a known one-meter reference in the source project. Include top, side and perspective previews. State the source tool version and export settings when they affect reproducibility.
3. **State authorship and rights.** Include author, applicable license, required notices and sources of any third-party materials. Code licensing does not automatically license imported artwork. Do not contribute extracted game assets or assume a download permits redistribution.
4. **Run the offline inspector.** `cargo run --locked -p arkline-asset-tool -- inspect YOUR_MODEL.glb > inspection.json`. Include the report with the submission. A successful report is a geometry/packaging check, not gameplay approval.
5. **Describe scale and axes.** Include intended hull class, total length/width/height, forward/up axes, origin and transformations. The current renderer uses its own conversion; the inspector does not silently rescale or rotate models into a game hull.
6. **Identify moving and functional pieces.** Propose separate named nodes for turret base, barrel, muzzle, drives, door leaf/frame, drone bay, cockpit and service interfaces. Include attachment coordinates in meters. Names are a submission convention until the runtime pipeline consumes them.
7. **Supply collision intent.** Mark solid walls, usable openings and proposed collision proxies. Door clearance must be checked with the 1.8 m avatar reference; render geometry alone does not define a safe walking path.
8. **Declare the resource cost.** Include triangles, material count, image sizes and dimensions, animation clips, skins, and intended level-of-detail variants. The current inspection tool does not decode texture dimensions; provide them from the source/export tool. Budgets need measured native/browser tests before acceptance.
9. **Review a small example in-engine.** Runtime GLB import is still being implemented. Until it exists, approval of a concept or inspection report does not mean the asset is shipping. Keep design, model import, collision, animation and gameplay acceptance as distinct checklist items in the issue.

## Useful first contributions

- Pioneer exterior detailing that preserves its public dimensions and two mining-turret mounts.
- Original industrial door, control panel, network jack and sensor designs that fit the room fixture schedule.
- A repeatable native/browser material-and-lighting test scene.
- Inspection tests for malformed GLB submissions and unusual transforms.
- Clear model-source/export instructions that another contributor can reproduce.

Use the issue to record decisions, the pull request to review the change, and the Wiki to describe accepted behavior. Keep old concept versions available for comparison while marking which one is current.
