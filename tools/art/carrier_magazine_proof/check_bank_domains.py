"""Full-route union separation proves arbitrary relative neighbor phases."""
import argparse,hashlib,itertools,json,sys
from pathlib import Path
import numpy as np
from parallel_bank_routes import stage,bay,translation
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args();d=json.loads(a.manifest.read_text());groupboxes={}
for g in d['mechanism_reference_poses']:
 b=np.array([r['bounds_body_m'] for r in d['parts'] if r.get('group')==g]);assert len(b);groupboxes[g]=np.array([b[:,0].min(0),b[:,1].max(0)])
def domains(fs,cid):
 rows=[]
 for g in fs[0]['owned_groups']:
  offsets=np.array([translation(d,g,f['state']) for f in fs])
  if np.all(np.ptp(offsets,axis=0)<1e-10):continue # unchanged source parts are included individually in each route's all-static sweep
  rows.append((g,np.array([groupboxes[g][0]+offsets.min(0),groupboxes[g][1]+offsets.max(0)])))
 body=np.array([f.get('drone_position',f['state']['cassettes'][str(cid)]) for f in fs]);rows.append(('drone',np.array([body.min(0)-[3.,.9,2.5],body.max(0)+[3.,.9,2.5]])))
 return rows
ss=[domains(stage(d,i,i),i+10) for i in range(10)];ll=[domains(bay(d,i),i) for i in range(10)];fail=[];checks=0;types={}
for kind,aa,bb,pairs in [('stage-stage',ss,ss,itertools.combinations(range(10),2)),('launch-launch',ll,ll,itertools.combinations(range(10),2)),('stage-launch',ss,ll,((i,j) for i in range(10) for j in range(10) if i!=j))]:
 count=0
 for i,j in pairs:
  right=np.array([r[1] for r in bb[j]])
  for name,b in aa[i]:
   checks+=len(right);count+=len(right);hits=np.flatnonzero(np.all(np.minimum(b[1],right[:,1])-np.maximum(b[0],right[:,0])>1e-5,axis=1))
   for hit in hits:fail.append({'kind':kind,'cell_a':i,'cell_b':j,'group_a':name,'group_b':bb[j][int(hit)][0],'a':b.tolist(),'b':right[int(hit)].tolist()})
 types[kind]=count
r={'scope':'full route group/body union separation, arbitrary independent phases; own target mutually exclusive by resource ownership','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'adapter_sha256':hashlib.sha256(Path(__file__).with_name('parallel_bank_routes.py').read_bytes()).hexdigest(),'canonical_route_sha256':hashlib.sha256(Path(__file__).with_name('parallel_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'pair_domains':checks,'kinds':types,'failures':fail};a.output.write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));sys.exit(bool(fail))
