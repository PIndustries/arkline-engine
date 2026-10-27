"""Actual cached drone triangles against shared continuous Stage hardware domains."""
import argparse,hashlib,itertools,json,sys
from pathlib import Path
import numpy as np
from candidate_routes import stage,translation,bay
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);p.add_argument('--source',type=int,default=0);p.add_argument('--destination',type=int,default=4);p.add_argument('--bay',type=int);p.add_argument('--class-name',default='mining');a=p.parse_args()
if a.bay is not None:a.source=a.bay;a.destination=a.bay
doc=json.loads(a.manifest.read_text());raw=Path('pioneer-independent-bays-20261026/actual-drone-meshes.json');model=json.loads(raw.read_text());row=next(r for r in model['classes'] if r['class']==a.class_name);tris=np.array([v['position_m'] for b in row['batches'] for v in b['vertices']]).reshape(-1,3,3)
lo_t=tris.min(1);hi_t=tris.max(1);bodylo=tris.min((0,1));bodyhi=tris.max((0,1))
base=json.loads(Path('pioneer-independent-bays-20261026/source-part-bounds.json').read_text());parts=doc['parts']+[{'semantic_id':r['name'],'group':None,'bounds_body_m':r['bounds']} for r in base['parts']]
axes=np.array([[0,0,-1],[0,1,0],[1,0,0]] if a.source<5 else [[0,0,1],[0,1,0],[-1,0,0]],float)
def local(bounds):
 corners=np.array(list(itertools.product(*zip(*bounds))))@axes.T
 return np.array([corners.min(0),corners.max(0)])
def sat(t,lo,hi):
 c=(lo+hi)/2;h=(hi-lo)/2;t=t-c;e=np.roll(t,-1,axis=1)-t
 axes0=[np.broadcast_to(x,(len(t),3)) for x in np.eye(3)]+[np.cross(e[:,0],e[:,1])]+[np.cross(e[:,i],x) for i in range(3) for x in np.eye(3)]
 hit=np.ones(len(t),bool)
 for ax in axes0:
  q=np.einsum('nij,nj->ni',t,ax);r=np.abs(ax)@h;hit&=(q.min(1)<=r+1e-10)&(q.max(1)>=-r-1e-10)
 return hit.any()
def winding(point,surface=tris):
 v=surface-point;x,y,z=v[:,0],v[:,1],v[:,2];lx=np.linalg.norm(x,axis=1);ly=np.linalg.norm(y,axis=1);lz=np.linalg.norm(z,axis=1)
 num=np.einsum('ij,ij->i',x,np.cross(y,z));den=lx*ly*lz+np.einsum('ij,ij->i',x,y)*lz+np.einsum('ij,ij->i',y,z)*lx+np.einsum('ij,ij->i',z,x)*ly
 return abs(np.arctan2(num,den).sum()/(2*np.pi))
vertices=np.array([[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]],float);closed=vertices[[[0,2,4],[2,1,4],[1,3,4],[3,0,4],[2,0,5],[1,2,5],[3,1,5],[0,3,5]]]
assert not sat(closed,np.array([-.1]*3),np.array([.1]*3)) and winding(np.zeros(3),closed)>.99
bounds=np.array([local(r['bounds_body_m']) for r in parts]);frames=stage(doc,a.source,a.destination) if a.bay is None else bay(doc,a.bay);loaded=str(a.source+10 if a.bay is None else a.bay);hits=[];checks=0;unique=set();meshchecks=0
for index,(f0,f1) in enumerate(zip(frames,frames[1:])):
 body0=np.array(f0.get('drone_position',f0['state']['cassettes'][loaded]));body1=np.array(f1.get('drone_position',f1['state']['cassettes'][loaded]))
 offsets0=np.array([(np.array(translation(doc,r['group'],f0['state'])) if r['group'] else np.zeros(3))-body0 for r in parts])@axes.T
 offsets1=np.array([(np.array(translation(doc,r['group'],f1['state'])) if r['group'] else np.zeros(3))-body1 for r in parts])@axes.T
 lows=np.minimum(bounds[:,0]+offsets0,bounds[:,0]+offsets1)+1e-5;highs=np.maximum(bounds[:,1]+offsets0,bounds[:,1]+offsets1)-1e-5;checks+=len(parts)
 for i in np.flatnonzero(np.all(highs>bodylo,axis=1)&np.all(lows<bodyhi,axis=1)):
  lo,hi=lows[i],highs[i];key=tuple(np.round(np.r_[lo,hi],8))
  if key in unique:continue
  unique.add(key);meshchecks+=1
  if a.class_name=='mining':
   center=np.array([2.,-.1,0.]);nearest=np.maximum(lo,np.minimum(hi,center))
   if np.linalg.norm(nearest-center)<.49-1e-5:hits.append({'segment':index,'part':parts[i]['semantic_id'],'reason':'full articulated head bound'})
  mask=np.all(hi_t>=lo,axis=1)&np.all(lo_t<=hi,axis=1)
  if (mask.any() and sat(tris[mask],lo,hi)) or winding((lo+hi)/2)>.5:hits.append({'segment':index,'part':parts[i]['semantic_id'],'reason':'actual body surface/containment','bounds':[lo.tolist(),hi.tolist()]})
 # All other reserve AND ready occupants may remain loaded. The selected ready
 # destination alone must be empty; this covers sequential prior reloads.
 for c in doc['cassettes']:
  if str(c['cassette_id'])==loaded or (c['initial_location']=='ready' and c['cassette_id']==a.destination):continue
  other=np.array(c['body_center_m']);half=np.array([3.,.9,2.5]);rel0=other-body0;rel1=other-body1
  lo=np.minimum(rel0-half,rel1-half)+1e-5;hi=np.maximum(rel0+half,rel1+half)-1e-5
  if np.all(hi>-half) and np.all(lo<half):hits.append({'segment':index,'part':'retained drone '+str(c['cassette_id']),'reason':'full body peer domain'})
receipt={'route_kind':'stage' if a.bay is None else 'launch','bay_id':a.bay,'cassette_id':None if a.bay is None else a.bay,'stage_loaded_cassette_id':a.source+10 if a.bay is None else None,'stage_empty_cassette_id':a.destination if a.bay is None else None,'scope':'actual cached class mesh vs conservative continuous shared-route hardware domains; 10um tangent tolerance; all other protected and ready occupants retain full envelopes','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'mesh_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'source_hull_sha256':base['sha256'],'class':a.class_name,'source_slot':a.source,'destination_berth':a.destination,'relative_domains':checks,'unique_near_body_domains':meshchecks,'containment_negative_control':'oblique closed octahedron detected','failures':hits}
a.output.write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2));sys.exit(bool(hits))
