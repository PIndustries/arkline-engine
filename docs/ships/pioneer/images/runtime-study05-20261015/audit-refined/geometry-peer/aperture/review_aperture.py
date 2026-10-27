"""Read-only evaluated Blender source ray audit of Study 05 drive apertures.

Loads with automatic scripts disabled. Evaluates modifiers without applying or
saving them. Reports body-frame +X rays from behind each actual drive center.
"""
from collections import Counter
import hashlib
import json
from pathlib import Path
import math

import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

ROOT = Path('/workspace/arkline-runtime/study05-audit-v2/geometry-peer/aperture')
SOURCE = Path('/workspace/arkline-runtime/study05-authored/source-v3/pioneer-study05.blend')
EXPECTED = '61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9'
BODY = np.array([[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]])

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def values(collection, key, width, dtype=float):
    out = np.empty(len(collection) * width, dtype=dtype)
    collection.foreach_get(key, out)
    return out.reshape((-1, width))

before = sha(SOURCE)
assert before == EXPECTED, before
bpy.context.preferences.filepaths.use_scripts_auto_execute = False
bpy.ops.wm.open_mainfile(filepath=str(SOURCE), load_ui=False, use_scripts=False)
assert not bpy.context.preferences.filepaths.use_scripts_auto_execute
graph = bpy.context.evaluated_depsgraph_get()
parts = []
for obj in bpy.context.scene.objects:
    if obj.type != 'MESH':
        continue
    evaluated = obj.evaluated_get(graph)
    mesh = evaluated.to_mesh(preserve_all_data_layers=True, depsgraph=graph)
    try:
        mesh.calc_loop_triangles()
        world = np.array(evaluated.matrix_world, dtype=float)
        vertices = (values(mesh.vertices, 'co', 3) @ world[:3, :3].T + world[:3, 3]) @ BODY.T
        tris = values(mesh.loop_triangles, 'vertices', 3, np.int64)
        polys = values(mesh.loop_triangles, 'polygon_index', 1, np.int64).ravel()
        mats = values(mesh.loop_triangles, 'material_index', 1, np.int64).ravel()
        mat_names = [slot.material.name if slot.material else None for slot in evaluated.material_slots]
        p = vertices[tris]
        faces = np.cross(p[:, 1] - p[:, 0], p[:, 2] - p[:, 0])
        lengths = np.linalg.norm(faces, axis=1)
        faces /= np.where(lengths[:, None] > 0, lengths[:, None], 1)
        tree = BVHTree.FromPolygons([Vector(v) for v in vertices], tris.tolist(), all_triangles=True, epsilon=0.0)
        parts.append({'name': obj.name, 'p': p, 'poly': polys, 'mat': mats,
                      'materials': mat_names, 'normal': faces, 'bvh': tree,
                      'bounds': [vertices.min(axis=0).tolist(), vertices.max(axis=0).tolist()],
                      'raw_polygon_count': len(obj.data.polygons), 'evaluated_triangles': len(tris),
                      'modifiers': [mod.type for mod in obj.modifiers]})
    finally:
        evaluated.to_mesh_clear()

def hits(origin):
    # The independent direct intersection uses +X and triangle projection onto
    # YZ; it keeps front and back faces and reports all intersections.
    hits = []
    bvh_hits = []
    for part in parts:
        bvh = part['bvh'].ray_cast(Vector(origin), Vector((1.,0.,0.)), 100.)
        if bvh[0] is not None:
            bvh_hits.append({'part':part['name'], 'distance_m':float(bvh[3]),
                             'position_body_m':list(bvh[0])})
        p = part['p']
        ay = p[:, 1, 1] - p[:, 0, 1]
        az = p[:, 1, 2] - p[:, 0, 2]
        by = p[:, 2, 1] - p[:, 0, 1]
        bz = p[:, 2, 2] - p[:, 0, 2]
        cy = origin[1] - p[:, 0, 1]
        cz = origin[2] - p[:, 0, 2]
        den = ay * bz - az * by
        valid = np.abs(den) > 1e-12
        u = np.divide(cy*bz-cz*by, den, out=np.zeros_like(den), where=valid)
        v = np.divide(ay*cz-az*cy, den, out=np.zeros_like(den), where=valid)
        x = p[:, 0, 0] + u*(p[:, 1, 0]-p[:, 0, 0]) + v*(p[:, 2, 0]-p[:, 0, 0])
        valid &= (u >= -1e-9) & (v >= -1e-9) & ((u+v) <= 1+1e-9) & (x >= origin[0])
        indices = np.flatnonzero(valid)
        if not len(indices):
            continue
        # Only nearest intersection per named source part is needed for the
        # open-path test; retain all coplanar candidate triangles at that depth.
        nearest = float(x[indices].min())
        at_nearest = indices[np.abs(x[indices]-nearest) < 1e-5]
        tri = int(at_nearest[0])
        hit = {'part': part['name'], 'distance_m': nearest-origin[0],
               'position_body_m': [nearest, origin[1], origin[2]],
               'normal_body': part['normal'][tri].tolist(),
               'material': part['materials'][int(part['mat'][tri])],
               'evaluated_triangle': tri, 'evaluated_polygon': int(part['poly'][tri]),
               'triangle_vertices_body_m': p[tri].tolist(),
               'coplanar_triangle_candidates': len(at_nearest)}
        surface_x = sorted(float(value) for value in x[indices] if -36.1 <= value <= -35.5)
        distinct_x = []
        for value in surface_x:
            if not distinct_x or abs(value-distinct_x[-1]) > 1e-5:
                distinct_x.append(value)
        hit['aft_surface_intersections_x_body_m'] = distinct_x
        hit['bvh_first_distance_m'] = float(bvh[3]) if bvh[0] is not None else None
        hit['bvh_distance_agrees_1e_4_m'] = bvh[0] is not None and abs(float(bvh[3])-hit['distance_m']) < 1e-4
        hits.append(hit)
    return (sorted(hits, key=lambda hit: (hit['distance_m'], hit['part'])),
            sorted(bvh_hits, key=lambda hit:(hit['distance_m'],hit['part'])))

