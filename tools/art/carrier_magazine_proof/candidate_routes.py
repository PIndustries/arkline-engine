"""Shared source-only cassette exchange keyframes; no clocks/world admission.
Every checker/provider export consumes this same module, never independent poses.
"""
import copy

def initial(doc):
 return {'cassettes':{str(c['cassette_id']):list(c['body_center_m']) for c in doc['cassettes']},
  'loaders':{'port':{'x':14.,'z':-22.8,'height':5.1,'grip':0.},'starboard':{'x':14.,'z':22.8,'height':5.1,'grip':0.}},
  'docks':{},'hatches':{},'doors':{},'cradles':{},'clamps':{}}

def stage(doc,source_slot,destination_berth,loaded_cassette=None,empty_cassette=None):
 assert 0<=source_slot<10 and 0<=destination_berth<10 and source_slot//5==destination_berth//5
 loaded_cassette=source_slot+10 if loaded_cassette is None else loaded_cassette
 empty_cassette=destination_berth if empty_cassette is None else empty_cassette
 assert loaded_cassette!=empty_cassette
 side='port' if source_slot<5 else 'starboard';sign=-1 if side=='port' else 1;buffer=source_slot//5
 source=next(s['body_center_m'] for s in doc['magazine_slots'] if s['side']==side and s['slot_id'].endswith('/'+str(source_slot%5)))
 destination=list(doc['bays'][destination_berth]['center_m']);buffer_pose=[14.,4.8,sign*22.8]
 s=initial(doc);s['cassettes'][str(loaded_cassette)]=list(source);s['cassettes'][str(empty_cassette)]=destination
 frames=[];leg=0;carried=None;handled=None;dockmap={str(loaded_cassette):['protected',source_slot],str(empty_cassette):['ready',destination_berth]}
 owned=[g for g in doc['mechanism_reference_poses'] if g.startswith((f'c{loaded_cassette:02d}/',f'c{empty_cassette:02d}/',side+'/loader/',f'b{destination_berth:02d}/',f'dock/protected/{source_slot}/',f'dock/ready/{destination_berth}/',f'dock/buffer/{buffer}/'))]
 def emit(label):
  frames.append({'label':label,'state':copy.deepcopy(s),'handled_cassette_id':handled,'carried_cassette_id':carried,
   'docked_cassettes':copy.deepcopy(dockmap),'completed_stage_legs':leg,'owned_groups':owned})
 def crane(label,**kw):s['loaders'][side].update(kw);emit(label)
 def hatch(value):s['hatches'][str(destination_berth)]=value;emit('inner transfer hatch '+('open' if value else 'closed'))
 def dock(kind,did,value):s['docks'][f'{kind}/{did}']=value;emit(f'{kind} {did} dock '+('released' if value else 'locked'))
 def move_with(label,cid,**kw):
  s['loaders'][side].update(kw);g=s['loaders'][side];s['cassettes'][str(cid)]=[g['x'],g['height'],g['z']];emit(label)
 def pickup(cid,where,pose):
  nonlocal carried,handled
  handled=cid
  # Aisle first; no diagonal path through parked neighboring cassettes.
  crane('empty trolley along aisle',x=pose[0]);crane('empty trolley to source row',z=pose[2])
  crane('pin approach above source eye',height=pose[1]+.3)
  crane('pin enters source eye',height=pose[1]);carried=cid;crane('eye shoes secured',grip=1.)
  dock(where[0],where[1],1.)
  carried=cid;dockmap.pop(str(cid));move_with('supported cassette lifted clear',cid,height=5.1)
 def place(cid,where,pose):
  nonlocal carried
  dock(where[0],where[1],1.)
  move_with('carried cassette to aisle',cid,z=sign*22.8)
  move_with('carried cassette along aisle',cid,x=pose[0])
  move_with('carried cassette to destination row',cid,z=pose[2])
  move_with('cassette seats on actual dock pads',cid,height=pose[1])
  dock(where[0],where[1],0.);dockmap[str(cid)]=list(where)
  carried=None;crane('eye shoes released after dock lock',grip=0.)
  crane('hoist clear of locked cassette',height=5.1)
 emit('source-bound exchange entry');hatch(1.)
 pickup(empty_cassette,('ready',destination_berth),destination);place(empty_cassette,('buffer',buffer),buffer_pose)
 leg=1;emit('empty cassette supported and locked in transit buffer')
 pickup(loaded_cassette,('protected',source_slot),source);place(loaded_cassette,('ready',destination_berth),destination)
 hatch(0.);leg=2;emit('loaded cassette supported and locked in ready berth')
 pickup(empty_cassette,('buffer',buffer),buffer_pose);place(empty_cassette,('protected',source_slot),source)
 crane('empty trolley exits reserve row',z=sign*22.8);crane('loader returns to safe rest',x=14.)
 dock('buffer',buffer,0.);leg=3;handled=empty_cassette;emit('exchange complete loader clear hardware safe')
 return frames

