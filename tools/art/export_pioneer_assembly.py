#!/usr/bin/env python3
"""Read the saved Pioneer .blend into reproducible, non-authoritative evidence.

Use ordinary Python to launch a bounded Blender subprocess with auto-execution
explicitly disabled. This script never saves the .blend or invokes its generator.
"""
import argparse
from collections import Counter
import os
from pathlib import Path
import subprocess
import sys
import tempfile

# Blender can ignore Python environment variables; keep the source tree read-only.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from pioneer_assembly import (  # noqa: E402
    BLENDER_TO_BODY, FRAME, LIMITS, SCHEMA, SOURCES, UNKNOWN_PHYSICS,
    ValidationError, canonical_json, finite_number, load_manifest, matmul,
    require, safe_source, snapshot_id, source_records, union_bounds,
    validate_manifest,
)

ROOT = Path(__file__).resolve().parents[2]


def _matrix(matrix):
    return [[float(value) if value else 0.0 for value in row] for row in matrix]


def _geometry(obj, depsgraph, used):
    if obj.type not in {"MESH", "CURVE", "SURFACE", "FONT", "META"}:
        return None
    mesh = obj.to_mesh(preserve_all_data_layers=False, depsgraph=depsgraph)
    require(mesh is not None, f"{obj.name}: evaluated mesh unavailable")
    try:
        nvertices, npolygons = len(mesh.vertices), len(mesh.polygons)
        used[0] += nvertices
        used[1] += npolygons
        require(nvertices <= LIMITS["vertices_per_object"]
                and used[0] <= LIMITS["vertices_total"]
                and used[1] <= LIMITS["polygons_total"], "evaluated geometry limit exceeded")
        local = {"min": [float("inf")] * 3, "max": [float("-inf")] * 3}
        world = {"min": [float("inf")] * 3, "max": [float("-inf")] * 3}
        for vertex in mesh.vertices:
            for target, point in ((local, vertex.co), (world, obj.matrix_world @ vertex.co)):
                require(all(finite_number(float(v)) for v in point),
                        f"{obj.name}: nonfinite evaluated vertex")
                for i, value in enumerate(point):
                    value = float(value) if value else 0.0
                    target["min"][i] = min(target["min"][i], value)
                    target["max"][i] = max(target["max"][i], value)
        from pioneer_assembly import convert_bounds_to_body
        counts = Counter(p.material_index for p in mesh.polygons)
        return {"vertices": nvertices, "polygons": npolygons,
                "local_blender": local if nvertices else None,
                "world_blender": world if nvertices else None,
                "body_m": convert_bounds_to_body(world) if nvertices else None,
                "polygon_material_counts": [{"slot": index, "polygons": counts[index]}
                                            for index in sorted(counts)]}
    finally:
        obj.to_mesh_clear()


def extract(root):
    import bpy
    require(not bpy.context.preferences.filepaths.use_scripts_auto_execute,
            "Blender auto-execution must be disabled")
    require(Path(bpy.data.filepath).resolve() == safe_source(root, SOURCES["blend"]),
            "loaded source does not match registered Pioneer source")
    require(not bpy.data.libraries, "linked libraries are outside this inspection baseline")
    require(len(bpy.data.scenes) == 1, "only one source scene is supported")
    require(len(bpy.data.objects) <= LIMITS["objects"]
            and len(bpy.data.materials) <= LIMITS["materials"], "source inventory limit exceeded")
    scene = bpy.context.scene
    require(len(scene.view_layers) == 1, "multiple source view layers are unsupported")
    # Do not silently omit detached, excluded, animated or instanced source data.
    require(set(bpy.data.objects) == set(scene.objects)
            == set(bpy.context.view_layer.objects), "not all source objects are in the evaluated scene")
    require(not bpy.data.actions, "animated source data is outside this static inspection baseline")
    sources = source_records(root)
    source_hash = next(r["sha256"] for r in sources if r["role"] == "blend")
    object_id = lambda name: snapshot_id("object", source_hash, name)
    material_id = lambda name: snapshot_id("material", source_hash, name)
    materials = [{"id": material_id(mat.name), "name": mat.name,
                  "viewport_rgba": [float(v) for v in mat.diffuse_color],
                  "use_nodes": bool(mat.use_nodes)}
                 for mat in sorted(bpy.data.materials, key=lambda mat: mat.name)]
    depsgraph = bpy.context.evaluated_depsgraph_get()
    objects, used = [], [0, 0]
    for obj in sorted(bpy.data.objects, key=lambda obj: obj.name):
        require(obj.parent_type == "OBJECT" and not obj.constraints,
                f"{obj.name}: non-object parenting/constraints unsupported")
        require(obj.instance_type == "NONE" and not obj.is_instancer,
                f"{obj.name}: collection/particle instancing unsupported")
        require(not obj.hide_viewport and not obj.hide_get(),
                f"{obj.name}: hidden viewport geometry cannot be fully evaluated")
        for datablock in (obj, obj.data):
            animation = getattr(datablock, "animation_data", None)
            require(animation is None, f"{obj.name}: animation/drivers unsupported")
        require(all(mod.type in {"BEVEL", "WEIGHTED_NORMAL"} for mod in obj.modifiers),
                f"{obj.name}: modifier outside static Pioneer baseline")
        evaluated = obj.evaluated_get(depsgraph)
        world = _matrix(evaluated.matrix_world)
        objects.append({
            "id": object_id(obj.name), "name": obj.name, "type": obj.type,
            "data_name": obj.data.name if obj.data else None,
            "parent_id": object_id(obj.parent.name) if obj.parent else None,
            "parent_type": obj.parent_type,
            "basis_blender": _matrix(obj.matrix_basis),
            "parent_inverse_blender": _matrix(obj.matrix_parent_inverse),
            "local_blender": _matrix(obj.matrix_local),
            "world_blender": _matrix(obj.matrix_world),
            "evaluated_world_blender": world,
            "body_m_from_object_local": matmul(BLENDER_TO_BODY, world),
            "material_slots": [material_id(s.material.name) if s.material else None
                               for s in obj.material_slots],
            "evaluated_material_slots": [material_id(s.material.name) if s.material else None
                                         for s in evaluated.material_slots],
            "modifiers": [{"name": mod.name, "type": mod.type,
                           "show_viewport": bool(mod.show_viewport),
                           "show_render": bool(mod.show_render)} for mod in obj.modifiers],
            "hide_render": bool(obj.hide_render),
            "geometry": _geometry(evaluated, depsgraph, used),
            "physics": UNKNOWN_PHYSICS,
        })
    manifest = {
        "schema": SCHEMA, "kind": "derived-inspection-only",
        "identity_scope": "source-hash-and-name-snapshot",
        "sources": sources, "frame": FRAME, "limits": LIMITS,
        "physics": UNKNOWN_PHYSICS,
        "extraction": {
            "autoexec": False, "scope": "all-bpy-data-objects-single-scene",
            "evaluation": "saved-frame-viewport-depsgraph",
            "blender_version": bpy.app.version_string,
            "blender_build_hash": bpy.app.build_hash.decode("ascii"),
            "source_file_version": list(bpy.data.version),
            "scene": scene.name, "view_layer": bpy.context.view_layer.name,
            "frame": scene.frame_current, "subframe": scene.frame_subframe,
            "scene_unit_settings": {"system": scene.unit_settings.system,
                                    "scale_length": scene.unit_settings.scale_length},
        },
        "objects": objects, "materials": materials,
        "assembly_bounds_body_m": union_bounds(o["geometry"]["body_m"] for o in objects
                                                if o["geometry"] and o["geometry"]["body_m"]),
        "summary": {"objects": len(objects), "materials": len(materials),
                    "evaluated_vertices": used[0], "evaluated_polygons": used[1],
                    "geometry_objects": sum(o["geometry"] is not None for o in objects),
                    "parented_objects": sum(o["parent_id"] is not None for o in objects)},
    }
    validate_manifest(manifest, root)
    return manifest


