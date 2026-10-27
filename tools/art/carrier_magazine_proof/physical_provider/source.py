"""Strict source loading, occupied CSG cells and source-preserving extraction."""
import hashlib
import itertools
import json
import struct
import numpy as np


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def read_pinned(root, asset):
    path = root / asset['path']
    require(sha(path) == asset['sha256'], f"source mismatch: {asset['path']}")
    return path


def glb_primitives(path):
    data = path.read_bytes()
    require(struct.unpack_from('<4sII', data) == (b'glTF', 2, len(data)), 'GLB header')
    size, kind = struct.unpack_from('<I4s', data, 12)
    require(kind == b'JSON', 'GLB JSON')
    doc = json.loads(data[20:20+size])
    n, kind = struct.unpack_from('<I4s', data, 20+size)
    blob = data[28+size:]
    require(kind == b'BIN\0' and n == len(blob), 'GLB binary')
    require(len(doc['nodes']) == 1 and doc['nodes'][0]['mesh'] == 0
            and set(doc['nodes'][0]) <= {'mesh', 'name'}, 'unexpected transform')
    require(len(doc['meshes']) == 1, 'unexpected meshes')
    def acc(i, pos):
        a = doc['accessors'][i]; v = doc['bufferViews'][a['bufferView']]
        require('sparse' not in a and not a.get('normalized', False), 'sparse accessor')
        require(v['buffer'] == 0, 'foreign buffer')
        if pos:
            require(a['componentType'] == 5126 and a['type'] == 'VEC3', 'position format')
            fmt = '<3f'
        else:
            require(a['type'] == 'SCALAR' and a['componentType'] in (5121,5123,5125), 'index format')
            fmt = {5121:'<B',5123:'<H',5125:'<I'}[a['componentType']]
        width=struct.calcsize(fmt); stride=v.get('byteStride',width); off=a.get('byteOffset',0)
        require(stride>=width and off>=0 and a['count']>0, 'accessor extent')
        require(off+(a['count']-1)*stride+width<=v['byteLength'], 'accessor bounds')
        require(v.get('byteOffset',0)+v['byteLength']<=len(blob), 'buffer bounds')
        return np.array([struct.unpack_from(fmt,blob,v.get('byteOffset',0)+off+j*stride) for j in range(a['count'])])
    result=[]
    for p in doc['meshes'][0]['primitives']:
        require(p.get('mode',4)==4, 'triangle mode')
        pos=acc(p['attributes']['POSITION'],True)
        indices=acc(p['indices'],False).reshape(-1) if 'indices' in p else np.arange(len(pos))
        require(len(indices)%3==0 and np.all(indices>=0) and np.all(indices<len(pos)), 'index bounds')
        t=pos[indices].reshape(-1,3,3)
        require(np.isfinite(t).all(), 'nonfinite triangle')
        result.append(t)
    return result


def occupied_cells(part):
    """Disjoint exact f32 box-union-minus-void cells, not filled bore AABBs."""
    outer=[np.asarray(x,dtype=np.float32).astype(float) for x in part['outer_boxes']]
    holes=[np.asarray(x,dtype=np.float32).astype(float) for x in part['void_boxes']]
    axes=[sorted(set(b[s,k] for b in outer+holes for s in (0,1))) for k in range(3)]
    def inside(c, boxes):
        return any(np.all(c>b[0]) and np.all(c<b[1]) for b in boxes)
    cells=set()
    for q in itertools.product(*(range(len(a)-1) for a in axes)):
        c=np.array([(axes[k][q[k]]+axes[k][q[k]+1])/2 for k in range(3)])
        if inside(c,outer) and not inside(c,holes):cells.add(q)
    out=[]
    while cells:
        start=min(cells); end=[v+1 for v in start]
        for k in range(3):
            while end[k]<len(axes[k])-1 and all(tuple(q) in cells for q in itertools.product(*(
                range(start[j],end[j]) if j!=k else [end[k]] for j in range(3)))):
                end[k]+=1
        cells.difference_update(itertools.product(*(range(start[k],end[k]) for k in range(3))))
        out.append([[axes[k][start[k]] for k in range(3)], [axes[k][end[k]] for k in range(3)]])
    require(out, 'empty owned material')
    return out


def export_guard(box):
    """Conservative source box under final f32 export, one outward f32 ULP.

    This adds material to collision proxies, never enlarges storage or shrinks
    obstacles. Actual triangle-union coverage remains an independent gate.
    """
    value=np.asarray(box,dtype=np.float32)
    return np.array([np.nextafter(value[0],np.float32(-np.inf)),
                     np.nextafter(value[1],np.float32(np.inf))]).astype(float).tolist()


def local_guard(box, reference, convert):
    """Guard only f64 arithmetic in rigid reference conversion."""
    value=np.asarray(convert(box,reference),dtype=float)
    # Eight machine epsilons at coordinate magnitude bound this fixed matrix
    # product and sum. The source-world f32 guard remains separately explicit.
    guard=8*np.finfo(float).eps*np.maximum(1.,np.abs(value))
    return [np.nextafter(value[0]-guard[0],-np.inf).tolist(),
            np.nextafter(value[1]+guard[1],np.inf).tolist()]


def measured_logical_enclosures(doc, receipt, primitives):
    """Bind nominal legacy leaves to actual triangles within the same render/root.

    Each rendered triangle is enclosed by a named conservative source leaf under
    the SAME rigid owner. Batched render objects may contain many logical leaves.
    Expansion is measured from actual exported vertices, not guessed tolerance.
    New hollow CSG module leaves are handled by their exact material subtraction.
    """
    by={r['name']:r for r in receipt['source_parts']};batches={}
    for p in doc['parts']:
        if p['group'] and 'new_module_part_id' not in p:
            batches.setdefault((p['group'],p['render_semantic_id']),[]).append(p)
    result={};witnesses=[]
    for (group,name),parts in batches.items():
        row=by[name];tri=primitives[row['render_primitive']][row['first_index']//3:(row['first_index']+row['index_count'])//3]
        bounds=np.array([p['bounds_body_m']for p in parts],float)
        expansion=np.zeros(len(parts))
        for index,t in enumerate(tri):
            lo=t.min(0);hi=t.max(0)
            required=np.maximum(0.,np.maximum(bounds[:,0]-lo,hi-bounds[:,1]).max(axis=1))
            owner=int(required.argmin());delta=float(required[owner])
            # A large mismatch means this source mapping must be reviewed, not
            # silently converted into a huge filled proxy or a new source solid.
            require(delta<=.0001, f'non-rounding logical/export mismatch: {group} {name} {delta}')
            if delta>expansion[owner]:
                expansion[owner]=delta
                witnesses.append({'source_part_id':parts[owner]['semantic_id'],'render_name':name,
                    'triangle':index,'measured_enclosure_expansion_m':delta})
        for p,b,delta in zip(parts,bounds,expansion):
            result[p['semantic_id']]=[np.nextafter(b[0]-delta,-np.inf).tolist(),np.nextafter(b[1]+delta,np.inf).tolist()]
    return result,witnesses
