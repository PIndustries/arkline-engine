#!/usr/bin/env python3
"""Export one explicitly selected saved Pioneer master to a new review directory.

This never invokes build_pioneer.py, saves the source, applies modifiers, joins
objects, or recalculates normals. Ordinary Python launches a bounded Blender
worker; --bpy-python also supports the official Blender Python module runtime.
"""
import argparse
import math
import os
from pathlib import Path
import shutil
import secrets
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from pioneer_export_contract import (  # noqa: E402
    ExportError, LIMITS, MATERIAL_NAMES, SCHEMA, affine_and_normal_matrix,
    canonical_json, file_hash, require, serialize, sha256, transform_normal,
    transform_point, validate_glb,
)

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve()
# Unrepresented Principled inputs must retain this Blender 5.0 baseline. They
# are not silently approximated when authored. Weight is Blender's hidden socket.
PRINCIPLED_DEFAULTS = {
    'IOR': 1.5, 'Alpha': 1., 'Normal': (0., 0., 0.), 'Weight': 0.,
    'Diffuse Roughness': 0., 'Subsurface Weight': 0., 'Subsurface Radius': (1., .2, .1),
    'Subsurface Scale': .05, 'Subsurface IOR': 1.4, 'Subsurface Anisotropy': 0.,
    'Specular IOR Level': .5, 'Specular Tint': (1., 1., 1., 1.),
    'Anisotropic': 0., 'Anisotropic Rotation': 0., 'Tangent': (0., 0., 0.),
    'Transmission Weight': 0., 'Coat Weight': 0., 'Coat Roughness': .03,
    'Coat IOR': 1.5, 'Coat Tint': (1., 1., 1., 1.), 'Coat Normal': (0., 0., 0.),
    'Sheen Weight': 0., 'Sheen Roughness': .5, 'Sheen Tint': (1., 1., 1., 1.),
    'Thin Film Thickness': 0., 'Thin Film IOR': 1.33,
}
REPRESENTED_INPUTS = {'Base Color', 'Metallic', 'Roughness', 'Emission Color', 'Emission Strength'}


def recipe_hashes():
    return {path.name: file_hash(path) for path in (SCRIPT, SCRIPT.with_name('pioneer_export_contract.py'))}


def _close(value, expected):
    if isinstance(expected, tuple):
        return len(value) == len(expected) and all(_close(a, b) for a, b in zip(value, expected))
    return math.isfinite(float(value)) and math.isclose(float(value), expected, abs_tol=1e-6, rel_tol=1e-6)


def material_record(material, source_hash):
    require(material.use_nodes and material.node_tree is not None, f'{material.name}: node material required')
    nodes = list(material.node_tree.nodes)
    require(len(nodes) == 2 and {node.bl_idname for node in nodes} ==
            {'ShaderNodeBsdfPrincipled', 'ShaderNodeOutputMaterial'}, f'{material.name}: unsupported shader nodes')
    principled = next(node for node in nodes if node.type == 'BSDF_PRINCIPLED')
    output = next(node for node in nodes if node.type == 'OUTPUT_MATERIAL')
    links = list(material.node_tree.links)
    require(output.is_active_output and output.target == 'ALL' and len(links) == 1,
            f'{material.name}: unsupported material output')
    link = links[0]
    require(link.from_node == principled and link.from_socket == principled.outputs['BSDF']
            and link.to_node == output and link.to_socket == output.inputs['Surface'],
            f'{material.name}: expected direct Principled to Surface')
    require(not any(socket.is_linked for socket in principled.inputs), f'{material.name}: linked shader input')
    sockets = {socket.name: socket for socket in principled.inputs if hasattr(socket, 'default_value')}
    require(set(sockets) == set(PRINCIPLED_DEFAULTS) | REPRESENTED_INPUTS,
            f'{material.name}: unsupported Principled socket/version')
    for name, expected in PRINCIPLED_DEFAULTS.items():
        require(_close(sockets[name].default_value, expected), f'{material.name}: unsupported authored {name}')
    base = [float(x) for x in sockets['Base Color'].default_value]
    emission = [float(x) for x in sockets['Emission Color'].default_value]
    strength = float(sockets['Emission Strength'].default_value)
    require(len(base) == 4 and base[3] == 1 and len(emission) == 4 and emission[3] == 1,
            f'{material.name}: opaque RGBA required')
    require(all(math.isfinite(v) and 0 <= v <= 1 for v in base + emission), f'{material.name}: color outside unit range')
    require(math.isfinite(strength) and 0 <= strength <= 64, f'{material.name}: unsupported emission strength')
    metallic, roughness = float(sockets['Metallic'].default_value), float(sockets['Roughness'].default_value)
    require(all(math.isfinite(x) and 0 <= x <= 1 for x in (metallic, roughness)), f'{material.name}: invalid scalar material')
    render = {'name': material.name, 'alphaMode': 'OPAQUE', 'doubleSided': not material.use_backface_culling,
              'pbrMetallicRoughness': {'baseColorFactor': base, 'metallicFactor': metallic, 'roughnessFactor': roughness},
              'emissiveFactor': emission[:3],
              'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': strength}}}
    identity = {'name': material.name, 'source_id': 'material:' + sha256((source_hash+'\0'+material.name).encode()),
                'render_material_index': MATERIAL_NAMES.index(material.name)}
    return render, identity


