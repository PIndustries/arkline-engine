"""Initial authored reference contacts: different moving groups vs all source boxes.
Same rigid group and fixed/fixed joins are intentionally not a strength proof.
"""
import argparse,hashlib,json,sys
from pathlib import Path
import numpy as np
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args();doc=json.loads(a.manifest.read_text());base=json.loads(Path('pioneer-independent-bays-20261026/source-part-bounds.json').read_text());rows=doc['parts']+[{'semantic_id':r['name'],'group':None,'bounds_body_m':r['bounds']} for r in base['parts']]
b=np.array([r['bounds_body_m'] for r in rows]);g=np.array([r['group'] or '' for r in rows]);lo=b[:,0]+1e-5;hi=b[:,1]-1e-5;hits=[];checks=0
for i in np.flatnonzero(g!=''):
 eligible=(g!=g[i])&((np.arange(len(rows))>i)|(g==''));checks+=int(eligible.sum());overlap=eligible&np.all(hi[i]>lo,axis=1)&np.all(lo[i]<hi,axis=1)
 for j in np.flatnonzero(overlap):hits.append({'a':rows[i]['semantic_id'],'b':rows[j]['semantic_id']})
r={'scope':'initial different dynamic-group/fixed authored-box overlaps, same-rigid and fixed/fixed structural joins excluded','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'source_hull_sha256':base['sha256'],'pairs':checks,'failures':hits};a.output.write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));sys.exit(bool(hits))
