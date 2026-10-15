"""Writes crates/gui/assets/cs_pbr_templates.json from Faultier's PBR landscape modders resource (design §4).

Run once by hand (not part of the build):
    python -I tools/gen-cs-pbr-templates.py "F:\\ai\\dds\\PBR landscape modders resource"
Files whose values are equal after parsing (ints read as floats) become one template; it keeps every EDID it was
used for. Its name is the group's first EDID alphabetically (plan ruling R2).
"""
import json
import os
import sys
from collections import defaultdict

KINDS = (("txst", "PBRTextureSets"), ("mato", "PBRMaterialObjects"))


def floats(v):
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, list):
        return [floats(x) for x in v]
    if isinstance(v, dict):
        return {k: floats(x) for k, x in v.items()}
    return v


def main(res, out):
    templates = []
    for kind, folder in KINDS:
        groups = defaultdict(list)
        values = {}
        for f in sorted(os.listdir(os.path.join(res, folder))):
            if not f.lower().endswith(".json"):
                continue
            with open(os.path.join(res, folder, f), encoding="utf-8") as h:
                v = floats(json.load(h))
            key = json.dumps(v, sort_keys=True)
            groups[key].append(f[:-5])
            values[key] = v
        for key, edids in groups.items():
            edids.sort()
            templates.append({"name": edids[0], "kind": kind, "edids": edids, "values": values[key]})
    templates.sort(key=lambda t: (t["kind"] != "txst", t["name"]))
    names = [t["name"] for t in templates]
    assert len(names) == len(set(names)), "template names must be unique"
    with open(out, "w", encoding="utf-8", newline="\n") as h:
        json.dump(templates, h, indent=2, sort_keys=True)
        h.write("\n")
    print({k: sum(t["kind"] == k for t in templates) for k, _ in KINDS})


if __name__ == "__main__":
    here = os.path.dirname(os.path.abspath(__file__))
    main(sys.argv[1], os.path.join(here, "..", "crates", "gui", "assets", "cs_pbr_templates.json"))
