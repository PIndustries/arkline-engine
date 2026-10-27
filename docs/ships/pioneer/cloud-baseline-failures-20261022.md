# Migration-baseline diagnosis of the eleven client failures

All eleven failures reported at cloud code commit
`cb385d5d6859241323ddde6ea28b530ae462d2e2` also reproduce at the clean migration
checkpoint `3d1129e543824a7e1c515aa03f2af54c2c1f4be7`. Every baseline assertion
signature matches the cloud aggregate failure. None of these eleven failing
tests was introduced by the cloud milestone. This does **not** establish that
they are harmless or exclusively test defects.

## Comparison and limits

The baseline was checked out in the detached worktree
`/workspace/arkline-migration-baseline-20261022`; the active cloud branch was not
reset or switched. Both worktrees remained clean throughout the comparison.
The existing workspace-local environment script was used for both revisions.
Cargo build metadata identifies the baseline's actual `apps/client/src/lib.rs`
and confirms the repository's client test opt-level **1**, debug assertions and
overflow checks enabled, with debug information disabled consistently by the
existing cloud environment. No optimization override was used.

The baseline build reused registry dependencies through the existing target
directory. Its executable and the already-built cloud executable were preserved
separately. Only the eleven named tests were invoked, each with
`--exact NAME --nocapture --test-threads=1`; the full baseline suite was not run.
Both targeted sets report **0 passed, 11 failed, 0 ignored**. The prior cloud
aggregate remains **465 passed, 11 failed, 23 ignored**.

Ten isolated failure signatures agree exactly. In item 6, cloud isolation stops
earlier at the launch acknowledgement, whereas the baseline and cloud aggregate
stop at the later missing Hold control. A second cloud isolated attempt also
stopped early. This variable failure point is retained in the
[comparison receipt](cloud-baseline-failures-20261022.json); it is not normalized
away or claimed to prove a particular resource-contention cause.

## Individual failures

1. `flight3d::hangar::pioneer_interior::room_inspection_tests::every_room_cutaway_is_an_ordered_subset_of_the_installed_deck`
   — D1-P1 has **176** inspected draws; the old budget asserts **28**. The
   preceding ordered-subset checks succeed. Architectural bulkhead and fitted
   furniture recipes now emit multiple parts per obstacle; the fixed old count
   no longer describes the installed recipe.

2. `flight3d::hangar::pioneer_interior::tests::removed_live_room_drops_its_bounds_and_prototype_fixture`
   — The obstacle exists, but the pre-removal render no longer contains one
   centered prototype box with color `(0.09, 0.13, 0.14)`. Storage now renders via
   `fitted_furniture_parts`, with multiple components and different finishes.
   The test fails before removing the room, so it does not establish a room
   removal regression.

3. `interior_acceptance::on_foot_camera_buttons_and_ship_command_boundary`
   — The full-app journey's painted-label click on `First person` leaves
   `ThirdPerson` selected. Its helper chooses text coordinates without checking
   the active interactive rectangle/layer. The existing isolated legacy camera
   selector control test passes, confirming that the selector handler can switch
   views without moving the avatar. The exact full-app clipping/layer/input
   mechanism remains open; this is a preexisting interaction failure, not proven
   to be merely a stale string.

4. `station_ui::tests::docked_hangar_requires_own_view_and_gathering_requires_embarked_pilot`
   — The on-foot restriction remains present, but the test requires the word
   `Enter`. `resource_start_reason()` now returns `Return to the pilot seat to
   assign a mining routine. Status remains visible on foot.` This assertion
   tests obsolete wording rather than the restriction itself.

5. `tests::acquiring_lock_blocks_duplicate_and_extract_until_server_confirmation`
   — The fixture sets presentation time to **6000 ms**, retaining a **1000 ms**
   snapshot. That exceeds the **1500 ms** operations freshness gate. `Cancel
   lock` is disabled by `fresh`, so no `UnlockTarget` enters the channel and
   `try_recv().unwrap()` fails. The fixture's time/snapshot expectation conflicts
   with the existing freshness rule; the rule was not relaxed.

6. `tests::drone_controls_emit_real_launch_hold_recall_and_selected_position`
   — The baseline and cloud aggregate reach the assertion looking up `Hold
   drone`, which is absent from the collected paint labels after introducing a
   deployed drone. The helper draws mining and drone panels into one fixed
   **1000×650** unscrolled viewport; the current selected-drone panel includes
   ownership, camera and grid content before the command row. Exact clipping or
   sizing behavior has not been instrumented. Cloud isolation additionally
   exposes an earlier missing launch acknowledgement, as recorded above. Neither
   symptom is hidden or treated as a passing command journey.

7. `tests::fleet_selection_routes_commands_by_id_and_limits_each_class`
   — Selecting drone 2 succeeds; the next `Hold drone` label lookup fails in the
   same unscrolled helper. This isolates the failure to selected-control
   visibility after selection, rather than proving that drone-ID selection is
   broken. The precise layout mechanism remains open.

8. `tests::hangar_undock_is_visible_on_every_tab_and_disabled_offline`
   — The test requires `Capacitor 2.5/10.0 kJ · reactor 300 W`. The current
   producer labels the quantity `regen rating 300 W`, not `reactor 300 W`.
   Failure occurs before the undock assertions, so it is not evidence that
   undocking itself regressed.

9. `tests::mining_panel_is_readable_on_foot_and_start_remains_disabled`
   — The same obsolete `Enter` wording assertion as item 4 fails before the
   subsequent readability/disabled-command checks.

10. `tests::operations_safety_reports_real_stage_and_full_hold_without_manual_orders`
    — The active-routine ownership text is found, but `Ship hold full` is absent
    from the first **950×1100** paint frame. The producer still contains that
    condition in the selected mining-drone controls. The exact visibility/sizing
    cause is not yet isolated; this remains an unresolved baseline UI failure,
    not a claim that hold capacity or command interlocks are correct.

11. `tests::overview_pointer_drag_persists_and_reflows_in_real_app_update`
    — The helper unwraps a missing `cockpit_overview_overlay` egui area. That ID
    is no longer created by runtime code; overview content is routed through
    Navigation service content. The test stops before exercising dragging or
    persistence, so its old floating-window fixture needs reconciliation with
    the current product behavior.

No assertion, timeout, world state, material/backlight feature or cache behavior
was changed for this diagnosis. No geometry pass has begun. CI remains
unavailable from the previously Forbidden Actions API lookup; that operation
was not retried through another route.
