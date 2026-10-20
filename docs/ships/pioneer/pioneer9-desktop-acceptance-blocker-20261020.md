# Pioneer .9 desktop acceptance: control blocker — 20 October 2026

**Outcome: blocked, acceptance not completed.** No gameplay input, placement, hatch command or simulation-state write was sent during this milestone. No requested desktop acceptance check is marked passed or failed functionally. Prior offscreen GPU passes remain separate evidence and have not been relabelled desktop acceptance.

## Exact actions and blocker

- `getAXState({emit:false,disableDiffing:true})` on the existing `.9` app binding timed out after the 60-second tool limit and reset the CUA kernel.
- After a read-only process check confirmed the same PID 31290 and explicit registration-3 QA arguments, `getApp` with the exact candidate `.app` path also timed out at 60 seconds and reset the kernel. No gameplay input was part of either call.
- `cua.getState()` succeeded and listed Arkline running. Binding by bundle ID `industries.p.arkline.editor.embedded` was explicitly refused as ambiguous among the preserved package paths. The tool requested an app name or full path; the full candidate path had already timed out. No older app was selected, removed or renamed to evade the ambiguity.
- This was a desktop adapter/accessibility control failure, not an automatic approval-review denial. Read outcomes timed out; gameplay actions remain not attempted. A one-second read-only process sample found event/render work, not evidence of a confirmed application deadlock.

## Evidence and current state

Evidence directory `/tmp/pioneer9-desktop-acceptance-20261020/` contains `baseline.jsonl`, `after-ui-timeouts.jsonl`, `process-sample.txt`, and `desktop-control-blocker.json`. Read-only observer source is `/tmp/pioneer9-desktop-observe-20261020.py`; it only reads the fresh QA SQLite world and writes evidence outside the project.

App process 31290 remains the `.9` candidate with `--project /tmp/arkline-registration3-candidate9-osqa-20261020 --pilot Registration3QA`. The process sample independently reports CFBundleVersion 2485.0.9. Registration remains 3 in the authority binding. During the read-only interval, simulation clock advanced from 1792512805368 to 1792513050418 and support changed from Deck2 to Station. We sent no gameplay input, so the cause is not attributed; this activity cannot be counted as a controlled acceptance journey. No forced restart was attempted against this changing QA state.

The prior actual OS-window screenshot at the launch milestone established `.9` title and Deck2 placement, with visible corrected-scale wall joints, floor grip and ceiling divisions. It still showed flat, repetitive white panels, simple emerald surrounds and unfinished lighting/equipment. No new screenshot was obtained in this blocked milestone, so no new visual acceptance is claimed.

Old registration-1 QA world JSON remains byte-identical to its saved pre-swap SHA-256. All older worlds/builds and source geometry remain unchanged. No build, package, MOVESPEED write, deletion/move, upload, publication, GitHub edit, whole-Documents permission or UI-permission bypass occurred. No observer/build worker remains running.

## Pending checks and exact next step

All requested checks remain pending: walk versus Shift sprint, visible third-person gait, measured hatch opening/closing, obstruction safety, complete cockpit/decks/airlock/lift/station/return/reseat, docked-space/deck consistency, and typed input/seated look. Recover a usable exact-app CUA binding, observe the current QA scene and clock, then start those controlled desktop trials with the retained read-only collector. Do not silently reset/rebind the QA or older worlds and do not replace this gap with offscreen-test claims.

A clearly labelled local evidence commit and complete recovery bundle `/tmp/pioneer9-desktop-blocked-recovery-20261020.bundle` preserve the status. There is no source implementation change.
