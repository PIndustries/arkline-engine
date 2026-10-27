"""Unadmitted first local-cell route. Actual translations, no runtime clocks."""
import copy
import candidate_routes as old

def initial(doc):
 s=old.initial(doc);s['local']={'z':-22.8,'height':5.1,'grip':0.,'shuttle_z':-22.8,'shuttle_lift':-.35,'floor':0.,'support':0.,'buffer_release':0.,'shuttle_clamp':1.}
 return s

def stage(doc,source_slot=0,destination_berth=0):
 assert source_slot==destination_berth==0
 s=initial(doc);frames=[];leg=0;carried=None;handled=None;docks={'0':['ready',0],'10':['protected',0]}
 owned=[g for g in doc['mechanism_reference_poses'] if g.startswith(('cell00/','b00/','c00/','c10/','dock/ready/0/','dock/protected/0/'))]
 def emit(label):frames.append({'label':label,'state':copy.deepcopy(s),'completed_stage_legs':leg,'handled_cassette_id':handled,'carried_cassette_id':carried,'docked_cassettes':copy.deepcopy(docks),'owned_groups':owned})
 def local(label,**kw):s['local'].update(kw);emit(label)
 def dock(kind,v):s['docks'][kind+'/0']=v;emit(kind+' dock '+str(v))
 def crane(label,cid=None,**kw):
  s['local'].update(kw)
  if cid is not None:s['cassettes'][str(cid)]=[-26.,s['local']['height'],s['local']['z']]
  emit(label)
 emit('local source entry')
 handled=0;local('shuttle touches fixed base',shuttle_lift=0.)
 carried=0;local('shuttle side clamps retain fixed cassette channels',shuttle_clamp=0.)
 dock('ready',1.);docks.pop('0');carried=0
 s['cassettes']['0'][1]=.1;local('shuttle lifts empty clear of ready pads',shuttle_lift=.1)
 local('lower buffer jaws open before arrival',buffer_release=1.)
 s['cassettes']['0'][2]=-30.;local('empty cassette translates to lower pocket',shuttle_z=-30.)
 s['cassettes']['0'][1]=0.;local('empty cassette seats lower buffer',shuttle_lift=0.)
 docks['0']=['buffer',0];local('lower buffer locks',buffer_release=0.);carried=None
 local('shuttle clamps release after buffer locks',shuttle_clamp=1.)
 local('shuttle releases supported cassette',shuttle_lift=-.35)
 local('shuttle returns under ready floor',shuttle_z=-22.8)
 leg=1;emit('empty secured lower buffer')
 s['hatches']['0']=1.;emit('ready transfer hatch opens')
 handled=10;crane('empty hoist to protected row',z=-30.)
 crane('pin enters protected eyes',height=4.8);carried=10;crane('protected eye latches engage',grip=1.)
 dock('protected',1.);docks.pop('10')
 crane('loaded cassette lifts off pads',10,height=5.1)
 crane('loaded cassette translates to ready shaft',10,z=-22.8)
 crane('loaded cassette lowers into ready',10,height=0.)
 dock('ready',0.);docks['10']=['ready',0];carried=None
 crane('ready supported release hoist',grip=0.);crane('hoist clears ready shaft',height=5.1)
 s['hatches']['0']=0.;emit('ready hatch closes before protected opening')
 leg=2;emit('loaded ready locked but launch corridor remains blocked')
 local('protected pads and tracks retract',support=1.)
 local('protected floor slides into closed ready roof pocket',floor=1.)
 handled=0;crane('empty hoist aligns lower buffer shaft',z=-30.)
 crane('pin enters lower empty cassette eyes',height=0.);carried=0;crane('lower cassette eye latches engage',grip=1.)
 local('lower buffer releases',buffer_release=1.);docks.pop('0')
 crane('empty cassette lifts through open protected shelf',0,height=5.1)
 local('protected floor closes below lifted cassette',floor=0.)
 local('protected supports extend below lifted cassette',support=0.)
 crane('empty cassette seats protected pads',0,height=4.8)
 dock('protected',0.);docks['0']=['protected',0];carried=None
 crane('protected hoist latches release',grip=0.)
 crane('hoist lifts clear',height=5.1);crane('hoist returns local safe rest',z=-22.8)
 local('empty lower buffer resets',buffer_release=0.)
 leg=3;emit('local exchange complete lower launch corridor clear')
 return frames

def translation(doc,g,s):
 v=s['local']
 if g.startswith('cell00/loader/'):
  t=[0.,0.,v['z']+22.8]
  if g.endswith('/bridge'):t[2]=0.
  if '/hoist/' in g:t[1]=-(5.1-v['height'])*int(g.rsplit('/',1)[1])/4
  if '/eye-latch/' in g:t[0]=int(g.rsplit('/',1)[1])*.035*v['grip'];t[1]=v['height']-5.1
  return t
 if g=='cell00/protected/floor':return [0.,0.,6.6*v['floor']]
 if g.startswith('cell00/protected/support/'):return [int(g.rsplit('/',1)[1])*.5*v['support'],0.,0.]
 if g.startswith('cell00/shuttle/clamp/'):
  return [int(g.rsplit('/',1)[1])*.12*v['shuttle_clamp'],v['shuttle_lift'],v['shuttle_z']+22.8]
 if g.startswith('cell00/shuttle/'):
  return [0.,v['shuttle_lift'] if g.endswith('/lift') else 0.,v['shuttle_z']+22.8]
 if g.startswith('cell00/buffer/latch/'):return [-int(g.rsplit('/',1)[1])*.15*v['buffer_release'],0.,0.]
 t=old.translation(doc,g,s)
 if g.startswith('dock/protected/0/'):
  # Existing jaws travel with the retractable source tracks.
  center=sum(next(r['bounds_body_m'] for r in doc['parts'] if r.get('group')==g)[i][0] for i in (0,1))/2
  t[0]+=(1 if center>-26 else -1)*.5*v['support']
 return t
