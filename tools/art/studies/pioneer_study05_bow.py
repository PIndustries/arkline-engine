"""Editable Study05 bridge/bow pass over the explicitly opened Study04 source.

Body metres are (X forward, Y up, Z starboard). This authoring helper changes
geometry only. Access panels/reveals are inferred exterior construction details;
there is no fitted or functional interior, pressure/strength or access claim.
"""
def apply(bpy):
    from mathutils import Vector, Matrix
    import bmesh

    if 'Study05 bridge recessed sill port' in bpy.data.objects:
        raise ValueError('Study05 bow is already present; reopen the selected Study04 master')
    source_names = set(o.name for o in bpy.context.scene.objects)
    changed = []
    created = []
    removed = []
    material = {m.name: m for m in bpy.data.materials}
    armor = 'Graphite ceramic armor'
    metal = 'Titanium machinery'
    carbon = 'Recessed carbon'
    emerald = 'Emerald structural frames'
    glass = 'Navigation glass'

    def cv(p):
        return (p[0], -p[2], p[1])

    def body(p):
        return (p.x, p.z, -p.y)

    def mesh(name, verts, faces, mat, edge=0.0):
        old = bpy.data.objects.get(name)
        data = bpy.data.meshes.new(name + ' Study05 editable surface')
        data.from_pydata([cv(p) for p in verts], [], faces)
        data.update()
        bm = bmesh.new()
        bm.from_mesh(data)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(data)
        bm.free()
        data.update()
        if old:
            old.data = data
            old.matrix_world = Matrix.Identity(4)
            for mod in list(old.modifiers):
                old.modifiers.remove(mod)
            obj = old
            changed.append(name)
        else:
            obj = bpy.data.objects.new(name, data)
            bpy.context.collection.objects.link(obj)
            created.append(name)
        data.materials.append(material[mat])
        if edge:
            mod = obj.modifiers.new('Manufactured edge radius', 'BEVEL')
            mod.width = edge
            mod.segments = 2
            mod = obj.modifiers.new('Weighted manufactured normals', 'WEIGHTED_NORMAL')
            mod.keep_sharp = True
        return obj

    def box(name, center, size, mat, edge=0.03):
        x,y,z = center
        a,b,c = (n/2 for n in size)
        verts = [(x+sx*a,y+sy*b,z+sz*c) for sx,sy,sz in
                 [(-1,-1,-1),(-1,-1,1),(-1,1,1),(-1,1,-1),
                  (1,-1,-1),(1,-1,1),(1,1,1),(1,1,-1)]]
        faces = [(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
        return mesh(name, verts, faces, mat, edge)

    def prism(name, polygon, outward, depth, mat, edge=0.0):
        normal = Vector(outward).normalized()
        points = [Vector(p) for p in polygon]
        if (points[1]-points[0]).cross(points[2]-points[0]).dot(normal) < 0:
            points.reverse()
        n = len(points)
        verts = [tuple(p) for p in points] + [tuple(p-normal*depth) for p in points]
        faces = [tuple(range(n)), tuple(reversed(range(n,2*n)))]
        faces.extend((i,(i+n-1)%n,(i+n-1)%n+n,i+n) for i in range(n))
        return mesh(name, verts, faces, mat, edge)

    def bar(name, a, b, width, depth, mat, normal):
        a,b = Vector(a),Vector(b)
        n = Vector(normal).normalized()
        lateral = (b-a).cross(n).normalized()*width/2
        return prism(name,[a-lateral,b-lateral,b+lateral,a+lateral],n,depth,mat,min(.025,width*.2))

    # Retain every aft vertex exactly. Only the three existing bow stations at
    # X26, X32 and X35.72 change: the old deep lower wedge and tall roof collapse
    # into an inhabited-core-height bow with an explicit underside step.
    hull = bpy.data.objects['Pressure hull']
    original_world = [hull.matrix_world @ v.co for v in hull.data.vertices]
    changed_vertices = 0
    targets = {26:(-6.35,6.8),32:(-4.9,4.8),35.72:(-3.65,3.72)}
    old_sections = {26:(-8.2,7.6),32:(-7.8,6.1),35.72:(-5.8,5.0)}
    for vertex,p in zip(hull.data.vertices, original_world):
        x,y,z = body(p)
        station = min(targets, key=lambda q: abs(x-q))
        if abs(x-station) > .002:
            continue
        old_bottom,old_top = old_sections[station]
        bottom,top = targets[station]
        yy = bottom+(y-old_bottom)/(old_top-old_bottom)*(top-bottom)
        vertex.co = hull.matrix_world.inverted() @ Vector(cv((x,yy,z)))
        changed_vertices += 1
    hull.data.update()
    changed.append(hull.name)

    # The old hull has a solid front cap. Remove actual material, rather than
    # laying luminous panes over it. A rectangular recess joins the two corner
    # recesses into a closed-bottom bridge pocket; the glass sits 0.54m behind
    # its exterior perimeter and ahead of its opaque back wall. Booleans are
    # applied now; the saved source keeps only Bevel/Weighted Normal modifiers.
    def cut_box(center,size):
        cutter = box('Study05 temporary bridge cavity cutter',center,size,carbon,0)
        bpy.context.view_layer.objects.active = hull
        mod = hull.modifiers.new('Study05 actual bridge cavity', 'BOOLEAN')
        mod.operation = 'DIFFERENCE'
        mod.solver = 'EXACT'
        mod.object = cutter
        # Cut the unmodified hull; leave the existing final bevel after the cut.
        bpy.ops.object.modifier_move_up(modifier=mod.name)
        bpy.ops.object.modifier_apply(modifier=mod.name)
        bpy.data.objects.remove(cutter,do_unlink=True)
        created.remove('Study05 temporary bridge cavity cutter')
    cut_box((35.7,1.03,0),(4.1,3.7,17.0))
    for s in (-1,1):
        cut_box((33.2,1.03,s*9.75),(5.2,3.7,4.7))
    hull.data.update()

    # Existing identity survives even where geometry becomes a different panel.
    # These are thin faceted armor skins over the modified core, rather than a
    # continuous tall opaque forehead. Gaps deliberately expose the dark core.
    prism('Bridge central armored brow',
          [(26.3,6.9,-6.5),(26.3,6.9,6.5),(35.9,3.88,6.9),(35.9,3.88,-6.9)],
          (.30,.95,0),.23,armor,.055)
    for s,label in ((-1,'port'),(1,'starboard')):
        prism('Bridge corner armored brow '+label,
              [(26.4,6.7,s*6.8),(26.4,6.1,s*10.0),(31.8,4.75,s*11.2),(35.88,3.66,s*7.15)],
              (.25,.93,s*.28),.20,armor,.05)
        # Short visible central seam and edge inset break the previous single
        # forehead slab into plausible manufactured plates.
        bar('Study05 bridge brow seam '+label,(27.0,6.75,s*3.1),(35.5,4.08,s*3.8),.08,.04,carbon,(.30,.95,0))
        # Shoulder protection stays outside the bridge cavity and has a compact
        # front section, rather than the old wide rising slab at the brow.
        shoulder = bpy.data.objects['Integrated forward shoulder '+label]
        for vertex in shoulder.data.vertices:
            p = shoulder.matrix_world @ vertex.co
            x,y,z = body(p)
            yy = y - max(0,min(1,(x-26)/5.0))*.9
            vertex.co = shoulder.matrix_world.inverted() @ Vector(cv((x,yy,z)))
        shoulder.data.update()
        changed.append(shoulder.name)

    windows = []
    for s,label in ((-1,'port'),(1,'starboard')):
        # Front face and two faceted corners, all small solid panes. These are
        # opaque dark-glass surfaces in the existing engine material contract.
        front = [(35.20,-.42,s*.25),(35.20,-.42,s*6.40),
                 (35.20,2.50,s*6.40),(35.20,2.50,s*.25)]
        corner = [(35.20,-.42,s*6.55),(35.20,-.42,s*7.70),
                  (35.20,2.35,s*7.70),(35.20,2.50,s*6.55)]
        chamfer = [(35.20,-.42,s*7.85),(34.45,-.34,s*8.65),
                   (34.45,2.38,s*8.65),(35.20,2.35,s*7.85)]
        wrap = [(34.32,-.32,s*8.78),(31.65,-.15,s*10.85),
                (31.65,2.45,s*10.85),(34.32,2.38,s*8.78)]
        pane_specs = [('Bridge front glazing '+label,front,(1,0,0)),
                      ('Bridge frontal corner glass '+label,corner,(1,0,0)),
                      ('Bridge chamfer corner glass '+label,chamfer,(.73,0,s*.68)),
                      ('Bridge wrap glazing '+label,wrap,(.61,0,s*.79))]
        for name,poly,normal in pane_specs:
            prism(name,poly,normal,.045,glass)
            center = sum((Vector(p) for p in poly),Vector())/len(poly)
            windows.append({'name':name,'center_body_m':list(center),
                            'outward_body':list(Vector(normal).normalized()),
                            'glass_depth_m':.045})
        # Perimeter sills and top lintels stand proud of the glazing by 0.54m.
        # Sloping solid side returns join their exterior edge to the pane edge.
        x=35.82
        box('Integrated bridge chin '+label,(35.93,-1.10,s*3.9),(.14,.52,7.5),armor,.04)
        box('Bridge corner lower frame '+label,(35.72,-.67,s*7.13),(.23,.24,1.40),metal,.03)
        bar('Bridge corner upper frame '+label,(35.72,2.78,s*6.50),(35.72,2.62,s*7.80),.20,.20,metal,(1,0,0))
        # The old diagonal crossed whole panes with a thick silvery stripe.
        # Retained name now denotes a narrow load-bearing vertical divider.
        box('Bridge front brace '+label,(35.72,1.02,s*6.48),(.24,3.12,.13),metal,.025)
        for k,(a,b) in enumerate(zip(wrap,wrap[1:]+wrap[:1])):
            n=Vector((.61,0,s*.79)).normalized()
            a=Vector(a)+n*.47
            b=Vector(b)+n*.47
            bar('Bridge wrap frame '+label+' '+str(k),a,b,.18,.22,metal,n)
        prism('Study05 bridge recessed sill '+label,
              [(35.82,-.69,s*.18),(35.82,-.69,s*7.78),(35.20,-.42,s*7.78),(35.20,-.42,s*.18)],
              (0,1,0),.16,carbon,.025)
        prism('Study05 bridge recessed upper reveal '+label,
              [(35.82,2.86,s*.18),(35.20,2.50,s*.18),(35.20,2.50,s*7.78),(35.82,2.71,s*7.78)],
              (0,-1,0),.16,carbon,.025)
        for sign,z in ((-1,.14),(1,7.82)):
            box('Study05 bridge front gasket '+label+' '+str(sign),(35.26,1.035,s*z),(.09,3.14,.10),carbon,.01)
        # Diagonal narrow support references the concept glazing subdivisions.
        bar('Study05 cockpit diagonal brace '+label,(35.27,-.35,s*1.10),(35.27,2.43,s*4.15),.075,.07,metal,(1,0,0))
        # Small fixed exterior service hatch: 1.05m × 1.55m, with a hand-sized
        # latch. It is a modeled scale cue, not a working entrance or interior.
        # This X26→32 side facet tapers 4.6m in Z over 6m in X.
        # Fit the hatch to that real sloping hull plane, rather than burying a
        # world-axis plate in its skin. Front/back polygons define a fixed panel.
        side_n = Vector((4.6/6,0,s)).normalized()
        side_t = Vector((1,0,-s*4.6/6)).normalized()
        hatch_center = Vector((30.30,.55,s*(16.4-(30.30-26)*4.6/6)))
        def hatch_rect(w,h,offset):
            c=hatch_center+side_n*offset
            up=Vector((0,h/2,0));right=side_t*w/2
            return [c-right-up,c+right-up,c+right+up,c-right+up]
        prism('Study05 bridge service hatch backing '+label,hatch_rect(1.34,1.87,.09),side_n,.13,carbon,.025)
        prism('Study05 bridge service hatch '+label,hatch_rect(1.08,1.60,.15),side_n,.07,armor,.03)
        latch_c=hatch_center+side_t*.33+side_n*.23
        up=Vector((0,.13,0));across=side_t*.055
        prism('Study05 bridge service hatch latch '+label,[latch_c-across-up,latch_c+across-up,latch_c+across+up,latch_c-across+up],side_n,.07,metal,.012)
        for offset in (-.47,.47):
            hc=hatch_center-side_t*.52+Vector((0,offset,0))+side_n*.23
            up=Vector((0,.09,0));across=side_t*.05
            prism('Study05 bridge service hatch hinge '+label+' '+str(offset),[hc-across-up,hc+across-up,hc+across+up,hc-across+up],side_n,.07,metal,.012)
        # One upper access coaming and two visible fasteners, not noise coverage.
        bar('Study05 bridge armor parting line '+label,(27.0,5.53,s*10.25),(31.3,4.29,s*11.53),.10,.05,carbon,(0,.96,s*.28))

    box('Bridge center mullion',(35.72,1.035,0),(.24,3.35,.22),metal,.03)
    box('Bridge panoramic frame',(35.72,-.69,0),(.25,.24,13.0),metal,.03)
    box('Bridge panoramic frame.001',(35.72,2.78,0),(.25,.24,13.0),metal,.03)
    # New convex authoring solids must remain positive and within the existing
    # canonical envelope. No parenting/instances/constraints or extra materials.
    bpy.context.view_layer.update()
    all_changed = sorted(set(changed+created))
    for name in all_changed:
        obj=bpy.data.objects.get(name)
        if obj and obj.type=='MESH':
            if obj.matrix_world.determinant() <= 0:
                raise ValueError('Non-positive transform: '+name)
            if any(m.type not in {'BEVEL','WEIGHTED_NORMAL'} for m in obj.modifiers):
                raise ValueError('Unexpected saved modifier: '+name)
    return {'schema':'pioneer-study05-bow/v1','changed_objects':sorted(set(changed)),
            'new_objects':sorted(set(created)),'removed_objects':removed,
            'pressure_hull_bow_vertices_changed':changed_vertices,
            'actual_hull_cut':'front and wrap bridge cavities, applied exact difference',
            'windows':windows,'canonical_materials_added':[],
            'inferred_details':['exterior fixed service hatches/latches','armor seams and reveal construction'],
            'not_implemented':['fitted crew interior','working doors/access','structural or pressure certification'],
            'source_names_preserved':sorted(source_names & set(o.name for o in bpy.context.scene.objects))}
