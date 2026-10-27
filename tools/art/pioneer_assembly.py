#!/usr/bin/env python3
"""Deterministic, stdlib-only validation of derived Pioneer inspection evidence.

This is not a CAD/mass model or a trusted alternative to the saved .blend.
Run: python3 tools/art/pioneer_assembly.py MANIFEST --root REPOSITORY
"""
import argparse
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import sys

SCHEMA = "arkline.pioneer-assembly-inspection/v1"
SOURCES = {
    "blend": "assets/ships/pioneer/pioneer.blend",
    "generator": "tools/art/build_pioneer_raised_bridge.py",
    "description": "assets/ships/pioneer/README.md",
    "contract": "docs/ships/design-runtime-contract.md",
    "extractor": "tools/art/export_pioneer_assembly.py",
    "validator": "tools/art/pioneer_assembly.py",
}
LIMITS = {
    "source_bytes": 128 * 1024 * 1024,
    "manifest_bytes": 32 * 1024 * 1024,
    "objects": 10000,
    "materials": 512,
    "vertices_per_object": 2000000,
    "vertices_total": 10000000,
    "polygons_total": 10000000,
    "json_nodes": 2000000,
    "json_depth": 64,
    "wall_seconds": 180,
    "cpu_seconds": 120,
    "address_space_bytes": 6 * 1024 * 1024 * 1024,
}
IDENTITY = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
BLENDER_TO_BODY = [[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]]
BODY_TO_BLENDER = [[1, 0, 0, 0], [0, 0, -1, 0], [0, 1, 0, 0], [0, 0, 0, 1]]
FRAME = {
    "blender_world_axes": "+X forward, +Y port, +Z up",
    "body_axes": "+X forward, +Y up, +Z starboard",
    "handedness": "right",
    "origin": "saved Blender world origin; documented hull center",
    "meters_per_blender_unit": 1,
    "unit_basis": "build_pioneer.py v(p)=(p[0],-p[2],p[1]); README declares meters",
    "blender_world_to_body_m": BLENDER_TO_BODY,
    "body_m_to_blender_world": BODY_TO_BLENDER,
    "object_local_basis": "unchanged Blender object-local coordinates",
}
DECLARED_BOUNDS = {"min": [-36, -9, -19], "max": [36, 11.35, 19]}
UNKNOWN_PHYSICS = {
    "status": "unresolved",
    "mass_kg": None,
    "density_kg_m3": None,
    "pressure_rating_pa": None,
    "strength_rating_pa": None,
    "thermal_rating_k": None,
}


class ValidationError(ValueError):
    """The manifest is malformed, stale or outside this inspection contract."""


def require(condition, message):
    if not condition:
        raise ValidationError(message)


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2,
                      allow_nan=False) + "\n"


def safe_source(root, relative):
    """Resolve only canonical, project-relative paths contained by root."""
    require(isinstance(relative, str) and 0 < len(relative) <= 1024,
            "invalid source path")
    path = PurePosixPath(relative)
    require(not path.is_absolute() and "\\" not in relative and ":" not in relative
            and all(part not in ("", ".", "..") for part in relative.split("/")),
            f"unsafe source path: {relative!r}")
    root = Path(root).resolve(strict=True)
    resolved = (root / path).resolve(strict=True)
    require(resolved.is_relative_to(root) and resolved.is_file(),
            f"source escapes repository or is not a file: {relative}")
    return resolved


def source_record(root, role, relative):
    path = safe_source(root, relative)
    size = path.stat().st_size
    require(size <= LIMITS["source_bytes"], f"source too large: {relative}")
    digest = hashlib.sha256()
    read = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            read += len(chunk)
            require(read <= LIMITS["source_bytes"], "source grew beyond limit")
            digest.update(chunk)
    require(read == size, f"source size changed while reading: {relative}")
    return {"role": role, "path": relative, "size_bytes": size,
            "sha256": digest.hexdigest()}


def source_records(root):
    return [source_record(root, role, path) for role, path in sorted(SOURCES.items())]


def snapshot_id(kind, source_hash, name):
    digest = hashlib.sha256((kind + "\0" + source_hash + "\0" + name).encode()).hexdigest()
    return f"{kind}:{digest}"


def matmul(left, right):
    return [[sum(left[i][k] * right[k][j] for k in range(4))
             for j in range(4)] for i in range(4)]


def transform_point(matrix, point):
    return [sum(matrix[i][j] * point[j] for j in range(3)) + matrix[i][3]
            for i in range(3)]


