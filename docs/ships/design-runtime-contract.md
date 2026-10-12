# Detailed vessel design → derived runtime contract

**Status:** proposed local authoring/bake contract. The engineering panel under development contains candidate mass inputs; it is not a verified CAD model, an authoritative vessel mass model, or a completed runtime bake. This work does not change public servers or require account-service deployment.

## One design, several derived representations

The detailed design is the reviewable source: assemblies, part identities, dimensions, material assignments, equipment envelopes, compartments, mounts, ports and routes in a declared meter-based frame. Preserve useful construction detail there, including fasteners when needed for fit or manufacturing explanation. Do not make every bolt a rendered object or simulation entity.

A reproducible bake derives separate artifacts:

| Artifact | Purpose | Required relationship to source |
| --- | --- | --- |
| Render meshes and levels of detail | Visible exterior/interior, bounded draw calls and triangles | Source part IDs map to merged/simplified surfaces; declared geometric error and retained silhouette/interaction features |
| Collision and walkable volumes | Traversal, doors, clearances and contact | Conservative containment/clearance tests; no automatic inference that every visible surface is a valid collider |
| Mass regions | Dry mass, center of mass and inertia | Every contributing source part counted once; excluded/unknown mass identified; sums reproduce source quantities within explicit tolerances |
| System graph | Equipment, supply/control topology and state links | Stable typed ports and mount references survive reduction; topology is not reconstructed from visual proximity |
| Sockets and interaction anchors | Turrets, engines, boarding, tools and controls | Exact source-relative transforms and tested runtime transforms; no visual-only duplicate anchors |
| Provenance/validation report | Reproduction and review | Source revision/hash, units, assumptions, tool versions/settings, output hashes, validator results and unresolved gates |

The public project keeps the source and derivation recipe reviewable. Large-source storage policy can be decided separately; a screenshot alone is not reconstructible design evidence. Ship appearance, engineering and gameplay remain related representations rather than independent copies with guessed coordinates.

## Quantities and evidence

Each mass contribution needs a stable part ID, local transform, quantity, and one explicit mass basis: measured/imported mass, volume × assigned density, or a labeled gameplay assumption. Do not infer density, wall thickness, fuel, strength or pressure ratings from concept art. Unknown values stay unresolved; zero must mean a deliberate zero, not missing data.

Volume-derived mass requires a validated solid or a documented shell/thickness integration. A triangle bounding box is not material volume. Detect duplicate assembly instances, overlapping mass contributions, hollow regions and equipment already counted in a parent assembly. Consumable/cargo mass stays separate from dry structure and references the commodity conversion policy; current ore units still have no implemented kilogram conversion.

For source contributions i, reduce using consistent coordinates:

- `M = sum(m_i)` and `c = sum(m_i * c_i) / M`.
- Rotate each local inertia tensor into the common frame, then apply the parallel-axis term `m_i * ((d_i · d_i) Identity - d_i d_iᵀ)` about the aggregate center.
- Preserve the full symmetric tensor and validate finite, physically admissible results. A diagonal-only box estimate is useful for a candidate layout, but is not a complete inertia tensor for rotated/off-axis assemblies.

Partitioning parts into bow/center/stern or other runtime regions must conserve total mass, first moments and inertia. A region is a reduction of declared contributions, not a new mass source. A part that spans regions needs an explicit split policy; assigning its entire mass by center point is only an approximate visualization, not a validated spatial bake.

## Frames and fitted relationships

Declare axes, handedness, units and origin for every imported design. The current runtime ship body uses +X forward, +Y up and +Z starboard; imported meter coordinates are divided by 100 for rendering. Pioneer has a 72 × 38 × 18 m class envelope (length X, width Z, height Y). Envelope agreement alone does not prove a fitted interior or structural integrity.

Existing dossier `bounds_xzy_lwh_m` arrays use a different ordering from XYZ points. Apply a named, tested transform to each source frame; never guess it from field order. Keep the detailed source, simplified surface, collider, mass region, room boundary and equipment mount linked by IDs and explicit transforms. Tests must include known boarding/turret/door sockets and round trips, not just min/max bounds.

## Validation and reproducibility gates

