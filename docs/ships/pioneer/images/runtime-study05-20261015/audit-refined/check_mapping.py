import bpy,pathlib,json,hashlib,sys,collections
ROOT=pathlib.Path('/workspace/arkline-engine');OUT=pathlib.Path('/workspace/arkline-runtime/study05-audit-v2');SOURCE=pathlib.Path('/workspace/arkline-runtime/study05-authored/source-v3/pioneer-study05.blend');BUNDLE=pathlib.Path('/workspace/arkline-runtime/study05-authored/export-v3');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();files=[SOURCE,BUNDLE/'pioneer-study05.blend',BUNDLE/'pioneer.glb',BUNDLE/'receipt.json'];before={str(p):sha(p) for p in files}
assert before[str(SOURCE)]=='61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9';assert before[str(BUNDLE/'pioneer.glb')]=='8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123';assert before[str(BUNDLE/'receipt.json')]=='fadece504f99169f645559f5c903636b6555f8d8b3f3c3b856cc267b639d5429';assert before[str(SOURCE)]==before[str(BUNDLE/'pioneer-study05.blend')]
sys.path.insert(0,str(ROOT/'tools/art'));from pioneer_export_contract import MATERIAL_NAMES,validate_glb,unpack_glb
r=json.loads((BUNDLE/'receipt.json').read_text());b=(BUNDLE/'pioneer.glb').read_bytes();decoded=validate_glb(b);doc,blob=unpack_glb(b);assert decoded==r['output'] or all(decoded[k]==r['output'][k] for k in decoded)
parts=r['source_parts'];assert len(parts)==len(set(p['name'] for p in parts))==686
assert len(r['material_identity'])==9
for m in r['material_identity']:
 assert m['source_id']=='material:'+hashlib.sha256((before[str(SOURCE)]+'\0'+m['name']).encode()).hexdigest()
 assert m['render_material_index']==MATERIAL_NAMES.index(m['name'])
for k,v in r['exporter'].items():assert sha(ROOT/'tools/art'/k)==v
bpy.context.preferences.filepaths.use_scripts_auto_execute=False;bpy.ops.wm.open_mainfile(filepath=str(SOURCE),load_ui=False,use_scripts=False);dg=bpy.context.evaluated_depsgraph_get();objects={o.name:o for o in bpy.context.scene.objects if o.type=='MESH'};assert set(objects)==set(p['name'] for p in parts)
byprimitive=collections.defaultdict(list);sourcecounts={};evaluated=0
for p in parts:
 o=objects[p['name']];assert len(o.material_slots)==1 and o.material_slots[0].material.name==p['material'];assert p['render_primitive']==p['render_material_index']==MATERIAL_NAMES.index(p['material']);assert p['source_id']=='part:'+hashlib.sha256((before[str(SOURCE)]+'\0'+p['name']).encode()).hexdigest();assert p['source_mesh_name']==o.data.name;assert p['source_world_blender']==[[float(x) for x in row] for row in o.matrix_world]
 ev=o.evaluated_get(dg);me=ev.to_mesh(preserve_all_data_layers=True,depsgraph=dg)
 try:me.calc_loop_triangles();n=len(me.loop_triangles);assert n==p['evaluated_triangles'];sourcecounts[p['name']]=n;evaluated+=n
 finally:ev.to_mesh_clear()
 assert p['index_count']==3*p['retained_triangles'];assert p['retained_triangles']+p['dropped_triangles']==p['evaluated_triangles'];byprimitive[p['render_primitive']].append(p)
for i,primitive in enumerate(doc['meshes'][0]['primitives']):
 cursor=0
 for p in sorted(byprimitive[i],key=lambda p:p['first_index']):assert p['first_index']==cursor;cursor+=p['index_count']
 assert cursor==doc['accessors'][primitive['indices']]['count']
assert evaluated==r['degeneracy']['evaluated_triangles'];assert sum(p['retained_triangles'] for p in parts)==decoded['triangles'];after={str(p):sha(p) for p in files};assert before==after
report={'schema':'pioneer-study05-mapping-readonly/v1','outcome':'PASS exact source copy, source object/mesh/material snapshot identity, canonical9slots, complete686part spans and decoded runtime-gate data','script_sha256':sha(pathlib.Path(__file__)),'hashes_before':before,'hashes_after':after,'models_unchanged':True,'autoexecution_disabled':True,'decoded':decoded,'source_parts':len(parts),'parts_with_retained_triangles':sum(p['retained_triangles']>0 for p in parts),'evaluated_triangles':evaluated,'dropped_triangles':r['degeneracy']['dropped_triangles'],'canonical_materials':list(MATERIAL_NAMES),'receipt_recipe_hashes_matching_current_code':r['exporter'],'all_names_unique_and_exact':True,'all_source_mesh_names_and_world_matrices_match_receipt':True,'all_part_material_slots_match_receipt':True,'source_hash_name_snapshot_ids_match':True,'part_spans_contiguous_complete':True,'limits':['Part IDs are sourcehash+exactname snapshot identities, not durable cross-revision IDs. Changed source/hash cannot retarget old graphs.','The portable decoded float32 gate was exercised; actualRustdecoder/GPU/native appearance acceptance remains root responsibility.','No source/model/export saves or edits, builds or publication.']}
p=OUT/'mapping.json';p.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
