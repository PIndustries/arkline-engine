#!/usr/bin/env python3
"""Read-only bpy5 source clearance audit for the explicit revision11 access path.

Samples <=.04m along path and .05m along capsule axis. A .05m distance
margin over radius conservatively covers between-sample displacement. This
does not replace runtime support, interruption, picking or native acceptance.
"""
import bpy,json,math,argparse,hashlib
from pathlib import Path
from mathutils import Vector
from mathutils.bvhtree import BVHTree
parser=argparse.ArgumentParser();parser.add_argument('--source',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--registration',type=int,default=11);args=parser.parse_args()
assert args.registration in (11,12), 'unsupported source registration'
bpy.context.preferences.filepaths.use_scripts_auto_execute=False
bpy.ops.wm.open_mainfile(filepath=str(args.source.resolve()),use_scripts=False)
vs=[];faces=[];names=[];opaque=[];dg=bpy.context.evaluated_depsgraph_get()
for o in bpy.context.scene.objects:
 if o.type!='MESH':continue
 eo=o.evaluated_get(dg);m=eo.to_mesh();m.calc_loop_triangles();off=len(vs);vs.extend([o.matrix_world@v.co for v in m.vertices]);faces.extend([tuple(off+i for i in t.vertices) for t in m.loop_triangles]);names.extend([o.name]*len(m.loop_triangles));opaque.extend([o.data.materials[0].name!='Navigation glass']*len(m.loop_triangles));eo.to_mesh_clear()
bvh=BVHTree.FromPolygons(vs,faces,all_triangles=True)
pts=[]
for flight in range(3):
 z=-1.3 if flight%2==0 else 1.3
 for i in range(1,16):
  x=21+(i-.5)*.32 if flight%2==0 else 25.8-(i-.5)*.32
  y=-.32+flight*8/3+i*8/45
  pts.append((f'flight{flight}-tread{i}',(x,y,z)))
for label,p in [('entry',(20.5,-.32,0)),('land1',(26.4,2.346667,0)),('land2',(20.5,5.013333,0)),('top',(26.4,7.68,0)),('bridge',(29.2,7.68,0)),('pilot',(31.48,7.68,0))]:pts.append((label,p))
# Densify all stair tread/landing travel, including between the sampled centers.
paths=[]
for f in range(3):
 z=-1.3 if f%2==0 else 1.3
 flight=[p for label,p in pts if label.startswith(f'flight{f}-')]
 start=(20.5 if f%2==0 else 26.4,-.32+f*8/3,z)
 end=(26.4 if f%2==0 else 20.5,-.32+(f+1)*8/3,z)
 paths.append([start]+flight+[end])
paths.append([(20.5,-.32,0),(29.2,-.32,0),(29.2,-.32,-3.5),(34.1,-.32,-3.5),(34.1,-.32,-7.8)])
paths.extend([[(20.5,-.32,0),(20.5,-.32,-1.3)],[(26.4,2.346667,-1.3),(26.4,2.346667,1.3)],[(20.5,5.013333,1.3),(20.5,5.013333,-1.3)],[(26.4,7.68,-1.3),(26.4,7.68,0),(29.2,7.68,0)],[(19.5,-.32,0),(20.5,-.32,0)]])
for route in paths:
 for a,b in zip(route,route[1:]):
  n=max(1,math.ceil(math.dist(a,b)/.04))
  for k in range(n+1):
   x=a[0]+(b[0]-a[0])*k/n;z=a[2]+(b[2]-a[2])*k/n
   nh=max(1,math.ceil(abs(a[1]-b[1])/.04))
   for hstep in range(nh+1):
    pts.append(('stair-height-supercover',(x,a[1]+(b[1]-a[1])*hstep/nh,z)))
hits=[];mind=100
for label,(x,y,z) in pts:
 for h in [0.30+i*.05 for i in range(25)]:
  nearest,normal,idx,d=bvh.find_nearest(Vector((x,-z,y+h)))
  mind=min(mind,d)
  if d<.299:hits.append({'pose':label,'point':[x,y+h,z],'part':names[idx],'distance':d})
# Actual opaque source window rims must clear horizontal pilot sightlines;
# authored glass is excluded only for aperture rays, never capsule clearance.
opaque_faces=[f for f,keep in zip(faces,opaque) if keep]
opaque_names=[n for n,keep in zip(names,opaque) if keep]
raytree=BVHTree.FromPolygons(vs,opaque_faces,all_triangles=True)
cv=lambda p:Vector((p[0],-p[2],p[1]));rays=[]
for label,p,d,L in [('pilot-front',(31.48,9.35,0),(1,0,0),10),('pilot-port',(31.48,9.35,0),(0,0,-1),10),('pilot-starboard',(31.48,9.35,0),(0,0,1),10),('standing-port',(31.8,9.33,0),(0,0,-1),10),('standing-starboard',(31.8,9.33,0),(0,0,1),10),('turret-port',(10.8,9.864,-5.2),(1,0,0),100),('turret-starboard',(10.8,9.864,5.2),(1,0,0),100)]:
 hit=raytree.ray_cast(cv(p),cv(d),L);rays.append({'ray':label,'hit':opaque_names[hit[2]] if hit[2] is not None else None,'distance':hit[3]})
# The far-side port hatch is intentional and must not be sealed with the nose.
port_normal=Vector((.6127103496,0,-.7903075524))
for y in [.1,.8,1.45]:
 p=Vector((32.985,y,-9.815));hit=raytree.ray_cast(cv(p+port_normal*.7),cv(-port_normal),1.4)
 rays.append({'ray':f'port-boarding-aperture-{y}','hit':opaque_names[hit[2]] if hit[2] is not None else None,'distance':hit[3]})
assert all(x['hit'] is None for x in rays),rays
# Seal the retired low bridge recess, including its former open lower sill and
# chamfer/wrap joint. These short probes cannot be satisfied by distant walls.
closure_rays=[]
for side in [-1,1]:
 for y in [-.70,-.50,.5,2.70]:
  for z in [1.0,5.0,6.47,7.7]:
   origin=cv((37.0,y,side*z));hit=raytree.ray_cast(origin,cv((-1,0,0)),2.0)
   closure_rays.append({'region':'front','side':side,'height_m':y,'z_m':side*z,'hit':opaque_names[hit[2]] if hit[2] is not None else None})
  normal=Vector((.70710678,0,side*.70710678));point=Vector((34.385,y,side*8.715))
  hit=raytree.ray_cast(cv(point+normal*2),cv(-normal),2.3)
  closure_rays.append({'region':'chamfer-wrap','side':side,'height_m':y,'hit':opaque_names[hit[2]] if hit[2] is not None else None})
assert all(x['hit'] is not None for x in closure_rays),[x for x in closure_rays if x['hit'] is None]

args.output.open('w').write(json.dumps({'source_blend_sha256':hashlib.sha256(args.source.read_bytes()).hexdigest(),'canonical_glb_sha256':hashlib.sha256((args.source.parent/'pioneer.glb').read_bytes()).hexdigest(),'registration':args.registration,'opaque_source_aperture_and_turret_rays':rays,'retired_lower_bridge_closure_rays':closure_rays,'route_id':'raised-bridge-three-flight-access-v1','method':'BVH triangle nearest-distance over full capsule-axis and stair foot-height supercover; .04m path/height and .05m axis spacing; conservative .05m between-sample displacement bound','scope':'all three flights, landings, lower central foyer entry and upper bridge entry; includes lower foyer and port service passage up to inner hatch; dynamic hatch and exterior lift are separate runtime tests','capsule_radius_m':.3,'capsule_height_m':1.8,'poses':len(pts),'min_sampled_clearance_m':mind,'violations':hits},indent=2))
print('poses',len(pts),'min',mind,'violations',len(hits));print(hits[:10])

assert not hits, "canonical source intersects registered bridge access capsule"
assert mind > .35, "source continuum clearance margin below conservative sample bound"
