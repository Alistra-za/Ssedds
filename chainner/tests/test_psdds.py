# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for _psdds.py. Run: python -I -m unittest discover -s chainner/tests (bundled chaiNNer Python)."""
import importlib.util
import os
import re
import tempfile
import threading
import tracemalloc
import unittest
from pathlib import Path
from unittest import mock

import numpy as np

REPO = Path(__file__).resolve().parents[2]
DLL = Path(os.environ.get("TESSALITH_PSDDS_DLL") or REPO / "target/psdds/x86_64-pc-windows-msvc/release/psdds.dll")
MOD = REPO / "chainner/tessalith_dds/_psdds.py"

P = None
if DLL.is_file():
    os.environ["TESSALITH_PSDDS_DLL"] = str(DLL)
    _spec = importlib.util.spec_from_file_location("_psdds", MOD)
    P = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(P)

COLOUR, NORMAL, MASK = 0, 1, 2
BC1, BC3, BC4, BC5, BC7, RGBA8 = range(6)
R, G, B, A = 2, 1, 0, 3  # indices in chaiNNer's BGR(A) arrays


def gradient(n=64, c=3):
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    ch = [x / (n - 1), y / (n - 1), (x + y) / (2 * (n - 1)), np.ones_like(x)]
    return (np.round(np.stack(ch[:c], axis=-1) * 255) / 255).astype(np.float32)  # 8-bit exact


def settings(fmt=RGBA8, **kw):
    s = P.defaults(COLOUR)
    s.update(format=fmt, srgb=0, mips=0, **kw)
    return s


