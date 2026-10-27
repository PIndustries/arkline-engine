//! Hull picking uses the same accepted source triangles and roots as color/depth.
use super::*;

pub(super) fn hull_geometry(
    pilot: &PilotView,
    space: Space,
    snapshot_time: u64,
    presentation_time: u64,
    advertised: Option<&arkline_protocol::CarrierInventoryRegistration>,
) -> Result<Vec<(Vec<Vertex>, Mat4)>, String> {
    if pilot.carrier_inventory.is_none()
        && (advertised.is_none() || pilot.ship_class != ShipClass::Pioneer)
    {
        return Ok(ship_geometry(
            pilot.ship_class,
            transform(
                space.position(pilot, presentation_time),
                Vec3::ONE,
                ship_orientation_at(pilot, presentation_time),
            ),
        ));
    }
    let public = pilot
        .carrier_inventory
        .as_ref()
        .ok_or("Advertised carrier hull projection missing")?;
    if advertised.is_some_and(|registration| registration != &public.binding().registration) {
        return Err("Carrier picking source differs from accepted world".into());
    }
    let parts = carrier_body_parts(pilot, snapshot_time)?;
    let mount = Mat4::from_scale_rotation_translation(
        Vec3::splat(0.01),
        Quat::from_array(public.parent.orientation_xyzw.map(|v| v as f32)).normalize(),
        space.scene(public.parent.position_m),
    );
    if !mount.is_finite() {
        return Err("Carrier picking parent is not representable".into());
    }
    Ok(parts
        .into_iter()
        .map(|(mesh, local, _, _)| (mesh_vertices(mesh), mount * local))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_hull_pick_is_unchanged_and_advertised_missing_never_falls_back() {
        let ctx = egui::Context::default();
        let app = crate::tests::view_test_cockpit(&ctx);
        let pilot = app.own().unwrap();
        let space = Space { origin: [0.; 3] };
        let expected = ship_geometry(
            pilot.ship_class,
            transform(
                space.position(&pilot, 1000),
                Vec3::ONE,
                ship_orientation_at(&pilot, 1000),
            ),
        );
        let actual = hull_geometry(&pilot, space, 1000, 1000, None).unwrap();
        assert_eq!(actual.len(), expected.len());
        for ((a, am), (b, bm)) in actual.iter().zip(&expected) {
            assert_eq!(am, bm);
            assert_eq!(a.len(), b.len());
            assert!(a.iter().zip(b).all(|(a, b)| a.p == b.p));
        }
        let registration =
            &arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
                .unwrap()
                .topology
                .registration;
        assert!(hull_geometry(&pilot, space, 1000, 1000, Some(registration)).is_err());
        let mut unrelated = pilot.clone();
        unrelated.ship_class = ShipClass::Prospector;
        let with_marker = hull_geometry(&unrelated, space, 1000, 1000, Some(registration)).unwrap();
        let without_marker = hull_geometry(&unrelated, space, 1000, 1000, None).unwrap();
        assert_eq!(with_marker.len(), without_marker.len());
        for ((a, am), (b, bm)) in with_marker.iter().zip(&without_marker) {
            assert_eq!(am, bm);
            assert_eq!(a.len(), b.len());
            assert!(a.iter().zip(b).all(|(a, b)| a.p == b.p));
        }
    }

    #[test]
    #[ignore = "Opt-in exact immutable1703 authority Responses; CPU picking only"]
    fn actual1703_hull_pick_uses_source_shoulders_and_confirmed_roots() {
        use sha2::{Digest, Sha256};
        let root =
            std::path::PathBuf::from(std::env::var_os("ARKLINE_CARRIER_EVIDENCE_ROOT").unwrap());
        for (file, hash) in [
            (
                "response-00000.json",
                "613f83cac7afb7d104296779c06a8ccdc188107f58bef940ac71851a15257d9e",
            ),
            (
                "response-00350.json",
                "9d82f0f0ac7d9e531d32dabfa4eafa02ee1ee8c8e49d5bfa086c0b432bdf494a",
            ),
        ] {
            let bytes = std::fs::read(
                root.join("parallel-1703-847b-moving-camera-prefix")
                    .join(file),
            )
            .unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), hash);
            let wrapper: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let response: arkline_protocol::Response =
                serde_json::from_value(wrapper["response"].clone()).unwrap();
            let snapshot = response.snapshot.unwrap();
            let pilot = snapshot.pilots.iter().find(|p| p.name == "Pilot").unwrap();
            let public = pilot.carrier_inventory.as_ref().unwrap();
            let space = Space {
                origin: public.parent.position_m,
            };
            let marker = snapshot.carrier_inventory_registration.as_ref();
            let actual = hull_geometry(
                pilot,
                space,
                snapshot.server_time_ms,
                snapshot.server_time_ms + 800,
                marker,
            )
            .unwrap();
            let reference = carrier_body_parts(pilot, snapshot.server_time_ms).unwrap();
            assert_eq!(actual.len(), reference.len());
            let parent = Mat4::from_scale_rotation_translation(
                Vec3::splat(0.01),
                Quat::from_array(public.parent.orientation_xyzw.map(|v| v as f32)).normalize(),
                space.scene(public.parent.position_m),
            );
            for ((vertices, world), (mesh, local, _, _)) in actual.iter().zip(&reference) {
                assert_eq!(*world, parent * *local);
                assert_eq!(vertices.len(), mesh_vertices(*mesh).len());
            }
            // A ray at the actual source's far positive-Z shoulder must hit it
            // before the old narrow Pioneer, not merely a widened proxy sphere.
            let source_triangle = reference
                .iter()
                .flat_map(|(mesh, local, _, _)| {
                    mesh_vertices(*mesh)
                        .chunks_exact(3)
                        .map(|t| {
                            t.iter()
                                .map(|v| local.transform_point3(v.p))
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .max_by(|a, b| {
                    let az = a.iter().map(|v| v.z).sum::<f32>() / 3.;
                    let bz = b.iter().map(|v| v.z).sum::<f32>() / 3.;
                    az.total_cmp(&bz)
                })
                .unwrap();
            let center = source_triangle.iter().copied().sum::<Vec3>() / 3.;
            assert!(center.z > 30., "actual shoulder must exceed old half-beam");
            let origin = parent.transform_point3(center + Vec3::Z * 10.);
            let direction = parent.transform_vector3(-Vec3::Z).normalize();
            let hit = actual
                .iter()
                .filter_map(|(mesh, model)| ray_mesh_hit(origin, direction, *model, mesh))
                .min_by(f32::total_cmp)
                .unwrap();
            assert!(
                hit < 0.101,
                "hit actual source shoulder rather than old hull"
            );
            assert!(hull_geometry(
                pilot,
                space,
                snapshot.server_time_ms + 1,
                snapshot.server_time_ms,
                marker
            )
            .is_err());
            let mut wrong = marker.unwrap().clone();
            wrong.assembly_source_sha256 = "0".repeat(64);
            assert!(hull_geometry(
                pilot,
                space,
                snapshot.server_time_ms,
                snapshot.server_time_ms,
                Some(&wrong)
            )
            .is_err());
        }
    }
}
