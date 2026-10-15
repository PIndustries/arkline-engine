"""Build Arkline's original, editable Pioneer hull. Run with Blender --background --python.
Body axes in meters: +X bow, +Y up, +Z starboard. No downloaded assets.
"""
import pathlib
ROOT=pathlib.Path(__file__).resolve().parents[2]
OUT=ROOT/'assets/ships/pioneer'
# This construction recipe clears its scene. Never let it overwrite an editable
# master, frozen export or evidence render that already exists in the checkout.
if any((OUT/name).exists() or (OUT/name).is_symlink()
       for name in ('pioneer.blend', 'pioneer.glb', 'pioneer-hull.png')):
 raise RuntimeError('Refusing to overwrite existing Pioneer assets. Export the explicitly selected saved master with tools/art/export_pioneer.py into a new directory.')
import bpy, bmesh, math, json, sys
from mathutils import Vector
OUT.mkdir(parents=True,exist_ok=True)
bpy.context.preferences.filepaths.save_version=0
bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
def v(p): return (p[0],-p[2],p[1])
def mat(name,color,metal=.0,rough=.5,emission=0):
 m=bpy.data.materials.new(name); m.diffuse_color=(*color,1); m.use_nodes=True
 p=m.node_tree.nodes.get('Principled BSDF'); p.inputs['Base Color'].default_value=(*color,1); p.inputs['Metallic'].default_value=metal;p.inputs['Roughness'].default_value=rough
 p.inputs['Emission Color'].default_value=(*color,1);p.inputs['Emission Strength'].default_value=emission
 return m
mats=[mat('Graphite ceramic armor',(.055,.075,.072),.08,.38),mat('Emerald structural frames',(.015,.30,.115),.06,.32),mat('Titanium machinery',(.26,.32,.33),.85,.28),mat('Recessed carbon',(.008,.012,.014),.3,.62),mat('Navigation glass',(.025,.22,.25),.7,.17,.25),mat('Safety amber',(.8,.36,.025),.5,.4),mat('Working lights',(.65,.9,.8),.1,.25,3),mat('Drive emission',(.01,.8,.25),.2,.3,4)]
mats.append(mat('Hull identification ivory',(.52,.62,.57),.0,.55))
def mesh(name,verts,faces,mi):
 me=bpy.data.meshes.new(name);me.from_pydata([v(p) for p in verts],[],faces);me.update();o=bpy.data.objects.new(name,me);bpy.context.collection.objects.link(o);o.data.materials.append(mats[mi]);return o
def bevel(o,width=.12):
 mod=o.modifiers.new('Machined edge radius','BEVEL');mod.width=width;mod.segments=2
 mod=o.modifiers.new('Weighted normals','WEIGHTED_NORMAL')
def box(name,pos,size,mi,edge=.1):
 bpy.ops.mesh.primitive_cube_add(size=1,location=v(pos));o=bpy.context.object;o.name=name;o.dimensions=(size[0],size[2],size[1]);bpy.ops.object.transform_apply(location=False,rotation=False,scale=True);o.data.materials.append(mats[mi]);
 if edge:bevel(o,edge)
 return o
def tube(name,a,b,r,mi,vertices=12):
 va,vb=Vector(v(a)),Vector(v(b));d=vb-va;bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=r,depth=d.length,location=(va+vb)/2);o=bpy.context.object;o.name=name;o.rotation_euler=d.to_track_quat('Z','Y').to_euler();o.data.materials.append(mats[mi]);bevel(o,min(.06,r*.2));return o
def loft(name,sections,mi):
 verts=[]
 for x,w,b,t in sections:
  # Clockwise chamfered cross section; retains the forward inhabited core.
  verts.extend([(x,b,w*.78),(x,b+.9,w),(x,t-1.1,w),(x,t,w*.78),(x,t,-w*.78),(x,t-1.1,-w),(x,b+.9,-w),(x,b,-w*.78)])
 faces=[tuple(reversed(range(8)))]
 for j in range(len(sections)-1):
  for k in range(8):faces.append((j*8+k,j*8+(k+1)%8,(j+1)*8+(k+1)%8,(j+1)*8+k))
 faces.append(tuple(range((len(sections)-1)*8,len(sections)*8)))
 o=mesh(name,verts,[tuple(reversed(f)) for f in faces],mi);mod=o.modifiers.new('Hull edge radius','BEVEL');mod.width=.12;mod.segments=2;return o
