"""Shared candidate kinematics used by authoring proof and future provider.
No clock, registration admission, stock status or transfer mutation occurs here.
"""
import json, math,sys,hashlib
from pathlib import Path
ZERO=dict(door=0.,cradle=0.,release=0.,arm=0.,insert=0.,latch=0.)
def state(phase,x,**kw):return dict(phase=phase,drone_x=x,controls=dict(ZERO,**kw))
LAUNCH=[state('preparing',0),state('opening',0,door=1),state('launching',5.9,door=1,cradle=1),state('releasing',5.9,door=1,cradle=1,release=1),state('departing',16,door=1,cradle=1,release=1),state('securing',16,door=1,release=1),state('securing',16)]
STORE=[state('preparing',16),state('opening',16,door=1),state('releasing',16,door=1,release=1),state('supporting',16,door=1,cradle=1,release=1),state('aligning',5.9,door=1,cradle=1,release=1),state('clamping',5.9,door=1,cradle=1),state('stowing',0,door=1),state('securing',0)]
SERVICE=[state('preparing',16),state('opening',16,door=1),state('aligning',5.9,door=1),state('coupling',5.9,door=1,arm=1),state('coupling',5.9,door=1,arm=1,insert=1),state('waiting_for_manifold',5.9,door=1,arm=1,insert=1,latch=1),state('uncoupling',5.9,door=1,arm=1,insert=1),state('uncoupling',5.9,door=1,arm=1),state('uncoupling',5.9,door=1),state('departing',16,door=1),state('securing',16)]
ROUTES={'launch':LAUNCH,'return_to_storage':STORE,'return_to_unload':SERVICE}
def translation(group,bay,s):
 prefix=f"b{bay['bay_id']:02d}/"
 if not group or not group.startswith(prefix):return [0.,0.,0.]
 g=group[len(prefix):];v=[0.,0.,0.]
 if g.startswith('door/'):v[1]=s['door']*(3.1 if bay['class']=='recon' else (1.35 if g.endswith('upper') else -1.8))
 elif g=='cradle':v[0]=5.9*s['cradle']
 elif g.startswith('clamp/'):v[0]=5.9*s['cradle'];v[2]=s['release']*(-.18 if g.endswith('port') else .18)
 elif g.startswith('cradle/stage/'):v[0]=5.9*s['cradle']*int(g.rsplit('/',1)[1])/4
 elif g.startswith('receiver/stage/'):v[2]=-3.65*s['arm']*int(g.rsplit('/',1)[1])/5
 elif g.startswith('receiver/'):
  v[0]=.08*s['insert'];v[2]=-3.65*s['arm']
  if g.startswith('receiver/latch/'):v[2]+=s['latch']*(-.03 if g.endswith('port') else .03)
 return v
def body_vector(bay,p):return [sum(bay[k][i]*p[j] for j,k in enumerate(['forward','up','right'])) for i in range(3)]
def pose(bay,p):
 q=([0,1,0,0] if bay['class']=='recon' else [0,(1 if bay['class']=='mining' else -1)*math.sqrt(.5),0,math.sqrt(.5)])
 v=body_vector(bay,p)
 return {'position_m':[bay['center_m'][i]+v[i] for i in range(3)],'orientation_xyzw':q}
def fixtures(manifest):
 result=[]
 for bay in manifest['bays']:
  routes={}
  for name,states in ROUTES.items():
   if name=='return_to_unload' and not bay['ore_service_supported']:continue
   rows=[]
   for i,r in enumerate(states):
    c=r['controls'];supported=abs(r['drone_x']-5.9*c['cradle'])<1e-8 and name!='return_to_unload'
    rows.append({'label':f'{name}/{i}/{r["phase"]}','phase':r['phase'],'drone_pose':pose(bay,[r['drone_x'],0,0]),'mechanism_poses':[[g,pose(bay,translation(g,bay,c))] for g in sorted(bay['groups'].values())],
     'milestones':{'door_clear':c['door']==1,'supported':supported,'clamped':supported and c['release']==0,'coupled':c['latch']==1 and c['insert']==1 and c['arm']==1,'exterior_clear':r['drone_x']>=16,'hardware_safe':all(v==0 for v in c.values())},'candidate_controls':c})
   routes[name]=rows
  result.append({'bay_id':bay['bay_id'],'routes':routes})
 return result
if __name__=='__main__':
 p=Path(sys.argv[1]);m=json.loads(p.read_text());out=Path(sys.argv[2]);assert not out.exists()
 out.write_text(json.dumps({'status':'unregistered candidate semantic route fixture; no timings','manifest_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'bays':fixtures(m)},indent=2))
