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
    def test_authoring_reproduces_the_checked_in_asset_and_manifest(self):
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
            self.assertEqual(manifest["mesh_count"], 22)
            self.assertEqual(manifest["triangle_count"], 2872)

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

    def test_enclosure_supports_and_slim_case_retain_human_scale(self):
        meshes = {mesh.identity: mesh for mesh in proof.geometry()}
        def bounds(identity):
            points = meshes[identity].positions
            return ([min(p[k] for p in points) for k in range(3)],
                    [max(p[k] for p in points) for k in range(3)])
        floor_lo, floor_hi = bounds("enclosure.deck")
        roof_lo, _ = bounds("enclosure.raked-overhead")
        case_lo, case_hi = bounds("instrument.central-sculpted-case")
        self.assertEqual(floor_hi[1], 0.0)
        self.assertGreaterEqual(roof_lo[1] - 1.8, 0.29 - 1e-9)
        for axis, maximum in enumerate((0.19, 0.45, 0.69)):
            self.assertLessEqual(case_hi[axis] - case_lo[axis], maximum)
        self.assertLess(case_lo[0], 0.9)
        self.assertGreater(case_hi[0], 0.9)
        # Floor support contains the full 0.6 m diameter standing footprint.
        # This checks geometry envelopes, not world registration or grounding.
        for i in range(33):
            feet = (-0.85 * i / 32, 0.0, 0.65 * i / 32)
            for axis in (0, 2):
                self.assertLessEqual(floor_lo[axis], feet[axis] - 0.3)
                self.assertGreaterEqual(floor_hi[axis], feet[axis] + 0.3)
        def contact(a, b):
            alo, ahi = bounds(a); blo, bhi = bounds(b)
            return all(max(alo[k], blo[k]) <= min(ahi[k], bhi[k]) + 1e-9
                       for k in range(3))
        for side in ("port", "starboard"):
            self.assertTrue(contact(f"console.{side}-deck-web", "enclosure.deck"))
            self.assertTrue(contact(f"console.{side}-deck-web", "console.continuous-dashboard-shell"))
            self.assertTrue(contact(f"enclosure.{side}-sidewall", f"enclosure.{side}-lower-haunch"))
            self.assertTrue(contact(f"enclosure.{side}-upper-haunch", "enclosure.raked-overhead"))


if __name__ == "__main__":
    unittest.main()
