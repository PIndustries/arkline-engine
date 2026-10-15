#!/usr/bin/env python3
"""Bounded, deterministic static Pioneer GLB contract; Python standard library only.

The renderer palette is identified by exact names, never by traversal or BIN order.
Geometry is already evaluated and transformed to body metres by export_pioneer.py.
"""
import hashlib
import json
import math
from pathlib import Path
import struct

SCHEMA = 'arkline.pioneer-saved-master-export/v1'
MATERIAL_NAMES = (
    'Graphite ceramic armor', 'Titanium machinery', 'Recessed carbon',
    'Emerald structural frames', 'Drive emission', 'Working lights',
    'Navigation glass', 'Safety amber', 'Hull identification ivory',
)
LIMITS = {'source_bytes': 128 * 1024 * 1024, 'glb_bytes': 16 * 1024 * 1024,
          'receipt_bytes': 16 * 1024 * 1024, 'objects': 4096, 'triangles': 200000,
          'evaluated_triangles': 400000, 'accessor_count': 600000,
          'wall_seconds': 180, 'cpu_seconds': 120, 'address_space_bytes': 8 * 1024**3}


class ExportError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ExportError(message)


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'),
                      allow_nan=False).encode('utf-8')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def f32(value):
    try:
        result = struct.unpack('<f', struct.pack('<f', float(value)))[0]
    except (OverflowError, struct.error, TypeError, ValueError) as exc:
        raise ExportError('non-finite or unrepresentable float32') from exc
    require(math.isfinite(result), 'non-finite float32')
    return result if result else 0.0


def vec3(value):
    require(isinstance(value, (list, tuple)) and len(value) == 3, 'expected XYZ triple')
    return tuple(f32(x) for x in value)


def unit(value):
    require(len(value) == 3 and all(math.isfinite(x) for x in value), 'invalid normal')
    length = math.sqrt(sum(x*x for x in value))
    require(length > 1e-12, 'zero normal')
    return vec3([x / length for x in value])


def affine_and_normal_matrix(matrix):
    require(len(matrix) == 4 and all(len(row) == 4 for row in matrix), 'expected affine 4x4')
    require(all(math.isfinite(x) and abs(x) <= 1e6 for row in matrix for x in row), 'unbounded transform')
    require(list(matrix[3]) == [0, 0, 0, 1], 'non-affine transform')
    a, b, c = matrix[0][:3]; d, e, f = matrix[1][:3]; g, h, i = matrix[2][:3]
    determinant = a*(e*i-f*h)-b*(d*i-f*g)+c*(d*h-e*g)
    require(math.isfinite(determinant) and determinant > 1e-10,
            'negative, singular or near-singular transform unsupported')
    inverse_transpose = [[(e*i-f*h), (f*g-d*i), (d*h-e*g)],
                         [(c*h-b*i), (a*i-c*g), (b*g-a*h)],
                         [(b*f-c*e), (c*d-a*f), (a*e-b*d)]]
    return [[x / determinant for x in row] for row in inverse_transpose]


def transform_point(matrix, point):
    p = [sum(matrix[i][j]*point[j] for j in range(3)) + matrix[i][3] for i in range(3)]
    return vec3((p[0], p[2], -p[1]))


def transform_normal(normal_matrix, normal, allow_zero=False):
    n = [sum(normal_matrix[i][j]*normal[j] for j in range(3)) for i in range(3)]
    if allow_zero and all(x == 0 for x in n):
        return (0., 0., 0.)
    return unit((n[0], n[2], -n[1]))


