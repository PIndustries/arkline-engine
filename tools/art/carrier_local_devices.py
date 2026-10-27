"""Composable local-loader hardware; call before source batching/export.
No registration/admission is conferred. Coordinates are body metres. Device
parents must resolve authoritative current trolley roots, never documentary eyes.
"""
import math


def definitions(reference_poses):
    result=[]
    for cell in range(10):
        parent=f'cell{cell:02}/loader/trolley'
        pose=reference_poses[parent]
        assert pose['orientation_xyzw']==[0.,0.,0.,1.]
        root=pose['position_m']; side='port' if cell<5 else 'starboard'; slot=cell%5
        result.append({'cell_id':cell,'parent_id':parent,'reference_position_m':root,
            'camera_id':f'pioneer.carrier.camera.loader.{side}.{slot}',
            'warning_id':f'pioneer.carrier.warning.loader.{side}.{slot}',
            'eye_parent_m':[1.,7.7,0.],'look_at_parent_m':[0.,3.5,0.],
            'vertical_fov_degrees':110.,'near_m':.03,'far_m':100.,
            'supply_model':'unmetered','health_model':'not_simulated',
            'normal_admission':False})
    return result


def append_hardware(doc):
    """Requires Blender5.0.1; append independent source objects/part records.
    Returns device definitions, whose source hash is assigned ONLY after export.
    All hardware is rigid to its actual trolley; beacon modulation is runtime
    presentation of actual door/loading/fault state, not a machinery animation.
    """
    import bpy,bmesh
    from mathutils import Vector
    assert bpy.app.version==(5,0,1)
    devices=definitions(doc['mechanism_reference_poses'])
    faces=[(0,1,2,3),(4,7,6,5),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
    f=Vector((-1.,-4.2,0.)).normalized();right=f.cross(Vector((0.,1.,0.))).normalized();up=right.cross(f)
    axes=[f,up,right];identity=[Vector((1,0,0)),Vector((0,1,0)),Vector((0,0,1))]
    for d in devices:
        root=Vector(d['reference_position_m']); eye=Vector(d['eye_parent_m']);c=eye-f*.12
        pieces=[('camera housing',c,[.10,.065,.075],axes,'Graphite ceramic armor'),
          ('camera lens',eye-f*.018,[.002,.045,.05],axes,'Recessed carbon'),
          ('camera bracket stem',Vector((c.x,(c.y+7.95)/2,0)),[.025,(7.95-c.y)/2,.025],identity,'Titanium machinery'),
          ('camera bracket plate',Vector((c.x,7.94,0)),[.07,.01,.06],identity,'Titanium machinery'),
          ('amber beacon base',Vector((-1,7.92,0)),[.07,.03,.07],identity,'Titanium machinery'),
          ('amber beacon lens',Vector((-1,7.81,0)),[.08,.08,.08],identity,'Safety amber')]
        for label,c,h,ax,material in pieces:
            name=f"pioneer.magazine_ready/cell{d['cell_id']:02}/devices/{label}"
            assert name not in bpy.data.objects
            points=[root+c+sum((ax[k]*h[k]*s[k] for k in range(3)),Vector()) for s in [(-1,-1,-1),(-1,-1,1),(-1,1,1),(-1,1,-1),(1,-1,-1),(1,-1,1),(1,1,1),(1,1,-1)]]
            mesh=bpy.data.meshes.new(name+' mesh');mesh.from_pydata([(v.x,-v.z,v.y) for v in points],[],faces);mesh.update()
            bm=bmesh.new();bm.from_mesh(mesh);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(mesh);bm.free()
            obj=bpy.data.objects.new(name,mesh);bpy.context.collection.objects.link(obj);mesh.materials.append(bpy.data.materials[material]);obj['semantic_id']=name;obj['kinematic_group']=d['parent_id']
            doc['parts'].append({'semantic_id':name,'group':d['parent_id'],'material':material,'bounds_body_m':[[min(v[k] for v in points) for k in range(3)],[max(v[k] for v in points) for k in range(3)]]})
    doc['mounted_loader_devices']=devices
    return devices
