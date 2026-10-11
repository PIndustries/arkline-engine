# How the ship systems work together

This is the fleet's behavior reference. It connects the concept drawings, room fittings and proposed system nodes to player actions. **Implemented** means behavior already in the simulation; **proposed** means the intended design still needs code and acceptance. A diagram alone does not make a system operational.

Start with the [fleet galleries](README.md), [system diagrams](systems-reference.md), [room plans](room-services.md) and [implementation gaps](gaps.md). Each ship has its own dimensions and power/cargo values; the common topology below does not mean every hull has identical equipment.

## 1. Power: source to individual equipment

```text
P01 source -> P02 main breaker -> P03 charge bank -> P04 main distribution
                                                     |
                         +---------------------------+------------------+
                         |                                              |
                  P05 industrial feeder                           P06 room feeder
                    |     |      |                               |      |      |
                 P07    P08      P09                            lights outlets HVAC
                 drive  turrets  drone bay

E01 emergency reserve -> E02 breaker -> E03 essential distribution
                                            |          |          |
                                         controls    alarms    emergency air
```

**Implemented:** each ship has a capacitor with capacity, recharge and confirmed action costs. Warp, turret mining cycles and drone activity consume that account; insufficient charge prevents the relevant action. Displayed charge is server data. These gameplay values are not physical reactor specifications.

**Proposed:** each feeder becomes an independently switchable, capacity-limited circuit. Equipment requests power; the server allocates it in priority order. Emergency air, control and alarms take priority over mining and comfort loads. A disabled branch stops its equipment without cutting unrelated branches. Reserve power cannot feed a failed main circuit backwards.

A room outlet is an endpoint on its room branch, not an unlimited energy source. Its panel shows circuit ID, requested/allocated power and state. Player actions are request enable, disable, reset after a cleared fault, or repair. Overload moves a circuit from enabled to tripped; resetting without removing the cause retrips it. Ratings, equipment loads, reserve endurance and circuit simulation remain to be defined and tested.

## 2. Control, wired data, sensors and RF

```text
Pilot / permitted automation
          |
     N01 bridge -> N02 core router -> N03 local control switch
                         |                    |
                 N04 room network     N07 industrial gateway
                  |   |   |                   |
                jacks AP sensors         N08 module controllers
                         |
                  N05 RF transceiver -> N06 antenna -> drones / local network

Local emergency controls -> E05 safe-state controller -> E06 trips / valves
                           (must still work after network loss)
```

**Implemented:** game clients send commands to the authoritative server. Drones have modeled signal and command behavior. Ship automation uses allowed game operations, not authority to rewrite inventory or physics.

**Proposed:** wired jacks, interior access points and field controllers are explicit separate components. A sensor produces a timestamped measurement with source, unit, validity and age. It does not send an actuator command. Control commands require ownership/permission checks even if the issuing terminal is physically connected.

The interface distinguishes `LIVE`, `STALE`, `OFFLINE`, `FAULT` and `DESIGN ONLY`. A stale pressure value must not be colored as a live safe reading. Cable loss disconnects its downstream segment; RF loss degrades external telemetry but leaves local controls available. A compromised ordinary terminal must not gain emergency-controller privileges.

Room sensors proposed in the plans include smoke, heat, air/temperature, pressure, leakage, occupancy and door position. Exact sensor ranges, sample intervals and failure thresholds are not yet implemented. The current Layouts viewer is a design browser, not a sensor dashboard.

## 3. Air, pressure and life support

```text
A01 makeup reserve -> A02 treatment / air handler -> A03 supply -> rooms
                             ^                                |
                             +------ A05 clean return --------+

Airlock -> A07 local controller -> A08 recovery / isolated exhaust
Workshop -> A09 separate extraction
A06 emergency reserve -> isolated essential supply
```

**Proposed:** each sealed room has pressure, temperature and air-quality state. Door and vent connections determine how gas moves between compartments. Fans consume power; scrubbers consume resources and remove contaminants. Stopping circulation is different from immediately losing all breathable air.

An airlock closes and verifies one boundary, equalizes to the destination pressure, then permits the other boundary to open. A failed sensor reports uncertainty; a local manual control is explicit rather than silently bypassing the sequence. Workshop contamination and airlock exhaust do not enter clean cabin return untreated.

A breach creates a leak connected to the outside. The response is alarm, identify compartment, isolate available boundaries, and move occupants toward refuge. Occupancy, pressure targets, consumption rates, gas inventories, equipment capacity and player consequences need gameplay decisions before this can be balanced. They are not simulated by the current room drawings.

## 4. Water and waste

```text
W01 potable tank -> W02 pump / check valve -> W03 branches -> W04 fixtures
                                                            |
                                              W05 graywater collection
                                                            |
                                                W06 treatment / test
                                                            |
                                                 W07 quarantine buffer
                                                            |
                                                 quality-approved return

Toilet / contaminated waste -> W08 retained waste -> W09 service handling
```

