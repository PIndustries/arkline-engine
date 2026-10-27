#!/usr/bin/env python3
"""Independent byte-level checks of the additive receiver export."""
import hashlib,json,struct,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]/'assets/ships/pioneer'
ASSET=ROOT/'ore-receiver-v1'
class ReceiverTests(unittest.TestCase):
    def test_exact_source_hashes_and_unchanged_hull(self):
        r=json.loads((ASSET/'receipt.json').read_text());a=json.loads((ASSET/'authoring.json').read_text())
        for path,digest in [(ASSET/'ore-receiver.glb',r['output']['sha256']),(ASSET/'ore-receiver.blend',r['source']['sha256']),(ROOT/'pioneer.glb',r['base_glb_sha256'])]:self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),digest)
        self.assertEqual(r['base_registration'],12);self.assertEqual(r['source']['sha256'],a['output_sha256']);self.assertEqual(r['authoring_sha256'],hashlib.sha256((ASSET/'authoring.json').read_bytes()).hexdigest())
    def test_complete_disjoint_partition_and_real_glb_geometry(self):
        data=(ASSET/'ore-receiver.glb').read_bytes();r=json.loads((ASSET/'receipt.json').read_text())
        self.assertEqual(struct.unpack_from('<III',data),(0x46546c67,2,len(data)));n,kind=struct.unpack_from('<II',data,12);self.assertEqual(kind,0x4e4f534a);g=json.loads(data[20:20+n]);blen,bkind=struct.unpack_from('<II',data,20+n);self.assertEqual(bkind,0x004e4942);self.assertEqual(28+n+blen,len(data));binary=data[28+n:]
        self.assertEqual(g['nodes'],[{'mesh':0,'name':'Pioneer additive ore receiver body-space'}]);self.assertEqual(len(g['meshes']),1)
        def acc(i):
            a=g['accessors'][i];v=g['bufferViews'][a['bufferView']];dim={'SCALAR':1,'VEC3':3}[a['type']];fmt='<'+('f' if a['componentType']==5126 else 'I')*dim;self.assertLessEqual(v['byteOffset']+v['byteLength'],len(binary));return list(struct.iter_unpack(fmt,binary[v['byteOffset']:v['byteOffset']+v['byteLength']]))
        total=0
        for i,p in enumerate(g['meshes'][0]['primitives']):
            self.assertEqual(p['mode'],4);v=acc(p['attributes']['POSITION']);normal=acc(p['attributes']['NORMAL']);indices=[n[0] for n in acc(p['indices'])];self.assertEqual(len(v),len(normal));self.assertEqual(indices,list(range(len(v))))
            self.assertTrue(all(abs(sum(x*x for x in n)-1)<1e-5 for n in normal));end=0
            for row in sorted([x for x in r['source_parts'] if x['render_primitive']==i],key=lambda x:x['first_index']):
                self.assertEqual(row['first_index'],end);self.assertGreater(row['index_count'],0);self.assertEqual(row['index_count']%3,0);end+=row['index_count'];self.assertEqual(row['material'],g['materials'][p['material']]['name'])
            self.assertEqual(end,len(indices));total+=len(indices)//3
        self.assertEqual(total,r['output']['triangles']);self.assertEqual(len(set(x['name'] for x in r['source_parts'])),len(r['source_parts']))
        a=json.loads((ASSET/'authoring.json').read_text());declared={name:group for group,names in a['dynamic_groups'].items() for name in names};actual={x['name']:x['group'] for x in r['source_parts'] if x['group']};self.assertEqual(declared,actual);self.assertEqual(len(a['dynamic_groups']),5)
    def test_clearance_and_negative_controls_are_not_empty_checks(self):
        p=json.loads((ASSET/'clearance.json').read_text());self.assertEqual(p['hits'],[]);self.assertEqual(p['hardware_hits'],[]);self.assertGreater(p['checks'],100000);self.assertGreater(p['legacy_15_route_pair_checks'],400000);self.assertTrue(p['negative_closed_socket_hits']);self.assertTrue(p['negative_closed_bay_hits']);self.assertTrue(p['explicit_sealed_hull_mount_contacts'])
    def test_all_pose_domains_cover_empty_return_door_and_reverse_motion(self):
        p=json.loads((ASSET/'clearance.json').read_text())['all_pose_legacy_route_separation']
        layout=ROOT.parent.parent.parent/'crates/arkline-protocol/src/drone_bay_layout.rs'
        self.assertEqual(p['route_source_sha256'],hashlib.sha256(layout.read_bytes()).hexdigest())
        self.assertEqual(p['legacy_authoring_sha256'],hashlib.sha256((ROOT/'drone-bay-authoring.json').read_bytes()).hexdigest())
        self.assertEqual(p['domain_count'],73)
        self.assertGreater(p['minimum_separating_gap_m'],2.8)
        self.assertEqual(p['door_fraction'],[0,1]);self.assertEqual(p['fork_fraction'],[0,1])
        self.assertTrue(all(x['minimum_gap_m']>0 for x in p['domains']))
if __name__=='__main__':unittest.main()
