"""Read-only Study05 raw/evaluated orientation and index-topology evidence."""
import collections
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
import bpy
import numpy as np

SOURCE = Path('/workspace/arkline-runtime/study05-authored/source-v3/pioneer-study05.blend')
MANIFEST = SOURCE.with_name('study05.json')
OUT = Path(__file__).resolve().parent
EXPECTED = '61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9'
BODY = np.array([[1.,0.,0.], [0.,0.,1.], [0.,-1.,0.]])
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()


def statistics(mesh, world):
    mesh.calc_loop_triangles()
    vertices = np.array([v.co[:] for v in mesh.vertices], dtype=float)
    vertices = (vertices @ world[:3,:3].T + world[:3,3]) @ BODY.T
    tris = np.array([t.vertices[:] for t in mesh.loop_triangles], dtype=int)
    if not len(vertices) or not len(tris):
        return {'vertices': len(vertices), 'triangles': len(tris), 'empty': True}
    points = vertices[tris]
    centered = points - (vertices.min(axis=0)+vertices.max(axis=0))/2
    face_cross = np.cross(points[:,1]-points[:,0], points[:,2]-points[:,0])
    lengths = np.linalg.norm(face_cross, axis=1)
    six_volumes = np.einsum('ij,ij->i', centered[:,0],
                           np.cross(centered[:,1], centered[:,2]))
    edges = collections.defaultdict(list)
    for index, triangle in enumerate(tris):
        for a, b in zip(triangle, np.roll(triangle, -1)):
            edges[tuple(sorted((int(a),int(b))))].append((index, int(a)<int(b)))
    incidents = collections.Counter(len(items) for items in edges.values())
    same = sum(len(es)==2 and es[0][1]==es[1][1] for es in edges.values())
    adjacency = [[] for _ in tris]
    for items in edges.values():
        for item in items[1:]:
            adjacency[items[0][0]].append(item[0])
            adjacency[item[0]].append(items[0][0])
    components, seen = [], set()
    for index in range(len(tris)):
        if index in seen:
            continue
        pending, indices = [index], []
        seen.add(index)
        while pending:
            current = pending.pop()
            indices.append(current)
            for neighbor in adjacency[current]:
                if neighbor not in seen:
                    seen.add(neighbor)
                    pending.append(neighbor)
        components.append({'triangles': len(indices),
                           'signed_volume_m3': float(six_volumes[indices].sum()/6)})
    # Read existing split/corner normals without recalculation or mesh repair.
    normals = np.array([item.vector[:] for item in mesh.corner_normals],dtype=float)
    normal_lengths = np.linalg.norm(normals,axis=1)
    zero_normals = normal_lengths<=1e-12
    invalid_normals = ~np.isfinite(normals).all(axis=1)
    loop_indices = np.array([t.loops[:] for t in mesh.loop_triangles],dtype=int)
    # Mirror export's final position-cyclic order and identity-node f32 gate.
    p32 = points.astype(np.float32)
    rotations = np.lexsort((p32[:,:,2],p32[:,:,1],p32[:,:,0]),axis=1)[:,0]
    order = (np.arange(3)[None,:]+rotations[:,None])%3
    p32 = np.take_along_axis(p32,order[:,:,None],axis=1)/np.float32(100.)
    a32,b32=p32[:,1]-p32[:,0],p32[:,2]-p32[:,0]
    c32=np.cross(a32,b32)
    gate32=(c32[:,0]*c32[:,0]+c32[:,1]*c32[:,1])+c32[:,2]*c32[:,2]
    retained=gate32>np.float32(1e-20)
    normal_audit={'corner_normals':len(normals),'loops':len(mesh.loops),
        'zero_corner_normals_le_1e_minus_12':int(zero_normals.sum()),
        'nonfinite_corner_normals':int(invalid_normals.sum()),
        'minimum_corner_normal_length':float(normal_lengths.min()),
        'runtime_retained_triangles':int(retained.sum()),
        'runtime_retained_triangles_with_zero_corner_normal':int((retained & zero_normals[loop_indices].any(axis=1)).sum()),
        'runtime_retained_triangles_with_nonfinite_corner_normal':int((retained & invalid_normals[loop_indices].any(axis=1)).sum())}
    return {'vertices': len(vertices), 'triangles': len(tris),
            'bounds_body_m': [vertices.min(axis=0).tolist(),vertices.max(axis=0).tolist()],
            'signed_volume_m3': float(six_volumes.sum()/6),
            'corner_normal_audit': normal_audit,
            'edges_by_incident_triangle_count': dict(incidents),
            'same_direction_two_incident_edges': same,
            'closed_consistently_oriented_by_index': bool(incidents.get(1,0)==0 and
                all(count==2 for count in incidents) and same==0),
            'zero_area_body_triangles_le_1e_minus_12_m2_twice': int((lengths<=1e-12).sum()),
            'components': components}


