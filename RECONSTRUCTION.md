# Cockpit integration reconstruction

This workspace starts at surviving committed baseline c4222513b4985918141e5447978f2c816c5a91e9. Fourteen cockpit-stage source files were imported from the preserved original-source recovery checkpoint3e500c9449dee3f7390c1cde2b2fdd0b53a2c07c, with hashes checked against its provenance.json.

The four large integration files and any necessary supporting glue will be newly reconstructed using the committed baseline and preserved original patches/fragments. They are not asserted to be byte-exact recovery of the missing build46/47 source.

Recovery evidence and its bundle remain untouched. This checkout has no remotes. Only meaningful local commits are authorized; no pushing, PR creation, deployment, MOVESPEED writes or storage migration.

Validation results will be recorded here as they occur. No successful build or runtime result is implied by importing the source.

Checkpoint: the four integration files are reconstructed and client/editor all-target cargo check passes with the repository-pinned installed Rust1.89.0. Formatting/test/lint/native results remain in progress. The working-copy Shift/GPU tests were adapted to the actual baseline UI/protocol/GLB provenance; no unavailable authored-part receipt or missing nested edge-test suite is fabricated. The source recovery evidence remains unchanged.
