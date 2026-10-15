#!/usr/bin/env python3
"""Derive a new editable fidelity study from the selected saved Study04 master."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    out = args.output_dir.absolute()
    if out.exists() or out.is_symlink():
        raise RuntimeError('Study output must be a new exclusive directory')
    expected = '772bf76aaf1416f28357e2f507963022c7c73b13fca58cb57e948e06761a7800'
    if digest(source) != expected:
        raise RuntimeError('Explicit Study04 master hash required for this study recipe')
    import bpy
    if bpy.app.version != (5, 0, 1):
        raise RuntimeError('This reviewed source-authoring recipe requires Blender5.0.1')
    bpy.context.preferences.filepaths.use_scripts_auto_execute = False
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.open_mainfile(filepath=str(source), load_ui=False, use_scripts=False)
    before = {o.name: [m.name for m in o.data.materials]
              for o in bpy.context.scene.objects if o.type == 'MESH'}
    root = Path(__file__).resolve().parent
    reports = {}
    recipe_hashes = {Path(__file__).name: digest(Path(__file__))}
    for name in ('pioneer_study05_bow', 'pioneer_study05_roof', 'pioneer_study05_aft'):
        path = root / (name + '.py')
        recipe_hashes[path.name] = digest(path)
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        reports[name] = module.apply(bpy)
    # Opaque scalar-PBR approximation: reduce the diffuse colored sheet while
    # retaining the exact material identity. Actual powered emission is the
    # separate accepted Window fixture parameter, not this source shader.
    glass = bpy.data.materials['Navigation glass']
    shader = glass.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Metallic'].default_value = .60
    shader.inputs['Roughness'].default_value = .12
    reports['glazing_material'] = {
        'name': glass.name, 'metallic': .60, 'roughness': .12,
        'base_color_retained': list(shader.inputs['Base Color'].default_value),
        'scope': 'Opaque scalar-PBR visual approximation; no transmission, '
                 'fitted interior or optical engineering claim.',
    }
    bpy.context.view_layer.update()
    after = {o.name: [m.name for m in o.data.materials]
             for o in bpy.context.scene.objects if o.type == 'MESH'}
    removed = sorted(set(before) - set(after))
    changed_materials = {name: {'before': before[name], 'after': after[name]}
                         for name in set(before) & set(after)
                         if before[name] != after[name]}
    if changed_materials:
        raise RuntimeError('Retained part material identity changed: ' + str(changed_materials))
    depsgraph = bpy.context.evaluated_depsgraph_get()
    lo, hi = [float('inf')] * 3, [float('-inf')] * 3
    for obj in bpy.context.scene.objects:
        if obj.type != 'MESH':
            raise RuntimeError('Static mesh-only source required')
        if obj.parent is not None or len(obj.data.materials) != 1:
            raise RuntimeError('Unparented single-slot source parts required')
        if any(m.type not in {'BEVEL', 'WEIGHTED_NORMAL'} for m in obj.modifiers):
            raise RuntimeError('Unsupported live modifier: ' + obj.name)
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()
        for vertex in mesh.vertices:
            v = evaluated.matrix_world @ vertex.co
            body = (v.x, v.z, -v.y)
            for axis in range(3):
                lo[axis] = min(lo[axis], body[axis])
                hi[axis] = max(hi[axis], body[axis])
        evaluated.to_mesh_clear()
    for actual, expected_bound in zip(lo + hi, [-36, -9, -19, 36, 9, 19]):
        if abs(actual - expected_bound) > .001:
            raise RuntimeError('Study lost canonical bounds: ' + str((lo, hi)))
    if digest(source) != expected:
        raise RuntimeError('Source master changed during authoring')
    out.mkdir(parents=True, exist_ok=False)
    blend = out / 'pioneer-study05.blend'
    bpy.ops.wm.save_as_mainfile(filepath=str(blend), check_existing=False, compress=True)
    if digest(source) != expected:
        raise RuntimeError('Source master changed after save')
    report = {'schema': 'arkline.pioneer-fidelity-study/v1', 'revision': 'study05',
              'date_utc': '2026-10-15', 'source_master_sha256': expected,
              'source_study05_sha256': digest(blend), 'recipes': recipe_hashes,
              'bounds_body_m': [lo, hi], 'parts_before': len(before),
              'parts_after': len(after), 'removed_parts': removed,
              'added_parts': sorted(set(after) - set(before)),
              'retained_material_changes': changed_materials, 'operations': reports,
              'scope': 'Editable inferred visual architecture; no fitted interior, '
                       'operating crane/hatch, propulsion or engineering rating.'}
    (out / 'study05.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'source': str(blend), 'sha256': digest(blend),
                      'parts': len(after), 'bounds': [lo, hi]}))


if __name__ == '__main__':
    main()