def extract_saved_master(source_copy, source_hash):
    import bpy
    require(bpy.app.version == (5, 0, 1), 'this reviewed exporter requires Blender 5.0.1')
    bpy.context.preferences.filepaths.use_scripts_auto_execute = False
    bpy.ops.wm.open_mainfile(filepath=str(source_copy), load_ui=False, use_scripts=False)
    require(not bpy.context.preferences.filepaths.use_scripts_auto_execute, 'auto-execution must be disabled')
    require(Path(bpy.data.filepath).resolve() == source_copy.resolve(), 'loaded source mismatch')
    require(not bpy.data.libraries and not bpy.data.actions, 'linked libraries/actions unsupported')
    require(len(bpy.data.scenes) == 1 and len(bpy.context.scene.view_layers) == 1, 'one scene/view layer required')
    require(bpy.context.scene.unit_settings.scale_length == 1., 'source unit scale must be one metre per Blender unit')
    require(0 < len(bpy.data.objects) <= LIMITS['objects'], 'source object budget exceeded')
    require(set(bpy.data.objects) == set(bpy.context.scene.objects) == set(bpy.context.view_layer.objects),
            'detached, excluded or hidden collection objects unsupported')
    for owner in bpy.data.user_map():
        # Embedded material/world/compositor node trees are not keys in
        # bpy.data.user_map(), but can carry their own socket drivers/actions.
        for block in (owner, getattr(owner, 'node_tree', None)):
            if block is None: continue
            require(block.library is None and block.override_library is None, 'linked/overridden datablock unsupported')
            require(getattr(block, 'animation_data', None) is None, f'{block.name}: animation/drivers unsupported')
    require(all(image.source == 'VIEWER' and image.type == 'RENDER_RESULT' and not image.filepath
                and image.packed_file is None for image in bpy.data.images)
            and not bpy.data.sounds and not bpy.data.movieclips and not bpy.data.cache_files,
            'external image/audio/movie/cache resources unsupported')
    require(all(font.filepath == '<builtin>' for font in bpy.data.fonts), 'external fonts unsupported')
    require(len(bpy.data.materials) == 9 and {m.name for m in bpy.data.materials} == set(MATERIAL_NAMES),
            'exact canonical nine source material names required')
    material_pairs = [material_record(bpy.data.materials[name], source_hash) for name in MATERIAL_NAMES]
    graph = bpy.context.evaluated_depsgraph_get()
    parts, triangles_used = [], 0
    bounds = [[math.inf]*3, [-math.inf]*3]
    for obj in sorted(bpy.data.objects, key=lambda value: value.name):
        require(obj.type == 'MESH' and obj.parent is None and not obj.constraints,
                f'{obj.name}: only unparented static meshes without constraints supported')
        require(obj.instance_type == 'NONE' and not obj.is_instancer and not obj.particle_systems,
                f'{obj.name}: instancing/particles unsupported')
        require(not obj.hide_viewport and not obj.hide_render and not obj.hide_get(), f'{obj.name}: hidden mesh unsupported')
        require(obj.data.shape_keys is None, f'{obj.name}: shape keys unsupported')
        require(len(obj.material_slots) == 1 and obj.material_slots[0].material is not None,
                f'{obj.name}: exactly one assigned material required')
        require(all(mod.type in {'BEVEL', 'WEIGHTED_NORMAL'} and mod.show_viewport and mod.show_render
                    for mod in obj.modifiers), f'{obj.name}: unsupported/disabled modifier')
        evaluated = obj.evaluated_get(graph)
        matrix = [[float(x) for x in row] for row in evaluated.matrix_world]
        normal_matrix = affine_and_normal_matrix(matrix)
        mesh = evaluated.to_mesh(preserve_all_data_layers=True, depsgraph=graph)
        try:
            require(mesh is not None and len(mesh.vertices) <= LIMITS['accessor_count'], f'{obj.name}: evaluated mesh too large')
            mesh.calc_loop_triangles()
            triangles_used += len(mesh.loop_triangles)
            require(triangles_used <= LIMITS['evaluated_triangles'], 'evaluated triangle budget exceeded')
            expected_material = obj.material_slots[0].material
            require(len(evaluated.material_slots) == 1 and evaluated.material_slots[0].material is not None
                    and evaluated.material_slots[0].material.original == expected_material.original
                    and evaluated.material_slots[0].material.name == expected_material.name,
                    f'{obj.name}: evaluated material identity changed')
            require(all(poly.material_index == 0 for poly in mesh.polygons), f'{obj.name}: polygon material index unsupported')
            positions = [transform_point(matrix, vertex.co) for vertex in mesh.vertices]
            normals = [transform_normal(normal_matrix, normal.vector, allow_zero=True) for normal in mesh.corner_normals]
            require(len(normals) == len(mesh.loops), f'{obj.name}: evaluated corner normals unavailable')
            triangles = []
            for triangle in mesh.loop_triangles:
                corners = tuple((positions[mesh.loops[loop].vertex_index], normals[loop]) for loop in triangle.loops)
                triangles.append(corners)
                for position, _ in corners:
                    for i, x in enumerate(position): bounds[0][i] = min(bounds[0][i], x); bounds[1][i] = max(bounds[1][i], x)
            parts.append({'name': obj.name, 'source_id': 'part:' + sha256((source_hash+'\0'+obj.name).encode()),
                          'source_mesh_name': obj.data.name, 'material': obj.material_slots[0].material.name,
                          'source_world_blender': matrix, 'source_modifiers': [{'name': m.name, 'type': m.type} for m in obj.modifiers],
                          'triangles': triangles})
        finally:
            evaluated.to_mesh_clear()
    expected = [[-36., -9., -19.], [36., 9., 19.]]
    require(all(abs(bounds[end][axis]-expected[end][axis]) <= .001 for end in range(2) for axis in range(3)),
            'evaluated source must retain centered 72x18x38 metre envelope within1mm')
    return parts, [pair[0] for pair in material_pairs], [pair[1] for pair in material_pairs], {
        'blender_version': bpy.app.version_string, 'blender_build_hash': bpy.app.build_hash.decode(),
        'source_file_version': list(bpy.data.version), 'autoexec': False,
        'evaluation': 'saved-frame viewport depsgraph; BEVEL and WEIGHTED_NORMAL; corner_normals',
        'saved_frame': bpy.context.scene.frame_current, 'body_bounds_m': bounds}


