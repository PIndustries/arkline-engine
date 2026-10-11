# Station hangar and player embodiment

Docking displays an interior bay with the active hull at its published dimensions.
The bay scales around that hull; the person remains 1.8 metres tall. This is a private
berth scene for the authenticated pilot, not a shared station concourse.

**Leave ship** creates a server-confirmed on-foot state at the boarding marker.
W/S walk and A/D strafe relative to the camera; left/right arrows turn the character.
Hold the right mouse button to inspect the scene. A visible articulated suited
character represents the pilot. Walk to the illuminated boarding marker and press
**Enter ship** to return aboard. Boarding requires being within 3 metres. **Undock
active vessel** becomes available once aboard. Stored hull selection continues to
use authenticated station inventory; enter the active ship before selecting a
replacement. Inventory, corporation storage and other station services remain
available through their panels.

The server owns avatar position and normalizes diagonal movement to 4 metres/second.
Walk commands expire after 500 ms without renewal; focus loss releases input. Hangar
walls and the ship footprint constrain movement. Disconnect and restart retain the
saved position but neutralize walking. Leaving the ship releases manual remote
control. Ship flight, undocking and resource jobs for the on-foot pilot are rejected;
station movement cannot be submitted through a remote ship-control wrapper.

## Captain's quarters

The connected private room is 18 by 12 metres with a 3.2-metre ceiling regardless
of hull class. Follow the boarding-deck path through its central doorway. Room
walls, bed, desk, sofa, table and locker share their collision layout with the
server. Nearby wall terminals open navigation, owned vessels, inventory,
corporation services and the ship computer. Displays use current ship data and
mark stale/disconnected telemetry as last known data. The hologram uses the active
hull model. Existing service shortcuts remain available.

Approach the sofa and click **Sit on sofa**. The server requires being within
2 metres; **Stand up** or a movement key returns to walking. Neutral input and
reconnection retain sitting. Boarding still requires returning to the ship marker.
Newly introduced obstacles can overlap an old saved walking position; only such
positions move to the safe boarding marker when the world loads.

This foundation does not yet include a walkable ship interior, multiple-player
concourse, animated boarding ramp or arbitrary station construction. The on-foot
model has procedural articulation rather than a production character asset/rig.
The next acceptance gate is the real dock → leave → walk → enter → undock journey,
including reconnect and varied hull sizes, followed by the complete mining loop.
