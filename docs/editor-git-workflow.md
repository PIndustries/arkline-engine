# Native Git workspace

The editor's **Git** workspace uses installed Git to contribute saved files from a
local checkout. Git operates on disk files; it does not save, apply, reload or
discard the authoring project's in-memory buffers.

**Save** writes an authoring document to disk. **Apply** sends a supported document
to the local runtime. **Stage** selects saved file versions for Git, **Commit**
records them locally, and **Push** publishes committed source to a remote. Each is
a separate action. Review retained buffers before saving after a branch change.

## Open or clone

1. Select **Git**, then **Browse checkout…** or enter a checkout directory. Choose
   **Open / Refresh** to inspect it. A directory within a checkout resolves to its
   repository root; the authoring project stays unchanged.
2. Check the displayed repository, branch, HEAD and changed paths. `I` is the index
   (staged version), `W` is the working file, and `?` means untracked.
3. Enable **Trust this checkout and its configured credential / signing helpers**
   before mutation or remote operations. Only trust repositories and transport
   configuration you recognize. This checkbox does not configure credentials.

To clone, expand **Clone a repository**, enter a clean HTTPS or SSH source (or an
absolute local repository path), and choose a **new folder that does not exist**.
Approve use of your configured transport helpers, then choose **Clone into new
folder**. Existing folders are never replaced. A cancelled or failed clone can
leave a partial folder; inspect it yourself before choosing another destination.
Filtered/LFS repositories may be cloned without checking out files when safe
checkout is unavailable.

Git must be available on `PATH`; safe branch switching requires Git 2.43 or newer.
This includes Linux: Ubuntu 22.04's default Git 2.34 is too old. The editor uses
`git check-attr --source` to inspect the target branch before checkout, so it can
refuse clean/smudge filters and LFS without running them. An older Git blocks the
branch operation and preserves the checkout; install a supported Git or use your
normal Git client. The Linux CI image builds checksum-pinned upstream Git 2.56.0
on Ubuntu 22.04, retaining the public binary's glibc 2.35 baseline. CI logs the
container's Git version and checks this capability before running Rust validation.
Existing credential helpers and SSH configuration
provide transport authentication; the Community panel's GitHub API login is a
separate identity. The workspace does not request broader grants, install helpers,
change Git identity or accept credentials embedded in a source URL. Interactive
Git credential prompting is disabled; resolve authentication in your normal Git
client if an operation fails.

## Branch, review and commit

Use **Create branch** for a new local contribution branch, or select an existing
local branch and choose **Switch branch**. Both require a clean checkout, including
no untracked files, saved **Files** tabs and no pending file job. Resolve saved
work explicitly first; there is no automatic stash, reset or discard. Branch
operations also block filtered/LFS targets and
index flags that could hide changes (`skip-worktree` or `assume-unchanged`). Remote
branch checkout is not offered here.

**Switch branch** is unavailable when the checkout contains, is contained by, or
equals the active authoring project. It is also blocked if the authoring project's
canonical path cannot be established. Close the editor before switching that
checkout in your Git client. **Create branch** from the current HEAD remains
available once the other guards pass; it does not check out different files.

Select changed files, choose **Working diff** or **Staged diff**, then **Inspect
selected diff**. The working diff compares saved files with the index; the staged
diff compares the index with HEAD. Untracked files have no Git diff yet: review
their saved contents before staging. Binary changes are identified without a text
preview. LFS content is not downloaded or expanded.
The text preview is limited to 256 KiB; a truncation notice directs larger reviews
to your Git client.

- **Stage selected files** stages whole saved files, including tracked deletions
  and rename paths. Unselected index entries are retained. A selected file that
  already has staged changes is blocked, preserving any partial staging.
- **Unstage selected files** retains working files. It requires fully staged
  selections without additional working changes; partially staged files are
  blocked.
- Enter a message and choose **Commit selected staged files**. Every selected file
  must be fully staged with no further working changes. Any staged file outside
  the selection blocks the commit. Select all intended staged files or reconcile
  staging in your Git client, then refresh. Unrelated unstaged/untracked files are
  not added to the commit.

A pending **Files** Save blocks Stage, Unstage and Commit. Conversely, Save waits
while one of those Git mutations or a branch operation is running. Unsaved buffer
contents are never included merely because their path is selected in Git.

