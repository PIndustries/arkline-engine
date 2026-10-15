"""Read-only evaluated-master / frozen-GLB triangle-corner correspondence audit.

No operators other than opening the existing master with use_scripts=False.
No exporter, modifier application, model save, or source mutation is invoked.
"""
from collections import Counter, defaultdict
import hashlib
from itertools import permutations, product
import json
import math
from pathlib import Path
import struct
import subprocess
import time

import bpy
import numpy as np

ROOT = Path('/workspace/arkline-engine')
OUT = Path('/workspace/arkline-runtime/study05-audit/normals')
BLEND = Path('/workspace/arkline-runtime/study05-authored/source-v1/pioneer-study05.blend')
GLB = Path('/workspace/arkline-runtime/study05-authored/export-v1/pioneer.glb')
EXPECTED = {
    'blend': '9cc3ac5d1e96937d8de1a61411ae0d80efd96e2394d71a90f62a88f5257c11b4',
    'glb': 'df8490f42379aca161df0aa26b24cb32ab8dbf73e83e0681ed2cfc29ef43a2c9',
}
BODY = np.array([[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]])
PERMS = list(permutations(range(3)))
OFFSETS = list(product((-1, 0, 1), repeat=3))
ANALYSIS_CROSS_SQ = 1e-20

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def normalized(values):
    lengths = np.linalg.norm(values, axis=-1, keepdims=True)
    return values / np.where(lengths > 0., lengths, 1.)

def mesh_array(collection, property_name, width, dtype=float):
    array = np.empty(len(collection) * width, dtype=dtype)
    collection.foreach_get(property_name, array)
    return array.reshape((-1, width))

def geometry(positions, normals, materials, objects, polygons):
    p, n = np.concatenate(positions), np.concatenate(normals)
    cross = np.cross(p[:, 1] - p[:, 0], p[:, 2] - p[:, 0])
    cross_sq = np.sum(cross * cross, axis=1)
    return {'p': p, 'n': normalized(n), 'normal_lengths': np.linalg.norm(n, axis=2),
            'mat': materials, 'obj': objects, 'poly': np.array(polygons, dtype=np.int64),
            'cross_sq': cross_sq, 'face': normalized(cross), 'area': np.sqrt(cross_sq) * .5}

def read_master():
    bpy.context.preferences.filepaths.use_scripts_auto_execute = False
    bpy.ops.wm.open_mainfile(filepath=str(BLEND), load_ui=False, use_scripts=False)
    assert not bpy.context.preferences.filepaths.use_scripts_auto_execute
    graph = bpy.context.evaluated_depsgraph_get()
    positions, normals, materials, objects, polys = [], [], [], [], []
    polygon_records = []
    inventory = Counter()
    negative_determinants = []
    source_names = []
    for obj in bpy.context.scene.objects:
        if obj.type != 'MESH':
            continue
        source_names.append(obj.name)
        inventory.update(mod.type for mod in obj.modifiers)
        evaluated = obj.evaluated_get(graph)
        mesh = evaluated.to_mesh(preserve_all_data_layers=True, depsgraph=graph)
        try:
            mesh.calc_loop_triangles()
            world = np.array(evaluated.matrix_world, dtype=float)
            linear = world[:3, :3]
            if np.linalg.det(linear) < 0:
                negative_determinants.append(obj.name)
            vp = mesh_array(mesh.vertices, 'co', 3)
            wp = (vp @ linear.T + world[:3, 3]) @ BODY.T
            ln = mesh_array(mesh.corner_normals, 'vector', 3)
            wn = normalized(ln @ np.linalg.inv(linear) @ BODY.T)
            tv = mesh_array(mesh.loop_triangles, 'vertices', 3, np.int64)
            tl = mesh_array(mesh.loop_triangles, 'loops', 3, np.int64)
            tm = mesh_array(mesh.loop_triangles, 'material_index', 1, np.int64).ravel()
            tp = mesh_array(mesh.loop_triangles, 'polygon_index', 1, np.int64).ravel()
            names = [slot.material.name if slot.material else '<none>' for slot in evaluated.material_slots]
            base = len(polygon_records)
            for poly in mesh.polygons:
                loop_ids = list(poly.loop_indices)
                polygon_records.append({'obj': obj.name, 'local_polygon': poly.index,
                    'mat': names[poly.material_index], 'p': wp[list(poly.vertices)],
                    'n': wn[loop_ids], 'smooth': poly.use_smooth})
            positions.append(wp[tv]); normals.append(wn[tl])
            materials.extend(names[index] for index in tm)
            objects.extend([obj.name] * len(tv)); polys.extend(base + tp)
        finally:
            evaluated.to_mesh_clear()
    result = geometry(positions, normals, materials, objects, polys)
    result.update({'polygons': polygon_records, 'object_names': source_names,
                   'modifiers': dict(inventory), 'negative_determinants': negative_determinants})
    return result

