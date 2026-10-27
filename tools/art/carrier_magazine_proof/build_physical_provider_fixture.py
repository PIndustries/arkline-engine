"""Compile a NEW exact1704 Mining/bay0 inspection fixture. Never normal admission.

Original1703 files and routes remain immutable. The common constructor reuses
only transform/topology extraction; each input is independently source pinned.
Dynamic hollow receiver parts are decomposed into occupied cells, never bores.
"""
import argparse
import copy
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import numpy as np
import build_parallel_provider_fixture as base
from physical_provider.source import sha, require, read_pinned, glb_primitives, occupied_cells, export_guard, local_guard, measured_logical_enclosures


def identity(root, manifest):
    assets={k:manifest['inputs'][k] for k in sorted(manifest['inputs'])}
    cells=json.loads(read_pinned(root,assets['ship_capacity_cells']).read_text())
    drone=json.loads(read_pinned(root,assets['drone_full_cell_path_valve']).read_text())
    box=[[int(Fraction(x)*1000) for x in row] for row in drone['capacity']['box_m']]
    def hold(name,role,values):
        return {'source_hold_id':name,'measurement_receipt_sha256':assets[role]['sha256'],'usable_cells':values}
    value={'schema_version':1,'candidate_revision':1704,
        'assembly_manifest_sha256':sha(root/'final-integration-proof/assembly-manifest.json'),
        'assets':assets,'receiver_panel_travel_mm':94,
        'measured_holds':{'ship_source_sha256':assets['ship_source']['sha256'],
            'mining_drone_source_sha256':assets['drone_source']['sha256'],
            'ship_hold':hold('pioneer.physical-1704.ship-hold','ship_full_capacity_proof',[{'min':c['min'],'max':c['max']}for c in cells['usable_cells_mm']]),
            'mining_drone_hold':hold('pioneer.physical-1704.mining-drone-hold','drone_full_cell_path_valve',[{'min':box[0],'max':box[1]}])}}
    return hashlib.sha256(json.dumps(value,separators=(',',':')).encode()).hexdigest()


def service(launch, old_frames, refs):
    """Ship sockets seat before explicit drone/ship opening; reverse before release."""
    idle=launch['keyframes'][-1]
    # Existing source-bound poses are preserved for old roots. New valves added
    # from their exact references, with both source-specific strokes explicit.
    frames=[]
    for i,old in enumerate(old_frames):
        f=copy.deepcopy(old)
        poses=dict(f['machinery'])
        for group,p in idle['machinery']:
            if group not in poses:poses[group]=copy.deepcopy(p)
        require(set(poses)==set(dict(idle['machinery'])), 'service writer mismatch')
        f['machinery']=[[g,poses[g]]for g,_ in idle['machinery']]
        f['label']='physical1704/'+f['label']
        f['milestones']['ore_coupled']=False
        frames.append({'frame':f,'drone_inlet_travel_mm':0})
        if i==5:
            opening=copy.deepcopy(frames[-1]); opening['drone_inlet_travel_mm']=250
            opening['frame']['label']='physical1704/drone inlet opens after seated socket jaws'
            frames.append(opening)
            coupled=copy.deepcopy(opening)
            coupled['frame']['label']='physical1704/ship valve opens: exact dual-open stable contact barrier'
            for g,p in coupled['frame']['machinery']:
                if g.startswith('b00/fluid/valve/'):
                    p['position_m'][1]=refs[g]['position_m'][1]+int(g.rsplit('/',1)[1])*.094
            coupled['frame']['milestones']['ore_coupled']=True
            frames.append(coupled)
            close_ship=copy.deepcopy(opening); close_ship['frame']['label']='physical1704/ship valve closes before drone inlet'
            frames.append(close_ship)
            close_drone={'frame':copy.deepcopy(f),'drone_inlet_travel_mm':0};close_drone['frame']['label']='physical1704/drone inlet closes before socket release'
            frames.append(close_drone)
    return {'berth_id':0,'cassette_id':0,'keyframes':frames}


