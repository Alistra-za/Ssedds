# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for _pbr.py (Save Normal body, RMAOS job) and the two nodes. _pbr needs only psdds.dll; the node tests need chaiNNer's src."""
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

P = B = RN = SN = None
if DLL.is_file():
    os.environ["TESSALITH_PSDDS_DLL"] = str(DLL)
    if "tessalith_t" not in sys.modules:  # test_ui may have made it already
        pkg = types.ModuleType("tessalith_t")  # bare package: the real __init__ needs chaiNNer's category
        pkg.__path__ = [str(PKG)]
        pkg.tessalith_group = types.SimpleNamespace(register=lambda **kw: (lambda fn: fn))
        sys.modules["tessalith_t"] = pkg
    P = importlib.import_module("tessalith_t._psdds")
    B = importlib.import_module("tessalith_t._pbr")
    if SRC.is_dir():
        if str(SRC) not in sys.path:
            sys.path.insert(0, str(SRC))
        RN = importlib.import_module("tessalith_t.rmaos_tessalith").rmaos_tessalith_node
        SN = importlib.import_module("tessalith_t.save_normal_tessalith").save_normal_tessalith_node

BC7 = 4
R, G, B_, A = range(4)  # channel dropdown values
DEFAULT, CUSTOM = 0, 1  # fill dropdown values
H = W = 64


def grey(v, h=H, w=W):
    return np.full((h, w), v, np.float32)


def normal_img(c=3):
    n = np.empty((H, W, c), np.float32)
    rng = np.random.default_rng(3)
    n[..., :3] = rng.random((H, W, 3), dtype=np.float32) * 0.2 + [0.9, 0.4, 0.4]  # BGR: mostly +Z
    if c == 4:
        n[..., 3] = 1
    return n


