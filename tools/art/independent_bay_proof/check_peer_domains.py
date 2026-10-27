import json,itertools,sys,hashlib
from pathlib import Path
import numpy as np
from candidate_routes import ROUTES,translation,body_vector
p=Path(sys.argv[1]);m=json.loads(p.read_text());domains={};counts={};hits=[];checks=0
for bay in m['bays']:
 root=f"b{bay['bay_id']:02d}/";rows=[]
 states=[s for action,ss in ROUTES.items() if action!='return_to_unload' or bay['ore_service_supported'] for s in ss]
 for part in m['parts']:
  if not part['group'] or not part['group'].startswith(root):continue
  shifts=np.array([body_vector(bay,translation(part['group'],bay,s['controls'])) for s in states]);bb=np.array(part['bounds_body_m'])
  rows.append((part['semantic_id'],bb[0]+shifts.min(0),bb[1]+shifts.max(0)))
 # Full body envelope for any time/progress; no socket carve for other lanes.
 corners=np.array(list(itertools.product([-2.5,18.5],[-.9,.9],[-3,3])))
 pts=np.array([body_vector(bay,c) for c in corners])+bay['center_m'];rows.append(('drone/full-route',pts.min(0),pts.max(0)))
 domains[bay['bay_id']]=rows
for a,b in itertools.combinations(domains,2):
 for na,al,ah in domains[a]:
  for nb,bl,bh in domains[b]:
   checks+=1
   if np.all(np.minimum(ah,bh)-np.maximum(al,bl)>1e-7):hits.append({'a':a,'b':b,'part_a':na,'part_b':nb})
report={'scope':'all pairwise moving hardware and full drone unions over every accepted candidate phase; independent clocks/progress covered conservatively, no contact exclusions','manifest_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'route_source_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'checks':checks,'failures':hits}
Path(str(p).replace('.json','-peer-domains.json')).write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2));sys.exit(bool(hits))
