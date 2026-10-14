# Material files and recovery

The material editor reads and writes `Assets/Pioneer.material.json` in the current
project. Startup, Save and Reload run on one background worker. While an operation
is pending, the toolbar shows its progress and disables additional file operations;
graph editing and Undo/Redo remain available.

Save writes the submitted material snapshot. Edits made afterward stay in the draft
and remain unsaved when the write finishes. Reload replaces the draft only if no
edit or history movement occurred while it was loading. Otherwise it records the
loaded file as the saved baseline and keeps the newer draft. A completed Reload is
undoable. A node drag in progress also prevents a late load from replacing it.

The built-in finish is used when no material file exists. That state is shown as
**Not saved**. Invalid JSON, unsupported documents, access failures and symbolic
links are load errors, rather than reasons to silently create a replacement file.

If a file cannot be loaded, Save stays locked. Either repair the file and choose
**Reload**, or choose **Allow replacing unreadable material** and then **Save**.
Allowing replacement does not itself write a file or reset the draft/history.
Save still uses the existing validation and atomic writer; it cannot follow a
symbolic-link destination. Keep a copy of an unreadable file if it may need recovery.

Each panel and worker is bound to the project it opened. File operations cannot be
redirected by passing a different project path. Reopening another project creates
a separate panel and result channel. An already submitted write may finish in its
original project after the window closes; it is not replayed in another project.
If the worker stops before reporting a save result, the editor reports that its
outcome is unknown and locks further saves until the project is reopened and read.
There is no automatic retry.

Materials remain local authoring data. Saving does not apply room geometry, change
authority or simulation time, or publish a document to collaborators. This is not
a file watcher or general asset/code reload system. Engineering and other startup
I/O have separate work remaining; material worker tests alone do not establish
native permission recovery or improved frame-time distributions.

Regression checks:

```sh
python3 tools/build.py test -p arkline-editor --locked
python3 tools/build.py clippy -p arkline-editor --all-targets --locked -- -D warnings
```