def translation(doc,g,state):
 """Actual root minus exact authored reference root (all routes translate)."""
 ref=doc['mechanism_reference_poses'][g]['position_m'];t=[0.,0.,0.]
 if g.startswith('c') and g[1:3].isdigit():
  cid=int(g[1:3]);p=state['cassettes'][str(cid)];t=[p[i]-ref[i] for i in range(3)]
  sign=-1 if cid<5 or 10<=cid<15 else 1
  stroke=state['cradles'].get(str(cid),0.);release=state['clamps'].get(str(cid),0.);suffix=g.split('/',1)[1]
  if suffix=='cradle' or suffix.startswith('clamp/'):t[2]+=sign*5.9*stroke
  elif suffix.startswith('cradle/stage/'):t[2]+=sign*5.9*stroke*int(suffix.rsplit('/',1)[1])/4
  if suffix.startswith('clamp/'):
   local_z=release*(-.18 if suffix.endswith('port') else .18);t[0]+=-sign*local_z
 elif g.startswith('b') and g[1:3].isdigit():
  bid=int(g[1:3]);sign=-1 if bid<5 else 1
  if g.endswith('/magazine/hatch'):t[2]=sign*7.2*state['hatches'].get(str(bid),0.)
  elif '/door/' in g:t[1]=state['doors'].get(str(bid),0.)*(1.35 if g.endswith('upper') else -1.8)
 elif g.startswith('dock/'):
  _,kind,did,_,direction=g.split('/');t[0]=-int(direction)*.15*state['docks'].get(f'{kind}/{did}',0.)
 else:
  side=g.split('/')[0];v=state['loaders'][side];t=[v['x']-14.,0.,v['z']-ref[2]]
  if g.endswith('/bridge'):t[2]=0.
  if '/hoist/' in g:t[1]=-(5.1-v['height'])*int(g.rsplit('/',1)[1])/4.
  if '/eye-latch/' in g:
   t[0]+=int(g.rsplit('/',1)[1])*.035*v['grip'];t[1]=-(5.1-v['height'])
 return t

def bay(doc,berth_id,cassette_id=None):
 """Source-owned Launch only. Reverse/service remain unavailable until proved."""
 cid=berth_id if cassette_id is None else cassette_id
 b=doc['bays'][berth_id];sign=-1 if berth_id<5 else 1;s=initial(doc)
 s['cassettes'][str(cid)]=list(b['center_m']);frames=[]
 owned=[g for g in doc['mechanism_reference_poses'] if g.startswith((f'c{cid:02d}/',f'b{berth_id:02d}/',f'dock/ready/{berth_id}/'))]
 for label,x,door,stroke,release in [('entry',0,0,0,0),('door clear',0,1,0,0),('supported extension',5.9,1,1,0),('clamps release',5.9,1,1,1),('body exterior clear',16,1,1,1),('cradle retracted',16,1,0,1),('hardware secured',16,0,0,0)]:
  s['doors'][str(berth_id)]=door;s['cradles'][str(cid)]=stroke;s['clamps'][str(cid)]=release
  p=list(b['center_m']);p[2]+=sign*x
  frames.append({'label':'launch/'+label,'state':copy.deepcopy(s),'drone_position':p,'handled_cassette_id':cid,'carried_cassette_id':None,'docked_cassettes':{str(cid):['ready',berth_id]},'completed_stage_legs':0,'owned_groups':owned,'launch_exterior_clear':x==16,'launch_drone_clamped':x==5.9*stroke and not release})
 return frames