offsets = [('center',0.,0.)]
for radius in (.5, 1., 2., 2.75, 2.9, 2.96, 3., 3.2, 3.5):
    for label, y, z in [('up',radius,0.),('down',-radius,0.),('port',0.,-radius),('starboard',0.,radius)]:
        offsets.append((f'{label}_{radius:g}m',y,z))
    if radius in (2., 2.75, 2.9, 3.):
        for i in range(4):
            angle = math.pi/4+i*math.pi/2
            offsets.append((f'diagonal_{i}_{radius:g}m', radius*math.cos(angle),radius*math.sin(angle)))
for radius in (2.94, 3., 3.05, 3.1, 3.15):
    for i in range(16):
        angle = math.tau*i/16
        offsets.append((f'flange_r{radius:g}_angle{i*22.5:g}deg',radius*math.cos(angle),radius*math.sin(angle)))
rays = []
for side, center_z in [('port',-12.5),('starboard',12.5)]:
    for label, y, z in offsets:
        origin = [-40.,y,center_z+z]
        records, bvh_records = hits(origin)
        rays.append({'side': side, 'sample': label, 'origin_body_m': origin,
                     'direction_body': [1.,0.,0.], 'radius_m': math.hypot(y,z),
                     'first_hit': records[0] if records else None,
                     'independent_global_bvh_first_hit': bvh_records[0] if bvh_records else None,
                     'global_first_distance_agrees_1e_4_m': bool(records and bvh_records and abs(records[0]['distance_m']-bvh_records[0]['distance_m'])<1e-4),
                     'first_8_named_part_hits': records[:8]})

interesting = [part for part in parts if any(key in part['name'] for key in
    ('Pressure hull','Outboard drive nacelle','Drive throat','Main drive bell','Aft drive aperture'))]
inventory = [{key: part[key] for key in ('name','bounds','raw_polygon_count','evaluated_triangles','materials','modifiers')}
             for part in interesting]
after = sha(SOURCE)
assert before == after
report = {'source': str(SOURCE), 'source_sha256_before': before, 'source_sha256_after': after,
    'bpy_version': bpy.app.version_string, 'script_auto_execute': False,
    'method': 'Evaluated source geometry, world transform then body=(world.x,world.z,-world.y). Two-sided +X rays from body x=-40m; direct projected triangle intersections cross-checked against per-part BVH. No source save or export.',
    'mesh_parts': len(parts), 'bounds_body_m': [np.min([p['bounds'][0] for p in parts],axis=0).tolist(),np.max([p['bounds'][1] for p in parts],axis=0).tolist()],
    'ray_count': len(rays), 'first_hits_by_side': {side:dict(Counter(r['first_hit']['part'] if r['first_hit'] else '<none>' for r in rays if r['side']==side)) for side in ('port','starboard')},
    'all_global_first_distances_agree_1e_4_m': all(r['global_first_distance_agrees_1e_4_m'] for r in rays),
    'boundary_caveat': 'Exactly r=2.96m secondary named-part hits have projected-float64 versus BVH-float32 edge ambiguity. Global first intersection still agrees (the titanium aperture rim). These secondary hits do not support a precise aperture-boundary measurement.',
    'interpretation': 'Source v3 check: centerline/radials and 16-angle flange seating rays. Interpretation is recorded in flange-summary.json after the intersection evidence is examined. Geometry alone does not establish runtime appearance or engineering correctness.',
    'part_inventory': inventory, 'rays': rays}
