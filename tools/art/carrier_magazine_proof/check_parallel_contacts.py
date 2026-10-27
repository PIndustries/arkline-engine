"""Source-derived local interface contacts/retention; no strength or motor model."""
import argparse,json,hashlib,sys
from pathlib import Path
import numpy as np
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);p.add_argument('--negative-missing-retainer',action='store_true');a=p.parse_args();d=json.loads(a.manifest.read_text());rows=d['parts'];checks=[];fail=[]
if a.negative_missing_retainer:rows=[r for r in rows if 'shuttle under cap retainer' not in r['semantic_id']]
def find(s):
 r=[r for r in rows if r['semantic_id'].endswith(s)]
 if len(r)!=1:fail.append('missing/ambiguous '+s);return None
 return np.array(r[0]['bounds_body_m'],float)
def touch(x,y,axis):
 if x is None or y is None:return False
 other=[i for i in range(3) if i!=axis]
 return abs(x[1,axis]-y[0,axis])<1e-6 and all(min(x[1,i],y[1,i])-max(x[0,i],y[0,i])>1e-6 for i in other)
def require(ok,label):checks.append(label);fail.extend([] if ok else [label])
def connected(group,start,end):
 rr=[r for r in rows if r.get('group')==group];n=[r['semantic_id'] for r in rr];b=np.array([r['bounds_body_m'] for r in rr]);seeds=[i for i,x in enumerate(n) if start in x];target={i for i,x in enumerate(n) if end in x};seen=set(seeds)
 while seeds:
  i=seeds.pop();neighbors=np.flatnonzero(np.all(b[:,1]>=b[i,0]-1e-6,axis=1)&np.all(b[:,0]<=b[i,1]+1e-6,axis=1))
  for j in neighbors:
   if int(j) not in seen:seen.add(int(j));seeds.append(int(j))
 return bool(target) and target<=seen
for dx in [-2.5,2.5]:
 for dz in [0.,1.5]:
  pad=find(f'cell00/shuttle pickup {dx}/{dz}')
  base=find('c00/fixed support '+('port' if dx<0 else 'starboard')+' base')
  require(touch(pad,base,1),f'fixed-base pad contact {dx}/{dz}')
  require(connected('cell00/shuttle/lift',f'shuttle pickup {dx}/{dz}',f'shuttle clamp upper guide {dx}/{dz}'),f'upper clamp guide connected {dx}/{dz}')
  for side in [-1,1]:
   shoe=find(f'cell00/shuttle clamp shoe {dx}/{dz}/{side}');wall=find('c00/fixed support '+('port' if dx<0 else 'starboard')+(' left' if side<0 else ' right'))
   require(touch(shoe,wall,0) if side<0 else touch(wall,shoe,0),f'positive side clamp contact {dx}/{dz}/{side}')
 for side in [-1,1]:
  lip=find(f'cell00/shuttle under cap retainer {dx}/{side}');cap=find(f'cell00/lower rail cap {dx}')
  require(touch(lip,cap,1),f'under-cap retention {dx}/{side}')
  require(connected('cell00/shuttle/travel',f'shuttle side {dx}',f'shuttle under cap retainer {dx}/{side}'),f'carriage retainer connected {dx}/{side}')
plate=find('cell00/protected floor plate')
for side in [-1,1]:
 lower=find(f'cell00/protected plate lower capture {side}')
 for stroke in [0.,6.6]:
  moved=plate.copy();moved[:,2]+=stroke;require(touch(lower,moved,1),f'plate captured support {side}/{stroke}')
 for z in [-32.75,-27.25]:
  tongue=find(f'cell00/protected pad tongue {side}/{z}');floor=find(f'cell00/protected pad guide floor {side}/{z}')
  for shift in [0.,.5]:
   moved=tongue.copy();moved[:,0]+=side*shift;require(touch(floor,moved,1),f'protected tongue supported {side}/{z}/{shift}')
   for flank in [-1,1]:
    cap=find(f'cell00/protected pad capture {side}/{z}/{flank}');require(touch(moved,cap,1),f'protected tongue captured {side}/{z}/{shift}/{flank}')
receipt={'scope':'source-derived plane contact, rigid connectivity and positive guide retention only; no force/strength/power claim','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'checks':len(checks),'negative_missing_retainer':a.negative_missing_retainer,'failures':fail};a.output.write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2));sys.exit(bool(fail))