@unittest.skipIf(B is None, "psdds.dll missing")
class PbrTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.n = normal_img()
        self.s = B.normal_settings(BC7, True, P.defaults(1)["quality"], False)

    def job(self, sources, fills=(-1, -1, -1, -1), quality=None):
        q = P.rmaos_default_quality() if quality is None else quality
        return B.RmaosJob(tuple(sources), tuple(fills), q, True)

    def save(self, job, name="x", normal=None, settings=None, skip=False):
        n = self.n if normal is None else normal
        return B.save_normal(lambda: n, settings or self.s, None if job is None else (lambda: job), self.dir, name, skip)

    def files(self, name="x"):
        return [(self.dir / f"{name}{s}.dds") for s in ("_n", "_rmaos")]

    def test_paired_writes_engine_bytes(self):
        rough, ao = grey(0.25), grey(0.75)
        job = self.job([(rough, 0), None, (ao, 0), None])
        with self.assertLogs("sanic.root", "INFO") as logs:
            out = self.save(job)
        n_path, rm_path = self.files()
        self.assertEqual(sorted(out), sorted([n_path.resolve(), rm_path.resolve()]))
        rm, nm, defaulted, baked, _ = P.rmaos_encode([(rough, 0), None, (ao, 0), None], [-1] * 4, job.quality, 1,
                                                      normal=self.n, normal_settings={**self.s, "discard_alpha": 0})
        self.assertTrue(baked)
        self.assertEqual(rm_path.read_bytes(), rm)
        self.assertEqual(n_path.read_bytes(), nm)
        self.assertIn("G", defaulted)
        text = "\n".join(logs.output)
        self.assertIn("baked", text)
        self.assertIn("PACK-DEFAULTED", text)
        px, _ = P.decode(rm)  # BGRA float; layout R roughness, G metallic, B AO, A specular
        self.assertLess(abs(float(px[..., 2].mean()) - 0.25), 4 / 255)
        self.assertLess(abs(float(px[..., 0].mean()) - 0.75), 4 / 255)

    def test_one_bgra_image_shared_by_two_roles(self):
        rough, ao = grey(0.25), grey(0.75)
        self.save(self.job([(rough, 0), None, (ao, 0), None]), name="two")
        orm = np.zeros((H, W, 4), np.float32)
        orm[..., 1], orm[..., 0], orm[..., 3] = 0.25, 0.75, 1  # G = roughness, B = AO (BGRA array)
        srcs = B.sources([orm, None, orm, None], [G, R, B_, R])
        self.assertEqual(srcs[0], (orm, 1))
        self.assertEqual(srcs[2], (orm, 0))
        self.assertIs(srcs[0][0], orm)  # no copy
        self.save(self.job(srcs), name="one")
        for a, b in zip(self.files("two"), self.files("one")):
            self.assertEqual(a.read_bytes(), b.read_bytes())

    def test_channel_mapping(self):
        bgra = np.zeros((4, 4, 4), np.float32)
        self.assertEqual([B.sources([bgra] * 4, [R, G, B_, A])[i][1] for i in range(4)], [2, 1, 0, 3])
        g = grey(0.5, 4, 4)
        self.assertEqual(B.sources([g, None, None, None], [R, R, R, R])[0], (g, 0))  # grey: dropdown hidden, index 0
        self.assertEqual(B.sources([None] * 4, [R] * 4), (None,) * 4)

    def test_missing_channel_is_psdds_error(self):
        rgb = np.zeros((H, W, 3), np.float32)
        with self.assertRaises(P.PsddsError) as cm:
            self.save(self.job(B.sources([rgb, None, None, None], [A, R, R, R])))
        self.assertEqual(str(cm.exception), "roughness channel 3 is not in a 3-channel image")
        self.assertEqual(list(self.dir.iterdir()), [])

    def test_fill_mapping(self):
        self.assertEqual(B.fill(DEFAULT, 77), -1)
        self.assertEqual(B.fill(CUSTOM, 77), 77)
        self.assertEqual(B.fill(CUSTOM, 0), 0)
        srcs = (None, (grey(0.5), 0), None, None)
        # a connected role's (hidden) fill is ignored; Default is -1
        self.assertEqual(B.fills(srcs, [CUSTOM, CUSTOM, DEFAULT, CUSTOM], [10, 20, 30, 40]), (10, -1, -1, 40))

    def test_alone_equals_save_dds_normal(self):
        out = self.save(None)
        n_path, rm_path = self.files()
        self.assertEqual(out, [n_path.resolve()])
        self.assertEqual(n_path.read_bytes(), P.encode(self.n, self.s))
        self.assertFalse(rm_path.exists())
        s = {**P.defaults(1), "format": BC7, "mips": 1, "quality": P.defaults(1)["quality"], "discard_alpha": 0}
        self.assertEqual(self.s, s)  # the Normal settings Save DDS would use with type Normal

    def test_normal_settings_srgb_off(self):  # DC34
        for d in (False, True):
            s = B.normal_settings(BC7, False, 3, d)
            self.assertEqual((s["srgb"], s["texture_type"], s["mips"], s["quality"], s["discard_alpha"]), (0, 1, 0, 3, int(d)))
            self.assertEqual(list(s), list(P.defaults(1)))

    def test_discard_alpha(self):  # DC36
        n = normal_img(4)
        n[0, 0, 3] = 0
        opaque = n.copy()
        opaque[..., 3] = 1
        on = B.normal_settings(BC7, True, 4, True)
        off = B.normal_settings(BC7, True, 4, False)
        self.save(None, name="on", normal=n, settings=on)
        self.save(None, name="off", normal=n, settings=off)
        self.assertEqual(self.files("on")[0].read_bytes(), P.encode(opaque, off))
        self.assertNotEqual(self.files("off")[0].read_bytes(), self.files("on")[0].read_bytes())
        job = self.job([(grey(0.5), 0), None, None, None])
        self.save(job, name="pon", normal=n, settings=on)
        self.save(job, name="poff", normal=n, settings=off)
        for a, b in zip(self.files("pon"), self.files("poff")):
            self.assertEqual(a.read_bytes(), b.read_bytes())

    def test_size_mismatch_writes_nothing(self):
        job = self.job([(grey(0.5), 0), (grey(0.5, 32, 32), 0), None, None])
        with self.assertRaises(P.PsddsError) as want:
            P.rmaos_encode(list(job.sources), list(job.fills), job.quality, 1, normal=self.n, normal_settings=self.s)
        with self.assertRaises(P.PsddsError) as cm:
            self.save(job)
        self.assertEqual(str(cm.exception), str(want.exception))
        self.assertEqual(list(self.dir.iterdir()), [])
        for p in self.files():
            p.write_bytes(b"old")
        with self.assertRaises(P.PsddsError):
            self.save(job)
        self.assertEqual([p.read_bytes() for p in self.files()], [b"old", b"old"])
        self.assertEqual(sorted(p.name for p in self.dir.iterdir()), ["x_n.dds", "x_rmaos.dds"])

    def test_skip_existing(self):
        def boom():
            raise AssertionError("evaluated")

        n_path, rm_path = self.files()
        job = self.job([(grey(0.5), 0), None, None, None])
        n_path.write_bytes(b"old")
        # alone: _n exists -> skip, nothing evaluated
        self.assertEqual(B.save_normal(boom, self.s, None, self.dir, "x", True), [])
        # paired, only _n exists -> write both
        self.assertEqual(len(self.save(job, skip=True)), 2)
        self.assertEqual(rm_path.read_bytes()[:4], b"DDS ")
        self.assertEqual(n_path.read_bytes()[:4], b"DDS ")
        # paired, both exist -> skip, nothing evaluated
        n_path.write_bytes(b"old")
        rm_path.write_bytes(b"old")
        self.assertEqual(B.save_normal(boom, self.s, boom, self.dir, "x", True), [])
        self.assertEqual([n_path.read_bytes(), rm_path.read_bytes()], [b"old", b"old"])
        # paired, only _rmaos exists -> write both
        n_path.unlink()
        self.assertEqual(len(self.save(job, skip=True)), 2)
        self.assertEqual(rm_path.read_bytes()[:4], b"DDS ")

    def test_type_names(self):
        self.assertEqual(B.RMAOS_TYPE, "TessalithRmaos")
        self.assertEqual(B.RMAOS_TYPE_DEF, "struct TessalithRmaos;")


