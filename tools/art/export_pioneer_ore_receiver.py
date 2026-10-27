#!/usr/bin/env python3
"""Export only the additive receiver, without relaxing the full hull contract.
One body-space mesh, material-grouped indexed primitives, exact part spans.
"""
import argparse, hashlib, json, math, struct
from pathlib import Path
import bpy
from export_pioneer import material_record
from pioneer_export_contract import affine_and_normal_matrix, transform_point, transform_normal, runtime_cross_squared, f32

def export(source, authoring, output):
    assert bpy.app.version == (5,0,1)
    before=hashlib.sha256(source.read_bytes()).hexdigest();a=json.loads(authoring.read_text());assert before==a['output_sha256']
    assert not output.exists();output.mkdir(parents=True)
    bpy.context.preferences.filepaths.use_scripts_auto_execute=False
    bpy.ops.wm.open_mainfile(filepath=str(source.resolve()),load_ui=False,use_scripts=False)
    assert not bpy.data.libraries and not bpy.data.actions
    expected={p['name']:p for p in a['parts']};assert set(expected)==set(o.name for o in bpy.data.objects)
    names=sorted(set(p['material'] for p in a['parts']));materials=[material_record(bpy.data.materials[n],before)[0] for n in names]
    parts=[];dropped={};graph=bpy.context.evaluated_depsgraph_get()
    for o in sorted(bpy.data.objects,key=lambda o:o.name):
        assert o.type=='MESH' and o.parent is None and not o.constraints and not o.animation_data
        assert all(m.type in {'BEVEL','WEIGHTED_NORMAL'} and m.show_viewport and m.show_render for m in o.modifiers)
        assert len(o.material_slots)==1 and o.material_slots[0].material.name==expected[o.name]['material']
        e=o.evaluated_get(graph);matrix=[list(row) for row in e.matrix_world];normal_matrix=affine_and_normal_matrix(matrix);m=e.to_mesh(preserve_all_data_layers=True,depsgraph=graph)
        try:
            m.calc_loop_triangles();positions=[transform_point(matrix,v.co) for v in m.vertices];normals=[transform_normal(normal_matrix,n.vector) for n in m.corner_normals];corners=[]
            for t in m.loop_triangles:
                vs=[positions[m.loops[i].vertex_index] for i in t.loops]
                if runtime_cross_squared(vs)<=f32(1e-20):
                    dropped[o.name]=dropped.get(o.name,0)+1
                    continue
                corners.extend((positions[m.loops[i].vertex_index],normals[i]) for i in t.loops)
            parts.append((o.name,o.material_slots[0].material.name,corners))
        finally:e.to_mesh_clear()
    doc={'asset':{'version':'2.0','generator':'arkline.additive-ore-receiver-export/v1'},'extensionsUsed':['KHR_materials_emissive_strength'],'scene':0,'scenes':[{'nodes':[0]}],'nodes':[{'mesh':0,'name':'Pioneer additive ore receiver body-space'}],'meshes':[{'primitives':[]}],'materials':materials,'buffers':[],'bufferViews':[],'accessors':[]}
    binary=bytearray();spans=[]
    def accessor(values,width,kind,target):
        i=len(doc['accessors']);start=len(binary);fmt='<'+('f' if kind==5126 else 'I')*width
        for value in values:binary.extend(struct.pack(fmt,*value))
        doc['bufferViews'].append({'buffer':0,'byteOffset':start,'byteLength':len(binary)-start,'target':target})
        a={'bufferView':i,'componentType':kind,'count':len(values),'type':'VEC3' if width==3 else 'SCALAR'}
        if width==3:a.update(min=[min(x[k] for x in values) for k in range(3)],max=[max(x[k] for x in values) for k in range(3)])
        doc['accessors'].append(a);return i
    all_positions=[]
    for pi,material in enumerate(names):
        vertices=[]
        for name,mat,corners in parts:
            if mat!=material:continue
            start=len(vertices);vertices.extend(corners);p=expected[name]
            positions=[list(v[0]) for v in corners];bounds=[[min(v[k] for v in positions) for k in range(3)],[max(v[k] for v in positions) for k in range(3)]]
            assert all(bounds[0][k]>=p['bounds_body_m'][0][k]-1e-5 and bounds[1][k]<=p['bounds_body_m'][1][k]+1e-5 for k in range(3)),name
            spans.append(dict(name=name,source_id='part:'+hashlib.sha256((before+'\0'+name).encode()).hexdigest(),render_primitive=pi,first_index=start,index_count=len(corners),retained_triangles=len(corners)//3,material=material,group=p['group'],bounds_body_m=bounds))
        positions=[tuple(f32(x) for x in p) for p,n in vertices];normals=[tuple(f32(x) for x in n) for p,n in vertices]
        assert all(abs(sum(x*x for x in n)-1)<1e-5 for n in normals)
        pos=accessor(positions,3,5126,34962);normal=accessor(normals,3,5126,34962);indices=accessor([(i,) for i in range(len(vertices))],1,5125,34963)
        doc['meshes'][0]['primitives'].append({'attributes':{'POSITION':pos,'NORMAL':normal},'indices':indices,'material':pi,'mode':4});all_positions.extend(positions)
    doc['buffers']=[{'byteLength':len(binary)}];encoded=json.dumps(doc,separators=(',',':'),allow_nan=False).encode();encoded+=b' '*((-len(encoded))%4);binary+=b'\0'*((-len(binary))%4)
    data=struct.pack('<III',0x46546c67,2,28+len(encoded)+len(binary))+struct.pack('<II',len(encoded),0x4e4f534a)+encoded+struct.pack('<II',len(binary),0x004e4942)+binary
    (output/'ore-receiver.glb').write_bytes(data)
    roots={g:{'position_m':a['reference_root_body_m'],'orientation_xyzw':[0,0,0,1]} for g in a['dynamic_groups']}
    receipt={'schema':'arkline.pioneer-ore-receiver-export/v1','receiver_revision':1,'base_registration':12,'base_glb_sha256':a['base_glb_sha256'],'source':{'sha256':before,'size_bytes':source.stat().st_size},'output':{'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data),'triangles':len(all_positions)//3,'primitives':len(names),'body_bounds_m':[[min(v[k] for v in all_positions) for k in range(3)],[max(v[k] for v in all_positions) for k in range(3)]]},'source_parts':spans,'degenerate_policy':'drop float32 cross-squared <=1e-20; record every discarded triangle count','dropped_triangles_by_part':dropped,'mechanism_reference_poses':roots,'coordinates':'body metres, identity node; transform dynamic ranges current_root * inverse(reference_root) once','exporter':{'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'blender':bpy.app.version_string},'authoring_sha256':hashlib.sha256(authoring.read_bytes()).hexdigest(),'status':'source-only candidate; native/authority acceptance separate'}
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');assert hashlib.sha256(source.read_bytes()).hexdigest()==before
    print(json.dumps(receipt['output']))
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--authoring',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();export(a.source,a.authoring,a.output)
