"""Compile exact source-owned fixture; ordinary admission remains disabled."""
import argparse,json,hashlib,itertools,math
from pathlib import Path
import numpy as np
from candidate_routes import stage,bay,translation
parser=argparse.ArgumentParser()
parser.add_argument('--manifest',type=Path,default=Path('pioneer-magazine-ready-20261026/fit-v15.json'))
parser.add_argument('--export-receipt',type=Path,default=Path('pioneer-magazine-ready-20261026/export-v15/receipt.json'))
parser.add_argument('--fixture-output',type=Path,default=Path('arkline-independent-bay-hull/crates/arkline-protocol/src/carrier_inventory_geometry/stationary_v1701.json'))
parser.add_argument('--binding-output',type=Path,default=Path('pioneer-magazine-ready-20261026/export-v15/runtime-binding.json'))
parser.add_argument('--revision',type=int,default=1701)
parser.add_argument('--extra-deployed-proof',type=Path)
args=parser.parse_args();assert args.revision>0
mp=args.manifest;doc=json.loads(mp.read_text());ex=json.loads(args.export_receipt.read_text());refs=doc['mechanism_reference_poses']
def pose(p,q=None):return {'position_m':p,'orientation_xyzw':q or [0.,0.,0.,1.]}
def qside(side):return [0.,(1 if side=='port' else -1)*math.sqrt(.5),0.,math.sqrt(.5)]
def dock(v):return {'kind':{'protected':'protected','ready':'ready','buffer':'transit_buffer'}[v[0]], {'protected':'slot_id','ready':'berth_id','buffer':'buffer_id'}[v[0]]:v[1]}
def union(boxes):return [np.min(np.array(boxes)[:,0],0).tolist(),np.max(np.array(boxes)[:,1],0).tolist()]
def axes(q):
 x,y,z,w=q;return np.array([[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w)],[2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w)],[2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y)]])
def local(b,p):
 points=(np.array(list(itertools.product(*zip(*b))))-p['position_m'])@axes(p['orientation_xyzw']);return [points.min(0).tolist(),points.max(0).tolist()]
reg={'schema_version':1,'host':{'geometry_revision':12,'source_sha256':'7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd'},'assembly_revision':args.revision,'assembly_source_sha256':ex['output']['sha256']}
classes=['recon','mining','attack'];top={'registration':reg,'ready_berths':[{'berth_id':i,'side':'port' if i<5 else 'starboard','compatible_classes':classes} for i in range(10)],'magazine_slots':[{'slot_id':i,'side':'port' if i<5 else 'starboard','compatible_classes':classes} for i in range(10)],'loaders':[{'loader_id':s,'side':['port','starboard'][s],'magazine_slots':list(range(s*5,s*5+5)),'ready_berths':list(range(s*5,s*5+5)),'transit_buffer_id':s} for s in range(2)],'transit_buffers':[{'buffer_id':s,'side':['port','starboard'][s]} for s in range(2)],'cassette_count':20}
mechanisms=[];dynamic=[];byname={r['name']:r for r in ex['source_parts']};groupboxes={}
for g,ref in refs.items():
 parts=[p for p in doc['parts'] if p['group']==g];bounds=union([p['bounds_body_m'] for p in parts]);groupboxes[g]=bounds
 if g.startswith('c') and g[1:3].isdigit():owner={'kind':'cassette','cassette_id':int(g[1:3])}
 elif g.startswith('b') and g[1:3].isdigit():owner={'kind':'berth','berth_id':int(g[1:3])}
 elif g.startswith('dock/'):
  _,kind,did,*_=g.split('/');owner={'kind':'dock','dock':dock([kind,int(did)])}
 else:owner={'kind':'loader','loader_id':0 if g.startswith('port/') else 1}
 mechanisms.append({'semantic_id':g,'owner':owner,'reference_pose':ref,'local_bounds_m':local(bounds,ref)})
 dynamic.append({'id':g,'reference_pose_body_m':{'translation':ref['position_m'],'rotation_xyzw':ref['orientation_xyzw']},'source_parts':[{k:byname[p['semantic_id']][k] for k in ['source_id','render_primitive','first_index','index_count']} for p in parts]})
cassettes=[];docks=[]
for c in doc['cassettes']:
 cid=c['cassette_id'];d=dock(['ready',cid] if cid<10 else ['protected',cid-10]);p=pose(c['body_center_m'],qside(c['side']));parts=[r['bounds_body_m'] for r in doc['parts'] if (r['group'] or '').startswith(f'c{cid:02d}/')]
 cassettes.append({'cassette_id':cid,'initial_dock':d,'reference_pose':p,'bounds_local_m':local(union(parts),p),'mechanism_ids':c['mechanism_ids']});docks.append({'dock':d,'cassette_pose':p,'working_bounds_m':union(parts)})
for i,b in enumerate(doc['exchange_buffers']):
 p=pose(b['body_center_m'],qside(b['side']));points=np.array(list(itertools.product(*zip(*cassettes[0]['bounds_local_m']))))@axes(p['orientation_xyzw']).T+np.array(p['position_m']);docks.append({'dock':dock(['buffer',i]),'cassette_pose':p,'working_bounds_m':[points.min(0).tolist(),points.max(0).tolist()]})