loft('Pressure hull', [(-35,8,-6,5),(-28,8.6,-8,7),(-12,8.8,-8.7,8),(8,8.8,-8.7,8),(25,8.6,-8.2,6.5),(36,8,-7.8,3.5)],0)
# Keel establishes the documented lower body limit.
box('Load bearing keel',(-2,-8.5,0),(52,1,8),2,.18)
for side in [-1,1]:
 z=side*14
 loft('Outboard drive nacelle', [(-36,3.1,-3.6,3.6),(-31,4.6,-5.8,6.1),(-23,4.6,-5.8,6.1),(-20,3.7,-4.8,5.0),(8,3.7,-4.8,5.0),(12,4.6,-5.8,6.1),(21,4.6,-5.8,6.1),(26,3.1,-3.8,4.0)],0).location.y=-z
 # Side armor plates, exposed ribs, and inset machinery.
 for x in [-26,-18,-10,-2,6,14]:
  box('Titanium nacelle rib',(x,0,side*18.0),(.35,8.8,.5),2,.10)
  box('Recessed side service bay',(x+3,0,side*17.7),(5.8,5.4,.55),3,.18)
  box('Service bay armor',(x+3,.4,side*18.0),(4.9,4,.3),0,.18)
  for yy in [-1.4,1.4]:
   tube('Service conduit',(x+.6,yy,side*18.25),(x+5.1,yy,side*18.25),.09,2)
  for yy in [-1.6,1.6]:
   for xx in [x+1,x+5]:box('Panel fastener',(xx,yy,side*18.22),(.18,.18,.12),2,.03)
 box('Outer impact rail',(-8,-2.8,side*18.6),(40,.65,.8),1,.15)
 for x in [-23,-15,-7,1,9]:
  box('Top machinery recess',(x,6.2 if x==-23 else 5.25,side*14),(5.5,.22,4),3,.08)
  for dx in [-1.8,-1.2,-.6,0,.6,1.2,1.8]:box('Radiator blade',(x+dx,6.43 if x==-23 else 5.48,side*14),(.18,.35,3.5),2,.03)
 tube('Longitudinal fluid trunk',(-25,8.35,side*8.7),(19,8.35,side*8.7),.26,2)
 tube('Green protective conduit',(-24,8.1,side*9.4),(13,8.1,side*9.4),.17,1)
 # Engine bells: ring surface with inset luminous core, not a solid cube.
 verts=[];faces=[]
 for x,r in [(-31.5,2.7),(-34.8,2.7),(-35.9,2.2),(-35.7,1.8),(-33,1.5)]:
  for i in range(24):
   a=i*math.tau/24;verts.append((x,math.sin(a)*r,side*12.5+math.cos(a)*r))
 for j in range(4):
  for i in range(24):faces.append((j*24+i,j*24+(i+1)%24,(j+1)*24+(i+1)%24,(j+1)*24+i))
 mesh('Main drive bell',verts,faces,2)
 tube('Drive throat',(-33.1,0,side*12.5),(-33.2,0,side*12.5),1.5,7,24)
 for x in [-25,-9,7,22]:box('Side running light',(x,3,side*17.2),(.6,.3,.25),6,.05)
 # Structural upper shoulders and segmented emerald protection.
 for x in [-21,-5,11]:
  box('Shoulder frame',(x,8.05,side*7.4),(1.2,1,5),1,.25)
 for x in [-18,-10,-2,6,14,22]:
  box('Upper armored plate',(x,8.45 if x<18 else 7.15,side*4.6),(6.9,.5,4.5),0,.17)
for x in [-25,23]:
 for side in [-1,1]:
  box('Armored emerald collar',(x,7.4,side*5.8),(1.6,.9,9),1,.18)
# Central roof with mount platform exact y9; turrets themselves remain articulated runtime entities.
box('Mining turret deck',(10.8,8.45,0),(8,1.1,8),1,.2)
for z in [-2.66,2.66]:tube('Turret mounting socket',(10.8,8.7,z),(10.8,9,z),1.25,2,24)
for x in [-22,-13,-4]:
 box('Cargo access recess',(x,8,0),(7,.25,6),3,.1)
 box('Cargo hatch',(x,8.25,0),(6.5,.3,5.5),2,.15)
 box('Hatch center reinforcement',(x,8.5,0),(.45,.2,5.3),1,.04)
 for z in [-2,2]:box('Hatch hinge',(x-2.7,8.5,z),(.5,.25,.5),2,.07)
