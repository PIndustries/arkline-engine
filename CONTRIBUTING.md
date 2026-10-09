# Contributing during private validation

Keep the repository private. Publication and license selection belong to the owner.
Do not import proprietary third-party assets, credentials, or old production data.

Changes should preserve a working two-pilot journey. Run formatting, Clippy, and
the workspace tests from README.md. Add regression coverage for changes affecting
ownership, balances, retries, persistence, or protocol compatibility.

Economic state changes and their ledger entry must commit together. The verifier
must remain independent of the server implementation. Wire/schema changes require
an explicit version decision and tests in both producer and consumer.

Use pull requests with the problem, resulting behavior, validation, and remaining
limitations. Do not claim deployment, real-world scale, or player acceptance from
a passing unit test or successful compile.
