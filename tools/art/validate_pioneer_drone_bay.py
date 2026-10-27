import struct,json,pathlib,numpy as np,collections,argparse,time
p=argparse.ArgumentParser();p.add_argument('export');p.add_argument('manifest');p.add_argument('--output',required=True);a=p.parse_args();root=pathlib.Path(a.export);manifest=json.loads(pathlib.Path(a.manifest).read_text());raw=(root/'pioneer.glb').read_bytes();n=struct.unpack_from('<I',raw,12)[0];j=json.loads(raw[20:20+n]);off=28+n

def acc(i):
 a=j['accessors'][i];v=j['bufferViews'][a['bufferView']];return np.frombuffer(raw,dtype={5126:'<f4',5125:'<u4',5123:'<u2'}[a['componentType']],count=a['count']*{'SCALAR':1,'VEC3':3}[a['type']],offset=off+v.get('byteOffset',0)+a.get('byteOffset',0)).reshape(-1,{'SCALAR':1,'VEC3':3}[a['type']])
prims=[acc(q['attributes']['POSITION'])[acc(q['indices']).ravel()].reshape(-1,3,3) for q in j['meshes'][0]['primitives']]
r=json.loads((root/'receipt.json').read_text());partrec=r.get('source_parts')
if partrec is None:
 print(r.keys());raise Exception('receipt parts missing')
groups={n:g for g,ns in manifest['dynamic_groups'].items() for n in ns};parts={}
for q in partrec:
 t=prims[q['render_primitive']][q['first_index']//3:(q['first_index']+q['index_count'])//3].astype(float);parts[q['name']]={'tri':t,'lo':t.min((0,1)),'hi':t.max((0,1)),'group':groups.get(q['name'])}

def sat_batch(tris,lo,hi):
 c=(lo+hi)/2;h=(hi-lo)/2;t=tris-c;edges=np.roll(t,-1,axis=1)-t
 axes=[np.broadcast_to(ax,(len(t),3)) for ax in np.eye(3)];axes +=[np.cross(edges[:,0],edges[:,1])];axes +=[np.cross(edges[:,e],ax) for e in range(3) for ax in np.eye(3)]
 hit=np.ones(len(t),bool)
 for ax in axes:
  q=np.einsum('nij,nj->ni',t,ax);rad=np.abs(ax)@h;hit &= (q.min(1)<=rad+1e-9)&(q.max(1)>=-rad-1e-9)
 return hit

def collisions(lo,hi,skip=(),dynamic=False):
 lo=np.array(lo)+1e-5;hi=np.array(hi)-1e-5;hits=[]
 for name,part in parts.items():
  if name in skip or (not dynamic and part['group']):continue
  if np.any(part['hi']<=lo) or np.any(part['lo']>=hi):continue
  t=part['tri'];m=(t.max(1)>=lo).all(1)&(t.min(1)<=hi).all(1)
  if m.any() and sat_batch(t[m],lo,hi).any():hits.append(name)
 return hits
results=[]
def test(label,lo,hi,skip=(),dynamic=False):
 hs=collisions(lo,hi,skip,dynamic);results.append({'label':label,'bounds':[list(lo),list(hi)],'hits':hs})
for s in manifest['slots']:
 x,y,z=s['body_center_m'];sid=s['id'];own=manifest['dynamic_groups'][s['tray_group']]
 test('body parked '+sid,[x-2.5,y-.9,.5],[x+2.5,y+.9,6.5],own,True)
 test('body lateral '+sid,[x-2.5,y-.9,-6.5],[x+2.5,y+.9,6.5])
 test('body lift '+sid,[x-2.5,min(y,-1)-.9,-6.5],[x+2.5,max(y,-1)+.9,-.5])
 # Each tray part's actual reference AABB swept along lateral, vertical, X.
 for name in own:
  q=parts[name];lo=q['lo'];hi=q['hi'];test('tray lateral '+sid+' '+name,lo+[0,0,-7],hi)
  lo=lo+[0,0,-7];hi=hi+[0,0,-7];dy=-1-y
  test('tray lift '+sid+' '+name,lo+[0,min(0,dy),0],hi+[0,max(0,dy),0])
  lo=lo+[0,dy,0];hi=hi+[0,dy,0];dx=-30.5-x;test('tray X '+sid+' '+name,lo+[min(dx,0),0,0],hi+[max(dx,0),0,0])
test('body aft departure against static',[-42.5,-1.9,-6.5],[-28,-.1,-.5])
for name,q in parts.items():
 g=q['group']
 if not g or g.startswith('tray/'):continue
 lo=q['lo'];hi=q['hi']
 if g=='carriage_x':test('hardware X '+name,lo,hi+[12,0,0])
 elif g in ['lift_xy','fork_stage0','fork_stage1','fork_stage2']:
  ext=0 if g=='lift_xy' else int(g[-1])*3.5
  test('hardware retracted XY '+name,lo+[0,-4.4,0],hi+[12,4.4,0])
  if ext:
   for slot in manifest['slots']:
    sx,sy,_=slot['body_center_m'];delta=np.array([sx+30.5,sy+1,0])
    test('fork at '+slot['id']+' '+name,lo+delta,hi+delta+[0,0,ext])
 elif g.startswith('door/'):
  dy=1.7 if g.endswith('upper') else -1.7
  test('door sweep '+name,lo+[0,min(0,dy),0],hi+[0,max(0,dy),0])
summary={'scope':'Conservative actual-part AABB continuous translation sweeps vs exact staged static GLB triangles; 10um inward tolerance removes intended tangent contacts. Does not prove all simultaneous moving-part interactions.','checks':results,'failed':sum(bool(t['hits']) for t in results),'part_count':len(parts)}
pathlib.Path(a.output).write_text(json.dumps(summary,indent=2));print('Checks',len(results),'failed',summary['failed']);print('\n'.join(q['label']+': '+str(q['hits']) for q in results if q['hits']))
