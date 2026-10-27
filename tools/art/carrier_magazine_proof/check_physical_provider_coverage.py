"""Actual final exported triangles covered by all dynamic collision leaves.
Uses exact full triangle-minus-union on unresolved cases, never vertex-only fit.
"""
import argparse,hashlib,json
from pathlib import Path
import numpy as np
import build_parallel_provider_fixture as base
from physical_provider.source import glb_primitives,sha,require,occupied_cells,export_guard
from physical_provider.coverage import triangle_union_covers
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',type=Path,required=True);p.add_argument('--fixture',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
raw=a.fixture.read_bytes();fixture=json.loads(raw);manifest=json.loads((a.root/'final-integration-proof/assembly-manifest.json').read_text());inputs=manifest['inputs']
for role in ['ship_glb','ship_manifest','ship_export_receipt','receiver_module']:
 require(sha(a.root/inputs[role]['path'])==inputs[role]['sha256'],'source mismatch')
doc=json.loads((a.root/inputs['ship_manifest']['path']).read_text());receipt=json.loads((a.root/inputs['ship_export_receipt']['path']).read_text());module=json.loads((a.root/inputs['receiver_module']['path']).read_text());tri=glb_primitives(a.root/inputs['ship_glb']['path']);by={r['name']:r for r in receipt['source_parts']};mods={r['id']:r for r in module['parts']};groups={g:[]for g in doc['mechanism_reference_poses']};names={g:set()for g in groups}
for part in doc['parts']:
 g=part['group']
 if g:names[g].add(part['render_semantic_id'])
# Verify the actual serialized runtime local leaves, not a regenerated proxy.
groups={r['semantic_id']:[p['local_bounds_m']for p in r['parts']]for r in fixture['mechanism_collision_parts']}
require(set(groups)==set(doc['mechanism_reference_poses']), 'complete dynamic root coverage')
fail=[];exact=0;covered=0;total=0
for g,boxes in groups.items():
 b=np.array(boxes); triangles=np.concatenate([tri[by[n]['render_primitive']][by[n]['first_index']//3:(by[n]['first_index']+by[n]['index_count'])//3]for n in sorted(names[g])]);total+=len(triangles)
 ref=doc['mechanism_reference_poses'][g];triangles=(triangles-np.array(ref['position_m']))@base.axes(ref['orientation_xyzw'])
 for i,t in enumerate(triangles):
  lo=t.min(0);hi=t.max(0)
  if np.any(np.all(b[:,0]<=lo,axis=1)&np.all(b[:,1]>=hi,axis=1)):
   covered+=1;continue
  relevant=b[np.all(b[:,0]<=hi,axis=1)&np.all(b[:,1]>=lo,axis=1)];exact+=1
  if triangle_union_covers(t,relevant):covered+=1
  else:fail.append({'group':g,'triangle':i,'positions':t.tolist(),'relevant_boxes':len(relevant)})
# Same exact path: a triangle whose vertices all lie in a disjoint union still
# exposes its nonzero middle area. A hollow passage remains empty.
assert not triangle_union_covers([[0,0,0],[3,0,0],[0,3,0]],[[[-.1,-.1,-.1],[.1,.1,.1]],[[2.9,-.1,-.1],[3.1,.1,.1]],[[-.1,2.9,-.1],[.1,3.1,.1]]])
assert triangle_union_covers([[0,0,0],[1,0,0],[0,1,0]],[[[0,0,0],[.5,1,1]],[[.5,0,0],[1,1,1]]])
r={'status':'PASS'if not fail else'FAIL','fixture_sha256':hashlib.sha256(raw).hexdigest(),'ship_glb_sha256':inputs['ship_glb']['sha256'],'total_dynamic_triangles':total,'covered':covered,'exact_union_tests':exact,'failures':fail,'scope':'Actual exported dynamic triangles transformed to exact source rigid root; full serialized local-cell union coverage, no query shrink','checker_sha256':sha(Path(__file__))}
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items()if k!='failures'}));print('first failures',fail[:2])
