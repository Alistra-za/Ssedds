# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for _ui.py (needs chaiNNer's src for Condition/DropDownInput; skipped if it is not installed)."""
import importlib
import json
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
BC1, BC3, BC4, BC5, BC7, RGBA8, BC6H, RGBA16F = range(8)


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


def state(t=COLOUR, fmt=BC7, srgb=0, mips=1, preset=0, hdr=0):
    return {U.PRESET_ID: preset, U.TYPE_ID: t, U.FMT_ID[t]: fmt, U.SRGB_ID: srgb, U.MIPS_ID: mips, U.HDR_ID: hdr}


@unittest.skipIf(U is None, "psdds.dll or chaiNNer src missing")
class UiTests(unittest.TestCase):
    def test_srgb_never_for_normal_or_mask_bc45(self):
        for fmt in range(8):
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
        # the srgb/filter table is probed with has_alpha 0 (alpha_mode with 1, Note DH); Discard never enters it
        self.assertEqual(set(U.TABLE), {"srgb", "filter", "quality", "discard", "alpha_mode"})
        self.assertEqual(U.TABLE, U.visibility_table())

    def test_formats_per_type(self):
        vals = [[o["value"] for o in i.to_dict()["options"]] for i in U.format_inputs()]
        self.assertEqual(vals[COLOUR], [BC1, BC3, BC7, RGBA8, BC6H, RGBA16F])  # HDR formats for Colour only
        self.assertEqual(vals[NORMAL], [BC1, BC3, BC5, BC7, RGBA8])
        self.assertEqual(vals[MASK], [BC1, BC3, BC4, BC5, BC7, RGBA8])

    def test_discard_visibility(self):
        shown = {(t, f): ev(U.visible("discard"), state(t, f)) for t in range(3) for f in range(8) if P.format_allowed(t, f)}
        for f in (BC1, BC4, BC5, BC6H):
            for t in range(3):
                if (t, f) in shown:
                    self.assertFalse(shown[(t, f)], (t, f))
        for f in (BC3, BC7, RGBA8, RGBA16F):
            self.assertTrue(shown[(COLOUR, f)], f)
        for hdr in (0, 1):  # the HDR switch changes no control (and is never named inside its own condition)
            for f in (BC6H, RGBA16F):
                self.assertEqual(ev(U.visible("discard"), state(COLOUR, f, hdr=hdr)), f == RGBA16F)
        self.assertFalse(ev(U.visible("discard"), state(COLOUR, BC7, preset=2)))

    def test_alpha_mode_visibility(self):
        # Note DH: shown where psdds can keep alpha (probed with has_alpha 1) and Discard is off; hidden elsewhere
        def shown(t, f, discard=0, **kw):
            return ev(U.alpha_mode_visible(), {**state(t, f, **kw), U.DISCARD_ID: discard})
        for t in range(3):
            for f in range(8):
                if P.format_allowed(t, f):
                    keeps = f in (BC3, BC7, RGBA8, RGBA16F)
                    self.assertEqual(shown(t, f), keeps, (t, f))
                    self.assertFalse(shown(t, f, discard=1), (t, f))
        for hdr in (0, 1):
            self.assertTrue(shown(COLOUR, RGBA16F, hdr=hdr))
        self.assertFalse(shown(COLOUR, BC7, preset=2))
        self.assertEqual(U.alpha_mode_input().default, 0)
        # DE20: Discard's own condition never names this input; this input's never names itself
        self.assertNotIn(f'"enum": {U.ALPHA_MODE_ID}', json.dumps(U.visible("discard").to_json()))
        self.assertNotIn(f'"enum": {U.ALPHA_MODE_ID}', json.dumps(U.alpha_mode_visible().to_json()))
        self.assertIn(f'"enum": {U.DISCARD_ID}', json.dumps(U.alpha_mode_visible().to_json()))
    def test_quality_bc6h(self):
        self.assertEqual(len(U.QUALITY_GROUPS), 4)
        self.assertEqual(U.QUALITY_GROUPS[3], ((BC6H,), P.quality_labels(BC6H)))
        self.assertEqual(len(P.quality_labels(BC6H)), 4)
        self.assertEqual(len(U.quality_inputs()[3].to_dict()["options"]), 4)
        ids = [i.id for i in U.quality_inputs()]
        self.assertEqual(len(set(ids)), 4)
        self.assertTrue(ev(U.quality_condition(3), state(COLOUR, BC6H)))
        self.assertFalse(any(ev(U.quality_condition(g), state(COLOUR, RGBA16F)) for g in range(4)))
        self.assertFalse(ev(U.quality_condition(3), state(NORMAL, BC5)))
        # Save Normal keeps its three groups and ids clear of the RMAOS input
        nq = U.normal_quality_inputs()
        self.assertEqual(len(nq), 3)
        self.assertNotIn(U.N_RMAOS_ID, [i.id for _, i in nq])

    def test_colour_format_list_has_hdr(self):
        self.assertNotIn(BC6H, [o["value"] for o in U.format_inputs()[NORMAL].to_dict()["options"]])
        self.assertNotIn(RGBA16F, [o["value"] for o in U.format_inputs()[MASK].to_dict()["options"]])

    def test_quality_groups(self):
        self.assertEqual([g[0] for g in U.QUALITY_GROUPS], [(BC1, BC3), (BC4, BC5), (BC7,), (BC6H,)])
        st = state(COLOUR, BC7)
        self.assertEqual([ev(U.quality_condition(g), st) for g in range(4)], [False, False, True, False])
        self.assertEqual([ev(U.quality_condition(g), state(NORMAL, BC5)) for g in range(4)], [False, True, False, False])
        self.assertFalse(any(ev(U.quality_condition(g), state(COLOUR, RGBA8)) for g in range(4)))

    def test_selected(self):
        fm = {COLOUR: BC1, NORMAL: BC5, MASK: BC7}
        self.assertEqual(U.selected(NORMAL, fm, {0: 3, 1: 1, 2: 6, 3: 0}), (BC5, 1))
        self.assertEqual(U.selected(COLOUR, fm, {0: 9, 1: 1, 2: 6, 3: 0}), (BC1, 9))  # hidden groups 1, 2 ignored
        fm[COLOUR] = RGBA8
        self.assertEqual(U.selected(COLOUR, fm, {0: 9, 1: 1, 2: 6, 3: 0})[0], RGBA8)