@unittest.skipIf(P is None, f"psdds.dll not found at {DLL} (run crates\\psdds\\build.ps1)")
class PsddsTests(unittest.TestCase):
    def test_abi_matches_header(self):
        hdr = (REPO / "photoshop/psdds.h").read_text(encoding="utf-8")
        self.assertEqual(P.ABI_VERSION, int(re.search(r"#define PSDDS_ABI_VERSION (\d+)u", hdr).group(1)))
        self.assertTrue(P.version())

    def test_to_rgba(self):
        g = np.full((2, 3), 0.5, np.float32)
        out = P.to_rgba(g)
        self.assertEqual(out.shape, (2, 3, 4))
        self.assertTrue(np.all(out[..., :3] == 0.5) and np.all(out[..., 3] == 1))
        bgr = np.full((2, 2, 3), [0.1, 0.2, 0.3], np.float32)
        np.testing.assert_allclose(P.to_rgba(bgr)[0, 0], [0.3, 0.2, 0.1, 1])
        bgra = np.full((2, 2, 4), [0.1, 0.2, 0.3, 0.4], np.float32)
        np.testing.assert_allclose(P.to_rgba(bgra)[0, 0], [0.3, 0.2, 0.1, 0.4])
        self.assertTrue(P.to_rgba(bgra).flags.c_contiguous)

    def test_size_refused_before_copy(self):
        img = np.zeros((1, 16385, 3), np.float32)
        tracemalloc.start()
        try:
            with self.assertRaises(P.PsddsError) as cm:
                P.to_rgba(img)
            peak = tracemalloc.get_traced_memory()[1]
        finally:
            tracemalloc.stop()
        self.assertLess(peak, 1 << 20)
        self.assertIn("16384", str(cm.exception))

    def test_alpha_used(self):
        a = np.ones((4, 4, 4), np.float32)
        a[..., 3] = 254 / 255
        self.assertFalse(P.alpha_used(P.to_rgba(a)))
        a[0, 0, 3] = 253 / 255
        self.assertTrue(P.alpha_used(P.to_rgba(a)))
        self.assertFalse(P.alpha_used(P.to_rgba(np.zeros((4, 4, 3), np.float32))))

    def test_round_trip(self):
        img = gradient()
        data = P.encode(img, settings(RGBA8))
        out, has_alpha = P.decode(data)
        self.assertTrue(has_alpha)  # an RGBA8 file stores alpha
        want = np.round(img * 255)
        np.testing.assert_array_equal(np.round(out[..., :3] * 255), want)
        bc7 = P.decode(P.encode(img, settings(BC7)))[0]
        self.assertLessEqual(np.abs(np.round(bc7[..., :3] * 255) - want).max(), 4)  # measured 4 on this 3-ramp gradient at the default quality

    def test_format_allowed(self):
        self.assertFalse(P.format_allowed(COLOUR, BC4))
        self.assertTrue(P.format_allowed(NORMAL, BC5))

    def test_defaults_dc34(self):
        for t in (COLOUR, NORMAL, MASK):
            self.assertEqual(P.defaults(t)["srgb"], 0)
        self.assertEqual(P.defaults(0)["discard_alpha"], 0)
        self.assertEqual(list(P.defaults(0)), ["format", "texture_type", "srgb", "filter", "mips", "quality", "discard_alpha"])

    def test_presets_dc35(self):
        ps = P.presets()
        self.assertEqual([n for n, _ in ps], ["Colour BC7", "Colour BC1 (small, opaque)", "Normal BC7",
                                              "Mask/data BC7", "Greyscale mask BC4"])
        for i, (name, s) in enumerate(ps):
            self.assertEqual(s["srgb"], 0, name)
            self.assertEqual(s["discard_alpha"], 1 if i == 1 else 0, name)
            self.assertEqual(s, P.resolve(s, False)[0], name)
            e = P.resolve(s, True)[0]
            self.assertEqual({k: v for k, v in s.items() if k != "filter"}, {k: v for k, v in e.items() if k != "filter"}, name)

    def test_resolve_dc39(self):
        s = P.defaults(COLOUR)
        s.update(format=BC7, mips=1, filter=3)
        for srgb in (0, 1):
            s["srgb"] = srgb
            eff, ctl = P.resolve(s, False)
            self.assertEqual(ctl["filter_enabled"], 1)
            self.assertEqual(eff["filter"], 3)
            eff, ctl = P.resolve(s, True)
            self.assertEqual(ctl["filter_enabled"], 0)
            self.assertEqual(eff["filter"], 0)
        img = gradient()
        a = P.encode(img, {**s, "srgb": 0})
        b = P.encode(img, {**s, "srgb": 1})
        self.assertEqual(a[148:], b[148:])
        self.assertNotEqual(a[:148], b[:148])

    def test_discard_alpha_dc36(self):
        s = settings(BC7)
        with self.assertRaises(P.PsddsError):
            P.resolve({**s, "discard_alpha": 2}, False)
        rgb = gradient(32, 3)
        bgra = np.concatenate([rgb, np.ones((32, 32, 1), np.float32)], axis=-1)
        bgra[0, 0, 3] = 0.0
        keep = P.encode(bgra, {**s, "discard_alpha": 0})
        drop = P.encode(bgra, {**s, "discard_alpha": 1})
        with mock.patch.object(P, "alpha_used", side_effect=AssertionError("called")):
            self.assertEqual(P.encode(bgra, {**s, "discard_alpha": 1}), drop)
        self.assertEqual(drop, P.encode(rgb, {**s, "discard_alpha": 1}))
        self.assertNotEqual(drop, keep)

    def _rmaos(self, sources, fills=(-1, -1, -1, -1), **kw):
        return P.rmaos_encode(sources, list(fills), P.rmaos_default_quality(), 0, **kw)

    def test_rmaos_roughness_only(self):
        rough = np.full((64, 64), 0.25, np.float32)
        rmaos, normal, defaulted, baked, _note = self._rmaos([(rough, 0), None, None, None])
        self.assertIsNone(normal)
        self.assertFalse(baked)
        self.assertEqual(defaulted, ["G", "B", "A"])
        px = P.decode(rmaos)[0][0, 0] * 255  # BGRA
        self.assertAlmostEqual(px[2], 64, delta=1)
        self.assertAlmostEqual(px[1], 0, delta=1)
        self.assertAlmostEqual(px[0], 255, delta=1)
        self.assertAlmostEqual(px[3], 255, delta=1)

    def test_rmaos_multichannel_equals_gray(self):
        r = gradient(64, 1)[..., 0]
        ao = gradient(64, 1)[..., 0].T.copy()
        bgra = np.stack([ao, np.zeros_like(r), r, np.ones_like(r)], axis=-1)  # B=ao, R=r
        one = self._rmaos([(bgra, R), None, (bgra, B), None])[0]
        two = self._rmaos([(r, 0), None, (ao, 0), None])[0]
        self.assertEqual(one, two)

    def test_rmaos_with_normal_bakes(self):
        rough = np.full((64, 64), 0.5, np.float32)
        n = np.zeros((64, 64, 3), np.float32)
        n[..., 0] = 0.5 + 0.3 * gradient(64, 1)[..., 0]
        n[..., 1], n[..., 2] = 0.5, 0.9
        _rmaos, normal, _d, baked, note = self._rmaos([(rough, 0), None, None, None], normal=n, normal_settings=P.defaults(NORMAL))
        self.assertTrue(normal)
        self.assertTrue(baked, note)

    def test_rmaos_errors(self):
        a, b = np.zeros((64, 64), np.float32), np.zeros((32, 32), np.float32)
        with self.assertRaises(P.PsddsError) as cm:
            self._rmaos([(a, 0), (b, 0), None, None])
        self.assertTrue(str(cm.exception).startswith("sources differ in size"), str(cm.exception))
        with self.assertRaises(P.PsddsError):
            self._rmaos([(a, 0), None, None, None], fills=(300, -1, -1, -1))

    def test_write_files(self):
        with tempfile.TemporaryDirectory() as d:
            p, q = Path(d, "a.dds"), Path(d, "b.dds")
            p.write_bytes(b"old")
            try:
                data = P.encode(np.zeros((1, 16385, 3), np.float32), settings())
                P.write_files([(p, data)])
            except P.PsddsError:
                pass
            self.assertEqual(p.read_bytes(), b"old")
            P.write_files([(p, b"new1"), (q, b"new2")])
            self.assertEqual((p.read_bytes(), q.read_bytes()), (b"new1", b"new2"))
            self.assertEqual(sorted(x.name for x in Path(d).iterdir()), ["a.dds", "b.dds"])

    def test_decode_helpers(self):
        px = np.full((2, 2, 4), 255, np.uint8)
        self.assertFalse(P.opens_with_alpha(px))
        px[0, 0, 3] = 253
        self.assertTrue(P.opens_with_alpha(px))
        self.assertGreater(P.max_file_bytes(), 1 << 20)
        self.assertTrue(P.dds_openable(P.encode(gradient(8), settings())[:148]))
        self.assertFalse(P.dds_openable(b"nope" * 40))

    def test_threads(self):
        imgs = [(np.round(gradient() * (0.3 + 0.2 * i) * 255) / 255).astype(np.float32) for i in range(4)]
        out = [None] * 4

        def work(i):
            out[i] = P.decode(P.encode(imgs[i], settings(RGBA8)))[0]

        ts = [threading.Thread(target=work, args=(i,)) for i in range(4)]
        [t.start() for t in ts]
        [t.join() for t in ts]
        for i in range(4):
            np.testing.assert_array_equal(np.round(out[i][..., :3] * 255), np.round(imgs[i] * 255))


if __name__ == "__main__":
    unittest.main()