def main():
    before = sha(SOURCE)
    assert before == EXPECTED
    manifest = json.loads(MANIFEST.read_text())
    groups = {}
    for op_name, operation in manifest['operations'].items():
        names = set()
        for key in ('changed_objects','new_objects','changed_parts','added_parts'):
            names.update(operation.get(key, []))
        groups[op_name] = names
    selected = set.union(*groups.values())
    bpy.context.preferences.filepaths.use_scripts_auto_execute = False
    bpy.ops.wm.open_mainfile(filepath=str(SOURCE), load_ui=False, use_scripts=False)
    assert bpy.app.version == (5,0,1)
    assert not bpy.context.preferences.filepaths.use_scripts_auto_execute
    assert Path(bpy.data.filepath).resolve() == SOURCE.resolve()
    graph = bpy.context.evaluated_depsgraph_get()
    records = []
    lower, upper = np.full(3, np.inf), np.full(3, -np.inf)
    all_names = {o.name for o in bpy.context.scene.objects}
    assert not selected-all_names, sorted(selected-all_names)
    for obj in sorted(bpy.context.scene.objects, key=lambda item: item.name):
        assert obj.type == 'MESH', (obj.name,obj.type)
        evaluated = obj.evaluated_get(graph)
        mesh = evaluated.to_mesh(preserve_all_data_layers=True, depsgraph=graph)
        try:
            world = np.array(evaluated.matrix_world, dtype=float)
            vertices = np.array([v.co[:] for v in mesh.vertices], dtype=float)
            body_vertices = (vertices @ world[:3,:3].T + world[:3,3]) @ BODY.T
            lower = np.minimum(lower,body_vertices.min(axis=0))
            upper = np.maximum(upper,body_vertices.max(axis=0))
            if obj.name in selected:
                records.append({'name':obj.name,
                    'groups':[group for group,names in groups.items() if obj.name in names],
                    'material':obj.material_slots[0].material.name,
                    'world_determinant':float(np.linalg.det(world[:3,:3])),
                    'raw':statistics(obj.data,np.array(obj.matrix_world,dtype=float)),
                    'evaluated':statistics(mesh,world)})
        finally:
            evaluated.to_mesh_clear()
    after = sha(SOURCE)
    assert before == after
    problems = {}
    for stage in ('raw','evaluated'):
        problems[stage] = {
            'nonclosed_or_inconsistent': [r['name'] for r in records
                if not r[stage].get('closed_consistently_oriented_by_index',False)],
            'negative_total_volume': [r['name'] for r in records
                if r[stage].get('signed_volume_m3',0)<-1e-8],
            'negative_components': [{'name':r['name'],'component':c} for r in records
                for c in r[stage].get('components',[]) if c['signed_volume_m3'] < -1e-8],
            'zero_or_nonfinite_normals_on_retained_triangles': [{'name':r['name'],'audit':r[stage]['corner_normal_audit']} for r in records if r[stage]['corner_normal_audit']['runtime_retained_triangles_with_zero_corner_normal'] or r[stage]['corner_normal_audit']['runtime_retained_triangles_with_nonfinite_corner_normal']],
            'zero_area_triangle_count': sum(r[stage].get('zero_area_body_triangles_le_1e_minus_12_m2_twice',0)
                                            for r in records)}
    report = {'schema':'arkline.study05-geometry-readonly/v1',
        'source':str(SOURCE),'sha256_before':before,'sha256_after':after,'source_unchanged':before==after,
        'manifest_sha256':sha(MANIFEST),'script_sha256':sha(Path(__file__)),
        'blender':bpy.app.version_string,'blender_build_hash':bpy.app.build_hash.decode(),
        'autoexecution_disabled':True,'body_rotation_determinant':float(np.linalg.det(BODY)),
        'source_mesh_objects':len(bpy.context.scene.objects),'bounds_body_m':[lower.tolist(),upper.tolist()],
        'selected_objects':len(records),
        'arm_parts_normal_audits':[{'name':r['name'],'raw':r['raw']['corner_normal_audit'],'evaluated':r['evaluated']['corner_normal_audit']} for r in records if 'handling arm' in r['name']],'group_counts':{k:len(v) for k,v in groups.items()},
        'problems':problems,'objects':records,
        'method':'Source names in the authored Study05 operation receipt select changed/new bow,roof,aft parts. Raw and evaluated meshes are triangulated read-only. Signed volume=sum dot(a-origin,cross(b-origin,c-origin))/6,origin=AABB center. Index edges count incident triangles and directed winding. Components use shared index edges. No welding, repair or normal recalculation.',
        'limits':['Signed volume establishes orientation for closed consistently oriented components; open/degenerate components have no reliable inside/outside from this scalar alone.',
                  'Index-topology degeneracies in evaluated bevels are reported separately; no runtime drop predicate is applied here.',
                  'Negative component/total threshold is -1e-8m3 for numerical classification, not engineering accuracy.',
                  'Separate aperture review identifies first-hit source geometry; no native render, canonical adoption or physics inference is made.',
                  'No source/model/export save, repository edit, build, Cargo command, Mac access or publication.']}
    (OUT/'study05-geometry.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['source_unchanged','source_mesh_objects','bounds_body_m',
                       'selected_objects','group_counts','problems']},indent=2))


if __name__=='__main__':
    main()
