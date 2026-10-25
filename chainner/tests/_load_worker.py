# SPDX-License-Identifier: GPL-3.0-or-later
"""Worker for test_load_nodes.py: load chaiNNer's nodes from a scratch src copy and print a JSON report.
argv: scratch_src. Env CHECK_LEVEL=error and TESSALITH_PSDDS_DLL are set by the caller."""
import json
import os
import re
import sys

root = sys.argv[1]
sys.path.insert(0, root)
os.chdir(root)
import api  # noqa: E402
import importlib  # noqa: E402

for pkg in ("chaiNNer_standard", "chaiNNer_pytorch", "chaiNNer_ncnn", "chaiNNer_onnx", "chaiNNer_external"):
    importlib.import_module("packages." + pkg)  # same list as server.py
errors = api.registry.load_nodes(os.path.join(root, "server.py"))
nodes = {k: v[0] for k, v in api.registry.nodes.items() if k.startswith("tessalith:")}
declared = sorted({m for n in nodes.values() for i in n.inputs
                   for m in re.findall(r"\b(?:struct|let)\s+(\w+)", i.type_definitions or "")})


def condition_errors(node_id, node):
    """The frontend's group check (index.js): an enum value missing from the dropdown's options blanks chaiNNer."""
    opts = {}
    for i in node.inputs:
        d = json.loads(json.dumps(i.to_dict()))
        if d.get("kind") == "dropdown":
            opts[d["id"]] = [json.dumps(o["value"]) for o in d["options"]]
    out, stack = [], [g for g in node.group_layout if isinstance(g, api.Group)]
    while stack:
        g = stack.pop()
        stack += [x for x in g.items if isinstance(x, api.Group)]
        conds = [json.loads(json.dumps(g.info.options["condition"]))] if "condition" in g.info.options else []
        while conds:
            c = conds.pop()
            if c["kind"] == "not":
                conds.append(c["condition"])
            elif c["kind"] in ("and", "or"):
                conds += c["items"]
            elif c["kind"] == "type":
                # the frontend evaluates type conditions in its built-in scope; node-declared names don't resolve
                if re.search(r"\b(" + "|".join(declared) + r")\b", json.dumps(c["condition"])):
                    out.append(f"{node_id}: input {c['input']} type condition names a node-declared type")
            elif c["kind"] == "enum":
                bad = [v for v in c["values"] if json.dumps(v) not in opts.get(c["enum"], [])]
                if c["enum"] not in opts or bad or not c["values"]:
                    out.append(f"{node_id}: input {c['enum']} values {bad or c['values']}")
    return sorted(set(out))


def enum_refs(c):
    if c["kind"] == "not":
        return enum_refs(c["condition"])
    if c["kind"] in ("and", "or"):
        return set().union(*map(enum_refs, c["items"]))
    return {c["enum"]} if c["kind"] == "enum" else set()


def condition_cycles(node_id, node):
    """The frontend evaluates an enum condition together with that dropdown's own conditions (index.js zJt), so an
    input whose visibility leads back to itself recurses until the stack overflows and the editor crashes."""
    refs = {}  # input id -> enum inputs named by the conditions of the groups around it

    def walk(items, outer):
        for x in items:
            if isinstance(x, api.Group):
                c = x.info.options.get("condition")
                walk(x.items, outer | (enum_refs(json.loads(json.dumps(c))) if c else set()))
            else:
                refs[int(x)] = outer

    walk(node.group_layout, set())
    out = []
    for start in refs:
        seen, todo = set(), list(refs[start])
        while todo:
            i = todo.pop()
            if i == start:
                out.append(f"{node_id}: input {start}'s visibility depends on itself")
                break
            if i not in seen:
                seen.add(i)
                todo += refs.get(i, ())
    return out


print("REPORT " + json.dumps({
    "condition_cycles": [e for k, n in nodes.items() for e in condition_cycles(k, n)],
    "condition_errors": [e for k, n in nodes.items() for e in condition_errors(k, n)],
    "rmaos_optional": {i.label: i.optional for i in nodes["tessalith:dds:rmaos"].inputs if i.kind == "generic"},
    "errors": [f"{e.module}: {e.error!r}" for e in errors],
    "total": len(api.registry.nodes),
    "ids": sorted(nodes),
    "side_effects": {k: n.side_effects for k, n in nodes.items()},
    "struct_defs": sum(
        "struct TessalithRmaos" in (i.type_definitions or "") for n in nodes.values() for i in n.inputs
    ),
}))
