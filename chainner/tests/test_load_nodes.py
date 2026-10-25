# SPDX-License-Identifier: GPL-3.0-or-later
"""Gated smoke test: load chaiNNer's nodes with CHECK_LEVEL=error plus ours, in a scratch copy of its src.
Set SDC_CHAINNER_SRC to the install's resources\src (read only). Skipped otherwise."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SRC = os.environ.get("SDC_CHAINNER_SRC")
DLL = Path(os.environ.get("TESSALITH_PSDDS_DLL") or REPO / "target/psdds/x86_64-pc-windows-msvc/release/psdds.dll")


@unittest.skipUnless(SRC and Path(SRC).is_dir() and DLL.is_file(), "set SDC_CHAINNER_SRC (and build psdds.dll) to run")
class LoadNodes(unittest.TestCase):
    def test_load_with_checks(self):
        with tempfile.TemporaryDirectory() as tmp:
            scratch = Path(tmp) / "src"
            # skip an installed copy so the repo's nodes are the ones tested
            shutil.copytree(SRC, scratch, ignore=shutil.ignore_patterns("__pycache__", "tessalith_dds*"))
            shutil.copytree(REPO / "chainner/tessalith_dds", scratch / "packages/chaiNNer_standard/tessalith_dds",
                            ignore=shutil.ignore_patterns("__pycache__"))
            env = dict(os.environ, CHECK_LEVEL="error", TESSALITH_PSDDS_DLL=str(DLL))
            p = subprocess.run([sys.executable, "-I", str(Path(__file__).with_name("_load_worker.py")), str(scratch)],
                               env=env, capture_output=True, text=True, timeout=600)
            line = [x for x in p.stdout.splitlines() if x.startswith("REPORT ")]
            self.assertTrue(line, f"no report (exit {p.returncode}):\n{p.stdout[-2000:]}\n{p.stderr[-2000:]}")
            r = json.loads(line[0][len("REPORT "):])
        # other packages may fail on this machine (e.g. onnx DLL); only ours count
        self.assertEqual([e for e in r["errors"] if "tessalith_dds" in e], [])
        self.assertEqual(r["condition_errors"], [])
        # owner 2026-10-25: roughness is required, the other three image inputs stay optional
        self.assertEqual(r["rmaos_optional"], {"Roughness": False, "Metallic": True, "AO": True, "Specular": True})
        self.assertEqual(r["ids"], ["tessalith:dds:rmaos", "tessalith:dds:save", "tessalith:dds:save_normal"])
        self.assertEqual(r["struct_defs"], 1)
        se = r["side_effects"]
        self.assertTrue(se["tessalith:dds:save"] and se["tessalith:dds:save_normal"])
        self.assertFalse(se["tessalith:dds:rmaos"])


if __name__ == "__main__":
    unittest.main()