def compile_physical(root, old_export_path, old_turret_path, old_glb_path, old_service_path):
    manifest_path=root/'final-integration-proof/assembly-manifest.json'
    manifest=json.loads(manifest_path.read_text()); inputs=manifest['inputs']
    paths={k:read_pinned(root,v) for k,v in inputs.items()}
    doc=json.loads(paths['ship_manifest'].read_text()); export=json.loads(paths['ship_export_receipt'].read_text())
    module=json.loads(paths['receiver_module'].read_text()); refs=doc['mechanism_reference_poses']
    require(doc['output_sha256']==inputs['ship_source']['sha256']==export['source_sha256'], 'authored source linkage')
    require(export['output']['sha256']==inputs['ship_glb']['sha256'], 'export linkage')
    require(manifest['motion_precedence']['receiver_panel_travel_mm']==94, 'wrong stroke')
    actual=glb_primitives(paths['ship_glb']); old=glb_primitives(old_glb_path)
    old_receipt=json.loads(old_export_path.read_text()); old_turret=json.loads(old_turret_path.read_text())
    require(sha(old_glb_path)==old_turret['composed_glb_sha256']==old_receipt['output']['sha256'], 'old turret provenance')
    new_by={p['name']:p for p in export['source_parts']}
    old_by={p['name']:p for p in old_receipt['source_parts']}
    # Verify every retained turret object from actual indexed exported triangles.
    retained=[]
    for name,row in old_by.items():
        if not(name.startswith('pioneer.turret') or 'turret' in name.lower()):continue
        n=new_by.get(name); require(n is not None,'removed turret object')
        t=old[row['render_primitive']][row['first_index']//3:(row['first_index']+row['index_count'])//3]
        u=actual[n['render_primitive']][n['first_index']//3:(n['first_index']+n['index_count'])//3]
        require(row['material']==n['material'] and np.array_equal(t,u), 'changed turret source')
        retained.append(name)
    require(len(retained)>20 and not old_turret['failures'], 'incomplete turret provenance')
    # This inherits only an envelope for exact unchanged turret geometry, NOT the
    # old route collision proof. New route acceptance is a separate signed receipt.
    envelope={'composed_glb_sha256':inputs['ship_glb']['sha256'],'failures':[],
        'turret_union_bounds_m':old_turret['turret_union_bounds_m'],
        'scope':'retained exact turret envelope only, no new route acceptance'}
    normalized=copy.deepcopy(export)
    normalized['authoring_sha256']=sha(paths['ship_manifest'])
    fixture,binding=base.compile_fixture(doc,normalized,sha(paths['ship_manifest']),envelope,revision=1704)
    fixture['scope']='NEW unadmitted1704 Mining bay0 inspection only; source-specific closed normal factory'
    definition=fixture['definition']
    for row in definition['topology']['ready_berths']+definition['topology']['magazine_slots']:
        row['compatible_classes']=['mining']
    drone=np.concatenate(glb_primitives(paths['drone_glb']))
    drone_bounds=[drone.min((0,1)).tolist(),drone.max((0,1)).tolist()]
    for b in definition['berths']:
        b['drone_bounds_local_m']=drone_bounds
    by_module={p['id']:p for p in module['parts']}
    grouped={g:[]for g in refs}; coverage=[]
    measured,witnesses=measured_logical_enclosures(doc,export,actual)
    for part in doc['parts']:
        group=part['group']
        if not group:continue
        boxes=[measured.get(part['semantic_id'],part['bounds_body_m'])]
        if 'new_module_part_id' in part:
            bid=int(group[1:3]); transform=doc['receiver_rebuild']['transforms'][bid]
            sign=np.asarray(transform['sign']);off=np.asarray(transform['offset_m'])
            p=by_module[part['new_module_part_id']]
            boxes=[np.sort(np.asarray(b)*sign+off,axis=0).astype(np.float32).astype(float).tolist()for b in occupied_cells(p)]
        for i,b in enumerate(boxes):
            name=part['semantic_id'] if len(boxes)==1 else f"{part['semantic_id']}/occupied-cell/{i:03d}"
            grouped[group].append({'source_part_id':name,'local_bounds_m':local_guard(export_guard(b),refs[group],base.local)})
        coverage.append({'source_part_id':part['semantic_id'],'root':group,'occupied_cells':len(boxes)})
    rows=[{'semantic_id':g,'parts':sorted(v,key=lambda x:x['source_part_id'])}for g,v in sorted(grouped.items())]
    fixture['mechanism_collision_parts']=rows
    by_collision={row['semantic_id']:row['parts']for row in rows}
    for mechanism in definition['mechanisms']:
        mechanism['local_bounds_m']=base.union([p['local_bounds_m']for p in by_collision[mechanism['semantic_id']]])
    definition['deployed_hardware_bounds_m']=export_guard(definition['deployed_hardware_bounds_m'])
    fixture['collision_coverage']={'assembly_source_sha256':inputs['ship_glb']['sha256'],'mechanism_count':len(rows),
        'logical_part_count':sum(len(r['parts'])for r in rows),'parts_sha256':base.compound_parts_sha256(rows)}
    # No all-cell route admission follows from complete source extraction.
    launch=fixture['launches'][0]; fixture['launches']=[launch]; fixture['stages']=[]
    capture=copy.deepcopy(launch);capture['keyframes'].reverse()
    for i,f in enumerate(capture['keyframes']):
        f['label']='physical1704/capture/'+f['label'];f['phase']='returning';f['milestones']['hardware_safe']=i==len(capture['keyframes'])-1
    fixture['captures']=[capture]
    mating=json.loads((root/'drone-reducer-new/exact-mating-transform.json').read_text())
    require(sha(old_service_path)==mating['route_sha256'], 'service source frames mismatch')
    old_service=json.loads(old_service_path.read_text());frames=next(r['frames']for r in old_service if r['berth']==0)
    fixture['services']=[service(launch,frames,refs)]
    fixture['physical_source_identity_sha256']=identity(root,manifest)
    fixture['receiver_motion_contract_sha256']=inputs['receiver_motion_contract']['sha256']
    fixture['receiver_panel_travel_mm']=94
    receipt={'scope':'extraction/provenance only; new route clearance tested separately','inputs':manifest['inputs'],
        'manifest_sha256':sha(manifest_path),'normalized_export_note':'authoring_sha256 recomputed from separately pinned manifest for shared constructor, not asserted as original export receipt field',
        'old_turret_proof_sha256':sha(old_turret_path),'old_export_sha256':sha(old_export_path),'old_service_sha256':sha(old_service_path),
        'retained_turret_objects':retained,'collision_coverage':fixture['collision_coverage'],'source_part_to_cells':coverage,
        'collision_export_guard':'one outward f32 ULP per world face; local f64 arithmetic guard; full actual triangle-union proof required',
        'measured_export_enclosure_witnesses':witnesses,
        'maximum_measured_enclosure_expansion_m':max((w['measured_enclosure_expansion_m']for w in witnesses),default=0.),
        'dynamic_source_parts':len(coverage),'static_manifest_parts':sum(p['group']is None for p in doc['parts']),
        'actual_drone_bounds_m':drone_bounds,'physical_source_identity_sha256':fixture['physical_source_identity_sha256']}
    return fixture,binding,receipt


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['physical-root','old-export','old-turret-proof','old-glb','old-service','output']:
        p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args();require(not a.output.exists(),'new output directory required')
    values=compile_physical(a.physical_root,a.old_export,a.old_turret_proof,a.old_glb,a.old_service)
    a.output.mkdir(parents=True)
    for name,value in zip(('fixture.json','runtime-binding.json','extraction-receipt.json'),values):
        (a.output/name).write_text(json.dumps(value,separators=(',',':'))+'\n')
    print(json.dumps(values[2]['collision_coverage']))
if __name__=='__main__':main()
