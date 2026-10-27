import json,sys,hashlib,itertools
from pathlib import Path
import numpy as np
from candidate_routes import ROUTES,translation,body_vector
p=Path(sys.argv[1]);d=json.loads(p.read_text());bid=int(sys.argv[2]);bay=next(b for b in d['bays'] if b['bay_id']==bid)
raw=Path('pioneer-independent-bays-20261026/actual-drone-meshes.json');mesh=json.loads(raw.read_text());row=next(r for r in mesh['classes'] if r['class']==bay['class']);tris=np.array([v['position_m'] for b in row['batches'] for v in b['vertices']]).reshape(-1,3,3)
lo_t=tris.min(1);hi_t=tris.max(1)
base=json.loads(Path('pioneer-independent-bays-20261026/source-part-bounds.json').read_text())['parts']
parts=d['parts']+[{'semantic_id':v['name'],'group':None,'bounds_body_m':v['bounds']} for v in base]
axes=np.array([bay['forward'],bay['up'],bay['right']],float);center=np.array(bay['center_m'])
def local(bounds):
 corners=np.array(list(itertools.product(*zip(*bounds))));q=(corners-center)@axes.T
 return np.array([q.min(0),q.max(0)])
def sat(t,lo,hi):
 c=(lo+hi)/2;h=(hi-lo)/2;t=t-c;e=np.roll(t,-1,axis=1)-t
 axes=[np.broadcast_to(a,(len(t),3)) for a in np.eye(3)]+[np.cross(e[:,0],e[:,1])]+[np.cross(e[:,i],a) for i in range(3) for a in np.eye(3)]
 hit=np.ones(len(t),bool)
 for a in axes:
  q=np.einsum('nij,nj->ni',t,a);r=np.abs(a)@h;hit&=(q.min(1)<=r+1e-10)&(q.max(1)>=-r-1e-10)
 return hit.any()
def winding(point, surface=tris):
 v=surface-point;a,b,c=v[:,0],v[:,1],v[:,2];la=np.linalg.norm(a,axis=1);lb=np.linalg.norm(b,axis=1);lc=np.linalg.norm(c,axis=1)
 num=np.einsum('ij,ij->i',a,np.cross(b,c));den=la*lb*lc+np.einsum('ij,ij->i',a,b)*lc+np.einsum('ij,ij->i',b,c)*la+np.einsum('ij,ij->i',c,a)*lb
 return abs(np.arctan2(num,den).sum()/(2*np.pi))
# Negative control: an oblique closed octahedron encloses a small box; no surface intersects it.
vertices=np.array([[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]],float)
faces=[[0,2,4],[2,1,4],[1,3,4],[3,0,4],[2,0,5],[1,2,5],[3,1,5],[0,3,5]]
closed=vertices[faces];test_lo=np.array([-.1]*3);test_hi=-test_lo
control_mask=(closed.max(1)>=test_lo).all(1)&(closed.min(1)<=test_hi).all(1)
assert control_mask.any() and not sat(closed[control_mask],test_lo,test_hi)
assert winding(np.zeros(3),closed)>.99, 'enclosed oblique solid must be detected'
local_bounds=[local(r['bounds_body_m']) for r in parts];hits=[];checks=0
for action,states in ROUTES.items():
 if action=='return_to_unload' and not bay['ore_service_supported']:continue
 for phase,(a,b) in enumerate(zip(states,states[1:])):
  for part,bb in zip(parts,local_bounds):
   va=np.array(translation(part['group'],bay,a['controls']))-np.array([a['drone_x'],0,0]);vb=np.array(translation(part['group'],bay,b['controls']))-np.array([b['drone_x'],0,0]);lo=np.minimum(bb[0]+va,bb[0]+vb)+1e-5;hi=np.maximum(bb[1]+va,bb[1]+vb)-1e-5
   checks+=1
   if np.any(hi<=lo):continue
   # Entire articulated Mining head is conservatively enclosed by this actual model bound.
   if bay['class']=='mining':
    head_center=np.array([2.,-.1,0.]);nearest=np.maximum(lo,np.minimum(hi,head_center))
    if np.linalg.norm(nearest-head_center)<.49-1e-5:hits.append({'action':action,'phase':phase,'part':part['semantic_id'],'reason':'articulated-head sphere'})
   mask=(hi_t>=lo).all(1)&(lo_t<=hi).all(1)
   if (mask.any() and sat(tris[mask],lo,hi)) or ((lo<tris.max((0,1))).all() and (hi>tris.min((0,1))).all() and winding((lo+hi)/2)>.5):hits.append({'action':action,'phase':phase,'part':part['semantic_id'],'bounds':[lo.tolist(),hi.tolist()]})
out={'scope':'actual cached neutral-body triangles vs continuous conservative relative authored-box domains;10um inward tolerance for intended tangent contacts; no arbitrary contact exclusion','manifest_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'mesh_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),'route_source_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'mining_head_scope':'full radius0.49m articulation bound around[2,-0.1,0]','containment_negative_control':'enclosed oblique octahedron rejected','bay_id':bid,'checks':checks,'failures':hits}
Path(str(p).replace('.json',f'-body-b{bid:02d}.json')).write_text(json.dumps(out,indent=2));print(json.dumps(out,indent=2));sys.exit(bool(hits))
