use super::*;
#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Account {
    /// Legacy and new vessels explicitly start with the exterior branch enabled.
    #[serde(default = "exterior_enabled_by_default")]
    pub(super) pioneer_exterior_enabled: bool,
    #[serde(default)]
    hull_catalog_version: u32,
    #[serde(default)]
    pub(super) fleet_hangar: bool,
    #[serde(default)]
    vessel_ids: Vec<(ShipClass, String)>,
    #[serde(default)]
    pub(super) pioneer_room: Option<PioneerRoomState>,
    #[serde(default)]
    pub(super) room_compatible: bool,
    pub(super) ships: Vec<HangarShip>,
    storage: BTreeMap<u32, u64>,
    corporation: Option<String>,
}
fn exterior_enabled_by_default() -> bool {
    true
}
impl Account {
    pub(super) fn vessel_id(&self, class: ShipClass) -> Option<&str> {
        self.vessel_ids
            .iter()
            .find(|(c, _)| *c == class)
            .map(|(_, id)| id.as_str())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Corporation {
    owner: String,
    members: Vec<String>,
    invitations: Vec<String>,
    storage: BTreeMap<u32, u64>,
}
pub(super) fn ensure_accounts(world: &mut World) {
    for (name, p) in &world.pilots {
        world
            .accounts
            .entry(name.clone())
            .or_insert_with(|| Account {
                pioneer_exterior_enabled: true,
                hull_catalog_version: 1,
                fleet_hangar: false,
                vessel_ids: Vec::new(),
                pioneer_room: None,
                room_compatible: false,
                ships: ALL_SHIP_CLASSES
                    .into_iter()
                    .filter(|class| *class != p.view.ship_class)
                    .map(|ship_class| HangarShip {
                        power: None,
                        ship_class,
                        system_id: HOME_SYSTEM_ID,
                        ore: 0,
                    })
                    .collect(),
                ..Account::default()
            });
    }
    for (name, account) in &mut world.accounts {
        if account.vessel_ids.is_empty() {
            account.vessel_ids = ALL_SHIP_CLASSES
                .into_iter()
                .map(|class| (class, format!("v1:{name}:{class:?}")))
                .collect();
        }
        if account.hull_catalog_version == 0 {
            for class in [ShipClass::Borealis, ShipClass::Stratum, ShipClass::Monolith] {
                if world.pilots[name].view.ship_class != class
                    && !account.ships.iter().any(|s| s.ship_class == class)
                {
                    account.ships.push(HangarShip {
                        power: None,
                        ship_class: class,
                        system_id: HOME_SYSTEM_ID,
                        ore: 0,
                    });
                }
            }
            account.hull_catalog_version = 1;
        }
    }
}
fn storage_view(storage: &BTreeMap<u32, u64>) -> Vec<StationStorage> {
    storage
        .iter()
        .map(|(&system_id, &ore)| StationStorage { system_id, ore })
        .collect()
}
pub(super) fn view(world: &World, name: &str) -> Option<AssetsView> {
    let a = world.accounts.get(name)?;
    Some(AssetsView {
        pioneer_room_layout: None,
        pioneer_room: (world.pilots[name].view.ship_class == ShipClass::Pioneer).then(|| {
            a.pioneer_room
                .clone()
                .unwrap_or_else(|| PioneerRoomState::uncommissioned(format!("v1:{name}:Pioneer")))
        }),
        hangar: a.fleet_hangar.then(|| hangar_view(world, name)),
        pioneer_interior_available: true,
        pioneer_interior_decks: 2,
        ships: a.ships.clone(),
        storage: storage_view(&a.storage),
        invitations: world
            .corporations
            .iter()
            .filter(|(_, c)| c.invitations.iter().any(|p| p == name))
            .map(|(n, _)| n.clone())
            .collect(),
        corporation: a.corporation.as_ref().and_then(|name| {
            world.corporations.get(name).map(|c| CorporationView {
                name: name.clone(),
                owner: c.owner.clone(),
                members: c.members.clone(),
                storage: storage_view(&c.storage),
            })
        }),
    })
}
pub(super) fn stored_ore(world: &World) -> u128 {
    world
        .accounts
        .values()
        .map(|a| {
            a.ships.iter().map(|s| s.ore as u128).sum::<u128>()
                + a.storage.values().map(|v| *v as u128).sum::<u128>()
        })
        .sum::<u128>()
        + world
            .corporations
            .values()
            .map(|c| c.storage.values().map(|v| *v as u128).sum::<u128>())
            .sum::<u128>()
}
pub(super) fn validate(world: &World) -> Result<()> {
    ensure!(
        world.accounts.len() == world.pilots.len(),
        "asset account count mismatch"
    );
    let known_system = |id| systems().iter().any(|s| s.id == id);
    for (name, a) in &world.accounts {
        ensure!(
            a.vessel_ids.len() == ALL_SHIP_CLASSES.len(),
            "invalid vessel identity count"
        );
        for class in ALL_SHIP_CLASSES {
            ensure!(
                a.vessel_ids
                    .iter()
                    .filter(|(c, id)| *c == class && *id == format!("v1:{name}:{class:?}"))
                    .count()
                    == 1,
                "invalid vessel identity"
            );
        }
        let p = world
            .pilots
            .get(name)
            .context("asset account has no pilot")?;
        ensure!(
            a.ships.len() == ALL_SHIP_CLASSES.len() - 1,
            "pilot must own the inactive preview hull catalog"
        );
        for (i, ship) in a.ships.iter().enumerate() {
            ensure!(
                ship.ship_class != p.view.ship_class
                    && !a.ships[..i]
                        .iter()
                        .any(|other| other.ship_class == ship.ship_class),
                "duplicate active or inactive hull class"
            );
            ensure!(
                known_system(ship.system_id),
                "stored hull has unknown system"
            );
            ensure!(
                ship.ore <= cargo_capacity(ship.ship_class),
                "stored hull exceeds cargo capacity"
            );
        }
        ensure!(
            a.storage.keys().all(|id| known_system(*id)),
            "personal storage has unknown system"
        );
        if let Some(corporation) = &a.corporation {
            let c = world
                .corporations
                .get(corporation)
                .context("membership references missing corporation")?;
            ensure!(
                c.members.iter().filter(|member| *member == name).count() == 1,
                "corporation membership mismatch"
            );
        }
    }
    for (name, c) in &world.corporations {
        ensure!(
            c.members.contains(&c.owner),
            "corporation owner is not a member"
        );
        let mut seen = std::collections::BTreeSet::new();
        for member in &c.members {
            ensure!(seen.insert(member), "duplicate corporation member");
            let a = world
                .accounts
                .get(member)
                .context("corporation member missing")?;
            ensure!(
                a.corporation.as_ref() == Some(name),
                "member account references different corporation"
            );
        }
        let mut invites = std::collections::BTreeSet::new();
        for invite in &c.invitations {
            ensure!(invites.insert(invite), "duplicate corporation invitation");
            let a = world
                .accounts
                .get(invite)
                .context("invited pilot missing")?;
            ensure!(
                a.corporation.is_none(),
                "invitation targets corporation member"
            );
        }
        ensure!(
            c.storage.keys().all(|id| known_system(*id)),
            "corporation storage has unknown system"
        );
    }
    Ok(())
}
pub(super) fn handles(r: &Request) -> bool {
    matches!(
        r,
        Request::SelectShip { .. }
            | Request::DepositOre { .. }
            | Request::WithdrawOre { .. }
            | Request::CreateCorporation { .. }
            | Request::InviteCorporation { .. }
            | Request::AcceptCorporation { .. }
            | Request::DepositCorporationOre { .. }
            | Request::WithdrawCorporationOre { .. }
    )
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    r: Request,
    now: u64,
) -> Result<Option<EconomicEvent>> {
    let p = w.pilots.get(name).context("pilot missing")?;
    let capacity = cargo_capacity(p.view.ship_class);
    let system_id = p.view.system_id;
    let docked = p.view.location == Location::Docked;
    let pilot = name.to_string();
    let event = match r {
        Request::SelectShip { ship_class } => {
            ensure!(docked, "boarding requires docking");
            let p = w.pilots.get_mut(name).unwrap();
            if p.view.ship_class == ship_class {
                return Ok(None);
            }
            let account = w.accounts.get_mut(name).unwrap();
            let index = account
                .ships
                .iter()
                .position(|s| s.ship_class == ship_class)
                .context("ship not owned")?;
            let selected = &account.ships[index];
            ensure!(
                selected.system_id == system_id,
                "ship is stored at another station"
            );
            let loaded_units = selected.ore;
            let mut selected_power = selected.power.clone();
            power::advance_state(&mut selected_power, ship_class, now)?;
            let stored_units = p.view.ore;
            let from_ship_class = p.view.ship_class;
            ensure!(
                loaded_units <= cargo_capacity(ship_class),
                "stored cargo exceeds capacity"
            );
            account.ships[index] = HangarShip {
                power: p.view.power.take(),
                ship_class: from_ship_class,
                system_id,
                ore: stored_units,
            };
            p.view.ship_class = ship_class;
            p.view.power = selected_power;
            // A different hull starts with its own idle module phases/reasons.
            p.view.mining_modules.clear();
            p.view.ore = loaded_units;
            EconomicEvent::ShipBoarded {
                from_ship_class,
                pilot,
                ship_class,
                system_id,
                stored_units,
                loaded_units,
            }
        }
        Request::DepositOre { units } | Request::WithdrawOre { units } => {
            ensure!(docked, "storage transfer requires docking");
            ensure!(units > 0, "quantity must be positive");
            let deposit = matches!(r, Request::DepositOre { .. });
            let p = w.pilots.get_mut(name).unwrap();
            let stored = w
                .accounts
                .get_mut(name)
                .unwrap()
                .storage
                .entry(system_id)
                .or_default();
            transfer(&mut p.view.ore, stored, units, deposit, capacity)?;
            if deposit {
                EconomicEvent::PersonalOreDeposited {
                    pilot,
                    system_id,
                    units,
                }
            } else {
                EconomicEvent::PersonalOreWithdrawn {
                    pilot,
                    system_id,
                    units,
                }
            }
        }
        Request::CreateCorporation { name: corporation } => {
            ensure!(
                !corporation.is_empty()
                    && corporation.len() <= 24
                    && corporation
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                "corporation name must be 1-24 ASCII letters, digits, underscore or hyphen"
            );
            ensure!(
                !w.corporations.contains_key(&corporation),
                "corporation already exists"
            );
            let a = w.accounts.get_mut(name).unwrap();
            ensure!(a.corporation.is_none(), "already a corporation member");
            a.corporation = Some(corporation.clone());
            w.corporations.insert(
                corporation.clone(),
                Corporation {
                    owner: pilot.clone(),
                    members: vec![pilot.clone()],
                    invitations: vec![],
                    storage: BTreeMap::new(),
                },
            );
            for c in w.corporations.values_mut() {
                c.invitations.retain(|p| p != name);
            }
            EconomicEvent::CorporationCreated {
                corporation,
                owner: pilot,
            }
        }
        Request::InviteCorporation { pilot: invitee } => {
            ensure!(w.pilots.contains_key(&invitee), "invited pilot not found");
            ensure!(
                w.accounts[&invitee].corporation.is_none(),
                "pilot already belongs to a corporation"
            );
            let corporation = w.accounts[name]
                .corporation
                .clone()
                .context("not a corporation member")?;
            let c = w.corporations.get_mut(&corporation).unwrap();
            ensure!(c.owner == name, "only owner can invite");
            if c.invitations.contains(&invitee) {
                return Ok(None);
            }
            c.invitations.push(invitee.clone());
            EconomicEvent::CorporationInvited {
                inviter: pilot,
                corporation,
                pilot: invitee,
            }
        }
        Request::AcceptCorporation { corporation } => {
            ensure!(
                w.accounts[name].corporation.is_none(),
                "already a corporation member"
            );
            let c = w
                .corporations
                .get_mut(&corporation)
                .context("corporation not found")?;
            ensure!(c.invitations.contains(&pilot), "owner invitation required");
            c.invitations.retain(|p| p != name);
            c.members.push(pilot.clone());
            w.accounts.get_mut(name).unwrap().corporation = Some(corporation.clone());
            for c in w.corporations.values_mut() {
                c.invitations.retain(|p| p != name);
            }
            EconomicEvent::CorporationJoined { corporation, pilot }
        }
        Request::DepositCorporationOre { units } | Request::WithdrawCorporationOre { units } => {
            ensure!(docked, "storage transfer requires docking");
            ensure!(units > 0, "quantity must be positive");
            let deposit = matches!(r, Request::DepositCorporationOre { .. });
            let corporation = w.accounts[name]
                .corporation
                .clone()
                .context("not a corporation member")?;
            let c = w.corporations.get_mut(&corporation).unwrap();
            ensure!(
                c.members.contains(&pilot),
                "corporation membership required"
            );
            ensure!(
                deposit || c.owner == name,
                "only owner can withdraw corporation ore"
            );
            transfer(
                &mut w.pilots.get_mut(name).unwrap().view.ore,
                c.storage.entry(system_id).or_default(),
                units,
                deposit,
                capacity,
            )?;
            if deposit {
                EconomicEvent::CorporationOreDeposited {
                    corporation,
                    pilot,
                    system_id,
                    units,
                }
            } else {
                EconomicEvent::CorporationOreWithdrawn {
                    corporation,
                    pilot,
                    system_id,
                    units,
                }
            }
        }
        _ => unreachable!(),
    };
    Ok(Some(event))
}
fn transfer(
    cargo: &mut u64,
    stored: &mut u64,
    units: u64,
    deposit: bool,
    capacity: u64,
) -> Result<()> {
    if deposit {
        ensure!(*cargo >= units, "insufficient cargo ore");
        *stored = stored.checked_add(units).context("storage overflow")?;
        *cargo -= units;
    } else {
        ensure!(*stored >= units, "insufficient station ore");
        *cargo = add_cargo(*cargo, units, capacity)?;
        *stored -= units;
    }
    Ok(())
}

pub(super) fn hangar_view(w: &World, name: &str) -> HangarView {
    let layout_revision = w.scene_layout.definition().fleet_layout;
    let a = &w.accounts[name];
    let p = &w.pilots[name].view;
    let berths = a
        .vessel_ids
        .iter()
        .filter_map(|(class, id)| {
            let active = *class == p.ship_class;
            let here = if active {
                p.location == Location::Docked
            } else {
                a.ships
                    .iter()
                    .any(|s| s.ship_class == *class && s.system_id == p.system_id)
            };
            here.then(|| {
                let pose = fleet_berth_pose(layout_revision, *class);
                HangarBerth {
                    id: id.clone(),
                    ship_class: *class,
                    position: pose.position,
                    yaw: pose.yaw,
                    boarding_point: pose.boarding_point,
                    active,
                    can_board: p.location == Location::Docked,
                }
            })
        })
        .collect();
    HangarView {
        layout_revision,
        layout: fleet_hangar_layout_for(layout_revision),
        berths,
    }
}
pub(super) fn board(
    w: &mut World,
    name: &str,
    id: &str,
    now: u64,
) -> Result<Option<EconomicEvent>> {
    ensure!(
        w.accounts[name].fleet_hangar,
        "fleet hangar client required"
    );
    let berth = hangar_view(w, name)
        .berths
        .into_iter()
        .find(|b| b.id == id)
        .context("vessel not owned at this station")?;
    let p = &w.pilots[name].view;
    ensure!(p.location == Location::Docked, "boarding requires docking");
    let avatar = p
        .station_avatar
        .as_ref()
        .context("walk to the vessel boarding marker first")?;
    ensure!(
        !avatar.onboard_interior
            && !avatar.seated
            && avatar.jump_started_ms.is_none()
            && flight::distance(avatar.position, berth.boarding_point) <= STATION_BOARD_RANGE_M,
        "walk to the vessel boarding marker first"
    );
    let event = execute(
        w,
        name,
        Request::SelectShip {
            ship_class: berth.ship_class,
        },
        now,
    )?;
    let p = w.pilots.get_mut(name).unwrap();
    p.view.station_avatar = None;
    p.walk_input = None;
    Ok(event)
}