1. Validate source references, units, finite geometry, material/mass provenance, assembly quantities and unresolved required values. Report defects by stable source ID.
2. Run a deterministic bake with versioned settings and bounded resource use. Equivalent source/settings produce the same logical manifest and normalized output hashes; record any unavoidable tool nondeterminism.
3. Compare source and runtime mass, first moments, full inertia, envelopes, collision clearances, sockets and system connectivity against declared tolerances. Tolerances are explicit reviewed parameters, not implied engineering certification.
4. Preview reduced geometry and highlighted mass regions locally. Inspect collision/walking and turret movement using the same runtime geometry/anchors; measure frame and bake costs with a stated fixture.
5. Save/export the source revision, recipe, derived artifacts and validation report together. A stale source hash invalidates previous validation. Only separately reviewed runtime code/migration may consume a baked mass model in gameplay.

First milestone: one Pioneer candidate assembly → mass report → reduced regions → repeatable validation export → reopen/reproduce, with no changes to economic balances or live flight tuning. Full CAD solids, automatic tessellation/LOD generation, structural analysis, thermal/fluid solvers and authoritative mass-based flight are separate gates.

## Present implementation boundary

The existing embedded GLB loader is a bounded static, opaque, untextured renderer input, not a CAD kernel or mass integrator. See [loader limits and authoring stages](../editor-authoring.md).

The current engineering-document work accepts local component boxes with entered mass, computes a box-based center/diagonal inertia estimate and groups mass by longitudinal center. Material names are labels, not a validated density catalog. It must remain labeled **candidate** until source provenance, full tensor handling and reduction validation exist; a successful save or attractive overlay does not establish those properties.

The existing [vessel engineering contract](engineering-contract.md) specifies proposed lifecycle/conservation/structure requirements. The first [connected room catalog](../pioneer-equipment-catalog.md) is a smaller implemented supply/control slice. Neither justifies claiming the entire detailed ship has a working physical simulation.

## Independent rendering and simulation budgets

Visual detail and physical detail are separate controls. Render LOD may merge thousands of fasteners into a normal-free simplified surface while the design record preserves their contribution to assembly mass. Conversely, an invisible thermal zone or supply link may need simulation state without a dedicated visible mesh. Neither triangle count nor source part count defines simulation accuracy.

Declare separate budgets for visible triangles/draw calls/materials/textures, collision complexity, active simulation nodes/connections, solver iterations/substeps, and background generation work. Reduction reports must state what each budget discards or aggregates. Test visual error and physical conservation independently; a lower mesh LOD must not change authoritative mass, ore, power or equipment connections. A high-detail inspection view must not silently enable an unbounded solver.

## Stable source identity and semantic review

The required trace is `stable asset/component ID → project-relative saved document and field → source revision/hash → Git history/blame → derived artifact IDs`. Git blame is evidence of an edit, not proof of engineering validity. Preserve IDs across renames, reorderings, undo/redo and branch edits; do not reuse an old component ID for a different component after undo. Current candidate-document IDs must be audited against this stronger history requirement before claiming it implemented.

A proposed design export includes the source documents, typed semantic change report, referenced evidence/assumptions and bake manifest. Diffs should identify component additions/removals, changed quantities with units, transform changes, port rewiring and changed derived totals; they must distinguish author-entered assumptions from computed results. Array reorder or harmless numeric formatting must not masquerade as a structural change. Validation and export remain local; GitHub readback comes first, with no editor PR-submission workflow currently claimed.

## Natural phenomena and material provenance — future work

A future world-generation or natural-phenomena job must be deterministic from an explicit seed, generator/ruleset version, input region and finite material inventory. Give each job an identity, bounded spatial/work budget, persisted progress and resumable deterministic ordering. Repeating a job or replaying a saved result must not create new resource reserves. The current finite asteroid ore/economic ledger must remain conserved.

A deposit must name its origin and source budget. Mining reduces that deposit; transfer, fragmentation, refining or accretion must trace input/output quantities. No depleted deposit respawns from nothing. If gameplay later introduces external influx, define that explicit source boundary and account for it rather than silently replenishing ore. Rendered dust or a kinematic particle is not automatically an extractable material unit. This is a future design requirement, not an implemented cosmology or geological simulator.
