#!/usr/bin/env python3
"""Mesh an isolated, protocol-validated canonical-fit enclosure export.

Historical input is the retained manifest's canonical_fit object. Its generating
example lives at commit 63caf70a532228fa4f4abe74e6cc71b0ae83ac26, not current main;
see docs/ships/pioneer/canonical-enclosure-proof-20261022.md for reproduction.
No hand-copied seat, deck, portal or instrument transforms; no world writes.
Uses the existing study palette unchanged. Glass is omitted from this opaque
geometry preview so it cannot masquerade as an exterior scene.
"""
import argparse
import json
from pathlib import Path
import forward_bay_proof as proof


def meshes(source, merge=True):
    result = []
    for panel in source['panels']:
        if panel['material'] == 'Glass':
            continue
        material = (2 if panel['material'] == 'Titanium' else
                    0 if panel['material'] in ('FittedBulkhead', 'WallPanel') else 1)
        mesh = proof.Mesh(panel['id'], material)
        center, half, axes = panel['center'], panel['half'], panel['axes']
        def point(signs):
            return tuple(center[k] + sum(signs[a]*half[a]*axes[a][k] for a in range(3))
                         for k in range(3))
        for axis in range(3):
            other = [a for a in range(3) if a != axis]
            for sign in (-1, 1):
                points = []
                for u, v in ((-1, -1), (-1, 1), (1, 1), (1, -1)):
                    signs = [0, 0, 0]
                    signs[axis], signs[other[0]], signs[other[1]] = sign, u, v
                    points.append(point(signs))
                mesh.face(points, tuple(sign*x for x in axes[axis]))
        result.append(mesh)
    if not merge:
        return result
    # Preserve all exact vertices/frames in the source inventory while fitting
    # the existing import object budget. Do not relax runtime admission limits.
    groups = {}
    for mesh in result:
        identity = (mesh.identity if mesh.identity.startswith('candidate/') or
                    mesh.identity in ('cockpit/aft-crosswalk', 'cockpit/seat-floor', 'cockpit/port-walkway') else
                    ('reference.chair.' if mesh.identity.startswith('pilot-chair/') else
                     'reference.canonical.') + str(mesh.material))
        group = groups.setdefault(identity, proof.Mesh(identity, mesh.material))
        offset = len(group.positions)
        group.positions.extend(mesh.positions)
        group.normals.extend(mesh.normals)
        group.indices.extend(i + offset for i in mesh.indices)
    return list(groups.values())


def write(source, output):
    assert source['original_panels_supports_portals_unchanged'] is True
    primary = next(m for m in source['instruments'] if m['slot'] == 3)
    instrument = dict(primary, canvas=[560, 380])
    proof.write(output, meshes(source), instrument, {
        'canonical_fit': source,
        'proof_eye': source['proof_eye'],
        'deck_component_id': 'cockpit/aft-crosswalk',
        'remaining_validation': 'Isolated fit overlay, not adopted runtime geometry; opaque palette preview without exterior or live UI.',
    })


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    write(json.loads(args.source.read_text()), args.output)
