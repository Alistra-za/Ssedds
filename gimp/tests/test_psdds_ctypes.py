# SPDX-License-Identifier: GPL-3.0-or-later
import ctypes
import os
import re
import sys
import unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(REPO, "gimp", "tessalith-dds"))
sys.path.insert(0, HERE)
os.environ.setdefault("TESSALITH_PSDDS_DLL", os.path.join(
    REPO, "target", "psdds", "x86_64-pc-windows-msvc", "release", "psdds.dll"))

import psdds_ctypes as P
from fixtures import dds_header, rgba_floats

DLL = P.Psdds()
NAMES = ["Colour BC7", "Colour BC1 (small, opaque)", "Normal BC7", "Mask/data BC7", "Greyscale mask BC4"]


def copy(s):
    return P.Settings.from_buffer_copy(s)


class T(unittest.TestCase):
    def test_abi_matches_header(self):
        with open(os.path.join(REPO, "photoshop", "psdds.h")) as f:
            hdr = int(re.search(r"#define PSDDS_ABI_VERSION (\d+)u", f.read()).group(1))
        self.assertEqual(hdr, P.EXPECTED_ABI)
        self.assertEqual(DLL._dll.psdds_abi_version(), P.EXPECTED_ABI)
        with mock.patch.object(P, "EXPECTED_ABI", 3):
            with self.assertRaises(P.PsddsError) as c:
                P.Psdds()
        self.assertIn("ABI version 2", str(c.exception))
        self.assertIn("needs 3", str(c.exception))

    def test_defaults_colour(self):
        d = DLL.defaults(P.COLOUR)
        self.assertEqual((d.format, d.srgb, d.mips, d.discard_alpha), (P.BC7, 0, 1, 0))
        for t in (P.NORMAL, P.LINEAR_MASK):
            self.assertEqual(DLL.defaults(t).srgb, 0)
        self.assertEqual(ctypes.sizeof(P.Settings), 28)

    def test_preset_seeds(self):
        ps = DLL.presets()
        self.assertEqual([n for n, _ in ps], NAMES)
        for i, (_, s) in enumerate(ps):
            self.assertEqual(s.srgb, 0)
            self.assertEqual(s.discard_alpha, 1 if i == 1 else 0)
            self.assertEqual(DLL.resolve(s, False)[0].key(), s.key())
            # with alpha kept only the filter may differ (a Colour seed keeping alpha greys it to Box)
            e = DLL.resolve(s, True)[0].key()
            for n, (a, b) in zip(P.Settings._fields_, zip(e, s.key())):
                if n[0] != "filter":
                    self.assertEqual(a, b, (i, n[0]))

    def test_match_preset(self):
        ps = DLL.presets()
        m = DLL.match_preset
        raw = copy(ps[0][1])
        self.assertEqual(m(ps, raw, False), 0)
        raw.srgb = 1
        self.assertEqual(m(ps, raw, False), -1)
        raw.srgb = 0
        self.assertEqual(m(ps, raw, False), 0)
        n = copy(ps[2][1])
        n.srgb = 1  # greyed for Normal, so it still matches
        self.assertEqual(m(ps, n, False), 2)
        b = copy(ps[1][1])
        b.discard_alpha = 0
        self.assertEqual(m(ps, b, True), -1)
        b.discard_alpha = 1
        self.assertEqual(m(ps, b, True), 1)
        for d in (0, 1):
            b.discard_alpha = d
            self.assertEqual(m(ps, b, False), 1)
        twin = [ps[0], ("twin", copy(ps[0][1]))]
        self.assertEqual(m(twin, ps[0][1], False, preferred=1), 1)
        self.assertEqual(m(twin, ps[0][1], False, preferred=-1), 0)
        self.assertEqual(m(ps, ps[2][1], False, preferred=0), 2)
        self.assertEqual(m([], ps[0][1], False), -1)

    def test_preset_state_custom_sticks(self):
        # PresetState indices are psdds seed indices (M4); the UI adds 1 for Custom.
        ps = DLL.presets()
        st = P.PresetState(DLL.resolve, 5)
        self.assertEqual(st.shown(ps, ps[0][1], False), 0)
        st.pick(None)
        self.assertEqual(st.shown(ps, ps[0][1], False), -1)
        self.assertEqual(st.shown(ps, ps[2][1], False), -1)
        st.pick(3)
        self.assertEqual(st.shown(ps, ps[3][1], False), 3)
        st.pick(None)
        st.saved()
        self.assertEqual(st.shown(ps, ps[0][1], False), 0)
        self.assertEqual(P.PresetState(DLL.resolve, 5).shown(ps, ps[0][1], False), 0)

    # Task 5: the logic the export dialog's refresh() relies on (M5: pure, gi-free).
    def test_raw_kept_across_type_switch(self):
        raw = DLL.defaults(P.COLOUR)
        raw.srgb = 1
        raw.texture_type = P.NORMAL
        e, c = DLL.resolve(raw, False)
        self.assertEqual((e.srgb, c.srgb_enabled), (0, 0))
        raw.texture_type = P.COLOUR
        self.assertEqual(DLL.resolve(raw, False)[0].srgb, 1)  # DC19: Normal -> Colour restores sRGB On
        self.assertEqual(raw.srgb, 1)

    def test_raw_format_kept_across_type_switch(self):
        raw = DLL.defaults(P.NORMAL)
        raw.format = P.BC5
        raw.texture_type = P.COLOUR
        self.assertEqual(DLL.resolve(raw, False)[0].format, DLL.defaults(P.COLOUR).format)
        self.assertNotIn(P.BC5, DLL.allowed_formats(P.COLOUR))
        raw.texture_type = P.NORMAL
        self.assertEqual(DLL.resolve(raw, False)[0].format, P.BC5)  # DC33

    def test_quality_from_other_format_falls_back(self):
        raw = DLL.defaults(P.LINEAR_MASK)
        raw.format, raw.quality = P.BC4, 6  # 6 is a BC7 index, outside BC4's list
        q = DLL.resolve(raw, False)[0].quality
        self.assertLess(q, len(DLL.quality_labels(P.BC4)))
        other = copy(raw)
        other.quality = 18
        self.assertEqual(DLL.resolve(other, False)[0].quality, q)  # the one BC4 default, not a clamp
        self.assertEqual(raw.quality, 6)  # DC8

    def test_filter_follows_mipmaps(self):
        def ctl(srgb, mips, alpha, discard=0, flt=P.KAISER):
            raw = DLL.defaults(P.COLOUR)
            raw.format, raw.srgb, raw.mips, raw.discard_alpha, raw.filter = P.BC7, srgb, mips, discard, flt
            e, c = DLL.resolve(raw, alpha)
            return c.filter_enabled, e.filter
        self.assertEqual(ctl(1, 0, False)[0], 0)
        self.assertEqual(ctl(1, 1, False), (1, P.KAISER))
        self.assertEqual(ctl(1, 1, True), (0, P.BOX))  # DC12, DC31
        self.assertEqual(ctl(1, 1, True, discard=1), (1, P.KAISER))  # DC36
        d = DLL.defaults(P.COLOUR)
        self.assertEqual(d.srgb, 0)
        self.assertEqual(DLL.resolve(d, False)[1].filter_enabled, 1)  # DC39
        for srgb in (0, 1):
            self.assertEqual(ctl(srgb, d.mips, False)[0], 1)
            self.assertEqual(ctl(srgb, d.mips, True), (0, P.BOX))

    def test_seed_pick_dc40(self):
        ps = DLL.presets()
        st = P.PresetState(DLL.resolve, len(ps))
        st.pick(1)
        raw = P.apply_preset(DLL.defaults(P.COLOUR), ps, 1)
        self.assertEqual(raw.discard_alpha, 1)
        self.assertEqual(st.shown(ps, raw, False), 1)  # discard ignored in the match, seed 1 still shown
        twin = ps + [("twin", copy(ps[0][1]))]
        st = P.PresetState(DLL.resolve, len(twin))
        self.assertEqual(st.shown(twin, ps[0][1], False), 0)
        st.pick(5)
        self.assertEqual(st.shown(twin, P.apply_preset(raw, twin, 5), False), 5)

    def test_seed_pick_sets_all_seven_raw(self):
        ps = DLL.presets()
        for i, (_, s) in enumerate(ps):
            raw = P.apply_preset(DLL.defaults(P.NORMAL), ps, i)
            self.assertEqual(raw.key(), s.key())
            self.assertEqual(DLL.match_preset(ps, raw, False), i)
            self.assertEqual(DLL.match_preset(ps, raw, True), i)
        self.assertEqual(P.apply_preset(DLL.defaults(P.NORMAL), ps, 1).discard_alpha, 1)
        raw = DLL.defaults(P.NORMAL)
        raw.quality = 3
        for custom in (None, -1):  # M4: Custom is None/-1 in seed-index space; raw is left alone
            self.assertIs(P.apply_preset(raw, ps, custom), raw)
        raw2 = P.apply_preset(raw, ps, 0)
        raw2.format = P.RGBA8
        self.assertEqual(ps[0][1].format, DLL.presets()[0][1].format)  # a copy, never the seed itself

    def test_preset_state_pick_validates(self):
        st = P.PresetState(DLL.resolve, len(DLL.presets()))
        for bad in (5, -2, 99):
            with self.assertRaises(ValueError):
                st.pick(bad)
        st.pick(4)
        self.assertEqual(st.picked, 4)

    def test_discard_matters_only_with_alpha_in_psdds(self):
        # match_preset zeroes discard_alpha without alpha (DC40); this pins that psdds still needs it done by the host.
        a = DLL.presets()[1][1]
        b = copy(a)
        b.discard_alpha = 0
        self.assertNotEqual(DLL.resolve(a, False)[0].key(), DLL.resolve(b, False)[0].key())

    def test_missing_abi_symbol(self):
        with mock.patch.object(P.ctypes, "CDLL", return_value=object()):
            with self.assertRaises(P.PsddsError) as c:
                P.Psdds()
        self.assertIn("needs 2", str(c.exception))

    def test_floats_length_mismatch(self):
        with self.assertRaises(ValueError):
            DLL.alpha_used(bytearray(15), 1, 1)
        with self.assertRaises(ValueError):
            DLL.encode(bytearray(15), 1, 1, False, DLL.defaults(P.COLOUR))

    def test_encode_error_path(self):
        with self.assertRaises(P.PsddsError) as c:
            DLL.encode(bytearray(16385 * 16), 16385, 1, False, DLL.defaults(P.COLOUR))
        self.assertTrue(str(c.exception).startswith("This image is larger than"))

    def test_err_truncation(self):
        buf = ctypes.create_string_buffer(8)
        self.assertNotEqual(DLL._dll.psdds_check_size(16385, 1, buf, 8), 0)
        self.assertEqual(len(buf.value), 7)  # truncated and NUL-terminated

    def test_resolve_srgb_off_keeps_filter(self):
        raw = DLL.defaults(P.COLOUR)
        raw.filter = P.KAISER
        for srgb in (0, 1):
            raw.srgb = srgb
            e, c = DLL.resolve(raw, False)
            self.assertEqual((c.filter_enabled, e.filter), (1, P.KAISER))
            e, c = DLL.resolve(raw, True)
            self.assertEqual((c.filter_enabled, e.filter), (0, P.BOX))
        img = rgba_floats([(0.2, 0.4, 0.6, 1.0)] * 16)
        a, b = DLL.defaults(P.COLOUR), DLL.defaults(P.COLOUR)
        b.srgb = 1
        ea, eb = DLL.encode(img, 4, 4, False, a), DLL.encode(img, 4, 4, False, b)
        self.assertEqual(ea[148:], eb[148:])
        self.assertNotEqual(ea[:148], eb[:148])  # the DXGI format tag differs

    def test_resolve_rejects_discard_2(self):
        raw = DLL.defaults(P.COLOUR)
        raw.discard_alpha = 2
        with self.assertRaises(ValueError):
            DLL.resolve(raw, False)

    def test_discard_transparency_encode(self):
        px = [(0.5, 0.25, 0.75, 1.0)] * 16
        a = list(px)
        a[3] = (0.5, 0.25, 0.75, 0.0)
        s, s1 = DLL.defaults(P.COLOUR), DLL.defaults(P.COLOUR)
        s1.discard_alpha = 1
        opaque = DLL.encode(rgba_floats(px), 4, 4, False, s)
        self.assertEqual(DLL.encode(rgba_floats(a), 4, 4, True, s1), opaque)
        self.assertNotEqual(DLL.encode(rgba_floats(a), 4, 4, True, s), opaque)

    def test_resolve_bc5_greys_srgb_and_filter(self):
        raw = DLL.defaults(P.NORMAL)
        raw.format, raw.srgb = P.BC5, 1
        e, c = DLL.resolve(raw, False)
        self.assertEqual((e.srgb, e.filter, c.srgb_enabled, c.filter_enabled), (0, P.BOX, 0, 0))
        self.assertEqual(raw.srgb, 1)

    def test_resolve_greys_filter_without_mips(self):
        raw = DLL.defaults(P.COLOUR)
        raw.mips, raw.filter = 0, P.KAISER
        e, c = DLL.resolve(raw, False)
        self.assertEqual((c.filter_enabled, e.filter), (0, P.KAISER))

    def test_resolve_rejects_bad_format(self):
        raw = DLL.defaults(P.COLOUR)
        raw.format = 9
        with self.assertRaises(ValueError):
            DLL.resolve(raw, False)

    def test_formats_per_type(self):
        self.assertEqual(DLL.allowed_formats(P.COLOUR), [P.BC1, P.BC3, P.BC7, P.RGBA8])
        self.assertEqual(DLL.allowed_formats(P.NORMAL), [P.BC1, P.BC3, P.BC5, P.BC7, P.RGBA8])
        self.assertEqual(DLL.allowed_formats(P.LINEAR_MASK), [P.BC1, P.BC3, P.BC4, P.BC5, P.BC7, P.RGBA8])

    def test_disallowed_format_resolves_to_default(self):
        raw = DLL.defaults(P.COLOUR)
        raw.format = P.BC4
        self.assertEqual(DLL.resolve(raw, False)[0].format, P.BC7)
        self.assertEqual(raw.format, P.BC4)

    def test_quality_labels(self):
        self.assertEqual(len(DLL.quality_labels(P.BC7)), 7)
        self.assertEqual(len(DLL.quality_labels(P.BC1)), 19)
        self.assertEqual(len(DLL.quality_labels(P.BC4)), 3)
        self.assertEqual(DLL.quality_labels(P.RGBA8), [])

    def test_encode_decode_roundtrip(self):
        vals = [(i / 15, (15 - i) / 15, 0.5, 1.0) for i in range(16)]
        s = DLL.defaults(P.COLOUR)
        s.format = P.RGBA8
        w, h, a, px = DLL.decode(DLL.encode(rgba_floats(vals), 4, 4, False, s))
        self.assertEqual((w, h), (4, 4))
        self.assertTrue(a)  # psdds reports has_alpha for any format that stores it, RGBA8 included
        want = bytes(round(v * 255) for p in vals for v in p[:3])
        got = bytes(b for i in range(16) for b in px[i * 4:i * 4 + 3])
        self.assertEqual(got, want)

    def test_encode_clamps(self):
        a = [(1.5, -0.25, float("nan"), 1.0)] * 16
        b = [(1.0, 0.0, 0.0, 1.0)] * 16
        s = DLL.defaults(P.COLOUR)
        self.assertEqual(DLL.encode(rgba_floats(a), 4, 4, False, s), DLL.encode(rgba_floats(b), 4, 4, False, s))

    def test_check_size(self):
        with self.assertRaises(P.PsddsError) as c:
            DLL.check_size(16385, 1)
        self.assertTrue(str(c.exception).startswith("This image is larger than 16384"))
        self.assertIsNone(DLL.check_size(16384, 16384))

    def test_alpha_used(self):
        def used(alpha):
            return DLL.alpha_used(rgba_floats([(0, 0, 0, 1.0)] * 3 + [(0, 0, 0, alpha)]), 2, 2)
        self.assertFalse(used(1.0))
        self.assertFalse(used(254 / 255))
        self.assertTrue(used(253 / 255))
        self.assertFalse(used(float("nan")))

    def test_opens_with_alpha(self):
        self.assertFalse(DLL.opens_with_alpha(bytes([1, 2, 3, 254] * 4), True))
        self.assertTrue(DLL.opens_with_alpha(bytes([1, 2, 3, 253] + [0, 0, 0, 255] * 3), True))
        self.assertFalse(DLL.opens_with_alpha(bytes([1, 2, 3, 253] * 4), False))

    def test_max_file_bytes(self):
        self.assertEqual(DLL.max_file_bytes(), 1_431_655_912)

    def test_decode_garbage(self):
        with self.assertRaises(P.PsddsError) as c:
            DLL.decode(b"nope")
        self.assertTrue(str(c.exception))

    def test_openable(self):
        self.assertEqual(len(dds_header(dxgi=71)), 148)
        self.assertFalse(DLL.openable(dds_header(dxgi=71, caps2=0x200)))
        self.assertTrue(DLL.openable(dds_header(fourcc=b"DXT1")))
        self.assertTrue(DLL.openable(b"DDS "))


if __name__ == "__main__":
    unittest.main()
