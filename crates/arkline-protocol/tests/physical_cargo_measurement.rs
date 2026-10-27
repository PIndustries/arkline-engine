//! Explicit external candidate arithmetic gate, not a registered hold factory.
use arkline_protocol::physical_cargo::{MeasuredCargoHold, VolumeCm3};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "Explicit immutable measured-cell candidate path/hash/capacity required"]
fn actual_candidate_disjoint_cells_use_per_cell_conservative_floor() {
    let path = std::env::var("ARKLINE_MEASURED_HOLD").expect("Exact candidate file");
    let expected_sha =
        std::env::var("ARKLINE_MEASURED_HOLD_SHA256").expect("Pinned candidate bytes");
    let expected_cm3: u64 = std::env::var("ARKLINE_MEASURED_HOLD_CM3")
        .unwrap()
        .parse()
        .unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(hex::encode(Sha256::digest(&bytes)), expected_sha);
    let hold: MeasuredCargoHold = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hold.capacity_cm3(), Some(VolumeCm3(expected_cm3)));
    let mut duplicate = hold.clone();
    duplicate.usable_cells.push(hold.usable_cells[0].clone());
    assert_eq!(duplicate.capacity_cm3(), None, "Never double-count a cell");
    eprintln!("MEASURED_CANDIDATE_ARITHMETIC hold={} receipt={} cells={} per_cell_floor_cm3={} source_admission=false", hold.source_hold_id, hold.measurement_receipt_sha256, hold.usable_cells.len(), expected_cm3);
}
