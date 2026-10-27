# Management destination search — 2026-10-21

The search indexes the existing fourteen management destinations across Flight, Ship, Operations and Station. It is local navigation over the canonical ServiceTab registry, not a second system inventory or a search of authoritative world objects. Existing grouped navigation returns for an empty/whitespace query.

Queries match case-insensitive whitespace tokens against actual destination names, groups and relevant content keywords. Results retain their group context. Typing, clearing and unmatched queries preserve the selected panel and its content; only explicit result selection changes it. No implicit Enter action or ship command is added. The results list is bounded and scrollable, with a clear empty-results explanation and Clear search button.

The search field supports normal pointer/Tab text focus. Cmd/Ctrl+K is scoped to an open management workspace and respects parent/text/computer input ownership; Cmd/Ctrl+F remains editor file search. Existing workspace input release, freshness, permissions and server command checks remain unchanged. Escape retains its text-first policy. Search navigation is also available in the existing separate ship-layout reference window.

This slice does not add a universal ship interaction binding, integrate the denied console draft, change camera initialization or edit renderer lighting. No protocol, collision, registration, authored assets or world migration changes.

Validation uses focused search matching, pointer/keyboard focus, stable-selection and no-command checks, then the guarded editor build and known PioneerFinish11QA save/reopen workflow. Preserve current avatar/job/ledger/hatches and FirstProject; do not restore an older snapshot. Live verification and any desktop input limitations belong in the package acceptance receipt.