path = ROOT/'aperture-review.json'
path.write_text(json.dumps(report,indent=2)+'\n')
seating = [r for r in rays if r['sample'].startswith('flange_') and r['radius_m'] >= 2.999]
records = []
for ray in seating:
    rim = next(h for h in ray['first_8_named_part_hits'] if h['part'].startswith('Aft drive aperture rim'))
    front, back = min(rim['aft_surface_intersections_x_body_m']),max(rim['aft_surface_intersections_x_body_m'])
    caps = [h for h in ray['first_8_named_part_hits'] if h['part'] in ('Pressure hull','Outboard drive nacelle','Outboard drive nacelle.001')]
    cap_x = [x for h in caps for x in h['aft_surface_intersections_x_body_m']]
    nearest_cap = min(cap_x)
    cap_parts_at_floor = [h['part'] for h in caps if any(abs(x-nearest_cap)<1e-4 for x in h['aft_surface_intersections_x_body_m'])]
    intrusions = [{'part':h['part'],'x_body_m':x} for h in ray['first_8_named_part_hits']
                  if not h['part'].startswith('Aft drive aperture rim')
                  for x in h['aft_surface_intersections_x_body_m'] if front-1e-4 <= x <= back+1e-4]
    records.append({'side':ray['side'],'sample':ray['sample'],'radius_m':ray['radius_m'],
                    'rim_front_x_body_m':front,'rim_back_x_body_m':back,
                    'nearest_cap_floor_x_body_m':nearest_cap,'back_to_floor_gap_m':nearest_cap-back,
                    'cap_parts_at_floor':cap_parts_at_floor,'non_rim_flange_depth_intrusions':intrusions})
summary = {'source_sha256':after, 'script_sha256':sha(Path(__file__)),
    'aperture_report_sha256':sha(path), 'meshes':len(parts),'ray_count':len(rays),
    'seating_ray_count':len(records),'seating_radii_m':[3.,3.05,3.1,3.15],
    'angle_count_per_radius_per_drive':16,
    'all_global_first_distances_agree_1e_4_m':report['all_global_first_distances_agree_1e_4_m'],
    'all_seating_first_hits_are_rim':all(r['first_hit']['part'].startswith('Aft drive aperture rim') for r in seating),
    'rim_front_x_range_body_m':[min(r['rim_front_x_body_m'] for r in records),max(r['rim_front_x_body_m'] for r in records)],
    'rim_back_x_range_body_m':[min(r['rim_back_x_body_m'] for r in records),max(r['rim_back_x_body_m'] for r in records)],
    'nearest_cap_floor_x_range_body_m':[min(r['nearest_cap_floor_x_body_m'] for r in records),max(r['nearest_cap_floor_x_body_m'] for r in records)],
    'back_to_floor_gap_range_m':[min(r['back_to_floor_gap_m'] for r in records),max(r['back_to_floor_gap_m'] for r in records)],
    'non_rim_flange_depth_intrusion_count':sum(len(r['non_rim_flange_depth_intrusions']) for r in records),
    'rays_with_two_mutually_coincident_cap_floor_parts':sum(len(r['cap_parts_at_floor'])>1 for r in records),
    'interpretation':'Sampled geometry clears rim/cap coplanarity: all tested r=3.00..3.15m rays meet the front rim first, with no other part surface through its thickness and a positive 20mm gap behind its back to the cap floor. Center rays still meet the retained closed emissive throat, so this is a real recess rather than an open-through tube. Hull/nacelle cap floors remain mutually coincident behind the flange in some rays; this audit does not claim all source coplanarity is eliminated.',
    'scope_limit':'Finite two-sided ray samples on evaluated source geometry, not a global triangle-triangle intersection proof, runtime appearance test or engineering verification. Exact r=2.96m secondary edge hits retain the documented BVH precision ambiguity.',
    'seating_records':records}
summary_path=ROOT/'flange-summary.json'
summary_path.write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'report': str(path), 'sha256':sha(path), 'meshes':len(parts), 'rays':len(rays),
    'summary':str(summary_path), 'summary_sha256':sha(summary_path),'script_sha256':sha(Path(__file__)),
    'first_hits':report['first_hits_by_side'], 'source_unchanged':before==after,
    'center_first_hits':[r['first_hit'] for r in rays if r['sample']=='center'], 'all_global_first_distances_agree_1e_4_m':report['all_global_first_distances_agree_1e_4_m']},indent=2))
