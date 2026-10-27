#!/usr/bin/env python3
"""Source-preserving ten-ready/twenty-total magazine fit candidate; NOT registered gameplay.

Original Pioneer source is never overwritten. New geometry has explicit roots and
metre coordinates. All clearances/motions remain acceptance work, not inferred
from this static build. Official bpy5.0.1 only.
"""
import argparse, hashlib, json
from pathlib import Path
import bpy, bmesh
SOURCE_SHA = 'ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9'
PREFIX = 'pioneer.magazine_ready/'
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
            mod=obj.modifiers.new('Manufactured edge radius','BEVEL');mod.width=bevel;mod.segments=1
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
        definitions += [(i,'mining',(x,0.,-22.8),(0,0,-1),(0,1,0),(1,0,0)),
                        (5+i,'mining',(x,0.,22.8),(0,0,1),(0,1,0),(-1,0,0))]
    for bid,kind,center,fwd,up,right in sorted(definitions):
        def frame(p,c=center,f=fwd,u=up,r=right):
            return tuple(c[k]+f[k]*p[0]+u[k]*p[1]+r[k]*p[2] for k in range(3))
        root=f'b{bid:02d}'; groups={}
        def cell(name,lo,hi,mat,group=None):
            g=f'{root}/{group}' if group else None
            if g:groups[group]=g
            return box(root+'/'+name,lo,hi,mat,g,frame,bevel=0 if (group and group.startswith(('cradle','clamp','cassette'))) or name.startswith('runner') else .025)
        # Closed inner boundary and individual portal; no base hull deletion.
        cell('inner bulkhead',(-3.3,-1.90,-3.55),(-3.15,2.70,3.55),'armor')
        cell('floor',(-3.3,-1.90,-3.55),(2.85,-1.75,3.55),'dark')
        # Transfer shaft is enclosed by the magazine hatch, not a solid ceiling.
        cell('upper portal pressure wall',(3.08,1.55,-3.55),(3.22,2.70,3.55),'armor')
        cell('transfer hatch',(-3.3,2.70,-3.55),(3.05,2.80,3.55),'metal','magazine/hatch')
        cell('transfer hatch sealing face',(-3.35,2.80,-3.60),(3.16,2.85,3.60),'dark','magazine/hatch')
        for side,lo,hi in [('port',-3.55,-3.4),('starboard',3.4,3.55)]:
            cell('wall '+side,(-3.3,-1.90,lo),(2.85,2.70,hi),'armor')
            cell('portal jamb '+side,(2.65,-2.0,lo-.08 if side=='port' else lo),(2.88,1.55,hi if side=='port' else hi+.08),'green')
        for label,y0,y1 in [('upper',1.3,1.55),('lower',-2.0,-1.75)]:
            cell('portal '+label,(3.08 if label=='upper' else 2.65,y0,-3.65),(3.22 if label=='upper' else 2.88,y1,3.65),'metal')
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
        pad_z = [-.3,.3]  # Proven shared keel underside across all three classes
        pad_half = .15
        for px in [-1., .4]:
            for pz in pad_z:
                cell(f'contact shoe {px} {pz}',(px-.25,-.93,pz-pad_half),(px+.25,-.90,pz+pad_half),'dark','cradle')
        for side,sign in [('port',-1),('starboard',1)]:
            for px in [-1.,.4]:
                contact_z = .65  # Proven shared keel side band
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
        # Fixed under-frame ties the lifting posts to the stationary support
        # anchors, below every telescoping stage (never weld to moving deck).
        cell('cassette lift rear crossmember',(-2.795,-1.56,-3.295),(-2.705,-1.48,3.295),'metal','cassette/base')
        for fz in [-3.25,3.25]:
            cell(f'cassette lift longitudinal beam {fz}',(-2.795,-1.56,fz-.045),(2.795,-1.48,fz+.045),'metal','cassette/base')
        for fx in [-2.75,2.75]:
            for fz in [-3.25,3.25]:
                cell(f'cassette lift post {fx} {fz}',(fx-.045,-1.75,fz-.045),(fx+.045,1.00,fz+.045),'metal','cassette/base')
                cell(f'cassette foot collar {fx} {fz}',(fx-.11,-1.60,fz-.11),(fx+.11,-1.57,fz+.11),'metal','cassette/base')
                cell(f'cassette lift fork base {fx} {fz}',(fx-.11,.92,fz-.11),(fx+.11,.95,fz+.11),'metal','cassette/base')
                # Two real fork cheeks support the ring on opposite X edges,
                # leaving the complete pin/lateral latch corridor open.
                for dx in [-.085,.085]:
                    cell(f'cassette lift fork cheek {fx} {fz} {dx}',(fx+dx-.02,.95,fz-.11),(fx+dx+.02,1.10,fz+.11),'metal','cassette/base')
                # Four boxes form a real open lifting eye, not a solid cap.
                for tag,x0,x1,z0,z1 in [('left',-.11,-.06,-.11,.11),('right',.06,.11,-.11,.11),('front',-.06,.06,-.11,-.06),('back',-.06,.06,.06,.11)]:
                    cell(f'cassette lift eye {fx} {fz} {tag}',(fx+x0,1.10,fz+z0),(fx+x1,1.25,fz+z1),'amber','cassette/base')
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
        bays.append({'bay_id':bid,'reference_contact_class':kind,'compatible_classes_target':['mining','attack','recon'],'center_m':center,'forward':fwd,'up':up,'right':right,
                     'chamber_local_m':[[-3.15,-1.75,-3.4],[2.9,1.3,3.4]],
                     'ore_service_supported':True,'groups':groups,'status':'fit candidate; no accepted motion/contact'})
    # Continuous bank spines bridge cell partitions, not15 disconnected boxes.
    for side,s in [('port',-1),('starboard',1)]:
        lo,hi=sorted((s*19.5,s*25.65))
        # Open transfer shafts replace the former solid upper casing.
        box(f'{side}/lower continuous casing',(-30,-2.05,lo),(10,-1.90,hi),'armor')
        # Broad sloped structural shoulders connect the working bank to hull.
        for top in [False]:
            a,b=(1.72,4.2) if top else (-2.05,-5.6)
            z0,z1=s*25.65,s*18.25
            verts=[(x,y,z) for x in [-30,10] for y,z in [(a,z0),(b,z1),(b+(.18 if top else -.18),z1),(a+(.18 if top else -.18),z0)]]
            mesh(f'{side}/'+('upper shoulder' if top else 'lower shoulder'),verts,faces,'green' if top else 'armor')
        for x in [-30,10]:
            box(f'{side}/end structural wall {x}',(x-.16,-2.05,lo),(x+.16,1.72,hi),'armor')
    # Two physically separate protected magazines. Each has five parking
    # stations and a shared aisle above its five ready berths. This is a fit
    # source, NOT capacity admission or a functioning loader claim.
    magazine_slots=[]
    for side,sign in [('port',-1),('starboard',1)]:
        def bank_box(name,lo,hi,mat,group=None):
            a=list(lo);b=list(hi)
            a[2],b[2]=sorted((sign*lo[2],sign*hi[2]))
            return box(side+'/magazine/'+name,tuple(a),tuple(b),mat,group,bevel=0 if group or 'support' in name else .015)
        # Aisle is Z19.5..26.1; parking row is Z26.1..33.6.
        bank_box('parking deck',(-31.2,2.85,26.10),(18,3.0,33.6),'metal')
        for x0,x1 in [(-31.2,-29.55),(-22.45,-21.55),(-14.45,-13.55),(-6.45,-5.55),(1.55,2.45),(9.55,10.2)]:
            bank_box(f'fixed aisle floor {x0}',(x0,2.85,19.35),(x1,3.0,26.1),'metal')
        bank_box('fixed aperture front floor',(-31.2,2.85,25.88),(10.2,3.0,26.1),'metal')
        bank_box('inner pressure wall',(-31.2,2.85,19.35),(18,8.75,19.5),'armor')
        bank_box('outer pressure wall',(-31.2,2.85,33.6),(18,8.75,33.8),'green')
        bank_box('sealed roof skin',(-31.2,8.75,19.35),(18,8.83,33.8),'armor')
        for px in [-30,-22,-14,-6,2,10]:
            bank_box('roof panel '+str(px),(px+.04,8.83,19.35),(px+7.96,8.98,33.8),'green')
        for x in [-31.2,18]:
            bank_box('end pressure wall '+str(x),(x-.16,2.85,19.35),(x+.16,8.75,33.8),'armor')
        for x in [-31.2,-22,-14,-6,2,10,18]:
            bank_box('underdeck crossmember '+str(x),(x-.12,2.65,19.5),(x+.12,2.85,33.6),'metal')
            bank_box('support pier '+str(x),(x-.12,-2.05,25.25),(x+.12,2.65,25.65),'metal')
        # Closed internal hatch pockets lie below the parking deck. Vertical
        # supports are restricted to the ends, never through the shared aisle.
        for z in [19.65,33.3]:
            bank_box('longitudinal rail '+str(z),(-31.2,7.8,z-.09),(17.8,8.0,z+.09),'metal')
        for i,x in enumerate([-26.,-18.,-10.,-2.,6.]):
            slot=f'{side}/reserve/{i}'
            magazine_slots.append({'slot_id':slot,'side':side,'body_center_m':[x,4.8,sign*30.0],
                'capacity':1,'compatible_classes':['mining','attack','recon'],
                'status':'support adapter and transfer path awaiting proof'})
            # Reserve cassettes rest on exact lower-anchor docking rails.
            for dx in [-3.25,3.25]:
                for dz in [-2.75,2.75]:
                    bank_box(f'{slot}/dock support {dx} {dz}',(x+dx-.12,3.0,30+dz-.12),(x+dx+.12,3.05,30+dz+.12),'metal')
        bank_box('exchange buffer deck',(10.2,2.85,19.5),(18,3.0,26.1),'metal')
        for dx in [-3.25,3.25]:
            for dz in [-2.75,2.75]:
                bank_box(f'exchange buffer support {dx} {dz}',(14+dx-.12,3.0,22.8+dz-.12),(14+dx+.12,3.05,22.8+dz+.12),'metal')
        # One source-owned bridge/trolley/hoist per side, parked empty above
        # the first aisle station. Moving geometry is explicitly identified.
        gx=14.0; gz=22.8
        bridge=side+'/loader/bridge'; trolley=side+'/loader/trolley'
        bank_box('moving bridge',(gx-.16,8.15,19.65),(gx+.16,8.4,33.3),'metal',bridge)
        bank_box('moving trolley base',(gx-.35,8.0,gz-.30),(gx+.35,8.10,gz+.30),'metal',trolley)
        for dx in [-.30,.30]:
            bank_box(f'wrapped trolley side {dx}',(gx+dx-.05,8.10,gz-.30),(gx+dx+.05,8.50,gz+.30),'metal',trolley)
        bank_box('wrapped trolley top',(gx-.35,8.50,gz-.30),(gx+.35,8.60,gz+.30),'metal',trolley)
        for dz in [-.20,.20]:
            bank_box(f'trolley bearing pad {dz}',(gx-.10,8.40,gz+dz-.06),(gx+.10,8.50,gz+dz+.06),'dark',trolley)
        for z in [19.65,33.3]:
            bank_box(f'bridge bearing pad {z}',(gx-.12,8.0,z-.08),(gx+.12,8.15,z+.08),'dark',bridge)
            for x in [-31.2,-22,-14,-6,2,10,18]:
                a,b=(19.35,19.74) if z<25 else (33.21,33.8)
                bank_box(f'rail wall support {x}/{z}',(x-.12,7.55,a),(x+.12,7.80,b),'metal')
        # The hoist handles an authored cassette eye, never a loose drone.
        # All telescopic stages have real hollow walls and overlap at full reach.
        for dx in [-3.25,3.25]:
            bank_box(f'spreader side {dx}',(gx+dx-.09,7.85,gz-2.60),(gx+dx+.09,8.05,gz+2.60),'metal',trolley)
        for dz in [-2.75,2.75]:
            bank_box(f'spreader cross {dz}',(gx-3.10,7.85,gz+dz-.09),(gx+3.10,8.05,gz+dz+.09),'metal',trolley)
        bank_box('spreader central X beam',(gx-3.25,7.95,gz-.08),(gx+3.25,8.10,gz+.08),'metal',trolley)
        bank_box('spreader central Z beam',(gx-.08,7.95,gz-2.75),(gx+.08,8.10,gz+2.75),'metal',trolley)
        for dx in [-3.25,3.25]:
            for dz in [-2.75,2.75]:
                corner=f'{dx}/{dz}'; cx=gx+dx;cz=gz+dz
                for stage,half in enumerate([.15,.12,.10,.08,.06]):
                    group=trolley if stage==0 else side+f'/loader/hoist/{stage}'
                    wall=.008
                    for tag,x0,x1,z0,z1 in [('left',-half,-half+wall,-half,half),('right',half-wall,half,-half,half),('front',-half+wall,half-wall,-half,-half+wall),('back',-half+wall,half-wall,half-wall,half)]:
                        bank_box(f'hoist {corner}/{stage}/{tag}',(cx+x0,6.4,cz+z0),(cx+x1,8.4,cz+z1),'metal',group)
                terminal=side+'/loader/hoist/4'
                bank_box(f'terminal pin cap {corner}',(cx-.06,6.40,cz-.06),(cx+.06,6.45,cz+.06),'metal',terminal)
                bank_box(f'eye pin {corner}',(cx-.045,6.22,cz-.045),(cx+.045,6.45,cz+.045),'metal',terminal)
                for tag,y0,y1 in [('lower',6.15,6.17),('upper',6.20,6.22)]:
                    bank_box(f'eye latch guide {corner}/{tag}',(cx-.055,y0,cz-.045),(cx+.055,y1,cz+.045),'metal',terminal)
                bank_box(f'eye latch spine {corner}',(cx-.005,6.17,cz-.045),(cx+.005,6.20,cz+.045),'metal',terminal)
                # Shoes deploy below the eye's underside. Exact travel/contact
                # is proven with the shared route, not assumed by a timer.
                for direction in [-1,1]:
                    x0,x1=sorted((cx+direction*.005,cx+direction*.045))
                    bank_box(f'eye latch {corner}/{direction}',(x0,6.17,cz-.04),(x1,6.20,cz+.04),'amber',side+f'/loader/eye-latch/{direction}')
        # Deep inter-cell webs connect the overhanging magazine to the lower
        # working hull while leaving full launch tunnels. A real service slot
        # clears the existing mouth-arm roots through each separating web.
        for x in [-30,-22,-14,-6,2,10]:
            for tag,y0,y1,z0,z1 in [('lower',-2.05,-.5,25.65,33.6),('upper',.5,2.65,25.65,33.6),('outer',-.5,.5,26.45,33.6)]:
                bank_box(f'load web {x}/{tag}',(x-.12,y0,z0),(x+.12,y1,z1),'armor')
        # Functional end transition volumes sit outside all five launch lanes.
        for end,near,far in [('aft',-31.2,-35.5),('forward',18.,24.)]:
            verts=[(near,y,sign*z) for y,z in [(-2.05,19.35),(8.75,19.35),(8.75,33.8),(-2.05,33.8)]]
            verts += [(far,y,sign*z) for y,z in [(-3.5,18.25),(5.8,18.25),(5.8,25.65),(-3.5,25.65)]]
            mesh(side+'/magazine/'+end+' integrated transition',verts,faces,'green',None,.02)

    # Reusable launch cassettes: the ready and reserve stock each retain one
    # actual supported/clamped cassette. A loader moves the complete cassette;
    # no unsupported drone handoff or extra inventory is implied by the buffer.
    cassette_prefixes=('cradle ', 'runner ', 'contact shoe ', 'pad clamp ', 'fixed support ', 'support ', 'terminal support cap ', 'cassette lift ', 'cassette foot ')
    cassette_records=[]
    for bay in bays:
        bid=bay['bay_id']; root=f'b{bid:02d}'; side='port' if bid<5 else 'starboard';sign=-1 if bid<5 else 1
        selected=[r for r in list(parts) if r['semantic_id'].startswith(PREFIX+root+'/') and r['semantic_id'].split(root+'/',1)[1].startswith(cassette_prefixes)]
        delta=(0.,4.8,sign*7.2)
        for record in selected:
            old_name=record['semantic_id'];obj=bpy.data.objects[old_name]
            local_name=old_name.split(root+'/',1)[1]
            ready_name=PREFIX+f'c{bid:02d}/'+local_name
            obj.name=ready_name;obj['semantic_id']=ready_name;record['semantic_id']=ready_name
            old_group=record['group']
            ready_group=(old_group.replace(root+'/',f'c{bid:02d}/') if old_group else f'c{bid:02d}/cassette/base')
            obj['kinematic_group']=ready_group;record['group']=ready_group
            duplicate=obj.copy();duplicate.data=obj.data.copy();bpy.context.collection.objects.link(duplicate)
            reserve_name=PREFIX+f'c{bid+10:02d}/'+local_name
            duplicate.name=reserve_name;duplicate['semantic_id']=reserve_name
            reserve_group=ready_group.replace(f'c{bid:02d}/',f'c{bid+10:02d}/')
            duplicate['kinematic_group']=reserve_group
            duplicate.location.x+=delta[0];duplicate.location.y-=delta[2];duplicate.location.z+=delta[1]
            parts.append({'semantic_id':reserve_name,'group':reserve_group,'material':record['material'],
                'bounds_body_m':[[record['bounds_body_m'][e][k]+delta[k] for k in range(3)] for e in range(2)]})
        cassette_records.extend([{'cassette_id':bid,'initial_location':'ready','slot_id':bid,'side':side,'body_center_m':bay['center_m']},
            {'cassette_id':bid+10,'initial_location':'protected','slot_id':bid%5,'side':side,'body_center_m':[bay['center_m'][0],4.8,sign*30.0]}])
    docks=[]
    for bay in bays:
        docks.append(('ready',bay['bay_id'],bay['center_m']))
    for record in cassette_records:
        if record['initial_location']=='protected':
            docks.append(('protected',record['cassette_id']-10,record['body_center_m']))
    docks.extend([('buffer',0,[14.,4.8,-22.8]),('buffer',1,[14.,4.8,22.8])])
    for kind,did,center in docks:
        x,y,z=center
        for dx in [-3.25,3.25]:
            direction=1 if dx>0 else -1
            for dz in [-2.75,2.75]:
                lo,hi=sorted((x+dx-direction*.095,x+dx-direction*.045))
                name=f'dock/{kind}/{did}/{dx}/{dz}'
                box(name+'/jaw',(lo,y-1.75,z+dz-.07),(hi,y-1.60,z+dz+.07),'amber',f'dock/{kind}/{did}/latch/{direction}',bevel=0)
                lo,hi=sorted((x+dx-direction*.30,x+dx-direction*.04))
                box(name+'/track',(lo,y-1.80,z+dz-.09),(hi,y-1.75,z+dz+.09),'metal',None,bevel=0)
    import math
    references={}
    cassette_by_id={c['cassette_id']:c for c in cassette_records}
    dock_by_key={f'dock/{kind}/{did}':center for kind,did,center in docks}
    for group in sorted({p['group'] for p in parts if p['group']}):
        if group[0]=='c' and group[1:3].isdigit():
            c=cassette_by_id[int(group[1:3])];center=c['body_center_m']; side=c['side']
            q=[0.,math.sqrt(.5)*(1 if side=='port' else -1),0.,math.sqrt(.5)]
        elif group[0]=='b' and group[1:3].isdigit():
            bay=bays[int(group[1:3])];center=bay['center_m']; q=[0.,math.sqrt(.5)*(1 if bay['center_m'][2]<0 else -1),0.,math.sqrt(.5)]
        elif group.startswith('dock/'):
            center=dock_by_key['/'.join(group.split('/')[:3])]; q=[0.,0.,0.,1.]
        else:
            center=[14.,0.,-22.8 if group.startswith('port/') else 22.8];q=[0.,0.,0.,1.]
        references[group]={'position_m':center,'orientation_xyzw':q}
    for bay in bays:
        bay['groups']={k:v for k,v in bay['groups'].items() if not k.startswith(('cradle','clamp','cassette'))}
    for cassette in cassette_records:
        cassette['mechanism_ids']=[g for g in references if g.startswith(f"c{cassette['cassette_id']:02d}/")]
    assert original.issubset(set(bpy.data.objects.keys()))
    bpy.ops.wm.save_as_mainfile(filepath=str(output),check_existing=False)
    manifest.write_text(json.dumps({'status':'20-drone target source fit only; no loader or capacity admission','source_sha256':digest,
        'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'base_parts_preserved':len(original),
        'coordinates':'body metres +Xforward +Yup +Zstarboard','bays':bays,'magazine_slots':magazine_slots,'cassettes':cassette_records,'exchange_buffers':[{'side':'port','body_center_m':[14,4.8,-22.8]},{'side':'starboard','body_center_m':[14,4.8,22.8]}],'target_ready_count':10,'target_reserve_count':10,'reload_scope':'same-side any compatible ready berth only','parts':parts,'mechanism_reference_poses':references,'transport_body_height_m':5.1,
        'pending':['full static and moving clearance','socket contact','engine/exhaust/crew/station checks',
                   'native visual review','authority registration and lifecycle']},indent=2))
    assert hashlib.sha256(source.read_bytes()).hexdigest()==digest

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('output',type=Path);p.add_argument('manifest',type=Path)
    a=p.parse_args();build(a.source.resolve(),a.output.resolve(),a.manifest.resolve())
