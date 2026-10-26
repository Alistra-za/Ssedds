# SPDX-License-Identifier: GPL-3.0-or-later
"""Pixel transfer helpers: the row bands moved between GEGL and psdds, and how an image's precision is read."""

BAND_CAP = 64 * 1024 * 1024  # DC17: transfer-size choice per read/write, not a settings rule


def band_rows(row_bytes, height, cap=BAND_CAP):
    """Rows per band: at least 1, at most height."""
    return max(1, min(height, cap // max(1, row_bytes)))


SRGB_FLOAT, LINEAR_FLOAT = "R'G'B'A float", "RGBA float"


def precision_depth(precision):
    """(psdds depth, babl format to read) for a GimpPrecision value (gimpbaseenums.h: 100 * the component kind,
    plus 0 linear, 50 non-linear, 75 perceptual). Photoshop's meaning of depth (DF3): integer images are 8 or 16
    bits of 0..1 coded values, read sRGB-coded; half, float and double are depth 32, read linear with values
    above 1 kept. babl converts from the image's own trc."""
    kind, trc = divmod(int(precision), 100)
    if trc not in (0, 50, 75):
        raise ValueError(f"unknown image precision {precision}")
    if kind == 1:  # u8
        return 8, SRGB_FLOAT
    if kind in (2, 3):  # u16, u32
        return 16, SRGB_FLOAT
    if kind in (5, 6, 7):  # half, float, double
        return 32, LINEAR_FLOAT
    raise ValueError(f"unknown image precision {precision}")
