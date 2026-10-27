#!/usr/bin/env python3
"""CPU conservative source-bound receiver clearances; no runtime/GPU claim."""
import argparse,hashlib,json,struct
from pathlib import Path
import numpy as np
from validate_drone_bay_relative_motion import overlap_interval

def triangles(glb,receipt):
    raw=glb.read_bytes();r=json.loads(receipt.read_text());assert hashlib.sha256(raw).hexdigest()==r['output']['sha256']
    n=struct.unpack_from('<I',raw,12)[0];j=json.loads(raw[20:20+n]);off=n+28
    def acc(i):
        a=j['accessors'][i];v=j['bufferViews'][a['bufferView']];d={'SCALAR':1,'VEC3':3}[a['type']];return np.frombuffer(raw,dtype={5126:'<f4',5125:'<u4',5123:'<u2'}[a['componentType']],count=a['count']*d,offset=off+v.get('byteOffset',0)+a.get('byteOffset',0)).reshape(-1,d)
    prim=[acc(p['attributes']['POSITION'])[acc(p['indices']).ravel()].reshape(-1,3,3) for p in j['meshes'][0]['primitives']]
    return [(p['name'],prim[p['render_primitive']][p['first_index']//3:(p['first_index']+p['index_count'])//3].astype(float)) for p in r['source_parts']]
def sat(t,lo,hi):
    c=(lo+hi)/2;h=(hi-lo)/2;t=t-c;e=np.roll(t,-1,axis=1)-t
    axes=[np.broadcast_to(v,(len(t),3)) for v in np.eye(3)]+[np.cross(e[:,0],e[:,1])]+[np.cross(e[:,i],v) for i in range(3) for v in np.eye(3)]
    ok=np.ones(len(t),bool)
    for a in axes:
        q=np.einsum('nij,nj->ni',t,a);r=np.abs(a)@h;ok&=(q.min(1)<=r+1e-9)&(q.max(1)>=-r-1e-9)
    return ok

def validate(base,study,export):
    a=json.loads((study/'authoring.json').read_text());r=json.loads((export/'receipt.json').read_text());parts=a['parts'];source=triangles(base/'pioneer.glb',base/'pioneer-export.json');module=triangles(export/'ore-receiver.glb',export/'receipt.json')
    assert a['base_glb_sha256']==hashlib.sha256((base/'pioneer.glb').read_bytes()).hexdigest()
    assert r['source']['sha256']==a['output_sha256']==hashlib.sha256((study/'ore-receiver.blend').read_bytes()).hexdigest()
    exported=dict(module)
    for p in parts:
        t=exported[p['name']];b=np.array(p['bounds_body_m']);assert (t.min((0,1))>=b[0]-1e-5).all() and (t.max((0,1))<=b[1]+1e-5).all()
    def delta(p,e,l):
        g=p['group'];d=np.zeros(3)
        if g and g.startswith('stage/'):d[0]=-int(g[-1])*e/3
        if g and g.startswith('latch/'):
            d[0]=-e;d[2]=(-.03 if g.endswith('port') else .03)*l
        return d
    states=[(0,0),(1,0),(1,1),(1,0),(0,0)]
    hits=[];mount_contacts=[];checks=0
    def source_test(label,box,allowed=False):
        nonlocal checks
        lo,hi=box
        for name,t in source:
            checks+=1
            if (t.max((0,1))<lo).any() or (t.min((0,1))>hi).any():continue
            m=(t.max(1)>=lo).all(1)&(t.min(1)<=hi).all(1)
            if m.any() and sat(t[m],lo,hi).any():
                if allowed and name=='Pressure hull':mount_contacts.append([label,name])
                else:hits.append([label,name])
    for p in parts:
        b=np.array(p['bounds_body_m'])
        for i,(s0,s1) in enumerate(zip(states,states[1:])):
            b0=b+delta(p,*s0);b1=b+delta(p,*s1);sweep=np.array([np.minimum(b0[0],b1[0]),np.maximum(b0[1],b1[1])]);source_test(p['name']+'/'+str(i),sweep,p['hull_contact'])
    # Approach through exact existing hull and STOWED module only. Expanded
    # safety padding is not used to reject intentional mating later.
    approach=np.array([[-52.75,-2.15,1.75],[-37.25,.15,8.25]])
    source_test('full_drone_approach_padded250mm',approach)
    for p in parts:
        b=np.array(p['bounds_body_m'])
        if overlap_interval(approach,approach,b,b):hits.append(['approach',p['name']])
    # Actual drone solid bounds minus the separately authored0.50square recess.
    # Only head/lock-shoe insertion may enter that cavity, never arbitrary hull.
    drone_solids=[np.array(x) for x in [[[-42.5,-1.9,2],[-37.8,-.1,8]],[[-37.8,-1.9,2],[-37.5,-1.25,8]],[[-37.8,-.75,2],[-37.5,-.1,8]],[[-37.8,-1.25,2],[-37.5,-.75,4.75]],[[-37.8,-1.25,5.25],[-37.5,-.75,8]]]]
    for p in parts:
        b=np.array(p['bounds_body_m'])
        for s0,s1 in zip(states,states[1:]):
            b0=b+delta(p,*s0);b1=b+delta(p,*s1)
            for solid in drone_solids:
                checks+=1
                if overlap_interval(b0,b1,solid,solid):hits.append(['socket_remainder',p['name']])
    # Pairwise relative moving hardware; same group is one manufactured assembly.
    # Exact contact surfaces count as legal tangency, never positive penetration.
    hardware_hits=[]
    for s0,s1 in zip(states,states[1:]):
        for i,p in enumerate(parts):
            for q in parts[i+1:]:
                if p['group']==q['group']:continue
                a0=np.array(p['bounds_body_m'])+delta(p,*s0);a1=np.array(p['bounds_body_m'])+delta(p,*s1);b0=np.array(q['bounds_body_m'])+delta(q,*s0);b1=np.array(q['bounds_body_m'])+delta(q,*s1);checks+=1
                if overlap_interval(a0,a1,b0,b1):hardware_hits.append([p['name'],q['name']])
    # Receiver's entire possible envelope versus every old bay moving part and
    # all15 full drone paths. Conservative independent simultaneous envelopes.
    legacy_authoring_sha256=hashlib.sha256((base/'drone-bay-authoring.json').read_bytes()).hexdigest()
    assert legacy_authoring_sha256=='342d050e111ff239d9d3ef540ffa5023906a2ae528db5ae9359eb73a46f32ae8', 'Re-review legacy boxes when authoring manifest changes'
    old=json.loads((base/'drone-bay-authoring.json').read_text());legacy_checks=0
    rb=[]
    for p in parts:
        bs=[np.array(p['bounds_body_m'])+delta(p,*s) for s in states];rb.append((p['name'],np.array([np.min([b[0] for b in bs],0),np.max([b[1] for b in bs],0)])))
    def olddelta(p,slot,st):
        x,y,f,tp,dp=st;g=p['group'];d=np.zeros(3)
        if g=='carriage_x':d[0]=x+30.5
        elif g=='lift_xy' or (g and g.startswith('fork_stage')):
            d[:2]=[x+30.5,y+1]
            if g.startswith('fork_stage'):d[2]=old['kinematics'][g]['extension_translation_z_m'][1]*f
        elif g==slot['tray_group']:d=np.array(tp)-slot['body_center_m']
        elif g=='door/upper':d[1]=1.70
        elif g=='door/lower':d[1]=-1.70
        return d
    for slot in old['slots']:
        x,y,z=slot['body_center_m'];home=[x,y,z];launch=[-30.5,-1,-3.5];lower=[-30.5,-1.2,-3.5]
        ss=[(-30.5,-1,0,home,home),(x,-1,0,home,home),(x,y,0,home,home),(x,y,1,home,home),(x,y,0,[x,y,-3.5],[x,y,-3.5]),(x,-1,0,[x,-1,-3.5],[x,-1,-3.5]),(-30.5,-1,0,launch,launch),(-30.5,-1.2,0,lower,launch),(-30.5,-1.2,0,lower,[-40,-1,-3.5])]
        for s0,s1 in zip(ss,ss[1:]):
            objects=[]
            for p in old['parts']:
                if p['group'] and 'bounds_body_m' in p:objects.append((p['name'] if 'name' in p else p['source_name'],np.array(p['bounds_body_m'])+olddelta(p,slot,s0),np.array(p['bounds_body_m'])+olddelta(p,slot,s1)))
            for other in old['slots']:
                b=np.array(other['drone_bounds_relative_m']);objects.append(('drone/'+other['id'],b+(s0[-1] if other['id']==slot['id'] else other['body_center_m']),b+(s1[-1] if other['id']==slot['id'] else other['body_center_m'])))
            for name,b0,b1 in objects:
                for rn,b in rb:
                    legacy_checks+=1
                    if overlap_interval(b0,b1,b,b):hits.append(['legacy/'+slot['id']+'/'+name,rn])
    # ALL-POSE domain proof, independent of the outbound segment list above.
    # Cartesian products deliberately include impossible simultaneous axis poses:
    # a clear superset covers door opening/closing, empty-tray return and the
    # entire exact reversed recovery route as well as every interpolation.
    receiver_domain=np.array([np.min([b[0] for _,b in rb],0),np.max([b[1] for _,b in rb],0)])
    all_pose_domains=[]
    for p in old['parts']:
        g=p['group']
        if not g:continue
        low=np.zeros(3);high=np.zeros(3)
        if g=='carriage_x':high[0]=12
        elif g=='lift_xy' or g.startswith('fork_stage'):
            high[0]=12;low[1]=-4.4;high[1]=4.4
            if g.startswith('fork_stage'):high[2]=old['kinematics'][g]['extension_translation_z_m'][1]
        elif g.startswith('tray/'):
            home=np.array(next(slot['body_center_m'] for slot in old['slots'] if slot['tray_group']==g))
            low=np.array([-30.5,-5.4,-3.5])-home;high=np.array([-18.5,3.4,3.5])-home
        elif g=='door/upper':high[1]=1.7
        elif g=='door/lower':low[1]=-1.7
        else:raise AssertionError('Unrecognized machinery domain '+g)
        box=np.array(p['bounds_body_m'])+np.array([low,high])
        all_pose_domains.append((p['source_name'],box))
    all_pose_domains += [('every_drone_internal_transport',np.array([[-33.0,-6.3,-6.5],[-16.0,4.3,6.5]])),('every_drone_external_throat',np.array([[-42.5,-1.9,-6.5],[-28.0,-.1,-.5]]))]
    all_pose_separation=[]
    for name,box in all_pose_domains:
        gap=np.maximum(box[0]-receiver_domain[1],receiver_domain[0]-box[1])
        axis=int(np.argmax(gap))
        assert gap[axis]>1e-5,('All-pose domain overlaps receiver',name,box.tolist())
        all_pose_separation.append({'part_or_domain':name,'body_bounds_m':box.tolist(),'separating_axis':axis,'minimum_gap_m':float(gap[axis])})
    # Base is assets/ships/pioneer; repository root is its third parent.
    layout_path=base.parent.parent.parent/'crates/arkline-protocol/src/drone_bay_layout.rs'
    assert hashlib.sha256(layout_path.read_bytes()).hexdigest()=='bd1c848d14a61cb3856fede8133bd4330813b191fca248b0080ddd4e36123ff2', 'Re-review Cartesian domains when canonical route code changes'
    all_pose_proof={'legacy_authoring_sha256':legacy_authoring_sha256,'method':'Conservative Cartesian axis domains; separation from union of every receiver pose. Covers BOTH directions, all door fractions and empty-return axes without relying on eight outbound samples.','route_source_sha256':hashlib.sha256(layout_path.read_bytes()).hexdigest(),'carriage_center_x_m':[-30.5,-18.5],'support_center_y_m':[-5.4,3.4],'fork_fraction':[0,1],'door_fraction':[0,1],'tray_centers_body_m':[[-30.5,-5.4,-3.5],[-18.5,3.4,3.5]],'drone_domain_split':'Internal route centerX>=-30.5; external-throat centerZ=-3.5,Y=-1. All current launch keyframes and exact reversed recovery stay in one of these closed domains.','domains':all_pose_separation,'domain_count':len(all_pose_separation),'minimum_separating_gap_m':min(v['minimum_gap_m'] for v in all_pose_separation)}
    # Negative controls exercise actual source and our interval detector.
    neg=[]
    for name,t in source:
        if 'door/' in name and sat(t,np.array([-37,-1.9,-6.5]),np.array([-34,-.1,-.5])).any():neg.append(name)
    assert neg and overlap_interval(np.zeros((2,3))+[[-1,-1,-1],[1,1,1]],np.zeros((2,3))+[[-1,-1,-1],[1,1,1]],np.array([[0,0,0],[2,2,2]]),np.array([[0,0,0],[2,2,2]]))
    full_drone=np.array([[-42.5,-1.9,2],[-37.5,-.1,8]])
    closed_socket_hits=[p['name'] for p in parts if overlap_interval(np.array(p['bounds_body_m'])+delta(p,1,1),np.array(p['bounds_body_m'])+delta(p,1,1),full_drone,full_drone)]
    assert any('coupling_head' in name for name in closed_socket_hits), 'Closed socket negative control must collide'
    out=dict(all_pose_legacy_route_separation=all_pose_proof,negative_closed_socket_hits=closed_socket_hits,schema='arkline.ore-receiver-clearance/v1',base_sha256=a['base_glb_sha256'],receiver_sha256=r['output']['sha256'],checks=checks,legacy_15_route_pair_checks=legacy_checks,hits=hits,hardware_hits=hardware_hits,explicit_sealed_hull_mount_contacts=mount_contacts,negative_closed_bay_hits=neg,expanded_module_bounds_m=[np.min([b[0] for _,b in rb],0).tolist(),np.max([b[1] for _,b in rb],0).tolist()],limitations=['Conservative continuous source/box proof only, not runtime world hazards','Socket carve requires matching independent procedural Mining drone proof','Hull contact allowed only named fixed mount pads/plate; no generic self-hull bypass'])
    return out
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--base',type=Path,required=True);p.add_argument('--study',type=Path,required=True);p.add_argument('--export',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=validate(a.base,a.study,a.export);a.output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k not in ['explicit_sealed_hull_mount_contacts']}));raise SystemExit(bool(r['hits'] or r['hardware_hits']))
