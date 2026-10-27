"""Explicit source-only 1703 fixture compiler scaffold; never normal admission.

Run only after the exact bank export and its proof gates are reviewed. All four
input/output paths are mandatory; outputs must be new. No legacy source/default
fixture is overwritten. The adapter preserves Stage/Launch orientation semantics.
"""
import argparse
import hashlib
import itertools
import json
import math
import struct
from pathlib import Path

import numpy as np
import parallel_bank_routes as routes

REVISION = 1703
HOST_SHA = '7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def pose(position, quaternion=None):
    return {'position_m': list(position),
            'orientation_xyzw': quaternion or [0., 0., 0., 1.]}


def qside(side):
    return [0., (1 if side == 'port' else -1)*math.sqrt(.5), 0., math.sqrt(.5)]


def dock(value):
    kind, number = value
    return {'kind': {'protected': 'protected', 'ready': 'ready', 'buffer': 'transit_buffer'}[kind],
            {'protected': 'slot_id', 'ready': 'berth_id', 'buffer': 'buffer_id'}[kind]: number}


def union(boxes):
    require(bool(boxes), 'empty bounds domain')
    values = np.array(boxes)
    require(np.isfinite(values).all(), 'nonfinite bounds')
    return [values[:, 0].min(0).tolist(), values[:, 1].max(0).tolist()]


