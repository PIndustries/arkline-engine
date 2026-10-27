"""Unadmitted ten-cell adapter of the immutable cell-0 route.

Only aligned Protected N <-> Ready N routes exist. This is a mapping adapter,
not clearance, timing, stock, concurrency or production-admission evidence.
Each snapshot has a GLOBAL state; merge_frames copies only the selected cell's
state, never the other snapshots' initial/unowned cassette poses.
"""
import copy
import re

import candidate_routes as old
import parallel_routes as canonical

_X = (-26., -18., -10., -2., 6.)


def _cell(bid):
    if type(bid) is not int or not 0 <= bid < 10:
        raise ValueError("local cell must be an integer in 0..9")
    return bid


def _point(point, bid, inverse=False):
    dx = _X[bid % 5] + 26.
    return [point[0] - dx if inverse else point[0] + dx,
            point[1], -point[2] if bid >= 5 else point[2]]


def _group(group, bid, inverse=False):
    pairs = (("cell00/", f"cell{bid:02d}/"),
             ("b00/", f"b{bid:02d}/"),
             ("c00/", f"c{bid:02d}/"),
             ("c10/", f"c{bid + 10:02d}/"),
             ("dock/ready/0/", f"dock/ready/{bid}/"),
             ("dock/protected/0/", f"dock/protected/{bid}/"))
    for source, destination in pairs:
        if inverse:
            source, destination = destination, source
        if group.startswith(source):
            return destination + group[len(source):]
    raise ValueError(f"group does not belong to selected local cell {bid}: {group}")


def _owner(group):
    match = re.match(r"(?:cell|b)(\d{2})/", group)
    if match:
        return _cell(int(match[1]))
    match = re.match(r"c(\d{2})/", group)
    if match:
        cid = int(match[1])
        if cid >= 20:
            raise ValueError("unknown finite cassette identity")
        return cid % 10
    match = re.match(r"dock/(?:ready|protected)/(\d+)/", group)
    return _cell(int(match[1])) if match else None


def _validate(doc):
    cells = doc.get("parallel_local_cells", [])
    if len(cells) != 10 or {c["cell_id"] for c in cells} != set(range(10)):
        raise ValueError("requires exactly ten explicitly authored local cells")
    for cell in cells:
        bid = _cell(cell["cell_id"])
        if (type(cell["ready_berth"]) is not int or type(cell["protected_slot"]) is not int
                or cell["ready_berth"] != bid or cell["protected_slot"] != bid):
            raise ValueError("only aligned local cells are supported")
    stock = doc["cassettes"]
    if len(stock) != 20 or {c["cassette_id"] for c in stock} != set(range(20)):
        raise ValueError("requires exactly twenty finite source cassette identities")
    for cassette in stock:
        cid = cassette["cassette_id"]
        if type(cid) is not int:
            raise ValueError("cassette identity must be an integer")
        bid = cid % 10
        expected = _point([-26., 4.8 if cid >= 10 else 0.,
                           -30. if cid >= 10 else -22.8], bid)
        if list(cassette["body_center_m"]) != expected:
            raise ValueError("cassette initial pose does not match the aligned source cell")
    refs = doc["mechanism_reference_poses"]
    for bid in range(10):
        for prefix in (f"cell{bid:02d}/", f"b{bid:02d}/", f"c{bid:02d}/",
                       f"c{bid+10:02d}/", f"dock/ready/{bid}/", f"dock/protected/{bid}/"):
            if not any(g.startswith(prefix) for g in refs):
                raise ValueError(f"missing source-owned groups: {prefix}")


def initial(doc):
    """Global idle snapshot; local coordinates are world metre coordinates."""
    _validate(doc)
    state = old.initial(doc)
    template = canonical.initial(doc)["local"]
    state["locals"] = {}
    state["launch_groups"] = []
    for bid in range(10):
        local = copy.deepcopy(template)
        if bid >= 5:
            local["z"] *= -1
            local["shuttle_z"] *= -1
        state["locals"][str(bid)] = local
    return state


def _canonical_doc(doc, bid):
    """Only canonical Stage's required fields, with an exact inverse mapping."""
    refs = {}
    for name, ref in doc["mechanism_reference_poses"].items():
        if _owner(name) == bid:
            value = copy.deepcopy(ref)
            value["position_m"] = _point(value["position_m"], bid, inverse=True)
            refs[_group(name, bid, inverse=True)] = value
    return {"mechanism_reference_poses": refs,
            "cassettes": [{"cassette_id": cid, "body_center_m": [-26., y, z]}
                          for cid, y, z in [(0, 0., -22.8), (10, 4.8, -30.)]]}


