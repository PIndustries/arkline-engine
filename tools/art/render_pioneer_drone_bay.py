import bpy,json,sys,hashlib
from pathlib import Path
from mathutils import Vector
source=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
before=hashlib.sha256(source.read_bytes()).hexdigest();bpy.context.preferences.filepaths.use_scripts_auto_execute=False;bpy.ops.wm.open_mainfile(filepath=str(source),load_ui=False,use_scripts=False)
def cv(p):return Vector((p[0],-p[2],p[1]))
for o in bpy.data.objects:
 if o.name.startswith('pioneer.drone_bay/door/upper/'):o.location.z+=1.7
 if o.name.startswith('pioneer.drone_bay/door/lower/'):o.location.z-=1.7
scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=24;scene.cycles.use_denoising=True
scene.render.resolution_x=1600;scene.render.resolution_y=1100;scene.render.resolution_percentage=100
scene.world.color=(.15,.15,.15);scene.world.use_nodes=True;scene.world.node_tree.nodes['Background'].inputs['Color'].default_value=(.15,.2,.25,1);scene.world.node_tree.nodes['Background'].inputs['Strength'].default_value=.7
scene.view_settings.view_transform='AgX'
def area(name,position,target,power,size):
 d=bpy.data.lights.new(name,'AREA');d.energy=power;d.shape='DISK';d.size=size;o=bpy.data.objects.new(name,d);scene.collection.objects.link(o);o.location=cv(position);o.rotation_euler=(cv(target)-o.location).to_track_quat('-Z','Y').to_euler()
area('inspection key',(-25,15,-15),(-25,-1,2),7000,16);area('inspection fill',(-40,3,0),(-25,-1,2),4500,12);area('inspection rack fill',(-24,1,-4),(-24,0,5),2400,10)
d=bpy.data.cameras.new('source inspection camera');cam=bpy.data.objects.new('source inspection camera',d);scene.collection.objects.link(cam);scene.camera=cam;d.type='ORTHO'
def shot(name,pos,target,scale):
 cam.location=cv(pos);cam.rotation_euler=(cv(target)-cam.location).to_track_quat('-Z','Y').to_euler();d.ortho_scale=scale;scene.render.filepath=str(out/name);bpy.ops.render.render(write_still=True)
hidden=[]
for o in bpy.data.objects:
 if o.type=='MESH' and (not o.name.startswith('pioneer.drone_bay/') or o.name in ['pioneer.drone_bay/compartment/wall/port','pioneer.drone_bay/compartment/roof']):o.hide_render=True;hidden.append(o.name)
shot('authored-bay-cutaway-open.png',(-40,9,-23),(-25,-1,1),30)
# Physical source-space station view; wide camera is not native gameplay camera.
shot('authored-racks-side.png',(-24.5,1,-25),(-24.5,-1,3.5),23)
for o in bpy.data.objects:
 if o.type=='MESH':o.hide_render=False
shot('authored-exterior-rear-open.png',(-54,2,-12),(-35,-1,-3.5),23)
(out/'render-receipt.json').write_text(json.dumps({'source_sha256':before,'engine':'Blender5.0.1 Cycles CPU24samples','scope':'Actual authored mesh diagnostic cutaway, not runtime; no placeholder drone bodies baked; source not saved','hidden_for_cutaway':hidden,'door_pose':'upper+1.7m Y / lower-1.7m Y','cameras':['authored-bay-cutaway-open.png','authored-racks-side.png','authored-exterior-rear-open.png']},indent=2))
assert hashlib.sha256(source.read_bytes()).hexdigest()==before
