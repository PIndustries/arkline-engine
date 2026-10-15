# Development coordination

Use GitHub issues, the roadmap project and linked pull requests as the shared record
for who owns a workstream and what it includes. This helps contributors split work
without editing the same files or relying on private handoffs.

## Before starting or delegating

Read the latest issue and project card, recent issue comments, and any linked open
pull request. Check the current status, owner, target branch and head SHA, plus the
published file or symbol scopes. Search for an existing issue that covers the work;
use it if one fits, and open a new issue only when the existing roadmap has no
appropriate place for the task. If a claim or linked PR conflicts with the proposed
scope, contact its owner and agree on the boundary before editing or dispatching.

Post a claim on the issue before implementation or delegation. A claim should have
one accountable owner, even when other contributors help. Include the exact base SHA
so others know which source state the scope was planned against. State dependencies
and acceptance checks in observable terms. Name the test commands and expected
build/version or target matrix; update them if the plan changes.

Copy and fill this template in an issue comment:

```text
Workstream claim
- Issue:
- Owner:
- Status: Todo / In Progress
- Branch and worktree:
- Target branch and base SHA:
- Owned files or symbols:
- Dependencies:
- Non-goals:
- Acceptance checks:
- Versioned test plan:
- Coordination notes / related claims:
```

After posting, refresh the issue and project to catch a competing claim. If two claims
overlap, resolve ownership before either workstream edits those paths; an issue comment
is a coordination record, not an atomic lock.

Use `In Progress` only while someone is actively executing the claimed implementation
or validation work. Focus Now / Next / Later expresses priority; it does not claim
ownership or establish active execution.
Use `Todo` for work that is planned, waiting on a dependency, or blocked. A stale
claim is not automatically released: contact its owner and ask them to update it or
explicitly release it. Do not silently remove the claim or reassign its scope.

## Parallel branches and delegation

Give independent workstreams separate branches and worktrees. Before editing, check
whether another claim owns the same file or symbol. When scopes overlap, agree on
the split and update the claims first; then edit only the agreed scope. A shared
checkout is not an ownership boundary.

Do not force-push a branch that others use or hard-reset a shared checkout. If the
target branch advances, coordinate with the owner before rebasing or merging the new
base, and preserve commits already made by collaborators. Record a changed base SHA
in the claim so reviewers can follow the branch history.

A delegated contributor works under the existing claim. Include its
link, owner, branch, base SHA, exact owned paths or symbols, dependencies, tests and
non-goals in the dispatch. Do not send it into an unclaimed neighboring scope. If
the task needs more files or behavior, agree on the expansion and update the claim
before making those edits. The workstream owner remains responsible for review and
integration.

## Progress and handoff

Add progress to the issue or pull request as it happens. Record these outcomes
separately so no one has to infer one from another:

- **Implementation:** files or behavior changed, and what remains.
- **Tests:** commands, build/version, target, counts and any skipped checks.
- **Push:** branch and full commit SHA.
- **CI:** workflow link, exact SHA and current result.
- **Merge:** merged or still open, with the merge commit when available.
- **Release:** version/build, publication or deployment evidence, and any player
  acceptance that was actually observed.

For a finished or paused workstream, append a handoff comment using this template:

```text
Workstream update
- Issue and claim:
- Owner and status: In Progress / Todo / released by owner
- Implementation:
- Tests and exact versions:
- Branch and pushed SHA:
- CI link, exact SHA and status:
- Merge status:
- Release/deployment status:
- Remaining acceptance checks and blockers:
- Scope explicitly released (if any):
```

Attach a log file or link when output is long. When using the GitHub CLI for a
multiline progress note, use `--body-file` so line breaks are preserved. Remove
credentials, access tokens, private world data and unrelated personal paths before
sharing logs. Keep limits visible: source changes, passing tests, pushed commits,
CI, merge, release and player acceptance are separate evidence. Do not mark a later
stage complete because an earlier one passed.

## Pull requests

Use the [pull request template](../.github/pull_request_template.md). Link the issue
and public claim, state the base SHA and dependencies, and list the files or symbols
changed. Name a reviewer and provide exact-SHA validation links. Keep remaining gaps
in the PR body until their acceptance evidence exists; link follow-up issues when
work is intentionally deferred.