Stage and Unstage hold Git's normal `index.lock` and work on a private copy of the
reviewed index. They recheck the reviewed state before atomically publishing that
copy, preserving the existing index permissions. An existing lock blocks the
operation; the workspace does not remove another tool's lock.

Commit builds the exact reviewed staged tree with the reviewed HEAD as its parent
(or no parent for the first commit). It advances the reviewed branch only if that
branch still points to the expected old commit, using Git's conditional ref
update. The real index is retained byte-for-byte rather than replaced by Commit.

Commit uses the configured Git author identity. Missing identity blocks the
operation without inventing a name or email. Configured commit signing is retained;
set up its helper in your normal Git client if it cannot complete. **Git hooks are
disabled for editor operations**; use your Git client when a hook must run. A
successful commit does not push anything.

## Push and draft PR handoff

Select the **Push remote** and inspect its displayed push URL and destination
branch. **Review push…** shows the repository, branch, exact commit and remote;
**Push branch** confirms that publication. The push sends the reviewed HEAD to the
same named remote branch, without force, tags or recursive submodule pushes.
Unsaved buffers and uncommitted files are excluded.

Before pushing, the workspace checks the remote branch. If it has advanced,
diverged, or references a commit unavailable locally, reconcile it explicitly in
your Git client and refresh. Nothing is fetched, merged or rebased automatically.
After a successful push, the workspace reads the remote branch again and only
reports verification when its commit equals the reviewed local HEAD.

Choose **Verify PR handoff** after pushing. This requires a clean `github.com`
HTTPS or `git@github.com:owner/repository` push URL, a contribution branch different
from the remote default branch, and a remote commit matching local HEAD. The
resulting **Open GitHub PR form** link compares the remote default branch with the
contribution branch in that repository. Fork-to-upstream selection and alternate
base branches must be reviewed on GitHub.

Opening the link **does not create or submit a pull request**. On GitHub, review
the repository, base/head branches, diff, title and description; choose **Create
draft pull request** and submit it there. A compare URL opens a form, as described
in [GitHub's compare-URL documentation](https://docs.github.com/en/pull-requests/reference/using-query-parameters-to-create-a-pull-request).

## Safety limits and recovery

One background Git job runs at a time; repeated action clicks cannot start a
second job. **Cancel Git job** requests cancellation. Cancellation or failure can
leave unused Git objects, or arrive after an index or branch update has completed.
Refresh and inspect the actual index, branch and HEAD before retrying; an error is
not proof that the operation had no effect. Local subprocesses
have a 20-second timeout; clone and network subprocesses have a 90-second timeout,
within a 120-second whole-job deadline. Output is bounded to 2 MiB per stream.
Changed-file inspection is limited to
32 MiB per file and 128 MiB in total.

Closing the editor is blocked while any Git job runs, or while source tabs are
dirty or a source Save is pending. Wait for the job, or cancel it and wait for its
result, then save source edits and close explicitly.

Status is a reviewed snapshot. Changes to HEAD, branch, index, Git configuration or
changed-file contents invalidate it before a later operation. On a stale-state
error, use **Open / Refresh**, inspect the new diff and review the selection again.
In-progress merge/rebase/cherry-pick and similar Git operations block mutations
and remote handoffs; finish or resolve them in your Git client. The locks and
review checks do not serialize arbitrary external tools. Avoid concurrent
mutation of the same checkout from another app or Git client.

Inspection disables repository hooks, filesystem-monitor hooks, external diff,
text conversion and configured clean/smudge/process filters. Staging and commits
reject selected filtered/LFS files; conflict resolution, partial staging, submodule
changes, non-UTF-8 paths and oversized or special files require an external Git
client. Refresh after completing that work. The workspace does not offer pull,
merge, rebase, force push, reset, stash or bulk staging.

## Validation boundary

Source tests and a successful build do not establish native UI acceptance or a
published release. Validation for this workspace should use disposable checkouts
and a local bare remote, covering selected-file commits, retained staging, stale
state, missing identity/Git/authentication, nonempty clone destinations, remote
divergence, cancellation/repeated actions, spaces/Unicode, renames/deletions,
binary/filter/conflict limits, and retained authoring buffers. Record the exact
source commit, build identity and actual UI evidence with the contribution.
