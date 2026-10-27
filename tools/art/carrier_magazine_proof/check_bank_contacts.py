"""Every exact replicated cell contact plus logical-to-render owner/material binding."""
import argparse,copy,hashlib,json,subprocess,sys,tempfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('export_receipt',type=Path);p.add_argument('output',type=Path);a=p.parse_args();doc=json.loads(a.manifest.read_text());ex=json.loads(a.export_receipt.read_text());checker=Path(__file__).with_name('check_parallel_contacts.py');fail=[];results=[]
byname={r['name']:r for r in ex['source_parts']};owners={}
for r in doc['parts']:
 render=r['render_semantic_id'];assert render in byname
 assert byname[render]['material']==r['material']
 value=(r.get('group'),r['material']);assert owners.setdefault(render,value)==value
assert doc['output_sha256']==ex['source_sha256']
with tempfile.TemporaryDirectory() as folder:
 for bid in range(10):
  rows=[];dx=8*(bid%5);sign=1 if bid<5 else -1
  for original in doc['parts']:
   name=original['semantic_id']
   if not (name.startswith(f'pioneer.magazine_ready/cell{bid:02d}/') or name.startswith(f'pioneer.magazine_ready/c{bid:02d}/')):continue
   r=copy.deepcopy(original);r['semantic_id']=name.replace(f'/cell{bid:02d}/','/cell00/').replace(f'/c{bid:02d}/','/c00/')
   if bid>=5 and '/c00/fixed support ' in r['semantic_id']:
    r['semantic_id']=r['semantic_id'].replace(' port ',' TEMP ').replace(' starboard ',' port ').replace(' TEMP ',' starboard ').replace(' left',' TEMP').replace(' right',' left').replace(' TEMP',' right')
   if r.get('group'):r['group']=r['group'].replace(f'cell{bid:02d}/','cell00/').replace(f'c{bid:02d}/','c00/')
   lo,hi=r['bounds_body_m'];r['bounds_body_m']=[[lo[0]-dx,lo[1],lo[2] if sign==1 else -hi[2]],[hi[0]-dx,hi[1],hi[2] if sign==1 else -lo[2]]];rows.append(r)
  source=Path(folder)/f'cell{bid}.json';out=Path(folder)/f'cell{bid}-result.json';source.write_text(json.dumps({'parts':rows}));run=subprocess.run([sys.executable,str(checker),str(source),str(out)],capture_output=True,text=True)
  result=json.loads(out.read_text());results.append({'cell':bid,'checks':result['checks'],'canonicalized_source_sha256':result['manifest_sha256'],'failures':result['failures']});fail.extend([{'cell':bid,'reason':x} for x in result['failures']])
r={'scope':'all ten exact reflected source interfaces/retention; logical/render owner+material mapping; no strength/runtime claim','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'export_receipt_sha256':hashlib.sha256(a.export_receipt.read_bytes()).hexdigest(),'glb_sha256':ex['output']['sha256'],'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'cell_checker_sha256':hashlib.sha256(checker.read_bytes()).hexdigest(),'logical_parts':len(doc['parts']),'resolved_render_parts':len(owners),'checks':sum(x['checks'] for x in results),'cells':results,'failures':fail};a.output.write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));sys.exit(bool(fail))
