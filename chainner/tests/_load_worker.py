# SPDX-License-Identifier: GPL-3.0-or-later
"""Worker for test_load_nodes.py: load chaiNNer's nodes from a scratch src copy and print a JSON report.
argv: scratch_src. Env CHECK_LEVEL=error and TESSALITH_PSDDS_DLL are set by the caller."""
import json
import os
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
print("REPORT " + json.dumps({
    "errors": [f"{e.module}: {e.error!r}" for e in errors],
    "total": len(api.registry.nodes),
    "ids": sorted(nodes),
    "side_effects": {k: n.side_effects for k, n in nodes.items()},
    "struct_defs": sum(
        "struct TessalithRmaos" in (i.type_definitions or "") for n in nodes.values() for i in n.inputs
    ),
}))
