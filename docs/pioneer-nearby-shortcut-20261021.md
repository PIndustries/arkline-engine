# Nearby Pioneer interaction shortcut — 2026-10-21

Unmodified F was unused in gameplay. E/Q remain movement, Cmd/Ctrl+F file search, Cmd/Ctrl+K management search. The visible F prompt is a nearby-control selector, not crosshair picking or a universal action system.

Supported candidates are the seated Leave pilot seat action, nearby pilot-seat return, same-side inner/outer boarding hatch controls, and an unambiguous opposite lift landing. Existing button requests and shared readiness checks remain authoritative. Walking candidates require the existing physical range and a clear CPU segment above floor level against registered static and current dynamic solid geometry. Invalid geometry fails closed; close ambiguous candidates suppress the shortcut rather than selecting another available action. No camera/rendering changes are made.

The prompt is reserved ahead of optional route details; only a completely clipped-in, unobscured painted prompt can arm. A previously displayed exact target, side, vessel binding and intent is pinned on a nonrepeating F press. Release dispatches once only if the same target/intent/connection and all current range, obstruction and readiness checks still match. A changed hatch state never turns a pinned Open into Close. Missing targets, blocked state, UI ownership or connection changes cancel the gesture. Held input cannot rearm without a fresh press.

Shortcuts require focused player-body viewport ownership, live/fresh authority and no pending command, text/search/computer input, management/engineering workspace, remote/observation/free camera, modifier/mouse input or session modal. Existing server interlocks, interior-only space traversal and routine ownership are preserved. An accepted shortcut uses the existing bound DockedTraversal send path and stops walking/flight input safely.

Coverage excludes room-equipment consoles, arbitrary authored doors, station services, between-landing lift recovery and ambiguous nearby actions; explicit buttons remain available. No console draft integration, blocked scene-init correction or renderer edits, protocol/collision/registration changes, world migrations, or synthetic success states are introduced.

Validate pinning/cancellation, shared blockers, wall/range/ambiguity suppression and exact requests in focused isolated fixtures. Do not move the user's avatar or issue ship actions merely for live acceptance. Build/package/reopen the known PioneerFinish11QA world via a fresh backup and normal save/shutdown, restore prior running mode and record actual live limits in the package receipt.