def close_matrix(left, right):
    return all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-5)
               for lrow, rrow in zip(left, right) for a, b in zip(lrow, rrow))


def finite_number(value):
    return type(value) in (int, float) and math.isfinite(value)


def validate_json_tree(value):
    # Also rejects nonfinite values in fields not otherwise consumed by validation.
    pending, count = [(value, 0)], 0
    while pending:
        item, depth = pending.pop()
        count += 1
        require(count <= LIMITS["json_nodes"] and depth <= LIMITS["json_depth"],
                "JSON resource limit exceeded")
        if isinstance(item, dict):
            require(all(isinstance(k, str) and len(k) <= 1024 for k in item),
                    "invalid JSON key")
            require(count + len(pending) + len(item) <= LIMITS["json_nodes"],
                    "JSON resource limit exceeded")
            pending.extend((v, depth + 1) for v in item.values())
        elif isinstance(item, list):
            require(count + len(pending) + len(item) <= LIMITS["json_nodes"],
                    "JSON resource limit exceeded")
            pending.extend((v, depth + 1) for v in item)
        elif type(item) in (int, float):
            require(finite_number(item), "nonfinite number")
        elif isinstance(item, str):
            require(len(item) <= 4096, "JSON string too long")
        else:
            require(item is None or type(item) is bool, "invalid JSON value")


def affine(value, label):
    require(isinstance(value, list) and len(value) == 4
            and all(isinstance(row, list) and len(row) == 4 for row in value),
            f"{label}: expected 4x4 affine matrix")
    require(all(finite_number(v) for row in value for v in row),
            f"{label}: nonfinite affine matrix")
    require(value[3] == [0, 0, 0, 1], f"{label}: non-affine bottom row")
    # Singular object scale is legal Blender data. No inverse is inferred here.


def bounds(value, label):
    require(isinstance(value, dict) and set(value) == {"min", "max"},
            f"{label}: invalid bounds")
    for end in ("min", "max"):
        require(isinstance(value[end], list) and len(value[end]) == 3
                and all(finite_number(v) for v in value[end]),
                f"{label}: expected finite XYZ bounds")
    require(all(a <= b for a, b in zip(value["min"], value["max"])),
            f"{label}: inverted bounds")


def union_bounds(items):
    items = list(items)
    if not items:
        return None
    return {"min": [min(b["min"][i] for b in items) for i in range(3)],
            "max": [max(b["max"][i] for b in items) for i in range(3)]}


def convert_bounds_to_body(value):
    # Exact for this signed-axis permutation, not a general rotated-box algorithm.
    return {"min": [value["min"][0], value["min"][2], -value["max"][1]],
            "max": [value["max"][0], value["max"][2], -value["min"][1]]}


def bounds_report(measured, tolerance_m=0.001):
    """Report measured-versus-declared deltas; never fit or rescale geometry."""
    mismatches = []
    if measured is not None:
        for i, axis in enumerate("XYZ"):
            for end in ("min", "max"):
                delta = measured[end][i] - DECLARED_BOUNDS[end][i]
                if abs(delta) > tolerance_m:
                    mismatches.append({"axis": axis, "end": end, "delta_m": delta})
    return {"status": "unmeasured" if measured is None else
            ("mismatch" if mismatches else "match"),
            "declared_body_bounds_m": DECLARED_BOUNDS,
            "measured_body_bounds_m": measured,
            "tolerance_m": tolerance_m, "mismatches": mismatches,
            "rescaled": False}


def _unique_named(records, kind, source_hash, maximum):
    require(isinstance(records, list) and len(records) <= maximum,
            f"{kind} count exceeds resource limit")
    by_id, names = {}, set()
    for record in records:
        require(isinstance(record, dict), f"invalid {kind} record")
        name = record.get("name")
        require(isinstance(name, str) and 0 < len(name) <= 1024,
                f"invalid {kind} source name")
        identity = record.get("id")
        require(isinstance(identity, str), f"invalid {kind} identity")
        require(identity not in by_id and name not in names,
                f"duplicate {kind} identity or name")
        require(identity == snapshot_id(kind, source_hash, name),
                f"{name}: invalid snapshot identity")
        by_id[identity] = record
        names.add(name)
    require([r["name"] for r in records] == sorted(names),
            f"{kind} records must be name-sorted")
    return by_id


