# Project files workspace

Select **Files** to browse saved files and keep editable text buffers in the native
editor. File editing, local simulation and Git publication are separate
steps: **Save** writes text to disk; a supported authoring tool's **Apply** changes
its runtime document; **Commit** records staged files; **Push** publishes commits.
Saving Rust source does not compile it or hot-reload the running editor.

## Browse and search

Use **Browse…** or enter a **Folder**, then choose **Open folder**. Selecting a
folder in the chooser does not open it automatically. The displayed **Current
root** identifies the file workspace; it does not replace the running authoring
project. Changing folders requires every dirty tab to be saved and current jobs
to finish. Clean tabs close when the folder changes.

In **Explorer**, expand folders to load their contents on demand. **Hidden files**
and **Git ignored files** are separate visibility choices. Showing them does not
grant access to restricted content. **Refresh folders** reloads the tree without
replacing open text buffers.
Ignore status uses installed Git when available; a plain folder or unavailable Git
metadata has no ignored-file classification.

**Search filenames** returns file paths whose full relative path matches without
case sensitivity, including matches in a parent folder name. It does not return
folder rows or search file contents. **Tree** returns to the folder view. A partial
result is not proof that no other matching files exist. Narrow the folder or query
when a limit is reached. Each folder list is limited to 1,000
entries from at most 4,000 scanned entries; recursive search visits at most 10,000
entries, returns at most 200 paths and descends at most 32 levels. Filesystem jobs
have a ten-second deadline and can be cancelled.

Paths stay within the selected real project folder. Symlinks are shown as metadata
and are not followed. Credential-like names, Git internals and private editor
runtime data are metadata-only, even when hidden and ignored entries are visible.
This includes `.git`, `.arkline-studio`, `.env*`, `.ssh`, `.aws`, credential files
and common private-key/token extensions. Name-based restrictions are a guardrail,
not a secret detector: do not put credentials in ordinary source files.

## Text editing and explicit Save

Ordinary UTF-8 files up to 1 MiB can be edited. Binary files, larger files,
restricted files and symlinks have metadata views instead of editable text.
Non-UTF-8 names, special files and paths that leave the root are unsupported.

Select a file to open a tab; at most 24 tabs are retained. A **•** marker and the
**UNSAVED** status identify edited text. Basic syntax colors cover Rust, JSON/JSONC
and TOML up to 256 KiB; larger editable files use plain text. These colors do not
provide compiler diagnostics, completion or language-server features. **Find in
file** (Cmd/Ctrl+F while editing) highlights case-sensitive exact matches in the
active buffer, up to 500 highlights. It is separate from Explorer's path search.

Choose **Save file**, or Cmd/Ctrl+S while the text editor has focus, to save only
that tab. Typing during a save is allowed: only the submitted text becomes the
saved baseline, and later edits remain unsaved. Closing a dirty tab offers
**Save file, keep tab open** or **Keep editing**; it does not discard the edit.
After saving, close the clean tab explicitly.

Saving checks that the file still matches the version originally opened. If an
external change or replacement is detected, Save is blocked: both the disk version and
the in-memory edit are retained. **Check disk** compares against the saved
baseline. **Copy editor text** preserves the edit on the clipboard, and **Compare
disk version** opens the checked disk contents in a read-only window. **Reload
disk version** is available only for clean tabs. Reconcile changes explicitly;
refreshing the folder is not permission to overwrite a changed file.

Successful Save prepares a temporary file in the same directory, preserves file
permissions, performs a final disk check and atomically replaces the original.
This is not a compare-and-swap against external writers: another app can change
the file between that check and replacement. Avoid editing the same file from
multiple apps concurrently. Read-only files and files with multiple hard links
are blocked. Save does not automatically stage, commit, push,
apply an authoring document or restart the local world. Cancellation can arrive
after a save has completed; inspect the result before retrying.

Git branch operations require saved source buffers and no pending file job.
Switching an existing branch is blocked when its checkout overlaps the active
authoring project, or when that project's canonical path cannot be verified;
creating a new branch at the current HEAD can still pass the guards. During a
branch operation, editing and file mutations pause. Afterwards, existing buffers
remain in memory and require **Check disk** before further editing or saving.
A pending Save blocks Git Stage, Unstage and Commit; Save waits for those mutations
in return. Branch operations never silently reload a tab.

Closing the editor is blocked by dirty source tabs, a pending Save or any running
Git job. Save source edits and wait for jobs to finish before closing. Cancelling
a Git job still requires waiting for its result.

## Preview and runtime limits

For a saved `.glb`, **Open model viewer** checks the file and reads a stable byte
snapshot up to 16 MiB, then queues those bytes in the existing model tools. It
switches to the asset workspace only when that queue accepts the request. If the
tools are busy, Files stays open with an explanation; finish the asset operation
and retry explicitly. A changed file is rejected before handoff.

The model tools decode the candidate and report supported geometry before
**Accept preview** and **Save assets**. These remain separate actions, documented
in [Project GLB preview](project-asset-import.md). Queuing a file is not proof of a
successful import, acceptance or save. The preview renders supported geometry in
a local asset view; it does not replace the live ship or spawn a world entity.

Texture/image previews, arbitrary material previews, Rust execution and source
hot reload are not supplied by the text editor. Use the relevant supported
authoring workspace to validate/apply its document, or build and restart source
changes through the normal development workflow.

## Validation boundary

This guide describes source behavior. Tests, a successful build, actual native UI
journeys and a published release are distinct evidence. Validate with disposable
folders: lazy listing and search limits, hidden/ignored visibility, blocked
symlinks and credential-like names, UTF-8/binary/oversized files, explicit saving,
external edits, dirty-buffer guards and retained buffers across Git operations.
Record the exact source commit, build identity and screenshots with the result.
