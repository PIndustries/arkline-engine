use arkline_protocol::*;
use arkline_server::Server;
#[test]
fn corrupt_hangar_and_storage_fail_open_without_panicking() {
    for corruption in ["duplicate", "overflow", "membership"] {
        let path = std::env::temp_dir().join(format!(
            "arkline-asset-validation-{}-{}.sqlite",
            std::process::id(),
            rand::random::<u64>()
        ));
        {
            let mut server = Server::open(&path).unwrap();
            assert!(
                server
                    .handle_at(
                        &mut None,
                        Request::Connect {
                            supports_room_controls: false,
                            supports_fleet_hangar: false,
                            supports_pioneer_interior: true,
                            protocol_version: PROTOCOL_VERSION,
                            pilot: "Pilot".into(),
                            resume_token: None
                        },
                        0
                    )
                    .ok
            );
        }
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            let json: String = db
                .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
            match corruption {
                "duplicate" => {
                    value["accounts"]["Pilot"]["ships"][1] =
                        value["accounts"]["Pilot"]["ships"][0].clone()
                }
                "overflow" => {
                    value["accounts"]["Pilot"]["storage"] =
                        serde_json::json!({"1":u64::MAX,"2":u64::MAX})
                }
                _ => value["accounts"]["Pilot"]["corporation"] = serde_json::json!("Missing"),
            }
            db.execute("UPDATE world SET json=?1 WHERE id=1", [value.to_string()])
                .unwrap();
        }
        assert!(Server::open(&path).is_err(), "accepted {corruption}");
        std::fs::remove_file(&path).unwrap();
        let mut lock = path.as_os_str().to_os_string();
        lock.push(".lock");
        std::fs::remove_file(lock).unwrap();
    }
}
