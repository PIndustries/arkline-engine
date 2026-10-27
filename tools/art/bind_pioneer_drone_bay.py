#!/usr/bin/env python3
"""Bind semantic bay groups to one exact bounded exporter receipt (stdlib only)."""
import argparse,hashlib,json
from pathlib import Path

def bind(authoring,receipt,glb):
    a=json.loads(authoring.read_text());r=json.loads(receipt.read_text())
    assert a['output_sha256']==r['source']['sha256']
    assert hashlib.sha256(glb.read_bytes()).hexdigest()==r['output']['sha256']
    names={p['name']:p for p in r['source_parts']};seen=set();groups=[]
    slots={s['tray_group']:s for s in a['slots']}
    for gid,members in a['dynamic_groups'].items():
        spans=[]
        for name in members:
            assert name not in seen and name in names;seen.add(name)
            p=names[name];assert p['first_index']%3==0 and p['index_count']%3==0
            spans.append({k:p[k] for k in ['name','source_id','render_primitive','first_index','index_count']})
        origin=[-30.5,-1,-3.5]
        if gid in slots:origin=slots[gid]['body_center_m']
        elif gid=='door/upper':origin=[-35.58,-.19,-3.5]
        elif gid=='door/lower':origin=[-35.58,-1.81,-3.5]
        rule=a['kinematics'][gid]
        groups.append({'id':gid,'reference_pose_body_m':{'translation':origin,'rotation_xyzw':[0,0,0,1]},'vertices_space':'absolute body metres; apply current_pose * inverse(reference_pose) once','motion':rule,'source_parts':spans})
    spans=sorted((p['render_primitive'],p['first_index'],p['first_index']+p['index_count'],p['name']) for p in r['source_parts'])
    ends={}
    for primitive,start,end,name in spans:
        assert start==ends.get(primitive,0),'source receipt must partition material indices without gaps/overlap'
        ends[primitive]=end
    assert sum(ends.values())//3==r['output']['triangles']
    dynamic_triangles=sum(p['index_count']//3 for g in groups for p in g['source_parts'])
    return {'schema':'arkline.pioneer-drone-bay-runtime-binding/v1','registration_revision':12,'blend_sha256':r['source']['sha256'],'glb_sha256':r['output']['sha256'],'authoring_manifest_sha256':hashlib.sha256(authoring.read_bytes()).hexdigest(),'export_receipt_sha256':hashlib.sha256(receipt.read_bytes()).hexdigest(),'source_triangle_count':r['output']['triangles'],'dynamic_triangle_count':dynamic_triangles,'static_triangle_count':r['output']['triangles']-dynamic_triangles,'partition':'Every receipt source part exactly once; subtract only listed dynamic ranges from original material primitives. Preserve original material identity per range. No arbitrary hull hiding.','dynamic_groups':groups,'slots':a['slots'],'floor_profile_xy_m':a['floor_profile_xy_m'],'launch_release_body_m':a['launch_release_body_m'],'release_tray_center_m':a['release_tray_center_m'],'release_disengage_y_m':a['release_disengage_y_m'],'release_contract':a['release_contract'],'aperture_body_m':a['aperture_body_m'],'group_translation_contract':{'carriage_x':'[carriage_x_m + 30.5, 0, 0]','lift_xy':'[carriage_x_m + 30.5, support_y_m + 1, 0]','fork_stageN':'[carriage_x_m + 30.5, support_y_m + 1, 3.5 * N * fork_extension_fraction]','tray/sNN':'current_tray_center_body_m - slot.body_center_m; inactive trays identity','door/upper':'[0, +1.70 * open_fraction, 0]','door/lower':'[0, -1.70 * open_fraction, 0]'},'phase_interlocks':['Only one active tray/drone and one throat operation','Fork must be fully retracted before any carriage X or support Y move','Fork may extend only at selected stationary station/tier; selected tray remains parked until extension complete','Extraction: tray Z shift = -7*t; fork extension fraction = 1-t','Move support Y to -1 before transporting selected tray to aft station X=-30.5','At exit, same drone holds Y=-1 while selected tray/support lower0.20m to Y=-1.2','Drone may translate aft only after200mm support disengagement and doors fully open','Recovery reverses: drone stops above lowered tray then support raises200mm before transport','Do not close door while drone/body swept volume occupies throat'],'limits':['Source geometry and kinematic contract, not authority or physics implementation','No force/strength/pressure ratings; no crew route','Stored drone meshes are not baked; draw persistent runtime IDs with unchanged class geometry','Full parent quaternion/velocity inherited at runtime; roots are ship-body frame, not station-yaw frame']}
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--authoring',type=Path,required=True);p.add_argument('--receipt',type=Path,required=True);p.add_argument('--glb',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();assert not a.output.exists();a.output.write_text(json.dumps(bind(a.authoring,a.receipt,a.glb),indent=2)+'\n')
