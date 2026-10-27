# Repository organization standard

## Hard checks and review judgment

`python3 tools/check_repository_organization.py` enforces the exact seven Cargo
workspace members, their canonical locations and their declared direct internal
dependency edges. `python3 tools/test_repository_organization.py` covers the
policy and real temporary offline Cargo workspaces. Both run in the existing
Linux test gate before formatting, locked Clippy and workspace tests.

The checker uses `cargo metadata --no-deps --offline --locked --format-version 1`.
Cargo resolves renamed dependencies and workspace inheritance. Normal, build,
dev and target-specific declarations count, including inactive target conditions.
No compilation, dependency download or source/lockfile write is needed. Crucially,
`--no-deps` metadata does **not** establish lockfile freshness, even with `--locked`:
the existing locked Clippy/test stages remain responsible for that validation.
Python uses only its standard library. Metadata failure is exit 2, policy failure
is exit 1, and success is exit 0; unavailable Cargo is not a passing check.

| Member location | Direct workspace dependencies |
| --- | --- |
| `crates/arkline-protocol` | None |
| `apps/asset-tool` | None |
| `apps/cli` | protocol |
| `apps/verifier` | protocol |
| `apps/server` | protocol, verifier |
| `apps/client` | protocol; server and verifier only under `cfg(not(target_arch = "wasm32"))` |
| `apps/editor` | protocol, client |

These are normal dependencies. The native client embeds local hosting and replay;
the server uses independent verification; the editor embeds client preview. These
are intentional existing relationships, not violations to remove. The verifier
must not depend on the server. A deliberate membership, location, dependency-kind
or target-boundary change must update the policy, fixtures, map and rationale in
the same reviewed change. The policy checks declarations, not transitive runtime
behavior or feature-level authority.

## Organize by responsibility

Use the [component map](component-map.md) to find the domain owner. Keep cohesive
implementation modules behind stable public facades. Entry points and coordination
seams should delegate domain work while keeping necessary ordered orchestration
readable. Review clarity and ownership, not arbitrary file-size ceilings or one
file per function. A small cohesive module can remain a single file.

State ownership, database transactions, serialized/wire contracts, numerical and
iteration order, resource lifetime, user-input focus, privacy and command guards
are behavior. A structural refactor must preserve them unless behavior changes are
explicitly scoped and tested. Separate organization moves from new functionality.
Retain asset provenance, source manifests, rejected design studies, dated receipts
and recovery archives; moving or deleting them is not cosmetic cleanup.

## Physical quantities and prototype contracts

Keep source revision/hash identity with geometry and measurement evidence. Capacity
requires the measured **usable cavity**, its exclusions (structure, machinery,
clearances, access and handling routes), and explicit dimensional units. Bounding
boxes or a visual fit alone do not establish usable volume or admission. Preserve
units on dimensions, volume, mass, energy and transfer rates, including the time
basis of rates. Unknown mass, load ratings or performance ratings remain unknown;
do not infer them from appearance or silently invent defaults.

Prototype/gameplay units must be explicitly labeled and versioned. A conversion
to physical quantities needs a documented source and migration/version decision;
it is not a rename. Rendering, a simulation fixture and measured/native acceptance
are distinct evidence. Never promote one into another without the missing proof.

## Contribution checklist

1. Name the domain, owner, base revision, paths/symbols and non-goals.
2. Preserve facade contracts and ordered coordination, or explain the intended change.
3. Update the component map when responsibilities move; review policy changes explicitly.
4. Run checker, fixture tests, formatting and applicable focused/integrated gates.
5. Record exact revisions and commands; distinguish passed, failed and not run,
   and automated, native, published, merged and released states.