def read_glb():
    content = GLB.read_bytes()
    magic, version, total = struct.unpack_from('<III', content)
    assert (magic, version, total) == (0x46546c67, 2, len(content))
    chunks, offset = {}, 12
    while offset < len(content):
        size, kind = struct.unpack_from('<II', content, offset)
        chunks[kind] = content[offset + 8:offset + 8 + size]
        offset += 8 + size
    doc = json.loads(chunks[0x4e4f534a])
    binary = chunks[0x004e4942]
    dtypes = {5120: 'i1', 5121: 'u1', 5122: '<i2', 5123: '<u2', 5125: '<u4', 5126: '<f4'}
    widths = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}
    def accessor(index):
        acc = doc['accessors'][index]
        assert 'sparse' not in acc and not acc.get('normalized', False)
        view = doc['bufferViews'][acc['bufferView']]
        assert view.get('buffer', 0) == 0
        dtype, width = np.dtype(dtypes[acc['componentType']]), widths[acc['type']]
        stride = view.get('byteStride', dtype.itemsize * width)
        start = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
        return np.ndarray((acc['count'], width), dtype, buffer=binary, offset=start,
                          strides=(stride, dtype.itemsize)).copy()
    def local(node):
        if 'matrix' in node:
            return np.array(node['matrix'], dtype=float).reshape(4, 4).T
        x, y, z, w = node.get('rotation', (0., 0., 0., 1.))
        rot = np.array([[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                        [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                        [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]])
        value = np.eye(4)
        value[:3, :3] = rot @ np.diag(node.get('scale', (1., 1., 1.)))
        value[:3, 3] = node.get('translation', (0., 0., 0.))
        return value
    positions, normals, materials, objects, polys, visits = [], [], [], [], [], []
    def visit(index, parent, ancestry):
        assert index not in ancestry
        node = doc['nodes'][index]
        world = parent @ local(node)
        if 'mesh' in node:
            for pi, primitive in enumerate(doc['meshes'][node['mesh']]['primitives']):
                assert primitive.get('mode', 4) == 4
                attrs = primitive['attributes']
                p = accessor(attrs['POSITION']).astype(float)
                n = accessor(attrs['NORMAL']).astype(float)
                indices = accessor(primitive['indices']).ravel() if 'indices' in primitive else np.arange(len(p))
                assert len(indices) % 3 == 0
                triangles = indices.reshape(-1, 3)
                wp = p @ world[:3, :3].T + world[:3, 3]
                wn = normalized(n @ np.linalg.inv(world[:3, :3]))
                name = doc['materials'][primitive['material']].get('name', '<unnamed>')
                positions.append(wp[triangles]); normals.append(wn[triangles])
                materials.extend([name] * len(triangles))
                objects.extend([node.get('name', f'node-{index}')] * len(triangles))
                polys.extend([-1] * len(triangles))
                visits.append({'node': index, 'mesh': node['mesh'], 'primitive': pi,
                               'material': name, 'triangles': len(triangles),
                               'world_matrix_row_major': world.tolist(),
                               'normal_matrix_inverse_transpose': np.linalg.inv(world[:3, :3]).T.tolist()})
        for child in node.get('children', []):
            visit(child, world, (*ancestry, index))
    for index in doc['scenes'][doc.get('scene', 0)]['nodes']:
        visit(index, np.eye(4), ())
    result = geometry(positions, normals, materials, objects, polys)
    result['nodes'] = visits
    return result

def basic(data):
    return {'triangles': len(data['p']), 'corners': len(data['p']) * 3,
        'finite_positions': bool(np.isfinite(data['p']).all()),
        'finite_normals': bool(np.isfinite(data['n']).all()),
        'bounds_min_body_m': data['p'].min(axis=(0, 1)).tolist(),
        'bounds_max_body_m': data['p'].max(axis=(0, 1)).tolist(),
        'analysis_near_zero_cross_squared_le_1e_20_body_m4': int(np.sum(data['cross_sq'] <= ANALYSIS_CROSS_SQ)),
        'positive_area_below_1e_6_m2': int(np.sum((data['area'] > 0.) & (data['area'] < 1e-6))),
        'triangles_by_material': dict(Counter(data['mat'])),
        'maximum_post_transform_normalized_corner_normal_length_error': float(np.max(np.abs(data['normal_lengths'] - 1.)))}

def parity(perm):
    return sum(perm[i] > perm[j] for i in range(3) for j in range(i+1, 3)) % 2

def stats(values):
    if not len(values):
        return {'count': 0}
    return {'count': len(values), 'min': float(np.min(values)), 'max': float(np.max(values)),
            'p50': float(np.percentile(values, 50)), 'p95': float(np.percentile(values, 95)),
            'p99': float(np.percentile(values, 99))}

def compare_normals(source, glb, edges):
    dots, errors, face_dots, by_mat, by_obj, examples = [], [], [], defaultdict(Counter), defaultdict(Counter), []
    for si, gi, perm, error in edges:
        ds = np.clip(np.sum(source['n'][si] * glb['n'][gi][list(perm)], axis=1), -1., 1.)
        dots.extend(ds); errors.append(error)
        face_dots.append(float(np.dot(source['face'][si], glb['face'][gi])))
        for name, threshold in [('dot_lt_0', 0.), ('dot_lt_05', .5), ('dot_lt_095', .95), ('dot_lt_0999', .999), ('dot_lt_099999', .99999)]:
            count = int(np.sum(ds < threshold))
            by_mat[source['mat'][si]][name] += count
            by_obj[source['obj'][si]][name] += count
        if float(np.min(ds)) < .95 and len(examples) < 20:
            examples.append({'source_triangle': si, 'glb_triangle': gi, 'source_object': source['obj'][si],
                'source_polygon': int(source['poly'][si]), 'material': source['mat'][si],
                'source_positions_body_m': source['p'][si].tolist(),
                'glb_positions_body_m_in_source_corner_order': glb['p'][gi][list(perm)].tolist(),
                'source_corner_normals_body': source['n'][si].tolist(),
                'glb_corner_normals_body_in_source_corner_order': glb['n'][gi][list(perm)].tolist(),
                'corner_normal_dots': ds.tolist(), 'max_corner_distance_m': error,
                'geometric_winding_dot': face_dots[-1]})
    dots = np.array(dots)
    return {'triangles': len(edges), 'corners': len(dots), 'normal_dot_statistics': stats(dots),
            'normal_angle_degrees_statistics': stats(np.degrees(np.arccos(dots))),
            'max_corner_distance_m_statistics': stats(errors), 'geometric_face_dot_statistics': stats(face_dots),
            'counts': {name: int(np.sum(dots < threshold)) for name, threshold in
                       [('dot_lt_0', 0.), ('dot_lt_05', .5), ('dot_lt_095', .95), ('dot_lt_0999', .999), ('dot_lt_099999', .99999)]},
            'counts_by_material': {k: dict(v) for k, v in by_mat.items()},
            'top_objects_by_dot_lt_0999': sorted([{'object': k, **v} for k, v in by_obj.items()],
                                               key=lambda x: x.get('dot_lt_0999', 0), reverse=True)[:30],
            'mismatch_examples': examples}

def correspondence(source, glb, tolerance):
    began = time.time()
    source_points, inverse = np.unique(source['p'].reshape(-1, 3), axis=0, return_inverse=True)
    spids = inverse.reshape(-1, 3)
    glb_points, ginverse = np.unique(glb['p'].reshape(-1, 3), axis=0, return_inverse=True)
    gpids = ginverse.reshape(-1, 3)
    grid = defaultdict(list)
    for i, cell in enumerate(np.floor(source_points / tolerance).astype(np.int64)):
        grid[tuple(cell)].append(i)
    point_candidates = []
    multiple, no_point, nearest_distances = 0, 0, []
    for point in glb_points:
        cell = np.floor(point / tolerance).astype(np.int64)
        nearby = []
        for delta in OFFSETS:
            nearby.extend(grid.get(tuple(cell + delta), ()))
        distances = [(float(np.linalg.norm(point - source_points[i])), i) for i in nearby]
        candidates = sorted((d, i) for d, i in distances if d <= tolerance)
        point_candidates.append([i for d, i in candidates])
        no_point += not candidates; multiple += len(candidates) > 1
        if candidates:
            nearest_distances.append(candidates[0][0])
    source_valid = np.flatnonzero(source['cross_sq'] > ANALYSIS_CROSS_SQ).tolist()
    glb_valid = np.flatnonzero(glb['cross_sq'] > ANALYSIS_CROSS_SQ).tolist()
    sorted_keys, adjacency = defaultdict(list), defaultdict(set)
    for si in source_valid:
        mat = source['mat'][si]
        sorted_keys[(mat, tuple(sorted(spids[si])))].append(si)
        for pid in spids[si]:
            adjacency[(mat, int(pid))].add(si)
    edges, corner_ambiguous, nearest_key_misses_recovered, no_edges = [], 0, 0, []
    same_g, same_s, reverse_g, reverse_s = defaultdict(list), defaultdict(list), defaultdict(list), defaultdict(list)
    for gi in glb_valid:
        mat = glb['mat'][gi]
        candidates_by_corner = [point_candidates[pid] for pid in gpids[gi]]
        if any(not candidates for candidates in candidates_by_corner):
            no_edges.append(gi); continue
        nearest_key = (mat, tuple(sorted(candidates[0] for candidates in candidates_by_corner)))
        candidate_sets = [set().union(*(adjacency.get((mat, pid), set()) for pid in candidates))
                          for candidates in candidates_by_corner]
        candidate_triangles = set.intersection(*candidate_sets)
        found = False
        for si in sorted(candidate_triangles):
            distances = np.linalg.norm(source['p'][si][:, None, :] - glb['p'][gi][None, :, :], axis=2)
            possible = [perm for perm in PERMS if all(distances[i, perm[i]] <= tolerance for i in range(3))]
            if len(possible) > 1:
                corner_ambiguous += 1; continue
            if not possible:
                continue
            perm = possible[0]
            edge = (si, gi, perm, float(max(distances[i, perm[i]] for i in range(3))))
            edges.append(edge); found = True
            if parity(perm):
                reverse_g[gi].append(edge); reverse_s[si].append(edge)
            else:
                same_g[gi].append(edge); same_s[si].append(edge)
        if found and not sorted_keys.get(nearest_key):
            nearest_key_misses_recovered += 1
        if not found:
            no_edges.append(gi)
    unique_same = [es[0] for gi, es in same_g.items() if len(es) == 1 and len(same_s[es[0][0]]) == 1]
    all_g = Counter(edge[1] for edge in edges)
    all_s = Counter(edge[0] for edge in edges)
    strict_same = [edge for edge in unique_same if all_g[edge[1]] == 1 and all_s[edge[0]] == 1]
    mixed_g = set(same_g) & set(reverse_g)
    mixed_s = set(same_s) & set(reverse_s)
    matched_s, matched_g = {e[0] for e in unique_same}, {e[1] for e in unique_same}
    unique_reverse = [es[0] for gi, es in reverse_g.items() if len(es) == 1 and len(reverse_s[es[0][0]]) == 1
                      and not same_g.get(gi) and not same_s.get(es[0][0])]
    matched_s.update(e[0] for e in unique_reverse); matched_g.update(e[1] for e in unique_reverse)
    ambiguous_g = {gi for _, gi, _, _ in edges if gi not in matched_g}
    ambiguous_s = {si for si, _, _, _ in edges if si not in matched_s}
    s_no_edges = set(source_valid) - {edge[0] for edge in edges}
    g_no_edges = set(no_edges)
    # Map unmatched GLB corners to source polygons independently of tessellation.
    source_point_lookup = {tuple(point): i for i, point in enumerate(source_points)}
    polygon_points, polygon_adjacency = {}, defaultdict(set)
    source_triangles_by_polygon = defaultdict(list)
    for si in source_valid:
        source_triangles_by_polygon[int(source['poly'][si])].append(si)
    for pi, polygon in enumerate(source['polygons']):
        if len(polygon['p']) <= 3:
            continue
        ids = [source_point_lookup.get(tuple(p)) for p in polygon['p']]
        if None in ids or len(set(ids)) != len(ids):
            continue
        polygon_points[pi] = ids
        for pid in ids:
            polygon_adjacency[(polygon['mat'], pid)].add(pi)
    alternate_assignments, alternate_ambiguous = {}, []
    for gi in sorted(g_no_edges):
        cs = [point_candidates[pid] for pid in gpids[gi]]
        if any(not values for values in cs):
            continue
        sets = [set().union(*(polygon_adjacency.get((glb['mat'][gi], pid), set()) for pid in values)) for values in cs]
        candidates = set.intersection(*sets)
        valid = []
        for pi in candidates:
            ids = polygon_points[pi]
            distances = np.linalg.norm(glb['p'][gi][:, None, :] - source['polygons'][pi]['p'][None, :, :], axis=2)
            choices = [np.flatnonzero(row <= tolerance).tolist() for row in distances]
            if all(len(choice) == 1 for choice in choices) and len({choice[0] for choice in choices}) == 3:
                valid.append((pi, [ids[choice[0]] for choice in choices]))
        if len(valid) == 1:
            alternate_assignments[gi] = valid[0]
        elif valid:
            alternate_ambiguous.append(gi)
    affected_polygons = {assignment[0] for assignment in alternate_assignments.values()}
    alt_groups = defaultdict(list)
    for si, gi, perm, _ in unique_same:
        pi = int(source['poly'][si])
        if pi in affected_polygons:
            mapped = [None] * 3
            for i, j in enumerate(perm): mapped[j] = int(spids[si][i])
            alt_groups[pi].append((gi, mapped, False))
    for gi, (pi, ids) in alternate_assignments.items():
        alt_groups[pi].append((gi, ids, True))
    alt_records, confirmed_source, confirmed_glb = [], set(), set()
    for pi, entries in alt_groups.items():
        polygon, ids = source['polygons'][pi], polygon_points[pi]
        boundary = Counter(tuple(sorted((ids[i], ids[(i+1) % len(ids)]))) for i in range(len(ids)))
        edges_count = Counter(tuple(sorted((corners[i], corners[(i+1) % 3])))
                              for _, corners, _ in entries for i in range(3))
        observed_boundary = Counter({edge: count for edge, count in edges_count.items() if count == 1})
        all_edges_manifold = all(count in (1, 2) for count in edges_count.values())
        source_ids = source_triangles_by_polygon[pi]
        source_area = float(np.sum(source['area'][source_ids]))
        glb_area = float(sum(glb['area'][gi] for gi, _, _ in entries))
        perimeter = float(np.sum(np.linalg.norm(np.roll(polygon['p'], -1, axis=0) - polygon['p'], axis=1)))
        reference_triangle_perimeters = 0.
        reference_triangulation_area = 0.
        actual_points_by_id = defaultdict(list)
        directed_edges = Counter()
        for gi, corners, _ in entries:
            reference = source_points[corners]
            reference_triangle_perimeters += float(np.sum(np.linalg.norm(np.roll(reference, -1, axis=0)-reference, axis=1)))
            reference_triangulation_area += float(np.linalg.norm(np.cross(reference[1]-reference[0], reference[2]-reference[0])) * .5)
            for i, pid in enumerate(corners):
                actual_points_by_id[pid].append(glb['p'][gi][i])
                directed_edges[(pid, corners[(i+1) % 3])] += 1
        maximum_corner_copy_spread = 0.
        for copies in actual_points_by_id.values():
            copies = np.array(copies)
            maximum_corner_copy_spread = max(maximum_corner_copy_spread,
                float(np.max(np.linalg.norm(copies[:, None, :] - copies[None, :, :], axis=2))))
        directed_boundary = Counter({edge: count-directed_edges[edge[::-1]] for edge, count in directed_edges.items()
                                    if count > directed_edges[edge[::-1]]})
        expected_directed_boundary = Counter((ids[i], ids[(i+1) % len(ids)]) for i in range(len(ids)))
        # For one triangle whose corners move <=epsilon, its area changes by at
        # most .5*epsilon*referencePerimeter + 1.5*epsilon^2. Summing gives a
        # conservative 3D bound even if separate GLB corner copies disagree.
        area_error_bound = max(1e-10, .5 * tolerance * reference_triangle_perimeters + 1.5 * len(entries) * tolerance**2)
        face = normalized(np.sum(np.cross(polygon['p'], np.roll(polygon['p'], -1, axis=0)), axis=0))
        plane_error = float(np.max(np.abs((polygon['p'] - polygon['p'][0]) @ face)))
        winding_dots = [float(np.dot(glb['face'][gi], face)) for gi, _, _ in entries]
        confirmed = (len(entries) == len(ids)-2 and observed_boundary == boundary and all_edges_manifold
                     and directed_boundary == expected_directed_boundary and maximum_corner_copy_spread <= 1e-12
                     and plane_error <= tolerance and min(winding_dots, default=-1) > .999
                     and abs(source_area-glb_area) <= area_error_bound)
        record = {'source_polygon': pi, 'object': polygon['obj'], 'material': polygon['mat'],
                  'source_polygon_vertices': len(ids), 'source_triangles': len(source_ids),
                  'assigned_glb_triangles': len(entries), 'glb_triangles_without_exact_triangle_correspondence': sum(e[2] for e in entries),
                  'boundary_edges_match': observed_boundary == boundary, 'edge_multiplicity_1_or_2': all_edges_manifold,
                  'source_polygon_plane_error_m': plane_error, 'source_triangle_area_m2': source_area,
                  'glb_triangle_area_m2': glb_area, 'min_winding_dot': min(winding_dots, default=None),
                  'source_polygon_perimeter_m': perimeter, 'area_error_bound_m2': area_error_bound,
                  'sum_reference_triangle_perimeters_m': reference_triangle_perimeters,
                  'reference_alternate_triangulation_area_m2': reference_triangulation_area,
                  'reference_alternate_vs_source_triangulation_area_difference_m2': abs(reference_triangulation_area-source_area),
                  'directed_polygon_boundary_match': directed_boundary == expected_directed_boundary,
                  'maximum_same_vertex_glb_corner_copy_spread_m': maximum_corner_copy_spread,
                  'confirmed_coplanar_alternate_triangulation': confirmed}
        alt_records.append(record)
        if confirmed:
            confirmed_source.update(si for si in source_ids if si in s_no_edges)
            confirmed_glb.update(gi for gi, _, alternate in entries if alternate)
    alt_failure_polygons, alt_failure_triangles = Counter(), Counter()
    for record in alt_records:
        if record['confirmed_coplanar_alternate_triangulation']:
            continue
        reasons = []
        if record['assigned_glb_triangles'] != record['source_polygon_vertices'] - 2: reasons.append('triangle_count')
        if not record['boundary_edges_match']: reasons.append('polygon_boundary')
        if not record['edge_multiplicity_1_or_2']: reasons.append('edge_multiplicity')
        if not record['directed_polygon_boundary_match']: reasons.append('directed_boundary')
        if record['maximum_same_vertex_glb_corner_copy_spread_m'] > 1e-12: reasons.append('incoherent_glb_corner_copies')
        if record['source_polygon_plane_error_m'] > tolerance: reasons.append('nonplanar_source_polygon')
        if record['min_winding_dot'] is None or record['min_winding_dot'] <= .999: reasons.append('winding_or_nonplanar_face_angle')
        if abs(record['source_triangle_area_m2'] - record['glb_triangle_area_m2']) > record['area_error_bound_m2']: reasons.append('area_error_bound')
        label = '+'.join(reasons)
        alt_failure_polygons[label] += 1
        alt_failure_triangles[label] += record['glb_triangles_without_exact_triangle_correspondence']
    def duplicate_summary(data, point_ids, valid):
        groups = defaultdict(list)
        for ti in valid:
            ids = point_ids[ti].tolist()
            groups[(data['mat'][ti], tuple(sorted(ids)))].append(ids)
        duplicate = [group for group in groups.values() if len(group) > 1]
        return {'exact_material_position_duplicate_groups': len(duplicate),
                'triangles_in_exact_material_position_duplicate_groups': sum(map(len, duplicate)),
                'groups_containing_both_windings': sum(len({parity([sorted(ids).index(x) for x in ids]) for ids in group}) > 1 for group in duplicate),
                'note': 'Exact transformed float-coordinate keys only; tolerance candidate ambiguity is reported independently.'}
    report = {'tolerance_m': tolerance, 'seconds': time.time()-began,
        'source_exact_position_points': len(source_points), 'glb_exact_position_points': len(glb_points),
        'glb_unique_points_no_source_point_within_tolerance': no_point,
        'glb_unique_points_multiple_source_exact_point_candidates': multiple,
        'nearest_source_point_distance_m_statistics': stats(nearest_distances),
        'nearest_sorted_corner_key_misses_recovered_by_neighbor_candidate_triangle_keys': nearest_key_misses_recovered,
        'material_position_triangle_candidate_edges': len(edges),
        'candidate_pairs_with_ambiguous_corner_permutation_excluded': corner_ambiguous,
        'globally_mutual_unique_same_winding': compare_normals(source, glb, strict_same),
        'unique_same_winding': compare_normals(source, glb, unique_same),
        'unique_same_winding_scope': 'Mutual uniqueness within same-winding candidates; opposite-winding alternatives remain separately counted. Globally mutual unique subset is stricter and excludes them.',
        'unique_opposite_winding': compare_normals(source, glb, unique_reverse),
        'source_triangles_with_both_winding_candidate_types': len(mixed_s),
        'glb_triangles_with_both_winding_candidate_types': len(mixed_g),
        'same_winding_source_triangles_with_multiple_glb_candidates': sum(len(v) > 1 for v in same_s.values()),
        'same_winding_glb_triangles_with_multiple_source_candidates': sum(len(v) > 1 for v in same_g.values()),
        'source_exact_duplicate_triangle_summary': duplicate_summary(source, spids, source_valid),
        'glb_exact_duplicate_triangle_summary': duplicate_summary(glb, gpids, glb_valid),
        'ambiguous_duplicate_or_nearcoincident_source_triangles': len(ambiguous_s),
        'ambiguous_duplicate_or_nearcoincident_glb_triangles': len(ambiguous_g),
        'unmatched_source_triangles_no_material_position_candidate': len(s_no_edges),
        'unmatched_glb_triangles_no_material_position_candidate': len(g_no_edges),
        'confirmed_alternate_triangulation_source_triangles_from_unmatched': len(confirmed_source),
        'confirmed_alternate_triangulation_glb_triangles_from_unmatched': len(confirmed_glb),
        'confirmed_alternate_triangulation_polygons': sum(r['confirmed_coplanar_alternate_triangulation'] for r in alt_records),
        'possible_alternate_triangulation_glb_triangles_unique_polygon_assignment': len(alternate_assignments),
        'possible_alternate_triangulation_glb_triangles_ambiguous_polygon_assignment': len(alternate_ambiguous),
        'alternate_triangulation_candidates': alt_records,
        'unconfirmed_alternate_triangulation_polygon_reason_combinations': dict(alt_failure_polygons),
        'unconfirmed_alternate_triangulation_unmatched_glb_triangle_reason_combinations': dict(alt_failure_triangles),
        'remaining_unmatched_source_triangles': len(s_no_edges-confirmed_source),
        'remaining_unmatched_glb_triangles': len(g_no_edges-confirmed_glb),
        'unmatched_source_by_material': dict(Counter(source['mat'][si] for si in s_no_edges)),
        'unmatched_glb_by_material': dict(Counter(glb['mat'][gi] for gi in g_no_edges)),
        'remaining_unmatched_glb_examples': [{'triangle': gi, 'material': glb['mat'][gi],
          'positions_body_m': glb['p'][gi].tolist(), 'area_m2': float(glb['area'][gi])}
          for gi in sorted(g_no_edges-confirmed_glb)[:15]]}
    assert len(unique_same)+len(unique_reverse)+len(ambiguous_g)+len(g_no_edges) == len(glb_valid)
    assert len(unique_same)+len(unique_reverse)+len(ambiguous_s)+len(s_no_edges) == len(source_valid)
    return report

def main():
    before = {'blend': digest(BLEND), 'glb': digest(GLB)}
    assert before == EXPECTED
    assert bpy.app.version == (5, 0, 1)
    print('Reading evaluated master with auto-execution disabled', flush=True)
    source = read_master()
    print('Reading frozen GLB with scene-node transforms', flush=True)
    glb = read_glb()
    print('Source/GLB triangles', len(source['p']), len(glb['p']), flush=True)
    reports = []
    for tolerance in (1e-5, 2e-5, 1e-4):
        print('Correspondence tolerance', tolerance, flush=True)
        value = correspondence(source, glb, tolerance)
        reports.append(value)
        print(json.dumps({k:value[k] for k in ('tolerance_m','seconds','ambiguous_duplicate_or_nearcoincident_glb_triangles',
             'unmatched_glb_triangles_no_material_position_candidate','confirmed_alternate_triangulation_polygons')}
             | {'unique_same_winding_triangles': value['unique_same_winding']['triangles'],
                'normal_dot_lt_0999': value['unique_same_winding']['counts']['dot_lt_0999']}), flush=True)
    after = {'blend': digest(BLEND), 'glb': digest(GLB)}
    result = {'date_utc': '2026-10-15',
        'source_head': subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip(),
        'blender_version': bpy.app.version_string, 'blender_build_hash': bpy.app.build_hash.decode(),
        'diagnostic_script_sha256': digest(Path(__file__)),
        'model_paths': {'blend': str(BLEND), 'glb': str(GLB)}, 'hashes_before': before, 'hashes_after': after,
        'models_unchanged': before == after, 'autoexecution_disabled': not bpy.context.preferences.filepaths.use_scripts_auto_execute,
        'method': {'source': 'Evaluated depsgraph mesh; calc_loop_triangles, evaluated corner_normals; no modifier_apply/export/save.',
            'coordinates': 'Source positions: evaluated matrix_world then Blender-to-body (x,z,-y). Source normals: inverse-transpose matrix_world, then same proper rotation, normalize. GLB positions: scene graph world transforms; GLB normals: inverse-transpose node world, normalize. GLB Y-up is body frame.',
            'correspondence': 'Source exact position corner IDs form sorted triangle keys with exact material names. Every GLB point searches all 27 neighboring tolerance grid cells and retains all source points within Euclidean tolerance. Candidate source triangles intersect the three corner adjacency sets; every candidate tests all six corner permutations with actual Euclidean distances. No normal-based selection. Globally mutual unique same-winding pairs require exactly one candidate edge for each triangle across both windings. Additional winding-prioritized same-winding pairs require uniqueness only in that winding; mixed-orientation alternatives are explicitly counted. Unique mutual opposite-winding pairs are accepted only if neither side has any same-winding candidate. Duplicates/multiple valid corner assignments stay separate.',
            'tolerance': '10 micrometres baseline, 20 and 100 micrometres sensitivity. Float32 ULP near 36m is 3.814697265625 micrometres; tolerance exceeds normal single-position storage error. Spatial buckets are candidate acceleration only, not distance acceptance.',
            'degenerate': 'Analysis-only near-zero cross-product squared <=1e-20 body-m4 excluded from correspondence, reported per file; positive triangle area <1e-6 body-m2 separately counted without assuming exporter deletion. This is NOT the engine runtime gate: importer scene units divide body positions by100, and its float32 cross-squared<=1e-20 scene-unit4 would be1e-12 body-m4 in exact arithmetic. No runtime gate reimplementation or acceptance claim is made here.',
            'alternate_triangulation': 'Unmatched GLB triangles may map uniquely to source polygon corner positions/material. Confirmed at tolerance only for n-2 triangles with matching directed and undirected polygon boundary, interior edges paired, copies of each source vertex agreeing on actual GLB positions within1e-12m, coplanarity within position tolerance, same winding, and absolute area difference <= max(1e-10m2, sum(0.5*tolerance*referenceTrianglePerimeter+1.5*tolerance^2)). The sum is a conservative 3D per-triangle area perturbation bound. Nonplanar, reversed, incoherent and other failed candidates remain unresolved. Alternate-group corner normals are not included in exact-triangle normal statistics.',
            'normal_thresholds': 'dot<0.999 is >2.5626 degrees; dot<0.95 is >18.1949 degrees. These are measurement bins, not an artist acceptance standard.'},
        'source': basic(source) | {'mesh_objects': len(source['object_names']), 'modifiers': source['modifiers'],
                                  'negative_world_determinant_objects': source['negative_determinants']},
        'glb': basic(glb) | {'decoded_node_primitives': glb['nodes']}, 'correspondence': reports,
        'limits': ['No claim of complete topology equality: duplicates, degenerate geometry and unmatched/nonplanar alternate tessellations are explicit.',
                   'This audits evaluated master corner normals against frozen export attributes, not offline rendering, actual engine output, canonical hull adoption, or the unpublished Mac exporter derivative.',
                   'No model mutation, regeneration, export, save, source edit, build, push, or Mac access occurred.']}
    assert before == after
    (OUT / 'report.json').write_text(json.dumps(result, indent=2) + '\n')
    print('Wrote', OUT / 'report.json', 'models unchanged:', before == after, flush=True)

if __name__ == '__main__':
    main()
