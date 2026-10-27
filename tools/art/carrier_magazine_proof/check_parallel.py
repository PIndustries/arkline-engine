"""Exact translating authored-box relative sweeps; strict source-bound receipt.
No timing, inventory mutation or native acceptance is inferred from this proof.
"""
import argparse,hashlib,json,sys
from pathlib import Path
import numpy as np
from parallel_routes import stage,translation
from candidate_routes import bay
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);p.add_argument('--source',type=int,default=0);p.add_argument('--destination',type=int,default=0);p.add_argument('--bay',type=int);p.add_argument('--negative-closed-hatch',action='store_true');a=p.parse_args()
doc=json.loads(a.manifest.read_text());base_path=Path('pioneer-independent-bays-20261026/source-part-bounds.json');base=json.loads(base_path.read_text());rows=doc['parts']+[{'semantic_id':r['name'],'group':None,'bounds_body_m':r['bounds']} for r in base['parts']];bounds=np.array([r['bounds_body_m'] for r in rows]);groups=[r['group'] for r in rows];names=[r['semantic_id'] for r in rows]
frames=stage(doc,a.source,a.destination) if a.bay is None else bay(doc,a.bay);failures=[];checks=0;pairs=0
if a.negative_closed_hatch:
 for frame in frames:frame['state']['hatches']={}
# Shrink only10micrometres to distinguish intended plane contact from overlap.
tol=1e-5
for index,(f0,f1) in enumerate(zip(frames,frames[1:])):
 offsets0=np.array([translation(doc,g,f0['state']) if g else [0.,0.,0.] for g in groups]); offsets1=np.array([translation(doc,g,f1['state']) if g else [0.,0.,0.] for g in groups]);velocity=offsets1-offsets0
 lo=bounds[:,0]+offsets0+tol;hi=bounds[:,1]+offsets0-tol
 moving_flags=np.any(abs(velocity)>1e-10,axis=1)
 for i in np.flatnonzero(moving_flags):
  relative=velocity[i]-velocity
  eligible=np.any(abs(relative)>1e-10,axis=1)
  # No duplicate dynamic pairs; unchanged welded groups have zero relative motion.
  eligible &= (np.arange(len(rows))>i)|(~moving_flags)
  pairs+=int(eligible.sum())
  sweep_lo=np.minimum(lo[i],lo[i]+relative);sweep_hi=np.maximum(hi[i],hi[i]+relative)
  candidates=np.flatnonzero(eligible & np.all(sweep_hi>lo,axis=1)&np.all(sweep_lo<hi,axis=1))
  if not len(candidates):continue
  checks+=len(candidates);v=relative[candidates];l=lo[candidates]-hi[i];h=hi[candidates]-lo[i]
  lower=np.full_like(v,-np.inf);upper=np.full_like(v,np.inf)
  moving=abs(v)>1e-12
  t0=np.divide(l,v,out=np.zeros_like(v),where=moving);t1=np.divide(h,v,out=np.zeros_like(v),where=moving)
  lower[moving]=np.minimum(t0,t1)[moving];upper[moving]=np.maximum(t0,t1)[moving]
  blocked=np.any((~moving)&((l>=0)|(h<=0)),axis=1)
  enter=np.maximum(0,lower.max(axis=1));leave=np.minimum(1,upper.min(axis=1))
  for k in np.flatnonzero((~blocked)&(enter<leave-1e-10)):
   j=int(candidates[k]);failures.append({'segment':index,'from':f0['label'],'to':f1['label'],'a':names[i],'b':names[j],'time_interval':[float(enter[k]),float(leave[k])]})
  if len(failures)>60:break
 if len(failures)>60:break
receipt={'route_kind':'stage' if a.bay is None else 'launch','bay_id':a.bay,'cassette_id':None if a.bay is None else a.bay,'stage_loaded_cassette_id':a.source+10 if a.bay is None else None,'stage_empty_cassette_id':a.destination if a.bay is None else None,'scope':'source-authored hardware translating-box continuous relative sweeps; no drone triangles/authority/capacity acceptance','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('parallel_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'source_slot':a.source,'destination_berth':a.destination,'keyframes':len(frames),'source_hull_sha256':base['sha256'],'negative_closed_hatch':a.negative_closed_hatch,'relative_pair_domains':pairs,'broadphase_candidates':checks,'failures':failures}
a.output.write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2));sys.exit(bool(failures))
