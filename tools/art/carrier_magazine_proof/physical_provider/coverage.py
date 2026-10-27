"""Exact triangle-minus-box-union coverage, including triangles bridging voids."""
from fractions import Fraction as F


def area_positive(poly):
    if len(poly)<3:return False
    a=poly[0]
    for b,c in zip(poly[1:],poly[2:]):
        u=[b[i]-a[i]for i in range(3)];v=[c[i]-a[i]for i in range(3)]
        if any(u[(i+1)%3]*v[(i+2)%3]-u[(i+2)%3]*v[(i+1)%3]for i in range(3)):return True
    return False


def clip(poly, axis, cut, greater):
    if not poly:return []
    def inside(p):return p[axis]>=cut if greater else p[axis]<=cut
    out=[]
    for a,b in zip(poly,poly[1:]+poly[:1]):
        ia,ib=inside(a),inside(b)
        if ia:out.append(a)
        if ia!=ib:
            t=(cut-a[axis])/(b[axis]-a[axis])
            out.append(tuple(a[i]+t*(b[i]-a[i])for i in range(3)))
    return out


def triangle_union_covers(triangle, boxes):
    todo=[[tuple(F(float(x))for x in p)for p in triangle]]
    for box in boxes:
        lo,hi=[[F(float(x))for x in p]for p in box]
        remaining=[]
        for poly in todo:
            inside=poly
            for k in range(3):
                # Outside pieces are disjoint except zero-area shared edges.
                for cut,greater in [(lo[k],True),(hi[k],False)]:
                    outside=clip(inside,k,cut,not greater)
                    # A face exactly on the clipping plane belongs to the box,
                    # not both outside and inside.
                    if area_positive(outside) and any((p[k]<cut if greater else p[k]>cut)for p in outside):
                        remaining.append(outside)
                    inside=clip(inside,k,cut,greater)
                    if not area_positive(inside):break
                if not area_positive(inside):break
        todo=remaining
        if not todo:return True
    return not any(area_positive(p)for p in todo)
