# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for _ui.py (needs chaiNNer's src for Condition/DropDownInput; skipped if it is not installed)."""
import importlib
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

import numpy as np

REPO = Path(__file__).resolve().parents[2]
DLL = Path(os.environ.get("TESSALITH_PSDDS_DLL") or REPO / "target/psdds/x86_64-pc-windows-msvc/release/psdds.dll")
SRC = Path(os.environ.get("CHAINNER_SRC") or Path(os.environ.get("LOCALAPPDATA", "")) / "chaiNNer/app-0.25.1/resources/src")
PKG = REPO / "chainner/tessalith_dds"

U = P = None
if DLL.is_file() and SRC.is_dir():
    os.environ["TESSALITH_PSDDS_DLL"] = str(DLL)
    sys.path.insert(0, str(SRC))
    pkg = types.ModuleType("tessalith_t")  # a bare package, so the real __init__ (needs chaiNNer's category) is not run
    pkg.__path__ = [str(PKG)]
    pkg.tessalith_group = types.SimpleNamespace(register=lambda **kw: (lambda fn: fn))  # the real group needs chaiNNer's category
    sys.modules["tessalith_t"] = pkg
    P = importlib.import_module("tessalith_t._psdds")
    U = importlib.import_module("tessalith_t._ui")
    N = importlib.import_module("tessalith_t.save_dds_tessalith").save_dds_tessalith_node

COLOUR, NORMAL, MASK = 0, 1, 2
BC1, BC3, BC4, BC5, BC7, RGBA8 = range(6)


def ev(c, st):
    """Evaluate a chaiNNer Condition (enum/and/or/not only) against {input id: value}."""
    j = c.to_json()

    def go(j):
        k = j["kind"]
        if k == "enum":
            return st.get(j["enum"]) in j["values"]
        if k == "and":
            return all(go(i) for i in j["items"])
        if k == "or":
            return any(go(i) for i in j["items"])
        if k == "not":
            return not go(j["condition"])
        raise AssertionError(k)

    return go(j)


def state(t=COLOUR, fmt=BC7, srgb=0, mips=1, preset=0):
    return {U.PRESET_ID: preset, U.TYPE_ID: t, U.FMT_ID[t]: fmt, U.SRGB_ID: srgb, U.MIPS_ID: mips}


@unittest.skipIf(U is None, "psdds.dll or chaiNNer src missing")
class UiTests(unittest.TestCase):
    def test_srgb_never_for_normal_or_mask_bc45(self):
        for fmt in range(6):
            for srgb in (0, 1):
                for mips in (0, 1):
                    self.assertFalse(ev(U.visible("srgb"), state(NORMAL, fmt, srgb, mips)))
        for fmt in (BC4, BC5):
            self.assertFalse(ev(U.visible("srgb"), state(MASK, fmt)))
        self.assertTrue(ev(U.visible("srgb"), state(COLOUR, BC7)))

    def test_srgb_default_false_and_filter_visible_by_default(self):
        self.assertEqual(U.srgb_input().default, 0)
        self.assertEqual(U.mips_input().default, 1)
        self.assertEqual(U.discard_input().default, 0)
        for srgb in (0, 1):  # DC39: sRGB does not hide the filter
            self.assertTrue(ev(U.visible("filter"), state(COLOUR, BC7, srgb, 1)))

    def test_filter_hidden_cases(self):
        self.assertFalse(ev(U.visible("filter"), state(COLOUR, BC7, 0, 0)))
        for t in (NORMAL, MASK):
            self.assertFalse(ev(U.visible("filter"), state(t, BC7, 0, 1)))
        self.assertFalse(ev(U.visible("filter"), state(COLOUR, BC7, 0, 1, preset=2)))

    def test_preset_options(self):
        opts = U.preset_input().to_dict()["options"]
        self.assertEqual(len(opts), 6)
        self.assertEqual(opts[0]["option"], "Custom")
        self.assertEqual([o["option"] for o in opts[1:]], [n for n, _ in P.presets()])
        self.assertEqual([o["value"] for o in opts], list(range(6)))
        self.assertEqual(U.preset_input().default, 0)

    def test_seed_hides_every_setting(self):
        for preset in range(1, 6):
            st = state(COLOUR, BC7, 0, 1, preset=preset)
            conds = [U.custom(), U.visible("srgb"), U.visible("filter")] + [U.quality_condition(g) for g in range(len(U.QUALITY_GROUPS))]
            for c in conds:
                self.assertFalse(ev(c, st))
        self.assertTrue(ev(U.custom(), state()))

    def test_settings_for(self):
        raw = P.defaults(COLOUR)
        raw["format"] = BC1
        self.assertIs(U.settings_for(0, raw), raw)
        self.assertEqual(U.settings_for(3, raw), P.presets()[2][1])
        self.assertEqual(U.settings_for(3, raw)["texture_type"], NORMAL)
        self.assertEqual(U.settings_for(2, raw)["discard_alpha"], 1)
        self.assertEqual(raw["format"], BC1)  # DC45: Custom keeps the raw inputs

    def test_discard_only_by_preset(self):
        # DC36: the node gates Discard and Mipmaps by custom() only
        for t in range(3):
            for f in range(6):
                if P.format_allowed(t, f):
                    self.assertTrue(ev(U.custom(), state(t, f)))
                    for preset in range(1, 6):
                        self.assertFalse(ev(U.custom(), state(t, f, preset=preset)))
        # the srgb/filter table is probed with has_alpha 0; Discard never enters it
        self.assertEqual(set(U.TABLE), {"srgb", "filter", "quality"})
        self.assertEqual(U.TABLE, U.visibility_table())

    def test_formats_per_type(self):
        n = [len(i.to_dict()["options"]) for i in U.format_inputs()]
        self.assertEqual(n, [4, 5, 6])
        self.assertEqual([o["value"] for o in U.format_inputs()[COLOUR].to_dict()["options"]], [BC1, BC3, BC7, RGBA8])

    def test_quality_groups(self):
        self.assertEqual([g[0] for g in U.QUALITY_GROUPS], [(BC1, BC3), (BC4, BC5), (BC7,)])
        st = state(COLOUR, BC7)
        self.assertEqual([ev(U.quality_condition(g), st) for g in range(3)], [False, False, True])
        self.assertEqual([ev(U.quality_condition(g), state(NORMAL, BC5)) for g in range(3)], [False, True, False])
        self.assertFalse(any(ev(U.quality_condition(g), state(COLOUR, RGBA8)) for g in range(3)))

    def test_selected(self):
        fm = {COLOUR: BC1, NORMAL: BC5, MASK: BC7}
        self.assertEqual(U.selected(NORMAL, fm, {0: 3, 1: 1, 2: 6}), (BC5, 1))
        self.assertEqual(U.selected(COLOUR, fm, {0: 9, 1: 1, 2: 6}), (BC1, 9))  # hidden groups 1, 2 ignored
        fm[COLOUR] = RGBA8
        self.assertEqual(U.selected(COLOUR, fm, {0: 9, 1: 1, 2: 6})[0], RGBA8)


