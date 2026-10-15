"""Edit the opened Study04 roof into the bounded Study05 service/work study.

Body coordinates are metres (+X bow, +Y up, +Z starboard).  This helper neither
opens nor saves a file.  It preserves the pressure hull, existing material
identities, mining mounts and runtime equipment.  The added handling/service
parts are visual authoring choices inferred from the published v05 concept;
they do not assert engineering ratings, access to a fitted interior, or motion.
"""
import math


def apply(bpy):
    """Apply once to the already-open saved Study04, returning an edit report."""
    from mathutils import Vector

    scene = bpy.context.scene
    if any(o.name.startswith('Study05 roof ') for o in scene.objects):
        raise ValueError('Study05 roof already applied; open the unchanged Study04 source')
    for required in ('Pressure hull', 'Mining turret deck', 'Cargo hatch'):
        if required not in scene.objects:
            raise ValueError('Expected saved Study04 source part: ' + required)
    material_names = (
        'Graphite ceramic armor', 'Titanium machinery', 'Recessed carbon',
        'Emerald structural frames', 'Safety amber', 'Hull identification ivory',
    )
    materials = {name: bpy.data.materials[name] for name in material_names}
    added, removed = [], []

    def xyz(p):
        return (p[0], -p[2], p[1])

    def mesh(label, vertices, faces, material, edge=0.0):
        name = 'Study05 roof ' + label
        if name in bpy.data.objects:
            raise ValueError('Duplicate authored part ' + name)
        data = bpy.data.meshes.new(name + ' mesh')
        data.from_pydata([xyz(p) for p in vertices], [], faces)
        data.update()
        obj = bpy.data.objects.new(name, data)
        scene.collection.objects.link(obj)
        data.materials.append(materials[material])
        if edge:
            bevel = obj.modifiers.new('Manufactured edge radius', 'BEVEL')
            bevel.width = edge
            bevel.segments = 2
            bevel.limit_method = 'ANGLE'
            obj.modifiers.new('Weighted corner normals', 'WEIGHTED_NORMAL')
        added.append(name)
        return obj

    def box(label, center, size, material='Graphite ceramic armor', edge=.05):
        x, y, z = center
        a, b, c = [n / 2 for n in size]
        return mesh(label, [(x-a,y-b,z-c),(x+a,y-b,z-c),(x+a,y+b,z-c),(x-a,y+b,z-c),
                            (x-a,y-b,z+c),(x+a,y-b,z+c),(x+a,y+b,z+c),(x-a,y+b,z+c)],
                    [(0,3,2,1),(4,5,6,7),(0,1,5,4),(3,7,6,2),(0,4,7,3),(1,2,6,5)],
                    material, edge)

    def tube(label, a, b, radius, material='Titanium machinery', sides=12):
        a, b = Vector(a), Vector(b)
        direction = (b-a).normalized()
        reference = Vector((0,1,0)) if abs(direction.y) < .9 else Vector((0,0,1))
        u = direction.cross(reference).normalized()
        w = direction.cross(u).normalized()
        points = [tuple(p + radius*(u*math.cos(i*math.tau/sides)+w*math.sin(i*math.tau/sides)))
                  for p in (a,b) for i in range(sides)]
        faces = [tuple(reversed(range(sides))),tuple(range(sides,2*sides))]
        faces += [(i,(i+1)%sides,(i+1)%sides+sides,i+sides) for i in range(sides)]
        return mesh(label, points, faces, material, min(.035,radius*.15))

    def beam(label, a, b, width, thickness, material='Graphite ceramic armor'):
        # Broad arm side plates, with the thin dimension across the ship.
        a, b = Vector(a), Vector(b)
        axis = (b-a).normalized()
        side = Vector((0,0,1))
        up = side.cross(axis).normalized()
        points = [tuple(p + up*dy*width/2 + side*dz*thickness/2)
                  for p in (a,b) for dy,dz in [(-1,-1),(1,-1),(1,1),(-1,1)]]
        return mesh(label, points, [(0,3,2,1),(4,5,6,7),(0,1,5,4),
                                   (1,2,6,5),(2,3,7,6),(3,0,4,7)], material,
                    min(.045, thickness*.20, width*.15))

    def coaming(label, xmin, xmax, z, bottom, top, width=.34):
        # A short chamfered rim, not a full replacement roof covering the bay.
        return box(label, ((xmin+xmax)/2,(bottom+top)/2,z),
                   (xmax-xmin,top-bottom,width), edge=.09)

    def handle(label, x, y, z, span=.42, direction='x'):
        if direction == 'x':
            a,b = (x-span/2,y+.11,z),(x+span/2,y+.11,z)
            feet = [(x-span/2,z),(x+span/2,z)]
        else:
            a,b = (x,y+.11,z-span/2),(x,y+.11,z+span/2)
            feet = [(x,z-span/2),(x,z+span/2)]
        tube(label+' grip',a,b,.035,sides=8)
        for i,(xx,zz) in enumerate(feet):
            tube(label+' foot '+str(i),(xx,y,zz),(xx,y+.11,zz),.035,sides=8)

    # These are the floating repeated lids and 2.6m-high rails above the actual
    # already-authored Y6.2 deck.  No pressure-hull vertices are touched.
    prefixes = ('Upper armored plate','Cargo access recess','Cargo hatch',
                'Hatch center reinforcement','Hatch hinge','Service handrail',
                'Rail stanchion')
    for obj in list(scene.objects):
        if any(obj.name == p or obj.name.startswith(p+'.') for p in prefixes):
            coords = [obj.matrix_world @ Vector(v) for v in obj.bound_box]
            if max(v.x for v in coords) <= 20.01:
                removed.append(obj.name)
                bpy.data.objects.remove(obj, do_unlink=True)

    # The floor is the real Study04 recess; thin dark inserts sit only 4cm above
    # it.  The central strobe at (-8,6.3,0) retains its clear source patch.
    for label,x0,x1 in [('aft',-23.8,-16.5),('middle',-16.1,-9.0),('forward',-7.0,-1.5)]:
        box(label+' inset service deck',((x0+x1)/2,6.22,0),(x1-x0,.04,12.9),
            'Recessed carbon',.01)
    for side,z in [('port',-6.55),('starboard',6.55)]:
        coaming(side+' aft low coaming',-23.8,-15.9,z,6.24,7.02)
        coaming(side+' forward layered coaming',-15.5,-1.4,z,6.24,7.48)
        box(side+' coaming upper wear strip',(-8.45,7.50,z),(13.8,.09,.40),
            'Titanium machinery',.025)
        # Two deliberately separate green inserts break up the continuous frame.
        for i,x in enumerate([-20.4,-3.3]):
            box(side+' edge armor insert '+str(i),(x,7.03 if i==0 else 7.52,z),
                (2.2,.13,.45),'Emerald structural frames',.04)
    for label,x in [('aft',-23.8),('middle',-16.25),('forward',-1.4)]:
        box(label+' deck cross sill',(x,6.45,0),(.28,.40,12.9),edge=.06)

    # One sizeable removable equipment cover, rather than nine repeated lids.
    box('aft access cover recessed surround',(-20.25,6.35,2.0),(5.8,.21,6.4),
        'Titanium machinery',.10)
    box('aft access cover',(-20.25,6.49,2.0),(5.36,.14,5.96),edge=.10)
    for z in [-.45,4.45]:
        box('aft access cover rib '+str(z),(-20.25,6.59,z),(4.8,.09,.10),
            'Titanium machinery',.025)
    for i,x in enumerate([-22.2,-18.3]):
        tube('aft cover hinge '+str(i),(x,6.61,4.9),(x+.55,6.61,4.9),.12)
    handle('aft cover handhold',-20.2,6.57,-.3,.55)
    box('aft cover visual identity tab',(-18.15,6.59,3.9),(.65,.035,.23),
        'Hull identification ivory',.015)

    # A narrow access route and a normal human-sized top service hatch.  These
    # cues give scale without asserting a working entry or fitted room behind it.
    for i,x in enumerate([-22.7,-20.7,-18.7,-16.7,-14.7,-12.7,-10.7]):
        box('walkway tread '+str(i),(x,6.29,-1.45),(1.7,.10,1.10),
            'Titanium machinery',.025)
        for j,dx in enumerate([-.46,0,.46]):
            box('walkway grip groove '+str(i)+' '+str(j),(x+dx,6.35,-1.45),
                (.045,.025,.78),'Recessed carbon',0)
    box('forward human scale hatch surround',(-4.4,6.38,3.3),(2.8,.24,2.1),
        'Titanium machinery',.06)
    box('forward human scale service hatch',(-4.4,6.55,3.3),(2.45,.13,1.76),
        'Emerald structural frames',.06)
    handle('service hatch handle',-4.05,6.63,3.3,.38,'z')
    tube('service hatch hinge',(-5.55,6.64,2.8),(-5.55,6.64,3.8),.07,sides=10)
    box('hatch fixed warning tab',(-3.52,6.64,3.91),(.35,.035,.13),'Safety amber',.015)

    # Open equipment bay: two compact exchanger blocks with exposed headers and
    # an adjacent pair of organised reels.  Dark backs, light edges and depth
    # matter more here than surface noise or imaginary equipment specifications.
    box('machinery bay open bed',(-12.6,6.34,3.8),(6.1,.16,4.5),'Recessed carbon',.05)
    for i,x in enumerate([-14.2,-11.2]):
        box('exchanger '+str(i)+' housing',(x,6.78,3.75),(2.2,.78,3.45),edge=.09)
        for j in range(6):
            box('exchanger '+str(i)+' fin '+str(j),(x-.82+j*.33,7.20,3.75),
                (.12,.20,3.15),'Titanium machinery',.018)
        tube('exchanger '+str(i)+' near header',(x-1.0,6.85,1.97),(x+1.0,6.85,1.97),.16)
        tube('exchanger '+str(i)+' far header',(x-1.0,6.85,5.53),(x+1.0,6.85,5.53),.16)
    tube('equipment isolated manifold',(-15.45,6.62,1.6),(-9.7,6.62,1.6),.13)
    for i,x in enumerate([-15,-12,-10]):
        tube('manifold branch '+str(i),(x,6.62,1.6),(x,6.62,2.0),.10)
        box('manifold isolation handle '+str(i),(x,6.86,1.6),(.32,.07,.08),'Safety amber',.02)

    # Folded visual service arm, compact enough to remain inside the existing
    # hull envelope.  Paired metal side plates leave a genuinely open centre;
    # the raised knuckle reads above the bay rim without emission or a new mast.
    # It stays aft of the dynamic guns and off the strobe/RCS axis.
    base=(-19.0,6.75,-4.8)
    elbow=(-17.5,8.45,-4.8)
    tip=(-9.8,8.35,-4.8)
    tube('handling arm base foot',(-19,6.25,-4.8),base,.82)
    tube('handling arm base rotary casing',(-19,6.63,-4.8),(-19,7.02,-4.8),.65,
         'Graphite ceramic armor',16)
    for label,a,b,width in [('root link',base,elbow,.73),('folded main link',elbow,tip,.66)]:
        for side,dz in [('port',-.32),('starboard',.32)]:
            aa=(a[0],a[1],a[2]+dz)
            bb=(b[0],b[1],b[2]+dz)
            beam('handling arm '+label+' '+side+' plate',aa,bb,width,.12,
                 'Titanium machinery')
    # Only the end pins bridge the plates; the intervening dark gap remains
    # empty.  Dark centre collars make the angled root joint legible in closeup.
    tube('handling arm root bearing',(base[0],base[1],base[2]-.32),
         (base[0],base[1],base[2]+.32),.28,'Recessed carbon')
    for i,(x,y,z) in enumerate([elbow,tip]):
        tube('handling arm centre bearing '+str(i),(x,y,z-.25),(x,y,z+.25),.30,
             'Recessed carbon')
        tube('handling arm pin '+str(i),(x,y,z-.49),(x,y,z-.25),.39)
        tube('handling arm starboard pin '+str(i),(x,y,z+.25),(x,y,z+.49),.39)
        tube('handling arm amber pin cap '+str(i),(x,y,z-.51),(x,y,z-.49),.25,'Safety amber')
        tube('handling arm starboard amber pin cap '+str(i),(x,y,z+.49),(x,y,z+.51),.25,
             'Safety amber')
    # The hydraulic-looking rod is explicitly visual; it is not a driven actuator.
    tube('handling arm visual cylinder',(-18.85,7.0,-5.19),(-15.55,8.05,-5.19),.14)
    tube('handling arm visual piston',(-15.55,8.05,-5.19),(-13.1,8.08,-5.19),.075)
    beam('handling arm stowed wrist',tip,(-9.1,6.92,-4.8),.42,.44,'Titanium machinery')
    box('handling arm tool cradle',(-9.1,6.52,-4.8),(1.65,.30,1.15),edge=.08)
    for side,z in [('port',-5.2),('starboard',-4.4)]:
        beam('handling arm '+side+' stowed jaw',(-9.4,6.94,z),(-8.7,6.66,z),.16,.12,
             'Titanium machinery')
    for i,(x,y,z) in enumerate([(-14.4,6.98,-3.05),(-11.7,6.90,-3.0)]):
        box('reel '+str(i)+' mounting foot',(x,6.42,z),(1.9,.30,1.1),edge=.07)
        tube('reel '+str(i)+' wound dark core',(x,y,z-.34),(x,y,z+.34),.64,'Recessed carbon',16)
        for j,dz in enumerate([-.40,.40]):
            tube('reel '+str(i)+' flange '+str(j),(x,y,z+dz-.055),(x,y,z+dz+.055),.77,
                 'Titanium machinery',16)
        tube('reel '+str(i)+' hub',(x,y,z-.48),(x,y,z+.48),.18)

    # A recessed forward vent with an angled protective cowl, different in form
    # from both the access cover and the exposed exchanger bay.
    box('forward vent shadow bed',(-4.35,6.37,-3.7),(5.3,.20,3.6),'Recessed carbon',.05)
    for i,x in enumerate([-6.4,-5.65,-4.9,-4.15,-3.4,-2.65]):
        beam('forward vent angled louver '+str(i),(x,6.55,-5.15),(x+.22,7.05,-2.25),.13,.15,
             'Titanium machinery')
    coaming('forward vent inner edge',-6.95,-1.7,-2.0,6.32,7.16,.22)
    box('forward vent partial armor hood',(-4.35,7.30,-4.85),(5.4,.20,1.05),edge=.07)
    for i,x in enumerate([-6.3,-2.4]):
        box('forward vent hood retaining plate '+str(i),(x,7.42,-4.85),(.42,.06,.85),
            'Emerald structural frames',.03)

    # Short 1.1m rails on the aft working edge.  Never cross the central upper
    # RCS ray at X0/Z0 or the forward mining platform and barrel sweep.
    for label,x0,x1,z in [('aft starboard',-22.7,-17.3,6.20),('middle starboard',-15.3,-10.0,6.20)]:
        tube(label+' crew rail',(x0,7.36,z),(x1,7.36,z),.045)
        for i,x in enumerate([x0,(x0+x1)/2,x1]):
            tube(label+' crew rail foot '+str(i),(x,6.25,z),(x,7.36,z),.045)
        tube(label+' warning rail sleeve',(x0+.15,7.36,z),(x0+1.1,7.36,z),.052,'Safety amber')

    # Small sills and a retained layered panel on the transition toward the
    # existing forward gun deck.  Keep a 1.5m circle around the upper RCS ray.
    for label,z in [('port',-5.65),('starboard',5.65)]:
        box(label+' forward transition armor',(3.3,7.27,z),(3.5,.24,2.0),edge=.09)
        box(label+' forward transition step',(3.3,7.08,z),(4.0,.15,2.4),
            'Titanium machinery',.04)

    return {
        'schema': 'pioneer-study05-roof-edit/v1',
        'source_expected': 'saved Study04, source SHA772bf76aaf1416f28357e2f507963022c7c73b13fca58cb57e948e06761a7800',
        'added_parts': added,
        'removed_parts': sorted(removed),
        'retained_critical_parts': ['Pressure hull','Mining turret deck','Turret mounting socket','Turret mounting socket.001'],
        'reference': 'docs/ships/pioneer/images/v05-interior-review.jpg',
        'visual_inference': 'Layered recessed service/work architecture, stowed visual handling arm, reels and human-scale access cues; no fitted interior, functional crane, motion or equipment rating is asserted.',
        'clearances': {'dorsal_strobe_body_m': [-8,6.3,0], 'upper_rcs_axis_xz_m': [0,0],
                       'turret_centres_body_m': [[10.8,9.864,-2.66],[10.8,9.864,2.66]],
                       'new_machinery_max_x_m': -1.4},
    }
