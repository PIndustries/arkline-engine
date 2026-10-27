# Repository working agreement

Read [CONTRIBUTING.md](CONTRIBUTING.md), the [component map](docs/component-map.md)
and [organization standard](docs/repository-organization.md) before changing code.
Respect the current workstream owner and agreed file/symbol scope; use isolated
worktrees. Publication, merging and deployment require their own authorization.

Keep cohesive domains behind stable facades and thin entry/coordination seams.
Do not split by arbitrary line counts or create one file per function. Preserve
transaction, frame, simulation, teardown and input-ownership ordering during moves.
Keep authoritative decisions on the server and independent replay in the verifier.
Do not silently reorganize source assets, rejected studies or acceptance archives.

Run the organization checker and its tests for every structural change. Update the
map and review dependency-policy changes explicitly; do not weaken checks to make a
move pass. Separate source-equivalence, automated, native and deployment evidence.
Record not-run checks and remaining gaps honestly.

For physical quantities, retain source identity, measured usable cavity and
exclusions, explicit units and rates. Unknown mass or ratings remain unknown.
Version and label prototype units; do not relabel them as measured SI quantities.