allbounds=[]
def convert(f,loaded,empty,action):
 side='port' if loaded<5 or 10<=loaded<15 else 'starboard';q=qside(side);s=f['state'];handled=f['handled_cassette_id'];ds=f['docked_cassettes'];supported=handled is not None and str(handled) in ds
 locked=supported and s['docks'].get('/'.join(map(str,ds[str(handled)])),0.)==0
 crane=s['loaders'][side];clear=crane['grip']==0 and crane['height']==5.1
 machinery=[]
 for g in f['owned_groups']:
  shift=np.array(translation(doc,g,s));ref=refs[g];machinery.append([g,pose((np.array(ref['position_m'])+shift).tolist(),ref['orientation_xyzw'])]);allbounds.append((np.array(groupboxes[g])+shift).tolist())
 dp=f.get('drone_position',s['cassettes'][str(loaded)])
 return {'label':f['label'],'phase':'loading' if action=='stage' else 'launching','drone_pose':pose(dp,q),'handled_cassette_id':handled,'carried_cassette_id':f['carried_cassette_id'],'cassette_poses':[[cid,pose(s['cassettes'][str(cid)],q)] for cid in sorted(set([loaded,empty]))],'docked_cassettes':[[int(cid),dock(d)] for cid,d in sorted(ds.items())],'machinery':machinery,'completed_stage_legs':f['completed_stage_legs'],'milestones':{'drone_clamped':f.get('launch_drone_clamped',True),'cassette_supported':supported,'cassette_locked':locked,'crane_latched':crane['grip']==1,'door_clear':bool(s['doors'].get(str(loaded),0)) if action!='stage' else False,'ore_coupled':False,'exterior_clear':f.get('launch_exterior_clear',False),'loader_clear':clear,'hardware_safe':f['label'] in ['exchange complete loader clear hardware safe','launch/hardware secured']}}
stages=[];launches=[]
for source in range(10):
 for dest in range((source//5)*5,(source//5)*5+5):stages.append({'source_slot':source,'destination_berth':dest,'loaded_cassette_id':source+10,'empty_cassette_id':dest,'keyframes':[convert(f,source+10,dest,'stage') for f in stage(doc,source,dest)]})
for bid in range(10):launches.append({'berth_id':bid,'cassette_id':bid,'keyframes':[convert(f,bid,bid,'launch') for f in bay(doc,bid)]})
berths=[]
for b in doc['bays']:
 bid=b['bay_id'];q=qside('port' if bid<5 else 'starboard');sign=-1 if bid<5 else 1
 def p(x):v=list(b['center_m']);v[2]+=sign*x;return pose(v,q)
 own=[groupboxes[g] for g in refs if g.startswith(f'b{bid:02d}/')]
 berths.append({'berth_id':bid,'support_pose':p(5.9),'contact_pose':p(5.9),'gate_pose':p(16),'clear_pose':p(16),'ore_transfer_capable':True,'drone_bounds_local_m':[[-2.5,-.9,-3],[2.5,.9,3]],'working_bounds_m':union(own+[[(np.array(b['center_m'])+v).tolist() for v in [np.array([-3.5,-2,-19]),np.array([3.5,3,19])]]])})
carrier_bounds=[(np.array(ex['output']['body_bounds_m'][0])-.001).tolist(),(np.array(ex['output']['body_bounds_m'][1])+.001).tolist()]
extra_bounds=[]
if args.extra_deployed_proof:
 proof=json.loads(args.extra_deployed_proof.read_text())
 assert proof['composed_glb_sha256']==reg['assembly_source_sha256'] and not proof['failures']
 extra_bounds=proof['turret_union_bounds_m']
 assert len(extra_bounds)==2
definition={'topology':top,'carrier_bounds_m':carrier_bounds,'deployed_hardware_bounds_m':union(allbounds+[carrier_bounds]+extra_bounds),'cassettes':cassettes,'docks':docks,'berths':berths,'mechanisms':mechanisms}
fixture={'scope':'compiled stationary test fixture only; normal lookup unavailable','manifest_sha256':hashlib.sha256(mp.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'definition':definition,'stages':stages,'launches':launches}
args.fixture_output.parent.mkdir(exist_ok=True,parents=True);args.fixture_output.write_text(json.dumps(fixture,separators=(',',':'))+'\n')
n=sum(r['index_count']//3 for g in dynamic for r in g['source_parts']);binding={'schema':'arkline.pioneer-drone-bay-runtime-binding/v1','registration_revision':args.revision,'glb_sha256':reg['assembly_source_sha256'],'source_triangle_count':ex['output']['triangles'],'dynamic_triangle_count':n,'static_triangle_count':ex['output']['triangles']-n,'dynamic_groups':dynamic};args.binding_output.parent.mkdir(exist_ok=True,parents=True);args.binding_output.write_text(json.dumps(binding,indent=2));print(reg);print(len(dynamic),'groups',n,'moving triangles')
