#!/usr/bin/env python3
"""First local parallel-reloader cell fit. Unregistered; all old sources immutable.
Derived from exact composed1702. Lower empty-cassette shuttle, local upper
hoist, retracting protected dock and real shelf opening; no capacity increase.
"""
import argparse,copy,hashlib,json
from pathlib import Path
import bpy,bmesh
SOURCE='790f6662b26c619a781bac5d29aaf41d90c5fc23ab76d777bc166b31f76288d9'
PREFIX='pioneer.magazine_ready/'
FACES=[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
def main():
 p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);p.add_argument('out_manifest',type=Path);a=p.parse_args()
 assert bpy.app.version==(5,0,1);assert hashlib.sha256(a.source.read_bytes()).hexdigest()==SOURCE
 assert not a.output.exists() and not a.out_manifest.exists()
 bpy.context.preferences.filepaths.use_scripts_auto_execute=False
 bpy.ops.wm.open_mainfile(filepath=str(a.source),load_ui=False,use_scripts=False)
 doc=json.loads(a.manifest.read_text());parts=copy.deepcopy(doc['parts']);refs=copy.deepcopy(doc['mechanism_reference_poses']);removed=[]
 def delete(record):
  obj=bpy.data.objects.get(record['semantic_id']);assert obj
  bpy.data.objects.remove(obj,do_unlink=True);parts.remove(record);removed.append(record['semantic_id'])
 def shift(record,delta):
  obj=bpy.data.objects[record['semantic_id']];obj.location.x+=delta[0];obj.location.y-=delta[2];obj.location.z+=delta[1]
  record['bounds_body_m']=[[v+delta[i] for i,v in enumerate(b)] for b in record['bounds_body_m']]
 def box(name,lo,hi,group=None,material='Titanium machinery'):
  name=PREFIX+name;assert name not in bpy.data.objects
  points=[(lo[0] if i==0 else hi[0],lo[1] if j==0 else hi[1],lo[2] if k==0 else hi[2]) for i,j,k in [(0,0,0),(0,0,1),(0,1,1),(0,1,0),(1,0,0),(1,0,1),(1,1,1),(1,1,0)]]
  data=bpy.data.meshes.new(name+' mesh');data.from_pydata([(x,-z,y) for x,y,z in points],[],FACES);data.update()
  bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(data);bm.free()
  obj=bpy.data.objects.new(name,data);bpy.context.collection.objects.link(obj);data.materials.append(bpy.data.materials[material]);obj['semantic_id']=name
  if group:obj['kinematic_group']=group;refs.setdefault(group,{'position_m':[-26.,0.,-22.8],'orientation_xyzw':[0.,0.,0.,1.]})
  parts.append({'semantic_id':name,'group':group,'material':material,'bounds_body_m':[list(lo),list(hi)]})
 # FIRST CELL ONLY: remove the obsolete shared port machine; reuse its exact
 # hollow telescopes/eyes at fixed column X=-26. Other source stock is untouched.
 for r in list(parts):
  g=r.get('group') or ''
  if g.startswith('port/loader/'):
   old=r['semantic_id'];new=PREFIX+'cell00/'+old.split('/magazine/',1)[1]
   obj=bpy.data.objects[old];obj.name=new;obj['semantic_id']=new;r['semantic_id']=new
   newg=g.replace('port/loader/','cell00/loader/');obj['kinematic_group']=newg;r['group']=newg
   shift(r,[-40.,0.,0.]);refs[newg]={'position_m':[-26.,0.,-22.8],'orientation_xyzw':[0.,0.,0.,1.]}
 for g in list(refs):
  if g.startswith('port/loader/'):refs.pop(g)
 # Relocate external leaves/portal to the outer end of the existing outboard
 # tunnel. This avoids running new floor rails through a vertically opening door.
 for r in list(parts):
  name=r['semantic_id']
  if name.startswith(PREFIX+'b00/') and any(tag in name for tag in ['/door ','/portal ','/upper portal pressure wall']):shift(r,[0.,0.,-7.7])
 # Remove one solid ready floor and open the protected parking shelf. Keep
 # unrelated four columns exactly as they were: this is a one-cell fit fixture.
 for r in list(parts):
  name=r['semantic_id']
  if name==PREFIX+'b00/floor':delete(r)
  elif name in (PREFIX+'port/lower continuous casing',PREFIX+'port/lower shoulder'):
   lo,hi=copy.deepcopy(r['bounds_body_m']);label=name.rsplit('/',1)[1];delete(r)
   box('cell00/'+label+' retained aft',lo,[-29.55,hi[1],hi[2]],material='Graphite ceramic armor')
   box('cell00/'+label+' retained forward',[-22.45,lo[1],lo[2]],hi,material='Graphite ceramic armor')
   if lo[1]<-2.95:box('cell00/lower shoulder beneath service channel',[-29.55,lo[1],lo[2]],[-22.45,-2.95,hi[2]],material='Graphite ceramic armor')
  elif name==PREFIX+'port/magazine/parking deck':
   delete(r)
   box('cell00/parking retained aft',[-31.2,2.85,-33.6],[-29.55,3.,-26.1])
   box('cell00/parking retained forward',[-22.45,2.85,-33.6],[18.,3.,-26.1])
  elif name==PREFIX+'port/magazine/fixed aperture front floor':
   delete(r)
   box('cell00/strip retained aft',[-31.2,2.85,-26.1],[-29.55,3.,-25.88])
   box('cell00/strip retained forward',[-22.45,2.85,-26.1],[10.2,3.,-25.88])
 # Protected floor plate slides inboard above the CLOSED ready hatch. Its
 # opening is wider than actual6.72m cassette and is not a phantom shaft.
 box('cell00/protected floor plate',[-29.4,2.90,-33.2],[-22.6,3.,-26.8],'cell00/protected/floor')
 for direction in [-1,1]:
  x=-26+direction*3.425
  edge_lo,edge_hi=sorted((-26+direction*3.37,-26+direction*3.55))
  box(f'cell00/protected plate lower capture {direction}',[edge_lo,2.85,-33.25],[edge_hi,2.90,-20.15])
  for k,(za,zb) in enumerate([(-33.25,-32.98),(-32.52,-27.48),(-27.02,-20.15)]):
   box(f'cell00/protected plate outer guide {direction}/{k}',[x-.025,2.90,za],[x+.025,3.08,zb])
   box(f'cell00/protected plate upper capture {direction}/{k}',[edge_lo,3.,za],[edge_hi,3.08,zb])
 # Protected support pads AND tracks withdraw0.5m outward inX, not just the jaws.
 for r in parts:
  name=r['semantic_id']
  if '/port/magazine/port/reserve/0/dock support ' in name or name.startswith(PREFIX+'dock/protected/0/'):
   direction=-1 if sum(b[0] for b in r['bounds_body_m'])/2 < -26 else 1
   if not r.get('group'):
    g=f'cell00/protected/support/{direction}';r['group']=g;bpy.data.objects[name]['kinematic_group']=g;refs[g]={'position_m':[-26.,0.,-22.8],'orientation_xyzw':[0.,0.,0.,1.]}
 # Captured short X slides retain each protected pad/track assembly.
 for direction in [-1,1]:
  for z in [-32.75,-27.25]:
   ga=f'cell00/protected/support/{direction}'
   xa,xb=sorted((-26+direction*3.25,-26+direction*3.45))
   box(f'cell00/protected pad tongue {direction}/{z}',[xa,3.,z-.12],[xb,3.05,z+.12],ga)
   xa,xb=sorted((-26+direction*3.4,-26+direction*3.95))
   box(f'cell00/protected pad guide floor {direction}/{z}',[xa,2.98,z-.15],[xb,3.,z+.15])
   for side in [-1,1]:
    za,zb=sorted((z+side*.13,z+side*.15))
    box(f'cell00/protected pad guide wall {direction}/{z}/{side}',[xa,3.,za],[xb,3.08,zb])
    za,zb=sorted((z+side*.10,z+side*.15))
    box(f'cell00/protected pad capture {direction}/{z}/{side}',[xa,3.05,za],[xb,3.08,zb])
 # A real lower service floor and continuous rail channels, below the complete
 # outward drone envelope. Four shoes pick up fixed cassette beams, not moving tray.
 box('cell00/lower structural floor',[-29.55,-2.95,-33.3],[-22.45,-2.80,-19.35],material='Graphite ceramic armor')
 for dx in [-2.5,2.5]:
  x=-26+dx
  box(f'cell00/lower rail web {dx}',[x-.12,-2.80,-33.2],[x+.12,-2.65,-19.5])
  box(f'cell00/lower rail cap {dx}',[x-.16,-2.65,-33.2],[x+.16,-2.60,-19.5])
  box(f'cell00/shuttle side {dx}',[x-.22,-2.60,-23.1],[x+.22,-2.52,-21.0],'cell00/shuttle/travel')
  for flank in [-1,1]:
   xx=x+flank*.195
   box(f'cell00/shuttle guide {dx}/{flank}',[xx-.025,-2.75,-23.1],[xx+.025,-2.52,-21.0],'cell00/shuttle/travel')
   xa,xb=sorted((x+flank*.13,x+flank*.22))
   box(f'cell00/shuttle under cap retainer {dx}/{flank}',[xa,-2.68,-23.1],[xb,-2.65,-21.0],'cell00/shuttle/travel')
  box(f'cell00/shuttle lift rail {dx}',[x-.10,-1.95,-23.1],[x+.10,-1.88,-21.0],'cell00/shuttle/lift')
  for dz in [0.,1.5]:
   z=-22.8+dz
   box(f'cell00/shuttle pickup {dx}/{dz}',[x-.09,-1.88,z-.14],[x+.09,-1.465,z+.14],'cell00/shuttle/lift')
   # Clamp slides are offset from vertical jack sleeves, with welded bridge to pickup.
   cz=z+.35
   box(f'cell00/shuttle clamp bridge {dx}/{dz}',[x-.09,-1.73,z-.02],[x+.09,-1.71,cz+.1],'cell00/shuttle/lift')
   box(f'cell00/shuttle clamp lower guide {dx}/{dz}',[x-.35,-1.73,cz-.10],[x+.35,-1.71,cz+.10],'cell00/shuttle/lift')
   box(f'cell00/shuttle clamp upper guide {dx}/{dz}',[x-.14,-1.66,cz-.10],[x+.14,-1.64,cz+.10],'cell00/shuttle/lift')
   for flank in [-1,1]:
    za,zb=sorted((cz+flank*.10,cz+flank*.12))
    box(f'cell00/shuttle clamp guide back {dx}/{dz}/{flank}',[x-.14,-1.71,za],[x+.14,-1.64,zb],'cell00/shuttle/lift')
   for side in [-1,1]:
    ga=f'cell00/shuttle/clamp/{side}'
    xa,xb=sorted((x+side*.10,x+side*.20))
    box(f'cell00/shuttle clamp slider {dx}/{dz}/{side}',[xa,-1.70,cz-.08],[xb,-1.67,cz+.08],ga)
    xa,xb=sorted((x+side*.165,x+side*.195))
    box(f'cell00/shuttle clamp shoe {dx}/{dz}/{side}',[xa,-1.67,cz-.08],[xb,-1.32,cz+.08],ga)
   # Nested open guide sleeves retain overlap across the0.2m pickup stroke.
   for axis in [0]:
    for sign in [-1,1]:
     lo=[x-.13,-2.52,z-.13];hi=[x+.13,-1.78,z+.13]
     lo[axis]=(x if axis==0 else z)+(.105 if sign>0 else -.13)
     hi[axis]=(x if axis==0 else z)+(.13 if sign>0 else -.105)
     box(f'cell00/shuttle lift sleeve {dx}/{dz}/{axis}/{sign}',lo,hi,'cell00/shuttle/travel')
   box(f'cell00/shuttle lift stem {dx}/{dz}',[x-.10,-2.10,z-.10],[x+.10,-1.95,z+.10],'cell00/shuttle/lift')
 # Buffer is explicitly empty-cassette transit, never21st stock capacity.
 for dx in [-3.25,3.25]:
  for dz in [-2.75,2.75]:
   x=-26+dx;z=-30+dz
   box(f'cell00/buffer support {dx}/{dz}',[x-.12,-2.80,z-.12],[x+.12,-1.75,z+.12])
   direction=1 if dx>0 else -1
   lo,hi=sorted((x-direction*.095,x-direction*.045))
   box(f'cell00/buffer jaw {dx}/{dz}',[lo,-1.75,z-.07],[hi,-1.60,z+.07],f'cell00/buffer/latch/{direction}',material='Safety amber')
 # Group references remain explicit; no runtime source adoption is implied.
 doc.update({'status':'UNACCEPTED first local parallel reload cell; source fit only','source_sha256':SOURCE,'parts':parts,'mechanism_reference_poses':refs,'parallel_local_cells':[{'cell_id':0,'ready_berth':0,'protected_slot':0,'empty_buffer_center_m':[-26.,0.,-30.],'loader_prefix':'cell00/loader/','shuttle_prefix':'cell00/shuttle/','protected_pad_retract_m':.5,'capacity_added':0}],'removed_source_parts':removed})
 bpy.ops.wm.save_as_mainfile(filepath=str(a.output),check_existing=False)
 doc['output_sha256']=hashlib.sha256(a.output.read_bytes()).hexdigest();a.out_manifest.write_text(json.dumps(doc,indent=2));print(doc['output_sha256'],len(parts),len(refs))
if __name__=='__main__':main()
