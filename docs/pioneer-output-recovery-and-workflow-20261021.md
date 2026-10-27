# Output recovery and workflow progress — 2026-10-21

## Implemented source checkpoint

Native audio now retains one output worker through device errors and initial-open failures. The worker drops a failed/explicitly retried stream before creating another on the current system default; it never changes OS device/volume settings. Errors and eight seconds without callbacks initiate up to five retries at 1/2/4/8/16 seconds. Fifteen seconds of stable callbacks renews the budget, without claiming acoustic success. Exhausted retries wait for explicit Retry audio, which also resets the budget. Browser audio retains its gesture-driven Retry path.

Mix/preferences survive native retries. A fresh bounded cue channel discards past transient events; unconsumed diagnostic tests are marked interrupted rather than replayed or left waiting indefinitely. Receipt bookkeeping is serialized against recovery, outside the audio callback. Lifetime PCM peak wording now reflects retention across reopened streams.

Exchange List/Buy/Cancel now use live server-aligned telemetry time, independently of frozen visual time. They explain disconnected/pause/pending/stale/remote/not-docked states and actual stock, credits, cargo, quantity and ownership limits. Server request payloads and transaction rules are unchanged.

## Focused evidence

Four native recovery lifecycle/policy checks pass, including fake-stream drop-before-reopen, initial-open failure recovery, cue disposal, saved mix and accepted-test bookkeeping. Existing transport recovery check also passes. One Exchange check passes for live freshness versus frozen presentation and real per-action limits. No full suite repeated. No hardware device removal or OS audio change is used as a test; listener confirmation remains separate.

## Prioritized queue

1. Deliver recovery + Exchange with actual app output/UI inspection, preserving live Pioneer and FirstProject.
2. Session consequence preview blocked: approval review rejected the worker edit as read-only; it was not executed or rerouted. The actual policy cancels routines only when the pilot’s last session disconnects.
3. Navigation and Flight-deck order widgets now require fresh live telemetry, independently of frozen visual time. Contact selection, readouts and panel access stay available; manual input and global command handling are unchanged. Focused stale Approach click/no-dispatch check passed.
4. Overview contact filtering/readability review is next independently permitted UI scope.
5. Persistent standalone server/reconnect and live asset workflow remain architectural work: do not change disconnect/mining semantics implicitly.

Denied camera initialization, walking-key integration, developer-console integration and renderer edits remain untouched. No uploads/publication or cleanup.