def runtime_cross_squared(positions):
    """Mirror identity-node ship_asset.rs f32 metre/100, subtract, cross, dot.

    Units of this predicate are renderer scene units to the fourth power.
    The actual Rust decoder remains the independent acceptance check.
    """
    p = [[f32(f32(v) / f32(100.0)) for v in point] for point in positions]
    a = [f32(p[1][i]-p[0][i]) for i in range(3)]
    b = [f32(p[2][i]-p[0][i]) for i in range(3)]
    cross = [f32(f32(a[1]*b[2])-f32(a[2]*b[1])),
             f32(f32(a[2]*b[0])-f32(a[0]*b[2])),
             f32(f32(a[0]*b[1])-f32(a[1]*b[0]))]
    return f32(f32(f32(cross[0]*cross[0])+f32(cross[1]*cross[1]))+f32(cross[2]*cross[2]))


def pack_glb(document, binary):
    text = canonical_json(document)
    text += b' ' * (-len(text) % 4)
    blob = binary + b'\0' * (-len(binary) % 4)
    result = (struct.pack('<IIIII', 0x46546c67, 2, 12+8+len(text)+8+len(blob), len(text), 0x4e4f534a)
              + text + struct.pack('<II', len(blob), 0x004e4942) + blob)
    require(len(result) <= LIMITS['glb_bytes'], 'GLB exceeds 16 MiB')
    return result


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def unpack_glb(data):
    require(isinstance(data, bytes) and 28 <= len(data) <= LIMITS['glb_bytes'], 'invalid GLB size')
    magic, version, size = struct.unpack_from('<III', data)
    require((magic, version, size) == (0x46546c67, 2, len(data)), 'invalid GLB header')
    chunks, offset = [], 12
    while offset < len(data):
        require(offset+8 <= len(data), 'truncated chunk')
        size, kind = struct.unpack_from('<II', data, offset)
        require(size % 4 == 0 and offset+8+size <= len(data), 'invalid chunk bounds')
        chunks.append((kind, data[offset+8:offset+8+size])); offset += 8+size
    require(len(chunks) == 2 and [c[0] for c in chunks] == [0x4e4f534a, 0x004e4942],
            'expected one JSON and one BIN chunk')
    try:
        doc = json.loads(chunks[0][1], object_pairs_hook=_unique_object,
                         parse_constant=lambda _: (_ for _ in ()).throw(ExportError('non-finite JSON')))
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise ExportError('invalid JSON document') from exc
    pending, count = [(doc, 0)], 0
    while pending:
        value, depth = pending.pop(); count += 1
        require(count <= 300000 and depth <= 32, 'JSON budget exceeded')
        if isinstance(value, dict): pending.extend((v, depth+1) for v in value.values())
        elif isinstance(value, list): pending.extend((v, depth+1) for v in value)
        elif isinstance(value, float): require(math.isfinite(value), 'non-finite JSON value')
    return doc, chunks[1][1]


def canonicalize_glb(data):
    """Canonical material identity/JSON only; BIN chunk bytes are preserved exactly."""
    doc, binary = unpack_glb(data)
    materials = doc.get('materials', [])
    require(len(materials) == len(MATERIAL_NAMES), 'expected nine material slots')
    names = [m.get('name') for m in materials]
    require(len(set(names)) == len(names) and set(names) == set(MATERIAL_NAMES), 'invalid material names')
    mapping = {old: MATERIAL_NAMES.index(name) for old, name in enumerate(names)}
    doc['materials'] = [materials[names.index(name)] for name in MATERIAL_NAMES]
    for mesh in doc.get('meshes', []):
        for primitive in mesh.get('primitives', []):
            old = primitive.get('material')
            require(type(old) is int and old in mapping, 'invalid material reference')
            primitive['material'] = mapping[old]
    result = pack_glb(doc, binary)
    validate_glb(result)
    require(unpack_glb(result)[1] == binary, 'canonicalization changed BIN')
    return result