def stage(doc, source_slot=0, destination_berth=0):
    """Map the current canonical route; never invent an any-to-any transfer."""
    bid = _cell(source_slot)
    if type(destination_berth) is not int or destination_berth != bid:
        raise ValueError("parallel bank supports only source_slot == destination_berth")
    base = initial(doc)
    template = canonical.stage(_canonical_doc(doc, bid))
    frames = []
    cid_map = {0: bid, 10: bid + 10}
    for original in template:
        frame = copy.deepcopy(original)
        state = copy.deepcopy(base)
        local = copy.deepcopy(original["state"]["local"])
        if bid >= 5:
            local["z"] *= -1
            local["shuttle_z"] *= -1
        state["locals"][str(bid)] = local
        for source_cid, target_cid in cid_map.items():
            state["cassettes"][str(target_cid)] = _point(
                original["state"]["cassettes"][str(source_cid)], bid)
        for key in ("hatches", "doors"):
            state[key] = {str(bid): value for source, value in original["state"][key].items()
                          if source == "0"}
        for key in ("cradles", "clamps"):
            state[key] = {str(cid_map[int(source)]): value
                          for source, value in original["state"][key].items()}
        state["docks"] = {f"{kind}/{bid}": value
                          for name, value in original["state"]["docks"].items()
                          for kind, source in [name.split("/")] if source == "0"}
        frame["state"] = state
        frame["cell_id"] = bid
        frame["owned_groups"] = [_group(g, bid) for g in original["owned_groups"]]
        for key in ("handled_cassette_id", "carried_cassette_id"):
            frame[key] = None if original[key] is None else cid_map[original[key]]
        frame["docked_cassettes"] = {
            str(cid_map[int(cid)]): [dock[0], bid]
            for cid, dock in original["docked_cassettes"].items()}
        frames.append(frame)
    return frames


def bay(doc, berth_id, cassette_id=None):
    """Exact legacy Launch trajectory plus idle new local machinery.

    Launch owns only its actual berth/cassette roots, not the entire cell.
    Consequently merge_frames deliberately rejects these subset snapshots.
    Original b/c root orientation (including starboard clamp X) is retained.
    """
    bid = _cell(berth_id)
    cid = bid if cassette_id is None else cassette_id
    if type(cid) is not int or not 0 <= cid < 20 or (cid % 10) // 5 != bid // 5:
        raise ValueError("Launch requires an existing same-side cassette identity")
    base = initial(doc)
    frames = old.bay(doc, bid, cid)
    for frame in frames:
        frame["cell_id"] = bid
        frame["route_kind"] = "launch"
        frame["state"]["locals"] = copy.deepcopy(base["locals"])
        frame["state"]["launch_groups"] = list(frame["owned_groups"])
    return frames


def _canonical_state(state, bid):
    local = dict(state["locals"][str(bid)])
    if bid >= 5:
        local["z"] *= -1
        local["shuttle_z"] *= -1
    return {"local": local,
            "cassettes": {str(cid): _point(state["cassettes"][str(bid+cid)], bid, True)
                          for cid in (0, 10)},
            "hatches": {"0": state["hatches"].get(str(bid), 0.)},
            "doors": {"0": state["doors"].get(str(bid), 0.)},
            "cradles": {str(cid): state["cradles"].get(str(bid+cid), 0.) for cid in (0, 10)},
            "clamps": {str(cid): state["clamps"].get(str(bid+cid), 0.) for cid in (0, 10)},
            "docks": {f"{kind}/0": state["docks"].get(f"{kind}/{bid}", 0.)
                      for kind in ("ready", "protected")}}


def translation(doc, group, state):
    """Translate only this root, dispatching to its cell's independent state.

    Inverse-map to canonical before evaluation, then mirror the returned Z.
    In particular, do NOT also flip X of mirrored cassette clamp motion.
    """
    if not group:
        return [0., 0., 0.]
    if group in state.get("launch_groups", []):
        return old.translation(doc, group, state)
    bid = _owner(group)
    if bid is None:
        return old.translation(doc, group, state)
    cg = _group(group, bid, inverse=True)
    ref = doc["mechanism_reference_poses"][group]
    minimal = {"mechanism_reference_poses": {
        cg: {"position_m": _point(ref["position_m"], bid, inverse=True)}}}
    if cg.startswith("dock/protected/0/"):
        part = next(p for p in doc["parts"] if p.get("group") == group)
        points = [_point(p, bid, inverse=True) for p in part["bounds_body_m"]]
        minimal["parts"] = [{"group": cg, "bounds_body_m": [
            [min(p[i] for p in points) for i in range(3)],
            [max(p[i] for p in points) for i in range(3)]]}]
    delta = canonical.translation(minimal, cg, _canonical_state(state, bid))
    return [delta[0], delta[1], -delta[2] if bid >= 5 else delta[2]]


