#!/usr/bin/env python3
"""Source-preserving first independent-bay fit candidate; NOT registered gameplay.

Original Pioneer source is never overwritten. New geometry has explicit roots and
metre coordinates. All clearances/motions remain acceptance work, not inferred
from this static build. Official bpy5.0.1 only.
"""
import argparse, hashlib, json
from pathlib import Path
import bpy, bmesh
SOURCE_SHA = 'ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9'
PREFIX = 'pioneer.independent_bay/'
MATERIALS = {'armor':'Graphite ceramic armor','metal':'Titanium machinery',
             'dark':'Recessed carbon','green':'Emerald structural frames',
             'amber':'Safety amber','ivory':'Hull identification ivory'}

def build(source, output, manifest):
    assert bpy.app.version == (5,0,1)
    assert not output.exists() and not manifest.exists()
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    assert digest == SOURCE_SHA
    bpy.context.preferences.filepaths.use_scripts_auto_execute=False
    bpy.ops.wm.open_mainfile(filepath=str(source),load_ui=False,use_scripts=False)
    original=set(bpy.data.objects.keys()); parts=[]; bays=[]
    def mesh(name, vertices, faces, material, group=None, bevel=.025):
        name=PREFIX+name; assert name not in bpy.data.objects
        data=bpy.data.meshes.new(name+' mesh')
        data.from_pydata([(x,-z,y) for x,y,z in vertices],[],faces);data.update()
        bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(data);bm.free()
        obj=bpy.data.objects.new(name,data);bpy.context.collection.objects.link(obj)
        data.materials.append(bpy.data.materials[MATERIALS[material]])
        obj['semantic_id']=name
        if group:obj['kinematic_group']=group
        if bevel:
            mod=obj.modifiers.new('Manufactured edge radius','BEVEL');mod.width=bevel;mod.segments=2
            mod=obj.modifiers.new('Weighted manufactured normals','WEIGHTED_NORMAL');mod.keep_sharp=True
        parts.append({'semantic_id':name,'group':group,'material':MATERIALS[material],
                      'bounds_body_m':[[min(p[i] for p in vertices) for i in range(3)],
                                       [max(p[i] for p in vertices) for i in range(3)]]})
        return obj
    faces=[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
    def box(name,lo,hi,mat,group=None,frame=None,bevel=.025):
        points=[(lo[0] if i==0 else hi[0],lo[1] if j==0 else hi[1],lo[2] if k==0 else hi[2])
                for i,j,k in [(0,0,0),(0,0,1),(0,1,1),(0,1,0),(1,0,0),(1,0,1),(1,1,1),(1,1,0)]]
        if frame:points=[frame(p) for p in points]
        return mesh(name,points,faces,mat,group,bevel)
    # Local cell: X forward/outward, Y up, Z span. Reference body center X=0.
    # 5m length,1.8m height,6m span fits chamber with explicit nonzero margins.
    definitions=[]
    for i,x in enumerate([-26.,-18.,-10.,-2.,6.]):
        definitions += [(5+i,'mining',(x,0.,-22.8),(0,0,-1),(0,1,0),(1,0,0)),
                        (10+i,'attack',(x,0.,22.8),(0,0,1),(0,1,0),(-1,0,0))]
    for i,z in enumerate([-14.8,-7.4,0.,7.4,14.8]):
        definitions.append((i,'recon',(-39.2,8.2,z),(-1,0,0),(0,1,0),(0,0,-1)))
    for bid,kind,center,fwd,up,right in sorted(definitions):
        def frame(p,c=center,f=fwd,u=up,r=right):
            return tuple(c[k]+f[k]*p[0]+u[k]*p[1]+r[k]*p[2] for k in range(3))
        root=f'b{bid:02d}'; groups={}
        def cell(name,lo,hi,mat,group=None):
            g=f'{root}/{group}' if group else None
            if g:groups[group]=g
            return box(root+'/'+name,lo,hi,mat,g,frame)
        # Closed inner boundary and individual portal; no base hull deletion.
        cell('inner bulkhead',(-3.3,-1.90,-3.55),(-3.15,1.45,3.55),'armor')
        cell('floor',(-3.3,-1.90,-3.55),(2.85,-1.75,3.55),'dark')
        cell('ceiling',(-3.3,1.3,-3.55),(2.85,1.45,3.55),'dark')
        for side,lo,hi in [('port',-3.55,-3.4),('starboard',3.4,3.55)]:
            cell('wall '+side,(-3.3,-1.90,lo),(2.85,1.45,hi),'armor')
            cell('portal jamb '+side,(2.65,-2.0,lo-.08),(2.88,1.55,hi),'green')
        for label,y0,y1 in [('upper',1.3,1.55),('lower',-2.0,-1.75)]:
            cell('portal '+label,(2.65,y0,-3.65),(2.88,y1,3.65),'metal')
        # Split external-pocket leaves; this is their closed reference pose.
        for label,y0,y1 in [('upper',0,1.3),('lower',-1.75,0)]:
            cell('door '+label,(2.91,y0,-3.4),(3.05,y1,3.4),'green','door/'+label)
            cell('door seal '+label,(2.88,y0,-3.36),(2.91,y1,3.36),'dark','door/'+label)
        # Contact tray/runner below real body envelope; independent short stroke.
        cell('cradle deck',(-2.65,-1.08,-3.18),(2.65,-.93,3.18),'metal','cradle')
        for z in [-2.9,2.9]:
            cell(f'runner {z}',(-2.9,-1.27,z-.08),(2.85,-1.12,z+.08),'metal')
        # Mining has four wing shoes; Recon/Attack use their actual common
        # central keel flat underside, not an invented Mining shoe interface.
        pad_z = [-2.5,2.5] if kind == 'mining' else [-.3,.3]
        pad_half = .35 if kind == 'mining' else .15
        for px in [-1., .4]:
            for pz in pad_z:
                cell(f'contact shoe {px} {pz}',(px-.25,-.93,pz-pad_half),(px+.25,-.90,pz+pad_half),'dark','cradle')
        for side,sign in [('port',-1),('starboard',1)]:
            for px in [-1.,.4]:
                contact_z = 2.86 if kind == 'mining' else .65
                z0,z1=sorted((sign*contact_z,sign*(contact_z+.14)))
                cell(f'pad clamp {side} {px}',(px-.28,-.93,z0),(px+.28,-.78,z1),'amber','clamp/'+side)
        # Four nested open-top support stages under the cradle; this authored
        # support stays separate from the quick-service receiver arm.
        for side,z in [('port',-2.5),('starboard',2.5)]:
            for label,lo,hi in [
                ('base',(-3.1,-1.465,z-.165),(.8,-1.453,z+.165)),
                ('left',(-3.1,-1.453,z-.165),(.8,-1.135,z-.153)),
                ('right',(-3.1,-1.453,z+.153),(.8,-1.135,z+.165)),
                ('anchor',(-3.1,-1.75,z-.165),(-2.7,-1.465,z+.165))]:
                box(root+'/fixed support '+side+' '+label,lo,hi,'metal',None,frame,bevel=0)
        for stage in range(4):
            half=.14-stage*.025; wall=.012
            for side,z in [('port',-2.5),('starboard',2.5)]:
                group=f'cradle/stage/{stage+1}'
                for label,lo,hi in [
                    ('bottom',(-2.9,-1.30-half,z-half),(.6,-1.30-half+wall,z+half)),
                    ('left',(-2.9,-1.30-half+wall,z-half),(.6,-1.30+half-wall,z-half+wall)),
                    ('right',(-2.9,-1.30-half+wall,z+half-wall),(.6,-1.30+half-wall,z+half))]:
                    box(root+f'/support {stage} {side} {label}',lo,hi,'metal',root+'/'+group,frame,bevel=0)
                    groups[group]=root+'/'+group
        for px in [-1.,.4]:
            for pz in [-2.5,2.5]:
                box(root+f'/terminal support cap {px} {pz}',(px-.24,-1.247,pz-.065),(px+.24,-1.235,pz+.065),'metal',root+'/cradle/stage/4',frame,bevel=0)
                cell(f'cradle riser {px} {pz}',(px-.22,-1.235,pz-.055),(px+.22,-1.08,pz+.055),'metal','cradle')
        if kind == 'mining':
            # Quick-service telescopic arm parks outside the padded wing envelope.
            # It reaches laterally BEHIND an exterior drone's tail, then inserts
            # only 0.08m into the existing socket. No long rear stub through body.
            cell('receiver mounting bracket',(2.78,-.3,3.4),(3.05,.3,3.95),'green')
            for label,x0,x1,y0,y1 in [('bottom',3.05,3.35,-.15,-.13),('top',3.05,3.35,.13,.15),('left',3.05,3.07,-.13,.13),('right',3.33,3.35,-.13,.13)]:
                box(root+'/receiver fixed collar '+label,(x0,y0,3.9),(x1,y1,4.7),'metal',None,frame,bevel=0)
            for stage in range(5):
                half=.12-stage*.02; wall=.008; group=f'receiver/stage/{stage+1}'
                for label,lo,hi in [
                    ('bottom',(3.2-half,-half,3.55),(3.2+half,-half+wall,4.70)),
                    ('top',(3.2-half,half-wall,3.55),(3.2+half,half,4.70)),
                    ('left',(3.2-half,-half+wall,3.55),(3.2-half+wall,half-wall,4.70)),
                    ('right',(3.2+half-wall,-half+wall,3.55),(3.2+half,half-wall,4.70))]:
                    lo=(lo[0],lo[1],3.9)
                    box(root+f'/receiver {stage} {label}',lo,hi,'metal',root+'/'+group,frame,bevel=0)
                    groups[group]=root+'/'+group
            # Same pocketed0.44m head/0.50m socket contract as receiver-v1,
            # rotated to local+X. Latches occupy actual absent side pockets.
            for side,y0,y1 in [('bottom',-.22,-.105),('top',.105,.22)]:
                cell('receiver head '+side,(3.32,y0,3.43),(3.40,y1,3.87),'metal','receiver/head')
            for side,sign in [('port',-1),('starboard',1)]:
                za,zb=sorted((3.65+sign*.105,3.65+sign*.22))
                for label,x0,x1,y0,y1 in [('front',3.38,3.40,-.105,.105),('back',3.32,3.34,-.105,.105),('lower',3.34,3.38,-.105,-.08),('upper',3.34,3.38,.08,.105)]:
                    box(root+'/receiver head '+side+' '+label,(x0,y0,za),(x1,y1,zb),'metal',root+'/receiver/head',frame,bevel=.001)
                za,zb=sorted((3.65+sign*.12,3.65+sign*.22))
                box(root+'/receiver latch '+side,(3.34,-.08,za),(3.38,.08,zb),'amber',root+'/receiver/latch/'+side,frame,bevel=.001)
                groups['receiver/latch/'+side]=root+'/receiver/latch/'+side
            box(root+'/receiver guide attachment',(3.155,-.045,3.725),(3.245,.045,3.90),'metal',root+'/receiver/stage/5',frame,bevel=0)
            # Small axial guide retains overlap during the0.08m insertion stroke.
            for label,y0,y1,z0,z1 in [('bottom',-.075,-.05,-.075,.075),('top',.05,.075,-.075,.075),('left',-.05,.05,-.075,-.05),('right',-.05,.05,.05,.075)]:
                box(root+'/receiver axial guide '+label,(3.12,y0,3.65+z0),(3.28,y1,3.65+z1),'metal',root+'/receiver/stage/5',frame,bevel=0)
            box(root+'/receiver insertion stem',(3.10,-.04,3.61),(3.32,.04,3.69),'metal',root+'/receiver/head',frame,bevel=0)
            box(root+'/receiver backed head plate',(3.30,-.14,3.51),(3.32,.14,3.79),'metal',root+'/receiver/head',frame,bevel=0)
        bays.append({'bay_id':bid,'class':kind,'center_m':center,'forward':fwd,'up':up,'right':right,
                     'chamber_local_m':[[-3.15,-1.75,-3.4],[2.9,1.3,3.4]],
                     'ore_service_supported':kind == 'mining','groups':groups,'status':'fit candidate; no accepted motion/contact'})
    # Continuous bank spines bridge cell partitions, not15 disconnected boxes.
    for side,s in [('port',-1),('starboard',1)]:
        lo,hi=sorted((s*19.5,s*25.65))
        box(f'{side}/upper continuous casing',(-30,1.45,lo),(10,1.72,hi),'green')
        box(f'{side}/lower continuous casing',(-30,-2.05,lo),(10,-1.90,hi),'armor')
        # Broad sloped structural shoulders connect the working bank to hull.
        for top in [True,False]:
            a,b=(1.72,4.2) if top else (-2.05,-5.6)
            z0,z1=s*25.65,s*18.25
            verts=[(x,y,z) for x in [-30,10] for y,z in [(a,z0),(b,z1),(b+(.18 if top else -.18),z1),(a+(.18 if top else -.18),z0)]]
            mesh(f'{side}/'+('upper shoulder' if top else 'lower shoulder'),verts,faces,'green' if top else 'armor')
        for x in [-30,10]:
            box(f'{side}/end structural wall {x}',(x-.16,-2.05,lo),(x+.16,1.72,hi),'armor')
    box('aft/continuous roof',(-42.05,9.65,-18.5),(-35.85,9.9,18.5),'green')
    box('aft/continuous floor',(-42.05,6.15,-18.5),(-35.85,6.30,18.5),'armor')
    # New fixed housing supports: geometry only, not a structural strength claim.
    # Tapered aft load-transfer casing joins the bank to the existing rear
    # upper hull. It is a real volume, not a thin floating cross-ship shelf.
    verts=[(x,y,z) for z in [-18.5,18.5] for x,y in
           [(-35.85,6.30),(-35.85,9.9),(-28.0,6.8),(-28.0,5.9)]]
    mesh('aft/integrated sloped back casing',verts,faces,'green')
    for z in [-9.,0.,9.]:
        box(f'aft/support {z}',(-39.3,6.10,z-.24),(-33.2,6.30,z+.24),'metal')
    assert original.issubset(set(bpy.data.objects.keys()))
    bpy.ops.wm.save_as_mainfile(filepath=str(output),check_existing=False)
    manifest.write_text(json.dumps({'status':'unregistered source fit candidate','source_sha256':digest,
        'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'base_parts_preserved':len(original),
        'coordinates':'body metres +Xforward +Yup +Zstarboard','bays':bays,'parts':parts,
        'pending':['full static and moving clearance','socket contact','engine/exhaust/crew/station checks',
                   'native visual review','authority registration and lifecycle']},indent=2))
    assert hashlib.sha256(source.read_bytes()).hexdigest()==digest

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('output',type=Path);p.add_argument('manifest',type=Path)
    a=p.parse_args();build(a.source.resolve(),a.output.resolve(),a.manifest.resolve())