@unittest.skipIf(U is None, "psdds.dll or chaiNNer src missing")
class NodeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.img = np.random.default_rng(1).random((16, 16, 3), dtype=np.float32)

    def run_node(self, img=None, preset=0, t=COLOUR, fmts=(BC7, BC7, BC7), skip=False, name="x", hdr=False, transparency=False):
        from api import Lazy
        lazy = Lazy.ready(self.img if img is None else img)
        return N(lazy, self.dir, None, name, preset, t, False, *fmts, False, transparency, True, 0, 4, 0, 4, 0, hdr, skip)

    def test_skip_existing_does_not_evaluate_image(self):
        p = self.dir / "x.dds"
        p.write_bytes(b"old")

        class Boom:
            @property
            def value(self):
                raise AssertionError("lazy image evaluated")

        N(Boom(), self.dir, None, "x", 0, COLOUR, False, BC7, BC7, BC7, False, False, True, 0, 4, 0, 4, 0, False, True)
        self.assertEqual(p.read_bytes(), b"old")

    def test_overwrite_goes_through_write_files(self):
        (self.dir / "x.dds").write_bytes(b"old")
        mod = sys.modules["tessalith_t.save_dds_tessalith"]
        with mock.patch.object(mod._psdds, "write_files", wraps=mod._psdds.write_files) as w:
            self.run_node()
        w.assert_called_once()
        self.assertEqual((self.dir / "x.dds").read_bytes()[:4], b"DDS ")

    def test_alpha_is_transparency_reaches_psdds(self):
        # Note DH: off (default) = alpha as data, on = Blend; each equals psdds's own encode of those settings
        img = np.random.default_rng(3).random((16, 16, 4), dtype=np.float32)
        img[:, :4, 3] = 0.0
        s = {**P.defaults(COLOUR), "format": BC7, "quality": 4, "mips": 1}
        for on in (False, True):
            self.run_node(img=img, name=f"a{on}", transparency=on)
            self.assertEqual((self.dir / f"a{on}.dds").read_bytes(), P.encode(img, {**s, "alpha_mode": int(on)}, 16))
        self.assertNotEqual((self.dir / "aFalse.dds").read_bytes(), (self.dir / "aTrue.dds").read_bytes())
    def test_preset_writes_seed_bytes(self):
        for k in (2, 3):
            self.run_node(preset=k, name=f"p{k}")
            want = P.encode(self.img, P.presets()[k - 1][1])
            self.assertEqual((self.dir / f"p{k}.dds").read_bytes(), want)

    def test_custom_maps_inputs(self):
        self.run_node(t=NORMAL, fmts=(BC7, BC1, BC7))
        s = {**P.defaults(NORMAL), "format": BC1, "quality": 4, "mips": 1}
        s["quality"] = U.selected(NORMAL, {0: BC7, 1: BC1, 2: BC7}, {0: 4, 1: 0, 2: 4, 3: 0})[1]
        self.assertEqual((self.dir / "x.dds").read_bytes(), P.encode(self.img, s))

    def test_psdds_error_text_surfaces(self):
        with self.assertRaises(P.PsddsError) as cm:
            P.check_size(1 << 20, 1 << 20)
        want = str(cm.exception)
        with self.assertRaises(RuntimeError) as cm2:
            self.run_node(img=np.broadcast_to(np.zeros((1, 1, 3), np.float32), (1 << 20, 1 << 20, 3)))
        self.assertEqual(str(cm2.exception), want)
        self.assertFalse((self.dir / "x.dds").exists())

    def test_failed_encode_makes_no_directory(self):
        from api import Lazy
        big = np.broadcast_to(np.zeros((1, 1, 3), np.float32), (1 << 20, 1 << 20, 3))
        with self.assertRaises(RuntimeError):
            N(Lazy.ready(big), self.dir, "new", "x", 0, COLOUR, False, BC7, BC7, BC7, False, False, True, 0, 4, 0, 4, 0, False, False)
        self.assertFalse((self.dir / "new").exists())


