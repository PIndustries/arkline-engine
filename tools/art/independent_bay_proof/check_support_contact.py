import json,hashlib,sys
from pathlib import Path
import numpy as np
p=Path('pioneer-independent-bays-20261026/actual-drone-meshes.json');mesh=json.loads(p.read_text());manifest_path=Path('pioneer-independent-bays-20261026/fit-v12.json');manifest=json.loads(manifest_path.read_text());hits=[];checks=0;results=[]
def ray(t,o,d):
 e1=t[:,1]-t[:,0];e2=t[:,2]-t[:,0];h=np.cross(np.broadcast_to(d,e2.shape),e2);det=np.einsum('ij,ij->i',e1,h);ok=abs(det)>1e-12;inv=np.divide(1,det,out=np.zeros_like(det),where=ok);s=o-t[:,0];u=inv*np.einsum('ij,ij->i',s,h);q=np.cross(s,e1);v=inv*(q@d);dist=inv*np.einsum('ij,ij->i',e2,q);valid=ok&(u>=-1e-7)&(v>=-1e-7)&(u+v<=1+1e-7)&(dist>=-1e-7)
 return float(dist[valid].min()) if valid.any() else None
def authored_local(cls,name):
 bay=next(b for b in manifest['bays'] if b['class']==cls)
 part=next(p for p in manifest['parts'] if p['semantic_id']==f"pioneer.independent_bay/b{bay['bay_id']:02d}/"+name)
 import itertools
 pts=np.array(list(itertools.product(*zip(*part['bounds_body_m']))))-bay['center_m'];axes=np.array([bay['forward'],bay['up'],bay['right']]);pts=pts@axes.T
 return np.array([pts.min(0),pts.max(0)])
for row in mesh['classes']:
 cls=row['class'];t=np.array([v['position_m'] for b in row['batches'] for v in b['vertices']]).reshape(-1,3,3);pad_z=[-2.5,2.5] if cls=='mining' else [-.3,.3];hw=.35 if cls=='mining' else .15;cz=2.86 if cls=='mining' else .65;count=0
 for x in [-1.,.4]:
  for z in pad_z:
   for dx in [-.15,0,.15]:
    for dz in [-hw*.6,0,hw*.6]:
     bb=authored_local(cls,f'contact shoe {x} {z}');assert bb[0,0]<x+dx<bb[1,0] and bb[0,2]<z+dz<bb[1,2];gap=ray(t,np.array([x+dx,bb[1,1]-.00001,z+dz]),np.array([0.,1.,0.]));checks+=1;count+=1
     if gap is None or abs(gap-1e-5)>2e-6:hits.append(dict(kind='support',cls=cls,x=x+dx,z=z+dz,gap=gap))
  for sign in [-1,1]:
   for dx in [-.15,0,.15]:
    for y in ([-.87,-.85,-.83] if cls=='mining' else [-.85,-.83,-.81]):
     bb=authored_local(cls,f'pad clamp {"port" if sign<0 else "starboard"} {x}');assert bb[0,0]<x+dx<bb[1,0] and bb[0,1]<y<bb[1,1];contact=bb[1,2] if sign<0 else bb[0,2];gap=ray(t,np.array([x+dx,y,contact+sign*.00001]),np.array([0.,0.,-sign]));checks+=1;count+=1
     if gap is None or abs(gap-1e-5)>2e-6:hits.append(dict(kind='clamp',cls=cls,x=x+dx,y=y,side=sign,gap=gap))
 results.append({'class':cls,'checks':count})
r={'scope':'sampled actual surface contact over interior pad footprints and clamp bands; not strength/friction proof','mesh_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'manifest_sha256':hashlib.sha256(manifest_path.read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'checks':checks,'classes':results,'failures':hits};Path('pioneer-independent-bays-20261026/support-contact-v12.json').write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));sys.exit(bool(hits))
