# Pioneer control recovery and material checkpoint — 20 October 2026

Continues `bf1adf8`. The `.9` app is still the existing package; the material changes below are source-only and have not been packaged or substituted into that process.

## Supported desktop control recovery

Read the current CUA documentation. On macOS it supports app names, bundle IDs and full paths; PID/window-ID targeting is not documented for macOS (window-ID routes are documented for Linux/Windows). Reset the CUA session with `js_reset`, then bind the exact `.9` app path. This succeeded and returned actual `.9` / Registration3QA. Normal Player camera, Deck2 and Play controls worked. Supported window action `Raise` and `getAXStateAndScreenshot` produced a complete 3000x1984 actual-window image after an earlier skewed-thumbnail capture.

Viewport focus matters: W sent with pointer over the toolbar produced no authority input, as designed by viewport ownership. A scoped click within the observed viewport followed by a held W reached real authority; Q and Shift+Q subsequently produced distinct sprint flags. Their positions encountered a doorway/frame and are not accepted as an unobstructed speed comparison. A later centred trial timed out in the adapter and recordings contain more changing input/heading than attributable to the intended one-second trial, so its outcome remains uncertain. No blind global input, ambiguous-bundle fallback, registration removal or permission change was used. Input/observation data is retained under `/tmp/pioneer9-desktop-acceptance-20261020/` (`controlled-motion-trials`, `focused-motion-trials`, `walk-sprint-comparison`, `centered-walk-sprint` JSONL files). Complete desktop journey, gait, hatch timing/obstruction and cockpit input acceptance remain pending; no offscreen pass is renamed desktop acceptance. No app hang was established.

## Bounded material work while desktop control remained unreliable

Read the retained reference assessment and current shared source. Coated seams previously changed albedo only, and the same composite role put service-slot stripes on both ceiling cassettes and carbon equipment. Changes now:

- Separate explicit service-ceiling and matte composite material roles; both retain the same existing cube upload. No new mesh, collision, light or draw instance.
- Shallow, antialiased BRDF-normal relief for seals (1.6 mm), grip (0.7 mm) and cassette slots (0.8 mm), using the same metre conversion in station and flight. Depth, silhouette, aperture and source geometry remain untouched. Fine relief fades by fragment footprint.
- Softer joint albedo contrast and coated response, rather than black painted grids; no random grunge or fake powered circuit.
- Warm crew/galley finish versus cooler lower-deck utility-room coating. Roles alter finish only, not installed equipment or room authority. Installed, cutaway, docked and flight recipes share this attribution.

The first client run found a genuine parity error in centre-point room attribution because walls extend outside their clear interiors. Attribution now uses exact authored obstacle identity; the existing ordered inspection/installed-deck assertion remains intact.

Validation: client suite **383 passed / 0 failed / 35 ignored** (`.build/logs/20261020T163727460052Z.log`); shared editor/gameplay/decks/space native route **passed in 94.58 s** (`20261020T163828513941Z.log`); enclosure 13 views x two resolutions **passed in 12.86 s** (`20261020T164020708866Z.log`); strict native client/all-target Clippy (`20261020T164034818235Z.log`) and Wasm client Clippy (`20261020T164044916962Z.log`) passed. Formatting and diff checks passed. Single guarded target retained disk thresholds.

Visually inspected after captures `/tmp/pioneer-material-response-route-20261020/{editor-deck2-registered-corridor,editor-deck1-registered-corridor,gameplay-galley-fitted-furniture}.png` against retained `.9` source captures. Utility and galley finishes are distinct, floor grip catches surface response and seams are less stark. The result is still flat/repetitive, galley furniture sparse, task lighting and equipment incomplete. Shader work does not replace physical construction/equipment design. Registered geometry and source payload invariance checks passed.

## Steering to the Mining workflow

User clarified a missing Mining tab. A read-only independent audit found the quantity routine survives unchanged from recovered snapshots including `78fdc5d`; current visible route is labelled AI and is hidden on foot. Historic Mining text also appears as an asteroid overview filter, so a distinct older quantity tab is not proved by the shallow recovered history. The existing StartResourceJob authority already handles quantity/full-hold return, drone recall, dock/deposit and repeated trips. Next work restores discoverable UI access to that exact existing workflow; it must not create a duplicate miner. Historical PR/issue leads are treated read-only, with no GitHub edits.