def _resource_limits():
    # Applied in the child before exec; the stdlib validator itself is portable.
    import resource
    resource.setrlimit(resource.RLIMIT_CPU, (LIMITS["cpu_seconds"], LIMITS["cpu_seconds"]))
    resource.setrlimit(resource.RLIMIT_AS, (LIMITS["address_space_bytes"], LIMITS["address_space_bytes"]))
    resource.setrlimit(resource.RLIMIT_FSIZE, (LIMITS["manifest_bytes"], LIMITS["manifest_bytes"]))
    resource.setrlimit(resource.RLIMIT_NOFILE, (256, 256))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def run_export(blender, root=ROOT):
    require(os.name == "posix", "bounded extraction requires POSIX resource limits")
    before = source_records(root)
    with tempfile.TemporaryDirectory(prefix="pioneer-inspection-") as temporary:
        directory = Path(temporary)
        manifest_path, log_path = directory / "manifest.json", directory / "blender.log"
        command = [str(blender), "--background", "--factory-startup", "--disable-autoexec",
                   "--threads", "2", str(safe_source(root, SOURCES["blend"])),
                   "--python-exit-code", "1", "--python", str(Path(__file__).resolve()),
                   "--", "--worker", "--output", str(manifest_path)]
        env = dict(os.environ, OMP_NUM_THREADS="2", OPENBLAS_NUM_THREADS="2",
                   BLENDER_USER_CONFIG=str(directory / "config"), PYTHONNOUSERSITE="1")
        try:
            with log_path.open("wb") as log:
                result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT,
                                        env=env, timeout=LIMITS["wall_seconds"],
                                        preexec_fn=_resource_limits, check=False)
        except subprocess.TimeoutExpired as exc:
            raise ValidationError("Blender exceeded inspection wall-time limit") from exc
        require(source_records(root) == before, "source or recipe changed during extraction")
        if result.returncode or not manifest_path.is_file():
            with log_path.open("rb") as log:
                log.seek(max(0, log_path.stat().st_size - 8000))
                details = log.read().decode("utf-8", errors="replace")
            raise ValidationError(f"Blender inspection failed ({result.returncode}); "
                                  f"use a compatible Blender 5.x build.\n{details}")
        manifest = load_manifest(manifest_path)
        validate_manifest(manifest, root)
        return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blender", default="blender", help="compatible Blender 5.x executable")
    parser.add_argument("--output", required=True, help="new JSON file outside repository, or - for stdout")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    args = parser.parse_args(argv)
    try:
        if args.output != "-":
            destination = Path(args.output)
            require(not destination.resolve().is_relative_to(ROOT),
                    "write derived evidence outside the source repository")
            require(not destination.exists() and not destination.is_symlink(),
                    "output already exists; refusing overwrite")
        manifest = extract(ROOT) if args.worker else run_export(args.blender)
        output = canonical_json(manifest)
        require(len(output.encode("utf-8")) <= LIMITS["manifest_bytes"], "manifest byte limit exceeded")
        if args.output == "-":
            print(output, end="")
        else:
            with destination.open("x", encoding="utf-8") as handle:
                handle.write(output)
    except (ValidationError, OSError) as exc:
        parser.exit(1, f"inspection failed: {exc}\n")


if __name__ == "__main__":
    main()
