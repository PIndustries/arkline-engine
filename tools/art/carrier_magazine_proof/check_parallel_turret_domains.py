import sys,json,hashlib,itertools,pathlib,numpy as np
root=pathlib.Path.cwd();sys.path.insert(0,str(root/'arkline-independent-bay-hull/tools/art/carrier_magazine_proof'));sys.path.insert(0,str(root/'arkline-turret-stow/tools/art'))
from parallel_bank_routes import stage,bay,translation
from check_turret_well_clearance import parts
manifest=root/'pioneer-parallel-reload-20261026/bank-v7.json';d=json.loads(manifest.read_text());refs=d['mechanism_reference_poses'];groupboxes={g:np.array([np.min([p['bounds_body_m'][0] for p in d['parts'] if p['group']==g],0),np.max([p['bounds_body_m'][1] for p in d['parts'] if p['group']==g],0)]) for g in refs};turrets=[]
for slot in [0,1]:
 mount=np.array([10.8,9.864,(-1 if slot==0 else 1)*5.2]);radius=max(np.linalg.norm(np.array(c)-mount+np.array(v)*h) for _,c,h,R in list(parts(slot,1))[:8] for v in itertools.product([-1,1],repeat=3));b=np.array([mount-radius,mount+radius]);b[0,1]-=2
 other=[]
 for f in [0,.15,.75,1]:
  for name,c,h,R in list(parts(slot,f))[8:]:other +=[c-np.abs(R)@h,c+np.abs(R)@h]
 b=np.array([np.minimum(b[0],np.min(other,0)),np.maximum(b[1],np.max(other,0))]);turrets.append(b)
checks=0;hits=[];min_gap=1e9
for source in range(10):
 routes=[('stage',source,stage(d,source,source))]+[('launch',source,bay(d,source))]
 for kind,dest,frames in routes:
  for a,b in zip(frames,frames[1:]):
   domains=[]
   for g in a['owned_groups']:
    x=np.array(translation(d,g,a['state']));y=np.array(translation(d,g,b['state']));domains.append((g,groupboxes[g]+np.array([np.minimum(x,y),np.maximum(x,y)])))
   loaded=source+10 if kind=='stage' else source
   x=np.array(a.get('drone_position',a['state']['cassettes'][str(loaded)]));y=np.array(b.get('drone_position',b['state']['cassettes'][str(loaded)]));domains.append(('drone',np.array([np.minimum(x,y)-[3,.9,2.5],np.maximum(x,y)+[3,.9,2.5]])))
   for name,box in domains:
    for slot,t in enumerate(turrets):
     checks+=1;gap=np.maximum(box[0]-t[1],t[0]-box[1]);min_gap=min(min_gap,float(np.max(gap)))
     if np.all(gap<0):hits.append([kind,source,dest,a['label'],name,slot])
out={'scope':'Conservative arbitrary-timing full turret rigid-body rotation/drop/shutter union versus every10alignedStage+10Launch hardware and full drone translation domain; no physics/strength claim','manifest_sha256':hashlib.sha256(manifest.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256((root/'arkline-independent-bay-hull/tools/art/carrier_magazine_proof/parallel_bank_routes.py').read_bytes()).hexdigest(),'composed_glb_sha256':hashlib.sha256((root/'pioneer-parallel-reload-20261026/bank-v7-export/pioneer.glb').read_bytes()).hexdigest(),'turret_source_sha256':hashlib.sha256((root/'arkline-turret-stow/tools/art/check_turret_well_clearance.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),'turret_union_bounds_m':[x.tolist() for x in turrets],'domain_pairs':checks,'minimum_separating_axis_gap_m':min_gap,'failures':hits};p=root/'pioneer-parallel-reload-20261026/bank-v7-turret-domains.json';p.write_text(json.dumps(out,indent=2));print(out)
assert not hits