def merge_frames(doc, frames, base_state=None):
    """Overlay disjoint cell snapshots without resetting any unowned state.

    Returns a new GLOBAL state; callers retain their own per-operation metadata.
    This is a geometry composition helper, not an authority reservation grant.
    """
    result = initial(doc) if base_state is None else copy.deepcopy(base_state)
    seen_cells, seen_groups = set(), set()
    for frame in frames:
        bid = _cell(frame["cell_id"])
        owned = frame["owned_groups"]
        if bid in seen_cells or len(set(owned)) != len(owned) or seen_groups.intersection(owned):
            raise ValueError("overlapping local-cell writers")
        expected = {g for g in doc["mechanism_reference_poses"] if _owner(g) == bid}
        if set(owned) != expected:
            raise ValueError("missing, foreign or unknown owned group")
        seen_cells.add(bid)
        seen_groups.update(owned)
        source = frame["state"]
        result["launch_groups"] = [g for g in result.get("launch_groups", []) if g not in owned]
        result["locals"][str(bid)] = copy.deepcopy(source["locals"][str(bid)])
        for cid in (bid, bid + 10):
            result["cassettes"][str(cid)] = list(source["cassettes"][str(cid)])
        for key, names in (("hatches", (str(bid),)), ("doors", (str(bid),)),
                           ("docks", (f"ready/{bid}", f"protected/{bid}")),
                           ("cradles", (str(bid), str(bid+10))),
                           ("clamps", (str(bid), str(bid+10)))):
            for name in names:
                if name in source[key]:
                    result[key][name] = source[key][name]
                else:
                    result[key].pop(name, None)
    return result


