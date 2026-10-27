//! New actual source-world command recording; never modifies earlier films.
use super::*;
use sha2::{Digest, Sha256};
#[test]
#[ignore = "Explicit fresh-directory actual1703 physical turret recording; no normal travel admission"]
fn export_actual_1703_turret_stow_and_redeploy_samples() {
    let out = std::path::PathBuf::from(std::env::var("ARKLINE_TURRET_SAMPLE_OUTPUT").unwrap());
    assert!(out.is_dir() && std::fs::read_dir(&out).unwrap().next().is_none());
    let producer = std::env::var("ARKLINE_TURRET_PRODUCER_COMMIT").unwrap();
    assert!(producer.len() == 40 && producer.bytes().all(|b| b.is_ascii_hexdigit()));
    let executable = hex::encode(Sha256::digest(
        std::fs::read(std::env::current_exe().unwrap()).unwrap(),
    ));
    let mut f = parallel_tests::fixture();
    let vessel = f.world().accounts["Pilot"]
        .vessel_id(ShipClass::Pioneer)
        .unwrap()
        .to_owned();
    let source = f.world().carrier_inventory_registration.unwrap();
    assert_eq!(
        source.assembly_source_sha256,
        arkline_protocol::parallel_turret_binding::SOURCE_SHA256
    );
    let start = f.now;
    let mut frames = Vec::new();
    let mut commands = Vec::new();
    let mut capture = |f: &mut Fixture, index: usize| {
        let response = f.ok(Request::Snapshot);
        assert!(response.resume_token.is_none());
        let snapshot = response.snapshot.as_ref().unwrap();
        let pilot = &snapshot.pilots[0];
        let state = arkline_protocol::parallel_turret_binding::confirmed_state(
            pilot,
            snapshot.server_time_ms,
        )
        .unwrap();
        let fractions = state.fractions_at(snapshot.server_time_ms);
        assert_eq!(snapshot.server_time_ms, start + index as u64 * 100);
        let file = format!("response-{index:05}.json");
        let row =
            serde_json::json!({"producer_commit":producer,"frame_index":index,"response":response});
        let bytes = serde_json::to_vec(&row).unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        std::fs::write(out.join(&file), bytes).unwrap();
        frames.push(serde_json::json!({"file":file,"response_sha256":hash,"sample_ms":start+index as u64*100,"fractions":fractions}));
    };
    let stow = f.ok(Request::SetTurretDeployment {
        vessel_id: vessel.clone(),
        deployed: false,
    });
    commands.push(serde_json::json!({"sample_ms":f.now,"deployed":false,"accepted":stow.ok,"message":stow.message}));
    capture(&mut f, 0);
    for i in 1..=24 {
        f.step();
        capture(&mut f, i);
    }
    assert!(f.world().accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .secure_at(f.now));
    let deploy = f.ok(Request::SetTurretDeployment {
        vessel_id: vessel,
        deployed: true,
    });
    commands.push(serde_json::json!({"sample_ms":f.now,"deployed":true,"accepted":deploy.ok,"message":deploy.message}));
    for i in 25..=48 {
        f.step();
        capture(&mut f, i);
    }
    assert!(f.world().accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .operational_at(f.now));
    let receipt = serde_json::json!({"qualification":"Actual accepted captain Stow/Deploy requests and49 real100ms source-world Responses; stationary1703 inspection only, no warp/normal admission or interpolated frames","pilot":"Pilot","producer_commit":producer,"test_executable_sha256":executable,"registration":source,"start_ms":start,"end_ms":f.now,"commands":commands,"frames":frames});
    std::fs::write(
        out.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