def worker(source_copy, source_hash, directory, policy, token):
    import json
    require(directory.is_absolute() and directory.resolve(strict=True) == directory
            and not directory.is_relative_to(ROOT), 'worker output must be canonical and outside repository')
    require(source_copy.parent == directory and not source_copy.is_symlink(), 'worker requires isolated editable source copy')
    marker = directory / '.export-session.json'
    require(marker.is_file() and not marker.is_symlink() and isinstance(token, str) and len(token) == 64,
            'worker must be launched by new-directory export session')
    recipes = recipe_hashes()
    require(json.loads(marker.read_text()) == {'schema': SCHEMA, 'source': source_copy.name,
            'sha256': source_hash, 'degenerate_policy': policy, 'token': token, 'recipes': recipes}, 'worker session mismatch')
    require(not (directory / 'pioneer.glb').exists() and not (directory / 'receipt.json').exists(), 'worker outputs already exist')
    require(source_copy.is_file() and file_hash(source_copy) == source_hash, 'editable copy hash mismatch')
    try:
        parts, materials, material_identity, evaluation = extract_saved_master(source_copy, source_hash)
        output, mapping, dropped, validation = serialize(parts, materials, policy)
        expected = [[-36., -9., -19.], [36., 9., 19.]]
        require(all(abs(validation['body_bounds_m'][end][axis]-expected[end][axis]) <= .001
                    for end in range(2) for axis in range(3)),
                'retained export must preserve centered 72x18x38 metre envelope after degenerate policy')
        receipt = {'schema': SCHEMA, 'status': 'derived-review-candidate-not-adopted',
            'source': {'name': source_copy.name, 'sha256': source_hash, 'size_bytes': source_copy.stat().st_size,
                       'editable_copy': source_copy.name, 'identity_scope': 'source-sha256-and-exact-name snapshot; not durable cross-revision IDs'},
            'exporter': recipes,
            'evaluation': evaluation, 'coordinates': {'units': 'body metres', 'blender_to_body': ['x', 'z', '-y'],
                'normal_transform': 'inverse-transpose evaluated object world linear, same proper body rotation, normalize then float32',
                'winding': 'retained evaluated loop triangle order; cyclic rotations only; negative determinants rejected'},
            'material_identity': material_identity, 'source_parts': mapping, 'degeneracy': dropped,
            'output': {'file': 'pioneer.glb', **validation},
            'limits': LIMITS, 'scope': 'static geometry/material export only; independent Rust decoder/native adoption required'}
        encoded = canonical_json(receipt) + b'\n'
        require(len(encoded) <= LIMITS['receipt_bytes'], 'receipt byte budget exceeded')
        require(file_hash(source_copy) == source_hash, 'editable copy changed during extraction')
        require(recipe_hashes() == recipes, 'export recipe changed during extraction')
        # No file exists until complete in-memory validation. Exclusive writes
        # still fail closed if another process races an output filename.
        with (directory / 'pioneer.glb').open('xb') as handle: handle.write(output)
        with (directory / 'receipt.json').open('xb') as handle: handle.write(encoded)
        print(f"exported {validation['triangles']} triangles; sha256 {validation['sha256']}", flush=True)
    finally:
        require(file_hash(source_copy) == source_hash, 'editable source copy changed')
        require(recipe_hashes() == recipes, 'export recipe changed during extraction')


