import json,sys,itertools,hashlib
from pathlib import Path
p=Path(sys.argv[1]);d=json.loads(p.read_text());bay=next(b for b in d['bays'] if b['bay_id']==int(sys.argv[2]));root=f"b{bay['bay_id']:02d}/"
parts=d['parts']; base=json.loads(Path('pioneer-independent-bays-20261026/source-part-bounds.json').read_text())['parts']
fixed=[{'semantic_id':v['name'],'group':None,'bounds_body_m':v['bounds']} for v in base]
allparts=parts+fixed
from candidate_routes import ROUTES,translation,body_vector
def velocity(group,s):return body_vector(bay,translation(group,bay,s))
def shifted(b,v):return [[x+v[i] for i,x in enumerate(r)] for r in b]
def sweep_overlap(a,va,b,vb):
 lo,hi=0.,1.
 for i in range(3):
  v=va[i]-vb[i];amin,amax=a[0][i],a[1][i];bmin,bmax=b[0][i],b[1][i]
  if abs(v)<1e-12:
   if min(amax,bmax)-max(amin,bmin)<=1e-7:return False
  else:
   t0=(bmin-amax+1e-7)/v;t1=(bmax-amin-1e-7)/v
   lo=max(lo,min(t0,t1));hi=min(hi,max(t0,t1))
   if hi<=lo:return False
 return hi>lo
phases=[]
for action,states in ROUTES.items():
 if action=='return_to_unload' and not bay['ore_service_supported']:continue
 for i,(a,b) in enumerate(zip(states,states[1:])):
  phases.append((f'{action}/{i}',a['controls'],b['controls']))
  # Same swept-domain proof covers interrupted reverse cleanup of every accepted segment.
  phases.append((f'{action}/{i}/reverse-cleanup',b['controls'],a['controls']))
hits=[];checks=0
moving=[i for i,v in enumerate(allparts) if v['group'] and v['group'].startswith(root)]
for phase,s0,s1 in phases:
 v0=[velocity(x['group'],s0) for x in allparts];v1=[velocity(x['group'],s1) for x in allparts]
 bounds=[shifted(x['bounds_body_m'],v) for x,v in zip(allparts,v0)];deltas=[[b-a for a,b in zip(x,y)] for x,y in zip(v0,v1)]
 for i in moving:
  for j in range(len(allparts)):
   if i==j or (j in moving and j<i) or allparts[i]['group']==allparts[j]['group']:continue
   checks+=1
   if sweep_overlap(bounds[i],deltas[i],bounds[j],deltas[j]):
    hits.append({'phase':phase,'a':allparts[i]['semantic_id'],'b':allparts[j]['semantic_id']})
out={'scope':'continuous same-time conservative authored-box mechanism screening; body socket/contact checked separately','source_manifest_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'route_source_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'coverage':'all semantic route segments, loaded clamp-closed strokes, each reversed swept domain; other lanes frozen','bay_id':bay['bay_id'],'checks':checks,'failure_count':len(hits),'failures':hits[:100]}
Path(str(p).replace('.json',f'-motion-b{bay["bay_id"]:02d}.json')).write_text(json.dumps(out,indent=2));print(json.dumps(out,indent=2));sys.exit(bool(hits))
