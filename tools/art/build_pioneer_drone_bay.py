#!/usr/bin/env python3
"""Author private 15-cradle bay candidate from exact revision11 master, bpy5.0.1.
Static reference poses plus explicit semantic kinematics; no simulation claim.
Never overwrite source/output. No stock drones embedded: runtime draws same IDs.
"""
import argparse, hashlib, json
from pathlib import Path
import bpy, bmesh
from mathutils import Vector
SOURCE_SHA='c5a55fe2c7e37c501e188249ae7eec90e292f35fb251b5b21e361013df8633de'
STATIONS=[-30.5,-24.5,-18.5]
TIERS=[-5.4,-3.2,-1.0,1.2,3.4]
PREFIX='pioneer.drone_bay/'
def build(source,output):
    assert bpy.app.version==(5,0,1)
    assert not output.exists(), 'new output required'
    before=hashlib.sha256(source.read_bytes()).hexdigest();assert before==SOURCE_SHA
    bpy.context.preferences.filepaths.use_scripts_auto_execute=False
    bpy.ops.wm.open_mainfile(filepath=str(source),load_ui=False,use_scripts=False)
    mats=bpy.data.materials
    roles={'armor':'Graphite ceramic armor','metal':'Titanium machinery','dark':'Recessed carbon','green':'Emerald structural frames','amber':'Safety amber','light':'Working lights'}
    parts=[];groups={};cuts=[]
    def cv(p):return (p[0],-p[2],p[1])
    def mesh(sid,verts,faces,material,group=None,bevel=.01):
        name=PREFIX+sid;assert name not in bpy.data.objects
        d=bpy.data.meshes.new(name+' mesh');d.from_pydata([cv(v) for v in verts],[],faces);d.update()
        bm=bmesh.new();bm.from_mesh(d);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(d);bm.free()
        o=bpy.data.objects.new(name,d);bpy.context.collection.objects.link(o);d.materials.append(mats[roles[material]])
        o['semantic_id']=name
        if group:o['kinematic_group']=group;groups.setdefault(group,[]).append(name)
        if bevel:
            m=o.modifiers.new('Manufactured edge radius','BEVEL');m.width=bevel;m.segments=2
            m=o.modifiers.new('Weighted manufactured normals','WEIGHTED_NORMAL');m.keep_sharp=True
        parts.append({'semantic_id':name,'source_name':name,'group':group,'material':roles[material]})
        return o
    def box(sid,lo,hi,material,group=None,bevel=.01):
        vs=[(lo[0] if i==0 else hi[0],lo[1] if j==0 else hi[1],lo[2] if k==0 else hi[2]) for i,j,k in [(0,0,0),(0,0,1),(0,1,1),(0,1,0),(1,0,0),(1,0,1),(1,1,1),(1,1,0)]]
        o=mesh(sid,vs,[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],material,group,bevel)
        parts[-1]['bounds_body_m']=[list(lo),list(hi)]
        return o
    def extrude_xy(sid,poly,z0,z1,material,bevel=0):
        n=len(poly);v=[(x,y,z0) for x,y in poly]+[(x,y,z1) for x,y in poly]
        return mesh(sid,v,[tuple(reversed(range(n))),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)],material,bevel=bevel)
    def beam(sid,start,end,width,material):
        a=Vector(start);b=Vector(end);axis=(b-a).normalized()
        u=axis.cross(Vector((0,0,1))).normalized()*width/2;v=axis.cross(u.normalized())*width/2
        vertices=[tuple(c+su*u+sv*v) for c in [a,b] for su,sv in [(-1,-1),(-1,1),(1,1),(1,-1)]]
        return mesh(sid,vertices,[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)],material,bevel=.005)
    def apply_cut(cutter,target=None):
        hull=target or bpy.data.objects['Pressure hull'];name=cutter.name
        bpy.context.view_layer.objects.active=hull;m=hull.modifiers.new('Actual unmanned bay cavity','BOOLEAN');m.operation='DIFFERENCE';m.solver='EXACT';m.object=cutter
        while hull.modifiers.find(m.name)>0:bpy.ops.object.modifier_move_up(modifier=m.name)
        bpy.ops.object.modifier_apply(modifier=m.name)
        bpy.data.objects.remove(cutter,do_unlink=True);parts[:]=[p for p in parts if p['source_name']!=name];cuts.append(name)
    # Lower contour retains the sloping belly. Shelf volume begins inboard of
    # the rear throat; no flat chamber cut is driven through the lower hull.
    floor=[(-35.8,-2.9),(-34.2,-6.42),(-33.2,-6.72),(-15.5,-6.72)]
    cavity=floor+[(-15.5,5.25),(-35.8,5.25)]
    apply_cut(extrude_xy('temporary/chamber',cavity,-7.15,7.15,'dark'))
    apply_cut(box('temporary/aperture',(-36.15,-2.6,-7.5),(-35.25,.6,.5),'dark',bevel=0))
    # Inboard pockets retain the original exterior skin outside the aperture.
    apply_cut(box('temporary/leaf_pocket',(-35.78,-4.4,-7.7),(-35.28,2.4,.7),'dark',bevel=0))
    # Floor lining rests immediately on the cavity's retained underlying hull.
    for i,((x0,y0),(x1,y1)) in enumerate(zip(floor,floor[1:])):
        extrude_xy(f'compartment/floor/{i}',[(x0,y0),(x1,y1),(x1,y1+.04),(x0,y0+.04)],-7.10,7.10,'dark')
    box('compartment/forward_bulkhead',(-15.60,-6.72,-7.15),(-15.50,5.25,7.15),'armor')
    box('compartment/roof',(-35.25,5.15,-7.15),(-15.50,5.25,7.15),'dark')
    for z,label in [(-7.15,'port'),(7.05,'starboard')]:
        # Side wall longitudinal lower edge follows the same floor contour.
        extrude_xy('compartment/wall/'+label,cavity,z,z+.10,'armor')
    def floor_y(x):
        if x<=-34.2:return -2.9+(x+35.8)*(-3.52/1.6)
        if x<=-33.2:return -6.42+(x+34.2)*(-.30)
        return -6.72
    # Added lining must respect the same real door pocket, not fill it back in.
    for target_name in ['compartment/floor/0','compartment/wall/port']:
        apply_cut(box('temporary/lining_pocket/'+target_name,(-35.79,-4.41,-7.71),(-35.27,2.41,.71),'dark',bevel=0),bpy.data.objects[PREFIX+target_name])
    # Fixed storage rack. Posts are outside drone/tray X envelopes and rails
    # support tray edge bearings, never block lateral withdrawal.
    slots=[]
    for si,x in enumerate(STATIONS):
        for dx,label in [(-2.83,'aft'),(2.83,'fore')]:
            for z,side in [(.10,'aisle'),(6.90,'back')]:
                xx=x+dx;fy=max(floor_y(xx-.08),floor_y(xx+.08))+.04
                box(f'rack/{si}/{label}/{side}/post',(xx-.07,fy,z-.07),(xx+.07,4.53,z+.07),'green')
                box(f'rack/{si}/{label}/{side}/foot',(xx-.13,fy,z-.13),(xx+.13,fy+.12,z+.13),'metal')
        for ti,y in enumerate(TIERS):
            sid=f's{si*5+ti:02d}';b=y-.9
            for dx,label in [(-2.62,'aft'),(2.62,'fore')]:
                box(f'rack/{sid}/runner/{label}',(x+dx-.08,b-.14,.28),(x+dx+.08,b-.08,6.94),'metal',bevel=.004)
                # Runner bracket joins actual post at outer X edge.
                for z,side in [(.1,'aisle'),(6.9,'back')]:
                    box(f'rack/{sid}/bracket/{label}/{side}',(min(x+dx,x+(-2.83 if dx<0 else 2.83))-.04,b-.18,z-.06),(max(x+dx,x+(-2.83 if dx<0 else 2.83))+.04,b-.08,z+.06),'green',bevel=.004)
            group='tray/'+sid
            box(f'tray/{sid}/deck',(x-2.64,b-.08,.40),(x+2.64,b,6.60),'metal',group,bevel=.004)
            # Low edge plates sit outside body bounds, not through its shell.
            for dx,label in [(-2.59,'aft'),(2.59,'fore')]:
                box(f'tray/{sid}/retainer/{label}',(x+dx-.025,b,.40),(x+dx+.025,b+.12,6.60),'amber',group,bevel=.005)
            box(f'rack/{sid}/identifier',(x+2.73,b+.18,6.76),(x+2.81,b+.48,6.86),'amber')
            slots.append({'id':sid,'station':si,'tier':ti,'body_center_m':[x,y,3.5],'body_forward':[-1,0,0],'drone_bounds_relative_m':[[-2.5,-.9,-3],[2.5,.9,3]],'tray_group':group,'reference_tray_body_contact_y':b,'class_seed':['recon','mining','attack'][si]})
        beam(f'rack/{si}/rear_diagonal',(x-2.83,-6.36,6.93),(x+2.83,4.35,6.93),.10,'green')
        for y in [-6.45,4.35]:
            box(f'rack/{si}/rear_tie/{y}',(x-2.9,y,6.82),(x+2.9,y+.09,6.98),'green')
    # Fixed gantry columns and continuous longitudinal head rails. These are
    # unmanned machinery, not walkable platforms or structurally rated members.
    for x,label in [(-34.0,'aft'),(-15.42,'fore')]:
        for z,side in [(-6.95,'port'),(-.05,'starboard')]:
            fy=max(floor_y(x-.08),floor_y(x+.08))+.04
            box(f'gantry/fixed/{label}/{side}/post',(x-.07,fy,z-.07),(x+.07,4.98,z+.07),'green')
            box(f'gantry/fixed/{label}/{side}/foot',(x-.12,fy,z-.12),(x+.12,fy+.12,z+.12),'metal')
    # Rails sit above chamber roof slots: remove only required narrow channels.
    for z,side in [(-6.95,'port'),(-.05,'starboard')]:
        box(f'gantry/fixed/rail/{side}',(-34.04,4.78,z-.10),(-15.35,4.94,z+.10),'metal')
    # Reference shuttle at aft station, exit height; group transforms combine
    # carriage X, lift Y and fork-stage extension. No Blender rig/animation.
    x=STATIONS[0];y=-1;b=y-.9
    for dx,label in [(-2.77,'aft'),(2.77,'fore')]:
        for z,side in [(-6.85,'port'),(-.15,'starboard')]:
            box(f'mechanism/carriage/{label}/{side}/mast',(x+dx-.05,-6.60,z-.05),(x+dx+.05,4.76,z+.05),'metal','carriage_x',bevel=.005)
            box(f'mechanism/carriage/{label}/{side}/head',(x+dx-.08,4.64,z-.15),(x+dx+.08,4.78,z+.15),'amber','carriage_x')
        box(f'mechanism/carriage/{label}/top_tie',(x+dx-.05,4.64,-6.85),(x+dx+.05,4.74,-.15),'green','carriage_x')
    for dx,label in [(-2.68,'aft'),(2.68,'fore')]:
        box(f'mechanism/lift/{label}/edge',(x+dx-.04,b-.25,-6.85),(x+dx+.04,b-.20,-.15),'metal','lift_xy',bevel=.004)
    for z,label in [(-6.75,'port'),(-.25,'starboard')]:
        box(f'mechanism/lift/{label}/tie',(x-2.72,b-.25,z-.04),(x+2.72,b-.20,z+.04),'metal','lift_xy',bevel=.004)
    # Three nested stages per tine. Reference is fully retracted; 3.5m stage
    # increments give 7m total extension and overlap at all intermediate poses.
    for dx,label in [(-1.8,'aft'),(1.8,'fore')]:
        for stage in range(3):
            low=b-.20+.04*stage
            box(f'mechanism/fork/{label}/stage{stage}',(x+dx-.07,low,-6.75),(x+dx+.07,low+.04,-2.55),'metal',f'fork_stage{stage}',bevel=.003)
    # Offset rear aperture: frame outside opening, separate semantic leaves.
    for z,label in [(-7.60,'port'),(.60,'starboard')]:
        box(f'door/frame/{label}',(-35.99,-2.75,z-.08),(-35.80,.75,z+.08),'green')
        box(f'door/guide/{label}',(-35.76,-4.40,z-.045),(-35.29,2.40,z+.045),'metal',bevel=.005)
    for yy,label in [(-2.7,'sill'),(.7,'header')]:
        box(f'door/frame/{label}',(-35.99,yy-.07,-7.60),(-35.80,yy+.07,.60),'green')
    for label,yl,yh in [('lower',-2.62,-1.0),('upper',-1.0,.62)]:
        g='door/'+label
        box(f'door/{label}/leaf',(-35.66,yl,-7.52),(-35.50,yh,.52),'armor',g,bevel=.01)
        box(f'door/{label}/rib',(-35.49,(yl+yh)/2-.06,-7.50),(-35.40,(yl+yh)/2+.06,.50),'green',g)
        box(f'door/{label}/edge_marker',(-35.50,(yl+yh)/2-.035,-7.3),(-35.485,(yl+yh)/2+.035,.30),'amber',g,bevel=.003)
    # Separate replaceable seal strips remain outside the declared clear throat.
    for z,label in [(-7.54,'port'),(.54,'starboard')]:
        box(f'door/seal/{label}',(-35.72,-2.62,z-.02),(-35.66,.62,z+.02),'dark',bevel=.002)
    for yy,label in [(-2.64,'sill'),(.64,'header')]:
        box(f'door/seal/{label}',(-35.72,yy-.02,-7.54),(-35.66,yy+.02,.54),'dark',bevel=.002)
    # Industrial lighting and cable trunk attached to room, above rack.
    for si,x in enumerate(STATIONS):
        box(f'lighting/{si}/housing',(x-1.8,4.44,6.65),(x+1.8,4.54,6.93),'metal')
        box(f'lighting/{si}/emitter',(x-1.7,4.42,6.67),(x+1.7,4.44,6.91),'light',bevel=.003)
    box('systems/cable_trunk',(-33.3,4.32,7.0),(-15.65,4.48,7.10),'dark')
    bpy.context.view_layer.update();output.parent.mkdir(parents=True,exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(output),compress=True)
    assert hashlib.sha256(source.read_bytes()).hexdigest()==before
    poses={'carriage_x':{'axes':['X'],'reference_center_m':[STATIONS[0],-1,-3.5],'x_range_m':[STATIONS[0],STATIONS[-1]]},'lift_xy':{'inherits':'carriage_x','y_reference_m':-1,'y_range_m':[TIERS[0],TIERS[-1]]}}
    for stage in range(3):poses[f'fork_stage{stage}']={'inherits':'lift_xy','extension_translation_z_m':[0,3.5*stage],'shared_extension_fraction_range':[0,1]}
    for s in slots:poses[s['tray_group']]={'body_center_reference_m':s['body_center_m'],'path':'rack -> aisle at same X/Y -> exit Y=-1 -> aft station X=-30.5; then lower0.20m while same drone holds Y=-1; hold while drone departs; reverse to store','translation_m_at_aisle':[0,0,-7]}
    poses['door/upper']={'translation_y_m':[0,1.70]};poses['door/lower']={'translation_y_m':[0,-1.70]}
    report={'schema':'arkline.pioneer-drone-bay-authoring/v1','source_sha256':before,'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'tool':'official bpy5.0.1','units':'body metres +X forward +Y up +Z starboard','bounds_body_m':[[-36,-9,-19],[36,11.35,19]],'slots':slots,'aisle_z_m':-3.5,'exit_y_m':-1,'launch_release_body_m':[-30.5,-1,-3.5],'release_tray_center_m':[-30.5,-1.2,-3.5],'release_disengage_y_m':-.20,'release_contract':'Same drone holds Y=-1 while tray/lift/retracted forks lower0.20m; only then translate drone aft. Recovery reverses. Explicit choreography, not force simulation.','aperture_body_m':[[-36,-2.6,-7.5],[-35.25,.6,.5]],'floor_profile_xy_m':floor,'door_pocket_body_m':[[-35.78,-4.4,-7.7],[-35.28,2.4,.7]],'parts':parts,'dynamic_groups':groups,'kinematics':poses,'cuts':cuts,'limits':['Geometry candidate; no pressure/strength/motor/power ratings','No crew access route','No drone bodies embedded: runtime persistent entities occupy slots','All state/roster/parent quaternion transforms remain runtime integration','Hardware clearance validation must pass before adopting anchors']}
    output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();build(a.source.resolve(),a.output.resolve())
