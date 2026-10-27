#!/usr/bin/env python3
"""Conservative continuous box proof of proposed drone-bay relative motions.

Independent from runtime; supports authoring/v1 staged manifest. Source boxes
contain beveled meshes. Static environment proof is deliberately separate.
"""
import argparse,hashlib,json,struct
from pathlib import Path
import numpy as np

def overlap_interval(a0,a1,b0,b1,eps=1e-5):
    low,high=0.,1.
    for axis in range(3):
        for c0,c1 in [(a0[1,axis]-b0[0,axis]-eps,a1[1,axis]-b1[0,axis]-eps),(b0[1,axis]-a0[0,axis]-eps,b1[1,axis]-a1[0,axis]-eps)]:
            slope=c1-c0
            if abs(slope)<1e-12:
                if c0<=0:return None
            elif slope>0:low=max(low,-c0/slope)
            else:high=min(high,-c0/slope)
            if low>=high:return None
    return [low,high]

def validate(manifest,glb=None,closed_doors=False,omit_release=False):
    p=json.loads(manifest.read_text());assert p['schema']=='arkline.pioneer-drone-bay-authoring/v1'
    parts=[v for v in p['parts'] if v['group'] is not None]
    assert all('bounds_body_m' in v for v in parts),'Every dynamic part must have conservative box'
    for v in parts:
        b=np.array(v['bounds_body_m']);assert b.shape==(2,3) and np.isfinite(b).all() and (b[1]>b[0]).all()
    groups=p['dynamic_groups'];claimed=[n for ns in groups.values() for n in ns]
    assert len(claimed)==len(set(claimed))==len(parts)
    assert set(claimed)=={m['source_name'] for m in parts}
    mesh_binding=None
    if glb:
        receipt=json.loads(glb.with_name('receipt.json').read_text())
        data=glb.read_bytes();digest=hashlib.sha256(data).hexdigest()
        assert receipt['output']['sha256']==digest
        assert receipt['source']['sha256']==p['output_sha256']
        n,_=struct.unpack_from('<II',data,12);g=json.loads(data[20:20+n]);off=28+n
        def access(i):
            a=g['accessors'][i];v=g['bufferViews'][a['bufferView']];dim={'SCALAR':1,'VEC3':3}[a['type']];dt={5126:'<f4',5125:'<u4',5123:'<u2'}[a['componentType']]
            return np.frombuffer(data,dtype=dt,offset=off+v.get('byteOffset',0)+a.get('byteOffset',0),count=dim*a['count']).reshape(-1,dim)
        primitives=[access(pr['attributes']['POSITION'])[access(pr['indices']).ravel()] for pr in g['meshes'][0]['primitives']]
        spans={v['name']:v for v in receipt['source_parts']};indices=set();count=0
        for m in parts:
            span=spans[m['source_name']];start=span['first_index'];end=start+span['index_count'];pr=span['render_primitive'];verts=primitives[pr][start:end]
            bounds=np.array(m['bounds_body_m']);assert (verts.min(0)>=bounds[0]-1e-5).all() and (verts.max(0)<=bounds[1]+1e-5).all(),m['source_name']
            keys={(pr,i) for i in range(start,end)};assert not indices.intersection(keys);indices.update(keys);count+=span['retained_triangles']
        mesh_binding={'receipt_source_matches_manifest':True,'all_dynamic_boxes_contain_exported_vertices_within_m':1e-5,'dynamic_source_ranges_disjoint':True,'dynamic_triangles':count}
    kin=p['kinematics'];base=np.array(kin['carriage_x']['reference_center_m']);exit_y=p['exit_y_m'];aisle=p['aisle_z_m'];release=0 if omit_release else p['release_disengage_y_m']
    assert len(p['slots'])==15
    def delta(m,s,x,y,f,tp):
        g=m['group'];q=np.zeros(3)
        if g=='carriage_x':q[0]=x-base[0]
        elif g=='lift_xy' or g.startswith('fork_stage'):
            q[:2]=[x-base[0],y-kin['lift_xy']['y_reference_m']]
            if g.startswith('fork_stage'):q[2]=kin[g]['extension_translation_z_m'][1]*f
        elif g==s['tray_group']:q=np.array(tp)-s['body_center_m']
        elif g.startswith('door/') and not closed_doors:q[1]=kin[g]['translation_y_m'][1]
        return q
    names=['empty_carriage_x','empty_lift_y','fork_extend','extract','loaded_lift_y','loaded_carriage_x','release_support','drone_depart']
    hits=[];tests=0
    for s in p['slots']:
        sx,sy,sz=s['body_center_m'];rack=[sx,sy,sz];launch=[base[0],exit_y,aisle];lowered=[base[0],exit_y+release,aisle]
        states=[(base[0],exit_y,0,rack,rack),(sx,exit_y,0,rack,rack),(sx,sy,0,rack,rack),(sx,sy,1,rack,rack),(sx,sy,0,[sx,sy,aisle],[sx,sy,aisle]),(sx,exit_y,0,[sx,exit_y,aisle],[sx,exit_y,aisle]),(base[0],exit_y,0,launch,launch),(base[0],exit_y+release,0,lowered,launch),(base[0],exit_y+release,0,lowered,[-40,exit_y,aisle])]
        for phase,(st0,st1) in enumerate(zip(states,states[1:])):
            objs=[]
            for m in parts:
                bounds=[np.array(m['bounds_body_m'])+delta(m,s,*st[:4]) for st in [st0,st1]]
                objs.append((m['source_name'],m['group'],*bounds))
            for other in p['slots']:
                points=[np.array(st[-1]) if other['id']==s['id'] else np.array(other['body_center_m']) for st in [st0,st1]]
                bbox=np.array(other['drone_bounds_relative_m'])
                objs.append(('drone/'+other['id'],'drone/'+other['id'],bbox+points[0],bbox+points[1]))
            for i,(an,ag,a0,a1) in enumerate(objs):
                for bn,bg,b0,b1 in objs[i+1:]:
                    if ag==bg:continue
                    tests+=1;interval=overlap_interval(a0,a1,b0,b1)
                    if interval is not None:hits.append({'slot':s['id'],'phase':names[phase],'a':an,'b':bn,'interval':interval})
    result={'schema':'arkline.drone-bay-relative-motion-proof/v1','authoring_manifest_sha256':hashlib.sha256(manifest.read_bytes()).hexdigest(),'export_glb_sha256':hashlib.sha256(glb.read_bytes()).hexdigest() if glb else None,'method':'Exact relative linear interval overlap for conservative authored boxes; positive-volume threshold10micrometres','mesh_binding':mesh_binding,'dynamic_groups':len(groups),'dynamic_source_parts':len(parts),'drone_boxes':15,'phases':names,'pair_segment_tests':tests,'closed_door_negative_control':closed_doors,'omitted_release_negative_control':omit_release,'hits':hits,'limitations':['Static environment not checked here','Authored boxes contain bevel geometry; no runtime execution or force proof','Tangent bearing and drone/cradle contact accepted within10micrometres','Departure endpoint X−40m; larger world hazards not covered']}
    return result
if __name__=='__main__':
    ap=argparse.ArgumentParser();ap.add_argument('--manifest',type=Path,required=True);ap.add_argument('--glb',type=Path);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--closed-doors',action='store_true');ap.add_argument('--omit-release',action='store_true');a=ap.parse_args()
    r=validate(a.manifest,a.glb,a.closed_doors,a.omit_release);a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(r,indent=2)+'\n');print(f"{r['pair_segment_tests']} pair/segments, {len(r['hits'])} overlaps")
    expected=bool(a.closed_doors or a.omit_release)
    if bool(r['hits'])!=expected:raise SystemExit('Unexpected clearance/negative-control result')
