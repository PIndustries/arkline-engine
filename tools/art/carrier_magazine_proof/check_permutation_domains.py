"""Actual cassette identity permutation and disjoint opposite-side working domains."""
import argparse,json,hashlib,itertools
from pathlib import Path
import numpy as np
from candidate_routes import stage,bay,translation
p=argparse.ArgumentParser();p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args();d=json.loads(a.manifest.read_text());fail=[]
normalized=[]
for c in d['cassettes']:
 cid=c['cassette_id'];center=np.array(c['body_center_m']);sign=-1 if c['side']=='port' else 1;axes=np.array([[0,0,sign],[0,1,0],[-sign,0,0]])
 rows=[]
 for part in d['parts']:
  if not (part['group'] or '').startswith(f'c{cid:02d}/'):continue
  points=(np.array(list(itertools.product(*zip(*part['bounds_body_m']))))-center)@axes.T
  rows.append((part['semantic_id'].split('/',2)[2],part['group'].split('/',1)[1],part['material'],np.round(points.min(0),8).tolist(),np.round(points.max(0),8).tolist()))
 normalized.append(sorted(rows))
for i,rows in enumerate(normalized):
 if rows!=normalized[0]:fail.append({'cassette':i,'reason':'non-equivalent physical cassette geometry'})
domains=[[],[]]
for side in range(2):
 for source in range(side*5,side*5+5):
  for dest in range(side*5,side*5+5):
   frames=stage(d,source,dest)
   for f in frames:
    for part in d['parts']:
     g=part['group']
     if g not in f['owned_groups']:continue
     domains[side].append(np.array(part['bounds_body_m'])+translation(d,g,f['state']))
    pos=np.array(f['state']['cassettes'][str(source+10)]);domains[side].append(np.array([pos+[-3,-.9,-2.5],pos+[3,.9,2.5]]))
  for f in bay(d,source):
   for part in d['parts']:
    g=part['group']
    if g in f['owned_groups']:domains[side].append(np.array(part['bounds_body_m'])+translation(d,g,f['state']))
   pos=np.array(f['drone_position']);domains[side].append(np.array([pos+[-3,-.9,-2.5],pos+[3,.9,2.5]]))
bounds=[[np.min(np.array(v)[:,0],0).tolist(),np.max(np.array(v)[:,1],0).tolist()] for v in domains]
if bounds[0][1][2]>=bounds[1][0][2]:fail.append({'reason':'opposite-side conservative route domains overlap'})
r={'scope':'all20 cassette local source boxes/materials/group suffixes equivalent under same-side ID permutation; opposite-side full Stage/Launch hardware/body unions disjoint for arbitrary concurrent timing; same-side loader remains serialized','manifest_sha256':hashlib.sha256(a.manifest.read_bytes()).hexdigest(),'route_sha256':hashlib.sha256(Path(__file__).with_name('candidate_routes.py').read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'cassette_count':20,'part_count_each':len(normalized[0]),'side_domains_m':bounds,'failures':fail};a.output.write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2));raise SystemExit(bool(fail))
