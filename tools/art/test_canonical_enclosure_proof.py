"""Portable export checks; authoritative collision checks live in the Rust example."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import canonical_enclosure_proof as canonical
import forward_bay_proof as original


class CanonicalEnclosureTests(unittest.TestCase):
    def setUp(self):
        self.study = Path(__file__).resolve().parents[2] / 'assets/studies/pioneer-canonical-enclosure-proof'
        self.manifest = json.loads((self.study / 'manifest.json').read_text())
        self.source = self.manifest['canonical_fit']

    def test_export_is_deterministic_and_uses_original_palette(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            with contextlib.redirect_stdout(io.StringIO()):
                canonical.write(self.source, out)
            for name in ('forward-bay-proof.glb', 'manifest.json'):
                self.assertEqual((out / name).read_bytes(), (self.study / name).read_bytes())
            # The palette is shared verbatim by the exporter, not a new material recipe.
            original_out = out / 'original'
            with contextlib.redirect_stdout(io.StringIO()):
                original.write(original_out)
            def materials(path):
                data = path.read_bytes()
                size = int.from_bytes(data[12:16], 'little')
                return json.loads(data[20:20+size])['materials']
            self.assertEqual(materials(out / 'forward-bay-proof.glb'),
                             materials(original_out / 'forward-bay-proof.glb'))

    def test_export_preserves_oriented_mounts_and_registered_floor(self):
        panels = {p['id']: p for p in self.source['panels']}
        components = {p['id']: p for p in self.manifest['components']}
        for identity in ('cockpit/aft-crosswalk', 'cockpit/seat-floor'):
            self.assertAlmostEqual(components[identity]['bounds_max'][1], self.source['deck_y'])
        self.assertEqual(self.manifest['instrument'],
                         dict(self.source['instruments'][2], canvas=[560, 380]))
        self.assertEqual([m['slot'] for m in self.source['instruments']], [1, 2, 3, 4, 5])
        for mesh in canonical.meshes(self.source, merge=False):
            panel = panels[mesh.identity]
            for vertex in mesh.positions:
                delta = original.sub(vertex, panel['center'])
                for axis, half in zip(panel['axes'], panel['half']):
                    self.assertAlmostEqual(abs(original.dot(delta, axis)), half)

    def test_triangles_are_finite_nondegenerate_and_outward(self):
        import math
        for mesh in canonical.meshes(self.source):
            for i in range(0, len(mesh.positions), 3):
                a, b, c = mesh.positions[i:i+3]
                face = original.cross(original.sub(b, a), original.sub(c, a))
                self.assertGreater(original.dot(face, face), 1e-20, mesh.identity)
                for position, normal in zip(mesh.positions[i:i+3], mesh.normals[i:i+3]):
                    self.assertTrue(all(math.isfinite(v) for v in position + normal))
                    self.assertAlmostEqual(original.dot(normal, normal), 1.0)
                    self.assertGreater(original.dot(face, normal), 0)


if __name__ == '__main__':
    unittest.main()