def self_test(doc):
    """Focused adapter proof; explicitly NOT swept-clearance/native acceptance."""
    import hashlib
    import json
    from pathlib import Path

    baseline = initial(doc)
    unchanged_doc = copy.deepcopy(doc)
    routes = [stage(doc, bid, bid) for bid in range(10)]
    group_sets = [set(route[0]["owned_groups"]) for route in routes]
    mapped_roots = 0
    for bid, route in enumerate(routes):
        template_doc = _canonical_doc(doc, bid)
        template_doc["parts"] = []
        for part in doc["parts"]:
            group = part.get("group")
            if group and _owner(group) == bid:
                points = [_point(p, bid, True) for p in part["bounds_body_m"]]
                template_doc["parts"].append({"group": _group(group, bid, True),
                    "bounds_body_m": [[min(p[i] for p in points) for i in range(3)],
                                      [max(p[i] for p in points) for i in range(3)]]})
        original = canonical.stage(template_doc)
        assert len(original) == len(route)
        assert route[0]["completed_stage_legs"] == 0
        assert route[-1]["completed_stage_legs"] == 3
        for frame, source in zip(route, original):
            assert frame["label"] == source["label"]
            assert frame["completed_stage_legs"] == source["completed_stage_legs"]
            assert set(frame["owned_groups"]) == group_sets[bid]
            for key in ("handled_cassette_id", "carried_cassette_id"):
                expected = None if source[key] is None else source[key] + bid
                assert frame[key] == expected
            assert frame["docked_cassettes"] == {
                str(int(cid)+bid): [dock[0], bid]
                for cid, dock in source["docked_cassettes"].items()}
            for cid in range(20):
                if cid not in (bid, bid+10):
                    assert frame["state"]["cassettes"][str(cid)] == baseline["cassettes"][str(cid)]
            for other in range(10):
                if other != bid:
                    assert frame["state"]["locals"][str(other)] == baseline["locals"][str(other)]
            for group in frame["owned_groups"]:
                cg = _group(group, bid, True)
                expected = canonical.translation(template_doc, cg, source["state"])
                if bid >= 5:
                    expected[2] *= -1
                actual = translation(doc, group, frame["state"])
                assert all(abs(a-b) < 1e-10 for a, b in zip(actual, expected)), (bid, group)
                mapped_roots += 1
        for other in range(bid):
            assert not group_sets[bid].intersection(group_sets[other])
        end = route[-1]
        assert end["docked_cassettes"] == {str(bid): ["protected", bid], str(bid+10): ["ready", bid]}
    # Staggered independent snapshots compose with exactly the same translations
    # as their standalone routes, irrespective of merge order.
    selected = [route[(bid*7) % len(route)] for bid, route in enumerate(routes)]
    merged = merge_frames(doc, selected)
    assert merged == merge_frames(doc, list(reversed(selected)))
    for bid, frame in enumerate(selected):
        for group in frame["owned_groups"]:
            assert translation(doc, group, merged) == translation(doc, group, frame["state"])
    sentinel = copy.deepcopy(baseline)
    sentinel["cassettes"]["19"] = [123., 456., 789.]
    sentinel["locals"]["9"]["height"] = 123.
    assert merge_frames(doc, [routes[0][-1]], sentinel)["cassettes"]["19"] == [123., 456., 789.]
    assert merge_frames(doc, [routes[0][-1]], sentinel)["locals"]["9"]["height"] == 123.
    # A stale global snapshot must not reset another cell already merged.
    staged = merge_frames(doc, [routes[0][-1]])
    assert merge_frames(doc, [routes[1][-1]], staged) == merge_frames(doc, [routes[0][-1], routes[1][-1]])
    invalid = [(0, 1), (4, 5), (-1, -1), (10, 10), (True, True), (0, False)]
    negatives = 0
    for source, destination in invalid:
        try:
            stage(doc, source, destination)
        except ValueError:
            negatives += 1
        else:
            raise AssertionError("invalid/unaligned route admitted")
    incomplete = copy.deepcopy(routes[0][0])
    incomplete["owned_groups"].pop()
    foreign = copy.deepcopy(routes[0][0])
    foreign["owned_groups"].append(routes[1][0]["owned_groups"][0])
    for frames in ([routes[0][0], routes[0][-1]], [incomplete], [foreign]):
        try:
            merge_frames(doc, frames)
        except ValueError:
            negatives += 1
        else:
            raise AssertionError("overlapping/incomplete/foreign writer admitted")
    launch_assertions = 0
    for bid in range(10):
        for cid in (bid, bid + 10, (bid // 5) * 5 + (bid + 1) % 5):
            launch = bay(doc, bid, cid)
            source_launch = old.bay(doc, bid, cid)
            for actual, source in zip(launch, source_launch):
                for key in source:
                    if key != "state":
                        assert actual[key] == source[key]
                for key, value in source["state"].items():
                    assert actual["state"][key] == value
                assert actual["state"]["locals"] == baseline["locals"]
                for group in actual["owned_groups"]:
                    assert translation(doc, group, actual["state"]) == old.translation(doc, group, source["state"])
                    launch_assertions += 1
                for group in doc["mechanism_reference_poses"]:
                    if group.startswith("cell"):
                        assert translation(doc, group, actual["state"]) == translation(doc, group, baseline)
            try:
                merge_frames(doc, [launch[-1]])
            except ValueError:
                negatives += 1
            else:
                raise AssertionError("Launch subset silently admitted as full-cell writer")
    for bid, cid in ((0, 5), (5, 0), (0, 20), (0, True)):
        try:
            bay(doc, bid, cid)
        except ValueError:
            negatives += 1
        else:
            raise AssertionError("invalid Launch identity admitted")
    malformed = []
    bad = copy.deepcopy(doc)
    bad["parallel_local_cells"].pop()
    malformed.append(bad)
    bad = copy.deepcopy(doc)
    bad["parallel_local_cells"][0]["protected_slot"] = 1
    malformed.append(bad)
    bad = copy.deepcopy(doc)
    bad["cassettes"][0]["cassette_id"] = 20
    malformed.append(bad)
    bad = copy.deepcopy(doc)
    bad["cassettes"][0]["body_center_m"][0] += 0.01
    malformed.append(bad)
    for bad in malformed:
        try:
            initial(bad)
        except ValueError:
            negatives += 1
        else:
            raise AssertionError("unsupported source topology/pose admitted")
    assert doc == unchanged_doc
    return {"scope": "source-only aligned route adapter mapping; no collision or native acceptance",
            "cells": 10, "routes": 10, "finite_cassettes": 20,
            "keyframes_per_route": len(routes[0]), "root_mapping_assertions": mapped_roots,
            "negative_cases": negatives, "disjoint_owner_pairs": 45,
            "launch_cases": 30, "launch_root_mapping_assertions": launch_assertions,
            "staggered_merge": "passed", "unowned_state_preservation": "passed",
            "adapter_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "canonical_route_sha256": hashlib.sha256(Path(canonical.__file__).read_bytes()).hexdigest(),
            "manifest_semantic_sha256": hashlib.sha256(json.dumps(doc, sort_keys=True).encode()).hexdigest()}


if __name__ == "__main__":
    import argparse
    import json
    from pathlib import Path
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    print(json.dumps(self_test(json.loads(args.manifest.read_text())), indent=2))