def validate_glb(data):
    """Fail closed on the small baked subset emitted here, not a generic glTF reader."""
    doc, binary = unpack_glb(data)
    require(doc.get('asset', {}).get('version') == '2.0', 'invalid glTF version')
    require(set(doc) <= {'asset', 'extensionsUsed', 'scene', 'scenes', 'nodes', 'meshes',
                         'materials', 'buffers', 'bufferViews', 'accessors'}, 'unsupported glTF field')
    require(doc.get('extensionsUsed', []) in ([], ['KHR_materials_emissive_strength']), 'unsupported extension')
    require(doc.get('scene') == 0 and doc.get('scenes') == [{'nodes': [0]}], 'unsupported scene')
    require(doc.get('nodes') == [{'mesh': 0, 'name': 'Pioneer saved-master body'}], 'unsupported node transform/hierarchy')
    require(len(doc.get('meshes', [])) == 1, 'expected one mesh')
    buffers = doc.get('buffers', [])
    require(len(buffers) == 1 and set(buffers[0]) == {'byteLength'}, 'external/multiple buffer unsupported')
    length = buffers[0]['byteLength']
    require(type(length) is int and 0 < length <= len(binary) and len(binary)-length < 4
            and not any(binary[length:]), 'invalid embedded buffer length/padding')
    materials = doc.get('materials', [])
    require([m.get('name') for m in materials] == list(MATERIAL_NAMES), 'noncanonical material slots')
    for material in materials:
        require(set(material) <= {'name', 'alphaMode', 'doubleSided', 'pbrMetallicRoughness',
                                  'emissiveFactor', 'extensions'}, 'unsupported material field')
        require(material.get('alphaMode') == 'OPAQUE' and type(material.get('doubleSided')) is bool, 'nonopaque material')
        pbr = material.get('pbrMetallicRoughness', {})
        require(set(pbr) == {'baseColorFactor', 'metallicFactor', 'roughnessFactor'}, 'textured/invalid PBR')
        rgba = pbr['baseColorFactor']
        require(len(rgba) == 4 and rgba[3] == 1 and all(type(v) in (int, float) and 0 <= v <= 1 for v in rgba), 'invalid base color')
        require(all(type(pbr[k]) in (int, float) and 0 <= pbr[k] <= 1 for k in ('metallicFactor', 'roughnessFactor')), 'invalid scalar PBR')
        emissive = material.get('emissiveFactor')
        require(isinstance(emissive, list) and len(emissive) == 3 and all(type(v) in (int, float) and 0 <= v <= 1 for v in emissive), 'invalid emissive factor')
        ext = material.get('extensions', {})
        require(set(ext) <= {'KHR_materials_emissive_strength'}, 'unsupported material extension')
        if ext:
            strength = ext['KHR_materials_emissive_strength']
            require(set(strength) == {'emissiveStrength'} and type(strength['emissiveStrength']) in (int, float)
                    and 0 <= strength['emissiveStrength'] <= 64, 'invalid emissive strength')
    accessors, views = doc.get('accessors', []), doc.get('bufferViews', [])
    require(len(accessors) == 27 and len(views) == 27, 'expected three accessors per material')
    used_accessors, used_views = set(), set()
    def read(index, width, component, position=False):
        require(type(index) is int and 0 <= index < len(accessors), 'accessor reference out of bounds')
        require(index not in used_accessors, 'shared/unused accessor outside export subset')
        used_accessors.add(index)
        a = accessors[index]
        require(set(a) <= {'bufferView', 'componentType', 'count', 'type', 'min', 'max'}, 'unsupported accessor field')
        require(a.get('componentType') == component and a.get('type') == ('VEC3' if width == 3 else 'SCALAR'), 'invalid accessor format')
        count, vi = a.get('count'), a.get('bufferView')
        require(type(count) is int and 0 < count <= LIMITS['accessor_count'], 'accessor count budget exceeded')
        require(type(vi) is int and 0 <= vi < len(views), 'invalid buffer view reference')
        require(vi not in used_views, 'shared/unused buffer view outside export subset')
        used_views.add(vi)
        view = views[vi]
        require(set(view) == {'buffer', 'byteOffset', 'byteLength', 'target'} and view['buffer'] == 0, 'invalid view')
        offset, size = view['byteOffset'], view['byteLength']
        require(type(offset) is int and type(size) is int and offset >= 0 and offset % 4 == 0
                and size == count*width*4 and offset+size <= length, 'accessor bounds mismatch')
        require(view['target'] == (34963 if component == 5125 else 34962), 'invalid buffer target')
        values = list(struct.iter_unpack('<' + ('f' if component == 5126 else 'I')*width, binary[offset:offset+size]))
        if component == 5126: require(all(math.isfinite(x) for row in values for x in row), 'non-finite vertex')
        if position or 'min' in a or 'max' in a:
            require(component == 5126 and width == 3 and 'min' in a and 'max' in a,
                    'position bounds required; optional normal bounds must be paired')
            for label, operation in (('min', min), ('max', max)):
                bounds = a[label]
                require(isinstance(bounds, list) and len(bounds) == 3 and
                        all(type(v) in (int, float) and math.isfinite(v) for v in bounds), 'invalid accessor bounds')
                require(bounds == [operation(row[i] for row in values) for i in range(3)], 'accessor bounds differ from decoded values')
        return values
    primitives = doc['meshes'][0].get('primitives', [])
    require(len(primitives) == 9 and sorted(p.get('material', -1) for p in primitives) == list(range(9)), 'invalid primitive palette')
    triangles, bounds = 0, [[math.inf]*3, [-math.inf]*3]
    for primitive in primitives:
        require(set(primitive) == {'attributes', 'indices', 'material', 'mode'} and primitive['mode'] == 4, 'unsupported primitive')
        attrs = primitive['attributes']
        require(set(attrs) == {'POSITION', 'NORMAL'}, 'unsupported vertex attribute')
        positions, normals, indices = read(attrs['POSITION'], 3, 5126, position=True), read(attrs['NORMAL'], 3, 5126), read(primitive['indices'], 1, 5125)
        require(len(positions) == len(normals) and len(indices) % 3 == 0, 'corner count mismatch')
        require(all(index[0] < len(positions) for index in indices), 'index out of range')
        require(all(abs(math.sqrt(sum(x*x for x in n))-1.) <= 1e-5 for n in normals), 'nonunit normal')
        triangles += len(indices)//3
        for offset in range(0, len(indices), 3):
            require(runtime_cross_squared([positions[indices[offset+i][0]] for i in range(3)]) > f32(1e-20), 'runtime-degenerate triangle')
        for point in positions:
            for i, value in enumerate(point): bounds[0][i] = min(bounds[0][i], value); bounds[1][i] = max(bounds[1][i], value)
    require(0 < triangles <= LIMITS['triangles'], 'triangle budget exceeded')
    require(used_accessors == set(range(27)) and used_views == set(range(27)), 'unused accessor/view outside export subset')
    return {'triangles': triangles, 'primitives': 9, 'materials': 9, 'body_bounds_m': bounds,
            'bytes': len(data), 'sha256': sha256(data)}


