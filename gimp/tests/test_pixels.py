# SPDX-License-Identifier: GPL-3.0-or-later
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(HERE)), "gimp", "tessalith-dds"))

import psdds_ctypes as P  # type ids only; no DLL load
import tdds_pixels as T


class Bands(unittest.TestCase):
    def test_cap_divides_wide_rows(self):
        self.assertEqual(T.band_rows(16384 * 16, 16384), 256)

    def test_huge_row_is_one(self):
        self.assertEqual(T.band_rows(10**9, 5), 1)

    def test_small_image_is_whole(self):
        self.assertEqual(T.band_rows(16, 3), 3)

    def test_custom_cap(self):
        self.assertEqual(T.band_rows(10, 100, cap=95), 9)


# Every GimpPrecision value of GIMP 3.0.4 (include\gimp-3.0\libgimpbase\gimpbaseenums.h).
PRECISIONS = {
    "U8_LINEAR": 100, "U8_NON_LINEAR": 150, "U8_PERCEPTUAL": 175,
    "U16_LINEAR": 200, "U16_NON_LINEAR": 250, "U16_PERCEPTUAL": 275,
    "U32_LINEAR": 300, "U32_NON_LINEAR": 350, "U32_PERCEPTUAL": 375,
    "HALF_LINEAR": 500, "HALF_NON_LINEAR": 550, "HALF_PERCEPTUAL": 575,
    "FLOAT_LINEAR": 600, "FLOAT_NON_LINEAR": 650, "FLOAT_PERCEPTUAL": 675,
    "DOUBLE_LINEAR": 700, "DOUBLE_NON_LINEAR": 750, "DOUBLE_PERCEPTUAL": 775,
}


class Precision(unittest.TestCase):
    def test_every_precision_every_type(self):
        depths = {"U8": 8, "U16": 16, "U32": 16, "HALF": 32, "FLOAT": 32, "DOUBLE": 32}
        for name, value in PRECISIONS.items():
            depth = depths[name.split("_")[0]]
            linear = name.endswith("_LINEAR") and "NON" not in name
            colour = T.LINEAR_FLOAT if depth == 32 else T.SRGB_FLOAT  # light: by depth
            data = T.LINEAR_FLOAT if linear else T.SRGB_FLOAT  # data: as stored
            for t, fmt in ((P.COLOUR, colour), (P.NORMAL, data), (P.LINEAR_MASK, data)):
                self.assertEqual(T.precision_depth(value, t), (depth, fmt), (name, t))
        self.assertEqual((T.SRGB_FLOAT, T.LINEAR_FLOAT), ("R'G'B'A float", "RGBA float"))

    def test_unknown_precision(self):
        for bad in (0, 99, 125, 400, 800):
            with self.assertRaises(ValueError):
                T.precision_depth(bad, P.COLOUR)


if __name__ == "__main__":
    unittest.main()