def _validate(manifest, root):
    validate_json_tree(manifest)
    require(isinstance(manifest, dict), "manifest must be an object")
    require(manifest.get("schema") == SCHEMA, "unsupported schema")
    require(manifest.get("kind") == "derived-inspection-only", "not inspection evidence")
    require(manifest.get("identity_scope") == "source-hash-and-name-snapshot",
            "identities must be snapshot-scoped, not durable component IDs")
    records = manifest.get("sources")
    require(isinstance(records, list) and len(records) == len(SOURCES),
            "invalid source inventory")
    seen = set()
    for record in records:
        require(isinstance(record, dict), "invalid source record")
        role = record.get("role")
        require(isinstance(role, str) and role in SOURCES and role not in seen,
                "duplicate or unknown source role")
        # Validate untrusted paths even though the exact role/path mapping is fixed.
        safe_source(root, record.get("path"))
        require(record["path"] == SOURCES[role], "unexpected source path for role")
        actual = source_record(root, role, record["path"])
        require(type(record.get("size_bytes")) is int and record == actual,
                f"stale source hash or size: {record['path']}")
        seen.add(role)
    require(records == sorted(records, key=lambda r: r["role"]), "sources must be role-sorted")
    source_hash = next(r["sha256"] for r in records if r["role"] == "blend")
    require(canonical_json(manifest.get("frame")) == canonical_json(FRAME),
            "unexpected frame conversion or unit basis")
    require(manifest.get("limits") == LIMITS, "resource limits changed")
    require(manifest.get("physics") == UNKNOWN_PHYSICS, "physical properties must remain unresolved")
    extraction = manifest.get("extraction")
    require(isinstance(extraction, dict) and extraction.get("autoexec") is False
            and extraction.get("evaluation") == "saved-frame-viewport-depsgraph"
            and extraction.get("scope") == "all-bpy-data-objects-single-scene",
            "unsupported extraction settings")
    require(isinstance(extraction.get("blender_version"), str)
            and isinstance(extraction.get("scene"), str)
            and isinstance(extraction.get("view_layer"), str)
            and type(extraction.get("frame")) is int
            and finite_number(extraction.get("subframe")), "missing evaluation provenance")
    require(isinstance(extraction.get("blender_build_hash"), str)
            and isinstance(extraction.get("source_file_version"), list)
            and len(extraction["source_file_version"]) == 3
            and all(type(v) is int and v >= 0 for v in extraction["source_file_version"]),
            "missing Blender build/file provenance")
    units = extraction.get("scene_unit_settings")
    require(isinstance(units, dict) and units.get("system") in ("METRIC", "NONE")
            and finite_number(units.get("scale_length")) and units["scale_length"] == 1.0,
            "scene units conflict with declared meter basis")
    mats = _unique_named(manifest.get("materials"), "material", source_hash, LIMITS["materials"])
    for mat in mats.values():
        rgba = mat.get("viewport_rgba")
        require(isinstance(rgba, list) and len(rgba) == 4
                and all(finite_number(v) for v in rgba), "invalid viewport material color")
        require(type(mat.get("use_nodes")) is bool, "invalid material node flag")
    objects = _unique_named(manifest.get("objects"), "object", source_hash, LIMITS["objects"])
    require(objects, "source has no objects")
    vertices, polygons = 0, 0
    for obj in objects.values():
        label = obj["name"]
        parent = obj.get("parent_id")
        require(parent is None or isinstance(parent, str) and parent in objects,
                f"{label}: missing parent")
        require(obj.get("parent_type") == "OBJECT", f"{label}: unsupported parenting")
        for field in ("basis_blender", "parent_inverse_blender", "local_blender",
                      "world_blender", "evaluated_world_blender", "body_m_from_object_local"):
            affine(obj.get(field), f"{label}/{field}")
        require(close_matrix(obj["body_m_from_object_local"],
                             matmul(BLENDER_TO_BODY, obj["evaluated_world_blender"])),
                f"{label}: incorrect world-to-body transform")
        expected_local = (matmul(obj["parent_inverse_blender"], obj["basis_blender"])
                          if parent else obj["basis_blender"])
        require(close_matrix(obj["local_blender"], expected_local),
                f"{label}: basis/parent-inverse/local relation mismatch")
        parent_world = objects[parent]["world_blender"] if parent else IDENTITY
        affine(parent_world, f"{label}/parent-world")
        require(close_matrix(obj["world_blender"], matmul(parent_world, obj["local_blender"])),
                f"{label}: local/world parent relation mismatch")
        require(obj.get("physics") == UNKNOWN_PHYSICS, f"{label}: invented physical properties")
        require(isinstance(obj.get("type"), str) and isinstance(obj.get("data_name"), (str, type(None))),
                f"{label}: missing source type/data reference")
        for slots in ("material_slots", "evaluated_material_slots"):
            require(isinstance(obj.get(slots), list) and len(obj[slots]) <= LIMITS["materials"],
                    f"{label}: invalid material slots")
            for identity in obj[slots]:
                require(identity is None or isinstance(identity, str) and identity in mats,
                        f"{label}: missing material reference")
        geometry = obj.get("geometry")
        if geometry is None:
            require(obj["type"] not in ("MESH", "CURVE", "SURFACE", "FONT", "META"),
                    f"{label}: missing evaluated geometry")
            continue
        require(isinstance(geometry, dict), f"{label}: invalid geometry")
        for count in ("vertices", "polygons"):
            require(type(geometry.get(count)) is int and geometry[count] >= 0,
                    f"{label}: invalid geometry count")
        require(geometry["vertices"] <= LIMITS["vertices_per_object"], "vertex limit exceeded")
        vertices += geometry["vertices"]
        polygons += geometry["polygons"]
        if geometry["vertices"]:
            for frame in ("local_blender", "world_blender", "body_m"):
                bounds(geometry.get(frame), f"{label}/{frame}")
            require(geometry["body_m"] == convert_bounds_to_body(geometry["world_blender"]),
                    f"{label}: bounds frame mismatch")
        else:
            require(all(geometry.get(frame) is None for frame in
                        ("local_blender", "world_blender", "body_m")), "empty mesh has bounds")
        assignments = geometry.get("polygon_material_counts")
        require(isinstance(assignments, list) and len(assignments) <= LIMITS["materials"],
                f"{label}: invalid face material inventory")
        indices = set()
        for assignment in assignments:
            require(isinstance(assignment, dict), "invalid face material assignment")
            index, count = assignment.get("slot"), assignment.get("polygons")
            require(type(index) is int and index >= 0 and index not in indices
                    and type(count) is int and count > 0, "invalid face material count")
            # Blender permits index zero on a mesh with no assigned material slots.
            require(index < max(1, len(obj["evaluated_material_slots"])), "invalid material slot index")
            indices.add(index)
        require(sum(a["polygons"] for a in assignments) == geometry["polygons"],
                "face material counts do not cover polygons")
    require(vertices <= LIMITS["vertices_total"] and polygons <= LIMITS["polygons_total"],
            "total geometry resource limit exceeded")
    # Linear, iterative graph walk avoids recursion failure on deep parent chains.
    done = set()
    for identity in objects:
        active, current = set(), identity
        while current is not None and current not in done:
            require(current not in active, "parent cycle")
            active.add(current)
            current = objects[current]["parent_id"]
        done.update(active)
    measured = union_bounds(obj["geometry"]["body_m"] for obj in objects.values()
                            if obj["geometry"] and obj["geometry"]["body_m"] is not None)
    require(manifest.get("assembly_bounds_body_m") == measured, "assembly bounds mismatch")
    summary = {"objects": len(objects), "materials": len(mats),
               "evaluated_vertices": vertices, "evaluated_polygons": polygons,
               "geometry_objects": sum(obj["geometry"] is not None for obj in objects.values()),
               "parented_objects": sum(obj["parent_id"] is not None for obj in objects.values())}
    require(isinstance(manifest.get("summary"), dict)
            and all(type(v) is int for v in manifest["summary"].values())
            and manifest["summary"] == summary, "inventory summary mismatch")
    return {"valid": True, "scope": "source freshness and manifest consistency; not engineering certification",
            "summary": summary, "envelope": bounds_report(measured)}


def validate_manifest(manifest, root):
    try:
        return _validate(manifest, root)
    except ValidationError:
        raise
    except (KeyError, TypeError, ValueError, OSError, OverflowError, RecursionError) as exc:
        raise ValidationError(f"malformed manifest or unreadable source: {exc}") from exc


def _unique_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_manifest(path):
    try:
        with Path(path).open("rb") as handle:
            raw = handle.read(LIMITS["manifest_bytes"] + 1)
        require(len(raw) <= LIMITS["manifest_bytes"], "manifest byte limit exceeded")
        result = json.loads(raw, object_pairs_hook=_unique_keys)
        validate_json_tree(result)
        return result
    except (OSError, ValueError, UnicodeError, RecursionError) as exc:
        raise ValidationError(f"invalid manifest: {exc}") from exc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    try:
        print(canonical_json(validate_manifest(load_manifest(args.manifest), args.root)), end="")
    except ValidationError as exc:
        parser.exit(1, f"validation failed: {exc}\n")


if __name__ == "__main__":
    main()
