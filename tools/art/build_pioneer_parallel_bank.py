#!/usr/bin/env python3
"""Replicate the proved local-cell candidate; no source admission.
Logical collision solids remain distinct. Render batching welds no new volume:
only exact same-group/same-material source meshes are joined.
"""
import argparse,copy,hashlib,json
from pathlib import Path
import bpy,bmesh
import numpy as np
from pioneer_export_contract import runtime_cross_squared,f32
P='pioneer.magazine_ready/'
F=[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
a=argparse.ArgumentParser();a.add_argument('source',type=Path);a.add_argument('manifest',type=Path);a.add_argument('output',type=Path);a.add_argument('out_manifest',type=Path);a=a.parse_args()
doc=json.loads(a.manifest.read_text());assert hashlib.sha256(a.source.read_bytes()).hexdigest()==doc['output_sha256'];assert not a.output.exists()
bpy.ops.wm.open_mainfile(filepath=str(a.source),load_ui=False,use_scripts=False)
parts=copy.deepcopy(doc['parts']);refs=copy.deepcopy(doc['mechanism_reference_poses'])
def delete(r):
 bpy.data.objects.remove(bpy.data.objects[r['semantic_id']],do_unlink=True);parts.remove(r)
def box(name,lo,hi,material='Graphite ceramic armor'):
 name=P+name;points=[(lo[0] if i==0 else hi[0],lo[1] if j==0 else hi[1],lo[2] if k==0 else hi[2]) for i,j,k in [(0,0,0),(0,0,1),(0,1,1),(0,1,0),(1,0,0),(1,0,1),(1,1,1),(1,1,0)]]
 m=bpy.data.meshes.new(name);m.from_pydata([(x,-z,y) for x,y,z in points],[],F);m.update();bm=bmesh.new();bm.from_mesh(m);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(m);bm.free();o=bpy.data.objects.new(name,m);bpy.context.collection.objects.link(o);m.materials.append(bpy.data.materials[material]);o['semantic_id']=name;parts.append({'semantic_id':name,'group':None,'material':material,'bounds_body_m':[lo,hi]})
def map_group(g,bid):
 if not g:return None
 return g.replace('cell00/',f'cell{bid:02d}/').replace('dock/protected/0/',f'dock/protected/{bid}/')
def clone(r,bid,name=None):
 sign=1 if bid<5 else -1;dx=8*(bid%5)
 old=bpy.data.objects[r['semantic_id']];o=old.copy();o.data=old.data.copy();bpy.context.collection.objects.link(o)
 # Convert each evaluated body position through a world-space reflection/offset.
 from mathutils import Matrix
 t=Matrix.Translation((dx,0,0));mirror=Matrix.Diagonal((1,sign,1,1));o.matrix_world=t@mirror@old.matrix_world
 new=name or r['semantic_id'].replace('/cell00/',f'/cell{bid:02d}/')
 o.name=new;o['semantic_id']=new;g=map_group(r.get('group'),bid)
 if g:o['kinematic_group']=g
 elif 'kinematic_group' in o:del o['kinematic_group']
 b=copy.deepcopy(r);b['semantic_id']=new;b['group']=g
 lo,hi=r['bounds_body_m'];b['bounds_body_m']=[[lo[0]+dx,lo[1],lo[2] if sign==1 else -hi[2]],[hi[0]+dx,hi[1],hi[2] if sign==1 else -lo[2]]];parts.append(b)
 if g:refs[g]={'position_m':[-26.+dx,0.,-22.8*sign],'orientation_xyzw':[0.,0.,0.,1.]}
# Explicitly retired original central rack: never extra stock capacity.
removed_obsolete_rack=[]
for obj in list(bpy.data.objects):
 if obj.name.startswith('pioneer.drone_bay/rack/'):
  removed_obsolete_rack.append(obj.name);bpy.data.objects.remove(obj,do_unlink=True)
assert len(removed_obsolete_rack)==138
# Retire superseded front exchange resources; retain their ordinary fixed floor only.
removed_shared_buffer_parts=[]
for r in list(parts):
 n=r['semantic_id']
 if 'exchange buffer deck' in n:
  new=n.replace('exchange buffer deck','forward fixed service floor');obj=bpy.data.objects[n];obj.name=new;obj['semantic_id']=new;r['semantic_id']=new
 elif 'exchange buffer support' in n or n.startswith(P+'dock/buffer/'):
  removed_shared_buffer_parts.append(n);delete(r)
for g in list(refs):
 if g.startswith('dock/buffer/'):refs.pop(g)
doc['exchange_buffers']=[{'buffer_id':i,'side':'port' if i<5 else 'starboard','body_center_m':[-26.+8*(i%5),0.,-30. if i<5 else 30.],'capacity_added':0} for i in range(10)]
doc['removed_shared_buffer_parts']=removed_shared_buffer_parts
# Snapshot reusable local mechanics before removing one-cell retained casing pieces.
local=[r for r in parts if r['semantic_id'].startswith(P+'cell00/') and 'retained ' not in r['semantic_id']]
protected=[r for r in parts if (r.get('group')or'').startswith(('cell00/protected/support/','dock/protected/0/')) and not r['semantic_id'].startswith(P+'cell00/')]
for r in list(parts):
 n=r['semantic_id'];g=r.get('group')or''
 if 'retained ' in n and n.startswith(P+'cell00/'):delete(r)
 elif g.startswith('starboard/loader/'):delete(r)
 elif n in [P+'starboard/lower continuous casing',P+'starboard/lower shoulder',P+'starboard/magazine/parking deck',P+'starboard/magazine/fixed aperture front floor']:delete(r)
for g in list(refs):
 if g.startswith('starboard/loader/'):refs.pop(g)
# Replace each original protected pad/track/jaw set with the exact paired module.
for bid in range(1,10):
 side='port' if bid<5 else 'starboard';idx=bid%5
 for r in list(parts):
  n=r['semantic_id']
  if n.startswith(P+f'dock/protected/{bid}/') or f'/{side}/magazine/{side}/reserve/{idx}/dock support ' in n:delete(r)
 for r in protected:
  n=r['semantic_id'].replace('/dock/protected/0/',f'/dock/protected/{bid}/').replace('/port/magazine/port/reserve/0/',f'/{side}/magazine/{side}/reserve/{idx}/')
  clone(r,bid,n)
 for r in local:clone(r,bid)
 # External door and its physical casing move together to the outer tunnel end.
 for r in list(parts):
  n=r['semantic_id']
  if n.startswith(P+f'b{bid:02d}/') and any(tag in n for tag in ['/door ','/portal ','/upper portal pressure wall']):
   dz=-7.7 if bid<5 else 7.7;o=bpy.data.objects[n];o.location.y-=dz
   for b in r['bounds_body_m']:b[2]+=dz
  elif n==P+f'b{bid:02d}/floor':delete(r)
# Fixed deck crossmembers remain between every actual shaft, not through it.
intervals=[(-31.2,-29.55)]+[(-22.45+8*i,-21.55+8*i) for i in range(4)]+[(9.55,18.)]
for side,sign in [('port',-1),('starboard',1)]:
 for i,(xa,xb) in enumerate(intervals):
  za,zb=sorted((sign*33.6,sign*26.1));box(f'{side}/parallel/deck crossmember {i}',[xa,2.85,za],[xb,3.,zb])
  za,zb=sorted((sign*26.1,sign*25.88));box(f'{side}/parallel/aperture crossmember {i}',[xa,2.85,za],[min(xb,10.2),3.,zb])
  za,zb=sorted((sign*33.3,sign*19.35));box(f'{side}/parallel/lower floor crossmember {i}',[xa,-2.95,za],[xb,-2.80,zb])
# Actual source-mounted selected crane camera and local amber warning hardware.
from carrier_local_devices import append_hardware
device_doc={'parts':parts,'mechanism_reference_poses':refs};append_hardware(device_doc);doc['mounted_loader_devices']=device_doc['mounted_loader_devices']
# Exact render-only batching, retaining a map for every logical collision solid.
batches={};unbatched=[]
for r in parts:
 if bpy.data.objects[r['semantic_id']].modifiers or '/devices/' in r['semantic_id']:
  assert bpy.data.objects[r['semantic_id']].matrix_world.to_3x3().determinant()>0
  r['render_semantic_id']=r['semantic_id'];unbatched.append(r)
 else:batches.setdefault((r.get('group'),r['material']),[]).append(r)
bpy.context.view_layer.update();deps=bpy.context.evaluated_depsgraph_get();batched_triangles=0;batch_proofs=[];expected_batches={}
for r in unbatched:
 obj=bpy.data.objects[r['semantic_id']];ev=obj.evaluated_get(deps);m=ev.to_mesh();m.calc_loop_triangles();mat=obj.matrix_world;nm=mat.to_3x3().inverted().transposed()
 positions=[tuple(sum(float(mat[i][j])*float(v.co[j]) for j in range(3))+float(mat[i][3]) for i in range(3)) for v in m.vertices]
 expected_batches[obj.name]=(np.array([[positions[k] for k in tri.vertices] for tri in m.loop_triangles]),np.array([tuple((nm@m.corner_normals[k].vector).normalized()) for tri in m.loop_triangles for k in tri.loops]));ev.to_mesh_clear()
for index,((group,material),rows) in enumerate(sorted(batches.items(),key=lambda x:str(x[0]))):
 vertices=[];faces=[];normals=[];smooth=[];objects=[]
 for r in rows:
  before_faces=len(faces)
  obj=bpy.data.objects[r['semantic_id']];objects.append(obj);ev=obj.evaluated_get(deps);m=ev.to_mesh();m.calc_loop_triangles();offset=len(vertices);mat=obj.matrix_world;normal_mat=mat.to_3x3().inverted().transposed();reverse=mat.to_3x3().determinant()<0
  vertices.extend(tuple(sum(float(mat[i][j])*float(v.co[j]) for j in range(3))+float(mat[i][3]) for i in range(3)) for v in m.vertices)
  for tri in m.loop_triangles:
   ids=list(tri.vertices);loops=list(tri.loops)
   if reverse:ids.reverse();loops.reverse()
   body_positions=[(vertices[offset+i][0],vertices[offset+i][2],-vertices[offset+i][1]) for i in ids]
   rotation=min(range(3),key=lambda i:body_positions[i:]+body_positions[:i])
   if runtime_cross_squared(body_positions[rotation:]+body_positions[:rotation])<=f32(1e-20):continue
   faces.append(tuple(offset+i for i in ids));normals.extend(tuple((normal_mat@m.corner_normals[i].vector).normalized()) for i in loops);smooth.append(m.polygons[tri.polygon_index].use_smooth)
  r['render_source_matrix_blender']=[list(row) for row in mat]
  r['evaluated_triangle_count']=len(faces)-before_faces
  part_positions=np.array([[vertices[k] for k in tri] for tri in faces[before_faces:]],dtype=np.float64)
  part_normals=np.array(normals[before_faces*3:],dtype=np.float64)
  r['evaluated_geometry_sha256']=hashlib.sha256(part_positions.tobytes()+part_normals.tobytes()).hexdigest()
  ev.to_mesh_clear()
 name=P+f'render_batch/{index:04d}';mesh=bpy.data.meshes.new(name);mesh.from_pydata(vertices,[],faces);mesh.update();mesh.materials.append(bpy.data.materials[material])
 for polygon,value in zip(mesh.polygons,smooth):polygon.use_smooth=value
 mesh.normals_split_custom_set(normals)
 expected_batches[name]=(np.array([[vertices[k] for k in tri] for tri in faces]),np.array(normals))
 obj=bpy.data.objects.new(name,mesh);bpy.context.collection.objects.link(obj);obj['semantic_id']=name
 if group:obj['kinematic_group']=group
 for old in objects:bpy.data.objects.remove(old,do_unlink=True)
 for r in rows:r['render_semantic_id']=name
 batched_triangles+=len(faces)
 if index%100==0:print('evaluated batches',index,'triangles',batched_triangles,flush=True)
doc.update(status='UNACCEPTED ten independent paired local reloaders; aligned routes only',parts=parts,mechanism_reference_poses=refs,parallel_local_cells=[{'cell_id':i,'ready_berth':i,'protected_slot':i,'buffer_id':i,'loader_id':i,'capacity_added':0,'same_column_only':True} for i in range(10)],render_batch_count=len(batches),unbatched_modified_parts=len(unbatched),batched_triangles=batched_triangles,logical_part_count=len(parts),parent_source_sha256=doc['output_sha256'],removed_obsolete_rack=removed_obsolete_rack)
bpy.ops.wm.save_as_mainfile(filepath=str(a.output),check_existing=False)
# Reload serialized source and independently extract every batched triangle/normal.
bpy.ops.wm.open_mainfile(filepath=str(a.output),load_ui=False,use_scripts=False)
for name,(before_positions,before_normals) in expected_batches.items():
 obj=bpy.data.objects[name];ev=obj.evaluated_get(bpy.context.evaluated_depsgraph_get());mesh=ev.to_mesh();mesh.calc_loop_triangles();mat=obj.matrix_world;nm=mat.to_3x3().inverted().transposed();positions=[tuple(sum(float(mat[i][j])*float(v.co[j]) for j in range(3))+float(mat[i][3]) for i in range(3)) for v in mesh.vertices];after_positions=np.array([[positions[k] for k in tri.vertices] for tri in mesh.loop_triangles]);after_normals=np.array([tuple((nm@mesh.corner_normals[k].vector).normalized()) for tri in mesh.loop_triangles for k in tri.loops]);ev.to_mesh_clear()
 assert before_positions.shape==after_positions.shape and before_normals.shape==after_normals.shape
 position_error=float(np.max(abs(before_positions-after_positions)));normal_error=float(np.max(abs(before_normals-after_normals)));assert position_error<1e-5 and normal_error<5e-4,(name,position_error,normal_error)
 batch_proofs.append({'render_semantic_id':name,'triangles':len(after_positions),'position_max_error_m':position_error,'normal_max_error':normal_error,'pre_geometry_sha256':hashlib.sha256(before_positions.tobytes()+before_normals.tobytes()).hexdigest(),'post_geometry_sha256':hashlib.sha256(after_positions.tobytes()+after_normals.tobytes()).hexdigest()})
doc['output_sha256']=hashlib.sha256(a.output.read_bytes()).hexdigest();a.out_manifest.write_text(json.dumps(doc,indent=2));a.out_manifest.with_suffix('.batch-proof.json').write_text(json.dumps({'source_sha256':doc['parent_source_sha256'],'output_sha256':doc['output_sha256'],'scope':'evaluated modifiers and reflected world-space triangle/normal/material-group preservation after save/reload; no gameplay admission','batches':batch_proofs},indent=2));print(doc['output_sha256'],len(parts),len(batches),len(refs))
