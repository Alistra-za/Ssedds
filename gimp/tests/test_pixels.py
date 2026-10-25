# SPDX-License-Identifier: GPL-3.0-or-later
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(HERE)), "gimp", "tessalith-dds"))

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


if __name__ == "__main__":
    unittest.main()