**Proposed:** tanks hold finite quantities. Pumps require power. Valves isolate branches. A leaking branch loses stored fluid into its compartment and raises a leak alarm. Repairs require isolation and replaceable resources. A quarantine/test stage must approve reuse; untreated waste never automatically becomes potable water.

Galley, hygiene and medical connections appear only in rooms whose service schedule includes them. A dry equipment room does not acquire plumbing merely because every room shares the same drawing template. Pipe routing, tank capacities, treatment behavior and fluid simulation remain open.

## 5. Cooling and heat

```text
Source / drive / turrets -> C02/C09 heat pickup -> primary coolant
                                                     |
                                            C03 heat exchanger
                                                     |
C06 electronics / C07 air coil -> secondary loop -> C04 heat rejection
                  pumps C01/C05; isolation / expansion C08
```

**Proposed:** equipment adds heat according to its actual operating state. Coolant transports heat; it does not delete it. The rejection system transfers it out of the ship. A pump failure or closed valve reduces flow; temperatures rise over time, equipment derates, then trips if limits are exceeded.

Mining and propulsion therefore compete for sustained cooling capacity as well as power. The UI should show temperature, trend, coolant flow, remaining thermal margin and the limiting component. Potable water and coolant are separate systems. Heat budgets, fluid parameters, radiator placement and thermal damage are not implemented yet.

## 6. Fire detection and suppression

```text
Room smoke / heat sensors -> local alarm -> crew notification
                                  |
                         confirmed response policy
                                  |
                isolate appropriate ventilation / power
                                  |
              local release -> dedicated suppression branch -> heads
```

**Proposed:** the S1/S2 fitting positions reserve candidate suppression heads; they do not prove coverage. Rooms need a hazard-appropriate agent, finite supply and activation rules. Electrical spaces, occupied cabins and workshops may need different responses. Suppression supply is separate from drinking water.

The player must see detection state, inhibited zones, agent availability and local/manual controls. Discharge affects the room and requires inspection/replenishment before returning to service. False alarms, faulty detectors, loss of power, occupied-space warnings and network loss need defined behavior. None of those effects is currently implied by the pink markers in Layouts.

## 7. Turrets, drones and cargo: the first working loop

```text
Select asteroid -> acquire target lock -> assign turret / drone
       -> verify range, signal, power and cargo room
       -> run cycle -> commit extracted ore and energy cost on server
       -> repeat while enabled and valid
       -> return / dock -> transfer ore to named station storage
       -> resume remaining job quantity
```

**Implemented foundation:** discrete turrets, target selection/locking, cycle-based extraction, ship cargo capacity, drone control, travel, docking and station storage. The acceptance work must prove their combined behavior, including multiple trips, interruption and recovery.

Ore should be credited once per committed extraction, never once per client animation. Cargo-full stops extraction instead of deleting ore. A multi-trip job tracks requested total, mined aboard and delivered total separately. Manual takeover pauses/cancels automation through a visible state transition. Reconnecting must not duplicate inventory or silently replace the player with the remote ship.

The physical deck layout adds a future cargo-handling path: turret interfaces feed processing/transfer equipment and cargo containers; drone berths handle launch, recovery, recharge and repair. Those conveyors, processing machines and internal hangar operations are proposals until implemented.

## 8. Doors, room access and player scale

The plans use a 1.8 m avatar reference, 1.4 m by 2.4 m door opening, 2.4 m main corridor and 3.2 m room clear height. These are design reservations; actual collision, camera clearance, equipment access and escape paths must be checked in the playable mesh.

A door needs a clear opening, frame, actuator, local control and position sensor. Access permission and pressure/fire interlocks are separate decisions. Losing ordinary network access must not silently trap the player. Service ports need reachable mounting positions and maintenance clearance, not just a dot inside a room boundary.

Pioneer is the first implementation target. The large ships' compact human cores are not claims that every meter of their industrial hull has a completed room plan.

## End-to-end acceptance

| Scenario | Required observable result |
|---|---|
| Normal mining trip | Ore moves asteroid -> ship -> station exactly once; energy costs and capacity agree after reconnect. |
| Manual takeover | Player identity remains clear, automation yields, return-to-own-ship works. |
| Main power loss (proposed) | Industrial equipment stops; reserve-backed controls and alarms remain for a finite period. |
| Network loss (proposed physical network) | Affected telemetry goes stale; local safe controls still work; unauthorized actions remain denied. |
| Leak / heat / smoke (proposed) | State changes over time, has a visible source, and can be isolated/repaired without resetting the universe. |
| Interior movement | Player fits doors, cannot cross solid walls, can return to ship controls, and cannot undock with an invalid embodiment state. |
| Save and restart | Ownership, cargo, job progress and implemented persistent state survive without duplication. |

Operational state belongs on the server. The client visualizes it and submits requests. Sound, lighting, animations and cockpit displays should follow the same confirmed state transitions, so they explain what is happening rather than inventing activity.