def serialize(parts, materials, degenerate_policy):
    require(degenerate_policy in {'drop', 'reject'}, 'explicit degenerate policy required')
    require(0 < len(parts) <= LIMITS['objects'], 'source object budget exceeded')
    require(len(materials) == 9 and {m['name'] for m in materials} == set(MATERIAL_NAMES), 'expected exact nine materials')
    ordered_materials = [next(m for m in materials if m['name'] == name) for name in MATERIAL_NAMES]
    doc = {'asset': {'version': '2.0', 'generator': SCHEMA}, 'extensionsUsed': ['KHR_materials_emissive_strength'],
           'scene': 0, 'scenes': [{'nodes': [0]}], 'nodes': [{'mesh': 0, 'name': 'Pioneer saved-master body'}],
           'meshes': [{'primitives': []}], 'materials': ordered_materials, 'buffers': [], 'bufferViews': [], 'accessors': []}
    binary, mapping, dropped = bytearray(), [], []
    evaluated_count, retained_count = 0, 0
    def accessor(values, width, component, target):
        index = len(doc['accessors']); offset = len(binary)
        fmt = '<' + ('f' if component == 5126 else 'I')*width
        for value in values: binary.extend(struct.pack(fmt, *value))
        doc['bufferViews'].append({'buffer': 0, 'byteOffset': offset, 'byteLength': len(binary)-offset, 'target': target})
        item = {'bufferView': index, 'componentType': component, 'count': len(values), 'type': 'VEC3' if width == 3 else 'SCALAR'}
        if component == 5126 and target == 34962 and width == 3:
            item['min'] = [min(row[i] for row in values) for i in range(3)]
            item['max'] = [max(row[i] for row in values) for i in range(3)]
        doc['accessors'].append(item)
        return index
    require(len({p['name'] for p in parts}) == len(parts), 'duplicate source part name')
    for material_index, material_name in enumerate(MATERIAL_NAMES):
        positions, normals = [], []
        for part in sorted((p for p in parts if p['material'] == material_name), key=lambda p: p['name']):
            triangles, removed = [], []
            evaluated_count += len(part['triangles'])
            require(evaluated_count <= LIMITS['evaluated_triangles'], 'evaluated triangle budget exceeded')
            for original_index, triangle in enumerate(part['triangles']):
                require(len(triangle) == 3, 'invalid source triangle')
                source_positions = [vec3(corner[0]) for corner in triangle]
                # The gate is float32-order-sensitive at its threshold. Choose
                # the final cyclic order first, then gate exactly what is stored.
                rotation = min(range(3), key=lambda i: source_positions[i:]+source_positions[:i])
                source_positions = source_positions[rotation:]+source_positions[:rotation]
                triangle = list(triangle[rotation:])+list(triangle[:rotation])
                gate = runtime_cross_squared(source_positions)
                if gate <= f32(1e-20):
                    require(degenerate_policy == 'drop', f"{part['name']}: runtime-degenerate triangle {original_index}")
                    removed.append({'evaluated_triangle': original_index, 'render_cross_squared_f32': gate}); continue
                corners = tuple((position, unit(corner[1])) for position, corner in zip(source_positions, triangle))
                # Cyclic rotation and sorting preserve winding and each normal's corner.
                triangles.append(corners)
            triangles.sort()
            start = len(positions)
            for triangle in triangles:
                for position, normal in triangle: positions.append(position); normals.append(normal)
            retained_count += len(triangles)
            require(retained_count <= LIMITS['triangles'], 'triangle budget exceeded')
            mapping.append({key: part[key] for key in part if key not in {'triangles'}} | {
                'render_primitive': material_index, 'render_material_index': material_index,
                'first_index': start, 'index_count': len(triangles)*3,
                'evaluated_triangles': len(part['triangles']), 'retained_triangles': len(triangles),
                'dropped_triangles': len(removed)})
            if removed: dropped.append({'part': part['name'], 'triangles': removed})
        require(positions, f'canonical material has no retained geometry: {material_name}')
        p = accessor(positions, 3, 5126, 34962); n = accessor(normals, 3, 5126, 34962)
        indices = accessor([(i,) for i in range(len(positions))], 1, 5125, 34963)
        doc['meshes'][0]['primitives'].append({'attributes': {'POSITION': p, 'NORMAL': n},
                                            'indices': indices, 'material': material_index, 'mode': 4})
    require(all(p['material'] in MATERIAL_NAMES for p in parts), 'unknown source material')
    doc['buffers'] = [{'byteLength': len(binary)}]
    output = canonicalize_glb(pack_glb(doc, bytes(binary)))
    require(canonicalize_glb(output) == output, 'canonicalization is not idempotent')
    summary = validate_glb(output)
    return output, mapping, {'policy': degenerate_policy,
        'predicate': 'identity-node Rust ship_asset: body float32 /100, float32 subtract/cross/dot <= float32(1e-20) renderer-scene-unit^4',
        'evaluated_triangles': evaluated_count, 'retained_triangles': retained_count,
        'dropped_triangles': evaluated_count-retained_count, 'dropped_by_part': dropped}, summary
