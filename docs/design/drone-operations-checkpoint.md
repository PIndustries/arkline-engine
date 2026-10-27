# Physical drone operations — local integration checkpoint

This revision connects the existing finite physical bay to explicit class-deployment and Recall All queues and a seated cockpit bay monitor. It is not a migration or a published runtime release.

## Compatibility and authority

- Existing Legacy/Orbital worlds retain legacy spawn/near-ship removal behavior and are explicitly labelled. Existing physical schema1 worlds retain individual operations.
- Editor Session offers a confirmed, separate create-or-reopen Physical flagship preview. Back/Close do not create or switch worlds; existing world contents remain unchanged.
- Newly created physical worlds advertise batch capability4/schema2. Geometry registration12 and immutable DockingV1 layout2 are unchanged. Older writers cannot discard the fresh batch stamp.
- Deploy class and Recall All retain a bounded immutable member roster, stable IDs/HP/home cradles, and exact retryable intents. One existing physical route runs at a time. Pending recovery is Active/Hold, never Stored. Stored requires actual home arrival and safe machinery.
- Hold/Resume and Cancel remaining retain safe current motion/cleanup. Restart/disconnect holds rather than completing offline. Resource jobs and manual queues cannot steal each other's vessel. STOP, neutral input and inspection remain available.
- Journals remain retained without eviction in this preview; total persistent history is not bounded.

## Presentation

The side-by-side operations controls and real Wide/Doorway monitor fit the supported900px viewport. The monitor does not redirect captain controls, selection or audio listener. It is unavailable on foot rather than allocating a second Hangar renderer budget. Full source hull/doors remain visible.

One existing renderer/cache admits at most one capture per UI pass, with13 physical sources plus at most one operations image. Each candidate uses its own source clock; image status reports actual cache age. Session epochs prevent same-named vessels in different worlds from reusing old monitor pixels. No timer or animation determines stock state.

## Evidence and limits

- Authority final4f1eb584:107 protocol tests;162 server tests,13 explicit ignores;13 new batch tests; check/strict/fmt pass. Durable authority archive:libfile_a84d74194fd8819188338d99d9d4fd27.
- Final authority export contains6303 actual100ms Responses,630.2s total, IDs6–10, final15Stored/0Active, unchanged HP/ore/credits. Each last-live pose equals its home pose before Stored; preceding phase SecuringCradle, door closed. Deployment rates and geometry are unchanged.
- Integrated client642/0/54 ignored and workspace strict Clippy passed before final native acceptance. Final fresh-profile/schema2, full workspace and native interaction/continuous evidence are recorded separately; do not infer a final aggregate from these intermediate checks.
- Captures are production software-renderer evidence, not live hardware FPS. Full fixed-bay/aft documentary motion is distinct from sampled cockpit UI/monitor proof. Camera-capture test warmup passes are explicit, not simulated extra ship motion.
- Eight inherited unavailable historical proof assets remain unchanged Git references; all present source/art bytes are retained. No substitute assets were fabricated.
