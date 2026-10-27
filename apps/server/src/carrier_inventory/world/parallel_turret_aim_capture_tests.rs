//! Explicit stationary fixture: relocate one real catalog target POSITION only.
//! Actual requests, accepted clocks, power/ore events and ledger remain authority-owned.
use super::*;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "Explicit actual source aim/extraction Response and verified ledger producer"]
fn export_actual_source_turret_aim_and_extraction() {
    let output = std::path::PathBuf::from(std::env::var("ARKLINE_TURRET_AIM_EXPORT").unwrap());
    assert!(!output.exists(), "Never overwrite actual producer evidence");
    std::fs::create_dir_all(&output).unwrap();
    let producer = std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap();
    assert!(producer.len() == 40 && producer.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut f = parallel_tests::fixture();
    let mut w = f.world();
    let target = w
        .asteroids
        .iter_mut()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap();
    let original_position = target.position;
    let direction = arkline_protocol::parallel_turret_aim::direction(-10_f64.to_radians(), 0.);
    target.position =
        std::array::from_fn(|i| PIONEER_MINING_TURRET_MOUNTS_M[0][i] + direction[i] * 1000.);
    let target_id = target.id;
    let fixture_position = target.position;
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target_id });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target_id)
        {
            break;
        }
        f.step();
    }
    let mut frames = Vec::new();
    let mut save = |f: &mut Fixture, label: &str| {
        let response = f.ok(Request::Snapshot);
        let export = f.ok(Request::ExportLedger).ledger.unwrap();
        arkline_verifier::verify(&export, None).unwrap();
        let index = frames.len();
        let response_file = format!("response-{index:05}.json");
        let ledger_file = format!("ledger-{index:05}.json");
        let response_bytes = serde_json::to_vec(
            &serde_json::json!({"producer_commit":producer,"response":response}),
        )
        .unwrap();
        let ledger_bytes = serde_json::to_vec(&export).unwrap();
        std::fs::write(output.join(&response_file), &response_bytes).unwrap();
        std::fs::write(output.join(&ledger_file), &ledger_bytes).unwrap();
        let world = f.world();
        let mechanisms = world.accounts["Pilot"].cockpit_mechanisms.as_ref().unwrap();
        frames.push(serde_json::json!({"file":response_file,"response_sha256":hex::encode(Sha256::digest(&response_bytes)),
            "ledger_file":ledger_file,"ledger_sha256":hex::encode(Sha256::digest(&ledger_bytes)),"sample_ms":f.now,"label":label,
            "aims":mechanisms.turrets.aims,"ship_ore":world.pilots["Pilot"].view.ore,"power":world.pilots["Pilot"].view.power}));
    };
    let initial_ore = f.world().pilots["Pilot"].view.ore;
    save(&mut f, "before-start");
    let accepted = f.ok(Request::StartMining {
        module_id: 0,
        target_id,
    });
    save(&mut f, "accepted-start-same-clock");
    let mut extracted = false;
    for _ in 0..150 {
        f.step();
        save(&mut f, "accepted-100ms-sample");
        if f.world().pilots["Pilot"].view.ore > initial_ore {
            extracted = true;
            break;
        }
    }
    assert!(
        extracted,
        "actual source target must yield through ordinary extraction transaction"
    );
    assert_eq!(
        f.world().pilots["Pilot"].view.ore,
        initial_ore + mining_spec(ShipClass::Pioneer).yield_units
    );
    f.ok(Request::StopMining { module_id: 0 });
    save(&mut f, "accepted-stop-same-clock");
    let receipt = serde_json::json!({"pilot":"Pilot","producer_commit":producer,
        "executable_sha256":hex::encode(Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap())),
        "qualification":"Actual stationary source fixture StartMining/100ms aim/settle/extraction/Stop with independently verified ledgers; one existing target position relocated, no ore/power injection; not normal flight admission",
        "registration":f.binding().registration,"target_id":target_id,"original_target_position":original_position,
        "fixture_target_position":fixture_position,"command":accepted.message,"frames":frames});
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
