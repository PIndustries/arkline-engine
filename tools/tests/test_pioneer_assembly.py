#!/usr/bin/env python3
"""Unit tests; opt into real-source integration with ARKLINE_BLENDER=/path/to/blender.

Run: PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/tests -p test_pioneer_assembly.py -v
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/art"))
import pioneer_assembly as assembly


def fixture(root):
    for role, relative in assembly.SOURCES.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture source for " + role, encoding="utf-8")
    sources = assembly.source_records(root)
    digest = next(s["sha256"] for s in sources if s["role"] == "blend")
    identity = copy.deepcopy(assembly.IDENTITY)
    material_id = assembly.snapshot_id("material", digest, "Visual material")
    obj = {
        "id": assembly.snapshot_id("object", digest, "Part A"), "name": "Part A",
        "type": "MESH", "data_name": "Mesh A", "parent_id": None, "parent_type": "OBJECT",
        **{field: copy.deepcopy(identity) for field in
           ("basis_blender", "parent_inverse_blender", "local_blender",
            "world_blender", "evaluated_world_blender")},
        "body_m_from_object_local": copy.deepcopy(assembly.BLENDER_TO_BODY),
        "material_slots": [material_id], "evaluated_material_slots": [material_id],
        "modifiers": [], "hide_render": False,
        "geometry": {"vertices": 8, "polygons": 6,
                     "local_blender": {"min": [-36, -19, -9], "max": [36, 19, 9]},
                     "world_blender": {"min": [-36, -19, -9], "max": [36, 19, 9]},
                     "body_m": copy.deepcopy(assembly.DECLARED_BOUNDS),
                     "polygon_material_counts": [{"slot": 0, "polygons": 6}]},
        "physics": copy.deepcopy(assembly.UNKNOWN_PHYSICS),
    }
    return {
        "schema": assembly.SCHEMA, "kind": "derived-inspection-only",
        "identity_scope": "source-hash-and-name-snapshot", "sources": sources,
        "frame": copy.deepcopy(assembly.FRAME), "limits": copy.deepcopy(assembly.LIMITS),
        "physics": copy.deepcopy(assembly.UNKNOWN_PHYSICS),
        "extraction": {"autoexec": False, "scope": "all-bpy-data-objects-single-scene",
                       "evaluation": "saved-frame-viewport-depsgraph", "blender_version": "5.0.1",
                       "blender_build_hash": "test-build", "source_file_version": [5, 0, 119],
                       "scene": "Scene", "view_layer": "ViewLayer", "frame": 1, "subframe": 0.0,
                       "scene_unit_settings": {"system": "METRIC", "scale_length": 1.0}},
        "objects": [obj],
        "materials": [{"id": material_id, "name": "Visual material",
                       "viewport_rgba": [0.1, 0.2, 0.3, 1.0], "use_nodes": True}],
        "assembly_bounds_body_m": copy.deepcopy(assembly.DECLARED_BOUNDS),
        "summary": {"objects": 1, "materials": 1, "evaluated_vertices": 8,
                    "evaluated_polygons": 6, "geometry_objects": 1, "parented_objects": 0},
    }


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.manifest = fixture(self.root)

    def validate(self):
        return assembly.validate_manifest(self.manifest, self.root)

    def reject(self, message):
        with self.assertRaisesRegex(assembly.ValidationError, message):
            self.validate()

    def child(self):
        child = copy.deepcopy(self.manifest["objects"][0])
        child["name"] = "Part B"
        digest = next(s["sha256"] for s in self.manifest["sources"] if s["role"] == "blend")
        child["id"] = assembly.snapshot_id("object", digest, child["name"])
        child["parent_id"] = self.manifest["objects"][0]["id"]
        self.manifest["objects"].append(child)
        self.manifest["summary"].update(objects=2, evaluated_vertices=16, evaluated_polygons=12,
                                        geometry_objects=2, parented_objects=1)
        return child

    def test_valid_deterministic_manifest(self):
        first = assembly.canonical_json(self.manifest)
        restored = json.loads(first)
        self.assertEqual(first, assembly.canonical_json(restored))
        report = self.validate()
        self.assertTrue(report["valid"])
        self.assertEqual(report["envelope"]["status"], "match")

    def test_valid_parent_graph_and_affine_transforms(self):
        child = self.child()
        # A nonuniform, rotated affine matrix is retained, not reduced to a pose.
        matrix = [[0, -2, 0, 3], [1, 0, 0, 4], [0, 0, 0.5, 5], [0, 0, 0, 1]]
        for field in ("basis_blender", "local_blender", "world_blender", "evaluated_world_blender"):
            child[field] = copy.deepcopy(matrix)
        child["body_m_from_object_local"] = assembly.matmul(assembly.BLENDER_TO_BODY, matrix)
        self.assertTrue(self.validate()["valid"])

    def test_parent_inverse_and_basis_consistency(self):
        child = self.child()
        parent = self.manifest["objects"][0]
        for field in ("basis_blender", "local_blender", "world_blender", "evaluated_world_blender"):
            parent[field][0][3] = 10
        parent["body_m_from_object_local"] = assembly.matmul(assembly.BLENDER_TO_BODY, parent["world_blender"])
        child["parent_inverse_blender"][0][3] = -5
        child["basis_blender"][0][3] = 2
        child["local_blender"][0][3] = -3
        child["world_blender"][0][3] = 7
        child["evaluated_world_blender"][0][3] = 7
        child["body_m_from_object_local"] = assembly.matmul(assembly.BLENDER_TO_BODY, child["world_blender"])
        self.assertTrue(self.validate()["valid"])
        child["parent_inverse_blender"][0][3] = -4
        self.reject("basis/parent-inverse/local relation mismatch")

    def test_unparented_basis_must_match_local(self):
        self.manifest["objects"][0]["basis_blender"][0][3] = 1
        self.reject("basis/parent-inverse/local relation mismatch")

    def test_scene_unit_conflict_rejected(self):
        self.manifest["extraction"]["scene_unit_settings"]["scale_length"] = 0.01
        self.reject("scene units conflict")

    def test_frame_known_points_and_roundtrips(self):
        # Body-space turret surface, engine center and axes from source recipe.
        points = [(10.8, 9, -2.66), (10.8, 9, 2.66), (-35.9, 0, -12.5),
                  (1, 0, 0), (0, 1, 0), (0, 0, 1), (-2.5, 8.1, 9.4)]
        for point in points:
            blender = assembly.transform_point(assembly.BODY_TO_BLENDER, point)
            self.assertEqual(blender, [point[0], -point[2], point[1]])
            self.assertEqual(assembly.transform_point(assembly.BLENDER_TO_BODY, blender), list(point))
        self.assertEqual(assembly.matmul(assembly.BLENDER_TO_BODY, assembly.BODY_TO_BLENDER),
                         assembly.IDENTITY)

    def test_snapshot_ids_change_on_rename_or_source_change(self):
        original = assembly.snapshot_id("object", "a" * 64, "Part A")
        self.assertNotEqual(original, assembly.snapshot_id("object", "a" * 64, "Part B"))
        self.assertNotEqual(original, assembly.snapshot_id("object", "b" * 64, "Part A"))

    def test_stale_blend_hash_is_rejected(self):
        (self.root / assembly.SOURCES["blend"]).write_text("different source")
        self.reject("stale source hash")

    def test_stale_recipe_hash_is_rejected(self):
        (self.root / assembly.SOURCES["extractor"]).write_text("different recipe")
        self.reject("stale source hash")

    def test_duplicate_object_rejected(self):
        self.manifest["objects"].append(copy.deepcopy(self.manifest["objects"][0]))
        self.reject("duplicate object")

    def test_duplicate_material_rejected(self):
        self.manifest["materials"].append(copy.deepcopy(self.manifest["materials"][0]))
        self.reject("duplicate material")

    def test_duplicate_source_role_rejected(self):
        self.manifest["sources"][-1] = copy.deepcopy(self.manifest["sources"][0])
        self.reject("duplicate or unknown source")

    def test_missing_parent_rejected(self):
        self.manifest["objects"][0]["parent_id"] = "missing"
        self.reject("missing parent")

    def test_cycle_and_self_parent_rejected(self):
        child = self.child()
        self.manifest["objects"][0]["parent_id"] = child["id"]
        self.reject("parent cycle")
        self.manifest["objects"][0]["parent_id"] = self.manifest["objects"][0]["id"]
        self.reject("parent cycle")

    def test_nonfinite_data_rejected(self):
        for value in (float("nan"), float("inf"), float("-inf")):
            with self.subTest(value=value):
                self.manifest["objects"][0]["world_blender"][0][0] = value
                self.reject("nonfinite")
        self.manifest["objects"][0]["world_blender"][0][0] = 1
        self.manifest["unused_extra_field"] = float("nan")
        self.reject("nonfinite")

    def test_nonaffine_and_invalid_world_relation_rejected(self):
        self.manifest["objects"][0]["world_blender"][3][2] = 1
        self.reject("non-affine")
        self.manifest["objects"][0]["world_blender"][3][2] = 0
        self.manifest["objects"][0]["world_blender"][0][3] = 3
        self.reject("parent relation mismatch")

    def test_wrong_frame_and_world_conversion_rejected(self):
        self.manifest["objects"][0]["body_m_from_object_local"][1][2] = -1
        self.reject("world-to-body transform")
        self.manifest["objects"][0]["body_m_from_object_local"] = copy.deepcopy(assembly.BLENDER_TO_BODY)
        self.manifest["frame"]["meters_per_blender_unit"] = 100
        self.reject("unexpected frame")

    def test_missing_material_reference_and_wrong_polygon_slot_rejected(self):
        self.manifest["objects"][0]["material_slots"] = ["not-a-source-material"]
        self.reject("missing material reference")
        self.manifest["objects"][0]["material_slots"] = []
        self.manifest["objects"][0]["geometry"]["polygon_material_counts"][0]["slot"] = 10
        self.reject("material slot index")

    def test_bounds_mismatch_reported_without_rescale(self):
        measured = {"min": [-37, -9, -19], "max": [36, 9, 19]}
        self.manifest["assembly_bounds_body_m"] = copy.deepcopy(measured)
        self.manifest["objects"][0]["geometry"]["body_m"] = copy.deepcopy(measured)
        self.manifest["objects"][0]["geometry"]["world_blender"]["min"][0] = -37
        before = copy.deepcopy(self.manifest)
        report = self.validate()
        self.assertTrue(report["valid"])
        self.assertEqual(report["envelope"]["status"], "mismatch")
        self.assertEqual(report["envelope"]["mismatches"], [{"axis": "X", "end": "min", "delta_m": -1}])
        self.assertFalse(report["envelope"]["rescaled"])
        self.assertEqual(self.manifest, before)

    def test_aggregate_or_frame_bounds_tampering_rejected(self):
        self.manifest["assembly_bounds_body_m"]["min"][0] = -40
        self.reject("assembly bounds mismatch")
        self.manifest["assembly_bounds_body_m"]["min"][0] = -36
        self.manifest["objects"][0]["geometry"]["body_m"]["min"][0] = -40
        self.reject("bounds frame mismatch")

    def test_physics_is_unknown_not_zero_or_guessed(self):
        self.manifest["objects"][0]["physics"]["mass_kg"] = 0
        self.reject("invented physical properties")

    def test_safe_source_paths_and_symlink_escape(self):
        for bad in ("../outside", "/etc/passwd", "x/../../outside", "x\\outside", "C:/outside",
                    "x//file", "./file"):
            with self.subTest(path=bad), self.assertRaises(assembly.ValidationError):
                assembly.safe_source(self.root, bad)
        with tempfile.TemporaryDirectory() as outside:
            external = Path(outside) / "external"
            external.write_text("outside")
            (self.root / "link").symlink_to(external)
            with self.assertRaisesRegex(assembly.ValidationError, "escapes repository"):
                assembly.safe_source(self.root, "link")
        self.manifest["sources"][0]["path"] = "../outside"
        self.reject("unsafe source path")

    def test_resource_limits_and_malformed_json(self):
        path = self.root / "manifest.json"
        path.write_text('{"duplicate":1,"duplicate":2}')
        with self.assertRaisesRegex(assembly.ValidationError, "duplicate JSON key"):
            assembly.load_manifest(path)
        path.write_text('{"number":1e9999}')
        with self.assertRaisesRegex(assembly.ValidationError, "nonfinite"):
            assembly.load_manifest(path)
        with patch.dict(assembly.LIMITS, manifest_bytes=2):
            with self.assertRaisesRegex(assembly.ValidationError, "byte limit"):
                assembly.load_manifest(path)
        with patch.dict(assembly.LIMITS, json_nodes=2):
            with self.assertRaisesRegex(assembly.ValidationError, "resource limit"):
                assembly.validate_json_tree([0, 1, 2])
        with patch.dict(assembly.LIMITS, json_depth=2):
            with self.assertRaisesRegex(assembly.ValidationError, "resource limit"):
                assembly.validate_json_tree([[[[0]]]])
        with patch.dict(assembly.LIMITS, source_bytes=2):
            with self.assertRaisesRegex(assembly.ValidationError, "source too large"):
                assembly.source_records(self.root)

    def test_geometry_resource_limit_and_summary_rejected(self):
        self.manifest["objects"][0]["geometry"]["vertices"] = assembly.LIMITS["vertices_per_object"] + 1
        self.reject("vertex limit")
        self.manifest["objects"][0]["geometry"]["vertices"] = 8
        self.manifest["summary"]["objects"] = 2
        self.reject("inventory summary mismatch")

    def test_booleans_do_not_impersonate_numeric_frame_or_summary(self):
        self.manifest["frame"]["meters_per_blender_unit"] = True
        self.reject("unexpected frame")
        self.manifest["frame"]["meters_per_blender_unit"] = 1
        self.manifest["summary"]["objects"] = True
        self.reject("inventory summary mismatch")

    def test_structurally_malformed_manifest_fails_cleanly(self):
        for value in (None, [], "bad", {"schema": assembly.SCHEMA}):
            with self.subTest(value=value), self.assertRaises(assembly.ValidationError):
                assembly.validate_manifest(value, self.root)


@unittest.skipUnless(os.environ.get("ARKLINE_BLENDER"), "set ARKLINE_BLENDER to run real-source Blender integration")
class BlenderIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix="pioneer-test-")
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.directory = Path(cls.tmp.name)
        cls.blender = os.environ["ARKLINE_BLENDER"]
        cls.before = assembly.source_records(ROOT)
        cls.paths = [cls.directory / "first.json", cls.directory / "second.json"]
        for path in cls.paths:
            subprocess.run([sys.executable, str(ROOT / "tools/art/export_pioneer_assembly.py"),
                            "--blender", cls.blender, "--output", str(path)], check=True,
                           timeout=assembly.LIMITS["wall_seconds"] + 10,
                           env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
        cls.manifest = assembly.load_manifest(cls.paths[0])
        # Independent inventory and evaluated-geometry sample from the real saved file.
        # This does not import the exporter or validator into Blender.
        inventory = cls.directory / "inventory.json"
        expression = (
            "import bpy,json,pathlib;"
            "names=sorted(o.name for o in bpy.data.objects);"
            "materials=sorted(m.name for m in bpy.data.materials);"
            "parents={o.name:(o.parent.name if o.parent else None) for o in bpy.data.objects};"
            "sockets={o.name:list(o.location) for o in bpy.data.objects if o.name.startswith('Turret mounting socket')};"
            f"pathlib.Path({str(inventory)!r}).write_text(json.dumps(dict(names=names,materials=materials,parents=parents,sockets=sockets)))"
        )
        subprocess.run([cls.blender, "--background", "--factory-startup", "--disable-autoexec",
                        "--threads", "2", str(ROOT / assembly.SOURCES["blend"]),
                        "--python-exit-code", "1", "--python-expr", expression], check=True,
                       stdout=subprocess.DEVNULL, timeout=assembly.LIMITS["wall_seconds"])
        cls.inventory = json.loads(inventory.read_text())

    def test_repeated_extraction_is_byte_identical(self):
        self.assertEqual(self.paths[0].read_bytes(), self.paths[1].read_bytes())
        self.assertTrue(assembly.validate_manifest(self.manifest, ROOT)["valid"])

    def test_complete_actual_object_material_and_parent_coverage(self):
        self.assertEqual([o["name"] for o in self.manifest["objects"]], self.inventory["names"])
        self.assertEqual([m["name"] for m in self.manifest["materials"]], self.inventory["materials"])
        by_id = {o["id"]: o["name"] for o in self.manifest["objects"]}
        parents = {o["name"]: by_id.get(o["parent_id"]) for o in self.manifest["objects"]}
        self.assertEqual(parents, self.inventory["parents"])
        self.assertEqual(len(self.inventory["sockets"]), 2)
        for name, point in self.inventory["sockets"].items():
            body = assembly.transform_point(assembly.BLENDER_TO_BODY, point)
            self.assertAlmostEqual(body[0], 10.8, places=5)
            self.assertAlmostEqual(body[1], 8.85, places=5)
            self.assertAlmostEqual(abs(body[2]), 2.66, places=5)
            obj = next(o for o in self.manifest["objects"] if o["name"] == name)
            self.assertEqual(assembly.transform_point(obj["body_m_from_object_local"], [0, 0, 0]), body)

    def test_no_source_or_recipe_changes(self):
        self.assertEqual(assembly.source_records(ROOT), self.before)

    def test_existing_output_and_repository_output_are_rejected(self):
        for path in (self.paths[0], ROOT / "must-not-create.json", ROOT / assembly.SOURCES["blend"]):
            result = subprocess.run([sys.executable, str(ROOT / "tools/art/export_pioneer_assembly.py"),
                                     "--blender", self.blender, "--output", str(path)],
                                    capture_output=True, text=True, timeout=10,
                                    env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("inspection failed", result.stderr)
        self.assertFalse((ROOT / "must-not-create.json").exists())


if __name__ == "__main__":
    unittest.main()
