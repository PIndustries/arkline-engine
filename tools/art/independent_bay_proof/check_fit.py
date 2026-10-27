import json,sys,hashlib
from pathlib import Path
m=Path(sys.argv[1]);d=json.loads(m.read_text());parts=d['parts'];base=json.loads(Path('pioneer-independent-bays-20261026/source-part-bounds.json').read_text())['parts']
def overlap(a,b):return all(min(a[1][i],b[1][i])-max(a[0][i],b[0][i])>1e-7 for i in range(3))
def moved(a,v):return [[p[i]+v[i] for i in range(3)] for p in a]
def union(a,b):return [[min(a[0][i],b[0][i]) for i in range(3)],[max(a[1][i],b[1][i]) for i in range(3)]]
def vector(b,p):return [sum(b[k][i]*p[j] for j,k in enumerate(['forward','up','right'])) for i in range(3)]
def body(b,x):
 c=[b['center_m'][i]+b['forward'][i]*x for i in range(3)]
 h=[sum(abs(b[k][i])*v for k,v in [('forward',2.5),('up',.9),('right',3.)]) for i in range(3)]
 return [[c[i]-h[i] for i in range(3)],[c[i]+h[i] for i in range(3)]]
fixed=[(p['semantic_id'],p['bounds_body_m']) for p in parts if p['group'] is None]+[(p['name'],p['bounds']) for p in base]
hits=[];checks=0
for bay in d['bays']:
 root=f"b{bay['bay_id']:02d}"
 for p in parts:
  if p['group'] not in (root+'/door/upper',root+'/door/lower'):continue
  dy=(3.1 if bay['class']=='recon' else (1.35 if p['group'].endswith('upper') else -1.8))
  swept=union(p['bounds_body_m'],moved(p['bounds_body_m'],vector(bay,[0,dy,0])))
  for name,bounds in fixed:
   checks+=1
   if overlap(swept,bounds):hits.append({'kind':'door/fixed','bay':bay['bay_id'],'moving':p['semantic_id'],'fixed':name})
 # Every neighbouring cell's fixed/stowed hardware must clear full outward
 # padded body corridor, not merely the target's own box or endpoint.
 corridor=union(body(bay,0),body(bay,16));corridor=[[v+(-.25 if e==0 else .25) for v in row] for e,row in enumerate(corridor)]
 for p in parts:
  if not p['semantic_id'].startswith('pioneer.independent_bay/b') or p['semantic_id'].startswith('pioneer.independent_bay/'+root+'/'):continue
  checks+=1
  if overlap(corridor,p['bounds_body_m']):hits.append({'kind':'peer departure corridor','bay':bay['bay_id'],'part':p['semantic_id']})
# Each whole route domain must be disjoint from every OTHER drone lane.
for i,a in enumerate(d['bays']):
 for b in d['bays'][i+1:]:
  aa=union(body(a,0),body(a,16));bb=union(body(b,0),body(b,16));checks+=1
  if overlap(aa,bb):hits.append({'kind':'drone lane pair','a':a['bay_id'],'b':b['bay_id']})
out={'scope':'conservative source-box screening, not final triangle/socket/crew/runtime proof','manifest_sha256':hashlib.sha256(m.read_bytes()).hexdigest(),'checks':checks,'failures':hits}
Path(str(m).replace('.json','-screening.json')).write_text(json.dumps(out,indent=2));print(json.dumps(out,indent=2));sys.exit(bool(hits))