@unittest.skipIf(U is None, "psdds.dll or chaiNNer src missing")
class NodeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.img = np.random.default_rng(1).random((16, 16, 3), dtype=np.float32)

    def run_node(self, img=None, preset=0, t=COLOUR, fmts=(BC7, BC7, BC7), skip=False, name="x"):
        from api import Lazy
        lazy = Lazy.ready(self.img if img is None else img)
        return N(lazy, self.dir, None, name, preset, t, False, *fmts, False, True, 0, 4, 0, 4, skip)

    def test_skip_existing_does_not_evaluate_image(self):
        p = self.dir / "x.dds"
        p.write_bytes(b"old")

        class Boom:
            @property
            def value(self):
                raise AssertionError("lazy image evaluated")

        N(Boom(), self.dir, None, "x", 0, COLOUR, False, BC7, BC7, BC7, False, True, 0, 4, 0, 4, True)
        self.assertEqual(p.read_bytes(), b"old")

    def test_overwrite_goes_through_write_files(self):
        (self.dir / "x.dds").write_bytes(b"old")
        mod = sys.modules["tessalith_t.save_dds_tessalith"]
        with mock.patch.object(mod._psdds, "write_files", wraps=mod._psdds.write_files) as w:
            self.run_node()
        w.assert_called_once()
        self.assertEqual((self.dir / "x.dds").read_bytes()[:4], b"DDS ")

    def test_preset_writes_seed_bytes(self):
        for k in (2, 3):
            self.run_node(preset=k, name=f"p{k}")
            want = P.encode(self.img, P.presets()[k - 1][1])
            self.assertEqual((self.dir / f"p{k}.dds").read_bytes(), want)

    def test_custom_maps_inputs(self):
        self.run_node(t=NORMAL, fmts=(BC7, BC1, BC7))
        s = {**P.defaults(NORMAL), "format": BC1, "quality": 4, "mips": 1}
        s["quality"] = U.selected(NORMAL, {0: BC7, 1: BC1, 2: BC7}, {0: 4, 1: 0, 2: 4})[1]
        self.assertEqual((self.dir / "x.dds").read_bytes(), P.encode(self.img, s))

    def test_psdds_error_text_surfaces(self):
        with self.assertRaises(P.PsddsError) as cm:
            P.check_size(1 << 20, 1 << 20)
        want = str(cm.exception)
        with self.assertRaises(RuntimeError) as cm2:
            self.run_node(img=np.broadcast_to(np.zeros((1, 1, 3), np.float32), (1 << 20, 1 << 20, 3)))
        self.assertEqual(str(cm2.exception), want)
        self.assertFalse((self.dir / "x.dds").exists())


if __name__ == "__main__":
    unittest.main()
