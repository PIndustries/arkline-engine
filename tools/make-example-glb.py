"""Create the original one-meter calibration cube used by the contribution guide."""
import json
import struct
from pathlib import Path
positions = [(x, y, z) for z in (0, 1) for y in (0, 1) for x in (0, 1)]
indices = [0,2,1,1,2,3,4,5,6,5,7,6,0,1,4,1,5,4,2,6,3,3,6,7,0,4,2,2,4,6,1,3,5,3,7,5]
binary = b''.join(struct.pack('<3f', *p) for p in positions) + struct.pack('<36H', *indices)
doc = {'asset': {'version': '2.0'}, 'scene': 0, 'scenes': [{'nodes': [0]}],
       'nodes': [{'name': 'calibration_cube', 'mesh': 0}],
       'meshes': [{'primitives': [{'attributes': {'POSITION': 0}, 'indices': 1}]}],
       'buffers': [{'byteLength': len(binary)}],
       'bufferViews': [{'buffer': 0, 'byteOffset': 0, 'byteLength': 96},
                       {'buffer': 0, 'byteOffset': 96, 'byteLength': 72}],
       'accessors': [{'bufferView': 0, 'componentType': 5126, 'count': 8,
                      'type': 'VEC3', 'min': [0,0,0], 'max': [1,1,1]},
                     {'bufferView': 1, 'componentType': 5123, 'count': 36, 'type': 'SCALAR'}]}
data = json.dumps(doc, separators=(',', ':')).encode()
data += b' ' * ((-len(data)) % 4)
result = (b'glTF' + struct.pack('<II', 2, 28 + len(data) + len(binary))
          + struct.pack('<I', len(data)) + b'JSON' + data
          + struct.pack('<I', len(binary)) + b'BIN\0' + binary)
Path(__file__).resolve().parents[1].joinpath('assets/examples/calibration-cube.glb').write_bytes(result)
