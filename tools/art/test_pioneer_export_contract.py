#!/usr/bin/env python3
"""Adversarial stdlib checks plus optional official-bpy saved-master journeys.

python3 tools/art/test_pioneer_export_contract.py
PIONEER_BPY_PYTHON=/path/to/bpy-python python3 tools/art/test_pioneer_export_contract.py
"""
import copy
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import pioneer_export_contract as contract
import export_pioneer as exporter

HERE = Path(__file__).resolve().parent
BPy = os.environ.get('PIONEER_BPY_PYTHON')


def material(name):
    return {'name': name, 'alphaMode': 'OPAQUE', 'doubleSided': True,
            'pbrMetallicRoughness': {'baseColorFactor': [.1, .2, .3, 1.],
                                    'metallicFactor': .2, 'roughnessFactor': .4},
            'emissiveFactor': [0., 0., 0.],
            'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 0.}}}


def parts_fixture():
    return [{'name': 'Part '+str(i), 'material': name,
             'triangles': [[((float(i), 0., 0.), (0., 0., 1.)),
                            ((float(i)+1., 0., 0.), (0., 0., 1.)),
                            ((float(i), 1., 0.), (0., 0., 1.))]]}
            for i, name in enumerate(contract.MATERIAL_NAMES)]


class ContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parts = parts_fixture()
        cls.materials = [material(name) for name in contract.MATERIAL_NAMES]
        cls.glb, cls.mapping, cls.dropped, _ = contract.serialize(cls.parts, cls.materials, 'reject')

    def test_palette_remap_preserves_bin_and_is_idempotent(self):
        doc, binary = contract.unpack_glb(self.glb)
        order = [8, 3, 1, 7, 0, 6, 2, 5, 4]
        doc['materials'] = [doc['materials'][old] for old in order]
        for primitive in doc['meshes'][0]['primitives']:
            primitive['material'] = order.index(primitive['material'])
        scrambled = contract.pack_glb(doc, binary)
        canonical = contract.canonicalize_glb(scrambled)
        self.assertEqual(contract.unpack_glb(canonical)[1], binary)
        self.assertEqual(contract.canonicalize_glb(canonical), canonical)
        self.assertEqual(canonical, self.glb)

    def test_palette_rejects_duplicate_unknown_and_missing_names(self):
        for edit in ('duplicate', 'unknown', 'missing'):
            with self.subTest(edit=edit):
                doc, binary = contract.unpack_glb(self.glb)
                if edit == 'duplicate': doc['materials'][1]['name'] = doc['materials'][0]['name']
                elif edit == 'unknown': doc['materials'][1]['name'] = 'Almost titanium'
                else: del doc['materials'][1]['name']
                with self.assertRaises(contract.ExportError): contract.canonicalize_glb(contract.pack_glb(doc, binary))

    def test_material_references_reject_bool_negative_and_out_of_range(self):
        for value in (True, -1, 9):
            with self.subTest(value=value):
                doc, binary = contract.unpack_glb(self.glb)
                doc['meshes'][0]['primitives'][0]['material'] = value
                with self.assertRaises(contract.ExportError): contract.canonicalize_glb(contract.pack_glb(doc, binary))

    def test_glb_resource_and_accessor_adversaries_fail_closed(self):
        mutations = [
            lambda d: d['buffers'][0].update(uri='file:///secret.bin'),
            lambda d: d.update(images=[{'uri': 'https://invalid/texture'}]),
            lambda d: d.update(animations=[{}]),
            lambda d: d.update(skins=[{}]),
            lambda d: d.update(extensionsUsed=['KHR_draco_mesh_compression']),
            lambda d: d['nodes'][0].update(scale=[-1, 1, 1]),
            lambda d: d['accessors'][0].update(sparse={'count': 1}),
            lambda d: d['accessors'][0].update(count=600001),
            lambda d: d['bufferViews'][0].update(byteOffset=2**32),
            lambda d: d['bufferViews'][0].update(byteLength=0),
            lambda d: d['accessors'][0].pop('min'),
            lambda d: d['accessors'][0].update(max=[1000.,1000.,1000.]),
            lambda d: d['accessors'][1].update(min=[-1000.,-1000.,-1000.]),
            lambda d: d['materials'][0]['pbrMetallicRoughness'].update(baseColorTexture={'index': 0}),
            lambda d: d['materials'][0].update(alphaMode='BLEND'),
            lambda d: d['materials'][0]['extensions']['KHR_materials_emissive_strength'].update(emissiveStrength=65),
            lambda d: d['meshes'][0]['primitives'][0].update(mode=5),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                doc, binary = contract.unpack_glb(self.glb); mutation(doc)
                with self.assertRaises(contract.ExportError): contract.validate_glb(contract.pack_glb(doc, binary))

    def test_binary_index_nonfinite_and_zero_normal_rejected(self):
        for kind in ('index', 'position', 'normal'):
            with self.subTest(kind=kind):
                doc, binary = contract.unpack_glb(self.glb); binary = bytearray(binary)
                primitive = doc['meshes'][0]['primitives'][0]
                accessor = primitive['indices'] if kind == 'index' else primitive['attributes']['POSITION' if kind == 'position' else 'NORMAL']
                offset = doc['bufferViews'][doc['accessors'][accessor]['bufferView']]['byteOffset']
                binary[offset:offset+4] = struct.pack('<I' if kind == 'index' else '<f', 999 if kind == 'index' else (math.nan if kind == 'position' else 0.))
                if kind == 'normal': binary[offset:offset+12] = b'\0'*12
                with self.assertRaises(contract.ExportError): contract.validate_glb(contract.pack_glb(doc, bytes(binary)))

    def test_bad_chunk_headers_duplicate_json_keys_and_oversize_rejected(self):
        bad = [b'not glb', self.glb[:-1], self.glb+b'\0'*4, b'\0'*(contract.LIMITS['glb_bytes']+1)]
        for payload in bad:
            with self.assertRaises(contract.ExportError): contract.unpack_glb(payload)
        text = b'{"asset":{},"asset":{}}'; text += b' '*(-len(text)%4)
        blob = struct.pack('<IIIII', 0x46546c67, 2, 28+len(text), len(text), 0x4e4f534a)+text+struct.pack('<II', 0, 0x004e4942)
        with self.assertRaises(contract.ExportError): contract.unpack_glb(blob)

    def test_runtime_gate_uses_scaled_float32_units(self):
        p = [(0., 0., 0.), (.0005, 0., 0.), (0., .0005, 0.)]
        self.assertGreater(.0005**4, 1e-20)  # Old body-unit nearzero test would keep it.
        self.assertLessEqual(contract.runtime_cross_squared(p), contract.f32(1e-20))
        self.assertAlmostEqual(contract.runtime_cross_squared([(0.,0.,0.), (1.,0.,0.), (0.,1.,0.)]), 1e-8, places=14)

    def test_final_cyclic_order_is_used_for_threshold_policy(self):
        positions=[(-.00555469561368227,.0006843843730166554,.0007778552826493979),
                   (-.006994819268584251,.0019770986400544643,.00008953951328294352),
                   (-.0062635852955281734,.001624578726477921,-.00009077871800400317)]
        self.assertGreater(contract.runtime_cross_squared(positions), contract.f32(1e-20))
        parts=copy.deepcopy(self.parts)
        parts[0]['triangles'].append([(p,(0.,0.,1.)) for p in positions])
        output,_,policy,_=contract.serialize(parts,self.materials,'drop')
        self.assertEqual(output,self.glb);self.assertEqual(policy['dropped_triangles'],1)
        with self.assertRaises(contract.ExportError):contract.serialize(parts,self.materials,'reject')

    def test_degenerate_policy_explicit_drops_without_repair_or_requires_reject(self):
        parts = copy.deepcopy(self.parts)
        parts[0]['triangles'].append([((0.,0.,0.), (0.,0.,0.))]*3)
        with self.assertRaises(contract.ExportError): contract.serialize(parts, self.materials, 'reject')
        exported, _, policy, _ = contract.serialize(parts, self.materials, 'drop')
        self.assertEqual(exported, self.glb)
        self.assertEqual(policy['dropped_triangles'], 1)
        with self.assertRaises(contract.ExportError): contract.serialize(parts, self.materials, 'unspecified')

    def test_sorting_is_deterministic_and_preserves_normals_and_winding(self):
        parts = copy.deepcopy(self.parts)
        triangle = parts[0]['triangles'][0]
        triangle[0] = (triangle[0][0], (.6, 0., .8))
        triangle[1] = (triangle[1][0], (0., .8, .6))
        one = contract.serialize(parts, self.materials, 'reject')[0]
        parts.reverse()
        for part in parts:
            part['triangles'] = [t[1:]+t[:1] for t in reversed(part['triangles'])]
        two = contract.serialize(parts, list(reversed(self.materials)), 'reject')[0]
        self.assertEqual(one, two)
        doc, binary = contract.unpack_glb(one)
        views = doc['bufferViews']; accessors = doc['accessors']; primitive = doc['meshes'][0]['primitives'][0]
        def array(attr):
            v = views[accessors[primitive['attributes'][attr]]['bufferView']]
            return list(struct.iter_unpack('<fff', binary[v['byteOffset']:v['byteOffset']+v['byteLength']]))
        correspondence = dict(zip(array('POSITION'), array('NORMAL')))
        self.assertAlmostEqual(correspondence[(0.,0.,0.)][0], .6, places=6)
        self.assertAlmostEqual(correspondence[(1.,0.,0.)][1], .8, places=6)
        p = array('POSITION'); cross = (p[1][0]-p[0][0])*(p[2][1]-p[0][1])-(p[1][1]-p[0][1])*(p[2][0]-p[0][0])
        self.assertGreater(cross, 0.)

    def test_nonuniform_inverse_transpose_and_negative_singular_rejection(self):
        matrix = [[2.,0.,0.,0.],[0.,3.,0.,0.],[0.,0.,4.,0.],[0.,0.,0.,1.]]
        normal = contract.transform_normal(contract.affine_and_normal_matrix(matrix), (1.,1.,1.))
        expected = (.5, .25, -1/3); length = math.sqrt(sum(x*x for x in expected))
        for a, b in zip(normal, expected): self.assertAlmostEqual(a, b/length, places=6)
        for scale in (-1., 0., 1e-15):
            bad = copy.deepcopy(matrix); bad[0][0] = scale
            with self.assertRaises(contract.ExportError): contract.affine_and_normal_matrix(bad)

    def test_budgets_empty_material_geometry_and_duplicate_names_rejected(self):
        with self.assertRaises(contract.ExportError): contract.serialize(self.parts[:-1], self.materials, 'reject')
        bad = copy.deepcopy(self.parts); bad[1]['name'] = bad[0]['name']
        with self.assertRaises(contract.ExportError): contract.serialize(bad, self.materials, 'reject')
        original = contract.LIMITS['triangles']
        try:
            contract.LIMITS['triangles'] = 8
            with self.assertRaises(contract.ExportError): contract.serialize(self.parts, self.materials, 'reject')
        finally: contract.LIMITS['triangles'] = original

    def test_generator_guard_runs_before_bpy_or_source_mutation(self):
        root = HERE.parents[1]
        assets = root/'assets/ships/pioneer'
        before = {name: contract.file_hash(assets/name) for name in ('pioneer.blend','pioneer.glb','pioneer-hull.png')}
        result = subprocess.run([sys.executable, str(HERE/'build_pioneer.py')], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Refusing to overwrite existing Pioneer assets', result.stderr)
        self.assertNotIn('No module named', result.stderr)
        self.assertEqual(before, {name: contract.file_hash(assets/name) for name in before})

    def test_worker_cannot_bypass_new_directory_boundary(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp); source = directory/'fixture.blend'; source.write_bytes(b'not a blend')
            with self.assertRaises(contract.ExportError):
                exporter.worker(source, contract.file_hash(source), directory, 'drop', None)
            self.assertEqual(sorted(p.name for p in directory.iterdir()), ['fixture.blend'])

    def test_launcher_refuses_existing_or_symlink_output_without_touching_source(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp); source = base/'fixture.blend'; source.write_bytes(b'not a blend')
            output = base/'existing'; output.mkdir(); (output/'sentinel').write_text('keep')
            link = base/'linked'; link.symlink_to(output, target_is_directory=True)
            for target in (output, link):
                with self.assertRaises(contract.ExportError): exporter.run_export(source, target, 'drop', bpy_python='/bin/false')
            self.assertEqual(source.read_bytes(), b'not a blend'); self.assertEqual((output/'sentinel').read_text(), 'keep')


def fixture_worker(directory, variant):
    import bpy
    from mathutils import Matrix
    bpy.ops.wm.read_factory_settings(use_empty=True)
    mats = []
    for name in contract.MATERIAL_NAMES:
        mat = bpy.data.materials.new(name); mat.use_nodes = True
        node = mat.node_tree.nodes.get('Principled BSDF')
        node.inputs['Base Color'].default_value = (.1, .2, .3, 1.)
        if name == 'Drive emission':
            node.inputs['Emission Color'].default_value = (.1, .8, .2, 1.)
            node.inputs['Emission Strength'].default_value = 4.
        mats.append(mat)
    def mesh(name, positions, faces, mat):
        data = bpy.data.meshes.new(name+' mesh'); data.from_pydata([(x,-z,y) for x,y,z in positions], [], faces); data.update()
        obj = bpy.data.objects.new(name, data); bpy.context.collection.objects.link(obj); data.materials.append(mat)
        return obj
    bounds = mesh('Envelope', [(-36,-9,-19), (36,9,-19), (0,0,19)], [(0,1,2)], mats[0])
    bpy.ops.mesh.primitive_cube_add(size=2, location=(-5,0,0)); weighted = bpy.context.object; weighted.name='Weighted nonuniform'
    weighted.data.materials.append(mats[1]); weighted.matrix_world = Matrix(((2,.2,0,-5),(0,3,0,0),(0,0,.5,0),(0,0,0,1)))
    bevel=weighted.modifiers.new('Live bevel','BEVEL'); bevel.width=.15; bevel.segments=2
    weighted.modifiers.new('Weighted normals','WEIGHTED_NORMAL')
    custom = mesh('Custom corners', [(0,0,0),(1,0,0),(0,1,0)], [(0,1,2)], mats[2]); custom.data.polygons[0].use_smooth=True
    custom.data.normals_split_custom_set([(.6,0,.8),(0,.8,.6),(.3,.4,math.sqrt(.75))])
    custom.matrix_world = Matrix(((2,.3,0,0),(0,3,0,0),(0,0,.5,0),(0,0,0,1)))
    others = [mesh('Part '+str(i), [(i,0,0),(i+.5,0,0),(i,.5,0)], [(0,1,2)], mats[i]) for i in range(3,9)]
    if variant == 'constraint': weighted.constraints.new('COPY_LOCATION')
    elif variant == 'driver': weighted.driver_add('location', 0).driver.expression='1'
    elif variant == 'animation': weighted.keyframe_insert(data_path='location', frame=1)
    elif variant == 'negative': weighted.scale.x=-1
    elif variant == 'singular': weighted.scale.x=0
    elif variant == 'parent': custom.parent=weighted
    elif variant == 'modifier': weighted.modifiers.new('Unsupported smooth','SMOOTH')
    elif variant == 'disabled_modifier': bevel.show_render=False
    elif variant == 'shape': custom.shape_key_add(name='Basis')
    elif variant == 'shader': mats[0].node_tree.nodes.new('ShaderNodeTexNoise')
    elif variant == 'shader_value': mats[0].node_tree.nodes.get('Principled BSDF').inputs['Coat Weight'].default_value=.5
    elif variant == 'shader_driver': mats[0].node_tree.nodes.get('Principled BSDF').inputs['Roughness'].driver_add('default_value')
    elif variant == 'world_driver':
        world=bpy.data.worlds.new('Driven world');world.use_nodes=True;bpy.context.scene.world=world
        world.node_tree.nodes.get('Background').inputs['Strength'].driver_add('default_value')
    elif variant == 'image': bpy.data.images.new('Unsupported image', width=1, height=1).use_fake_user=True
    elif variant == 'material_slots': weighted.data.materials.append(mats[2])
    elif variant == 'hidden': custom.hide_render=True
    elif variant == 'missing_material': others[-1].data.materials.clear()
    elif variant == 'extra_material': bpy.data.materials.new('Unknown tenth').use_fake_user=True
    elif variant == 'wrong_envelope': bounds.scale=(.5,.5,.5)
    elif variant == 'units': bpy.context.scene.unit_settings.scale_length=.01
    elif variant == 'degenerate_envelope':
        bounds.data.clear_geometry()
        points=[(-36,-9,-19),(-36,-9,-19),(-36,-9,-19),(36,9,19),(36,9,19),(36,9,19)]
        bounds.data.from_pydata([(x,-z,y) for x,y,z in points],[],[(0,1,2),(3,4,5)])
    elif variant == 'degenerate':
        mesh('Tiny face', [(0,0,0),(.0005,0,0),(0,.0005,0)], [(0,1,2)], mats[0])
    elif variant == 'linked':
        library=directory/'library.blend'
        bpy.data.libraries.write(str(library), {weighted}, fake_user=True)
        with bpy.data.libraries.load(str(library), link=True) as (available, loaded): loaded.objects=[available.objects[0]]
        bpy.context.scene.collection.objects.link(loaded.objects[0])
    source=directory/'fixture.blend'
    bpy.ops.wm.save_as_mainfile(filepath=str(source))
    # Independent expected normals from evaluated loop-corner data, retaining
    # original triangle order and using mathutils inverse transpose here.
    expected={}
    graph=bpy.context.evaluated_depsgraph_get()
    for obj in ((weighted, custom) if variant in {'valid', 'degenerate'} else ()):
        evaluated=obj.evaluated_get(graph); data=evaluated.to_mesh(preserve_all_data_layers=True,depsgraph=graph)
        try:
            data.calc_loop_triangles(); normal_matrix=evaluated.matrix_world.to_3x3().inverted().transposed()
            values=[]
            for triangle in data.loop_triangles:
                for loop in triangle.loops:
                    p=evaluated.matrix_world@data.vertices[data.loops[loop].vertex_index].co
                    n=(normal_matrix@data.corner_normals[loop].vector).normalized()
                    values.append({'p':[p.x,p.z,-p.y], 'n':[n.x,n.z,-n.y]})
            expected[obj.name]=values
        finally:evaluated.to_mesh_clear()
    (directory/'expected.json').write_text(json.dumps(expected))


@unittest.skipUnless(BPy, 'set PIONEER_BPY_PYTHON for official Blender 5.0.1 integration checks')
class BpyTests(unittest.TestCase):
    def make_source(self, directory, variant='valid'):
        result=subprocess.run([BPy, str(Path(__file__).resolve()), '--fixture-worker', str(directory), variant],
                              capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
        return directory/'fixture.blend'

    def invoke(self, source, output, policy='drop'):
        return subprocess.run([sys.executable, str(HERE/'export_pioneer.py'), '--source', str(source),
            '--output-dir', str(output), '--degenerate-policy', policy, '--bpy-python', BPy],
            capture_output=True, text=True, timeout=60)

    def test_saved_weighted_custom_sheared_normals_source_copy_mapping_and_determinism(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp); source=self.make_source(base); source.chmod(0o444); before=source.read_bytes()
            first,second=base/'one',base/'two'
            for output in (first,second):
                result=self.invoke(source, output, 'reject')
                details=(output/'worker.log').read_text() if (output/'worker.log').exists() else ''
                self.assertEqual(result.returncode,0,result.stdout+result.stderr+details)
            self.assertEqual(source.read_bytes(), before)
            self.assertEqual((first/source.name).read_bytes(), before)
            self.assertEqual((first/'pioneer.glb').read_bytes(),(second/'pioneer.glb').read_bytes())
            self.assertEqual((first/'receipt.json').read_bytes(),(second/'receipt.json').read_bytes())
            receipt=json.loads((first/'receipt.json').read_text())
            self.assertEqual(len(receipt['source_parts']),9); self.assertFalse(receipt['evaluation']['autoexec'])
            self.assertEqual([m['name'] for m in receipt['material_identity']],list(contract.MATERIAL_NAMES))
            doc,binary=contract.unpack_glb((first/'pioneer.glb').read_bytes()); expected=json.loads((base/'expected.json').read_text())
            for part in receipt['source_parts']:
                if part['name'] not in expected:continue
                primitive=doc['meshes'][0]['primitives'][part['render_primitive']]
                def rows(attr):
                    a=doc['accessors'][primitive['attributes'][attr]];v=doc['bufferViews'][a['bufferView']]
                    return list(struct.iter_unpack('<fff',binary[v['byteOffset']:v['byteOffset']+v['byteLength']]))
                positions,normals=rows('POSITION'),rows('NORMAL')
                start,count=part['first_index'],part['index_count']
                candidates=expected[part['name']]
                for position,normal in zip(positions[start:start+count],normals[start:start+count]):
                    found=[corner for corner in candidates if math.dist(position,corner['p'])<1e-5]
                    self.assertTrue(found)
                    self.assertGreater(max(sum(x*y for x,y in zip(normal,corner['n'])) for corner in found),.999999)
            again=self.invoke(source,first)
            self.assertNotEqual(again.returncode,0);self.assertIn('already exists',again.stderr)
            self.assertEqual(source.read_bytes(),before)

    def test_unsupported_static_sources_fail_without_source_mutation_or_glb(self):
        variants=('constraint','driver','animation','negative','singular','parent','modifier','disabled_modifier',
                  'shape','shader','shader_value','shader_driver','world_driver','image','material_slots','hidden','missing_material','extra_material','wrong_envelope','units','degenerate_envelope','linked')
        with tempfile.TemporaryDirectory() as temp:
            for variant in variants:
                with self.subTest(variant=variant):
                    base=Path(temp)/variant;base.mkdir();source=self.make_source(base,variant);before=source.read_bytes()
                    output=base/'rejected';result=self.invoke(source,output)
                    self.assertNotEqual(result.returncode,0)
                    self.assertEqual(source.read_bytes(),before)
                    self.assertFalse((output/'pioneer.glb').exists());self.assertFalse((output/'receipt.json').exists())

    def test_saved_tiny_face_drop_and_reject_are_explicit(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);source=self.make_source(base,'degenerate');before=source.read_bytes()
            rejected=self.invoke(source,base/'reject','reject');self.assertNotEqual(rejected.returncode,0)
            dropped=self.invoke(source,base/'drop','drop')
            self.assertEqual(dropped.returncode,0,dropped.stderr)
            receipt=json.loads((base/'drop/receipt.json').read_text())
            self.assertEqual(receipt['degeneracy']['dropped_triangles'],1)
            self.assertEqual(source.read_bytes(),before)


if __name__ == '__main__':
    if '--fixture-worker' in sys.argv:
        index=sys.argv.index('--fixture-worker');fixture_worker(Path(sys.argv[index+1]),sys.argv[index+2])
    else:
        unittest.main(verbosity=2)
