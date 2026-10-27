#!/usr/bin/env python3
"""Author revision11 raised bridge from an explicit preserved Study05 master.

Run with official bpy5.0.1. Body coordinates are X forward/Y up/Z starboard.
This changes static geometry only, not engineering ratings or system simulation.
The output must be new; the input source is never overwritten.
"""
import argparse, json, hashlib
from pathlib import Path
import bpy, bmesh
from mathutils import Vector


def build(source, output):
    assert bpy.app.version == (5,0,1)
    assert not output.exists()
    before=hashlib.sha256(source.read_bytes()).hexdigest()
    assert before == '61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9', 'explicit preserved Study05 master required'
    bpy.context.preferences.filepaths.use_scripts_auto_execute=False
    bpy.ops.wm.open_mainfile(filepath=str(source),load_ui=False,use_scripts=False)
    mats=bpy.data.materials
    armor='Graphite ceramic armor';metal='Titanium machinery';carbon='Recessed carbon'
    green='Emerald structural frames';glass='Navigation glass'
    created=[];removed=[];changed=[]
    def cv(p):return (p[0],-p[2],p[1])
    def mesh(name,verts,faces,mat,bevel=0):
        data=bpy.data.meshes.new(name+' editable surface');data.from_pydata([cv(p) for p in verts],[],faces);data.update()
        bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(data);bm.free()
        o=bpy.data.objects.new(name,data);bpy.context.collection.objects.link(o);data.materials.append(mats[mat]);created.append(name)
        if bevel:
            m=o.modifiers.new('Manufactured edge radius','BEVEL');m.width=bevel;m.segments=2
            m=o.modifiers.new('Weighted manufactured normals','WEIGHTED_NORMAL');m.keep_sharp=True
        return o
    def box(name,lo,hi,mat,bevel=.025):
        verts=[(lo[0] if i==0 else hi[0],lo[1] if j==0 else hi[1],lo[2] if k==0 else hi[2]) for i,j,k in [(0,0,0),(0,0,1),(0,1,1),(0,1,0),(1,0,0),(1,0,1),(1,1,1),(1,1,0)]]
        return mesh(name,verts,[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],mat,bevel)
    def prism(name,poly,normal,depth,mat,bevel=0):
        n=Vector(normal).normalized();p=[Vector(v) for v in poly]
        if (p[1]-p[0]).cross(p[2]-p[0]).dot(n)<0:p.reverse()
        N=len(p);v=[tuple(q) for q in p]+[tuple(q-n*depth) for q in p]
        return mesh(name,v,[tuple(range(N)),tuple(reversed(range(N,2*N)))]+[(i,(i+1)%N,(i+1)%N+N,i+N) for i in range(N)],mat,bevel)
    def cutbox(target,name,lo,hi):
        c=box(name,lo,hi,carbon,0); cname=c.name
        bpy.context.view_layer.objects.active=target;m=target.modifiers.new('Actual registered opening','BOOLEAN');m.operation='DIFFERENCE';m.solver='EXACT';m.object=c
        while target.modifiers.find(m.name)>0:bpy.ops.object.modifier_move_up(modifier=m.name)
        bpy.ops.object.modifier_apply(modifier=m.name);bpy.data.objects.remove(c,do_unlink=True);created.remove(cname)
    # Retire the low nose bridge. Its former glass becomes solid service-shell
    # closure; no ghost glazing, duplicate pilot station or floating bridge.
    for o in list(bpy.context.scene.objects):
        n=o.name
        if n.startswith(('Bridge front glazing','Bridge frontal corner glass','Bridge chamfer corner glass','Bridge wrap glazing')):
            # Seal the whole old cavity, not just the inset glass rectangle.
            # The old sill/reveal frames are retired below, so these opaque
            # closures overlap the cut recess at Y[-.82,2.88] on both edges.
            for v in o.data.vertices:
                q=o.matrix_world@v.co
                q.z=-.90 if q.z<1.0 else 3.05
                v.co=o.matrix_world.inverted()@q
            o.data.update()
            o.data.materials.clear();o.data.materials.append(mats[armor]);o.name=n.replace('Bridge','Forward service closure').replace('glazing','panel').replace('glass','panel');changed.append(o.name)
        elif n.startswith(('Bridge corner lower frame','Bridge corner upper frame','Bridge wrap frame','Bridge front brace','Bridge center mullion','Bridge panoramic frame','Study05 cockpit diagonal brace','Study05 bridge front gasket','Study05 bridge recessed')):
            removed.append(n);bpy.data.objects.remove(o,do_unlink=True)
    # Close the old center/seam gaps with modest solid longitudinal ribs.
    box('Forward service shell center seam',(35.14,-.90,-.27),(35.25,3.05,.27),armor)
    for s,label in [(-1,'port'),(1,'starboard')]:
        for z in [6.47,7.77]:box('Forward service seam '+label+str(z),(35.14,-.90,s*z-.12),(35.25,3.05,s*z+.12),armor)
    for sign,label in [(-1,'port'),(1,'starboard')]:
        # Close the chamfer-to-wrap construction joint and the deep aft return
        # to the original hull. The port doorway is cut separately after this.
        prism('Forward service chamfer return '+label,[(34.48,-.90,sign*8.62),(34.29,-.90,sign*8.81),(34.29,3.05,sign*8.81),(34.48,3.05,sign*8.62)],(.71,0,sign*.71),.10,armor,.01)
        prism('Forward service aft return '+label,[(31.67,-.90,sign*10.82),(31.42,-.90,sign*12.40),(31.42,3.05,sign*12.40),(31.67,3.05,sign*10.82)],(-.99,0,sign*.16),.10,armor,.01)
    hull=bpy.data.objects['Pressure hull']
    # Bake the exact prior rear-door/recess openings formerly derived at runtime.
    cutbox(hull,'temporary lower rear aperture',(33.549,-.441,-6.881),(34.901,2.881,5.881))
    cutbox(hull,'temporary lower threshold recess',(33.549,-.441,-.901),(33.751,-.319,.901))
    # Hollow the registered two-deck volume and connected lower forward service
    # passage first. Overlapping cuts form one open interior: no artificial
    # Boolean cap at the stairs' lower entry or below an ascending avatar.
    cutbox(hull,'temporary two deck interior',(-12.05,-4.24,-7.05),(20.02,2.98,7.05))
    cutbox(hull,'temporary lower forward interior',(19.80,-.44,-8.50),(34.95,2.98,2.85))
    # Open the top shell for the three-flight enclosed access tower, preserving
    # the lower center connector and the pressure hull outside this exact bay.
    cutbox(hull,'temporary tower aperture',(19.90,-.44,-2.18),(27.12,11.60,2.18))
    # The original port boarding aperture is cut into the new opaque closure.
    p=Vector((32.985,0,-9.815));t=Vector((.7903075524,0,.6127103496));n=Vector((.6127103496,0,-.7903075524))
    verts=[tuple(p+t*x+Vector((0,y,0))+n*z) for x,y,z in [(-.72,-.45,-.85),(-.72,-.45,.85),(-.72,2.10,.85),(-.72,2.10,-.85),(.72,-.45,-.85),(.72,-.45,.85),(.72,2.10,.85),(.72,2.10,-.85)]]
    target=bpy.data.objects['Forward service closure wrap panel port']
    c=mesh('temporary port access',verts,[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],carbon)
    cname=c.name;bpy.context.view_layer.objects.active=target;m=target.modifiers.new('Port boarding actual aperture','BOOLEAN');m.operation='DIFFERENCE';m.solver='EXACT';m.object=c;bpy.ops.object.modifier_apply(modifier=m.name);bpy.data.objects.remove(c,do_unlink=True);created.remove(cname)
    # Remove old crown equipment only where it lies in the tower/bridge footprint.
    for name in ['Bridge central armored brow','Study05 bridge brow service coaming','Study05 bridge brow service cover','Study05 bridge brow service latch']:
        o=bpy.data.objects.get(name)
        if o:
            if name=='Bridge central armored brow':cutbox(o,'temporary brow tower clearance',(19.9,-.44,-2.18),(27.12,11.6,2.18))
            else:removed.append(o.name);bpy.data.objects.remove(o,do_unlink=True)
    # Enclosed support tower. The front is shared with the actual cabin rear
    # entry; lower sides sit within the original hull, upper sides are visible.
    for s,label in [(-1,'port'),(1,'starboard')]:
        z=s*2.25
        box('Raised bridge access tower '+label,(20.0,2.0,z-.07),(27.0,11.10,z+.07),armor)
        for x in [20.2,23.4,26.7]:box('Raised tower structural rib '+label+str(x),(x-.06,6.6,z-.10),(x+.06,11.12,z+.10),green,.02)
    box('Raised bridge access tower aft',(19.92,6.7,-2.32),(20.08,11.10,2.32),armor)
    box('Raised bridge access tower roof',(19.92,11.10,-2.38),(27.08,11.35,2.38),armor,.03)
    # Taper the aft tower into the existing service spine rather than leaving
    # an abrupt vertical cabin wall above the machinery deck.
    prism('Raised bridge aft sloped armor',[(16.5,8.20,-2.38),(19.92,11.35,-2.38),(19.92,11.35,2.38),(16.5,8.20,2.38)],(-.68,.73,0),.16,armor,.03)
    for s,label in [(-1,'port'),(1,'starboard')]:
        prism('Raised bridge aft shoulder '+label,[(16.5,7.9,s*2.38),(19.92,7.9,s*2.38),(19.92,11.35,s*2.38),(16.5,8.2,s*2.38)],(0,0,s),.14,green,.035)

    # Solid but hollow deckhouse shoulders tie the raised cabin down into the
    # tapered forebody. They never fill the inhabited cabin or stair volume.
    for s,label in [(-1,'port'),(1,'starboard')]:
        poly=[(26.8,6.55,s*3.30),(35.86,3.70,s*3.30),(35.86,7.68,s*3.00),(26.8,7.68,s*3.00)]
        prism('Raised bridge structural shoulder '+label,poly,(0,0,s),.18,armor,.04)
        box('Raised bridge deck coaming '+label,(26.8,7.48,s*2.94-.09),(35.86,7.68,s*2.94+.09),green,.02)
        # Side skin has a real rectangular viewing aperture, not glass on armor.
        side=box('Raised bridge side shell '+label,(27.0,7.68,s*2.90-.06),(35.72,11.10,s*2.90+.06),armor,.025)
        cutbox(side,'temporary side window '+label,(28.15,9.05,s*2.9-.3),(33.85,10.68,s*2.9+.3))
        prism('Bridge wrap glazing '+label,[(28.20,9.10,s*2.92),(33.80,9.10,s*2.92),(33.80,10.63,s*2.92),(28.20,10.63,s*2.92)],(0,0,s),.035,glass)
        for y in [9.07,10.66]:box('Raised bridge side window transom '+label+str(y),(28.12,y-.04,s*2.94-.045),(33.88,y+.04,s*2.94+.045),metal,.015)
        for x in [28.16,33.84]:box('Raised bridge side window post '+label+str(x),(x-.04,9.05,s*2.94-.045),(x+.04,10.68,s*2.94+.045),metal,.015)
    # Low faceted crown and a tied-in nose apron avoid a floating box on a hull.
    prism('Raised bridge roof',[(26.8,11.35,-2.90),(26.95,11.35,-3.05),(35.70,11.23,-3.05),(35.86,11.22,-2.90),(35.86,11.22,2.90),(35.70,11.23,3.05),(26.95,11.35,3.05),(26.8,11.35,2.90)],(0,1,0),.12,armor,.025)
    prism('Raised bridge nose support apron',[(35.86,3.70,-3.30),(35.86,3.70,3.30),(35.72,7.68,3.00),(35.72,7.68,-3.00)],(1,0,0),.18,armor,.04)
    for s,label in [(-1,'port'),(1,'starboard')]:
        for x in [27.18,34.85]:
            box('Raised bridge frame attachment '+label+str(x),(x-.06,7.65,s*2.96-.045),(x+.06,11.10,s*2.96+.045),green,.02)

    box('Raised bridge front lower shell',(35.62,7.68,-2.96),(35.84,8.44,2.96),armor,.025)
    box('Raised bridge front upper shell',(35.62,10.60,-2.96),(35.84,11.10,2.96),armor,.025)
    for s,label in [(-1,'port'),(1,'starboard')]:
        z=s*2.70
        box('Raised bridge front pillar '+label,(35.62,8.40,z-.20),(35.84,10.64,z+.20),metal,.02)
    for z0,z1,label in [(-2.50,0.0,'port'),(0.0,2.50,'starboard')]:
        prism('Bridge front glazing '+label,[(35.72,8.52,z0),(35.72,8.52,z1),(35.72,10.60,z1),(35.72,10.60,z0)],(1,0,0),.035,glass)
    for y in [8.48,10.62]:box('Raised bridge front transom '+str(y),(35.66,y-.04,-2.52),(35.84,y+.04,2.52),metal,.015)
    # Retain actual socket hardware and move it with supported side platforms.
    for o in list(bpy.context.scene.objects):
        if o.name.startswith('Turret mounting socket'):
            pts=[o.matrix_world@Vector(v) for v in o.bound_box];z=-sum(p.y for p in pts)/8;s=1 if z>0 else -1
            o.location.y-=s*(5.2-2.66);changed.append(o.name)
    for s,label in [(-1,'port'),(1,'starboard')]:
        box('Outboard turret supported deck '+label,(8.0,8.50,min(s*3.35,s*6.60)),(13.6,9.0,max(s*3.35,s*6.60)),green,.08)
        for x in [8.3,13.3]:
            prism('Outboard turret load web '+label+str(x),[(x,7.4,s*3.4),(x,8.5,s*6.5),(x,8.5,s*3.4)],(1,0,0),.18,metal,.02)
    bpy.context.view_layer.update()
    output.parent.mkdir(parents=True,exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(output),compress=True)
    assert hashlib.sha256(source.read_bytes()).hexdigest()==before
    report={'source_sha256':before,'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'bridge_offset_body_m':[0,8,0],'expected_bounds_body_m':[[-36,-9,-19],[36,11.35,19]],'turret_mounts_body_m':[[10.8,9.864,-5.2],[10.8,9.864,5.2]],'created':created,'removed':removed,'changed':changed,'physics':'static geometry; no pressure, strength, mass, motor or power simulation'}
    output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();build(a.source.resolve(),a.output.resolve())
