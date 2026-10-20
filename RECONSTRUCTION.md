# Cockpit integration reconstruction

This workspace starts at surviving committed baseline c4222513b4985918141e5447978f2c816c5a91e9. Fourteen cockpit-stage source files were imported from the preserved original-source recovery checkpoint3e500c9449dee3f7390c1cde2b2fdd0b53a2c07c, with hashes checked against its provenance.json.

The four large integration files and any necessary supporting glue will be newly reconstructed using the committed baseline and preserved original patches/fragments. They are not asserted to be byte-exact recovery of the missing build46/47 source.

Recovery evidence and its bundle remain untouched. This checkout has no remotes. Only meaningful local commits are authorized; no pushing, PR creation, deployment, MOVESPEED writes or storage migration.

Validation results will be recorded here as they occur. No successful build or runtime result is implied by importing the source.

Checkpoint: the four integration files are reconstructed and client/editor all-target cargo check passes with the repository-pinned installed Rust1.89.0. Formatting/test/lint/native results remain in progress. The working-copy Shift/GPU tests were adapted to the actual baseline UI/protocol/GLB provenance; no unavailable authored-part receipt or missing nested edge-test suite is fabricated. The source recovery evidence remains unchanged.

Input review checkpoint: fixed nested egui context access on a glass press, duplicate/replayed platform commands, and head-look bypass of editor viewport permission. Five focused physical-computer integration tests pass, including a child-process deadline/reap regression and exact clipboard command counts. The optional guard timeout was exercised against an owned sleeper and stopped it. Full workspace/native reruns remain pending.

Workspace test checkpoint: client suite passes307tests with26explicitlyignored; new pointer-lock, clipboard and head-look regressions pass. Five editor browser tests exposed temporary-folder Git ancestry inherited from this checkout. The guard now sets a Git discovery ceiling for its temporary directory; an actual disposable-repository guard regression passes (five guard tests total). Application browser filtering code is unchanged; editor/full-suite rerun pending.

Full workspace checkpoint: bounded offline workspace test run20261020T043100964775Z completed with exit0. Client307passed/26ignored; editor269passed/1ignored; all other workspace suites passed. Final formatting and workspace/all-target Clippy also passed. Browser Clippy could not start compilation because gloo-timers0.3.0 is absent from the local offline cache; browser acceptance is not established.

Native Mac checkpoint: actual private CGL execution isolated a legacy DEPTH_BITS query producing INVALID_ENUM1280 on core OpenGL. The three physical render passes now inspect the current draw framebuffer attachment without rebinding. Real color-only/depth/default target regression passes on AppleM4Max, GL4.1Metal91.7 (depth0/32/32), and the original display depth/perspective/state test passes. Source enclosure/aperture test also passes. The Shift fixture now renders a real parent TextEdit and checks both denied and positive head-look. Remaining native journeys and a rebuilt app are pending. The initial packaged26.10.19.1 app predates this GL fix and has not been launched.
