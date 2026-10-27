"""Source-derived support/clamp/eye contact, not structural strength or pressure."""
import argparse,hashlib,itertools,json,sys
from pathlib import Path
import numpy as np
from candidate_routes import stage,translation
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args();doc=json.loads(a.manifest.read_text());raw=Path('pioneer-independent-bays-20261026/actual-drone-meshes.json');mesh=json.loads(raw.read_text());parts=doc['parts'];checks=0;fail=[]
def ray(t,o,d):
 e1=t[:,1]-t[:,0];e2=t[:,2]-t[:,0];h=np.cross(np.broadcast_to(d,e2.shape),e2);det=np.einsum('ij,ij->i',e1,h);ok=abs(det)>1e-12;inv=np.divide(1,det,out=np.zeros_like(det),where=ok);s=o-t[:,0];u=inv*np.einsum('ij,ij->i',s,h);q=np.cross(s,e1);v=inv*(q@d);dist=inv*np.einsum('ij,ij->i',e2,q);valid=ok&(u>=-1e-7)&(v>=-1e-7)&(u+v<=1+1e-7)&(dist>=-1e-7)
 return float(dist[valid].min()) if valid.any() else None
axes=np.array([[0,0,-1],[0,1,0],[1,0,0]],float);center=np.array(doc['cassettes'][0]['body_center_m'])
def local(part):
 q=(np.array(list(itertools.product(*zip(*part['bounds_body_m']))))-center)@axes.T
 return np.array([q.min(0),q.max(0)])
for row in mesh['classes']:
 t=np.array([v['position_m'] for b in row['batches'] for v in b['vertices']]).reshape(-1,3,3)
 for part in parts:
  name=part['semantic_id']
  if not name.startswith('pioneer.magazine_ready/c00/'):continue
  bb=local(part)
  if '/contact shoe ' in name:
   mid=(bb[0]+bb[1])/2
   for x in np.linspace(mid[0]-.15,mid[0]+.15,3):
    for z in np.linspace(mid[2]-.09,mid[2]+.09,3):
     checks+=1;gap=ray(t,np.array([x,bb[1,1]-1e-5,z]),np.array([0,1,0]))
     if gap is None or abs(gap-1e-5)>2e-6:fail.append({'class':row['class'],'part':name,'gap':gap})
  if '/pad clamp ' in name:
   sign=-1 if bb[:,2].mean()<0 else 1;contact=bb[1,2] if sign<0 else bb[0,2];mid=bb[:,0].mean()
   for x in [mid-.15,mid,mid+.15]:
    for y in [-.85,-.83,-.81]:
     checks+=1;gap=ray(t,np.array([x,y,contact+sign*1e-5]),np.array([0,0,-sign]))
     if gap is None or abs(gap-1e-5)>2e-6:fail.append({'class':row['class'],'part':name,'gap':gap})
def overlap_xz(a,b):return np.maximum(0,np.minimum(a[1,[0,2]],b[1,[0,2]])-np.maximum(a[0,[0,2]],b[0,[0,2]])).prod()
def bb(part,state):
 shift=np.array(translation(doc,part['group'],state)) if part['group'] else np.zeros(3)
 return np.array(part['bounds_body_m'])+shift
contact_frames=0
for source,dest in [(0,4),(5,9)]:
 side='port' if source<5 else 'starboard';frames=stage(doc,source,dest)
 for f in frames:
  s=f['state'];handled=f['handled_cassette_id']
  if handled is not None and s['loaders'][side]['grip']==1:
   contact_frames+=1;eyes=[r for r in parts if r['semantic_id'].startswith(f'pioneer.magazine_ready/c{handled:02d}/cassette lift eye ')]
   shoes=[r for r in parts if (r['group'] or '').startswith(side+'/loader/eye-latch/')]
   for shoe in shoes:
    sb=bb(shoe,s);checks+=1
    if not any(abs(sb[1,1]-bb(eye,s)[0,1])<1e-7 and overlap_xz(sb,bb(eye,s))>1e-5 for eye in eyes):fail.append({'frame':f['label'],'shoe':shoe['semantic_id'],'reason':'no underside eye contact'})
  for cid,dock in f['docked_cassettes'].items():
   kind,did=dock
   feet=[r for r in parts if r['semantic_id'].startswith(f'pioneer.magazine_ready/c{int(cid):02d}/cassette lift post ')]
   supports=[r for r in parts if (kind=='ready' and r['semantic_id']==f'pioneer.magazine_ready/b{did:02d}/floor') or (kind=='protected' and f'/reserve/{did%5}/dock support ' in r['semantic_id'] and ('port/' if did<5 else 'starboard/') in r['semantic_id']) or (kind=='buffer' and '/exchange buffer support ' in r['semantic_id'] and ('port/' if did==0 else 'starboard/') in r['semantic_id'])]
   for foot in feet:
    fb=bb(foot,s);checks+=1
    if not any(abs(bb(support,s)[1,1]-fb[0,1])<1e-7 and overlap_xz(fb,bb(support,s))>.0001 for support in supports):fail.append({'frame':f['label'],'foot':foot['semantic_id'],'dock':dock,'reason':'no source dock pad contact'})
# Independent rigid-load-path connectivity: never use the translating deck.
def joined(a,b):
 return bool(np.all(np.minimum(a[1],b[1])-np.maximum(a[0],b[0])>=-1e-7))
for cid in range(20):
 fixed=[r for r in parts if r['semantic_id'].startswith(f'pioneer.magazine_ready/c{cid:02d}/') and r['group']==f'c{cid:02d}/cassette/base']
 boxes=[np.array(r['bounds_body_m']) for r in fixed]
 reached={i for i,r in enumerate(fixed) if '/fixed support ' in r['semantic_id']}
 while True:
  added={j for j in range(len(fixed)) if j not in reached and any(joined(boxes[j],boxes[i]) for i in reached)}
  if not added:break
  reached |= added
 for i,r in enumerate(fixed):
  if '/cassette lift post ' in r['semantic_id']:
   checks+=1
   if i not in reached:fail.append({'part':r['semantic_id'],'reason':'lift post disconnected from fixed support frame'})
for r in parts:
 if '/eye pin ' not in r['semantic_id']:continue
 corner=r['semantic_id'].split('/eye pin ',1)[1];prefix=r['semantic_id'].split('/eye pin ',1)[0]
 caps=[x for x in parts if x['semantic_id']==prefix+'/terminal pin cap '+corner]
 walls=[x for x in parts if x['semantic_id'].startswith(prefix+'/hoist '+corner+'/4/')]
 checks+=1
 if len(caps)!=1 or not joined(np.array(r['bounds_body_m']),np.array(caps[0]['bounds_body_m'])) or not walls or not all(joined(np.array(w['bounds_body_m']),np.array(caps[0]['bounds_body_m'])) for w in walls):
  fail.append({'part':r['semantic_id'],'reason':'pin lacks terminal tube cap connection'})
receipt={'scope':'all3 cached drone keel pads/clamps; actual authored cassette foot supports and pin-shoe underside-eye contacts over shared route endpoints; not strength/friction/pressure proof','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'mesh_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'checks':checks,'craned_contact_frames':contact_frames,'failures':fail};a.output.write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2));sys.exit(bool(fail))
