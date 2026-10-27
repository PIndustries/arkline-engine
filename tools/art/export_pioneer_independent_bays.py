#!/usr/bin/env python3
"""Explicit, unregistered independent-bay fit export. Never retarget old master."""
import argparse,json,hashlib,sys
from pathlib import Path
import export_pioneer as exporter
from pioneer_export_contract import serialize,canonical_json,require,file_hash,LIMITS

def main():
    p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('manifest',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    source=a.source.resolve(strict=True);manifest=a.manifest.resolve(strict=True);out=a.output.resolve()
    require(not out.exists(),'new output directory required')
    doc=json.loads(manifest.read_text());sha=file_hash(source)
    require(doc['output_sha256']==sha,'authoring/source binding mismatch')
    bounds=[[-42.25,-9.,-26.2],[36.,11.35,25.85]]
    # Old exporter default remains the immutable reg12 envelope. Only this
    # explicit candidate path selects a different, bounded source envelope.
    parts,mats,identity,evaluation=exporter.extract_saved_master(source,sha,bounds)
    data,mapping,dropped,validation=serialize(parts,mats,'drop')
    require(all(abs(validation['body_bounds_m'][e][i]-bounds[e][i])<.001 for e in range(2) for i in range(3)), 'retained candidate bounds mismatch')
    out.mkdir();(out/'pioneer.glb').write_bytes(data)
    receipt={'scope':'unregistered independent-bay fit, not gameplay adoption','source_sha256':sha,
             'authoring_sha256':file_hash(manifest),'output':validation,'evaluation':evaluation,
             'source_parts':mapping,'material_identity':identity,'dropped':dropped,
             'limits':LIMITS,'legacy_export_default_unchanged':True}
    (out/'receipt.json').write_bytes(canonical_json(receipt))
    require(file_hash(source)==sha,'source changed')
    print(json.dumps(validation))
if __name__=='__main__':main()
