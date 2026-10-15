"""Open the existing aft caps around the existing static drive bells.

Study05 source-authoring operation only. Dimensions are inferred modeling
choices within the existing hull; this does not add a propulsion simulation.
"""
import math


def apply(bpy):
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree

    def body(v):
        return Vector((v.x, v.z, -v.y))

    def cut(obj, center_z):
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=48, radius=2.96, depth=7.0,
            location=(-35.3, -center_z, 0.0),
            rotation=(0.0, math.pi / 2, 0.0))
        cutter = bpy.context.object
        cutter.name = 'Study05 temporary aft aperture cutter'
        bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
        bpy.context.view_layer.objects.active = obj
        modifier = obj.modifiers.new('Study05 true drive opening', 'BOOLEAN')
        modifier.operation = 'DIFFERENCE'
        modifier.solver = 'EXACT'
        modifier.object = cutter
        while obj.modifiers.find(modifier.name) > 0:
            bpy.ops.object.modifier_move_up(modifier=modifier.name)
        bpy.ops.object.modifier_apply(modifier=modifier.name)
        bpy.data.objects.remove(cutter, do_unlink=True)

    changed = []
    hull = bpy.data.objects['Pressure hull']
    for label, z, nacelle in [
        ('port', -12.5, 'Outboard drive nacelle'),
        ('starboard', 12.5, 'Outboard drive nacelle.001'),
    ]:
        for obj in (hull, bpy.data.objects[nacelle]):
            cut(obj, z)
            if obj.name not in changed:
                changed.append(obj.name)
        # A closed, faceted annular rim provides an actual reveal at the cut
        # boundary. It does not cover the existing recessed bell or throat.
        verts = []
        profile = [(-36.0, 3.16), (-35.76, 3.16),
                   (-35.76, 2.93), (-36.0, 2.93)]
        for x, radius in profile:
            for i in range(48):
                angle = math.tau * i / 48
                verts.append((x, -(z + math.cos(angle) * radius),
                              math.sin(angle) * radius))
        faces = []
        for ring in range(4):
            nxt = (ring + 1) % 4
            for i in range(48):
                j = (i + 1) % 48
                faces.append((ring * 48 + i, ring * 48 + j,
                              nxt * 48 + j, nxt * 48 + i))
        # Orient only this newly authored closed rim from its signed volume.
        volume = 0.0
        for face in faces:
            a = Vector(verts[face[0]])
            for k in range(1, len(face) - 1):
                volume += a.dot(Vector(verts[face[k]]).cross(
                    Vector(verts[face[k + 1]]))) / 6
        if volume < 0:
            faces = [tuple(reversed(face)) for face in faces]
        name = 'Aft drive aperture rim ' + label
        mesh = bpy.data.meshes.new(name)
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.collection.objects.link(obj)
        obj.data.materials.append(bpy.data.materials['Titanium machinery'])

    bpy.context.view_layer.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    proof = []
    for z in (-12.5, 12.5):
        hits = []
        for obj in bpy.context.scene.objects:
            if obj.type != 'MESH':
                continue
            evaluated = obj.evaluated_get(depsgraph)
            mesh = evaluated.to_mesh()
            verts = [body(evaluated.matrix_world @ v.co) for v in mesh.vertices]
            tree = BVHTree.FromPolygons(
                verts, [list(p.vertices) for p in mesh.polygons],
                all_triangles=False)
            hit, normal, _, distance = tree.ray_cast(
                Vector((-40.0, 0.0, z)), Vector((1.0, 0.0, 0.0)), 12.0)
            if hit is not None:
                hits.append({'part': obj.name, 'position_body_m': list(hit),
                             'normal_body': list(normal), 'distance_m': distance})
            evaluated.to_mesh_clear()
        hits.sort(key=lambda row: row['distance_m'])
        assert hits and hits[0]['part'].startswith('Drive throat'), hits[:3]
        assert -33.3 < hits[0]['position_body_m'][0] < -33.0
        proof.append({'drive_z_m': z, 'centerline_hits': hits[:5]})
    return {
        'changed_parts': changed,
        'added_parts': ['Aft drive aperture rim port',
                        'Aft drive aperture rim starboard'],
        'opening_radius_m': 2.96,
        'existing_drive_centers_body_m': [[-35.9, 0, -12.5], [-35.9, 0, 12.5]],
        'centerline_visibility_proof': proof,
        'scope': 'Real static source cap openings and rims; existing bells/throats '
                 'retained. Aperture dimensions are inferred authoring choices, '
                 'not a verified propulsion or structural specification.',
    }
