"""Supplement only: position-only exact float32 corner matching for eight tolerance-ambiguous slivers."""
import pathlib,json,hashlib,importlib.util,itertools,numpy as np
OUT=pathlib.Path('/workspace/arkline-runtime/study05-audit-v2');D=OUT/'normals/diagnose.py';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();spec=importlib.util.spec_from_file_location('study05_diagnostic',D);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
before={'source':sha(m.BLEND),'glb':sha(m.GLB)};assert before['source']==m.EXPECTED['blend'] and before['glb']==m.EXPECTED['glb']
r=json.loads((OUT/'normals/report.json').read_text());targets=r['correspondence'][0]['remaining_unmatched_glb_examples'];assert len(targets)==8
s=m.read_master();g=m.read_glb();results=[]
for target in targets:
 gi=target['triangle'];gp=g['p'][gi].astype('<f4');candidates=[]
 for si,owner in enumerate(s['obj']):
  if owner!='Study05 roof aft access cover' or s['mat'][si]!=g['mat'][gi]:continue
  sp=s['p'][si].astype('<f4')
  for perm in itertools.permutations(range(3)):
   if np.array_equal(sp[list(perm)],gp):candidates.append((si,perm))
 assert len(candidates)==1,(gi,candidates)
 si,perm=candidates[0];inversions=sum(perm[i]>perm[j] for i in range(3) for j in range(i+1,3));assert inversions%2==0
 sn=s['n'][si][list(perm)];gn=g['n'][gi];dots=np.sum(sn*gn,axis=1);assert np.all(dots>=.999999)
 results.append({'glb_triangle':gi,'source_triangle':si,'source_object':s['obj'][si],'source_material':s['mat'][si],'exact_float32_position_candidate_count':len(candidates),'source_corner_permutation':list(perm),'same_winding':True,'corner_normal_dots':dots.tolist(),'source_unquantized_positions_body_m':s['p'][si].tolist(),'glb_positions_body_m':gp.tolist(),'maximum_prequantization_position_error_m':float(np.linalg.norm(s['p'][si][list(perm)]-g['p'][gi],axis=1).max())})
after={'source':sha(m.BLEND),'glb':sha(m.GLB)};assert before==after
report={'schema':'pioneer-study05-thin-corner-position-only-supplement/v1','outcome':'All eight tolerance-ambiguous retained triangles have one exact float32-position/source-part/material candidate, same winding and matching corner normals. This supplements rather than changes the original strict tolerance counts.','method':'Evaluate saved source using the unchanged peer-reviewed diagnostic read_master and GLB reader. Restrict candidates to source object named in receipt for each retained GLB span; compare only source positions independently cast to IEEE float32 against decoded GLB positions under all six corner permutations. Require exactly one positional candidate, then check winding parity and corner-normal dot independently. Normals do not select candidates.','models_before':before,'models_after':after,'models_unchanged':True,'autoexecution_disabled':not m.bpy.context.preferences.filepaths.use_scripts_auto_execute,'script_sha256':sha(pathlib.Path(__file__)),'reused_diagnostic_sha256':sha(D),'triangles':results,'limits':['This exact float32 position comparison is a separate method for the eight named thin faces; strict10/20/100micrometre audit still reports103458triangles and8excluded corner-permutation ambiguities.','Restricting to the receipt-mapped source object adds source-part provenance to the positional condition. No claim about full topology equivalence or GPU/native appearance.','No source/model/export edits, saves, re-export, builds or publication.']}
p=OUT/'thin-corner-supplement.json';p.write_text(json.dumps(report,indent=2)+'\n');print('PASS8samewindingpositionuniquematch; minimumdot',min(min(t['corner_normal_dots']) for t in results),'unchanged',before==after,'reportsha',sha(p))
