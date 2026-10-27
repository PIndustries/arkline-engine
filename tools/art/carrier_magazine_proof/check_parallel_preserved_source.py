"""Compare retained Blender source objects using explicit archived proof inputs."""
import argparse, json, hashlib, struct, sys
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline-source', type=Path, required=True)
parser.add_argument('--candidate-source', type=Path, required=True)
parser.add_argument('--manifest', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else None)
# Parse the explicit paths before importing Blender so --help stays portable.
import bpy

a = args.baseline_source
b = args.candidate_source
allowed = set(json.loads(args.manifest.read_text())['removed_obsolete_rack'])
assert len(allowed) == 138 and all(x.startswith('pioneer.drone_bay/rack/') for x in allowed)
def snap(path):
 bpy.context.preferences.filepaths.use_scripts_auto_execute=False
 bpy.ops.wm.open_mainfile(filepath=str(path),load_ui=False,use_scripts=False)
 graph=bpy.context.evaluated_depsgraph_get();out={}
 for o in bpy.data.objects:
  if o.name in allowed or o.name.startswith('pioneer.magazine_ready/'):continue
  h=hashlib.sha256();h.update(repr((o.type,list(o.matrix_world),sorted((k,str(o[k])) for k in o.keys()))).encode())
  if o.type in {'MESH','FONT','CURVE','SURFACE','META'}:
   ev=o.evaluated_get(graph);m=ev.to_mesh(preserve_all_data_layers=True,depsgraph=graph)
   for v in m.vertices:h.update(struct.pack('<3d',*v.co))
   for p in m.polygons:h.update(struct.pack('<I',len(p.vertices)));h.update(struct.pack('<'+'I'*len(p.vertices),*p.vertices));h.update(struct.pack('<I',p.material_index))
   h.update(repr([m.name if m else None for m in m.materials]).encode());ev.to_mesh_clear()
  out[o.name]=h.hexdigest()
 return out
before=snap(a);after=snap(b);changed=[n for n in before if before[n]!=after.get(n)];added=sorted(set(after)-set(before));r={'source_sha256':hashlib.sha256(a.read_bytes()).hexdigest(),'combined_sha256':hashlib.sha256(b.read_bytes()).hexdigest(),'untouched_objects_compared':len(before),'explicitly_retired_obsolete_rack_objects':sorted(allowed),'changed_or_missing':changed,'unexpected_added':added,'passed':not changed and not added,'method':'Exact evaluated local vertices, polygon indices/material indices, material names, object matrix/type/custom semantic properties; only new magazine/loader objects and138explicitly retired obsolete central rack objects excluded; all crew/hull/bridge/gear/turret objects compared.'};args.output.write_text(json.dumps(r,indent=2));print(json.dumps(r));assert r['passed']
