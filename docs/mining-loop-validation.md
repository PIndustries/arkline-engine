# Mining-loop acceptance

This acceptance exercises the authoritative gameplay loop using ordinary commands. It does not seed ore, edit the database, teleport ships or bypass lock/range/power/cargo rules.

## Deterministic regression

```sh
cargo test --locked -p arkline-server --test mining_loop_acceptance
```

The tests use the real server logic with controlled time. A new Pioneer completes a 1,200-ore job (larger than its 1,000-unit hold) with two turrets and mining drones, returns, docks and stores multiple hauls. Assertions cover capacity, cumulative mined/stored totals and independent economic-ledger replay. Additional steps cover successful manual takeover, disconnect cancellation, valid-token recovery, invalid-token rejection and reopening persistent storage with a formerly active job. Disconnecting during extraction must not award more ore.

These tests complement `mining_modules`, `resource_jobs` and `resource_job_atomicity`, including existing full-hold and transaction-failure regressions.

## Real-clock TCP journey

```sh
cargo build --locked -p arkline-server -p arkline-verifier
python3 tools/mining-loop-acceptance.py --receipt /tmp/arkline-mining-receipt.json
```

Allow several minutes. The script starts its own loopback-only server on an unused ephemeral port with a temporary world. It creates two pilots and keeps both connections polling: an idle TCP session expires after 120 seconds, and an offline controller intentionally cannot continue an AI job.

The manual pilot undocks, travels to the belt, approaches and locks an actual asteroid, runs both turrets and a mining drone, retargets when finite asteroids deplete, fills cargo to capacity, recalls the drone, returns, docks and unloads, then repeats a smaller haul. In parallel an AI worker delivers at least 1,200 ore through multiple trips. The script then checks manual takeover, disconnect/reconnect and an abrupt server restart with an active job, comparing economic history before and after recovery. The standalone verifier must accept the resulting ledger.

Resume tokens remain in memory and are not printed in the receipt. The script cleans up its clients, server and temporary world on normal completion or a failed assertion. No production endpoint is accepted by the script. The receipt records stage timings and quantities; it is not a load or performance benchmark.

## Limits

This is protocol/server acceptance. It does not prove native/browser pointer control, visible beam alignment, interior transitions, every hull/load combination, economic operator honesty, or large-player concurrency. Interrupted jobs deliberately cancel rather than silently resuming autonomous orders. Restart recovery preserves cargo, storage and economic history; ephemeral drones and active controls do not survive restart. Issues #7 and #8 remain broader than these acceptance checks.

## Recorded local acceptance

On source `04e2af9` (26.10.11.10), the real-clock loopback run completed in approximately 243 seconds. The manual pilot depleted and retargeted an asteroid at 852 cargo, stored 1,000 ore, then completed a second 50-ore delivery. The independent AI pilot stored 1,200 ore across multiple trips. Manual takeover and disconnect/reconnect cancelled active jobs as intended. An abrupt server restart preserved storage and the exact economic event history; the standalone verifier accepted all 138 events. This is a local functional receipt, not a public deployment or load benchmark. No server gameplay code changes were needed to pass this acceptance.
