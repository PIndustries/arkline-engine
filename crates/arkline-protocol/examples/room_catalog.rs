//! Export the authoritative catalog; no separate handwritten design data.
fn main() {
    let catalog = arkline_protocol::pioneer_room_catalog();
    if std::env::args().any(|a| a == "--mermaid") {
        println!("flowchart LR");
        for (i, e) in catalog.equipment.iter().enumerate() {
            println!("  n{i}[\"{}\"]", e.label);
        }
        for e in catalog.connections {
            let a = catalog
                .equipment
                .iter()
                .position(|n| n.id == e.from_equipment)
                .unwrap();
            let b = catalog
                .equipment
                .iter()
                .position(|n| n.id == e.to_equipment)
                .unwrap();
            println!(
                "  n{a} -->|{:?}: {} → {}| n{b}",
                e.kind, e.from_port, e.to_port
            );
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&catalog).unwrap());
    }
}
