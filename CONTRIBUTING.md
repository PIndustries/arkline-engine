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
the workspace tests in the [player and developer guide](https://github.com/PIndustries/arkline-engine/wiki/Player-Guide). Add regression coverage for changes affecting
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

The [official Wiki](https://github.com/PIndustries/arkline-engine/wiki) is the published documentation. Edit the versioned Markdown linked on each page and submit documentation changes through pull requests.

Start with the [documentation source index](docs/README.md), [component map](docs/component-map.md),
and [API reference](docs/api.md). The official account service is operated separately;
local worlds currently use the documented pilot credentials. Configurable OAuth/OIDC
integration is planned, not part of the current login implementation.

## Coordinate work in GitHub

Use the [community roadmap](https://github.com/orgs/PIndustries/projects/14),
[issues](https://github.com/PIndustries/arkline-engine/issues) and
[milestones](https://github.com/PIndustries/arkline-engine/milestones) to coordinate.
Before starting work or delegating it, read the latest issue, project card, linked
pull request and recent comments. Check the current owner, status, branch, base SHA
and active scope. Search for an existing issue and use it when it covers the work;
open a new issue only when no existing issue fits. Do not begin a conflicting
workstream until its owner has agreed on the boundary.

Publish a workstream claim on the issue before implementation or dispatch. Include
one accountable owner, the branch and worktree, exact base commit SHA, files or
symbols owned, dependencies, non-goals, acceptance checks, and the versioned test
plan. Use the templates in the [development coordination guide](docs/development-coordination.md).
Keep the claim current as its branch, scope or dependencies change.

Refresh the issue after posting to detect competing claims before editing. Use
`In Progress` only for active implementation or validation. Focus Now / Next / Later
is priority, not ownership or execution. Use `Todo` for
planned or waiting work; if implementation is blocked, keep the blocker visible on
the issue. A stale claim is a request to contact its owner: ask them to update it
or explicitly release the scope. Do not silently delete, reassign or treat a claim
as expired.

Give parallel work separate branches and worktrees. Check file and symbol ownership
before editing. If scopes overlap, agree on the split and update the public claim
before either side changes the overlapping code. Never force-push a branch others
use or hard-reset a shared checkout. When a base branch advances, coordinate how
to incorporate it before rebasing or merging, and preserve commits made by others.

When delegating to a contributor, include the claim link, owner, branch,
base SHA and exact paths or symbols they may change. Keep the delegation inside that
scope; publish a separate claim before expanding it. The coordinator remains
responsible for integration and reporting the resulting diff and checks.

Append progress to the issue or pull request as separate facts: implementation,
tests, push, exact-SHA CI, merge and release. Include build versions and links to
relevant logs; attach long output as a file when useful, and remove credentials,
tokens and private world data. A passing test, pushed branch, passing CI, merged PR
and released build are different results. Record remaining limits and blockers so
the next contributor can continue without guessing.

Write commits in plain, specific language describing the resulting change. Preserve
accurate Git authorship and attribution; do not add promotional or generated filler.

## Keep organization a maintained standard

Follow the [repository organization standard](docs/repository-organization.md)
and [agent working agreement](AGENTS.md). Keep cohesive domain modules, stable
facades, thin entry seams and explicit state/transaction ownership; no arbitrary
line-count limit or one-file-per-function rule applies. Update the component map
when responsibilities move. Structural changes must run:

```sh
python3 tools/check_repository_organization.py
python3 tools/test_repository_organization.py
```

These checks supplement, not replace, formatting, locked Clippy/tests and relevant
native validation. Explain deliberate workspace/dependency-policy changes in the
same review. Physical data must preserve source identity, usable-cavity exclusions,
units/rates and unknown ratings; label and version prototype units explicitly.
