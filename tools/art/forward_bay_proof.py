#!/usr/bin/env python3
"""Original isolated forward-bay mesh proof. Metres: +X forward, +Y up, +Z right.
Standard library only; no canonical assets, saved state or runtime settings.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct


def sub(a, b): return tuple(x-y for x, y in zip(a, b))
def dot(a, b): return sum(x*y for x, y in zip(a, b))
def cross(a, b): return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])


class Mesh:
    def __init__(self, identity, material):
        self.identity, self.material = identity, material
        self.positions, self.normals, self.indices = [], [], []

    def face(self, points, direction):
        for i in range(1, len(points)-1):
            tri = [points[0], points[i], points[i+1]]
            normal = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))
            length = math.sqrt(dot(normal, normal))
            if length < 1e-12:
                raise ValueError('Degenerate triangle in '+self.identity)
            if dot(normal, direction) < 0:
                tri.reverse()
                normal = tuple(-v for v in normal)
            normal = tuple(v/length for v in normal)
            start = len(self.positions)
            self.positions.extend(tri)
            self.normals.extend([normal]*3)
            self.indices.extend((start, start+1, start+2))


    def weighted_normals(self, crease_degrees=35):
        """Area-weight shared broad curves; preserve genuinely sharp bevels."""
        by_position={}
        areas=[]
        for i in range(0,len(self.positions),3):
            a,b,c=self.positions[i:i+3]
            area=cross(sub(b,a),sub(c,a))
            areas.extend([area]*3)
            for j in range(i,i+3):
                key=tuple(round(v,8) for v in self.positions[j])
                by_position.setdefault(key,[]).append(j)
        old=self.normals[:]
        threshold=math.cos(math.radians(crease_degrees))
        for i,position in enumerate(self.positions):
            adjacent=by_position[tuple(round(v,8) for v in position)]
            accepted=[areas[j] for j in adjacent if dot(old[i],old[j])>=threshold]
            total=tuple(sum(n[k] for n in accepted) for k in range(3))
            length=math.sqrt(dot(total,total))
            self.normals[i]=tuple(v/length for v in total)


def octagon(x, center_y, half_z, half_y, cut):
    return [(x, center_y+y, z) for y, z in [
        (-half_y,-half_z+cut),(-half_y,half_z-cut),
        (-half_y+cut,half_z),(half_y-cut,half_z),
        (half_y,half_z-cut),(half_y,-half_z+cut),
        (half_y-cut,-half_z),(-half_y+cut,-half_z)]]


def annular_shell(mesh, center_y, outer, inner):
    outside = [octagon(x,center_y,z,y,c) for x,z,y,c in outer]
    inside = [octagon(x,center_y,z,y,c) for x,z,y,c in inner]
    for rings, sign in [(outside,1),(inside,-1)]:
        for a,b in zip(rings,rings[1:]):
            for i in range(8):
                j=(i+1)%8
                middle=tuple((a[i][k]+a[j][k]+b[i][k]+b[j][k])/4 for k in range(3))
                mesh.face([a[i],a[j],b[j],b[i]],(0,sign*(middle[1]-center_y),sign*middle[2]))
    for a,b,direction in [(outside[0],inside[0],(-1,0,0)),(outside[-1],inside[-1],(1,0,0))]:
        for i in range(8):
            j=(i+1)%8
            mesh.face([a[i],a[j],b[j],b[i]],direction)


def closed_loft(mesh, rings):
    center=tuple(sum(p[k] for ring in rings for p in ring)/sum(map(len,rings)) for k in range(3))
    for a,b in zip(rings,rings[1:]):
        for i in range(len(a)):
            j=(i+1)%len(a)
            middle=tuple((a[i][k]+a[j][k]+b[i][k]+b[j][k])/4 for k in range(3))
            mesh.face([a[i],a[j],b[j],b[i]],sub(middle,center))
    mesh.face(rings[0],sub(rings[0][0],rings[1][0]))
    mesh.face(rings[-1],sub(rings[-1][0],rings[-2][0]))


def section(z, front, back, top, bottom=.28):
    bevel=min(.04,(top-bottom)/4)
    return [(x,y,z) for x,y in [(front+bevel,bottom),(back-bevel,bottom),
        (back,bottom+bevel),(back,top-bevel),(back-bevel,top),
        (front+bevel,top-bevel),(front,top-2*bevel),(front,bottom+bevel)]]


def curve(a,b,c,d,t):
    return tuple(.5*(2*b[i]+(-a[i]+c[i])*t+(2*a[i]-5*b[i]+4*c[i]-d[i])*t*t+
        (-a[i]+3*b[i]-3*c[i]+d[i])*t*t*t) for i in range(len(a)))


def geometry():
    meshes=[]
    def part(identity,material):
        m=Mesh(identity,material)
        meshes.append(m)
        return m
    # Open eight-sided surround: angled window corners and actual bevel depth.
    annular_shell(part('panorama.structural-surround',0),1.375,
        [(1.55,1.56,.965,.16),(1.575,1.58,.985,.17),(1.64,1.58,.985,.17),(1.68,1.56,.965,.16)],
        [(1.55,1.48,.875,.13),(1.59,1.49,.885,.14),(1.68,1.49,.885,.14)])
    annular_shell(part('panorama.metal-reveal',2),1.375,
        [(1.548,1.483,.878,.13),(1.568,1.483,.878,.13)],
        [(1.548,1.468,.863,.13),(1.568,1.468,.863,.13)])
    # The central face is a genuinely open recess, not a slab with a painted UI.
    annular_shell(part('instrument.central-sculpted-case',0),1.0,
        [(.83,.365,.245,.035),(.865,.39,.27,.05),(1.05,.37,.25,.035)],
        [(.83,.325,.205,.014),(.865,.322,.202,.014),(1.05,.315,.190,.012)])
    annular_shell(part('instrument.recessed-liner',2),1.0,
        [(.85,.325,.204,.014),(.882,.324,.203,.014)],
        [(.85,.320,.199,.012),(.882,.319,.198,.012)])
    closed_loft(part('instrument.central-substrate',1),
        [octagon(x,1.0,.302,.177,.004) for x in (.914,.924)])
    # One continuous sculpted dashboard replaces both the isolated stalk and
    # the separate drum. It supports the full instrument width, then sweeps
    # forward and rises into the adjoining console rather than touching at a
    # single disconnected corner.
    controls=[(-.46,.88,1.25,.76),(-.30,.80,1.26,.79),(0.,.77,1.28,.80),
              (.30,.78,1.29,.81),(.52,.69,1.37,.89),(.82,.70,1.48,.84),(1.10,.86,1.60,.73)]
    def sample(points):
        result=[]
        for i in range(len(points)-1):
            for step in range(8):
                result.append(curve(points[max(0,i-1)],points[i],points[i+1],points[min(len(points)-1,i+2)],step/8))
        result.append(points[-1])
        return result
    samples=sample(controls)
    shell=part('console.continuous-dashboard-shell',0)
    closed_loft(shell,[section(*row,bottom=.25) for row in samples])
    shell.weighted_normals()
    # An asymmetric rising shoulder blends the central housing into the side
    # body. It is a broad beveled, closed loft, not a rail or another monitor.
    shoulder=part('console.rising-central-shoulder',0)
    shoulder_controls=[(.345,.88,1.20,1.035),(.42,.84,1.27,1.015),
                       (.56,.75,1.35,.955),(.72,.73,1.43,.875)]
    closed_loft(shoulder,[section(*row,bottom=.72) for row in sample(shoulder_controls)])
    shoulder.weighted_normals()
    # A finite metal seam follows the real front-body curve, adding a true
    # construction break rather than a texture line or a coplanar overlay.
    seam=part('console.front-construction-seam',2)
    seam_rings=[]
    for z,front,back,top in samples:
        seam_rings.append(section(z,front-.004,front+.007,.565,.553))
    closed_loft(seam,seam_rings)
    seam.weighted_normals()
    # The task worktop is seated into the broad shoulder; its underside enters
    # the supporting body, while its upper bevel is a restrained 12 mm step.
    worktop=part('console.starboard-inset-worktop',1)
    worktop_controls=[(.66,.74,1.42,.875),(.84,.72,1.49,.835),(1.055,.84,1.57,.75)]
    closed_loft(worktop,[section(z,front+.075,back-.075,top+.012,top-.06)
        for z,front,back,top in sample(worktop_controls)])
    worktop.weighted_normals()
    return meshes


def write(output):
    meshes=geometry()
    instrument={'center':[.932,1.0,0],'half':[.02,.183,.308],
        'axes':[[1,0,0],[0,1,0],[0,0,1]],'plane_x':.900,'canvas':[400,240],
        'id':'pioneer.study.central.primary','size_m':[.600,.350],
        'backing_front_x':.914,'content_policy':'Recorded or authoritative data only; no modeled telemetry'}
    materials=[
        {'name':'study.ivory-coated-alloy','pbrMetallicRoughness':{'baseColorFactor':[.62,.58,.48,1],'metallicFactor':.15,'roughnessFactor':.55}},
        {'name':'study.graphite-polymer','pbrMetallicRoughness':{'baseColorFactor':[.035,.05,.055,1],'metallicFactor':.05,'roughnessFactor':.7}},
        {'name':'study.satin-metal','pbrMetallicRoughness':{'baseColorFactor':[.22,.25,.27,1],'metallicFactor':.8,'roughnessFactor':.35}}]
    doc={'asset':{'version':'2.0','generator':'Arkline original authored forward-bay proof'},
        'scene':0,'scenes':[{'nodes':list(range(len(meshes)))}],'nodes':[],'meshes':[],
        'materials':materials,'accessors':[],'bufferViews':[],'buffers':[],
        'extras':{'studyOnly':True,'instrument':instrument,'units':'metres','forward':'+X','up':'+Y','right':'+Z'}}
    blob=bytearray()
    def accessor(values,fmt,kind,component,target,bounds=False):
        blob.extend(b'\0'*(-len(blob)%4))
        offset=len(blob)
        flat=[v for row in values for v in row] if kind=='VEC3' else values
        blob.extend(struct.pack('<'+fmt*len(flat),*flat))
        view=len(doc['bufferViews'])
        doc['bufferViews'].append({'buffer':0,'byteOffset':offset,'byteLength':len(blob)-offset,'target':target})
        item={'bufferView':view,'componentType':component,'count':len(values),'type':kind}
        if bounds:
            item['min']=[min(p[i] for p in values) for i in range(3)]
            item['max']=[max(p[i] for p in values) for i in range(3)]
        doc['accessors'].append(item)
        return len(doc['accessors'])-1
    components=[]
    for index,m in enumerate(meshes):
        if not all(math.isfinite(v) for p in m.positions+m.normals for v in p):
            raise ValueError('Nonfinite authored geometry')
        pos=accessor(m.positions,'f','VEC3',5126,34962,True)
        normal=accessor(m.normals,'f','VEC3',5126,34962)
        indices=accessor(m.indices,'I','SCALAR',5125,34963)
        bounds=doc['accessors'][pos]
        component={'id':m.identity,'node':index,'mesh':index,'bounds_min':bounds['min'],'bounds_max':bounds['max']}
        components.append(component)
        doc['meshes'].append({'name':m.identity,'primitives':[{'attributes':{'POSITION':pos,'NORMAL':normal},'indices':indices,'material':m.material,'mode':4}],
            'extras':{'componentId':m.identity}})
        doc['nodes'].append({'name':m.identity,'mesh':index,'extras':{'componentId':m.identity}})
    doc['buffers']=[{'byteLength':len(blob)}]
    encoded=json.dumps(doc,separators=(',',':')).encode()
    encoded+=b' '*(-len(encoded)%4)
    blob.extend(b'\0'*(-len(blob)%4))
    glb=struct.pack('<III',0x46546c67,2,12+8+len(encoded)+8+len(blob))
    glb+=struct.pack('<II',len(encoded),0x4e4f534a)+encoded+struct.pack('<II',len(blob),0x004e4942)+blob
    if len(glb)>16*1024*1024 or len(meshes)>128:
        raise ValueError('Proof budget exceeded')
    output.mkdir(parents=True,exist_ok=True)
    asset=output/'forward-bay-proof.glb'
    asset.write_bytes(glb)
    report={'study_only':True,'glb':'forward-bay-proof.glb','sha256':hashlib.sha256(glb).hexdigest(),
        'byte_length':len(glb),'mesh_count':len(meshes),'material_count':len(materials),
        'triangle_count':sum(len(m.indices)//3 for m in meshes),'instrument':instrument,'components':components,
        'proof_eye':[0,1.35,0],'coordinate_frame':'+X forward,+Y up,+Z right; metres',
        'remaining_validation':'Runtime matched-view appearance, readability and occlusion; not canonical or shipped.'}
    (output/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['glb','sha256','byte_length','mesh_count','material_count','triangle_count']}))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=Path(__file__).resolve().parents[2]/'assets/studies/pioneer-forward-bay-proof')
    write(parser.parse_args().output)
