//! Explicit validation of a retained copy; never points at a live project.
use arkline_server::editor::EditorWorld;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
fn state(path: &std::path::Path) -> (serde_json::Value, Vec<(i64, String)>, u64) {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let world: String = c
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let mut q = c
        .prepare("SELECT sequence,json FROM ledger ORDER BY sequence")
        .unwrap();
    let ledger = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let clock: String = c
        .query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    (
        serde_json::from_str(&world).unwrap(),
        ledger,
        clock.parse().unwrap(),
    )
}
#[test]
#[ignore = "Explicit saved-world copy compatibility; ARKLINE_COCKPIT_SAVED_COPY required"]
fn cockpit_refit_opens_saved_copy_without_world_or_ledger_migration() {
    let root = PathBuf::from(std::env::var_os("ARKLINE_COCKPIT_SAVED_COPY").unwrap());
    assert!(root.starts_with("/tmp/pioneer-console-refit-copy-20261021"));
    assert!(root.join("COPY_ONLY").is_file());
    let database = root.join("world.sqlite");
    let before = state(&database);
    let world = EditorWorld::open(&root)
        .expect("new console geometry must accept saved traversal without migration");
    assert!(world.is_paused());
    drop(world);
    let mut after = state(&database);
    assert_eq!(
        after.1, before.1,
        "opening must preserve every full ledger row"
    );
    assert_eq!(
        after.2, before.2,
        "opening must not advance the editor clock"
    );
    // Existing normal load refreshes only actuator sample timestamps to the
    // saved clock. Do not permit pose, job, cargo, geometry or ledger changes.
    for (name, account) in before.0["accounts"].as_object().unwrap() {
        if let Some(bindings) = account.get("docked_actuators").and_then(|v| v.as_object()) {
            for (binding, actuator) in bindings {
                for leaf in [
                    "/lift/updated_at_ms",
                    "/hatches/0/updated_at_ms",
                    "/hatches/1/updated_at_ms",
                ] {
                    let old = actuator.pointer(leaf).unwrap();
                    let current = after.0["accounts"][name]["docked_actuators"][binding]
                        .pointer_mut(leaf)
                        .unwrap();
                    assert!(current.as_u64().unwrap() >= old.as_u64().unwrap());
                    assert_eq!(current.as_u64().unwrap(), before.2);
                    *current = old.clone();
                }
            }
        }
    }
    assert_eq!(
        after.0, before.0,
        "all saved world values other than actuator sample clocks must remain exact"
    );
}