@unittest.skipIf(U is None, "psdds.dll or chaiNNer src missing")
class HdrTests(unittest.TestCase):
    def setUp(self):
        self.img = np.random.default_rng(7).random((16, 16, 4), dtype=np.float32)
        self.img[..., 3] = np.where(np.arange(256).reshape(16, 16) % 2, 1.0, 0.4)  # real alpha

    @staticmethod
    def old_encode(img, s):
        """Today's bytes: psdds_encode (depth 8), as the node called it before the HDR switch."""
        import ctypes as C
        rgba = P.to_rgba(img)
        ha = 0 if s["discard_alpha"] == 1 else int(P.alpha_used(rgba))
        out, err = P._Buf(), C.create_string_buffer(1024)
        assert P.lib.psdds_encode(rgba.ctypes.data_as(C.POINTER(C.c_float)), 16, 16, ha, C.byref(P._s(s)), C.byref(out), err, 1024) == 0
        try:
            return C.string_at(out.data, out.len)
        finally:
            P.lib.psdds_free(out)

    def test_switch_off_is_todays_bytes(self):
        n = 0
        for t in range(3):
            for f in range(6):  # the formats that existed before
                if P.format_allowed(t, f):
                    s = {**P.defaults(t), "format": f}
                    self.assertEqual(P.encode(self.img, s, 16), self.old_encode(self.img, s), (t, f))
                    n += 1
        for _, s in P.presets():
            self.assertEqual(P.encode(self.img, s, 16), self.old_encode(self.img, s), s)
            n += 1
        self.assertGreaterEqual(n, 15)

    def test_bc6h_off_decodes_srgb_grey_to_linear(self):
        grey = np.full((8, 8, 3), 0.5, np.float32)
        dds = P.encode(grey, {**P.defaults(COLOUR), "format": BC6H}, 16)
        px = P.decode_depth32(dds)
        self.assertLess(abs(float(px[..., :3].mean()) - 0.2140), 0.01)

    def test_hdr_on_keeps_values_above_one(self):
        img = np.full((8, 8, 3), 4.0, np.float32)
        px = P.decode_depth32(P.encode(img, {**P.defaults(COLOUR), "format": BC6H}, 32))
        self.assertLess(abs(float(px[..., :3].mean()) - 4.0), 0.1)
        s = {**P.defaults(COLOUR), "format": RGBA16F, "mips": 0}
        half = np.frombuffer(P.encode(img, s, 32)[-8 * 8 * 8:], np.float16).reshape(-1, 4)
        self.assertTrue((half[:, :3] == 4.0).all())

    def test_nodes_hdr_switch_reaches_psdds(self):
        # the node passes depth 32 only when the switch is on: BC6H of 4.0 keeps 4.0, off clamps to 1.0 (then decodes to 1.0)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        from api import Lazy
        img = np.full((8, 8, 3), 4.0, np.float32)
        for hdr, want in ((True, 4.0), (False, 1.0)):
            N(Lazy.ready(img), Path(tmp.name), None, f"h{hdr}", 0, COLOUR, False, BC6H, BC7, BC7, False, False, True, 0, 4, 0, 4, 0, hdr, False)
            px = P.decode_depth32((Path(tmp.name) / f"h{hdr}.dds").read_bytes())
            self.assertLess(abs(float(px[..., :3].mean()) - want), 0.1)


if __name__ == "__main__":
    unittest.main()