# Forebody layered angular armor, glazing and landing gear structures.
for s in [-1,1]:
 mesh('Forward glazing',[(29,5.55,s*.6),(35.7,3.72,s*.6),(35.7,3.72,s*6.8),(29,5.55,s*7)],[(0,1,2,3)],4)
 tube('Canopy mullion',(29,5.65,s*.6),(35.7,3.85,s*.6),.14,2)
 box('Forward protective cheek',(29,-2,s*9),(7.5,4,1.8),1,.3)
 box('Forward work lamp',(34.5,-1,s*8.7),(1,.4,.3),6,.06)
 for x in [-22,17]:
  tube('Landing strut',(x,-5,s*10),(x-1,-8,s*12),.4,2)
  box('Landing shoe',(x-1,-8.6,s*12),(4,.8,2.5),0,.15)
# Finite central end caps ensure axial RCS ray intersections.
# Small rear antenna and top service rail stay within 18m body envelope.
for z in [-6,6]:
 tube('Service handrail',(-23,8.8,z),(-5,8.8,z),.065,2)
 for x in [-23,-17,-11,-5]:tube('Rail stanchion',(x,7.6,z),(x,8.8,z),.065,2)
# Distinct pod collars, diagonal bracing and utility bundles establish industrial scale.
for side in [-1,1]:
 for x in [-27,16]:
  for dx in [-2.0,2.0]:
   box('Collar outer armor',(x+dx,0,side*18.45),(1.3,10.0,.65),1,.20)
   box('Collar crown',(x+dx,6.3,side*14),(1.3,.65,8.0),1,.18)
   box('Collar lower rail',(x+dx,-5.8,side*14),(1.3,.5,7.8),2,.12)
  # Recessed end-cap service equipment with visible hazard edging.
  box('Pod utility recess',(x,0,side*18.5),(2.5,5.8,.18),3,.08)
  for yy in [-2,0,2]:
   tube('Pressure coupling',(x,yy,side*18.4),(x,yy,side*18.8),.38,2,12)
   tube('Coupling inset',(x,yy,side*18.81),(x,yy,side*18.84),.24,3,12)
 for x in [-16,-5,6]:
  # Visible external truss spans the negative space; no cabin penetration.
  tube('Pod structural diagonal',(x-2,3.2,side*8.8),(x+2,3.2,side*10.6),.23,2)
  tube('Pod structural tie',(x+2,-3,side*8.8),(x-2,-3,side*10.6),.23,2)
  tube('Insulated service feed',(x,4.2,side*8.8),(x,4.2,side*10.6),.18,1)
 for x in [-18,-10,-2,6]:
  tube('External coolant vessel',(x-2,-.3,side*18.0),(x+2,-.3,side*18.0),.5,2,12)
  for dx in [-1.5,1.5]:tube('Coolant vessel clamp',(x+dx-.08,-.3,side*18.0),(x+dx+.08,-.3,side*18.0),.58,0,12)
  tube('Protected signal harness',(x-2,1.3,side*18.3),(x+2,1.3,side*18.3),.075,5,8)
 # Maintainer-sized top safety rails and an access ladder, rather than oversized trim.
 for x in [-19,-13,-7,-1,5]:
  tube('Pod walkway stanchion',(x,5.2,side*16.5),(x,6.3,side*16.5),.055,5,8)
 tube('Pod safety rail',(-19,6.3,side*16.5),(5,6.3,side*16.5),.055,5,8)
 for x in [11.1,12.0]:tube('Service ladder upright',(x,-4.6,side*18.8),(x,4.7,side*18.8),.055,2,8)
 for yy in [-4,-3.6,-3.2,-2.8,-2.4,-2,-1.6,-1.2,-.8,-.4,0,.4,.8,1.2,1.6,2,2.4,2.8,3.2,3.6,4]:
  tube('Service ladder rung',(11.1,yy,side*18.8),(12.0,yy,side*18.8),.04,2,8)
 # Short lift eyes, access locks and identification plates are geometry, not textures.
 for x in [-24,14]:
  box('Service identity backing',(x,2.8,side*18.83),(2.7,.65,.08),3,.03)
  for i in range(5):box('Asset identity mark',(x-1+i*.4,2.8,side*18.9),(.15,.32,.025),6 if i==0 else 2,0)
 for x in [-30,23]:
  tube('Directional jet housing',(x,0,side*17.4),(x,0,side*18.8),.52,2,16)
  tube('Directional jet throat',(x,0,side*18.81),(x,0,side*18.86),.33,3,16)
