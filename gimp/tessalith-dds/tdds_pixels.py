# SPDX-License-Identifier: GPL-3.0-or-later
"""Pixel transfer helpers: the row bands moved between GEGL and psdds, and how an image's precision is read."""

BAND_CAP = 64 * 1024 * 1024  # DC17: transfer-size choice per read/write, not a settings rule


def band_rows(row_bytes, height, cap=BAND_CAP):
    """Rows per band: at least 1, at most height."""
    return max(1, min(height, cap // max(1, row_bytes)))


SRGB_FLOAT, LINEAR_FLOAT = "R'G'B'A float", "RGBA float"


def precision_depth(precision, texture_type):
    """(psdds depth, babl format to read) for a GimpPrecision value (gimpbaseenums.h: 100 * the component kind,
    plus 0 linear, 50 non-linear, 75 perceptual) and the effective texture type (psdds ids, 0 = Colour).

    Depth follows Photoshop's meaning (DF3): u8 is 8; u16 and u32 are 16; half, float and double are 32.
    Colour is light: 8 and 16 bits read sRGB-coded, 32 bits linear with values above 1 kept (babl converts).
    Normal and Linear mask are data: read in the image's own encoding, never converted (controller 2026-10-26).
    """
    kind, trc = divmod(int(precision), 100)
    depth = {1: 8, 2: 16, 3: 16, 5: 32, 6: 32, 7: 32}.get(kind)
    if depth is None or trc not in (0, 50, 75):
        raise ValueError(f"unknown image precision {precision}")
    if texture_type == 0:
        return depth, LINEAR_FLOAT if depth == 32 else SRGB_FLOAT
    return depth, LINEAR_FLOAT if trc == 0 else SRGB_FLOAT
