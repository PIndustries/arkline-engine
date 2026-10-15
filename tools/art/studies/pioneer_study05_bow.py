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

    # Three separate 0.32m armor solids have open 0.20m joints. Their
    # longitudinal bends and lowered outer facets replace the old surface seam
    # strips. The existing core remains visible at the bottom of each joint.
    def roof_panel(name, sections):
        verts=[]
        for x,z_left,z_right,y_left,y_right in sections:
            verts.extend([(x,y_left,z_left),(x,y_right,z_right),
                          (x,y_right-.32,z_right),(x,y_left-.32,z_left)])
        faces=[(0,3,2,1)]
        for k in range(len(sections)-1):
            for i in range(4):
                a=k*4+i; b=k*4+(i+1)%4
                faces.append((a,b,b+4,a+4))
        faces.append(tuple(range((len(sections)-1)*4,len(sections)*4)))
        return mesh(name,verts,faces,armor,.045)

    center_plate=roof_panel('Bridge central armored brow',
                 [(26.3,-3.05,3.05,7.15,7.15),
                  (31.1,-3.40,3.40,5.80,5.80),
                  (35.9,-3.75,3.75,4.08,4.08)])
    for s,label in ((-1,'port'),(1,'starboard')):
        sections=[(26.3,s*3.25,s*6.50,7.11,6.94),
                  (31.1,s*3.60,s*6.70,5.76,5.34),
                  (35.9,s*3.95,s*6.90,4.04,3.91)]
        roof_panel('Study05 bridge brow armor '+label,sections)
        # A separate descending terrace keeps a real edge between the crown
        # plate and the outer green shoulder, as in the reference architecture.
        prism('Bridge corner armored brow '+label,
              [(26.4,6.65,s*6.78),(26.4,6.1,s*10.0),
               (31.8,4.75,s*11.2),(35.88,3.62,s*7.15)],
              (.25,.93,s*.28),.20,armor,.05)
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

    # One small service cover supplies a human maintenance scale cue. Its
    # coaming surrounds an actual hole through the center armor panel; the
    # cover is set 0.20m down in that pocket, above the existing opaque core.
    # This is fixed authored exterior geometry, with no door/interior claim.
    service_u=Vector((1,-1.35/4.8,0)).normalized()
    service_v=Vector((0,0,1))
    service_n=Vector((1.35/4.8,1,0)).normalized()
    service_center=Vector((29.1,7.15-(29.1-26.3)*1.35/4.8,0))
    def service_rect(length,width,offset):
        c=service_center+service_n*offset
        u=service_u*length/2;v=service_v*width/2
        return [c-u-v,c+u-v,c+u+v,c-u+v]
    cutter=prism('Study05 temporary brow service cutter',
                 service_rect(1.65,1.15,.40),service_n,1.0,carbon)
    bpy.context.view_layer.objects.active=center_plate
    mod=center_plate.modifiers.new('Cut recessed service cover aperture','BOOLEAN')
    mod.operation='DIFFERENCE';mod.solver='EXACT';mod.object=cutter
    while center_plate.modifiers.find(mod.name)>0:
        bpy.ops.object.modifier_move_up(modifier=mod.name)
    bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.data.objects.remove(cutter,do_unlink=True)
    created.remove('Study05 temporary brow service cutter')
    # Four connected loops make a single solid, bevelled coaming ring.
    loops=[service_rect(1.95,1.45,.045),service_rect(1.65,1.15,.045),
           service_rect(1.95,1.45,-.15),service_rect(1.65,1.15,-.15)]
    verts=[tuple(p) for loop in loops for p in loop]
    faces=[]
    for k in range(4):
        j=(k+1)%4
        faces.extend([(k,j,4+j,4+k),(8+k,12+k,12+j,8+j),
                      (k,8+k,8+j,j),(4+k,4+j,12+j,12+k)])
    mesh('Study05 bridge brow service coaming',verts,faces,metal,.02)
    prism('Study05 bridge brow service cover',service_rect(1.57,1.07,-.20),
          service_n,.065,armor,.025)
    # A 0.22m hand latch remains below the coaming's top, so it cannot read as a
    # full-size crew access hatch or an unrelated raised roof ornament.
    c=service_center+service_n*(-.12)+service_u*.40
    u=service_u*.035;v=service_v*.11
    prism('Study05 bridge brow service latch',[c-u-v,c+u-v,c+u+v,c-u+v],
          service_n,.065,metal,.012)

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
        # All four inner gasket strips meet at the actual front-pane corners.
        # Their back faces overlap the glass by 0.02m, avoiding a floating trim
        # strip or a gap between the gasket and opaque pane edge.
        for sign,z in ((-1,.25),(1,6.40)):
            bar('Study05 bridge front gasket '+label+' '+str(sign),
                (35.32,-.42,s*z),(35.32,2.50,s*z),.10,.14,carbon,(1,0,0))
        for y,edge_name in ((-.42,'lower'),(2.50,'upper')):
            bar('Study05 bridge front gasket '+label+' '+edge_name,
                (35.32,y,s*.25),(35.32,y,s*6.40),.10,.14,carbon,(1,0,0))
        # The protective diagonal joins the real outer frame at both ends.
        # It overlaps the center mullion/lower lintel and upper lintel solids;
        # unlike the former short bar, neither end floats inside a glass pane.
        bar('Study05 cockpit diagonal brace '+label,
            (35.62,-.61,s*.11),(35.62,2.72,s*6.48),.075,.07,metal,(1,0,0))
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
            'brow_armor':{'panels':['Bridge central armored brow',
                          'Study05 bridge brow armor port','Study05 bridge brow armor starboard'],
                          'nominal_vertical_thickness_m':.32,'open_lateral_joint_m':.20,
                          'service_cover_face_m':[1.57,1.07],'service_cover_recess_m':.20},
            'inferred_details':['exterior fixed service hatches/latches','armor seams and reveal construction'],
            'not_implemented':['fitted crew interior','working doors/access','structural or pressure certification'],
            'source_names_preserved':sorted(source_names & set(o.name for o in bpy.context.scene.objects))}
