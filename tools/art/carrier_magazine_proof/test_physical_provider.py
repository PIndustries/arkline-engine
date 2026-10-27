"""Fast source/extraction negatives; geometric route acceptance is separate."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import numpy as np
from physical_provider.source import occupied_cells, read_pinned, glb_primitives, measured_logical_enclosures
from physical_provider.coverage import triangle_union_covers


class SourceTests(unittest.TestCase):
    def test_actual_bytes_not_claimed_hash(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);p=root/'source.json';p.write_bytes(b'original')
            asset={'path':p.name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
            self.assertEqual(read_pinned(root,asset),p)
            p.write_bytes(b'modified')
            with self.assertRaises(ValueError):read_pinned(root,asset)

    def test_hollow_cells_preserve_material_without_filling_bore(self):
        p={'outer_boxes':[[[0,0,0],[3,3,3]]],'void_boxes':[[[1,1,-1],[2,2,4]]]}
        cells=np.array(occupied_cells(p));volume=np.prod(cells[:,1]-cells[:,0],axis=1).sum()
        self.assertEqual(volume,24.)
        for a in cells:
            self.assertFalse(np.all(np.array([1.5,1.5,1.5])>a[0])and np.all(np.array([1.5,1.5,1.5])<a[1]))
        for i,a in enumerate(cells):
            for b in cells[i+1:]:self.assertFalse(np.all(a[0]<b[1])and np.all(b[0]<a[1]))

    def test_disjoint_and_overlapping_box_union_counted_once(self):
        p={'outer_boxes':[[[0,0,0],[2,1,1]],[[1,0,0],[3,1,1]],[[5,0,0],[6,1,1]]],'void_boxes':[]}
        cells=np.array(occupied_cells(p));self.assertEqual(np.prod(cells[:,1]-cells[:,0],axis=1).sum(),4.)

    def test_empty_material_is_not_admitted(self):
        with self.assertRaises(ValueError):occupied_cells({'outer_boxes':[[[0,0,0],[1,1,1]]],'void_boxes':[[[-1,-1,-1],[2,2,2]]]})

    def test_malformed_glb_rejects(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'bad.glb';p.write_bytes(b'not a GLB document long enough to parse')
            with self.assertRaises(ValueError):glb_primitives(p)

    def test_triangle_vertices_do_not_prove_union_coverage(self):
        triangle=[[0,0,0],[3,0,0],[0,3,0]]
        boxes=[[[-.1,-.1,-.1],[.1,.1,.1]],[[2.9,-.1,-.1],[3.1,.1,.1]],[[-.1,2.9,-.1],[.1,3.1,.1]]]
        self.assertFalse(triangle_union_covers(triangle,boxes))
        self.assertTrue(triangle_union_covers([[0,0,0],[1,0,0],[0,1,0]],[[[0,0,0],[.5,1,1]],[[.5,0,0],[1,1,1]]]))

    def test_measured_export_reconciliation_stays_same_owner_and_rejects_large_mismatch(self):
        doc={'parts':[{'group':'root','semantic_id':'part','render_semantic_id':'render','bounds_body_m':[[0,0,0],[1,1,1]]}]}
        receipt={'source_parts':[{'name':'render','render_primitive':0,'first_index':0,'index_count':3}]}
        tri=[np.array([[[0,0,0],[1.000002,0,0],[0,1,0]]])]
        bounds,witnesses=measured_logical_enclosures(doc,receipt,tri)
        self.assertGreaterEqual(bounds['part'][1][0],1.000002)
        self.assertAlmostEqual(witnesses[0]['measured_enclosure_expansion_m'],.000002)
        tri[0][0,1,0]=1.01
        with self.assertRaises(ValueError):measured_logical_enclosures(doc,receipt,tri)

    def test_new_typed_service_has_both_valves_before_coupled(self):
        fixture=Path(__file__).parents[3]/'crates/arkline-protocol/src/carrier_inventory_geometry/stationary_v1704.json'
        d=json.loads(fixture.read_text());frames=d['services'][0]['keyframes'];refs={m['semantic_id']:m['reference_pose']for m in d['definition']['mechanisms']}
        self.assertEqual(len(frames),15)
        coupled=[]
        for i,t in enumerate(frames):
            f=t['frame'];poses=dict(f['machinery']);ship=[]
            for sign in [-1,1]:
                g=f'b00/fluid/valve/{sign}';ship.append((poses[g]['position_m'][1]-refs[g]['position_m'][1])*sign)
            if f['milestones']['ore_coupled']:
                coupled.append(i);self.assertEqual(t['drone_inlet_travel_mm'],250)
                self.assertTrue(all(abs(x-.094)<1e-12 for x in ship))
            if i>=10:self.assertEqual(t['drone_inlet_travel_mm'],0);self.assertTrue(all(x==0 for x in ship))
        self.assertEqual(coupled,[7]);self.assertEqual(frames[9]['frame']['drone_pose'],frames[5]['frame']['drone_pose'])
        self.assertEqual(frames[9]['frame']['machinery'],frames[5]['frame']['machinery'])


if __name__=='__main__':unittest.main()
