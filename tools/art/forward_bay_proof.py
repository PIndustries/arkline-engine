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
    outside = [row if isinstance(row[0],tuple) else octagon(row[0],center_y,row[1],row[2],row[3]) for row in outer]
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
    def prism(identity,material,x0,x1,section0,section1=None):
        # Separate convex structural members retain useful conservative bounds.
        closed_loft(part(identity,material),[
            [(x0,y,z) for y,z in section0],
            [(x1,y,z) for y,z in (section1 or section0)]])
    def box(identity,material,lo,hi):
        x0,y0,z0=lo; x1,y1,z1=hi
        prism(identity,material,x0,x1,[(y0,z0),(y0,z1),(y1,z1),(y1,z0)])
    def sample(points):
        result=[]
        for i in range(len(points)-1):
            for step in range(8):
                result.append(curve(points[max(0,i-1)],points[i],points[i+1],points[min(len(points)-1,i+2)],step/8))
        result.append(points[-1])
        return result

    # The forward aperture now meets a raked roof instead of an unbounded sky
    # opening. Its lower sill and width retain the original instrument context.
    annular_shell(part('panorama.structural-surround',0),1.255,
        [(1.55,1.56,.845,.16),(1.575,1.58,.865,.17),
         (1.64,1.58,.865,.17),(1.68,1.56,.845,.16)],
        [(1.55,1.48,.755,.13),(1.59,1.49,.765,.14),(1.68,1.49,.765,.14)])
    annular_shell(part('panorama.metal-reveal',2),1.255,
        [(1.548,1.483,.758,.13),(1.568,1.483,.758,.13)],
        [(1.548,1.468,.743,.13),(1.568,1.468,.743,.13)])

    # Same glass and input plane, with a 25 mm recess and a restrained tapered
    # case. The old rear flare/large shoulder no longer enlarges the display box.
    annular_shell(part('instrument.central-sculpted-case',0),1.0,
        [(.875,.334,.214,.018),(.895,.338,.218,.020),
         (1.055,.327,.207,.018)],
        [(.875,.325,.205,.014),(.895,.322,.202,.014),(1.055,.315,.190,.012)])
    annular_shell(part('instrument.recessed-liner',2),1.0,
        [(.887,.325,.204,.014),(.897,.324,.203,.014)],
        [(.887,.320,.199,.012),(.897,.319,.198,.012)])
    closed_loft(part('instrument.central-substrate',1),
        [octagon(x,1.0,.302,.177,.004) for x in (.914,.924)])

    # A shallow continuous U-shaped desk joins both jambs. The under-desk volume
    # is open rather than a solid drum; three real webs take it to the deck.
    controls=[(-1.48,1.18,1.64,.78),(-1.20,.98,1.64,.78),
              (-.85,.89,1.58,.76),(-.50,.82,1.47,.78),
              (-.30,.80,1.30,.79),(0.,.77,1.28,.80),
              (.30,.80,1.30,.79),(.50,.82,1.47,.78),
              (.85,.89,1.58,.76),(1.20,.98,1.64,.78),(1.48,1.18,1.64,.78)]
    shell=part('console.continuous-dashboard-shell',0)
    closed_loft(shell,[section(*row,bottom=.56) for row in sample(controls)])
    shell.weighted_normals()
    for side,sign in [('starboard',1),('port',-1)]:
        shoulder=part('console.rising-central-shoulder' if sign==1 else 'console.port-shoulder',0)
        rows=[(.32,.89,1.24,.825),(.45,.87,1.36,.82),(.64,.88,1.48,.80)]
        closed_loft(shoulder,[section(sign*z,front,back,top,bottom=.75)
            for z,front,back,top in sample(rows)])
        shoulder.weighted_normals()
        worktop=part('console.starboard-inset-worktop' if sign==1 else 'console.port-inset-worktop',1)
        rows=[(.67,.90,1.52,.775),(.98,.96,1.59,.79),(1.38,1.16,1.61,.79)]
        closed_loft(worktop,[section(sign*z,front+.045,back-.035,top,top-.025)
            for z,front,back,top in sample(rows)])
        worktop.weighted_normals()
        z0,z1=sorted((sign*1.26,sign*1.52))
        box('console.'+side+'-deck-web',0,(1.26,0.,z0),(1.61,.60,z1))
    box('console.central-deck-web',1,(1.22,0.,-.20),(1.48,.59,.20))

    # Connected human-scale enclosure, open at the rear for study access. These
    # are visual structural members, not registered hull/collision replacements.
    box('enclosure.deck',1,(-1.55,-.08,-1.60),(1.68,0.,1.60))
    prism('enclosure.raked-overhead',1,-1.55,1.56,
        [(2.40,-1.32),(2.40,1.32),(2.48,1.32),(2.48,-1.32)],
        [(2.09,-1.32),(2.09,1.32),(2.17,1.32),(2.17,-1.32)])
    for side,sign in [('starboard',1),('port',-1)]:
        def yz(points): return [(y,sign*z) for y,z in points]
        prism('enclosure.'+side+'-sidewall',0,-1.55,1.56,
            yz([(.28,1.48),(.28,1.60),(2.24,1.60),(2.18,1.48)]),
            yz([(.28,1.48),(.28,1.60),(1.99,1.60),(1.93,1.48)]))
        prism('enclosure.'+side+'-upper-haunch',0,-1.55,1.56,
            yz([(2.40,1.30),(2.18,1.48),(2.24,1.60),(2.48,1.34)]),
            yz([(2.09,1.30),(1.93,1.48),(1.99,1.60),(2.17,1.34)]))
        prism('enclosure.'+side+'-lower-haunch',0,-1.55,1.56,
            yz([(0.,1.30),(.28,1.48),(.28,1.60),(-.04,1.60)]))
    box('enclosure.forward-sill-apron',0,(1.55,0.,-1.56),(1.68,.44,1.56))
    return meshes


def write(output, meshes=None, instrument=None, metadata=None):
    meshes=geometry() if meshes is None else meshes
    instrument=instrument or {'center':[.932,1.0,0],'half':[.02,.183,.308],
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
    if metadata:
        report.update(metadata)
    (output/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['glb','sha256','byte_length','mesh_count','material_count','triangle_count']}))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=Path(__file__).resolve().parents[2]/'assets/studies/pioneer-forward-bay-proof')
    write(parser.parse_args().output)