def axes(q):
    x, y, z, w = q
    return np.array([[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                     [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                     [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]])


def corners(bounds):
    return np.array(list(itertools.product(*zip(*bounds))))


def local(bounds, reference):
    points = (corners(bounds)-reference['position_m']) @ axes(reference['orientation_xyzw'])
    return [points.min(0).tolist(), points.max(0).tolist()]


def mechanism_owner(group):
    if group.startswith('cell'):
        bid = int(group[4:6])
        require(0 <= bid < 10, 'invalid cell mechanism owner')
        if group.startswith(f'cell{bid:02d}/buffer/latch/'):
            return {'kind': 'dock', 'dock': dock(['buffer', bid])}
        if group.startswith(f'cell{bid:02d}/protected/'):
            return {'kind': 'dock', 'dock': dock(['protected', bid])}
        return {'kind': 'loader', 'loader_id': bid}
    if group.startswith('c') and group[1:3].isdigit():
        return {'kind': 'cassette', 'cassette_id': int(group[1:3])}
    if group.startswith('b') and group[1:3].isdigit():
        return {'kind': 'berth', 'berth_id': int(group[1:3])}
    if group.startswith('dock/'):
        _, kind, number, *_ = group.split('/')
        return {'kind': 'dock', 'dock': dock([kind, int(number)])}
    raise ValueError(f'unowned/shared mechanism is not a parallel-cell root: {group}')


def compound_parts_sha256(rows):
    """Canonical v1 binary, NOT JSON float text (shared Rust/Python contract).

    Prefix b'arkline-carrier-compound-v1\0'; u32LE group count; for each
    lexicographically ordered group: UTF8 ID with u32LE byte length, u32LE
    part count; each lexicographically ordered part: length-prefixed UTF8
    source ID then six IEEE754 f64LE values minXYZ,maxXYZ. No trailing bytes.
    """
    digest = hashlib.sha256(b'arkline-carrier-compound-v1\0')
    def u32(number):
        digest.update(struct.pack('<I', number))
    def string(value):
        raw = value.encode('utf-8')
        u32(len(raw))
        digest.update(raw)
    require([r['semantic_id'] for r in rows] == sorted(r['semantic_id'] for r in rows),
            'compound groups are not canonically ordered')
    u32(len(rows))
    for row in rows:
        string(row['semantic_id'])
        parts = row['parts']
        require([p['source_part_id'] for p in parts] == sorted(p['source_part_id'] for p in parts),
                'compound leaves are not canonically ordered')
        u32(len(parts))
        for part in parts:
            string(part['source_part_id'])
            bounds = np.asarray(part['local_bounds_m'], dtype=float)
            require(bounds.shape == (2, 3) and np.isfinite(bounds).all()
                    and (bounds[0] <= bounds[1]).all(), 'invalid compound leaf bounds')
            digest.update(struct.pack('<6d', *bounds.reshape(6)))
    return digest.hexdigest()


def source_collision_parts(doc):
    """Every original logical solid, under its original root/reference pose.

    source_part_id is the manifest logical semantic_id, NOT the batched render
    name: batching must never coarsen collision leaves or lose leaf identity.
    """
    refs = doc['mechanism_reference_poses']
    grouped = {group: [] for group in refs}
    seen = set()
    for part in doc['parts']:
        group = part['group']
        if group is None:
            continue
        require(group in refs, 'logical solid has unknown dynamic owner')
        identity = part['semantic_id']
        require(identity not in seen, 'duplicate dynamic logical source solid')
        seen.add(identity)
        bounds = np.asarray(part['bounds_body_m'], dtype=float)
        require(bounds.shape == (2, 3) and np.isfinite(bounds).all()
                and (bounds[0] <= bounds[1]).all(), 'invalid source solid bounds')
        grouped[group].append({'source_part_id': identity,
                               'local_bounds_m': local(bounds, refs[group])})
    require(all(grouped.values()), 'dynamic root has no logical collision solids')
    return [{'semantic_id': group,
             'parts': sorted(grouped[group], key=lambda p: p['source_part_id'])}
            for group in sorted(grouped)]


def validate_collision_parts(doc, rows, coverage, assembly_source_sha256):
    """Fail closed on missing/duplicate/misowned solids, even if rehashed."""
    expected = source_collision_parts(doc)
    require(rows == expected, 'compound coverage differs from exact logical source ownership/bounds')
    count = sum(len(row['parts']) for row in rows)
    require(coverage == {'assembly_source_sha256': assembly_source_sha256,
                        'mechanism_count': len(rows), 'logical_part_count': count,
                        'parts_sha256': compound_parts_sha256(rows)},
            'compound coverage source/count/hash mismatch')
    return count


def compile_fixture(doc, export, manifest_sha, extra_proof=None, *, revision=REVISION):
    """Pure construction; caller owns exact-source proof review and persistence."""
    routes.initial(doc)  # validate finite stock and aligned ten-cell topology
    require(export['source_sha256'] == doc['output_sha256'], 'export/source hash mismatch')
    require(export['authoring_sha256'] == manifest_sha, 'export/manifest hash mismatch')
    refs = doc['mechanism_reference_poses']
    require(not any(g.startswith('dock/buffer/') for g in refs),
            'obsolete front-buffer roots must be removed, not aliased to local pockets')
    buffers = doc['exchange_buffers']
    require(len(buffers) == 10, 'requires ten actual lower buffer pockets')
    for bid, buffer in enumerate(buffers):
        require(buffer['body_center_m'] == [-26.+8*(bid % 5), 0., -30. if bid < 5 else 30.],
                'declared lower buffer pocket pose mismatch')
        require(buffer['side'] == ('port' if bid < 5 else 'starboard'), 'buffer side mismatch')
    for cell in doc['parallel_local_cells']:
        require(cell['loader_id'] == cell['cell_id'] == cell['buffer_id'],
                'local loader/buffer ownership must match cell identity')
        require(cell['capacity_added'] == 0, 'local buffer is transit, not added stock capacity')
    registration = {'schema_version': 1,
                    'host': {'geometry_revision': 12, 'source_sha256': HOST_SHA},
                    'assembly_revision': revision,
                    'assembly_source_sha256': export['output']['sha256']}
    classes = ['recon', 'mining', 'attack']
    side = lambda bid: 'port' if bid < 5 else 'starboard'
    topology = {'registration': registration,
        'ready_berths': [{'berth_id': i, 'side': side(i), 'compatible_classes': classes} for i in range(10)],
        'magazine_slots': [{'slot_id': i, 'side': side(i), 'compatible_classes': classes} for i in range(10)],
        'loaders': [{'loader_id': i, 'side': side(i), 'magazine_slots': [i],
                     'ready_berths': [i], 'transit_buffer_id': i} for i in range(10)],
        'transit_buffers': [{'buffer_id': i, 'side': side(i)} for i in range(10)],
        'cassette_count': 20}
    byname = {row['name']: row for row in export['source_parts']}
    require(len(byname) == len(export['source_parts']), 'ambiguous exported render name')
    render_owners = {}
    for part in doc['parts']:
        name = part['render_semantic_id']
        require(name in byname, f'missing exported batch: {name}')
        if name in render_owners:
            require(render_owners[name] == part['group'], 'render batch crosses kinematic owners')
        render_owners[name] = part['group']
    mechanisms, dynamic, groupboxes = [], [], {}
    ranges = {}
    keys = ('source_id', 'render_primitive', 'first_index', 'index_count')
    for group, reference in refs.items():
        parts = [p for p in doc['parts'] if p['group'] == group]
        bounds = union([p['bounds_body_m'] for p in parts])
        groupboxes[group] = bounds
        mechanisms.append({'semantic_id': group, 'owner': mechanism_owner(group),
                           'reference_pose': reference, 'local_bounds_m': local(bounds, reference)})
        # Many logical support pieces share one exported batch. Emit its range
        # ONCE, not once per logical piece (which would duplicate triangles).
        unique = {}
        for part in parts:
            row = byname[part['render_semantic_id']]
            identity = tuple(row[key] for key in keys)
            unique[identity] = {key: row[key] for key in keys}
        for row in unique.values():
            primitive, start, count = row['render_primitive'], row['first_index'], row['index_count']
            require(count > 0 and count % 3 == 0 and start >= 0 and start % 3 == 0, 'invalid exported triangle range')
            for lo, hi, previous_group in ranges.setdefault(primitive, []):
                require(start+count <= lo or start >= hi,
                        f'overlapping render ranges: {previous_group}, {group}')
            ranges[primitive].append((start, start+count, group))
        dynamic.append({'id': group,
            'reference_pose_body_m': {'translation': reference['position_m'],
                                      'rotation_xyzw': reference['orientation_xyzw']},
            'source_parts': list(unique.values())})
    cassettes, docks = [], []
    for cassette in doc['cassettes']:
        cid = cassette['cassette_id']
        position = pose(cassette['body_center_m'], qside(cassette['side']))
        bounds = union([p['bounds_body_m'] for p in doc['parts']
                        if (p['group'] or '').startswith(f'c{cid:02d}/')])
        location = dock(['ready', cid] if cid < 10 else ['protected', cid-10])
        cassettes.append({'cassette_id': cid, 'initial_dock': location, 'reference_pose': position,
                          'bounds_local_m': local(bounds, position),
                          'mechanism_ids': [g for g in refs if g.startswith(f'c{cid:02d}/')]})
        docks.append({'dock': location, 'cassette_pose': position, 'working_bounds_m': bounds})
    for bid in range(10):
        # Exact declared lower pocket, not the retired front-side buffers.
        position = pose([-26.+8*(bid % 5), 0., -30. if bid < 5 else 30.], qside(side(bid)))
        source = next(c for c in cassettes if c['cassette_id'] == bid)
        points = corners(source['bounds_local_m']) @ axes(position['orientation_xyzw']).T + position['position_m']
        docks.append({'dock': dock(['buffer', bid]), 'cassette_pose': position,
                      'working_bounds_m': [points.min(0).tolist(), points.max(0).tolist()]})
    allbounds = []

    def convert(frame, loaded, empty, action, bid):
        state, handled = frame['state'], frame['handled_cassette_id']
        locations = frame['docked_cassettes']
        local_state = state['locals'][str(bid)]
        supported = handled is not None and str(handled) in locations
        locked = False
        if supported:
            kind, number = locations[str(handled)]
            locked = ((local_state['buffer_release'] == 0.) if kind == 'buffer'
                      else state['docks'].get(f'{kind}/{number}', 0.) == 0.)
        hoist_retains = local_state['grip'] == 1.
        shuttle_retains = local_state['shuttle_clamp'] == 0.
        retains = hoist_retains or shuttle_retains
        require(retains == (frame['carried_cassette_id'] is not None),
                f"retention/carry metadata mismatch: {frame['label']}")
        # A hoist parked overhead is not enough while the shuttle is still
        # carrying or interfering with the fixed base channel below the drone.
        clear = (local_state['grip'] == 0. and local_state['height'] == 5.1
                 and local_state['shuttle_clamp'] == 1.
                 and local_state['shuttle_lift'] == -.35)
        terminal_stage = action == 'stage' and frame['completed_stage_legs'] == 3
        ready_z = -22.8 if bid < 5 else 22.8
        stage_safe = (terminal_stage and clear and local_state['z'] == ready_z
                      and local_state['shuttle_z'] == ready_z
                      and local_state['floor'] == 0. and local_state['support'] == 0.
                      and local_state['buffer_release'] == 0.
                      and state['hatches'].get(str(bid), 0.) == 0.
                      and locations.get(str(loaded)) == ['ready', bid]
                      and locations.get(str(empty)) == ['protected', bid])
        if terminal_stage:
            require(stage_safe, 'final local exchange is not hardware safe')
        machinery = []
        for group in frame['owned_groups']:
            shift = np.array(routes.translation(doc, group, state))
            reference = refs[group]
            machinery.append([group, pose((np.array(reference['position_m'])+shift).tolist(), reference['orientation_xyzw'])])
            allbounds.append((np.array(groupboxes[group])+shift).tolist())
        orientation = qside(side(bid))
        return {'label': frame['label'], 'phase': 'loading' if action == 'stage' else 'launching',
            'drone_pose': pose(frame.get('drone_position', state['cassettes'][str(loaded)]), orientation),
            'handled_cassette_id': handled, 'carried_cassette_id': frame['carried_cassette_id'],
            'cassette_poses': [[cid, pose(state['cassettes'][str(cid)], orientation)] for cid in sorted({loaded, empty})],
            'docked_cassettes': [[int(cid), dock(location)] for cid, location in sorted(locations.items())],
            'machinery': machinery, 'completed_stage_legs': frame['completed_stage_legs'],
            'milestones': {'drone_clamped': frame.get('launch_drone_clamped', True),
                'cassette_supported': supported, 'cassette_locked': locked,
                'crane_latched': retains, 'door_clear': bool(state['doors'].get(str(bid), 0.)),
                'ore_coupled': False, 'exterior_clear': frame.get('launch_exterior_clear', False),
                'loader_clear': clear, 'hardware_safe': stage_safe or (action == 'launch' and frame['label'] == 'launch/hardware secured')}}

    stages = [{'source_slot': bid, 'destination_berth': bid, 'loaded_cassette_id': bid+10,
               'empty_cassette_id': bid,
               'keyframes': [convert(f, bid+10, bid, 'stage', bid) for f in routes.stage(doc, bid, bid)]}
              for bid in range(10)]
    launches = [{'berth_id': bid, 'cassette_id': bid,
                 'keyframes': [convert(f, bid, bid, 'launch', bid) for f in routes.bay(doc, bid)]}
                for bid in range(10)]
    berths = []
    for berth in doc['bays']:
        bid = berth['bay_id']
        def berth_pose(distance):
            p = list(berth['center_m'])
            p[2] += (-1 if bid < 5 else 1)*distance
            return pose(p, qside(side(bid)))
        bounds = [groupboxes[g] for g in refs if g.startswith(f'b{bid:02d}/')]
        bounds.append([(np.array(berth['center_m'])+v).tolist() for v in ([-3.5, -2., -19.], [3.5, 3., 19.])])
        berths.append({'berth_id': bid, 'support_pose': berth_pose(5.9), 'contact_pose': berth_pose(5.9),
            'gate_pose': berth_pose(16.), 'clear_pose': berth_pose(16.), 'ore_transfer_capable': False,
            'drone_bounds_local_m': [[-2.5, -.9, -3.], [2.5, .9, 3.]], 'working_bounds_m': union(bounds)})
    carrier_bounds = [(np.array(export['output']['body_bounds_m'][0])-.001).tolist(),
                      (np.array(export['output']['body_bounds_m'][1])+.001).tolist()]
    # This composed bank still carries both moving turrets. A closed-hull-only
    # envelope is not a valid deployed fixture, so the exact-source union is mandatory.
    require(extra_proof is not None, 'exact-source deployed turret proof required')
    require(extra_proof['composed_glb_sha256'] == registration['assembly_source_sha256']
            and not extra_proof['failures'], 'unbound/failed extra deployed proof')
    turret_bounds = np.asarray(extra_proof['turret_union_bounds_m'], dtype=float)
    require(turret_bounds.shape == (2, 2, 3), 'expected two turret AABBs (2x2x3)')
    require(np.isfinite(turret_bounds).all(), 'nonfinite deployed turret bounds')
    require((turret_bounds[:, 0] <= turret_bounds[:, 1]).all(), 'inverted deployed turret bounds')
    extra = []
    extra.extend(turret_bounds.tolist())
    # Authored roots are binding references, not necessarily the real idle
    # mechanism configuration (the shuttle must be lowered and clamps open).
    idle_state = routes.initial(doc)
    idle_machinery = []
    for group, reference in refs.items():
        shift = np.asarray(routes.translation(doc, group, idle_state), dtype=float)
        require(shift.shape == (3,) and np.isfinite(shift).all(), 'invalid idle root translation')
        idle_machinery.append([group, pose((np.asarray(reference['position_m'])+shift).tolist(),
                                           reference['orientation_xyzw'])])
        allbounds.append((np.asarray(groupboxes[group])+shift).tolist())
    definition = {'topology': topology, 'carrier_bounds_m': carrier_bounds,
        'deployed_hardware_bounds_m': union(allbounds+[carrier_bounds]+extra),
        'cassettes': cassettes, 'docks': docks, 'berths': berths, 'mechanisms': mechanisms}
    route_hashes = {name: hashlib.sha256(Path(routes.__file__).with_name(name).read_bytes()).hexdigest()
                    for name in ('parallel_bank_routes.py', 'parallel_routes.py', 'candidate_routes.py')}
    fixture = {'scope': 'unadmitted 1703 stationary test fixture only; aligned Stage and Launch; normal factory disabled',
        'manifest_sha256': manifest_sha, 'route_sha256': route_hashes['parallel_bank_routes.py'],
        'route_source_sha256': route_hashes, 'definition': definition, 'stages': stages, 'launches': launches,
        'idle_machinery': idle_machinery,
        'retention_semantics': 'crane_latched means positively retained by hoist OR lower shuttle; presentation must say cassette retained unless device is identified'}
    collision_parts = source_collision_parts(doc)
    collision_coverage = {'assembly_source_sha256': registration['assembly_source_sha256'],
        'mechanism_count': len(collision_parts),
        'logical_part_count': sum(len(row['parts']) for row in collision_parts),
        'parts_sha256': compound_parts_sha256(collision_parts)}
    validate_collision_parts(doc, collision_parts, collision_coverage, registration['assembly_source_sha256'])
    broad = {m['semantic_id']: np.asarray(m['local_bounds_m']) for m in mechanisms}
    for row in collision_parts:
        for part in row['parts']:
            box = np.asarray(part['local_bounds_m'])
            require(np.all(box[0] >= broad[row['semantic_id']][0]-1e-10)
                    and np.all(box[1] <= broad[row['semantic_id']][1]+1e-10),
                    'compound leaf exceeds unchanged broad union')
    fixture['mechanism_collision_parts'] = collision_parts
    fixture['collision_coverage'] = collision_coverage
    count = sum(row['index_count']//3 for group in dynamic for row in group['source_parts'])
    require(count <= export['output']['triangles'], 'dynamic geometry exceeds source total')
    binding = {'schema': 'arkline.pioneer-drone-bay-runtime-binding/v1', 'registration_revision': revision,
        'glb_sha256': registration['assembly_source_sha256'], 'source_triangle_count': export['output']['triangles'],
        'dynamic_triangle_count': count, 'static_triangle_count': export['output']['triangles']-count,
        'dynamic_groups': dynamic}
    return fixture, binding


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--export-receipt', type=Path, required=True)
    parser.add_argument('--fixture-output', type=Path, required=True)
    parser.add_argument('--binding-output', type=Path, required=True)
    parser.add_argument('--extra-deployed-proof', type=Path, required=True)
    args = parser.parse_args()
    require(args.fixture_output.resolve() != args.binding_output.resolve(), 'outputs must be distinct')
    require(not args.fixture_output.exists() and not args.binding_output.exists(), 'new outputs required; will not overwrite')
    raw = args.manifest.read_bytes()
    fixture, binding = compile_fixture(json.loads(raw), json.loads(args.export_receipt.read_text()),
        hashlib.sha256(raw).hexdigest(),
        json.loads(args.extra_deployed_proof.read_text()) if args.extra_deployed_proof else None)
    for path, value in ((args.fixture_output, fixture), (args.binding_output, binding)):
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open('x') as output:
            output.write(json.dumps(value, separators=(',', ':'))+'\n')
    print(json.dumps({'registration': fixture['definition']['topology']['registration'],
                      'stages': len(fixture['stages']), 'launches': len(fixture['launches']),
                      'dynamic_groups': len(binding['dynamic_groups']),
                      'dynamic_triangles': binding['dynamic_triangle_count']}))


if __name__ == '__main__':
    main()