@unittest.skipIf(SN is None, "psdds.dll or chaiNNer src missing")
class NodeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.n = normal_img()
        self.rough, self.ao = grey(0.25), grey(0.75)

    def rmaos(self, **kw):
        a = dict(r=self.rough, m=None, ao=self.ao, sp=None, picks=(R, R, R, R), modes=(DEFAULT,) * 4, nums=(0,) * 4)
        a.update(kw)
        imgs = (a["r"], a["m"], a["ao"], a["sp"])
        args = []
        for i in range(4):
            args += [imgs[i], a["picks"][i], a["modes"][i], a["nums"][i]]
        return RN(*args, P.rmaos_default_quality(), True)

    def save(self, rmaos_lazy, skip=False, discard=False, normal_lazy=None):
        from api import Lazy
        q = P.defaults(1)["quality"]
        return SN(normal_lazy or Lazy.ready(self.n), self.dir, None, "x", BC7, discard, True, q, q, q, rmaos_lazy, skip)

    def test_rmaos_node_builds_job_only(self):
        mod = sys.modules["tessalith_t.rmaos_tessalith"]
        with mock.patch.object(mod._pbr._psdds, "rmaos_encode") as enc, \
                mock.patch.object(mod._pbr._psdds, "write_files") as wf:
            job = self.rmaos(modes=(DEFAULT, CUSTOM, DEFAULT, DEFAULT), nums=(9, 200, 9, 9))
        enc.assert_not_called()
        wf.assert_not_called()
        self.assertEqual(list(self.dir.iterdir()), [])  # a lone RMAOS writes nothing
        self.assertIsInstance(job, B.RmaosJob)
        self.assertIs(job.sources[0][0], self.rough)
        self.assertEqual(job.fills, (-1, 200, -1, -1))
        self.assertEqual((job.quality, job.mips), (P.rmaos_default_quality(), True))

    def test_paired_node_matches_body(self):
        from api import Lazy
        job = self.rmaos()
        self.save(Lazy.ready(job), discard=True)
        got = [(self.dir / f).read_bytes() for f in ("x_n.dds", "x_rmaos.dds")]
        s = B.normal_settings(BC7, True, P.defaults(1)["quality"], False)
        rm, nm, *_ = P.rmaos_encode(list(job.sources), list(job.fills), job.quality, 1, normal=self.n, normal_settings=s)
        self.assertEqual(got, [nm, rm])

    def test_lone_save_normal_node(self):
        from api import Lazy
        self.save(Lazy.ready(None))
        s = B.normal_settings(BC7, True, P.defaults(1)["quality"], False)
        self.assertEqual((self.dir / "x_n.dds").read_bytes(), P.encode(self.n, s))
        self.assertFalse((self.dir / "x_rmaos.dds").exists())

    def test_skip_pair_evaluates_nothing(self):
        from api import Lazy

        def boom():
            raise AssertionError("evaluated")

        for f in ("x_n.dds", "x_rmaos.dds"):
            (self.dir / f).write_bytes(b"old")
        self.save(Lazy(boom), skip=True, normal_lazy=Lazy(boom))
        self.assertEqual((self.dir / "x_n.dds").read_bytes(), b"old")


if __name__ == "__main__":
    unittest.main()
