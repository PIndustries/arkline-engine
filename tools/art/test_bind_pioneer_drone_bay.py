#!/usr/bin/env python3
import copy,hashlib,json,tempfile,unittest
from pathlib import Path
from bind_pioneer_drone_bay import bind
class BindingTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name);self.raw=b'bounded fixture bytes';h=hashlib.sha256(self.raw).hexdigest()
  self.a={'output_sha256':'blend','dynamic_groups':{'door/upper':['upper']},'kinematics':{'door/upper':{'translation_y_m':[0,1.7]}},'slots':[],'floor_profile_xy_m':[],'launch_release_body_m':[-30.5,-1,-3.5],'release_tray_center_m':[-30.5,-1.2,-3.5],'release_disengage_y_m':-.2,'release_contract':'lower support before departure','aperture_body_m':[[-36,-2.6,-7.5],[-35.25,.6,.5]]}
  self.r={'source':{'sha256':'blend'},'output':{'sha256':h,'triangles':2},'source_parts':[{'name':'static','source_id':'part:static','render_primitive':0,'first_index':0,'index_count':3},{'name':'upper','source_id':'part:upper','render_primitive':0,'first_index':3,'index_count':3}]}
 def tearDown(self):self.tmp.cleanup()
 def run_bind(self):
  ap=self.root/'authoring.json';rp=self.root/'receipt.json';gp=self.root/'fixture.glb';ap.write_text(json.dumps(self.a));rp.write_text(json.dumps(self.r));gp.write_bytes(self.raw);return bind(ap,rp,gp)
 def test_exact_partition_and_pose(self):
  r=self.run_bind();self.assertEqual(r['dynamic_triangle_count'],1);self.assertEqual(r['static_triangle_count'],1);self.assertEqual(r['dynamic_groups'][0]['reference_pose_body_m']['rotation_xyzw'],[0,0,0,1]);self.assertEqual(r['registration_revision'],12)
 def test_duplicate_dynamic_membership_rejected(self):
  self.a['dynamic_groups']['door/lower']=['upper']
  with self.assertRaises(AssertionError):self.run_bind()
 def test_wrong_source_rejected(self):
  self.a['output_sha256']='wrong'
  with self.assertRaises(AssertionError):self.run_bind()
 def test_wrong_glb_rejected(self):
  self.r['output']['sha256']='wrong'
  with self.assertRaises(AssertionError):self.run_bind()
 def test_triangle_gap_rejected(self):
  self.r['source_parts'][1]['first_index']=6
  with self.assertRaises(AssertionError):self.run_bind()
 def test_triangle_overlap_rejected(self):
  self.r['source_parts'][1]['first_index']=0
  with self.assertRaises(AssertionError):self.run_bind()
 def test_missing_dynamic_part_rejected(self):
  self.a['dynamic_groups']['door/upper']=['absent']
  with self.assertRaises(AssertionError):self.run_bind()
if __name__=='__main__':unittest.main()