def resource_limits():
    import resource
    for kind, bound in ((resource.RLIMIT_CPU, LIMITS['cpu_seconds']),
                        (resource.RLIMIT_AS, LIMITS['address_space_bytes']),
                        (resource.RLIMIT_FSIZE, max(LIMITS['receipt_bytes'], LIMITS['glb_bytes'])),
                        (resource.RLIMIT_NOFILE, 256), (resource.RLIMIT_CORE, 0)):
        resource.setrlimit(kind, (bound, bound))


def run_export(source, directory, policy, blender='blender', bpy_python=None):
    require(os.name == 'posix', 'bounded worker currently requires POSIX')
    source, directory = Path(source).absolute(), Path(directory).absolute()
    require(not source.is_symlink() and source.is_file() and source.suffix == '.blend', 'explicit saved regular .blend source required')
    source = source.resolve(strict=True)
    require(0 < source.stat().st_size <= LIMITS['source_bytes'], 'source byte limit exceeded')
    require(not directory.exists() and not directory.is_symlink(), 'output directory already exists; refusing overwrite')
    require(directory.parent.is_dir() and directory.parent.resolve(strict=True) == directory.parent,
            'output parent must be existing canonical directory without symlink components')
    require(not directory.is_relative_to(ROOT), 'output must be outside the source repository')
    before, recipes = file_hash(source), recipe_hashes()
    directory.mkdir(mode=0o700)  # Exclusive creation, including against races.
    copy = directory / source.name
    require(copy.name not in {'pioneer.glb', 'receipt.json', 'worker.log'}, 'source name conflicts with output')
    try:
        with source.open('rb') as src, copy.open('xb') as dst: shutil.copyfileobj(src, dst)
        require(file_hash(copy) == before and file_hash(source) == before, 'source changed while copying')
        token = secrets.token_hex(32)
        marker = directory / '.export-session.json'
        with marker.open('xb') as handle:
            handle.write(canonical_json({'schema': SCHEMA, 'source': copy.name, 'sha256': before,
                                         'degenerate_policy': policy, 'token': token, 'recipes': recipes}))
        args = ['--worker', '--source', str(copy), '--output-dir', str(directory),
                '--source-hash', before, '--degenerate-policy', policy, '--worker-token', token]
        command = ([str(bpy_python), str(SCRIPT), *args] if bpy_python else
                   [str(blender), '--background', '--factory-startup', '--disable-autoexec', '--threads', '2',
                    '--python-exit-code', '1', '--python', str(SCRIPT), '--', *args])
        env = dict(os.environ, OMP_NUM_THREADS='2', OPENBLAS_NUM_THREADS='2', PYTHONNOUSERSITE='1',
                   PYTHONDONTWRITEBYTECODE='1', BLENDER_USER_CONFIG=str(directory / 'blender-config'))
        with (directory / 'worker.log').open('xb') as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, env=env,
                                    timeout=LIMITS['wall_seconds'], preexec_fn=resource_limits, check=False)
        require(result.returncode == 0, f'worker failed ({result.returncode}); inspect {directory / "worker.log"}')
        output = (directory / 'pioneer.glb').read_bytes()
        validation = validate_glb(output)
        require(file_hash(copy) == before, 'editable copy changed')
        marker.unlink()
        print(f"review candidate {directory / 'pioneer.glb'}; sha256 {validation['sha256']}")
        return validation
    except subprocess.TimeoutExpired as exc:
        raise ExportError('bounded export worker exceeded wall time') from exc
    finally:
        require(file_hash(source) == before, 'selected source changed during export (success or failure)')
        require(recipe_hashes() == recipes, 'export recipe changed during worker lifetime')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--degenerate-policy', required=True, choices=('drop', 'reject'))
    runtime = parser.add_mutually_exclusive_group()
    runtime.add_argument('--blender', default='blender')
    runtime.add_argument('--bpy-python', help='Python executable/wrapper with official bpy5.0.1 installed')
    parser.add_argument('--worker', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--source-hash', help=argparse.SUPPRESS)
    parser.add_argument('--worker-token', help=argparse.SUPPRESS)
    args = parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else None)
    try:
        if args.worker:
            require(args.source_hash is not None, 'worker source hash required')
            worker(args.source, args.source_hash, args.output_dir, args.degenerate_policy, args.worker_token)
        else:
            run_export(args.source, args.output_dir, args.degenerate_policy, args.blender, args.bpy_python)
    except (ExportError, OSError) as exc:
        parser.exit(1, f'export failed: {exc}\n')


if __name__ == '__main__':
    main()
