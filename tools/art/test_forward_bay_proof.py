"""Portable regression checks for the isolated study, without Blender or a world."""
import contextlib
import io
import json
import math
from pathlib import Path
import tempfile
import unittest

import forward_bay_proof as proof


class ForwardBayProofTests(unittest.TestCase):
    def test_authoring_reproduces_the_migrated_asset_and_manifest(self):
        study = Path(__file__).resolve().parents[2] / "assets/studies/pioneer-forward-bay-proof"
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "study"
            with contextlib.redirect_stdout(io.StringIO()):
                proof.write(output)
            for filename in ("forward-bay-proof.glb", "manifest.json"):
                self.assertEqual((output / filename).read_bytes(),
                                 (study / filename).read_bytes(), filename)
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertTrue(manifest["study_only"])
            self.assertEqual(manifest["mesh_count"], 16)
            self.assertEqual(manifest["triangle_count"], 3256)

    def test_authored_triangles_have_finite_unit_normals_and_consistent_winding(self):
        meshes = proof.geometry()
        self.assertEqual(len({mesh.identity for mesh in meshes}), len(meshes))
        for mesh in meshes:
            self.assertEqual(len(mesh.positions), len(mesh.normals), mesh.identity)
            self.assertEqual(len(mesh.indices) % 3, 0)
            for i in range(0, len(mesh.indices), 3):
                indices = mesh.indices[i:i + 3]
                a, b, c = [mesh.positions[index] for index in indices]
                face = proof.cross(proof.sub(b, a), proof.sub(c, a))
                self.assertGreater(proof.dot(face, face), 1e-20, mesh.identity)
                for index in indices:
                    normal = mesh.normals[index]
                    self.assertTrue(all(math.isfinite(v) for v in mesh.positions[index] + normal))
                    self.assertAlmostEqual(proof.dot(normal, normal), 1.0, places=6)
                    self.assertGreater(proof.dot(face, normal), 0, mesh.identity)


if __name__ == "__main__":
    unittest.main()
