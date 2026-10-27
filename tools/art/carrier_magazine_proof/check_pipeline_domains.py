"""Conservative arbitrary-timing Stage vs other ready-berth Launch domains."""
import argparse,json,hashlib
from pathlib import Path
import numpy as np
from candidate_routes import stage,bay,translation
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args();doc=json.loads(a.manifest.read_text());fail=[];checks=0
boxes={}
for g in doc['mechanism_reference_poses']:
 b=np.array([p['bounds_body_m'] for p in doc['parts'] if p['group']==g]);boxes[g]=np.array([b[:,0].min(0),b[:,1].max(0)])
def domains(frames,loaded):
 result=[]
 for f0,f1 in zip(frames,frames[1:]):
  for g in f0['owned_groups']:
   d0=np.array(translation(doc,g,f0['state']));d1=np.array(translation(doc,g,f1['state']))
   # An unchanged reference group is already part of every route's source-static proof.
   if np.all(abs(d0)<1e-8) and np.all(abs(d1)<1e-8):continue
   result.append((g,np.array([boxes[g][0]+np.minimum(d0,d1),boxes[g][1]+np.maximum(d0,d1)])))
  p0=np.array(f0.get('drone_position',f0['state']['cassettes'][str(loaded)]));p1=np.array(f1.get('drone_position',f1['state']['cassettes'][str(loaded)]));result.append(('drone',np.array([np.minimum(p0,p1)-[3,.9,2.5],np.maximum(p0,p1)+[3,.9,2.5]])))
 return result
launches={i:domains(bay(doc,i),i) for i in range(10)}
for source in range(10):
 for dest in range((source//5)*5,(source//5)*5+5):
  ss=domains(stage(doc,source,dest),source+10)
  for other in range((source//5)*5,(source//5)*5+5):
   if other==dest:continue # atomically reserved destination forbids own overlap
   bb=np.array([r[1] for r in launches[other]])
   for name,b in ss:
    checks+=len(bb);hits=np.flatnonzero(np.all(np.minimum(b[1],bb[:,1])-np.maximum(b[0],bb[:,0])>1e-5,axis=1))
    for h in hits:fail.append({'source':source,'destination':dest,'other_berth':other,'stage_group':name,'launch_group':launches[other][int(h)][0],'stage_bounds':b.tolist(),'launch_bounds':bb[int(h)].tolist()})
    if len(fail)>20:break
   if len(fail)>20:break
  if len(fail)>20:break
 if len(fail)>20:break
r={'scope':'arbitrary relative timing conservative group/body segment unions Stage vs other same-side Launch; own target reservation required; no global wave barrier','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'pair_domains':checks,'failures':fail};a.output.write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));raise SystemExit(bool(fail))