# Permanent hull markings use a small geometry budget and survive the scalar GLB pipeline.
def label(text,body_pos,size,side):
 bpy.ops.object.text_add(location=v(body_pos));o=bpy.context.object;o.name='Hull marking '+text
 o.data.body=text;o.data.align_x='CENTER';o.data.align_y='CENTER';o.data.size=size;o.data.extrude=0;o.data.resolution_u=2
 o.rotation_euler=(math.pi/2,0,0 if side>0 else math.pi)
 o.data.materials.append(mats[8]);bpy.ops.object.convert(target='MESH')
for side in [-1,1]:
 label('P.INDUSTRIES',(-7,2.8,side*18.45),.65,side)
 label('PIONEER  /  01',(17,4.3,side*18.81),.42,side)
 for x in [-18,-10,-2,6]:
  for dx in [-1.5,0,1.5]:
   box('Service panel inset seam',(x+dx,-2.5,side*18.2),(.04,1.0,.025),3,0)
 # Fore collar chevrons, backed by armor instead of floating in space.
 for yy in [-3,-2.4,-1.8]:
  tube('Access warning chevron',(18,yy,side*18.81),(18.4,yy+.25,side*18.81),.055,5,6)

# Layered front fascia, small worklight guards and underbody service panels.
for side in [-1,1]:
 for x,y in [(29,5.7),(32,4.8),(35,3.9)]:
  tube('Forebody glazing frame',(x,y,side*.7),(x,y,side*6.5),.08,2,8)
 for z in [2.0,4.0,6.0]:
  box('Bow impact rib',(35.65,-2.0,side*z),(.55,5.4,.18),2,.08)
 box('Bow lamp surround',(35.6,1.0,side*6.5),(.65,.8,1.25),3,.12)
 box('Bow worklight lens',(35.94,1.0,side*6.5),(.08,.38,.85),6,.04)
 for x in [-20,-8,4]:box('Ventral maintenance plate',(x,-8.8,side*4),(8,.25,3.3),0,.12)

# Save editable named components; export evaluated, material-batched geometry separately.
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'pioneer.blend'))
for o in list(bpy.context.scene.objects):
 if o.type=='MESH':
  bpy.context.view_layer.objects.active=o;o.select_set(True)
  for mod in list(o.modifiers):bpy.ops.object.modifier_apply(modifier=mod.name)
  bm=bmesh.new();bm.from_mesh(o.data)
  bmesh.ops.dissolve_degenerate(bm,dist=0.000001,edges=list(bm.edges))
  bmesh.ops.triangulate(bm,faces=list(bm.faces))
  # Text outlines and bevel corners can create zero-area export triangles.
  bmesh.ops.delete(bm,geom=[f for f in bm.faces if f.calc_area()<0.000001],context='FACES')
  bm.to_mesh(o.data);bm.free();o.data.update()
  o.select_set(False)
for material in mats:
 objects=[o for o in bpy.context.scene.objects if o.type=='MESH' and o.data.materials and o.data.materials[0]==material]
 if not objects:continue
 bpy.ops.object.select_all(action='DESELECT')
 for o in objects:o.select_set(True)
 bpy.context.view_layer.objects.active=objects[0];bpy.ops.object.join();objects[0].name=material.name
bpy.ops.export_scene.gltf(filepath=str(OUT/'pioneer.glb'),export_format='GLB',export_yup=True,export_apply=True,export_cameras=False,export_lights=False)
# Render the actual exported surface source, not an illustrative generated concept.
scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.samples=32
scene.world.color=(.12,.12,.12)
def area(name,loc,power,size,color):
 bpy.ops.object.light_add(type='AREA',location=v(loc));o=bpy.context.object;o.name=name;o.data.energy=power;o.data.shape='DISK';o.data.size=size;o.data.color=color;o.rotation_euler=(-o.location).to_track_quat('-Z','Y').to_euler()
area('Key',(35,65,40),150000,50,(.8,.92,1));area('Rim',(-35,35,-40),170000,35,(.3,1,.6));area('Fill',(40,0,-45),90000,40,(.7,.8,1))
bpy.ops.object.camera_add(location=v((90,60,85)));cam=bpy.context.object;cam.rotation_euler=(-cam.location).to_track_quat('-Z','Y').to_euler();cam.data.type='ORTHO';cam.data.ortho_scale=100;scene.camera=cam
scene.render.resolution_x=1800;scene.render.resolution_y=1200;scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG';scene.render.filepath=str(OUT/'pioneer-hull.png');bpy.ops.render.render(write_still=True)
