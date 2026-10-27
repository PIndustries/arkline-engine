import json,itertools,hashlib,sys
from pathlib import Path
import numpy as np
from candidate_routes import ROUTES,fixtures,pose,translation,body_vector
root=Path('pioneer-independent-bays-20261026');m=json.loads((root/'fit-v12.json').read_text());export=json.loads((root/'export-v12/receipt.json').read_text());routes=fixtures(m)
reg={'schema_version':1,'host':{'geometry_revision':12,'source_sha256':'7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd'},'assembly_revision':1601,'assembly_source_sha256':export['output']['sha256']}
def union(boxes):return [np.min(np.array(boxes)[:,0,:],0).tolist(),np.max(np.array(boxes)[:,1,:],0).tolist()]
def local(bay,b):
 pts=np.array(list(itertools.product(*zip(*b))))-bay['center_m'];axes=np.array([bay['forward'],bay['up'],bay['right']]);pts=pts@axes.T;return [pts.min(0).tolist(),pts.max(0).tolist()]
source_by_name={r['name']:r for r in export['source_parts']};dynamic=[];bays=[];deployed=[]
for bay,route in zip(m['bays'],routes):
 groups=[];allbounds=[];states=[s for a,ss in ROUTES.items() if a!='return_to_unload' or bay['ore_service_supported'] for s in ss]
 for group in sorted(bay['groups'].values()):
  parts=[p for p in m['parts'] if p['group']==group];box=union([p['bounds_body_m'] for p in parts]);reference=pose(bay,[0,0,0]);groups.append({'semantic_id':group,'reference_pose':reference,'local_bounds_m':local(bay,box)})
  ranges=[source_by_name[p['semantic_id']] for p in parts];dynamic.append({'id':group,'reference_pose_body_m':{'translation':reference['position_m'],'rotation_xyzw':reference['orientation_xyzw']},'source_parts':[{k:r[k] for k in ['source_id','render_primitive','first_index','index_count']} for r in ranges]})
  for s in states:
   v=np.array(body_vector(bay,translation(group,bay,s['controls'])));bb=(np.array(box)+v).tolist();allbounds.append(bb);deployed.append(bb)
 pts=np.array([body_vector(bay,c) for c in itertools.product([-2.5,18.5],[-.9,.9],[-3,3])])+bay['center_m'];allbounds.append([pts.min(0).tolist(),pts.max(0).tolist()])
 bays.append({'bay_id':bay['bay_id'],'compatible_classes':[bay['class']],'preferred_home':bay['bay_id'],'ore_transfer_capable':bay['ore_service_supported'],'stored_pose':pose(bay,[0,0,0]),'support_pose':pose(bay,[5.9,0,0]),'contact_pose':pose(bay,[5.9,0,0]),'gate_pose':pose(bay,[16,0,0]),'clear_pose':pose(bay,[16,0,0]),'mechanisms':groups,'working_bounds_m':union(allbounds),'drone_bounds_local_m':[[-2.5,-.9,-3],[2.5,.9,3]]})
 for ss in route['routes'].values():
  for s in ss:s.pop('candidate_controls')
definition={'registration':reg,'bays':bays,'carrier_bounds_m':export['output']['body_bounds_m'],'deployed_hardware_bounds_m':union(deployed)}
fixture={'scope':'stationary fixture only; general world admission remains disabled','source_manifest_sha256':hashlib.sha256((root/'fit-v12.json').read_bytes()).hexdigest(),'route_generator_sha256':hashlib.sha256((root/'candidate_routes.py').read_bytes()).hexdigest(),'definition':definition,'routes':routes}
out=Path('arkline-independent-bay-hull/crates/arkline-protocol/src/independent_bay_geometry');out.mkdir(exist_ok=True);(out/'stationary_v1601.json').write_text(json.dumps(fixture,separators=(',',':'))+'\n')
dynamic_n=sum(r['index_count']//3 for g in dynamic for r in g['source_parts']);binding={'schema':'arkline.pioneer-drone-bay-runtime-binding/v1','registration_revision':1601,'glb_sha256':reg['assembly_source_sha256'],'source_triangle_count':export['output']['triangles'],'dynamic_triangle_count':dynamic_n,'static_triangle_count':export['output']['triangles']-dynamic_n,'dynamic_groups':dynamic};(root/'export-v12/runtime-binding.json').write_text(json.dumps(binding,indent=2));print(reg);print('groups',len(dynamic),'dynamic triangles',dynamic_n)
