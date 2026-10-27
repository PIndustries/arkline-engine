#!/usr/bin/env python3
"""Author a separate sealed receiver module; never mutate the registered hull.
Official bpy5.0.1, body metres (Blender x,-z,y), no simulation or load rating.
"""
import argparse, hashlib, json
from pathlib import Path
import bpy, bmesh
BASE_BLEND = 'ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9'
BASE_GLB = '7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd'
PREFIX = 'pioneer.ore_receiver/'
def build(source, output):
    assert bpy.app.version == (5,0,1)
    assert hashlib.sha256(source.read_bytes()).hexdigest() == BASE_BLEND
    assert not output.exists()
    output.mkdir(parents=True)
    bpy.context.preferences.filepaths.use_scripts_auto_execute = False
    bpy.ops.wm.open_mainfile(filepath=str(source.resolve()), load_ui=False, use_scripts=False)
    for obj in list(bpy.data.objects): bpy.data.objects.remove(obj, do_unlink=True)
    parts=[]
    def box(name, lo, hi, material, group=None, bevel=.006, hull_contact=False):
        vertices=[(x,-z,y) for x,y,z in [(lo[0],lo[1],lo[2]),(lo[0],lo[1],hi[2]),(lo[0],hi[1],hi[2]),(lo[0],hi[1],lo[2]),(hi[0],lo[1],lo[2]),(hi[0],lo[1],hi[2]),(hi[0],hi[1],hi[2]),(hi[0],hi[1],lo[2])]]
        mesh=bpy.data.meshes.new(PREFIX+name+' editable mesh');mesh.from_pydata(vertices,[],[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]);mesh.update()
        bm=bmesh.new();bm.from_mesh(mesh);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(mesh);bm.free()
        obj=bpy.data.objects.new(PREFIX+name,mesh);bpy.context.collection.objects.link(obj);mesh.materials.append(bpy.data.materials[material]);obj['semantic_id']=PREFIX+name
        if group:obj['kinematic_group']=group
        if bevel:
            m=obj.modifiers.new('Manufactured edge radius','BEVEL');m.width=bevel;m.segments=2
            obj.modifiers.new('Weighted manufactured normals','WEIGHTED_NORMAL')
        parts.append(dict(name=obj.name,group=group,bounds_body_m=[lo,hi],material=material,hull_contact=hull_contact))
    metal='Titanium machinery';dark='Recessed carbon';green='Emerald structural frames';amber='Safety amber';ivory='Hull identification ivory'
    # Sealed face plate and continuous hull landing pads: only these intentional
    # contact solids touch the original pressure hull. No hole is cut into it.
    box('sealed_feedthrough_plate',[-36.08,-1.65,4.35],[-35.98,-.35,5.65],metal,hull_contact=True)
    for side,z in [('port',4.28),('starboard',5.62)]:
        box('anchor/'+side,[-36.10,-1.80,z],[-35.98,-.20,z+.10],green,hull_contact=True)
        box('gusset/'+side,[-36.44,-1.60,z],[-36.08,-.40,z+.10],green)
        box('shoe/'+side,[-36.46,-1.64,z-.02],[-36.40,-.36,z+.12],metal)
    # Four-wall nested sleeves: real open bores, not opaque solid plugs.
    def tube(name,x0,x1,outer,inner,material,group=None):
        for side,y0,y1,z0,z1 in [('bottom',-outer,-inner,-outer,outer),('top',inner,outer,-outer,outer),('port',-inner,inner,-outer,-inner),('starboard',-inner,inner,inner,outer)]:
            box(name+'/'+side,[x0,-1+y0,5+z0],[x1,-1+y1,5+z1],material,group,bevel=.003)
    tube('fixed_collar',-36.50,-36.08,.30,.255,metal)
    # Three0.40m sleeves travel1/3m each, retaining66.7mm axial overlap.
    for i,(outer,inner) in enumerate([(.25,.205),(.20,.155),(.15,.105)],1):
        tube('sleeve'+str(i),-36.50,-36.10,outer,inner,metal,'stage/'+str(i))
    # Staggered head stays ahead of every sleeve even at stow. Side pockets
    # are genuine absent geometry; locking shoes slide into the socket bore.
    for side,y0,y1 in [('bottom',-.22,-.105),('top',.105,.22)]:
        box('coupling_head/'+side,[-36.58,-1+y0,4.78],[-36.50,-1+y1,5.22],metal,'stage/3',bevel=.003)
    for side,sign in [('port',-1),('starboard',1)]:
        zlo,zhi=sorted([5+sign*.105,5+sign*.22])
        for label,x0,x1,y0,y1 in [('front',-36.58,-36.56,-.105,.105),('back',-36.52,-36.50,-.105,.105),('lower',-36.56,-36.52,-.105,-.08),('upper',-36.56,-36.52,.08,.105)]:
            box('coupling_head/'+side+'/'+label,[x0,-1+y0,zlo],[x1,-1+y1,zhi],metal,'stage/3',bevel=.001)
        a,b=sorted([5+sign*.12,5+sign*.22])
        box('locking_shoe/'+side,[-36.56,-1.08,a],[-36.52,-.92,b],amber,'latch/'+side,bevel=.001)
    tube('head_seal',-36.515,-36.505,.225,.218,dark,'stage/3')
    # Backed gasket/identification bands improve readable seal edges without
    # pretending a light communicates authority or adding unsupported materials.
    tube('mount_gasket',-36.09,-36.075,.61,.57,dark)
    for y in [-1.57,-.45]:
        box('service_band/'+str(y),[-36.105,y,4.58],[-36.09,y+.025,5.42],amber,bevel=.002)
    box('cargo_identifier',[-36.115,-.37,4.72],[-36.10,-.32,5.28],ivory,bevel=.002)
    groups={g:[p['name'] for p in parts if p['group']==g] for g in ['stage/1','stage/2','stage/3','latch/port','latch/starboard']}
    path=output/'ore-receiver.blend';bpy.ops.wm.save_as_mainfile(filepath=str(path.resolve()),check_existing=False,compress=True)
    manifest=dict(schema='arkline.pioneer-ore-receiver-authoring/v1',base_registration=12,base_blend_sha256=BASE_BLEND,base_glb_sha256=BASE_GLB,output_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),parts=parts,dynamic_groups=groups,reference_root_body_m=[-36.5,-1,5],gate_pose={'position_m':[-50,-1,5],'orientation_xyzw':[0,1,0,0]},hold_pose={'position_m':[-40,-1,5],'orientation_xyzw':[0,1,0,0]},socket={'mouth_body_m':[-37.5,-1,5],'clear_square_m':.50,'flange_square_m':.72,'head_square_m':.44,'insertion_m':.08,'final_tip_x_m':-37.58},kinematics={'stages_travel_x_m':[-1/3,-2/3,-1.0],'latch_head_travel_x_m':-1.0,'latch_travel_z_m':{'port':-.03,'starboard':.03},'order':'extend all three stages together; then close jaws; open jaws before retract; drone stationary through coupling'},limits=['Separate additive module; canonical hull/old saves remain byte-identical','Sealed feedthrough face only; no open cargo-room tunnel or crew access','Stationary carrier; no force,pressure,power,kg,throughput or inventory simulation in this asset','Static mounting shoes intentionally contact hull; moving sleeves must not intersect it','Runtime compatibility and native visuals require separate acceptance'])
    (output/'authoring.json').write_text(json.dumps(manifest,indent=2)+'\n')
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();build(a.source,a.output)
