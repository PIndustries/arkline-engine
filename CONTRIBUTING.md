# Contributing to Arkline

Arkline is an early-access community project. Report reproducible bugs and propose
changes through GitHub issues and pull requests. For a substantial feature or asset
pipeline change, open an issue first so contributors can agree on interfaces and scope.
Use [SECURITY.md](SECURITY.md) for private vulnerability reports.
Contributions to first-party Arkline code are accepted under AGPL-3.0-only, the
same license as the project. Contributors retain their copyright. By submitting
a contribution for inclusion, you agree to license it on those terms and confirm
that you have the rights to do so. Identify third-party material and preserve its
license and attribution; do not assume the project license replaces those terms.
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

Public fork pull requests do not run automatically on our persistent self-hosted
build machines. Include your local validation results; maintainers review changes
before running trusted CI. Do not add secrets or production world data to a PR.

Start with the [documentation index](docs/README.md), [component map](docs/component-map.md),
and [API reference](docs/api.md). The official account service is operated separately;
local worlds currently use the documented pilot credentials. Configurable OAuth/OIDC
integration is planned, not part of the current login implementation.
